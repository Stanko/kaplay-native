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
- Bridge: WebGL 1, Canvas 2D, images, fonts, timers, events, `localStorage`, `fetch`
- Assets: directory or `.pak` archives
- Steam: dynamic `steam_api` loading, user, achievements, stats, cloud storage
- CLI: `run`, `dev`, `build`, `test`, `doctor`

Not implemented yet: audio playback, network requests (`--allow-network` only
sets a flag), gamepads, `Intl`, and the Steam Input action manifest.

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
node cli/src/index.mjs dev ./my-game
node cli/src/index.mjs build ./my-game --out dist/my-game.pak
node cli/src/index.mjs test ./my-game --update
node cli/src/index.mjs doctor
```

The host binary is found with `--host`, `KAPLAY_NATIVE_HOST`, next to the CLI,
or on `PATH`.

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

## License

MIT
