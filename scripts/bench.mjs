import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
    existsSync,
    mkdirSync,
    readFileSync,
    readdirSync,
    rmSync,
    statSync,
    writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "..");
const platform = `${process.platform}-${process.arch}`;
const warnThreshold = 0.05;
const failThreshold = 0.1;
// Metrics below this baseline value are reported but not gated: sub-microsecond
// numbers bounce by 2x between runs even on an idle machine.
const minGatedValue = 1000;

function usage() {
    console.log(`kaplay-native bench

  node scripts/bench.mjs run       [options]   run everything and write bench/results
  node scripts/bench.mjs baseline  [options]   run everything and replace the committed baseline
  node scripts/bench.mjs compare   [options]   run everything and diff against the baseline

Options:
  --engine <name>     quickjs (default), v8, or both
  --frames <n>        frames for the integrated scenario (default 900)
  --frax-frames <n>   frames for the Frax ground-truth run (default 900)
  --only <part>       crates, game, or all (default all)
  --quick             pass --quick to criterion (shorter, noisier)
  --host <path>       host binary for the quickjs engine
  --v8-host <path>    host binary for the v8 engine
  --baseline <path>   baseline json for compare (default benchmarks/baselines/<platform>-<engine>.json)
  --no-build          skip cargo builds and use existing binaries
  --verbose           print host output for the integrated and frax runs
`);
}

function parse(argv) {
    const options = { mode: argv[0] ?? "run", engine: "quickjs", frames: 900, fraxFrames: 900, only: "all", quick: false, build: true };
    for (let index = 1; index < argv.length; index++) {
        const arg = argv[index];
        const next = argv[index + 1];
        switch (arg) {
            case "--engine":
                options.engine = next;
                index++;
                break;
            case "--frames":
                options.frames = Number(next);
                index++;
                break;
            case "--frax-frames":
                options.fraxFrames = Number(next);
                index++;
                break;
            case "--only":
                options.only = next;
                index++;
                break;
            case "--baseline":
                options.baseline = next;
                index++;
                break;
            case "--host":
                options.host = next;
                index++;
                break;
            case "--v8-host":
                options.v8Host = next;
                index++;
                break;
            case "--quick":
                options.quick = true;
                break;
            case "--no-build":
                options.build = false;
                break;
            case "--verbose":
                options.verbose = true;
                break;
            case "--help":
                usage();
                process.exit(0);
                break;
            default:
                console.error(`unknown option: ${arg}`);
                process.exit(2);
        }
    }
    return options;
}

function run(command, args, { cwd = repo, env = process.env, timeout = 600_000 } = {}) {
    const result = spawnSync(command, args, { cwd, env, encoding: "utf8", timeout, maxBuffer: 64 * 1024 * 1024 });
    return { status: result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "", error: result.error };
}

function runAsync(command, args, { cwd = repo, env = process.env, timeout = 900_000 } = {}) {
    return new Promise((resolveRun) => {
        const child = spawn(command, args, { cwd, env, stdio: ["ignore", "pipe", "pipe"] });
        let stdout = "";
        let stderr = "";
        let timedOut = false;
        const timer = setTimeout(() => {
            timedOut = true;
            child.kill("SIGKILL");
        }, timeout);
        child.stdout.on("data", (chunk) => (stdout += chunk));
        child.stderr.on("data", (chunk) => (stderr += chunk));
        child.on("close", (status) => {
            clearTimeout(timer);
            resolveRun({ status, stdout, stderr, error: timedOut ? new Error("timed out") : null });
        });
        child.on("error", (error) => {
            clearTimeout(timer);
            resolveRun({ status: -1, stdout, stderr, error });
        });
    });
}

function hostBinaryPath(engine) {
    return join(repo, "target", "release", `kaplay-native-host-${engine}`);
}

function buildHosts(options) {
    const engines = options.engine === "both" ? ["quickjs", "v8"] : [options.engine];
    const binaries = {};
    for (const engine of engines) {
        const explicit = engine === "v8" ? options.v8Host : options.host;
        if (explicit) {
            binaries[engine] = resolve(explicit);
            continue;
        }
        const cached = hostBinaryPath(engine);
        if (!options.build && existsSync(cached)) {
            binaries[engine] = cached;
            continue;
        }
        const args = ["build", "--release", "-p", "kaplay-native-host"];
        if (engine === "v8") args.push("--no-default-features", "--features", "v8");
        console.log(`building the ${engine} host`);
        const result = run("cargo", args, { timeout: 3_600_000 });
        if (result.status !== 0) {
            console.error(result.stderr || result.stdout);
            throw new Error(`failed to build the ${engine} host`);
        }
        const built = join(repo, "target", "release", "kaplay-native-host");
        const target = hostBinaryPath(engine);
        const copied = run("cp", [built, target]);
        if (copied.status !== 0) throw new Error(`failed to copy the ${engine} host`);
        binaries[engine] = target;
    }
    return binaries;
}

function acceptKey(key) {
    return createHash("sha1")
        .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
        .digest("base64");
}

function decodeFrame(buffer) {
    if (buffer.length < 6) return { kind: "none" };
    const opcode = buffer[0] & 0x0f;
    const masked = (buffer[1] & 0x80) !== 0;
    let length = buffer[1] & 0x7f;
    let offset = 2;
    if (length === 126) {
        length = buffer.readUInt16BE(2);
        offset = 4;
    } else if (length === 127) {
        length = Number(buffer.readBigUInt64BE(2));
        offset = 10;
    }
    let mask = null;
    if (masked) {
        mask = buffer.subarray(offset, offset + 4);
        offset += 4;
    }
    const payload = Buffer.from(buffer.subarray(offset, offset + length));
    if (masked) {
        for (let index = 0; index < payload.length; index++) {
            payload[index] ^= mask[index % 4];
        }
    }
    if (opcode === 8) return { kind: "close" };
    if (opcode === 1 || opcode === 2) return { kind: "message", payload };
    return { kind: "none" };
}

function encodeFrame(payload, opcode) {
    const data = Buffer.from(payload);
    let header;
    if (data.length < 126) {
        header = Buffer.from([0x80 | opcode, data.length]);
    } else if (data.length < 65536) {
        header = Buffer.alloc(4);
        header[0] = 0x80 | opcode;
        header[1] = 126;
        header.writeUInt16BE(data.length, 2);
    } else {
        header = Buffer.alloc(10);
        header[0] = 0x80 | opcode;
        header[1] = 127;
        header.writeBigUInt64BE(BigInt(data.length), 2);
    }
    return Buffer.concat([header, data]);
}

async function startServer() {
    const body = Buffer.alloc(65536);
    for (let index = 0; index < body.length; index++) body[index] = (index * 7) & 0xff;
    const server = createServer((request, response) => {
        if (request.url === "/body.bin") {
            response.writeHead(200, { "content-length": String(body.length), "content-type": "application/octet-stream" });
            response.end(body);
            return;
        }
        response.writeHead(404, { "content-length": "0" });
        response.end();
    });
    server.on("upgrade", (request, socket) => {
        if (request.url !== "/socket") {
            socket.destroy();
            return;
        }
        socket.write(
            [
                "HTTP/1.1 101 Switching Protocols",
                "Upgrade: websocket",
                "Connection: Upgrade",
                `Sec-WebSocket-Accept: ${acceptKey(request.headers["sec-websocket-key"])}`,
                "",
                "",
            ].join("\r\n"),
        );
        socket.on("data", (chunk) => {
            const frame = decodeFrame(chunk);
            if (frame.kind === "message") socket.write(encodeFrame(frame.payload, 1));
            else if (frame.kind === "close") socket.end();
        });
        socket.on("error", () => socket.destroy());
    });
    await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
    return {
        port: server.address().port,
        close: () => new Promise((closed) => server.close(closed)),
    };
}

function benchLines(output) {
    const checks = {};
    for (const line of output.split("\n")) {
        const match = line.match(/BENCH ([^=]+)=(.+?)\s*$/);
        if (match) checks[match[1]] = match[2];
    }
    return checks;
}

function infoLines(output) {
    const info = {};
    for (const line of output.split("\n")) {
        const match = line.match(/BENCH_INFO ([^=]+)=(.+?)\s*$/);
        if (match) info[match[1]] = match[2];
    }
    return info;
}

async function runHost(binary, args, options = {}) {
    const jsonPath = join(tmpdir(), `kaplay-bench-${process.pid}-${Math.random().toString(36).slice(2)}.json`);
    const result = await runAsync(binary, [...args, "--bench-json", jsonPath], {
        timeout: options.timeout ?? 900_000,
    });
    let bench = null;
    if (existsSync(jsonPath)) {
        try {
            bench = JSON.parse(readFileSync(jsonPath, "utf8"));
        } catch (error) {
            bench = { error: String(error) };
        }
        rmSync(jsonPath, { force: true });
    }
    const output = `${result.stdout}\n${result.stderr}`;
    const renderer = output.match(/renderer: (.+)/)?.[1]?.trim() ?? "";
    if (options.verbose) console.log(output.trim());
    return {
        status: result.status,
        error: result.error ? String(result.error) : null,
        checks: benchLines(output),
        info: infoLines(output),
        bench,
        renderer,
        output: output.split("\n").filter((line) => line.includes("BENCH")).join("\n"),
    };
}

function integratedArgs(port, frames) {
    return [
        "--script",
        "examples/bench/main.js",
        "--dir",
        "examples/bench",
        "--window",
        "960x540",
        "--frames",
        String(frames),
        "--no-vsync",
        "--no-audio",
        "--headless",
        "--x11",
        "--allow-network",
        "--url",
        `http://127.0.0.1:${port}/index.html`,
    ];
}

function collectCriterion(root, since) {
    const found = {};
    const walk = (directory, prefix) => {
        if (!existsSync(directory)) return;
        for (const entry of readdirSync(directory)) {
            const full = join(directory, entry);
            if (!statSync(full).isDirectory()) continue;
            if (entry === "new") {
                const file = join(full, "estimates.json");
                if (existsSync(file) && statSync(file).mtimeMs >= since) {
                    try {
                        const data = JSON.parse(readFileSync(file, "utf8"));
                        found[prefix] = { unit: "ns", value: data.median.point_estimate };
                    } catch {
                        continue;
                    }
                }
                continue;
            }
            walk(full, prefix ? `${prefix}/${entry}` : entry);
        }
    };
    walk(root, "");
    return found;
}

function runCriterion(options) {
    const crates = [
        ["kaplay-native-pak", "pak"],
        ["kaplay-native-audio", "audio"],
        ["kaplay-native-canvas2d", "canvas2d"],
        ["kaplay-native-video", "video"],
        ["kaplay-native-net", "net"],
    ];
    const started = Date.now();
    for (const [crate, bench] of crates) {
        const args = ["bench", "-p", crate, "--bench", bench, "--"];
        if (options.quick) args.push("--quick");
        console.log(`criterion ${crate}`);
        const result = run("cargo", args, { timeout: 3_600_000 });
        if (result.status !== 0) {
            console.error(result.stderr || result.stdout);
            throw new Error(`criterion failed for ${crate}`);
        }
    }
    return collectCriterion(join(repo, "target", "criterion"), started);
}

function runGfxBench(options) {
    if (options.build) {
        console.log("building the gfx bench");
        const built = run("cargo", ["build", "--release", "-p", "kaplay-native-gfx", "--example", "bench"], { timeout: 1_800_000 });
        if (built.status !== 0) throw new Error("failed to build the gfx bench");
    }
    const binary = join(repo, "target", "release", "examples", "bench");
    if (!existsSync(binary)) return { renderer: "", metrics: {} };
    const result = run(binary, [], { timeout: 900_000 });
    const output = `${result.stdout}\n${result.stderr}`;
    if (result.status !== 0) {
        return { renderer: "", metrics: {}, error: output.trim().split("\n").slice(-3).join(" ") };
    }
    const metrics = {};
    let renderer = "";
    for (const line of output.split("\n")) {
        const rendererMatch = line.match(/GFX_RENDERER (\{.*\})/);
        if (rendererMatch) renderer = JSON.parse(rendererMatch[1]).renderer;
        const match = line.match(/GFX_BENCH (\{.*\})/);
        if (!match) continue;
        const entry = JSON.parse(match[1]);
        metrics[entry.bench] = { unit: "ns", value: entry.ns_per_op, gib_per_s: entry.gib_per_s, bytes_per_op: entry.bytes_per_op };
    }
    return { renderer, metrics };
}

async function gather(options, binaries, server) {
    const result = { created: new Date().toISOString(), platform, engines: {} };
    const engines = Object.keys(binaries);
    for (const engine of engines) {
        const entry = { renderer: "", integrated: {}, frax: {}, checks: {}, info: {} };
        if (options.only !== "crates") {
            console.log(`integrated scenario (${engine}, ${options.frames} frames)`);
            const integrated = await runHost(binaries[engine], integratedArgs(server.port, options.frames), { verbose: options.verbose });
            entry.integrated = integrated.bench ?? { status: integrated.status };
            entry.checks = integrated.checks;
            entry.info = integrated.info;
            entry.renderer = integrated.renderer || entry.renderer;
            if (integrated.status !== 0) console.error(integrated.output || `host exited with ${integrated.status}`);

            const pak = join(repo, "frax-kaplay.pak");
            if (existsSync(pak)) {
                console.log(`frax ground truth (${engine}, ${options.fraxFrames} frames)`);
                const frax = await runHost(
                    binaries[engine],
                    [
                        "--pak",
                        pak,
                        "--frames",
                        String(options.fraxFrames),
                        "--window",
                        "1280x720",
                        "--no-vsync",
                        "--no-audio",
                        "--headless",
                        "--x11",
                    ],
                    { verbose: options.verbose },
                );
                entry.frax = frax.bench ?? { status: frax.status };
            }
        }
        result.engines[engine] = entry;
    }
    if (options.only !== "game") {
        result.criterion = runCriterion(options);
        result.gfx = runGfxBench(options);
    }
    return result;
}

function flatten(value, prefix, out) {
    for (const [key, entry] of Object.entries(value)) {
        const path = prefix ? `${prefix}.${key}` : key;
        if (typeof entry === "number") {
            out[path] = entry;
        } else if (entry && typeof entry === "object") {
            flatten(entry, path, out);
        }
    }
    return out;
}

function better(key) {
    return key.endsWith(".gib_per_s") || key.endsWith(".fps") ? "up" : "down";
}

function compare(baseline, current) {
    const base = flatten(baseline, "", {});
    const now = flatten(current, "", {});
    const rows = [];
    let failures = 0;
    let warnings = 0;
    for (const [key, value] of Object.entries(now)) {
        if (typeof value !== "number" || !Number.isFinite(value)) continue;
        const previous = base[key];
        if (typeof previous !== "number" || previous === 0) {
            rows.push({ key, delta: null });
            continue;
        }
        const raw = (value - previous) / previous;
        const delta = better(key) === "up" ? -raw : raw;
        const gated = previous >= minGatedValue;
        rows.push({ key, delta, previous, value, gated });
        if (!gated) continue;
        if (delta > failThreshold) failures++;
        else if (delta > warnThreshold) warnings++;
    }
    rows.sort((a, b) => {
        if (a.delta === null && b.delta === null) return 0;
        if (a.delta === null) return -1;
        if (b.delta === null) return 1;
        return b.delta - a.delta;
    });
    console.log(`\nbiggest regressions:`);
    for (const row of rows.slice(0, 12)) {
        const tag = row.delta === null ? "new  " : `${(row.delta * 100).toFixed(1)}%`.padStart(6);
        console.log(`  ${tag}${row.gated === false ? " *" : "  "}  ${row.key}`);
    }
    console.log(`\nbiggest improvements:`);
    for (const row of rows.slice(-6).reverse()) {
        if (row.delta === null) continue;
        console.log(`  ${(row.delta * 100).toFixed(1)}%`.padStart(8) + `  ${row.key}`);
    }
    console.log(`\n${failures} failing, ${warnings} warning (* below the ${minGatedValue} ns gate)`);
    return failures;
}

function checkParity(result) {
    const engines = Object.keys(result.engines);
    if (engines.length < 2) return 0;
    const [first, second] = engines;
    const left = result.engines[first].checks;
    const right = result.engines[second].checks;
    let failures = 0;
    for (const key of new Set([...Object.keys(left), ...Object.keys(right)])) {
        if (left[key] !== right[key]) {
            console.error(`parity mismatch ${key}: ${first}=${left[key]} ${second}=${right[key]}`);
            failures++;
        }
    }
    return failures;
}

async function main() {
    const options = parse(process.argv.slice(2));
    if (!["run", "baseline", "compare"].includes(options.mode)) {
        usage();
        process.exit(2);
    }
    const binaries = buildHosts(options);
    const server = await startServer();
    let result;
    try {
        result = await gather(options, binaries, server);
    } finally {
        await server.close();
    }
    result.baseline = options.mode === "baseline";
    const resultsDir = join(repo, "benchmarks", "results");
    const baselinesDir = join(repo, "benchmarks", "baselines");
    mkdirSync(resultsDir, { recursive: true });
    mkdirSync(baselinesDir, { recursive: true });
    const engines = Object.keys(result.engines);
    const engineLabel = engines.length > 1 ? "both" : engines[0];
    const resultPath = join(resultsDir, `${platform}-${engineLabel}.json`);
    writeFileSync(resultPath, `${JSON.stringify(result, null, 2)}\n`);
    console.log(`\nwrote ${resultPath}`);

    let failures = checkParity(result);
    if (options.mode === "baseline") {
        const baselinePath = join(baselinesDir, `${platform}-${engineLabel}.json`);
        writeFileSync(baselinePath, `${JSON.stringify(result, null, 2)}\n`);
        console.log(`wrote ${baselinePath}`);
    } else if (options.mode === "compare") {
        const baselinePath =
            options.baseline ?? join(baselinesDir, `${platform}-${engineLabel}.json`);
        if (!existsSync(baselinePath)) {
            console.error(`no baseline at ${baselinePath}; run the baseline mode first`);
            process.exit(2);
        }
        const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
        const baselineEngine = engines[0];
        const comparable = {};
        if (baseline.engines?.[baselineEngine]) {
            comparable[`engines.${baselineEngine}`] = baseline.engines[baselineEngine];
        }
        if (baseline.criterion) comparable.criterion = baseline.criterion;
        if (baseline.gfx) comparable.gfx = baseline.gfx;
        const current = {};
        if (result.engines[baselineEngine]) current[`engines.${baselineEngine}`] = result.engines[baselineEngine];
        if (result.criterion) current.criterion = result.criterion;
        if (result.gfx) current.gfx = result.gfx;
        failures += compare(comparable, current);
    }
    if (failures > 0) process.exit(1);
}

main().catch((error) => {
    console.error(error.stack || String(error));
    process.exit(1);
});
