# Steam

Steam support has two sides: the runtime must be hookable by the overlay, and
games need Steamworks features.

## Overlay

The Steam overlay hooks the swap chain of the process. Chromium shells,
webviews, and Electron cannot be hooked reliably, which is why this runtime
owns its window and GL context directly and calls `SwapBuffers` itself. When
`steam_api` is loaded and `SteamAPI_Init` succeeds, the overlay attaches to the
window automatically; there is no code path in KAPLAY Native that draws the
overlay itself.

Two ordering rules come from the Steamworks documentation:

1. `SteamAPI_RestartAppIfNecessary` runs before any window or GL object exists.
   The host calls it with `--appid` and exits when Steam asks for a relaunch.
2. `SteamAPI_Init` runs before the GL context is created so the overlay can
   hook the swap chain from the start.

`SteamAPI_RunCallbacks` runs once per frame after the game's frame work.

## Library loading

Steamworks redistributables are never bundled. At startup the host searches for
`steam_api64.dll` / `steam_api.dll` (Windows), `libsteam_api.so` (Linux), or
`libsteam_api.dylib` (macOS) in this order:

1. the path in `KAPLAY_NATIVE_STEAM_LIB`
2. the directory containing the executable
3. the current working directory

Each flat API symbol is resolved individually. A missing library or symbol
disables Steam integration, logs at debug level, and the game keeps running.
`--no-steam` skips loading entirely, which is what CI uses.

Interfaces whose symbol names carry a version suffix are requested by the
suffix the current SDK exports:

- `SteamAPI_SteamUser_v023`
- `SteamAPI_SteamUserStats_v012`
- `SteamAPI_SteamRemoteStorage_v016`

If a future SDK bumps those versions, loading fails soft and the rest of the
runtime is unaffected; update the suffix in `crates/steam/src/lib.rs`.

## Game API

Games reach Steam through the `kaplayNative.steam` namespace:

```js
const steam = kaplayNative.steam;
steam.available();                        // boolean
steam.playerName();                       // string | null
steam.steamId();                          // string (decimal, 64-bit safe)
steam.unlockAchievement("FIRST_WIN");     // boolean
steam.isAchievementUnlocked("FIRST_WIN"); // boolean
steam.storeStats();                       // boolean
steam.setStatInt("wins", 3);              // boolean
steam.getStatInt("wins");                 // number | null
steam.setStatFloat("play_time", 12.5);    // boolean
steam.getStatFloat("play_time");          // number | null
steam.writeCloud("save.json", "{}");      // boolean
steam.readCloud("save.json");             // string | null
steam.cloudExists("save.json");           // boolean
steam.deleteCloud("save.json");           // boolean
steam.runCallbacks();                     // returns undefined
```

Every call returns a safe fallback when Steam is unavailable, so games can
call the API unconditionally. Achievement unlocks and stat writes commit
`StoreStats` immediately. Cloud files are written through the remote storage
API and are subject to Steam's per-file size limits.

The host calls `runCallbacks` every frame already; the JavaScript method exists
for games that load Steam state from their own event loops or tests.

## Packaging

`kaplay-native build` packs assets, not Steam binaries. For a Steam depot:

1. Place `steam_api64.dll` / `libsteam_api.so` next to the executable.
2. Ship `steam_appid.txt` only in development builds. Never ship it in a depot
   release: Steam sets the app id from the launch context.
3. Pass the app id in production with `--appid <id>` (or the CLI equivalent)
   so `RestartAppIfNecessary` works.

## Manual validation checklist

Overlay behavior cannot be verified without a Steam client, so it stays a
manual test. Run it on each platform before a release:

- [ ] Launch from the Steam library. The overlay appears on Shift+Tab.
- [ ] Launch from outside Steam. `RestartAppIfNecessary` relaunches through
      Steam (or exits cleanly when no Steam client is running).
- [ ] The overlay draws over the game and the game keeps rendering behind it.
- [ ] Screenshots (F12) and the in-overlay web browser work.
- [ ] Achievements unlock while the overlay is open and appear in the client.
- [ ] Cloud writes from one machine appear on another.
- [ ] Alt-tab, window resize, and fullscreen toggles do not detach the overlay.
- [ ] The game still runs with `--no-steam` and with the library removed.
