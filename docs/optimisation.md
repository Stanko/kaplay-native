# Optimisation notes

Measured on Linux x64 with `cargo build --release -p kaplay-native-host`
(`lto = "thin"`, `codegen-units = 1`, `strip = true`) and
`cargo bloat --release -p kaplay-native-host --crates`.

## Bundle size

The release host used to be built with `--features v8`, which compiles V8 and
QuickJS into one binary. V8 and its ICU data add ~48 MB to the stripped
binary, so every QuickJS game paid for it. Release now ships an engine-specific
host per platform and the CLI downloads the matching flavor (`-v8` for
`--js-engine v8`).

| Host | Before | After |
| --- | --- | --- |
| QuickJS (default) | 13.8 MB | 11.8 MB |
| V8 | 60.1 MB | 58.1 MB |
| QuickJS + V8 (old single artifact) | 61.7 MB | n/a |

What the 11.8 MB QuickJS binary spends its text section on, according to
`cargo bloat`:

| Component | Text |
| --- | --- |
| `std` + unknown/glue | ~2.1 MB |
| `winit` + `wayland`/`x11` | ~800 KB |
| `rustls` + `ring` + `ureq` + `tungstenite` | ~950 KB |
| `tiny_skia` + `fontdb`/`ttf-parser` | ~600 KB |
| `symphonia` + `opus_pure` (incl. `rustfft` ~700 KB) | ~1.3 MB |
| QuickJS (`rquickjs_core` + sys) | ~330 KB |
| `clap_builder` (CLI parser) | ~230 KB |
| image decoding (`image` + `zune-jpeg` + `png`) | ~250 KB |
| `kaplay-native-*` crates | ~380 KB |

Cargo changes on this branch:

- `strip = true` in the release profile. The unstripped binary carries ~2.7 MB
  of symbol/debug sections that never ship.
- `zip` only enables `deflate-flate2`. The pak format is written by
  `cli/src/zip.mjs` and only ever uses store (method 0) and deflate (method 8),
  so bzip2, zstd, xz/lzma, ppmd, deflate64, aes/sha1/hmac and zopfli were dead
  weight.
- `rquickjs` no longer enables `full` (chrono, dyn-load, module loader,
  either, indexmap, phf) and drops the derive macros; the bridge only needs
  `std` and `array-buffer`.
- `env_logger` no longer pulls `regex`/`regex-automata`/`aho-corasick`
  (~600 KB) for log filtering.
- `symphonia` drops `all-meta`, `adpcm`, `isomp4` and `aac`. The documented
  formats (WAV, MP3, FLAC, Ogg Vorbis, Opus) are unchanged.

### Inside V8

The `v8` crate exposes only `use_custom_libcxx` (default on, statically links
libc++), `simdutf`, `v8_enable_pointer_compression`, `v8_enable_sandbox` and
`v8_enable_v8_checks`. `simdutf` and `v8_enable_v8_checks` only add code; the
compression/sandbox flags are memory and security knobs that are off in this
build. None of them shrink the JS feature set.

The large optional V8 components are GN arguments baked into rusty_v8's
prebuilt archive, which is what CI downloads (`V8_FROM_SOURCE` is not set):

| Component | Archive symbols | Notes |
| --- | --- | --- |
| ICU / `Intl` | ~2.5 MB | locale data and 24 k ICU symbols |
| WebAssembly | ~2.1 MB | decoder, Liftoff/TurboFan tiers, JS API |
| Maglev | ~5.7 MB | mid-tier JIT; TurboFan remains |
| Inspector / debug | ~0.7 MB | CDP bindings, unused by the host |

Probing both engines with `typeof WebAssembly` / `typeof Intl` shows QuickJS has
neither, while the prebuilt V8 has both. Dropping ICU and WebAssembly from V8
would therefore *improve* cross-engine parity, at roughly 5–8 MB per V8
binary, but it requires a from-source V8 build (`V8_FROM_SOURCE=1` plus
`GN_ARGS`), which is slow on all
platforms and painful on Windows. The practical route is a separate workflow
that builds and publishes slim `librusty_v8` archives per platform once per V8
version, then points the host build at them with `RUSTY_V8_ARCHIVE` or
`RUSTY_V8_MIRROR`. Disabling Maglev would save more but gives up the JIT tier
that justifies using V8 at all.

Remaining bundle-size candidates:

1. **Codec presets.** `rustfft` (700 KB) is a hard dependency of
   `symphonia-core`; a game that only uses WAV/Opus could drop all of
   symphonia with a `kaplay-native-audio` feature set. Same idea for the
   network stack (~950 KB) and `zune-jpeg`/`png` width.
2. **`clap`.** ~230 KB for ~25 flags. A hand-rolled parser or `lexopt`
   (~30 KB) would pay for itself on every shipped game.
3. **Shim minification.** `shim/src/shim.js` is 121 KB of source evaluated at
   startup. A minified ~45 KB variant would cut both bytes and parse time;
   V8 could also use a startup snapshot.
4. **`opt-level = "s"` for dependencies** (`[profile.release.package."*"]`)
   with the hot crates kept at `3`; `lto = "fat"` is worth re-measuring.

## Threading

Today:

| Subsystem | Thread | Communication |
| --- | --- | --- |
| JavaScript engine, GL, winit, Steam | main | direct calls |
| Audio output (ALSA/CoreAudio/WASAPI) | device callback thread | `Mutex<Mixer>`, `Mutex<Vec<Event>>` |
| Audio decode | pool of `ceil(cores/2)` clamped to 1–4 | mpsc jobs, mpsc events |
| HTTP fetch | 4 workers | mpsc jobs/events, one shared `ureq::Agent` |
| WebSocket | one thread per socket | mpsc out, mpsc events |
| Canvas 2D, fonts | main | direct calls |

Constraints that keep GL and the JS engine on the main thread: OpenGL contexts
are thread-affine, and neither QuickJS nor the V8 isolate is `Send`. Everything
else is a candidate for a dedicated thread.

Changes on this branch:

- Audio decoding used `std::thread::spawn` per request, so a large game could
  create hundreds of short-lived threads during load. It now runs on a bounded
  pool.
- The silent backend no longer runs a render thread that woke up every ~10 ms.
  `Engine::advance_silent` catches the mixer clock up to wall time from the
  frame loop, which removes a thread and a wakeup source from `--no-audio`
  runs and from devices that fail to open.
- HTTP fetch workers used to build a new `ureq::Agent` for every request, which
  discards the connection pool and forces a fresh TLS handshake each time.
  Workers now share one agent, which matters for the polling traffic games
  generate.

Target shape, if more isolation is wanted: every subsystem owns a command
sender and an event receiver, and the main thread never blocks on a subsystem
lock.

- **Mixer.** The audio callback currently locks the `Mixer` mutex that the main
  thread also holds while updating voices. Replacing it with a lock-free
  command ring (main -> mixer) and an event ring (mixer -> main) removes the
  possibility that the frame thread stalls the audio device.
- **Asset I/O.** `AssetSource::read` runs on the JS thread, so a big PNG blocks
  the frame. Image decode (`image::load_from_memory`) is CPU-heavy and could
  use the same job/event pattern as audio decode, with the shim awaiting an
  event.
- **Pak loading.** `from_pak_bytes` decompresses every entry at startup. For
  large games, keep the archive and decompress on demand into a small cache.
- **Canvas 2D.** Synchronous text/rect drawing has to stay on the main thread
  because the JS calls return immediately, but `to_png`, `getImageData` and
  image decode can move to workers.

## Hot paths

- **Mixer pan law.** `pan_gains` called `cos`/`sin` once per voice per sample,
  i.e. ~1M trig calls per second with a dozen voices. The gain pair is now
  computed when a voice is created or its pan changes.
- **Asset reads.** Every `fs.read`/`image.load` cloned the whole file out of the
  archive. Entries are now `Arc<[u8]>`, so a read is a refcount bump. `fs.size`
  also stopped reading the whole file just to report its length, and directory
  mode canonicalizes the asset root once instead of twice per lookup.
- **QuickJS texture uploads.** `texImage2D`/`texSubImage2D` copied the typed
  array into a `Vec` before upload; they now borrow the array for the duration
  of the GL call.
- **Frame statistics.** `record_frame` grew a `Vec` by one entry per frame
  forever, even when `--stats` was off. It is now a 10 000-sample ring, which
  caps memory at 40 KB.
- **Shim frame loop.** `__kapNative_onFrame` copied the animation-frame queue
  with `splice` (a fresh array every frame) and walked every timer. It now
  swaps buffers and skips the timer map when it is empty.

Remaining hot-path candidates:

- `GlState::present` issues ~10 `glGet*` calls per frame to save and restore
  state. All GL mutations funnel through `GlState`, so the values could be
  tracked in Rust and the queries dropped.
- `Mixer::render` reads interleaved samples through a bounds-checked accessor
  four times per output frame and computes the pan gains per sample (fixed),
  leaving it a good SIMD candidate.
- `AssetSource` keeps pak entries uncompressed in memory next to the archive.
- `canvas2d` premultiplies a copy of the source on every `drawImage`; most
  sprites are fully opaque, so an opaque fast path avoids a full pass.
- Input events serialize JSON for every key/mouse event before entering the
  shim.
- The shim re-walks the Web Audio node graph for every live voice on every
  frame (`graphParams` allocates a stack and a `Set` per voice).

## How to measure

```sh
cargo build --release -p kaplay-native-host
cargo bloat --release -p kaplay-native-host --crates -n 40
stat -c '%s' target/release/kaplay-native-host
```

Runtime frame cost, without the vertical-sync wait:

```sh
target/release/kaplay-native-host --pak game.pak --no-vsync --stats --frames 600
```

Cross-engine parity and packed-game smoke tests live in `ci.yml` and
`release.yml`.
