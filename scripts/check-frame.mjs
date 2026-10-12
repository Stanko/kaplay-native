import { readFileSync } from "node:fs";

const [path, widthArg, heightArg] = process.argv.slice(2);
if (!path || !widthArg || !heightArg) {
    console.error("usage: node scripts/check-frame.mjs <frame.rgba> <width> <height>");
    process.exit(2);
}

const width = Number(widthArg);
const height = Number(heightArg);
const pixels = readFileSync(path);
const expected = width * height * 4;
if (pixels.length !== expected) {
    console.error(`expected ${expected} bytes for ${width}x${height}, read ${pixels.length}`);
    process.exit(1);
}

const colors = new Set();
for (let i = 0; i < pixels.length; i += 4) {
    colors.add(pixels.readUInt32LE(i));
    if (colors.size > 64) break;
}

if (colors.size < 3) {
    console.error(`frame is flat: only ${colors.size} distinct color(s)`);
    process.exit(1);
}

console.log(`frame has ${colors.size} distinct colors`);
