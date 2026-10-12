import { existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

function parseArgs(argv) {
    const options = { defines: {}, virtuals: {}, _: [] };
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (!arg.startsWith("--")) {
            options._.push(arg);
            continue;
        }
        const key = arg.slice(2);
        const value = argv[++i];
        if (key === "define") {
            const separator = value.indexOf("=");
            options.defines[value.slice(0, separator)] = value.slice(separator + 1);
        } else if (key === "virtual") {
            const separator = value.indexOf("=");
            options.virtuals[value.slice(0, separator)] = value.slice(separator + 1);
        } else {
            options[key] = value;
        }
    }
    return options;
}

function loadEsbuild(root) {
    if (process.env.KAPLAY_NATIVE_ESBUILD) {
        return createRequire(import.meta.url)(process.env.KAPLAY_NATIVE_ESBUILD);
    }
    const candidates = [
        join(root, "node_modules", "esbuild"),
        join(root, "..", "node_modules", "esbuild"),
        join(root, "..", "server", "node_modules", "esbuild"),
    ];
    for (const candidate of candidates) {
        if (existsSync(join(candidate, "package.json"))) {
            return createRequire(join(candidate, "package.json"))(candidate);
        }
    }
    try {
        return createRequire(join(root, "package.json"))("esbuild");
    } catch {
        throw new Error(
            "esbuild was not found; install it in the game project or set KAPLAY_NATIVE_ESBUILD",
        );
    }
}

const options = parseArgs(process.argv.slice(2));
if (!options.root) {
    console.error(`usage: node integrations/vite-game/build.mjs --root <project> [options]

  --root <dir>       Project containing the entry script (required)
  --entry <file>     Entry relative to the root (default: src/main.ts)
  --out <file>       Output bundle (default: build/<project>/game.js)
  --define K=V       Override an import.meta.env value (repeatable)
  --virtual name=src Virtual module contents (repeatable)`);
    process.exit(2);
}

const root = resolve(options.root);
const entry = options.entry ?? "src/main.ts";
const projectName = root.split(/[\\/]/).filter(Boolean).at(-1) ?? "game";
const outfile = resolve(options.out ?? join(repoRoot, "build", projectName, "game.js"));
mkdirSync(dirname(outfile), { recursive: true });

const virtuals = {
    "virtual:pwa-register": "export const registerSW = () => () => {};",
    "virtual:kaplay-sprite-atlas-manifest": "export default {};",
    "@stanko/kaplay-inspector": "export default () => {};",
    "@stanko/kaplay-inspector/dist/styles.css": "",
    ...options.virtuals,
};

const defines = {
    "import.meta.env.DEV": "false",
    "import.meta.env.PROD": "true",
    "import.meta.env.MODE": '"production"',
    "import.meta.env.VITE_ASSET_URL": '""',
    "import.meta.env.VITE_SERVER_URL": '""',
    ...options.defines,
};

const esbuild = loadEsbuild(root);
const result = await esbuild.build({
    absWorkingDir: root,
    entryPoints: [join(root, entry)],
    bundle: true,
    format: "iife",
    platform: "browser",
    target: "es2020",
    outfile,
    sourcemap: true,
    logLevel: "warning",
    loader: { ".png": "copy", ".ttf": "copy", ".webp": "copy", ".jpg": "copy" },
    define: defines,
    plugins: [
        {
            name: "kaplay-native-virtual-modules",
            setup(build) {
                build.onResolve({ filter: /^(virtual:|@stanko\/kaplay-inspector)/ }, (args) => ({
                    path: args.path,
                    namespace: "vite-virtual",
                }));
                build.onLoad({ filter: /.*/, namespace: "vite-virtual" }, (args) => ({
                    contents: virtuals[args.path] ?? "",
                    loader: args.path.endsWith(".css") ? "css" : "js",
                }));
            },
        },
    ],
});

console.log(`bundled ${entry} into ${outfile} (${result.warnings.length} warnings)`);

const assetDir = join(root, "public");
if (existsSync(assetDir)) {
    console.log("");
    console.log("run it with:");
    console.log(
        `  kaplay-native-host --script ${outfile} --dir ${assetDir} --window 1280x720`,
    );
}
