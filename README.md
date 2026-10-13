# KAPLAY Native

Run [KAPLAY](https://kaplayjs.com) games as native desktop applications on
Windows, Linux, and macOS, with a working Steam overlay and Steamworks
integration.

KAPLAY Native is not a browser shell. It is a small Rust host that owns the
window, the GL context, and the frame loop, implements the browser APIs KAPLAY
uses, and evaluates the unmodified KAPLAY bundle with a built-in JavaScript
engine. Nothing in KAPLAY is patched: the runtime installs the globals, then
loads the game exactly as a browser would.

```
game script ──► QuickJS ──► KAPLAY bundle (unchanged)
                   │
                   ▼
          JS shim (DOM, canvas, WebGL, fetch, storage)
                   │
                   ▼
        Rust host (winit + glutin + tiny-skia + Steam)
                   │
                   ▼
        window / OpenGL / disk / Steam overlay
```

## Status

Early. The runtime runs real KAPLAY games today: assets load, sprites and text
render through the GL pipeline, input, storage, and screenshots work. Steam
support is implemented behind runtime library loading but has not yet been
validated against a shipping Steam depot - see [docs/steam.md](docs/steam.md)
for the manual validation checklist.

- Runtime: window, GL 2.1 compatibility context, frame loop, aspect-fit present
- Bridge: WebGL 1, Canvas 2D, images, fonts, timers, events, `localStorage`
- Network: `fetch` (HTTP/HTTPS) and `WebSocket` (ws/wss) on background threads
- Audio: Web Audio nodes over a native software mixer, plus `HTMLAudioElement`
  music, decoding WAV, MP3, FLAC, Ogg Vorbis, and Opus
- Assets: directory or `.pak` archives
- Steam: dynamic `steam_api` loading, user, achievements, stats, cloud storage
- CLI: `run`, `dev`, `build`, `test`, `doctor`

Not implemented yet: gamepads, `Intl`, and the Steam Input action manifest.

## Quick start

Build the host and run the examples with the KAPLAY bundle from a checkout:

```sh
cargo build --release
node cli/src/index.mjs run examples/hello --preload path/to/kaplay.js
```

Or run the host directly:

```sh
target/release/kaplay-native-host \
  --script examples/hello/main.js \
  --preload path/to/kaplay.js \
  --dir examples/hello \
  --window 960x540
```

The headless smoke test uses raw WebGL, so it needs no preload:

```sh
target/release/kaplay-native-host --script examples/smoke/main.js --dir examples/smoke --frames 5 --screenshot smoke.png
```

## CLI

The CLI is a small Node script that locates the host binary, bundles the game
with esbuild when available, and packs assets into a `.pak`:

```sh
node cli/src/index.mjs run ./my-game
node cli/src/index.mjs run --web-dist ./my-game/dist
node cli/src/index.mjs dev ./my-game
node cli/src/index.mjs build ./my-game --out dist/my-game.pak
node cli/src/index.mjs test ./my-game --update
node cli/src/index.mjs doctor
```

The host binary is found with `--host`, `KAPLAY_NATIVE_HOST`, next to the CLI,
or on `PATH`.

## Running a web build

For games built with Vite or Rollup, run the production build directly. The
host reads `index.html`, runs the module script it references, and resolves
assets from the same directory, so the project's own Vite plugins (sprite
atlases, PWA registration, `virtual:` modules, aliases) apply exactly as they
do on the web:

```sh
node cli/src/index.mjs run --web-dist path/to/game/dist
```

or with the host binary directly:

```sh
kaplay-native-host --dir path/to/game/dist --window 1280x720
```

The entry script is evaluated as an ES module, like
`<script type="module">` in a browser. Top-level declarations stay
module-scoped, so they cannot collide with globals that KAPLAY or another
library exports onto `window`.

## Bundling from source

Games without Vite plugin dependencies can be inlined into one script with the
esbuild helper:

```sh
node integrations/vite-game/build.mjs --root path/to/game/client --out build/game/game.js
kaplay-native-host --script build/game/game.js --dir path/to/game/client/public --window 1280x720
```

The bundle inlines the game and KAPLAY into one script, so no module loader is
involved. Assets keep loading from `--dir` (or a `.pak`), which means no URL
rewriting is needed. Vite virtual modules are stubbed (`virtual:pwa-register`,
atlas manifests, devtools); use `--virtual name=file` and `--define K=value` to
override them per project.

This path does not run Vite plugin transforms, so games that depend on them
(for example a sprite-atlas plugin that rewrites `loadSprite` calls) should
build with their own Vite config and use the `--web-dist` workflow above.

Games that talk to a server over HTTP or WebSockets need two flags: `--allow-network`
enables the transports and `--url` sets the page URL games see as
`window.location`, which is how relative requests and SDK endpoints resolve:

```sh
kaplay-native-host \
  --dir path/to/game/dist \
  --allow-network \
  --url "http://127.0.0.1:5173/?debug&instantLogin"
```

Relative requests check bundled assets first and only then fall back to the
page origin, so sprites and fonts keep loading from `--dir` while API calls go
to the server. With the example above, a Colyseus client pointed at `/colyseus`
connects to `ws://127.0.0.1:5173/colyseus`, exactly like in a browser served by
the Vite dev server.

## Audio

Sound plays through a native software mixer. The shim implements the Web Audio
surface KAPLAY uses (`AudioContext`, `AudioBuffer`, `AudioBufferSourceNode`,
`GainNode`, `StereoPannerNode`, `MediaElementAudioSourceNode`, `AudioParam`)
and `HTMLAudioElement` for `playMusic`, all routed through the same engine:

- Decoding: WAV, MP3, FLAC, Ogg Vorbis, and Opus, done on background threads.
- Output: ALSA on Linux, WASAPI on Windows, CoreAudio on macOS. If no device is
  available the game keeps running silently and `currentTime` still advances, so
  sounds still start and end.
- Pass `--no-audio` to skip opening a device entirely (used in CI).

## Steam

Steam support never bundles or links the Steamworks redistributables. The host
loads `steam_api64.dll` / `libsteam_api.so` / `libsteam_api.dylib` at runtime
from the game directory (or `KAPLAY_NATIVE_STEAM_LIB`) and resolves only the
flat API symbols it needs. If the library or the Steam client is missing, games
run normally and `kaplayNative.steam.available()` reports `false`.

Games call the platform API from JavaScript:

```js
if (kaplayNative.steam.available()) {
    kaplayNative.steam.unlockAchievement("FIRST_WIN");
    kaplayNative.steam.writeCloud("save.json", JSON.stringify(save));
}
```

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
node --test shim/test/*.test.js
```

Set `KAPLAY_NATIVE_WATCHDOG=1` to make the runtime interrupt JavaScript that
stalls a phase for more than four seconds. The interrupt is reported with the
phase name and, because job exceptions are logged by the raw QuickJS pump, the
JavaScript stack that was running.

## License

MIT
