import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import vm from "node:vm";

import { installStub } from "./stub.js";

const here = dirname(fileURLToPath(import.meta.url));
const stub = installStub();
vm.runInThisContext(readFileSync(join(here, "..", "src", "shim.js"), "utf8"));

function advance(ms) {
    stub.state.time += ms;
    globalThis.__kapNative_onFrame(stub.state.time);
}

function drain() {
    globalThis.__kapNative_onFrame(stub.state.time);
}

function makeCanvas(width = 320, height = 240) {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    document.body.appendChild(canvas);
    return canvas;
}

test("timers and animation frames run in order", () => {
    const order = [];
    setTimeout(() => order.push("timeout"), 10);
    requestAnimationFrame(() => order.push("frame1"));
    requestAnimationFrame(() => order.push("frame2"));
    const interval = setInterval(() => order.push("interval"), 5);

    drain();
    assert.deepEqual(order, ["frame1", "frame2"]);

    advance(10);
    assert.deepEqual(order, ["frame1", "frame2", "interval", "timeout"]);

    clearInterval(interval);
    advance(10);
    assert.deepEqual(order, ["frame1", "frame2", "interval", "timeout"]);
});

test("cancelAnimationFrame removes a queued callback", () => {
    let called = false;
    const id = requestAnimationFrame(() => {
        called = true;
    });
    cancelAnimationFrame(id);
    drain();
    assert.equal(called, false);
});

test("keyboard events reach canvas listeners", () => {
    const canvas = makeCanvas();
    const received = [];
    canvas.addEventListener("keydown", (event) => received.push(event));
    globalThis.__kapNative_dispatch(
        "keydown",
        JSON.stringify({ key: "ArrowLeft", code: "ArrowLeft", repeat: true, capsLock: true }),
    );
    assert.equal(received.length, 1);
    assert.equal(received[0].key, "ArrowLeft");
    assert.equal(received[0].code, "ArrowLeft");
    assert.equal(received[0].repeat, true);
    assert.equal(received[0].getModifierState("CapsLock"), true);
    assert.equal(received[0].defaultPrevented, false);
    received[0].preventDefault();
    assert.equal(received[0].defaultPrevented, true);
});

test("mouse offsets are relative to the drawn canvas", () => {
    const canvas = makeCanvas(960, 540);
    const events = [];
    canvas.addEventListener("mousemove", (event) => events.push(event));
    globalThis.__kapNative_dispatch(
        "mousemove",
        JSON.stringify({ clientX: 480, clientY: 270, movementX: 2, movementY: -3 }),
    );
    assert.equal(events[0].offsetX, 480);
    assert.equal(events[0].offsetY, 270);
    assert.equal(events[0].movementX, 2);
    assert.equal(events[0].movementY, -3);
});

test("fixed size canvases are letterboxed in the window", () => {
    const canvas = makeCanvas();
    canvas.style.cssText = "cursor: default;width: 320px;height: 240px";
    const events = [];
    canvas.addEventListener("mousedown", (event) => events.push(event));
    canvas.addEventListener("mouseup", (event) => events.push(event));
    globalThis.__kapNative_dispatch("mousedown", JSON.stringify({ clientX: 480, clientY: 270, button: 0 }));

    const rect = canvas.getBoundingClientRect();
    assert.equal(rect.x, 120);
    assert.equal(rect.y, 0);
    assert.equal(rect.width, 720);
    assert.equal(rect.height, 540);
    assert.equal(events[0].offsetX, 160);
    assert.equal(events[0].offsetY, 120);
    assert.equal(events[0].buttons, 1);

    globalThis.__kapNative_dispatch("mouseup", JSON.stringify({ clientX: 480, clientY: 270, button: 0 }));
    assert.equal(events[1].buttons, 0);
});

test("resize updates layout and notifies observers", () => {
    const canvas = makeCanvas();
    const observed = [];
    const observer = new ResizeObserver((entries) => observed.push(entries));
    observer.observe(canvas);
    let windowResized = 0;
    window.addEventListener("resize", () => windowResized++);

    globalThis.__kapNative_dispatch(
        "resize",
        JSON.stringify({ width: 640, height: 480, framebufferWidth: 640, framebufferHeight: 480, dpr: 1 }),
    );

    assert.equal(window.innerWidth, 640);
    assert.equal(window.innerHeight, 480);
    assert.equal(canvas.offsetWidth, 640);
    assert.equal(windowResized, 1);
    assert.equal(observed.length, 1);
    assert.equal(observed[0][0].target, canvas);
});

test("canvas backing store resizes the drawing buffer", () => {
    const canvas = makeCanvas();
    const gl = canvas.getContext("webgl");
    canvas.width = 640;
    const resize = stub.calls.filter((call) => call.name === "gl.resizeDrawingBuffer").at(-1);
    assert.deepEqual(resize.args, [640, 240]);
    assert.equal(gl.drawingBufferWidth, 320);
});

test("images load from data urls and fire onload asynchronously", () => {
    const image = new Image();
    let loaded = null;
    image.onload = () => {
        loaded = [image.width, image.height];
    };
    image.src = "data:image/png;base64,AQIDBA==";
    assert.equal(loaded, null);
    drain();
    assert.deepEqual(loaded, [4, 1]);
});

test("images report errors for unknown sources", () => {
    const image = new Image();
    let error = null;
    image.onerror = () => {
        error = true;
    };
    image.src = "missing.png";
    drain();
    assert.equal(error, true);
});

test("local storage persists through the native bridge", () => {
    window.localStorage["highscore"] = JSON.stringify(4200);
    assert.equal(window.localStorage["highscore"], "4200");
    const saved = stub.calls.filter((call) => call.name === "storage.save").at(-1);
    assert.equal(saved.args[0], '{"highscore":"4200"}');
    assert.equal(window.localStorage.getItem("highscore"), "4200");
    window.localStorage.removeItem("highscore");
    assert.equal(window.localStorage.getItem("highscore"), null);
});

test("fonts register bytes and check families", async () => {
    const face = new FontFace("MyFont", new Uint8Array([9, 9, 9]));
    await face.load();
    assert.equal(face.status, "loaded");
    const registered = stub.calls.filter((call) => call.name === "font.register").at(-1);
    assert.deepEqual(registered.args, ["MyFont", 3]);
    assert.equal(document.fonts.check("64px MyFont"), true);
    assert.equal(document.fonts.check("64px monospace"), true);
    assert.equal(document.fonts.check("64px Unknown"), false);

    const urlFace = new FontFace("Loaded", "url(font.ttf)");
    await urlFace.load();
    const urlRegistered = stub.calls.filter((call) => call.name === "font.register").at(-1);
    assert.deepEqual(urlRegistered.args, ["Loaded", 4]);
});

test("webgl texture uploads dispatch on the source type", () => {
    const canvas = makeCanvas();
    const gl = canvas.getContext("webgl");

    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 16, 8, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    let call = stub.calls.filter((entry) => entry.name === "gl.texImage2D").at(-1);
    assert.deepEqual(call.args, [3553, 6408, 16, 8, 6408, 5121, null]);

    gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        2,
        2,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        new Uint8Array([1, 2, 3, 4]),
    );
    call = stub.calls.filter((entry) => entry.name === "gl.texImage2D").at(-1);
    assert.deepEqual(call.args.slice(0, 6), [3553, 6408, 2, 2, 6408, 5121]);
    assert.equal(Array.from(call.args[6]).join(","), "1,2,3,4");

    const image = new Image();
    image.src = "data:image/png;base64,AQIDBA==";
    drain();
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    call = stub.calls.filter((entry) => entry.name === "gl.texImage2DImage").at(-1);
    assert.equal(call.args.length, 5);
    assert.deepEqual(call.args.slice(0, 4), [3553, 6408, 6408, 5121]);

    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, canvas);
    assert.ok(stub.calls.some((entry) => entry.name === "gl.texImage2DCanvas"));

    gl.texSubImage2D(gl.TEXTURE_2D, 0, 3, 4, gl.RGBA, gl.UNSIGNED_BYTE, image);
    call = stub.calls.filter((entry) => entry.name === "gl.texSubImage2DImage").at(-1);
    assert.deepEqual(call.args, [3553, 3, 4, 6408, 5121, image.__id]);

    gl.texSubImage2D(gl.TEXTURE_2D, 0, 1, 2, gl.RGBA, gl.UNSIGNED_BYTE, canvas);
    call = stub.calls.filter((entry) => entry.name === "gl.texSubImage2DCanvas").at(-1);
    assert.deepEqual(call.args, [3553, 1, 2, 6408, 5121, canvas.__id]);
});

test("webgl buffers and uniforms normalize their arguments", () => {
    const canvas = makeCanvas();
    const gl = canvas.getContext("webgl");

    gl.bufferData(gl.ARRAY_BUFFER, 128, gl.DYNAMIC_DRAW);
    assert.ok(stub.calls.some((entry) => entry.name === "gl.bufferDataSize"));

    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([1, 2]), gl.STATIC_DRAW);
    let call = stub.calls.filter((entry) => entry.name === "gl.bufferData").at(-1);
    assert.equal(call.args[1].length, 8);

    gl.bufferSubData(gl.ARRAY_BUFFER, 4, new Uint16Array([7, 8]));
    call = stub.calls.filter((entry) => entry.name === "gl.bufferSubData").at(-1);
    assert.deepEqual(call.args.slice(0, 2), [34962, 4]);
    assert.equal(call.args[2].length, 4);

    gl.uniform1fv(null, [1, 2, 3]);
    call = stub.calls.filter((entry) => entry.name === "gl.uniform1fv").at(-1);
    assert.deepEqual(call.args, [-1, new Float32Array([1, 2, 3])]);

    gl.uniformMatrix4fv(5, false, new Float32Array(16));
    call = stub.calls.filter((entry) => entry.name === "gl.uniformMatrix4fv").at(-1);
    assert.equal(call.args[0], 5);
    assert.equal(call.args[1].length, 16);

    assert.equal(gl.getUniformLocation(1, "missing"), null);
    assert.ok(gl.getUniformLocation(1, "present") > 0);
    assert.equal(gl.getExtension("WEBGL_lose_context").loseContext instanceof Function, true);
    assert.equal(gl.getExtension("EXT_texture_filter_anisotropic"), null);
});

test("canvas 2d contexts proxy to native calls", () => {
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 32;
    const context = canvas.getContext("2d");
    context.fillStyle = "#ff8800";
    context.font = "24px monospace";
    context.textBaseline = "top";
    context.textAlign = "left";
    context.lineWidth = 3;
    context.lineJoin = "round";
    context.fillText("hi", 1, 2);
    context.strokeText("hi", 1, 2);
    context.clearRect(0, 0, 8, 8);

    const fill = stub.calls.filter((entry) => entry.name === "canvas.setFillStyle").at(-1);
    assert.equal(fill.args[1], "#ff8800");
    const font = stub.calls.filter((entry) => entry.name === "canvas.setFont").at(-1);
    assert.equal(font.args[1], "24px monospace");
    const baseline = stub.calls.filter((entry) => entry.name === "canvas.setTextBaseline").at(-1);
    assert.equal(baseline.args[1], "top");

    const metrics = context.measureText("hello");
    assert.deepEqual(metrics, {
        width: 35,
        actualBoundingBoxLeft: 0,
        actualBoundingBoxRight: 35,
        actualBoundingBoxAscent: 10,
        actualBoundingBoxDescent: 2,
    });

    const pixels = context.getImageData(0, 0, 2, 2);
    assert.ok(pixels instanceof ImageData);
    assert.ok(pixels.data instanceof Uint8ClampedArray);
    assert.equal(pixels.width, 2);
    assert.equal(pixels.height, 2);
    assert.equal(pixels.data[10], 10);

    context.putImageData(pixels, 4, 5);
    const put = stub.calls.filter((entry) => entry.name === "canvas.putImageData").at(-1);
    assert.deepEqual(put.args.slice(0, 5), [put.args[0], 4, 5, 2, 2]);
    assert.ok(put.args[5] instanceof Uint8Array);
});

test("canvas 2d drawImage forwards the right number of arguments", () => {
    const canvas = document.createElement("canvas");
    canvas.width = 64;
    canvas.height = 64;
    const context = canvas.getContext("2d");
    const image = new Image();
    image.src = "data:image/png;base64,AQIDBA==";
    drain();

    context.drawImage(image, 10, 20);
    let call = stub.calls.filter((entry) => entry.name === "canvas.drawImage").at(-1);
    assert.deepEqual(call.args.slice(1, 3), [0, image.__id]);
    assert.equal(call.args.length, 11);
    assert.deepEqual(call.args.slice(3, 11), [0, 0, 4, 1, 10, 20, 4, 1]);

    context.drawImage(image, 1, 2, 3, 4, 5, 6, 7, 8);
    call = stub.calls.filter((entry) => entry.name === "canvas.drawImage").at(-1);
    assert.deepEqual(call.args.slice(3, 11), [1, 2, 3, 4, 5, 6, 7, 8]);
});

test("canvas screenshots use the native image path", () => {
    const canvas = document.createElement("canvas");
    canvas.width = 4;
    canvas.height = 4;
    document.body.appendChild(canvas);
    const url = canvas.toDataURL();
    assert.ok(url.startsWith("data:image/png;base64,"));

    const glCanvas = document.createElement("canvas");
    glCanvas.width = 4;
    glCanvas.height = 4;
    document.body.appendChild(glCanvas);
    glCanvas.getContext("webgl");
    assert.ok(glCanvas.toDataURL().startsWith("data:image/png;base64,"));
    assert.ok(stub.calls.some((entry) => entry.name === "gl.screenshotLength"));
    assert.ok(stub.calls.some((entry) => entry.name === "gl.copyScreenshot"));
});

test("the platform namespace exposes steam", () => {
    assert.equal(kaplayNative.platform, "test");
    assert.equal(kaplayNative.version, "test");
    assert.equal(kaplayNative.steam.available(), true);
    assert.equal(kaplayNative.steam.playerName(), "Tester");
    assert.equal(kaplayNative.steam.steamId(), "76561198000000000");

    assert.equal(kaplayNative.steam.unlockAchievement("FIRST"), true);
    assert.equal(kaplayNative.steam.unlockAchievement("locked"), false);
    assert.equal(kaplayNative.steam.isAchievementUnlocked("unlocked"), true);
    assert.equal(kaplayNative.steam.storeStats(), true);

    assert.equal(kaplayNative.steam.setStatInt("deaths", 3), true);
    assert.equal(kaplayNative.steam.getStatInt("deaths"), 7);
    assert.equal(kaplayNative.steam.getStatInt("missing"), null);
    assert.equal(kaplayNative.steam.setStatFloat("time", 2.5), true);
    assert.equal(kaplayNative.steam.getStatFloat("time"), 2.5);

    assert.equal(kaplayNative.steam.writeCloud("save.json", "{}"), true);
    assert.equal(kaplayNative.steam.readCloud("save.json"), "cloud-data");
    assert.equal(kaplayNative.steam.readCloud("missing"), null);
    assert.equal(kaplayNative.steam.cloudExists("save.json"), true);
    assert.equal(kaplayNative.steam.deleteCloud("save.json"), true);

    kaplayNative.steam.runCallbacks();
    assert.ok(stub.calls.some((entry) => entry.name === "steam.runCallbacks"));
    assert.ok(stub.calls.some((entry) => entry.name === "steam.setStatInt" && entry.args[1] === 3));
});

test("url and search params follow the web api", () => {
    const parsed = new URL("/colyseus?x=1&y=a+b", "ws://127.0.0.1:2567");
    assert.equal(parsed.protocol, "ws:");
    assert.equal(parsed.hostname, "127.0.0.1");
    assert.equal(parsed.port, "2567");
    assert.equal(parsed.pathname, "/colyseus");
    assert.equal(parsed.search, "?x=1&y=a+b");
    assert.equal(parsed.href, "ws://127.0.0.1:2567/colyseus?x=1&y=a+b");
    assert.equal(parsed.searchParams.get("y"), "a b");
    assert.equal(parsed.toString(), parsed.href);

    const relative = new URL("../other", "wss://example.com:8443/a/b/c");
    assert.equal(relative.href, "wss://example.com:8443/a/other");
    const schemeRelative = new URL("//cdn.example.com/x", "https://example.com");
    assert.equal(schemeRelative.href, "https://cdn.example.com/x");
    const samePage = new URL("#frag", "https://example.com/a?b=1");
    assert.equal(samePage.href, "https://example.com/a?b=1#frag");

    assert.throws(() => new URL("not a url"), TypeError);

    const params = new URLSearchParams("?a=1&a=2&b=%20c");
    assert.equal(params.getAll("a").join(","), "1,2");
    assert.equal(params.get("b"), " c");
    assert.equal(params.has("a"), true);
    params.set("a", "9");
    params.append("d", "e f");
    assert.equal(params.toString(), "a=9&b=+c&d=e+f");
    params.delete("b");
    assert.equal(params.toString(), "a=9&d=e+f");
    assert.equal(new URLSearchParams({ deep: true }).get("deep"), "true");
    assert.equal(new URLSearchParams([["k", 2]]).get("k"), "2");
    assert.equal([...params.keys()].join(","), "a,d");
    assert.equal(params.size, 2);
});

test("matchMedia reports pointer capabilities", () => {
    assert.equal(window.matchMedia("(hover: none) and (pointer: coarse)").matches, false);
    assert.equal(window.matchMedia("(prefers-color-scheme: dark)").media, "(prefers-color-scheme: dark)");
});

test("location exposes the fields sdk clients parse", () => {
    assert.equal(location.protocol, "kaplay-native:");
    assert.equal(location.hostname, "game");
    assert.equal(location.port, "");
    assert.equal(location.searchParams.get("solo"), null);
});

test("crypto fills typed arrays and mints uuids", () => {
    const bytes = new Uint8Array(16);
    assert.equal(crypto.getRandomValues(bytes), bytes);
    assert.ok(bytes.some((value) => value !== 0));

    const uuid = crypto.randomUUID();
    assert.match(uuid, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    assert.notEqual(crypto.randomUUID(), uuid);

    const words = new Uint32Array(2);
    crypto.getRandomValues(words);
    assert.equal(words.length, 2);
});

test("dom elements support lookup, nesting, and input classes", () => {
    const banner = document.createElement("div");
    banner.id = "offline-banner";
    banner.textContent = "offline";
    Object.assign(banner.style, { position: "fixed", display: "none" });
    document.body.prepend(banner);

    assert.equal(document.getElementById("offline-banner"), banner);
    assert.equal(document.querySelector("#offline-banner"), banner);
    assert.equal(banner.parentElement, document.body);
    assert.equal(banner.style.display, "none");
    assert.equal(banner.style.cssText.includes("position: fixed"), true);

    const input = document.createElement("input");
    assert.ok(input instanceof HTMLInputElement);
    assert.ok(input instanceof HTMLElement);
    input.type = "password";
    input.setAttribute("autocomplete", "off");
    assert.equal(input.getAttribute("autocomplete"), "off");
    input.value = "hunter2";
    let seen = null;
    input.addEventListener("input", () => {
        seen = input.value;
    });
    input.dispatchEvent(new Event("input"));
    assert.equal(seen, "hunter2");
    assert.equal(document.getElementsByTagName("input").includes(input), false);

    const form = document.createElement("form");
    form.noValidate = true;
    form.addEventListener("submit", (event) => event.preventDefault());
    document.body.prepend(form);
    form.appendChild(input);
    assert.equal(input.parentElement, form);
    assert.equal(form.children[0], input);
    assert.ok(document.getElementsByTagName("input").includes(input));

    const event = new MouseEvent("mousemove", { clientX: 5, clientY: 6 });
    assert.equal(event.type, "mousemove");
    assert.equal(event.clientX, 5);
    document.body.dispatchEvent(event);

    input.remove();
    banner.remove();
    assert.equal(document.getElementById("offline-banner"), null);
    assert.equal(banner.parentElement, null);
    assert.equal(form.children.length, 0);
});
