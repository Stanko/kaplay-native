kaplay({ width: 640, height: 480, background: [14, 18, 26] });

onLoad(() => {
    const steam = kaplayNative.steam;
    const cloudFile = "kaplay-native-demo.json";

    add([rect(640, 480), color(14, 18, 26)]);
    add([text("KAPLAY native + Steamworks", { size: 26 }), pos(24, 28), color(120, 220, 255)]);

    const status = add([
        text("", { size: 18, lineSpacing: 6 }),
        pos(24, 84),
        color(235, 235, 235),
    ]);

    const dot = add([circle(16), pos(560, 110), color(240, 80, 80), opacity(0.9)]);
    add([text("still rendering", { size: 14 }), pos(516, 138), color(120, 130, 145)]);

    add([text("Shift+Tab opens the Steam overlay", { size: 22 }), pos(24, 296), color(255, 220, 0)]);
    add([text("Space  write the cloud file", { size: 17 }), pos(24, 336), color(160, 170, 185)]);
    add([text("A      unlock ACH_WIN_ONE_GAME", { size: 17 }), pos(24, 362), color(160, 170, 185)]);
    add([text("S      add a NumWins stat and store", { size: 17 }), pos(24, 388), color(160, 170, 185)]);
    add([text("D      delete the cloud file", { size: 17 }), pos(24, 414), color(160, 170, 185)]);

    const refresh = (action) => {
        const lines = [
            `available: ${steam.available()}`,
            `player: ${steam.playerName() ?? "-"}`,
            `steam id: ${steam.steamId() || "-"}`,
            `cloud file: ${steam.cloudExists(cloudFile) ? "present" : "absent"}`,
            `ACH_WIN_ONE_GAME: ${steam.isAchievementUnlocked("ACH_WIN_ONE_GAME")}`,
            `NumWins: ${steam.getStatInt("NumWins") ?? "-"}`,
        ];
        if (action) lines.push(`last: ${action}`);
        status.text = lines.join("\n");
    };
    refresh();

    let t = 0;
    onUpdate(() => {
        t += dt();
        dot.pos.x = 560 + Math.sin(t * 1.4) * 26;
    });

    onKeyPress("space", () => {
        const ok = steam.writeCloud(cloudFile, `written ${Date.now()}`);
        refresh(`write ${ok}, read ${steam.readCloud(cloudFile) ?? "none"}`);
    });

    onKeyPress("a", () => {
        const ok = steam.unlockAchievement("ACH_WIN_ONE_GAME");
        refresh(`unlock ${ok}, store ${steam.storeStats()}`);
    });

    onKeyPress("s", () => {
        const wins = (steam.getStatInt("NumWins") ?? 0) + 1;
        const ok = steam.setStatInt("NumWins", wins) && steam.storeStats();
        refresh(`NumWins ${wins}, store ${ok}`);
    });

    onKeyPress("d", () => {
        const ok = steam.deleteCloud(cloudFile);
        refresh(`delete ${ok}`);
    });

    console.log("steam demo ready:", steam.available(), steam.playerName());
});
