#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import {
    copyFileSync,
    existsSync,
    mkdirSync,
    readdirSync,
    readFileSync,
    statSync,
    watch,
    writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { createZip } from "./zip.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");
const hostName = process.platform === "win32" ? "kaplay-native-host.exe" : "kaplay-native-host";

function usage() {
    console.log(`kaplay-native <command> [options]

Commands:
  run [dir]         Run a game from a directory
  dev [dir]         Run and restart on file changes
  build [dir]       Bundle a game and pack it into a .pak
  test [dir]        Run a game headlessly and compare a screenshot
  doctor            Check the local toolchain

Common options:
  --host <path>     Path to the kaplay-native-host binary
  --pak <path>      Run or write a .pak archive
  --entry <file>    Game entry script (default: main.js)
  --web-dist <dir>  Run a web build directory (Vite/Rollup output, uses its index.html entry)
  --preload <file>  Script evaluated before the entry (repeatable)
  --window <WxH>    Window size (default: 960x540)
  --frames <n>      Exit after n frames
  --screenshot <p>  Write a screenshot on exit
  --out <path>      Output path for build
  --headless        Do not show a window
  --fullscreen      Start fullscreen
  --title <name>    Window title
  --allow-network   Allow network requests (disabled by default)
  --no-audio        Run without audio output
  --no-vsync        Disable vertical sync (uncapped frame rate)
  --stats           Log the frame rate while the game runs
  --url <url>       Page URL games see as window.location (resolves relative requests)
  --x11             Prefer the X11 backend on Linux
`);
}

function parseArgs(argv) {
    const options = { preload: [], _: [] };
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (!arg.startsWith("--")) {
            options._.push(arg);
            continue;
        }
        const key = arg.slice(2);
        const next = argv[i + 1];
        const takesValue = !["headless", "fullscreen", "allow-network", "no-audio", "no-vsync", "stats", "x11", "help", "update"].includes(key);
        if (takesValue && next !== undefined && !next.startsWith("--")) {
            if (key === "preload") options.preload.push(next);
            else options[key] = next;
            i++;
        } else {
            options[key] = true;
        }
    }
    return options;
}

function which(binary) {
    const result = spawnSync(process.platform === "win32" ? "where" : "which", [binary], {
        encoding: "utf8",
    });
    return result.status === 0 ? binary : null;
}

function findHost(explicit) {
    const candidates = [
        explicit,
        process.env.KAPLAY_NATIVE_HOST,
        join(repoRoot, "target", "release", hostName),
        join(repoRoot, "target", "debug", hostName),
    ].filter(Boolean);
    for (const candidate of candidates) {
        if (candidate.includes("/") || candidate.includes("\\")) {
            if (existsSync(candidate)) return candidate;
        } else if (which(candidate)) {
            return candidate;
        }
    }
    return which(hostName);
}

function hostArgs(options, extra = {}) {
    const args = [];
    if (options.pak) args.push("--pak", options.pak);
    if (extra.dir) args.push("--dir", extra.dir);
    if (extra.script) args.push("--script", extra.script);
    for (const preload of options.preload) args.push("--preload", preload);
    if (options.window) args.push("--window", options.window);
    if (options.frames) args.push("--frames", String(options.frames));
    if (options.screenshot) args.push("--screenshot", options.screenshot);
    if (options.title) args.push("--title", options.title);
    if (options.appid) args.push("--appid", String(options.appid));
    if (options.fullscreen) args.push("--fullscreen");
    if (options.headless) args.push("--headless");
    if (options["allow-network"]) args.push("--allow-network");
    if (options["no-audio"]) args.push("--no-audio");
    if (options["no-vsync"]) args.push("--no-vsync");
    if (options.stats) args.push("--stats");
    if (options.url) args.push("--url", options.url);
    if (options.x11) args.push("--x11");
    return args;
}

function gameDir(options) {
    const dir = resolve(options["web-dist"] ?? options._[0] ?? ".");
    if (!existsSync(dir)) {
        console.error(`game directory not found: ${dir}`);
        process.exit(1);
    }
    return dir;
}

function withHost(options, args, { capture = false } = {}) {
    const host = findHost(options.host);
    if (!host) {
        console.error("kaplay-native-host not found. Build it with `cargo build --release` or pass --host <path>.");
        process.exit(1);
    }
    if (capture) {
        return spawnSync(host, args, { encoding: "utf8" });
    }
    const child = spawn(host, args, { stdio: "inherit" });
    child.on("exit", (code) => process.exit(code ?? 0));
    return child;
}

function autoPreload(dir, options) {
    if (options.preload.length) return options;
    const candidate = join(dir, "kaplay.js");
    if (existsSync(candidate)) {
        return { ...options, preload: [candidate] };
    }
    return options;
}

function runCommand(options) {
    const dir = gameDir(options);
    withHost(autoPreload(dir, options), hostArgs(options, { dir }));
}

function devCommand(options) {
    const dir = gameDir(options);
    const base = autoPreload(dir, options);
    const start = () => {
        const child = spawn(findHost(options.host) ?? "kaplay-native-host", hostArgs(base, { dir }), {
            stdio: "inherit",
        });
        child.on("exit", () => {});
        return child;
    };
    let child = start();
    console.log(`watching ${dir} for changes`);
    watch(dir, { recursive: true }, (_event, file) => {
        if (!file || file.includes("node_modules") || file.endsWith("~")) return;
        console.log(`reloading (${file})`);
        child.kill();
        child = start();
    });
}

function collectFiles(root, base = root, output = []) {
    for (const name of readdirSync(root)) {
        if (name.startsWith(".") || name === "node_modules" || name === "dist" || name === "target") continue;
        const full = join(root, name);
        const stats = statSync(full);
        if (stats.isDirectory()) {
            collectFiles(full, base, output);
        } else {
            output.push({ name: relative(base, full).split("\\").join("/"), path: full });
        }
    }
    return output;
}

async function bundle(entry, outfile) {
    let esbuild = null;
    try {
        esbuild = await import("esbuild");
    } catch {
        esbuild = null;
    }
    if (!esbuild) return false;
    await esbuild.build({
        entryPoints: [entry],
        bundle: true,
        format: "iife",
        outfile,
        logLevel: "warning",
    });
    return true;
}

async function buildCommand(options) {
    const dir = gameDir(options);
    const out = options.out ? resolve(options.out) : join(dir, "dist", `${basename(dir)}.pak`);
    mkdirSync(dirname(out), { recursive: true });
    const entryName = options.entry ?? "main.js";
    let bundled = false;
    if (options.bundle !== false) {
        const entry = join(dir, entryName);
        if (existsSync(entry)) {
            bundled = await bundle(entry, join(dir, ".kaplay-native-bundle.js"));
        }
    }
    const files = collectFiles(dir).filter((file) => !file.name.startsWith(".kaplay-native-bundle"));
    if (bundled) {
        files.push({ name: entryName, path: join(dir, ".kaplay-native-bundle.js") });
    }
    const entries = files.map((file) => ({ name: file.name, data: readFileSync(file.path) }));
    writeFileSync(out, createZip(entries));
    console.log(`packed ${entries.length} files into ${out}${bundled ? " (bundled with esbuild)" : ""}`);
}

function testCommand(options) {
    const dir = gameDir(options);
    const screenshot = resolve(options.screenshot ?? join(dir, "test", "actual.png"));
    const golden = resolve(options.golden ?? join(dir, "test", "golden.png"));
    mkdirSync(dirname(screenshot), { recursive: true });
    const args = hostArgs({ ...options, frames: options.frames ?? 120, screenshot, headless: true }, { dir });
    const result = withHost(options, args, { capture: true });
    if (result.status !== 0) {
        console.error(result.stdout);
        console.error(result.stderr);
        process.exit(result.status ?? 1);
    }
    if (options.update) {
        copyFileSync(screenshot, golden);
        console.log(`updated golden screenshot ${golden}`);
        return;
    }
    if (!existsSync(golden)) {
        console.log(`no golden screenshot at ${golden}; wrote ${screenshot} (run with --update to accept)`);
        return;
    }
    const actual = readFileSync(screenshot);
    const expected = readFileSync(golden);
    if (actual.equals(expected)) {
        console.log("screenshot matches golden");
        return;
    }
    console.error(`screenshot differs from golden (${actual.length} vs ${expected.length} bytes)`);
    console.error(`actual: ${screenshot}`);
    process.exit(1);
}

function doctorCommand(options) {
    const checks = [];
    checks.push(["node", process.version]);
    checks.push(["platform", `${process.platform} ${process.arch}`]);
    const host = findHost(options.host);
    checks.push(["host binary", host ?? "not found"]);
    checks.push(["DISPLAY", process.env.DISPLAY ?? "(unset)"]);
    checks.push(["WAYLAND_DISPLAY", process.env.WAYLAND_DISPLAY ?? "(unset)"]);
    if (host) {
        const version = spawnSync(host, ["--version"], { encoding: "utf8" });
        checks.push(["host version", (version.stdout || version.stderr || "").trim() || "unknown"]);
    }
    for (const [name, value] of checks) {
        console.log(`${name.padEnd(18)} ${value}`);
    }
}

const command = process.argv[2];
const options = parseArgs(process.argv.slice(3));

if (!command || options.help) {
    usage();
    process.exit(command ? 0 : 1);
}

switch (command) {
    case "run":
        runCommand(options);
        break;
    case "dev":
        devCommand(options);
        break;
    case "build":
        await buildCommand(options);
        break;
    case "test":
        testCommand(options);
        break;
    case "doctor":
        doctorCommand(options);
        break;
    default:
        console.error(`unknown command: ${command}`);
        usage();
        process.exit(1);
}
