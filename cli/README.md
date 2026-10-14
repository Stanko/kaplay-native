# kaplay-native

Run and package [KAPLAY](https://kaplayjs.com) games as native desktop
applications on Linux, Windows, and macOS.

```sh
npm install --global kaplay-native

kaplay-native run ./my-game
kaplay-native pack ./my-game --out dist/my-game --js-engine v8
kaplay-native doctor
```

`run` executes a game with the native host. `pack` writes a single-file
executable for the current platform with the game assets and defaults embedded,
so players need nothing else. The CLI downloads the matching native host from
GitHub releases on first use and caches it.

The full documentation, including the packed format and a copy-paste GitHub
Actions workflow for shipping to all platforms, lives in
[docs/distribution.md](https://github.com/beemdvp/kaplay-native/blob/main/docs/distribution.md).
