const canvas = document.createElement("canvas");
canvas.width = 320;
canvas.height = 240;
document.body.appendChild(canvas);

const gl = canvas.getContext("webgl");

const video = document.createElement("video");
video.playsInline = true;
video.addEventListener("loadedmetadata", () => {
    console.log("VIDEO META", video.videoWidth, video.videoHeight, video.duration);
});
video.src = "assets/sample.mp4";

const vertices = new Float32Array([-1, -1, 0, 0, 1, -1, 1, 0, -1, 1, 0, 1, 1, 1, 1, 1]);
const buffer = gl.createBuffer();
gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
gl.enableVertexAttribArray(0);
gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
gl.enableVertexAttribArray(1);
gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 16, 8);
const vert = "attribute vec2 a_pos;attribute vec2 a_uv;varying vec2 v_uv;void main(){v_uv=a_uv;gl_Position=vec4(a_pos,0.0,1.0);}";
const frag = "precision mediump float;varying vec2 v_uv;uniform sampler2D u_tex;void main(){gl_FragColor=texture2D(u_tex,v_uv);}";
const vs = gl.createShader(gl.VERTEX_SHADER);
gl.shaderSource(vs, vert);
gl.compileShader(vs);
const fs = gl.createShader(gl.FRAGMENT_SHADER);
gl.shaderSource(fs, frag);
gl.compileShader(fs);
const program = gl.createProgram();
gl.attachShader(program, vs);
gl.attachShader(program, fs);
gl.bindAttribLocation(program, 0, "a_pos");
gl.bindAttribLocation(program, 1, "a_uv");
gl.linkProgram(program);
gl.useProgram(program);
gl.uniform1i(gl.getUniformLocation(program, "u_tex"), 0);

const texture = gl.createTexture();
gl.bindTexture(gl.TEXTURE_2D, texture);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

function waitForFrame(callback) {
    if (video.readyState >= 2) {
        callback();
        return;
    }
    requestAnimationFrame(() => waitForFrame(callback));
}

async function show(time) {
    video.currentTime = time;
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, video);
    gl.viewport(0, 0, 320, 240);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    const pixels = new Uint8Array(4);
    gl.readPixels(160, 120, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    console.log("VIDEO FRAME", time, Array.from(pixels).join(","));
}

video.play()
    .then(() => {
        video.pause();
        video.currentTime = 0.5;
        waitForFrame(() => show(0.5));
    })
    .catch((error) => console.log("VIDEO ERROR", error.message));
