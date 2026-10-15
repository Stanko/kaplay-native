import { answer } from "./chunk.js";

console.log(`MODULE STATIC ${answer}`);

import("./lazy.js").then((module) => {
    console.log(`MODULE DYNAMIC ${module.value}`);
});
