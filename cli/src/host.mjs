import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";

function compiledEngines(host) {
    const result = spawnSync(host, ["--engines"], { encoding: "utf8" });
    return result.status === 0 ? result.stdout.trim().split(/\s+/) : [];
}

// Keep release preference unless another local build supports the requested engine.
export function findLocalHost(candidates, engine, listEngines = compiledEngines) {
    const existing = candidates.filter((candidate) => existsSync(candidate));
    if (engine) {
        const compatible = existing.find((candidate) => listEngines(candidate).includes(engine.toLowerCase()));
        if (compatible) return compatible;
    }
    // Let the host report an unsupported engine when no compatible build exists.
    return existing[0] ?? null;
}
