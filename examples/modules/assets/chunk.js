globalThis.chunkLoads = (globalThis.chunkLoads ?? 0) + 1;

export const answer = 42;
export const url = import.meta.url;
export const loads = globalThis.chunkLoads;
