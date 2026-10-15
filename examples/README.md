# Examples

- `hello` - a KAPLAY game: rectangles, a circle, and text. Requires a KAPLAY
  bundle passed with `--preload`.
- `smoke` - raw WebGL without KAPLAY: clears the screen, uploads a textured
  quad, and captures a screenshot. Used as the CI smoke test.
- `modules` - a multi-chunk ES module build: the entry imports a sibling chunk
  and loads another chunk with `import()`. Used as the CI module-resolution
  test.
- `steam` - KAPLAY demo for Steamworks: player/stat/achievement and cloud
  status, with Space/A/S/D to write the cloud file, unlock
  `ACH_WIN_ONE_GAME`, add a `NumWins` stat, and delete the cloud file. Requires
  a KAPLAY bundle via `--preload`; easiest to launch with
  `scripts/steam-overlay-test.sh --demo` (app id 480).

```sh
cargo run -p kaplay-native-host -- --script examples/smoke/main.js --dir examples/smoke --frames 5 --screenshot smoke.png
cargo run -p kaplay-native-host -- --dir examples/modules --frames 1
cargo run -p kaplay-native-host -- --script examples/hello/main.js --preload path/to/kaplay.js --dir examples/hello
```
