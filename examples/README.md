# Examples

- `hello` - a KAPLAY game: rectangles, a circle, and text. Requires a KAPLAY
  bundle passed with `--preload`.
- `smoke` - raw WebGL without KAPLAY: clears the screen, uploads a textured
  quad, and captures a screenshot. Used as the CI smoke test.

```sh
cargo run -p kaplay-native-host -- --script examples/smoke/main.js --dir examples/smoke --frames 5 --screenshot smoke.png
cargo run -p kaplay-native-host -- --script examples/hello/main.js --preload path/to/kaplay.js --dir examples/hello
```
