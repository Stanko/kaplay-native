import { answer, url } from "./chunk.js";

console.log(`MODULE STATIC ${answer} ${url}`);

import("./lazy.js").then((module) => {
    console.log(`MODULE DYNAMIC ${module.value} ${module.url}`);
});
