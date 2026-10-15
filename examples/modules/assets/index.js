import { answer, loads, url } from "./chunk.js";

console.log(`MODULE STATIC ${answer} ${url}`);

const canvas = document.querySelector("canvas");
if (!canvas || canvas.width !== 320 || canvas.height !== 240) {
    throw new Error("missing canvas from index.html");
}
if (!document.querySelector(".viewport")) throw new Error("missing viewport element");
if (document.title !== "Split module smoke test") throw new Error("missing document title");
document.body.classList.add("playing");
if (!document.body.classList.contains("playing")) throw new Error("missing classList");
console.log(`MODULE DOM ${canvas.width}x${canvas.height}`);

import("./lazy.js").then((module) => {
    if (module.value !== 7 || module.loads !== loads || loads !== 1) {
        throw new Error("shared chunk loaded more than once");
    }
    console.log(`MODULE DYNAMIC ${module.value} ${module.url} ${loads}`);
});
