const native = __kapNative;

const QUADS = 512;
const MAX_FRAMES = 900;
const CHECKS_AT = 30;

function record(name, value) {
    console.log(`BENCH ${name}=${value}`);
}

function recordInfo(name, value) {
    console.log(`BENCH_INFO ${name}=${value}`);
}

function checksum(bytes) {
    let total = 0;
    for (let index = 0; index < bytes.length; index++) {
        total = (total * 31 + bytes[index]) >>> 0;
    }
    return total;
}

const canvas = document.createElement("canvas");
canvas.width = 960;
canvas.height = 540;
document.body.appendChild(canvas);
const gl = canvas.getContext("webgl");

record("gl.version", gl.getParameter(gl.VERSION));

const vertexSource =
    "attribute vec2 a_pos;attribute vec2 a_uv;varying vec2 v_uv;uniform vec2 u_offset;void main(){v_uv=a_uv;gl_Position=vec4(a_pos+u_offset,0.0,1.0);}";
const fragmentSource =
    "precision mediump float;varying vec2 v_uv;uniform sampler2D u_tex;void main(){gl_FragColor=texture2D(u_tex,v_uv);}";
function compile(kind, source) {
    const shader = gl.createShader(kind);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
        throw new Error(gl.getShaderInfoLog(shader));
    }
    return shader;
}
const program = gl.createProgram();
gl.attachShader(program, compile(gl.VERTEX_SHADER, vertexSource));
gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fragmentSource));
gl.bindAttribLocation(program, 0, "a_pos");
gl.bindAttribLocation(program, 1, "a_uv");
gl.linkProgram(program);
gl.useProgram(program);
gl.uniform1i(gl.getUniformLocation(program, "u_tex"), 0);
const offsetLocation = gl.getUniformLocation(program, "u_offset");

const vertices = new Float32Array(QUADS * 6 * 4);
const quadState = [];
for (let index = 0; index < QUADS; index++) {
    quadState.push({
        x: (index % 32) / 32 - 0.5,
        y: Math.floor(index / 32) / 16 - 0.5,
        vx: ((index % 7) - 3) * 0.05,
        vy: ((index % 5) - 2) * 0.05,
        size: 0.015 + (index % 4) * 0.004,
    });
}

function updateVertices(seconds) {
    let cursor = 0;
    for (let index = 0; index < QUADS; index++) {
        const quad = quadState[index];
        quad.x += quad.vx * seconds;
        quad.y += quad.vy * seconds;
        if (quad.x < -1 || quad.x > 1) quad.vx = -quad.vx;
        if (quad.y < -1 || quad.y > 1) quad.vy = -quad.vy;
        const x0 = quad.x;
        const y0 = quad.y;
        const x1 = quad.x + quad.size;
        const y1 = quad.y + quad.size;
        const corners = [x0, y0, 0, 0, x1, y0, 1, 0, x0, y1, 0, 1, x1, y0, 1, 0, x1, y1, 1, 1, x0, y1, 0, 1];
        vertices.set(corners, cursor);
        cursor += corners.length;
    }
}

const buffer = gl.createBuffer();
gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
gl.bufferData(gl.ARRAY_BUFFER, vertices.byteLength, gl.DYNAMIC_DRAW);
gl.enableVertexAttribArray(0);
gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
gl.enableVertexAttribArray(1);
gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 16, 8);

const textCanvas = document.createElement("canvas");
textCanvas.width = 256;
textCanvas.height = 64;
const textContext = textCanvas.getContext("2d");
textContext.font = "16px monospace";
textContext.textBaseline = "top";
textContext.textAlign = "left";

const spriteCanvas = document.createElement("canvas");
spriteCanvas.width = 128;
spriteCanvas.height = 128;
const spriteContext = spriteCanvas.getContext("2d");
spriteContext.font = "16px monospace";
spriteContext.textBaseline = "top";

function makeTexture(source) {
    const texture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, source);
    return texture;
}

const spriteTexture = makeTexture(spriteCanvas);
const textTexture = makeTexture(textCanvas);
const videoTexture = makeTexture(spriteCanvas);

const audio = new AudioContext();
let audioSource = null;
let audioGain = null;
let video = null;
let videoReady = false;
let videoUploads = 0;
let frames = 0;
let frameMs = 0;
let last = performance.now();

function prepareText() {
    textContext.clearRect(0, 0, textCanvas.width, textCanvas.height);
    spriteContext.clearRect(0, 0, spriteCanvas.width, spriteCanvas.height);
    spriteContext.fillStyle = "#44cc88";
    spriteContext.fillText("sprite", 8, 24);
    spriteContext.fillText("layer", 8, 48);
    spriteContext.fillStyle = "#cc8844";
    spriteContext.fillText("0123456789", 8, 72);
    textContext.fillText("wave 12  score 987654", 4, 4);
    textContext.fillText("native bench frame", 4, 24);
    const metrics = textContext.measureText("wave 12  score 987654");
    gl.bindTexture(gl.TEXTURE_2D, textTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, textCanvas);
    return [metrics.width, textContext.getImageData(0, 0, textCanvas.width, textCanvas.height).data];
}

function frame(now) {
    const frameStart = performance.now();
    frames += 1;
    const seconds = Math.min(0.05, (now - last) / 1000);
    last = now;

    const text = prepareText();
    const spritePixels = spriteContext.getImageData(0, 0, spriteCanvas.width, spriteCanvas.height);
    textContext.drawImage(spriteCanvas, 0, 0, 128, 128, 96, -32, 128, 128);
    const small = textContext.getImageData(0, 0, 32, 32);

    updateVertices(seconds);
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, vertices);
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clearColor(0.05, 0.06, 0.09, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindTexture(gl.TEXTURE_2D, spriteTexture);
    gl.uniform2f(offsetLocation, 0, 0);
    gl.drawArrays(gl.TRIANGLES, 0, QUADS * 6);
    gl.bindTexture(gl.TEXTURE_2D, textTexture);
    gl.uniform2f(offsetLocation, 0, 0.8);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    if (video && videoReady && video.readyState >= 2) {
        gl.bindTexture(gl.TEXTURE_2D, videoTexture);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video);
        gl.uniform2f(offsetLocation, -0.8, -0.8);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        videoUploads += 1;
    }

    if (audioGain) {
        audioGain.gain.value = 0.25;
    }

    if (frames === CHECKS_AT) {
        record("canvas.text_checksum", checksum(text[1]));
        record("canvas.metrics", Math.round(text[0]));
        record("canvas.sprite_checksum", checksum(spritePixels.data));
        record("canvas.small_checksum", checksum(small.data));
        record("gl.error", gl.getError());
    }

    if (frames >= MAX_FRAMES) {
        recordInfo("frames", frames);
        recordInfo("video.uploads", videoUploads);
        recordInfo("js_frame_ms", (frameMs / frames).toFixed(4));
        native.env.quit();
        return;
    }
    frameMs += performance.now() - frameStart;
    requestAnimationFrame(frame);
}

async function prepareAudio() {
    const response = await fetch("assets/impact.opus");
    const bytes = await response.arrayBuffer();
    record("audio.bytes", bytes.byteLength);
    const decoded = await audio.decodeAudioData(bytes);
    record("audio.frames", decoded.length);
    record("audio.sample_rate", decoded.sampleRate);
    record("audio.channels", decoded.numberOfChannels);
    audioSource = audio.createBufferSource();
    audioSource.buffer = decoded;
    audioSource.loop = true;
    audioGain = audio.createGain();
    audioGain.gain.value = 0.25;
    audioSource.connect(audioGain);
    audioGain.connect(audio.destination);
    audioSource.start();
}

async function prepareVideo() {
    video = document.createElement("video");
    video.playsInline = true;
    video.src = "assets/sample.mp4";
    await video.play();
    record("video.width", video.videoWidth);
    record("video.height", video.videoHeight);
    video.currentTime = 0.5;
    await new Promise((resolve) => {
        const poll = () => {
            if (video.readyState >= 2 && video.currentTime >= 0.5) {
                resolve();
                return;
            }
            requestAnimationFrame(poll);
        };
        poll();
    });
    videoReady = true;
}

async function prepareNetwork() {
    if (!native.env.networkAllowed) {
        record("net.enabled", 0);
        return;
    }
    const base = location.href;
    const target = new URL("body.bin", base).href;
    const response = await fetch(target);
    const bytes = new Uint8Array(await response.arrayBuffer());
    record("net.fetch_bytes", bytes.byteLength);
    record("net.fetch_checksum", checksum(bytes));
    const echo = await new Promise((resolve, reject) => {
        const socket = new WebSocket(new URL("socket", base).href.replace(/^http/, "ws"));
        const timer = setTimeout(() => reject(new Error("websocket timeout")), 10000);
        socket.onopen = () => socket.send("bench-echo");
        socket.onmessage = (event) => {
            clearTimeout(timer);
            resolve(event.data);
            socket.close();
        };
        socket.onerror = () => {
            clearTimeout(timer);
            reject(new Error("websocket error"));
        };
    });
    record("net.ws_echo", checksum(new TextEncoder().encode(String(echo))));
}

prepareText();
prepareVideo().catch((error) => recordInfo("video.error", String(error.message || error)));
prepareAudio().catch((error) => recordInfo("audio.error", String(error.message || error)));
prepareNetwork().catch((error) => recordInfo("net.error", String(error.message || error)));
requestAnimationFrame(frame);
