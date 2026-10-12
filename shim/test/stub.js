export function installStub() {
    const calls = [];
    const logs = [];
    const files = new Map([["font.ttf", new Uint8Array([1, 2, 3, 4])]]);
    const images = new Map();
    const canvases = new Map();
    const state = {
        time: 0,
        storage: new Map(),
        nextId: 1,
        nextCanvas: 1,
    };

    function record(name, args) {
        calls.push({ name, args });
    }

    function nextId() {
        return state.nextId++;
    }

    globalThis.__kapNative = {
        env: {
            hostVersion: "test",
            platform: "test",
            gameName: "stub",
            networkAllowed: false,
            now: () => state.time,
            log: (level, message) => logs.push({ level, message }),
            quit: () => record("env.quit", []),
        },
        window: {
            size: () => [960, 540],
            setTitle: (title) => record("window.setTitle", [title]),
            setFullscreen: (value) => record("window.setFullscreen", [value]),
            isFullscreen: () => false,
            setCursor: (value) => record("window.setCursor", [value]),
            setCursorLocked: (value) => record("window.setCursorLocked", [value]),
            isCursorLocked: () => false,
        },
        fs: {
            size: (path) => {
                const data = files.get(path);
                return data ? data.length : -1;
            },
            exists: (path) => files.has(path),
            read: (path, out) => {
                const data = files.get(path);
                if (!data) return false;
                out.set(data.subarray(0, out.length));
                return true;
            },
        },
        storage: {
            load: () => JSON.stringify(Object.fromEntries(state.storage)),
            save: (text) => {
                state.storage = new Map(Object.entries(JSON.parse(text)));
                record("storage.save", [text]);
            },
        },
        image: {
            load: (url) => {
                if (files.has(url)) {
                    const id = nextId();
                    images.set(id, { width: 4, height: 3 });
                    return id;
                }
                return -1;
            },
            decode: (bytes) => {
                const id = nextId();
                images.set(id, { width: bytes.length, height: 1 });
                return id;
            },
            width: (id) => images.get(id)?.width ?? -1,
            height: (id) => images.get(id)?.height ?? -1,
        },
        font: {
            register: (family, bytes) => {
                record("font.register", [family, bytes.length]);
                return true;
            },
            has: (family) => family === "monospace" || family === "MyFont",
        },
        canvas: {
            create: (width, height) => {
                const id = nextId();
                canvases.set(id, { width, height });
                record("canvas.create", [id, width, height]);
                return id;
            },
            resize: (id, width, height) => {
                const canvas = canvases.get(id);
                if (canvas) {
                    canvas.width = width;
                    canvas.height = height;
                }
                record("canvas.resize", [id, width, height]);
            },
            destroy: (id) => record("canvas.destroy", [id]),
            createContext: (id) => nextId(),
            setFillStyle: (context, value) => record("canvas.setFillStyle", [context, value]),
            setStrokeStyle: (context, value) => record("canvas.setStrokeStyle", [context, value]),
            setFont: (context, value) => record("canvas.setFont", [context, value]),
            setLineWidth: (context, value) => record("canvas.setLineWidth", [context, value]),
            setLineJoin: (context, value) => record("canvas.setLineJoin", [context, value]),
            setTextBaseline: (context, value) => record("canvas.setTextBaseline", [context, value]),
            setTextAlign: (context, value) => record("canvas.setTextAlign", [context, value]),
            clearRect: (...args) => record("canvas.clearRect", args),
            fillText: (...args) => record("canvas.fillText", args),
            strokeText: (...args) => record("canvas.strokeText", args),
            measureText: (context, text) => {
                record("canvas.measureText", [context, text]);
                return [text.length * 7, 10, 2];
            },
            drawImage: (...args) => record("canvas.drawImage", args),
            putImageData: (...args) => record("canvas.putImageData", args),
            getImageData: (context, x, y, width, height, out) => {
                record("canvas.getImageData", [context, x, y, width, height]);
                for (let i = 0; i < out.length; i++) out[i] = i % 251;
            },
            pngLength: () => 4,
            copyPng: (canvas, out) => {
                out.set([1, 2, 3, 4]);
                return true;
            },
        },
        gl: {},
        steam: {
            available: () => true,
            playerName: () => "Tester",
            steamId: () => "76561198000000000",
            unlockAchievement: (name) => {
                record("steam.unlockAchievement", [name]);
                return name !== "locked";
            },
            isAchievementUnlocked: (name) => {
                record("steam.isAchievementUnlocked", [name]);
                return name === "unlocked";
            },
            storeStats: () => {
                record("steam.storeStats", []);
                return true;
            },
            setStatInt: (name, value) => {
                record("steam.setStatInt", [name, value]);
                return true;
            },
            getStatInt: (name) => {
                record("steam.getStatInt", [name]);
                return name === "missing" ? null : 7;
            },
            setStatFloat: (name, value) => {
                record("steam.setStatFloat", [name, value]);
                return true;
            },
            getStatFloat: (name) => {
                record("steam.getStatFloat", [name]);
                return name === "missing" ? null : 2.5;
            },
            writeCloud: (name, text) => {
                record("steam.writeCloud", [name, text]);
                return true;
            },
            readCloud: (name) => {
                record("steam.readCloud", [name]);
                return name === "missing" ? null : "cloud-data";
            },
            cloudExists: (name) => {
                record("steam.cloudExists", [name]);
                return name !== "missing";
            },
            deleteCloud: (name) => {
                record("steam.deleteCloud", [name]);
                return true;
            },
            runCallbacks: () => record("steam.runCallbacks", []),
        },
    };

    const glCalls = [
        "viewport",
        "clearColor",
        "clear",
        "enable",
        "disable",
        "blendFuncSeparate",
        "stencilFunc",
        "stencilOp",
        "pixelStorei",
        "getParameter",
        "activeTexture",
        "createTexture",
        "deleteTexture",
        "bindTexture",
        "texParameteri",
        "texImage2D",
        "texSubImage2D",
        "texImage2DImage",
        "texSubImage2DImage",
        "texImage2DCanvas",
        "texSubImage2DCanvas",
        "createBuffer",
        "deleteBuffer",
        "bindBuffer",
        "bufferDataSize",
        "bufferData",
        "bufferSubData",
        "enableVertexAttribArray",
        "disableVertexAttribArray",
        "vertexAttribPointer",
        "drawArrays",
        "drawElements",
        "createShader",
        "deleteShader",
        "shaderSource",
        "compileShader",
        "getShaderInfoLog",
        "createProgram",
        "deleteProgram",
        "attachShader",
        "bindAttribLocation",
        "linkProgram",
        "getLinkStatus",
        "getProgramInfoLog",
        "useProgram",
        "getAttribLocation",
        "getUniformLocation",
        "uniform1f",
        "uniform1i",
        "uniform2f",
        "uniform3f",
        "uniform1fv",
        "uniform2fv",
        "uniform3fv",
        "uniformMatrix4fv",
        "createFramebuffer",
        "deleteFramebuffer",
        "bindFramebuffer",
        "createRenderbuffer",
        "deleteRenderbuffer",
        "bindRenderbuffer",
        "renderbufferStorage",
        "framebufferTexture2D",
        "framebufferRenderbuffer",
        "checkFramebufferStatus",
        "readPixels",
    ];
    for (const name of glCalls) {
        globalThis.__kapNative.gl[name] = (...args) => {
            record(`gl.${name}`, args);
            if (name === "createTexture" || name === "createBuffer") return nextId();
            if (name === "createShader" || name === "createProgram") return nextId();
            if (name === "createFramebuffer" || name === "createRenderbuffer") return nextId();
            if (name === "getShaderInfoLog" || name === "getProgramInfoLog") return "";
            if (name === "getLinkStatus") return true;
            if (name === "getUniformLocation") {
                return String(args[1]).startsWith("missing") ? -1 : nextId();
            }
            if (name === "getAttribLocation") return 0;
            if (name === "getParameter") return 16;
            if (name === "checkFramebufferStatus") return 0x8cd5;
            return undefined;
        };
    }
    globalThis.__kapNative.gl.drawingBufferWidth = () => 320;
    globalThis.__kapNative.gl.drawingBufferHeight = () => 240;
    globalThis.__kapNative.gl.resizeDrawingBuffer = (...args) => record("gl.resizeDrawingBuffer", args);
    globalThis.__kapNative.gl.screenshotLength = () => {
        record("gl.screenshotLength", []);
        return 4;
    };
    globalThis.__kapNative.gl.copyScreenshot = (out) => {
        record("gl.copyScreenshot", [out.length]);
        out.set([9, 8, 7, 6]);
        return true;
    };
    globalThis.__kapNative.glCalls = () => globalThis.__kapNative.gl;

    return { calls, logs, state, files, canvases };
}
