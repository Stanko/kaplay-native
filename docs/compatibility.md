# Compatibility

KAPLAY Native implements the browser surface KAPLAY uses, not the whole web
platform. This page lists what works today and what happens when something is
missing.

## JavaScript

- QuickJS via `rquickjs` with ES2020 including classes, generators, typed
  arrays, `WeakMap`, and modules-in-script support.
- The game entry is evaluated as an ES module, matching `<script
  type="module">` in a browser: top-level declarations stay module-scoped and
  cannot clobber globals exported by KAPLAY or other libraries. `--preload`
  scripts stay classic scripts, like `<script src>` on a page.
- `Promise`, `async`/`await` with an explicit job queue pump.
- `TextEncoder` and `TextDecoder` (UTF-8) are shimmed because QuickJS does not
  provide them.
- `Promise.withResolvers` is polyfilled.
- `URL` and `URLSearchParams` are implemented, including base resolution and
  form encoding, because SDKs parse their endpoint URLs at import time.
- `crypto.getRandomValues` and `crypto.randomUUID` are backed by `Math.random`,
  which is fine for gameplay identifiers but not for secrets.
- `matchMedia` returns a non-matching media query list, so pointer-capability
  checks report a desktop.
- `Intl` is not available. KAPLAY falls back to a code-point splitter in
  `runes.ts`, so text still works, but locale-aware segmentation does not.

## DOM and events

- `document`, `document.head`, `document.body`, `document.documentElement`
- `createElement`, `createTextNode`, `getElementById`, `getElementsByTagName`,
  `getElementsByClassName`, `querySelector`, `appendChild`, `prepend`,
  `removeChild`, `insertBefore`, `setAttribute` for `div`, `input`, `textarea`,
  and `form` elements
- `Event`, `MouseEvent`, `KeyboardEvent`, `WheelEvent`, and `CustomEvent`
- canvas creation, `appendChild`, `getBoundingClientRect`, style subset
  (`width`, `height`, `cssText`, `cursor`)
- `document.activeElement`, `focus()`, `blur()`
- `addEventListener`/`removeEventListener` on canvas, document, and window
- `dispatchEvent`
- `document.visibilityState` and `visibilitychange`
- `ResizeObserver`
- `MutationObserver` (records are not observed; the callback API is a stub used
  by bundler polyfills)
- `exitPointerLock`, `exitFullscreen`
- `requestAnimationFrame`, `setTimeout`, `setInterval` and cancellations

Pointer events carry `clientX/clientY`, `offsetX/offsetY` scaled through the
drawn canvas rectangle, `movementX/movementY`, and a `buttons` bitmask.
Keyboard events carry `key`, `code`, `repeat`, modifier flags, and
`getModifierState`.

Setting `cursor` on a `style` (`style.cursor`, or `cursor: ...` inside
`cssText`) changes the real OS cursor. `none` hides it, which is what
`fakeMouse` games rely on, and the standard keywords map to platform cursor
icons.

Window resizes update `innerWidth`/`innerHeight`, the canvas box reported to
`ResizeObserver`, and dispatch a `resize` event on `window`, so KAPLAY's
`onResize` and any `ResizeObserver`-based layout run exactly as they do in a
browser.

Wheel events carry `deltaX`, `deltaY`, and `deltaMode`.

## Canvas 2D

`clearRect`, `fillRect` and `strokeRect` (no-ops used by KAPLAY's background
helpers), `fillText`, `strokeText`, `measureText` (width plus
`actualBoundingBoxAscent`/`Descent`), `drawImage` in 3, 5, and 9 argument
forms, `getImageData`, `putImageData`, style setters, and `toDataURL`/`toBlob`
backed by the native surfaces. Path helpers exist as no-ops so accidental use
does not throw.

## WebGL 1

The full surface KAPLAY uses: textures, buffers, shaders and programs,
uniforms, vertex attributes, framebuffer and renderbuffer objects, state
toggles, `readPixels`, and `getExtension("WEBGL_lose_context")`.

The context is desktop OpenGL. Shaders are translated from GLSL ES 1.00 for
both compatibility profiles (`#version 120`) and core profiles (`#version 150
core` with `in`/`out` and a declared fragment output); the profile is detected
from the context, and core profiles get a default vertex array object because
one is required there. macOS always yields a core profile through glutin, so
that path is exercised by CI on every run.

`getExtension` returns `null` for every other extension, `getError` always
returns `NO_ERROR`, and `isContextLost` returns `false`.

## Assets and resources

- `fetch` for data URLs, blob URLs, bundled files, and HTTP(S) URLs.
- HTTP(S) requests require `--allow-network`; without it they reject with a
  `TypeError` that names the flag.
- `--url` sets the page URL (`window.location`). Relative requests first check
  the asset root and then resolve against that URL, so bundled sprites keep
  loading from disk while API calls go to the game server.
- `Headers`, `Request`-style `fetch(url, init)` options (`method`, `headers`,
  `body` as string/`URLSearchParams`/`Blob`/`ArrayBuffer`/typed array), response
  `json()`, `text()`, `arrayBuffer()`, `blob()`, `ok`, `status`, `statusText`,
  and `AbortController` cancellation are supported. Response bodies are read on
  the network thread and copied into the JS array buffer on the frame that
  delivers the response.
- `WebSocket` implements the browser surface: `readyState` constants,
  `binaryType` (`arraybuffer` or `blob`), `send` for strings and binary data,
  `close(code, reason)`, and `onopen`/`onmessage`/`onerror`/`onclose` plus
  `addEventListener`. Plain `ws://` and TLS `wss://` connections are both
  supported.
- `Image` with data URLs and file paths (the decode path also accepts any
  image format supported by the `image` crate).
- `FontFace` from an `ArrayBuffer`/typed array or from `url(...)`.
- `localStorage` is persisted as JSON in the platform data directory
  (`--save-dir` overrides it) and written atomically.
- `AudioContext`, `AudioBuffer`, `AudioBufferSourceNode`, `GainNode`,
  `StereoPannerNode`, `MediaElementAudioSourceNode`, and `AudioParam` cover the
  Web Audio surface KAPLAY uses. `start(when, offset, duration)`, `stop`,
  `loop`, `playbackRate`, `detune`, `pan`, `gain` (including
  `setValueAtTime`/`linearRampToValueAtTime`/`exponentialRampToValueAtTime`),
  `connect`/`disconnect`, and `onended` all work.
- `HTMLAudioElement` (`new Audio(url)`) plays music through the same mixer:
  `play`, `pause`, `currentTime`, `duration`, `volume`, `playbackRate`, `loop`,
  and `onended`.
- Decoding supports WAV, MP3, FLAC, Ogg Vorbis, and Opus, from bundled files,
  data URLs, and HTTP(S) URLs.
- `AudioWorklet`, `AnalyserNode`, `BiquadFilterNode`, `OscillatorNode`, and
  `AudioContext.createMediaStreamSource` are not implemented. Without an audio
  device the engine runs silently and playback still advances in real time.
- `navigator.getGamepads()` returns an empty list.

## Platforms

The GL path targets desktop OpenGL everywhere: compatibility contexts on
Windows and Linux, and the core profile on macOS. On Linux, X11 and Wayland are
supported through winit and glutin; `--x11` forces the X11 backend (XWayland on
Wayland sessions) and `--no-vsync` uncaps the frame rate, which some Wayland
compositors otherwise ignore. The CI matrix builds, tests, and renders on
Linux, macOS, and Windows.

## Verification story

`cargo test` covers the native helpers, `node --test shim/test/*.test.js`
covers the browser semantics against a stub bridge, and the runtime can render
a fixed number of frames and capture a screenshot. There is no browser or
Chromium dependency anywhere in the tree by design: Steam overlay support
requires a host that owns `SwapBuffers`, which webview shells do not allow.
