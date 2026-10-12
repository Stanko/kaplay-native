(function () {
    "use strict";

    const native = __kapNative;
    const global = globalThis;

    const screen = {
        width: 960,
        height: 540,
        dpr: 1,
    };

    try {
        const size = native.window.size();
        screen.width = size[0];
        screen.height = size[1];
    } catch (error) {
        reportError(error);
    }

    function reportError(error) {
        const message = error && error.stack ? error.stack : String(error);
        native.env.log("error", message);
    }

    function bytesView(value) {
        if (value instanceof Uint8Array) return value;
        if (value instanceof Uint8ClampedArray) {
            return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
        }
        if (ArrayBuffer.isView(value)) {
            return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
        }
        if (value instanceof ArrayBuffer) return new Uint8Array(value);
        return new Uint8Array(0);
    }

    function float32(value) {
        if (value instanceof Float32Array) return value;
        if (value && typeof value.length === "number") return new Float32Array(value);
        return new Float32Array(0);
    }

    // ---------------------------------------------------------------- listeners

    function makeListeners() {
        const map = new Map();
        return {
            add(type, fn) {
                if (typeof fn !== "function") return;
                const list = map.get(type);
                if (list) list.push(fn);
                else map.set(type, [fn]);
            },
            remove(type, fn) {
                const list = map.get(type);
                if (!list) return;
                const index = list.indexOf(fn);
                if (index >= 0) list.splice(index, 1);
            },
            emit(event) {
                const list = map.get(event.type);
                if (list) {
                    for (const fn of list.slice()) {
                        try {
                            fn.call(null, event);
                        } catch (error) {
                            reportError(error);
                        }
                    }
                }
            },
        };
    }

    const canvasListeners = makeListeners();
    const documentListeners = makeListeners();
    const windowListeners = makeListeners();

    // ------------------------------------------------------------------ timers

    const timers = new Map();
    const frames = [];
    let nextTimer = 1;
    let nextFrame = 1;

    function setTimeoutFn(fn, delay) {
        const args = Array.prototype.slice.call(arguments, 2);
        const id = nextTimer++;
        timers.set(id, { fn, args, due: native.env.now() + Number(delay || 0), interval: 0 });
        return id;
    }

    function setIntervalFn(fn, delay) {
        const args = Array.prototype.slice.call(arguments, 2);
        const id = nextTimer++;
        const interval = Math.max(1, Number(delay || 0));
        timers.set(id, { fn, args, due: native.env.now() + interval, interval });
        return id;
    }

    function clearTimer(id) {
        timers.delete(id);
    }

    function requestAnimationFrame(callback) {
        const id = nextFrame++;
        frames.push({ id, callback });
        return id;
    }

    function cancelAnimationFrame(id) {
        const index = frames.findIndex((entry) => entry.id === id);
        if (index >= 0) frames.splice(index, 1);
    }

    function tickTimers(now) {
        const due = [];
        for (const entry of timers) {
            if (entry[1].due <= now) due.push(entry);
        }
        due.sort((a, b) => a[1].due - b[1].due || a[0] - b[0]);
        for (const [id, timer] of due) {
            if (!timers.has(id)) continue;
            if (timer.interval > 0) timer.due = now + timer.interval;
            else timers.delete(id);
            try {
                timer.fn.apply(null, timer.args);
            } catch (error) {
                reportError(error);
            }
        }
    }

    // ------------------------------------------------------------------ base64

    const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    function atob(value) {
        let input = String(value).replace(/[^A-Za-z0-9+/=]/g, "");
        let output = "";
        for (let i = 0; i < input.length; i += 4) {
            const a = BASE64.indexOf(input[i]);
            const b = BASE64.indexOf(input[i + 1]);
            const c = BASE64.indexOf(input[i + 2]);
            const d = BASE64.indexOf(input[i + 3]);
            output += String.fromCharCode((a << 2) | (b >> 4));
            if (c >= 0) output += String.fromCharCode(((b & 15) << 4) | (c >> 2));
            if (d >= 0) output += String.fromCharCode(((c & 3) << 6) | d);
        }
        return output;
    }

    function btoa(value) {
        const input = String(value);
        let output = "";
        for (let i = 0; i < input.length; i += 3) {
            const a = input.charCodeAt(i) & 0xff;
            const b = i + 1 < input.length ? input.charCodeAt(i + 1) & 0xff : NaN;
            const c = i + 2 < input.length ? input.charCodeAt(i + 2) & 0xff : NaN;
            output += BASE64[a >> 2];
            output += BASE64[((a & 3) << 4) | (isNaN(b) ? 0 : b >> 4)];
            output += isNaN(b) ? "=" : BASE64[((b & 15) << 2) | (isNaN(c) ? 0 : c >> 6)];
            output += isNaN(c) ? "=" : BASE64[c & 63];
        }
        return output;
    }

    function decodeDataURL(url) {
        const match = /^data:([^;,]*)?(;base64)?,(.*)$/s.exec(String(url));
        if (!match) return null;
        const payload = match[3];
        if (match[2]) {
            const binary = atob(payload);
            const bytes = new Uint8Array(binary.length);
            for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
            return bytes;
        }
        return null;
    }

    // ------------------------------------------------------------------- style

    function makeStyle() {
        const style = {};
        Object.defineProperty(style, "cssText", {
            get() {
                return Object.entries(style)
                    .filter(([key]) => key.startsWith("__") ? false : typeof style[key] !== "function")
                    .map(([key, value]) => `${key}: ${value}`)
                    .join("; ");
            },
            set(value) {
                for (const part of String(value).split(";")) {
                    const index = part.indexOf(":");
                    if (index < 0) continue;
                    const key = part.slice(0, index).trim();
                    const entry = part.slice(index + 1).trim();
                    if (key) style[key] = entry;
                }
            },
        });
        return style;
    }

    function cssColor(value) {
        if (typeof value === "string") return value;
        if (value && typeof value.toHex === "function") return value.toHex();
        if (value && typeof value.r === "number") {
            return `rgb(${value.r}, ${value.g}, ${value.b})`;
        }
        return String(value);
    }

    // ------------------------------------------------------------------ canvas

    function canvasSize(canvas) {
        const width = canvas.offsetWidth;
        const height = canvas.offsetHeight;
        if (!width || !height) return { x: 0, y: 0, width: 0, height: 0, scale: 1 };
        const scale = Math.min(screen.width / width, screen.height / height);
        const targetWidth = width * scale;
        const targetHeight = height * scale;
        return {
            x: (screen.width - targetWidth) / 2,
            y: (screen.height - targetHeight) / 2,
            width: targetWidth,
            height: targetHeight,
            scale,
        };
    }

    function px(value) {
        if (typeof value === "string" && value.trim().endsWith("px")) {
            return parseFloat(value) || 0;
        }
        return 0;
    }

    class CanvasRenderingContext2D {
        constructor(canvas) {
            this.canvas = canvas;
            this.__id = native.canvas.createContext(canvas.__id);
            this.fillStyle = "#000000";
            this.strokeStyle = "#000000";
            this.font = "10px sans-serif";
            this.lineWidth = 1;
            this.lineJoin = "miter";
            this.textBaseline = "alphabetic";
            this.textAlign = "start";
            this.globalAlpha = 1;
            this.globalCompositeOperation = "source-over";
            this.imageSmoothingEnabled = true;
        }

        set fillStyle(value) {
            this.__fillStyle = value;
            native.canvas.setFillStyle(this.__id, cssColor(value));
        }

        get fillStyle() {
            return this.__fillStyle;
        }

        set strokeStyle(value) {
            this.__strokeStyle = value;
            native.canvas.setStrokeStyle(this.__id, cssColor(value));
        }

        get strokeStyle() {
            return this.__strokeStyle;
        }

        set font(value) {
            this.__font = value;
            native.canvas.setFont(this.__id, String(value));
        }

        get font() {
            return this.__font;
        }

        set lineWidth(value) {
            this.__lineWidth = Number(value) || 0;
            native.canvas.setLineWidth(this.__id, this.__lineWidth);
        }

        get lineWidth() {
            return this.__lineWidth;
        }

        set lineJoin(value) {
            this.__lineJoin = value;
            native.canvas.setLineJoin(this.__id, String(value));
        }

        get lineJoin() {
            return this.__lineJoin;
        }

        set textBaseline(value) {
            this.__textBaseline = value;
            native.canvas.setTextBaseline(this.__id, String(value));
        }

        get textBaseline() {
            return this.__textBaseline;
        }

        set textAlign(value) {
            this.__textAlign = value;
            native.canvas.setTextAlign(this.__id, String(value));
        }

        get textAlign() {
            return this.__textAlign;
        }

        clearRect(x, y, width, height) {
            native.canvas.clearRect(this.__id, Number(x), Number(y), Number(width), Number(height));
        }

        fillText(text, x, y) {
            native.canvas.fillText(this.__id, String(text), Number(x), Number(y));
        }

        strokeText(text, x, y) {
            native.canvas.strokeText(this.__id, String(text), Number(x), Number(y));
        }

        measureText(text) {
            const metrics = native.canvas.measureText(this.__id, String(text));
            return {
                width: metrics[0],
                actualBoundingBoxLeft: 0,
                actualBoundingBoxRight: metrics[0],
                actualBoundingBoxAscent: metrics[1],
                actualBoundingBoxDescent: metrics[2],
            };
        }

        drawImage(source, ...values) {
            let kind = 0;
            let id = -1;
            let sourceWidth = 0;
            let sourceHeight = 0;
            if (source instanceof HTMLCanvasElement) {
                kind = 1;
                id = source.__id;
                sourceWidth = source.width;
                sourceHeight = source.height;
            } else if (source instanceof ImageData) {
                this.putImageData(source, Number(values[0]) || 0, Number(values[1]) || 0);
                return;
            } else if (source instanceof Image) {
                kind = 0;
                id = source.__id;
                sourceWidth = source.width;
                sourceHeight = source.height;
            }
            if (id < 0) return;
            if (values.length >= 8) {
                native.canvas.drawImage(
                    this.__id,
                    kind,
                    id,
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    values[4],
                    values[5],
                    values[6],
                    values[7],
                );
            } else if (values.length === 4) {
                native.canvas.drawImage(
                    this.__id,
                    kind,
                    id,
                    0,
                    0,
                    sourceWidth,
                    sourceHeight,
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                );
            } else {
                native.canvas.drawImage(
                    this.__id,
                    kind,
                    id,
                    0,
                    0,
                    sourceWidth,
                    sourceHeight,
                    values[0],
                    values[1],
                    sourceWidth,
                    sourceHeight,
                );
            }
        }

        getImageData(x, y, width, height) {
            const w = Math.max(0, Math.floor(Number(width) || 0));
            const h = Math.max(0, Math.floor(Number(height) || 0));
            const bytes = new Uint8Array(w * h * 4);
            native.canvas.getImageData(this.__id, Math.floor(x), Math.floor(y), w, h, bytes);
            return new ImageData(new Uint8ClampedArray(bytes.buffer), w, h);
        }

        putImageData(image, x, y) {
            native.canvas.putImageData(
                this.__id,
                Math.floor(Number(x) || 0),
                Math.floor(Number(y) || 0),
                image.width,
                image.height,
                bytesView(image.data),
            );
        }

        save() {}

        restore() {}

        translate() {}

        scale() {}

        rotate() {}

        setTransform() {}

        resetTransform() {}

        beginPath() {}

        closePath() {}

        moveTo() {}

        lineTo() {}

        stroke() {}

        fill() {}

        fillRect() {}

        strokeRect() {}

        arc() {}
    }

    class HTMLCanvasElement {
        constructor() {
            this.__id = native.canvas.create(1, 1);
            this.__width = 1;
            this.__height = 1;
            this.__context2d = null;
            this.__gl = null;
            this.__parent = null;
            this.style = makeStyle();
            this.tabIndex = 0;
        }

        get width() {
            return this.__width;
        }

        set width(value) {
            this.__resize(Math.max(1, Math.floor(Number(value) || 1)), this.__height);
        }

        get height() {
            return this.__height;
        }

        set height(value) {
            this.__resize(this.__width, Math.max(1, Math.floor(Number(value) || 1)));
        }

        __resize(width, height) {
            this.__width = width;
            this.__height = height;
            native.canvas.resize(this.__id, width, height);
            if (this.__gl) native.gl.resizeDrawingBuffer(width, height);
        }

        get offsetWidth() {
            return px(this.style.width) || screen.width;
        }

        get offsetHeight() {
            return px(this.style.height) || screen.height;
        }

        get parentElement() {
            return this.__parent;
        }

        getBoundingClientRect() {
            const rect = canvasSize(this);
            return {
                x: rect.x,
                y: rect.y,
                left: rect.x,
                top: rect.y,
                width: rect.width,
                height: rect.height,
                right: rect.x + rect.width,
                bottom: rect.y + rect.height,
            };
        }

        appendChild(child) {
            if (child instanceof HTMLCanvasElement) {
                child.__parent = this;
                return child;
            }
            return child;
        }

        getContext(type) {
            if (type === "2d") {
                if (!this.__context2d) this.__context2d = new CanvasRenderingContext2D(this);
                return this.__context2d;
            }
            if (type === "webgl" || type === "experimental-webgl") {
                if (!this.__gl) {
                    this.__gl = new WebGLRenderingContext(this);
                    native.gl.resizeDrawingBuffer(this.__width, this.__height);
                }
                return this.__gl;
            }
            return null;
        }

        addEventListener(type, fn) {
            canvasListeners.add(type, fn);
        }

        removeEventListener(type, fn) {
            canvasListeners.remove(type, fn);
        }

        dispatchEvent(event) {
            canvasListeners.emit(event);
            return true;
        }

        focus() {
            document.activeElement = this;
        }

        blur() {
            if (document.activeElement === this) document.activeElement = null;
        }

        toDataURL() {
            let length = -1;
            let copy = null;
            if (this.__gl) {
                length = native.gl.screenshotLength();
                copy = (view) => native.gl.copyScreenshot(view);
            } else {
                length = native.canvas.pngLength(this.__id);
                copy = (view) => native.canvas.copyPng(this.__id, view);
            }
            if (length < 0) return "data:,";
            const bytes = new Uint8Array(length);
            copy(bytes);
            let binary = "";
            for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
            return "data:image/png;base64," + btoa(binary);
        }

        toBlob(callback) {
            if (typeof callback !== "function") return;
            const url = this.toDataURL();
            const bytes = decodeDataURL(url) || new Uint8Array(0);
            callback(new Blob([bytes], { type: "image/png" }));
        }
    }

    // ---------------------------------------------------------------------- gl

    const GL = {
        DEPTH_BUFFER_BIT: 0x00000100,
        STENCIL_BUFFER_BIT: 0x00000400,
        COLOR_BUFFER_BIT: 0x00004000,
        POINTS: 0x0000,
        LINES: 0x0001,
        LINE_LOOP: 0x0002,
        LINE_STRIP: 0x0003,
        TRIANGLES: 0x0004,
        TRIANGLE_STRIP: 0x0005,
        TRIANGLE_FAN: 0x0006,
        ZERO: 0,
        ONE: 1,
        SRC_COLOR: 0x0300,
        ONE_MINUS_SRC_COLOR: 0x0301,
        SRC_ALPHA: 0x0302,
        ONE_MINUS_SRC_ALPHA: 0x0303,
        DST_ALPHA: 0x0304,
        ONE_MINUS_DST_ALPHA: 0x0305,
        DST_COLOR: 0x0306,
        ONE_MINUS_DST_COLOR: 0x0307,
        SRC_ALPHA_SATURATE: 0x0308,
        FUNC_ADD: 0x8006,
        BLEND_EQUATION: 0x8009,
        BLEND_EQUATION_RGB: 0x8009,
        BLEND_EQUATION_ALPHA: 0x883D,
        BLEND_DST_RGB: 0x80C8,
        BLEND_SRC_RGB: 0x80C9,
        BLEND_DST_ALPHA: 0x80CA,
        BLEND_SRC_ALPHA: 0x80CB,
        BLEND_COLOR: 0x8005,
        ARRAY_BUFFER: 0x8892,
        ELEMENT_ARRAY_BUFFER: 0x8893,
        ARRAY_BUFFER_BINDING: 0x8894,
        ELEMENT_ARRAY_BUFFER_BINDING: 0x8895,
        STREAM_DRAW: 0x88E0,
        STATIC_DRAW: 0x88E4,
        DYNAMIC_DRAW: 0x88E8,
        BUFFER_SIZE: 0x8764,
        BUFFER_USAGE: 0x8765,
        CURRENT_VERTEX_ATTRIB: 0x8626,
        FRONT: 0x0404,
        BACK: 0x0405,
        CULL_FACE: 0x0B44,
        BLEND: 0x0BE2,
        DITHER: 0x0BD0,
        STENCIL_TEST: 0x0B90,
        DEPTH_TEST: 0x0B71,
        SCISSOR_TEST: 0x0C11,
        POLYGON_OFFSET_FILL: 0x8037,
        SAMPLE_ALPHA_TO_COVERAGE: 0x809E,
        SAMPLE_COVERAGE: 0x80A0,
        NO_ERROR: 0,
        INVALID_ENUM: 0x0500,
        INVALID_VALUE: 0x0501,
        INVALID_OPERATION: 0x0502,
        OUT_OF_MEMORY: 0x0505,
        CONTEXT_LOST_WEBGL: 0x9242,
        CW: 0x0900,
        CCW: 0x0901,
        LINE_WIDTH: 0x0B21,
        ALIASED_LINE_WIDTH_RANGE: 0x846E,
        CULL_FACE_MODE: 0x0B45,
        FRONT_FACE: 0x0B46,
        DEPTH_RANGE: 0x0B70,
        DEPTH_WRITEMASK: 0x0B72,
        DEPTH_CLEAR_VALUE: 0x0B73,
        DEPTH_FUNC: 0x0B74,
        STENCIL_CLEAR_VALUE: 0x0B91,
        STENCIL_FUNC: 0x0B92,
        STENCIL_FAIL: 0x0B94,
        STENCIL_PASS_DEPTH_FAIL: 0x0B95,
        STENCIL_PASS_DEPTH_PASS: 0x0B96,
        STENCIL_REF: 0x0B97,
        STENCIL_VALUE_MASK: 0x0B93,
        STENCIL_WRITEMASK: 0x0B98,
        STENCIL_BACK_FUNC: 0x8800,
        STENCIL_BACK_FAIL: 0x8801,
        STENCIL_BACK_PASS_DEPTH_FAIL: 0x8802,
        STENCIL_BACK_PASS_DEPTH_PASS: 0x8803,
        STENCIL_BACK_REF: 0x8CA3,
        STENCIL_BACK_VALUE_MASK: 0x8CA4,
        STENCIL_BACK_WRITEMASK: 0x8CA5,
        STENCIL_BITS: 0x0D57,
        VIEWPORT: 0x0BA2,
        SCISSOR_BOX: 0x0C10,
        COLOR_CLEAR_VALUE: 0x0C22,
        COLOR_WRITEMASK: 0x0C23,
        UNPACK_ALIGNMENT: 0x0CF5,
        PACK_ALIGNMENT: 0x0D05,
        MAX_TEXTURE_SIZE: 0x0D33,
        MAX_VIEWPORT_DIMS: 0x0D3A,
        SUBPIXEL_BITS: 0x0D50,
        RED_BITS: 0x0D52,
        GREEN_BITS: 0x0D53,
        BLUE_BITS: 0x0D54,
        ALPHA_BITS: 0x0D55,
        DEPTH_BITS: 0x0D56,
        TEXTURE_2D: 0x0DE1,
        TEXTURE: 0x1702,
        TEXTURE0: 0x84C0,
        TEXTURE1: 0x84C1,
        TEXTURE2: 0x84C2,
        TEXTURE3: 0x84C3,
        TEXTURE4: 0x84C4,
        TEXTURE5: 0x84C5,
        TEXTURE6: 0x84C6,
        TEXTURE7: 0x84C7,
        TEXTURE8: 0x84C8,
        TEXTURE9: 0x84C9,
        TEXTURE10: 0x84CA,
        TEXTURE11: 0x84CB,
        TEXTURE12: 0x84CC,
        TEXTURE13: 0x84CD,
        TEXTURE14: 0x84CE,
        TEXTURE15: 0x84CF,
        TEXTURE16: 0x84D0,
        TEXTURE17: 0x84D1,
        TEXTURE18: 0x84D2,
        TEXTURE19: 0x84D3,
        TEXTURE20: 0x84D4,
        TEXTURE21: 0x84D5,
        TEXTURE22: 0x84D6,
        TEXTURE23: 0x84D7,
        TEXTURE24: 0x84D8,
        TEXTURE25: 0x84D9,
        TEXTURE26: 0x84DA,
        TEXTURE27: 0x84DB,
        TEXTURE28: 0x84DC,
        TEXTURE29: 0x84DD,
        TEXTURE30: 0x84DE,
        TEXTURE31: 0x84DF,
        TEXTURE_CUBE_MAP: 0x8513,
        TEXTURE_BINDING_CUBE_MAP: 0x8514,
        TEXTURE_MAG_FILTER: 0x2800,        TEXTURE_MIN_FILTER: 0x2801,
        TEXTURE_WRAP_S: 0x2802,
        TEXTURE_WRAP_T: 0x2803,
        REPEAT: 0x2901,
        CLAMP_TO_EDGE: 0x812F,
        MIRRORED_REPEAT: 0x8370,
        NEAREST: 0x2600,
        LINEAR: 0x2601,
        NEAREST_MIPMAP_NEAREST: 0x2700,
        LINEAR_MIPMAP_NEAREST: 0x2701,
        NEAREST_MIPMAP_LINEAR: 0x2702,
        LINEAR_MIPMAP_LINEAR: 0x2703,
        BYTE: 0x1400,
        UNSIGNED_BYTE: 0x1401,
        SHORT: 0x1402,
        UNSIGNED_SHORT: 0x1403,
        INT: 0x1404,
        UNSIGNED_INT: 0x1405,
        FLOAT: 0x1406,
        UNSIGNED_SHORT_4_4_4_4: 0x8033,
        UNSIGNED_SHORT_5_5_5_1: 0x8034,
        UNSIGNED_SHORT_5_6_5: 0x8363,
        ALPHA: 0x1906,
        RGB: 0x1907,
        RGBA: 0x1908,
        LUMINANCE: 0x1909,
        LUMINANCE_ALPHA: 0x190A,
        FRAGMENT_SHADER: 0x8B30,
        VERTEX_SHADER: 0x8B31,
        MAX_VERTEX_ATTRIBS: 0x8869,
        MAX_VERTEX_UNIFORM_VECTORS: 0x8DFB,
        MAX_VARYING_VECTORS: 0x8DFC,
        MAX_COMBINED_TEXTURE_IMAGE_UNITS: 0x8B4D,
        MAX_VERTEX_TEXTURE_IMAGE_UNITS: 0x8B4C,
        MAX_TEXTURE_IMAGE_UNITS: 0x8872,
        MAX_FRAGMENT_UNIFORM_VECTORS: 0x8DFD,
        SHADER_TYPE: 0x8B4F,
        DELETE_STATUS: 0x8B80,
        COMPILE_STATUS: 0x8B81,
        LINK_STATUS: 0x8B82,
        VALIDATE_STATUS: 0x8B83,
        ATTACHED_SHADERS: 0x8B85,
        ACTIVE_UNIFORMS: 0x8B86,
        ACTIVE_ATTRIBUTES: 0x8B89,
        SHADING_LANGUAGE_VERSION: 0x8B8C,
        CURRENT_PROGRAM: 0x8B8D,
        FRAMEBUFFER: 0x8D40,
        RENDERBUFFER: 0x8D41,
        RGBA4: 0x8056,
        RGB5_A1: 0x8057,
        RGB565: 0x8D62,
        DEPTH_COMPONENT16: 0x81A5,
        STENCIL_INDEX8: 0x8D48,
        DEPTH_STENCIL: 0x84F9,
        DEPTH_STENCIL_ATTACHMENT: 0x821A,
        COLOR_ATTACHMENT0: 0x8CE0,
        DEPTH_ATTACHMENT: 0x8D00,
        STENCIL_ATTACHMENT: 0x8D20,
        FRAMEBUFFER_COMPLETE: 0x8CD5,
        FRAMEBUFFER_INCOMPLETE_ATTACHMENT: 0x8CD6,
        FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT: 0x8CD7,
        FRAMEBUFFER_INCOMPLETE_DIMENSIONS: 0x8CD9,
        FRAMEBUFFER_UNSUPPORTED: 0x8CDD,
        FRAMEBUFFER_BINDING: 0x8CA6,
        RENDERBUFFER_BINDING: 0x8CA7,
        MAX_RENDERBUFFER_SIZE: 0x84E8,
        INVALID_FRAMEBUFFER_OPERATION: 0x0506,
        VERSION: 0x1F02,
        VENDOR: 0x1F00,
        RENDERER: 0x1F01,
        UNPACK_FLIP_Y_WEBGL: 0x9240,
        UNPACK_PREMULTIPLY_ALPHA_WEBGL: 0x9241,
        UNPACK_COLORSPACE_CONVERSION_WEBGL: 0x9243,
        BROWSER_DEFAULT_WEBGL: 0x9244,
        NEVER: 0x0200,
        LESS: 0x0201,
        EQUAL: 0x0202,
        LEQUAL: 0x0203,
        GREATER: 0x0204,
        NOTEQUAL: 0x0205,
        GEQUAL: 0x0206,
        ALWAYS: 0x0207,
        KEEP: 0x1E00,
        REPLACE: 0x1E01,
        INCR: 0x1E02,
        DECR: 0x1E03,
        INVERT: 0x150A,
        INCR_WRAP: 0x8507,
        DECR_WRAP: 0x8508,
    };

    class WebGLRenderingContext {
        constructor(canvas) {
            this.canvas = canvas;
        }

        get drawingBufferWidth() {
            return native.gl.drawingBufferWidth();
        }

        get drawingBufferHeight() {
            return native.gl.drawingBufferHeight();
        }

        viewport(x, y, width, height) {
            native.gl.viewport(x, y, width, height);
        }

        clearColor(red, green, blue, alpha) {
            native.gl.clearColor(red, green, blue, alpha);
        }

        clear(mask) {
            native.gl.clear(mask);
        }

        enable(capability) {
            native.gl.enable(capability);
        }

        disable(capability) {
            native.gl.disable(capability);
        }

        blendFuncSeparate(srgb, drgb, salpha, dalpha) {
            native.gl.blendFuncSeparate(srgb, drgb, salpha, dalpha);
        }

        stencilFunc(func, reference, mask) {
            native.gl.stencilFunc(func, reference, mask);
        }

        stencilOp(fail, zfail, zpass) {
            native.gl.stencilOp(fail, zfail, zpass);
        }

        pixelStorei(parameter, value) {
            native.gl.pixelStorei(parameter, value ? 1 : 0);
        }

        getParameter(parameter) {
            return native.gl.getParameter(parameter);
        }

        activeTexture(unit) {
            native.gl.activeTexture(unit);
        }

        createTexture() {
            const id = native.gl.createTexture();
            return id < 0 ? null : id;
        }

        deleteTexture(texture) {
            if (texture != null) native.gl.deleteTexture(texture);
        }

        bindTexture(target, texture) {
            native.gl.bindTexture(target, texture == null ? -1 : texture);
        }

        texParameteri(target, parameter, value) {
            native.gl.texParameteri(target, parameter, value);
        }

        texImage2D(target, level, internalformat, ...rest) {
            if (rest.length >= 6) {
                const width = rest[0];
                const height = rest[1];
                const format = rest[3];
                const type = rest[4];
                const pixels = rest[5];
                native.gl.texImage2D(
                    target,
                    internalformat,
                    width,
                    height,
                    format,
                    type,
                    pixels == null ? null : bytesView(pixels),
                );
                return;
            }
            const format = rest[0];
            const type = rest[1];
            const source = rest[2];
            if (source instanceof HTMLCanvasElement) {
                native.gl.texImage2DCanvas(
                    target,
                    internalformat,
                    format,
                    type,
                    source.__id,
                );
            } else if (source instanceof Image) {
                native.gl.texImage2DImage(target, internalformat, format, type, source.__id);
            } else if (source instanceof ImageData) {
                native.gl.texImage2D(
                    target,
                    internalformat,
                    source.width,
                    source.height,
                    format,
                    type,
                    bytesView(source.data),
                );
            } else if (source == null) {
                native.gl.texImage2D(target, internalformat, 0, 0, format, type, null);
            }
        }

        texSubImage2D(target, level, xoffset, yoffset, format, type, source) {
            if (source instanceof HTMLCanvasElement) {
                native.gl.texSubImage2DCanvas(
                    target,
                    xoffset,
                    yoffset,
                    format,
                    type,
                    source.__id,
                );
            } else if (source instanceof Image) {
                native.gl.texSubImage2DImage(
                    target,
                    xoffset,
                    yoffset,
                    format,
                    type,
                    source.__id,
                );
            } else if (source instanceof ImageData) {
                native.gl.texSubImage2D(
                    target,
                    xoffset,
                    yoffset,
                    source.width,
                    format,
                    type,
                    bytesView(source.data),
                );
            }
        }

        createBuffer() {
            const id = native.gl.createBuffer();
            return id < 0 ? null : id;
        }

        deleteBuffer(buffer) {
            if (buffer != null) native.gl.deleteBuffer(buffer);
        }

        bindBuffer(target, buffer) {
            native.gl.bindBuffer(target, buffer == null ? -1 : buffer);
        }

        bufferData(target, data, usage) {
            if (typeof data === "number") {
                native.gl.bufferDataSize(target, data, usage);
            } else {
                native.gl.bufferData(target, bytesView(data), usage);
            }
        }

        bufferSubData(target, offset, data) {
            native.gl.bufferSubData(target, offset, bytesView(data));
        }

        enableVertexAttribArray(index) {
            native.gl.enableVertexAttribArray(index);
        }

        disableVertexAttribArray(index) {
            native.gl.disableVertexAttribArray(index);
        }

        vertexAttribPointer(index, size, type, normalized, stride, offset) {
            native.gl.vertexAttribPointer(index, size, type, Boolean(normalized), stride, offset);
        }

        drawArrays(mode, first, count) {
            native.gl.drawArrays(mode, first, count);
        }

        drawElements(mode, count, type, offset) {
            native.gl.drawElements(mode, count, type, offset);
        }

        createShader(type) {
            const id = native.gl.createShader(type);
            return id < 0 ? null : id;
        }

        deleteShader(shader) {
            if (shader != null) native.gl.deleteShader(shader);
        }

        shaderSource(shader, source) {
            native.gl.shaderSource(shader, String(source));
        }

        compileShader(shader) {
            native.gl.compileShader(shader);
        }

        getShaderInfoLog(shader) {
            return shader == null ? "" : native.gl.getShaderInfoLog(shader);
        }

        getShaderParameter(shader, parameter) {
            if (shader == null) return false;
            if (parameter === GL.COMPILE_STATUS) {
                return !native.gl.getShaderInfoLog(shader);
            }
            return false;
        }

        createProgram() {
            const id = native.gl.createProgram();
            return id < 0 ? null : id;
        }

        deleteProgram(program) {
            if (program != null) native.gl.deleteProgram(program);
        }

        attachShader(program, shader) {
            if (program != null && shader != null) native.gl.attachShader(program, shader);
        }

        bindAttribLocation(program, index, name) {
            native.gl.bindAttribLocation(program, index, String(name));
        }

        linkProgram(program) {
            native.gl.linkProgram(program);
        }

        getProgramParameter(program, parameter) {
            if (program == null) return false;
            if (parameter === GL.LINK_STATUS) return native.gl.getLinkStatus(program);
            return 0;
        }

        getProgramInfoLog(program) {
            return program == null ? "" : native.gl.getProgramInfoLog(program);
        }

        useProgram(program) {
            native.gl.useProgram(program == null ? -1 : program);
        }

        getAttribLocation(program, name) {
            return program == null ? -1 : native.gl.getAttribLocation(program, String(name));
        }

        getUniformLocation(program, name) {
            if (program == null) return null;
            const location = native.gl.getUniformLocation(program, String(name));
            return location < 0 ? null : location;
        }

        __location(location) {
            return location == null ? -1 : location;
        }

        uniform1f(location, value) {
            native.gl.uniform1f(this.__location(location), value);
        }

        uniform1i(location, value) {
            native.gl.uniform1i(this.__location(location), value);
        }

        uniform2f(location, x, y) {
            native.gl.uniform2f(this.__location(location), x, y);
        }

        uniform3f(location, x, y, z) {
            native.gl.uniform3f(this.__location(location), x, y, z);
        }

        uniform1fv(location, value) {
            native.gl.uniform1fv(this.__location(location), float32(value));
        }

        uniform2fv(location, value) {
            native.gl.uniform2fv(this.__location(location), float32(value));
        }

        uniform3fv(location, value) {
            native.gl.uniform3fv(this.__location(location), float32(value));
        }

        uniformMatrix4fv(location, transpose, value) {
            native.gl.uniformMatrix4fv(this.__location(location), float32(value));
        }

        createFramebuffer() {
            const id = native.gl.createFramebuffer();
            return id < 0 ? null : id;
        }

        deleteFramebuffer(framebuffer) {
            if (framebuffer != null) native.gl.deleteFramebuffer(framebuffer);
        }

        bindFramebuffer(target, framebuffer) {
            native.gl.bindFramebuffer(target, framebuffer == null ? -1 : framebuffer);
        }

        createRenderbuffer() {
            const id = native.gl.createRenderbuffer();
            return id < 0 ? null : id;
        }

        deleteRenderbuffer(renderbuffer) {
            if (renderbuffer != null) native.gl.deleteRenderbuffer(renderbuffer);
        }

        bindRenderbuffer(target, renderbuffer) {
            native.gl.bindRenderbuffer(target, renderbuffer == null ? -1 : renderbuffer);
        }

        renderbufferStorage(target, internalformat, width, height) {
            native.gl.renderbufferStorage(target, internalformat, width, height);
        }

        framebufferTexture2D(target, attachment, textarget, texture, level) {
            native.gl.framebufferTexture2D(
                target,
                attachment,
                textarget,
                texture == null ? -1 : texture,
                level,
            );
        }

        framebufferRenderbuffer(target, attachment, renderbuffertarget, renderbuffer) {
            native.gl.framebufferRenderbuffer(
                target,
                attachment,
                renderbuffertarget,
                renderbuffer == null ? -1 : renderbuffer,
            );
        }

        checkFramebufferStatus(target) {
            return native.gl.checkFramebufferStatus(target);
        }

        readPixels(x, y, width, height, format, type, pixels) {
            native.gl.readPixels(x, y, width, height, bytesView(pixels));
        }

        getExtension(name) {
            if (name === "WEBGL_lose_context") {
                return { loseContext() {}, restoreContext() {} };
            }
            return null;
        }

        getSupportedExtensions() {
            return ["WEBGL_lose_context"];
        }

        getError() {
            return GL.NO_ERROR;
        }

        isContextLost() {
            return false;
        }

        flush() {}

        finish() {}
    }

    for (const [name, value] of Object.entries(GL)) {
        WebGLRenderingContext.prototype[name] = value;
    }

    // ------------------------------------------------------------------ images

    class Image {
        constructor(width, height) {
            this.__id = -1;
            this._src = "";
            this.width = Number(width) || 0;
            this.height = Number(height) || 0;
            this.onload = null;
            this.onerror = null;
            this.complete = false;
            this.crossOrigin = null;
        }

        get src() {
            return this._src;
        }

        set src(value) {
            this._src = String(value);
            let id = -1;
            try {
                const data = decodeDataURL(this._src);
                id = data ? native.image.decode(data) : native.image.load(this._src);
            } catch (error) {
                reportError(error);
            }
            if (id >= 0) {
                this.__id = id;
                this.width = native.image.width(id);
                this.height = native.image.height(id);
                this.complete = true;
                setTimeoutFn(() => {
                    if (typeof this.onload === "function") this.onload({ target: this });
                }, 0);
            } else {
                setTimeoutFn(() => {
                    if (typeof this.onerror === "function") {
                        this.onerror(new Error("Failed to load image " + this._src));
                    }
                }, 0);
            }
        }
    }

    class ImageData {
        constructor(a, b, c) {
            if (typeof a === "number") {
                this.width = a;
                this.height = b;
                this.data = new Uint8ClampedArray(a * b * 4);
            } else {
                this.data = a;
                this.width = b;
                this.height = c === undefined ? a.length / 4 / b : c;
                if (this.height === undefined) this.height = 0;
            }
        }
    }

    class Blob {
        constructor(parts, options) {
            this.__bytes = [];
            for (const part of parts || []) {
                if (part instanceof Uint8Array) this.__bytes.push(part);
                else if (part instanceof Blob) this.__bytes.push(part.__bytes[0] || new Uint8Array(0));
                else this.__bytes.push(new TextEncoder().encode(String(part)));
            }
            this.type = (options && options.type) || "";
            this.size = this.__bytes.reduce((sum, part) => sum + part.length, 0);
        }

        arrayBuffer() {
            const output = new Uint8Array(this.size);
            let offset = 0;
            for (const part of this.__bytes) {
                output.set(part, offset);
                offset += part.length;
            }
            return Promise.resolve(output.buffer);
        }

        text() {
            return this.arrayBuffer().then((buffer) => new TextDecoder().decode(buffer));
        }
    }

    const objectURLs = new Map();
    let nextObjectURL = 1;

    class URLClass {
        constructor(url) {
            this.href = String(url);
        }

        toString() {
            return this.href;
        }

        static createObjectURL(blob) {
            const url = "blob:kaplay-native/" + nextObjectURL++;
            objectURLs.set(url, blob);
            return url;
        }

        static revokeObjectURL(url) {
            objectURLs.delete(url);
        }
    }

    // ------------------------------------------------------------------- fonts

    class FontFace {
        constructor(family, source) {
            this.family = family;
            this.source = source;
            this.status = "unloaded";
            this.loaded = null;
            if (source instanceof ArrayBuffer) this.__bytes = new Uint8Array(source);
            else if (ArrayBuffer.isView(source)) {
                this.__bytes = new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
            } else {
                this.__bytes = null;
            }
        }

        load() {
            if (this.loaded) return this.loaded;
            let bytes = this.__bytes;
            if (!bytes) {
                const match = /url\((?:"|')?(.*?)(?:"|')?\)/.exec(String(this.source));
                const path = match ? match[1] : String(this.source);
                const size = native.fs.size(path);
                if (size >= 0) {
                    bytes = new Uint8Array(size);
                    native.fs.read(path, bytes);
                }
            }
            if (bytes) {
                native.font.register(this.family, bytes);
                this.status = "loaded";
                this.loaded = Promise.resolve(this);
            } else {
                this.status = "error";
                this.loaded = Promise.reject(new Error("Failed to load font " + this.family));
            }
            return this.loaded;
        }
    }

    const fontFaces = new Set();

    function parseFontFamily(spec) {
        const match = /(?:\d+)?(?:\.\d+)?px\s+(.*)$/.exec(String(spec));
        if (!match) return String(spec);
        return match[1]
            .split(",")[0]
            .trim()
            .replace(/^["']|["']$/g, "");
    }

    // ------------------------------------------------------------------- audio

    class AudioBuffer {
        constructor(options) {
            const settings = options || {};
            this.length = Number(settings.length) || 1;
            this.sampleRate = Number(settings.sampleRate) || 44100;
            this.numberOfChannels = Number(settings.numberOfChannels) || 1;
            this.duration = this.length / this.sampleRate;
            this.__channels = [];
            for (let i = 0; i < this.numberOfChannels; i++) {
                this.__channels.push(new Float32Array(this.length));
            }
        }

        getChannelData(channel) {
            return this.__channels[channel] || this.__channels[0];
        }

        copyFromChannel(destination, channel, start) {
            const source = this.getChannelData(channel);
            destination.set(source.subarray(start || 0, (start || 0) + destination.length));
        }

        copyToChannel(source, channel, start) {
            this.getChannelData(channel).set(source, start || 0);
        }
    }

    class AudioContext {
        constructor() {
            this.sampleRate = 44100;
            this.currentTime = 0;
            this.state = "running";
            this.destination = { connect() {}, disconnect() {} };
        }

        createGain() {
            return {
                gain: {
                    value: 1,
                    setValueAtTime() {},
                    linearRampToValueAtTime() {},
                    exponentialRampToValueAtTime() {},
                    cancelScheduledValues() {},
                },
                connect() {},
                disconnect() {},
            };
        }

        createBufferSource() {
            return {
                buffer: null,
                playbackRate: { value: 1 },
                detune: { value: 0 },
                loop: false,
                onended: null,
                connect() {},
                disconnect() {},
                start() {},
                stop() {},
            };
        }

        createStereoPanner() {
            return { pan: { value: 0 }, connect() {}, disconnect() {} };
        }

        createMediaElementSource() {
            return { connect() {}, disconnect() {} };
        }

        createBuffer(channels, length, sampleRate) {
            return new AudioBuffer({
                numberOfChannels: channels,
                length,
                sampleRate,
            });
        }

        decodeAudioData(buffer, success, failure) {
            try {
                const result = this.createBuffer(2, 44100, 44100);
                if (typeof success === "function") success(result);
                return Promise.resolve(result);
            } catch (error) {
                if (typeof failure === "function") failure(error);
                return Promise.reject(error);
            }
        }

        suspend() {
            this.state = "suspended";
            return Promise.resolve();
        }

        resume() {
            this.state = "running";
            return Promise.resolve();
        }

        close() {
            this.state = "closed";
            return Promise.resolve();
        }
    }

    class HTMLAudioElement {
        constructor(src) {
            this.src = src;
            this.loop = false;
            this.volume = 1;
            this.playbackRate = 1;
            this.currentTime = 0;
            this.duration = 0;
            this.crossOrigin = null;
            this.onended = null;
            this.paused = true;
        }

        play() {
            this.paused = false;
            return Promise.resolve();
        }

        pause() {
            this.paused = true;
        }

        load() {}

        addEventListener() {}

        removeEventListener() {}
    }

    // -------------------------------------------------------------- observables

    const resizeObservers = new Set();

    class ResizeObserver {
        constructor(callback) {
            this.callback = callback;
            this.targets = new Set();
            resizeObservers.add(this);
        }

        observe(target) {
            this.targets.add(target);
        }

        unobserve(target) {
            this.targets.delete(target);
        }

        disconnect() {
            this.targets.clear();
            resizeObservers.delete(this);
        }

        __notify() {
            const entries = [];
            for (const target of this.targets) {
                entries.push({
                    target,
                    contentRect: target.getBoundingClientRect(),
                });
            }
            if (entries.length) {
                try {
                    this.callback(entries, this);
                } catch (error) {
                    reportError(error);
                }
            }
        }
    }

    function notifyResize() {
        for (const observer of Array.from(resizeObservers)) observer.__notify();
    }

    // --------------------------------------------------------------- dom events

    function makeEvent(type, payload) {
        payload = payload || {};
        const event = {
            type,
            target: null,
            defaultPrevented: false,
            preventDefault() {
                this.defaultPrevented = true;
            },
            stopPropagation() {},
            stopImmediatePropagation() {},
            getModifierState(name) {
                if (name === "CapsLock") return Boolean(payload.capsLock);
                if (name === "Shift") return Boolean(payload.shiftKey);
                if (name === "Control") return Boolean(payload.ctrlKey);
                if (name === "Alt") return Boolean(payload.altKey);
                if (name === "Meta") return Boolean(payload.metaKey);
                return false;
            },
        };
        return event;
    }

    function canvasCoordinates(clientX, clientY) {
        const rect = mainCanvas ? canvasSize(mainCanvas) : { x: 0, y: 0, scale: 1 };
        if (!rect.scale) return { x: 0, y: 0 };
        return {
            x: (clientX - rect.x) / rect.scale,
            y: (clientY - rect.y) / rect.scale,
        };
    }

    function dispatchPointer(type, payload) {
        const event = makeEvent(type, payload);
        Object.assign(event, payload);
        const position = canvasCoordinates(payload.clientX || 0, payload.clientY || 0);
        event.offsetX = position.x;
        event.offsetY = position.y;
        event.movementX = payload.movementX || 0;
        event.movementY = payload.movementY || 0;
        event.buttons = mouseButtons;
        canvasListeners.emit(event);
    }

    function dispatchKey(type, payload) {
        const event = makeEvent(type, payload);
        Object.assign(event, payload);
        canvasListeners.emit(event);
    }

    function dispatchWheel(payload) {
        const event = makeEvent("wheel", payload);
        event.deltaX = payload.deltaX || 0;
        event.deltaY = payload.deltaY || 0;
        event.deltaMode = 0;
        canvasListeners.emit(event);
    }

    let mouseButtons = 0;

    global.__kapNative_onFrame = function (now) {
        tickTimers(now);
        const queue = frames.splice(0, frames.length);
        for (const entry of queue) {
            try {
                entry.callback(now);
            } catch (error) {
                reportError(error);
            }
        }
    };

    global.__kapNative_dispatch = function (type, payloadJson) {
        let payload = {};
        try {
            payload = JSON.parse(payloadJson);
        } catch (error) {
            reportError(error);
        }
        switch (type) {
            case "resize": {
                screen.width = payload.width;
                screen.height = payload.height;
                screen.dpr = payload.dpr;
                notifyResize();
                windowListeners.emit(makeEvent("resize", payload));
                break;
            }
            case "visibilitychange": {
                document.visibilityState = payload.hidden ? "hidden" : "visible";
                documentListeners.emit(makeEvent("visibilitychange", payload));
                break;
            }
            case "focus":
                windowListeners.emit(makeEvent("focus", payload));
                break;
            case "blur":
                windowListeners.emit(makeEvent("blur", payload));
                break;
            case "keydown":
                dispatchKey("keydown", payload);
                break;
            case "keyup":
                dispatchKey("keyup", payload);
                break;
            case "mousemove":
                dispatchPointer("mousemove", payload);
                break;
            case "mousedown":
                mouseButtons |= 1 << payload.button;
                dispatchPointer("mousedown", payload);
                break;
            case "mouseup":
                mouseButtons &= ~(1 << payload.button);
                dispatchPointer("mouseup", payload);
                break;
            case "wheel":
                dispatchWheel(payload);
                break;
            default:
                break;
        }
    };

    // -------------------------------------------------------------------- dom

    const documentElement = {
        style: makeStyle(),
        appendChild(child) {
            if (child instanceof HTMLCanvasElement) child.__parent = documentElement;
            return child;
        },
        getBoundingClientRect() {
            return { x: 0, y: 0, left: 0, top: 0, width: screen.width, height: screen.height };

        },
    };

    const body = {
        style: makeStyle(),
        appendChild(child) {
            if (child instanceof HTMLCanvasElement) {
                child.__parent = body;
                mainCanvas = child;
            }
            return child;
        },
        get offsetWidth() {
            return screen.width;
        },
        get offsetHeight() {
            return screen.height;
        },
        getBoundingClientRect() {
            return { x: 0, y: 0, left: 0, top: 0, width: screen.width, height: screen.height };
        },
        addEventListener(type, fn) {
            documentListeners.add(type, fn);
        },
        removeEventListener(type, fn) {
            documentListeners.remove(type, fn);
        },
    };

    let mainCanvas = null;

    function utf8Encode(value) {
        const text = String(value);
        const bytes = [];
        for (let i = 0; i < text.length; i++) {
            let code = text.codePointAt(i);
            if (code > 0xffff) i++;
            if (code < 0x80) bytes.push(code);
            else if (code < 0x800) {
                bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
            } else if (code < 0x10000) {
                bytes.push(
                    0xe0 | (code >> 12),
                    0x80 | ((code >> 6) & 0x3f),
                    0x80 | (code & 0x3f),
                );
            } else {
                bytes.push(
                    0xf0 | (code >> 18),
                    0x80 | ((code >> 12) & 0x3f),
                    0x80 | ((code >> 6) & 0x3f),
                    0x80 | (code & 0x3f),
                );
            }
        }
        return new Uint8Array(bytes);
    }

    function utf8Decode(value) {
        const bytes = bytesView(value);
        let output = "";
        for (let i = 0; i < bytes.length;) {
            const first = bytes[i++];
            let code;
            if (first < 0x80) code = first;
            else if (first < 0xe0) code = ((first & 0x1f) << 6) | (bytes[i++] & 0x3f);
            else if (first < 0xf0) {
                code = ((first & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f);
            } else {
                code = ((first & 0x07) << 18) | ((bytes[i++] & 0x3f) << 12)
                    | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f);
            }
            output += String.fromCodePoint(code);
        }
        return output;
    }

    class TextEncoder {
        encode(value) {
            return utf8Encode(value === undefined ? "" : value);
        }
    }

    class TextDecoder {
        constructor(encoding) {
            this.encoding = encoding || "utf-8";
        }

        decode(value) {
            if (value === undefined) return "";
            return utf8Decode(value);
        }
    }

    const document = {
        body,
        documentElement,
        visibilityState: "visible",
        activeElement: null,
        fullscreenElement: null,
        pointerLockElement: null,
        fonts: {
            add(face) {
                fontFaces.add(face);
            },
            delete(face) {
                fontFaces.delete(face);
            },
            check(spec) {
                const family = parseFontFamily(spec);
                return native.font.has(family);
            },
            load() {
                return Promise.resolve([]);
            },
            ready: Promise.resolve(),
            forEach(callback) {
                fontFaces.forEach(callback);
            },
        },
        createElement(tag) {
            if (String(tag).toLowerCase() === "canvas") return new HTMLCanvasElement();
            return { style: makeStyle(), addEventListener() {}, removeEventListener() {} };
        },
        createElementNS(_namespace, tag) {
            return document.createElement(tag);
        },
        querySelector(selector) {
            if (mainCanvas && String(selector).startsWith("#")) return mainCanvas;
            return null;
        },
        exitPointerLock() {
            native.window.setCursorLocked(false);
            document.pointerLockElement = null;
        },
        exitFullscreen() {
            native.window.setFullscreen(false);
            document.fullscreenElement = null;
        },
        addEventListener(type, fn) {
            documentListeners.add(type, fn);
        },
        removeEventListener(type, fn) {
            documentListeners.remove(type, fn);
        },
        dispatchEvent(event) {
            documentListeners.emit(event);
            return true;
        },
    };

    const storageData = (function () {
        try {
            return JSON.parse(native.storage.load()) || {};
        } catch (error) {
            return {};
        }
    })();

    function persistStorage() {
        try {
            native.storage.save(JSON.stringify(storageData));
        } catch (error) {
            reportError(error);
        }
    }

    const localStorage = new Proxy(storageData, {
        get(target, key) {
            if (key === "getItem") return (name) => (name in target ? target[name] : null);
            if (key === "setItem") {
                return (name, value) => {
                    target[name] = String(value);
                    persistStorage();
                };
            }
            if (key === "removeItem") {
                return (name) => {
                    delete target[name];
                    persistStorage();
                };
            }
            if (key === "clear") {
                return () => {
                    for (const name in target) delete target[name];
                    persistStorage();
                };
            }
            if (key === "key") return (index) => Object.keys(target)[index] ?? null;
            if (key === "length") return Object.keys(target).length;
            return target[key];
        },
        set(target, key, value) {
            target[key] = String(value);
            persistStorage();
            return true;
        },
        deleteProperty(target, key) {
            delete target[key];
            persistStorage();
            return true;
        },
    });

    const navigator = {
        userAgent: "kaplay-native",
        platform: native.env.platform,
        language: "en-US",
        getGamepads() {
            return [];
        },
        maxTouchPoints: 0,
        vibrate() {
            return false;
        },
    };

    const location = {
        href: "kaplay-native://game/",
        origin: "kaplay-native://game",
        protocol: "kaplay-native:",
        host: "game",
        hostname: "game",
        pathname: "/",
        search: "",
        hash: "",
        toString() {
            return this.href;
        },
    };

    function makeResponse(bytes, url) {
        return {
            ok: true,
            status: 200,
            statusText: "OK",
            url,
            headers: { get() { return null; } },
            json() {
                return Promise.resolve(JSON.parse(new TextDecoder().decode(bytes)));
            },
            text() {
                return Promise.resolve(new TextDecoder().decode(bytes));
            },
            arrayBuffer() {
                const output = new Uint8Array(bytes.length);
                output.set(bytes);
                return Promise.resolve(output.buffer);
            },
            blob() {
                return Promise.resolve(new Blob([bytes]));
            },
        };
    }

    function fetchResource(input) {
        const url = typeof input === "string" ? input : String(input && input.url ? input.url : input);
        return new Promise((resolve, reject) => {
            const data = decodeDataURL(url);
            if (data) {
                resolve(makeResponse(data, url));
                return;
            }
            if (/^https?:/i.test(url)) {
                reject(new Error("Network requests are disabled in kaplay-native"));
                return;
            }
            const size = native.fs.size(url);
            if (size < 0) {
                setTimeoutFn(() => reject(new TypeError("Failed to fetch " + url)), 0);
                return;
            }
            const bytes = new Uint8Array(size);
            native.fs.read(url, bytes);
            setTimeoutFn(() => resolve(makeResponse(bytes, url)), 0);
        });
    }

    const console = {
        log(...args) {
            native.env.log("info", args.map(String).join(" "));
        },
        info(...args) {
            native.env.log("info", args.map(String).join(" "));
        },
        debug(...args) {
            native.env.log("debug", args.map(String).join(" "));
        },
        warn(...args) {
            native.env.log("warn", args.map(String).join(" "));
        },
        error(...args) {
            native.env.log("error", args.map(String).join(" "));
        },
        trace(...args) {
            native.env.log("debug", args.map(String).join(" "));
        },
    };

    // ----------------------------------------------------------- game platform

    const steam = {
        available: () => native.steam.available(),
        playerName: () => native.steam.playerName(),
        steamId: () => native.steam.steamId(),
        unlockAchievement: (name) => native.steam.unlockAchievement(String(name)),
        isAchievementUnlocked: (name) => native.steam.isAchievementUnlocked(String(name)),
        storeStats: () => native.steam.storeStats(),
        setStatInt: (name, value) => native.steam.setStatInt(String(name), Number(value) | 0),
        getStatInt: (name) => native.steam.getStatInt(String(name)),
        setStatFloat: (name, value) => native.steam.setStatFloat(String(name), Number(value)),
        getStatFloat: (name) => native.steam.getStatFloat(String(name)),
        writeCloud: (name, text) => native.steam.writeCloud(String(name), String(text)),
        readCloud: (name) => native.steam.readCloud(String(name)),
        cloudExists: (name) => native.steam.cloudExists(String(name)),
        deleteCloud: (name) => native.steam.deleteCloud(String(name)),
        runCallbacks: () => native.steam.runCallbacks(),
    };

    const kaplayNative = {
        version: native.env.hostVersion,
        platform: native.env.platform,
        steam,
    };

    // ------------------------------------------------------------------ export

    const define = (name, value) => {
        try {
            Object.defineProperty(global, name, { value, writable: true, configurable: true });
        } catch {
            global[name] = value;
        }
    };
    define("window", global);
    define("kaplayNative", kaplayNative);
    define("self", global);
    define("document", document);
    define("navigator", navigator);
    define("location", location);
    define("console", console);
    define("localStorage", localStorage);
    define("Image", Image);
    define("ImageData", ImageData);
    define("Blob", Blob);
    global.URL = URLClass;
    define("FontFace", FontFace);
    global.Audio = HTMLAudioElement;
    define("AudioBuffer", AudioBuffer);
    define("AudioContext", AudioContext);
    global.webkitAudioContext = AudioContext;
    define("ResizeObserver", ResizeObserver);
    define("HTMLCanvasElement", HTMLCanvasElement);
    define("TextEncoder", TextEncoder);
    define("TextDecoder", TextDecoder);
    global.fetch = fetchResource;
    global.atob = atob;
    global.btoa = btoa;
    global.requestAnimationFrame = requestAnimationFrame;
    global.cancelAnimationFrame = cancelAnimationFrame;
    global.addEventListener = (type, fn) => windowListeners.add(type, fn);
    global.removeEventListener = (type, fn) => windowListeners.remove(type, fn);
    global.dispatchEvent = (event) => {
        windowListeners.emit(event);
        return true;
    };
    global.setTimeout = setTimeoutFn;
    global.clearTimeout = clearTimer;
    global.setInterval = setIntervalFn;
    global.clearInterval = clearTimer;
    global.performance = { now: () => native.env.now() };
    global.kaplayjs_assetsAliases = {};
    global.onerror = null;

    Object.defineProperty(global, "innerWidth", {
        get: () => screen.width,
        configurable: true,
    });
    Object.defineProperty(global, "innerHeight", {
        get: () => screen.height,
        configurable: true,
    });
    Object.defineProperty(global, "devicePixelRatio", {
        get: () => screen.dpr,
        configurable: true,
    });

    if (typeof Promise.withResolvers !== "function") {
        Promise.withResolvers = function () {
            let resolve;
            let reject;
            const promise = new Promise((res, rej) => {
                resolve = res;
                reject = rej;
            });
            return { promise, resolve, reject };
        };
    }
})();
