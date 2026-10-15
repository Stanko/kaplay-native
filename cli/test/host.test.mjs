import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { findLocalHost } from "../src/host.mjs";

function hosts(t, engines) {
    const dir = mkdtempSync(join(tmpdir(), "kaplay-native-host-test-"));
    t.after(() => rmSync(dir, { recursive: true, force: true }));
    const release = join(dir, "release");
    const debug = join(dir, "debug");
    writeFileSync(release, "");
    writeFileSync(debug, "");
    const listEngines = (host) => engines[host === release ? 0 : 1];
    return { release, debug, listEngines };
}

test("V8 selects debug when release is QuickJS-only", (t) => {
    const { release, debug, listEngines } = hosts(t, [["quickjs"], ["quickjs", "v8"]]);
    assert.equal(findLocalHost([release, debug], "v8", listEngines), debug);
});

test("compatible release remains preferred", (t) => {
    const { release, debug, listEngines } = hosts(t, [["quickjs", "v8"], ["v8"]]);
    assert.equal(findLocalHost([release, debug], "v8", listEngines), release);
});

test("default selection prefers release without probing engines", (t) => {
    const { release, debug } = hosts(t, []);
    assert.equal(findLocalHost([release, debug], undefined, () => assert.fail("unexpected probe")), release);
});

test("unsupported engine preserves the default host for its startup error", (t) => {
    const { release, debug, listEngines } = hosts(t, [["quickjs"], ["quickjs"]]);
    assert.equal(findLocalHost([release, debug], "v8", listEngines), release);
});

test("engine names match the host's case-insensitive parsing", (t) => {
    const { release, debug, listEngines } = hosts(t, [["quickjs"], ["v8"]]);
    assert.equal(findLocalHost([release, debug], "V8", listEngines), debug);
});

test("missing release falls back to debug", (t) => {
    const { release, debug, listEngines } = hosts(t, [["quickjs"], ["v8"]]);
    rmSync(release);
    assert.equal(findLocalHost([release, debug], "v8", listEngines), debug);
});

test("no local build leaves selection to PATH or the download cache", () => {
    assert.equal(findLocalHost([], "v8"), null);
});

const cli = fileURLToPath(new URL("../src/index.mjs", import.meta.url));
for (const override of ["--host", "KAPLAY_NATIVE_HOST"]) {
    test(`${override} remains authoritative with a requested engine`, () => {
        const env = { ...process.env };
        delete env.KAPLAY_NATIVE_HOST;
        const args = [cli, "doctor", "--js-engine", "v8"];
        if (override === "--host") args.push("--host", process.execPath);
        else env.KAPLAY_NATIVE_HOST = process.execPath;
        const result = spawnSync(process.execPath, args, { env, encoding: "utf8" });
        assert.equal(result.status, 0, result.stderr);
        const line = result.stdout.split("\n").find((line) => line.startsWith("host binary"));
        assert.equal(line.slice("host binary".length).trim(), process.execPath);
    });
}
