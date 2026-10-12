const canvas = document.createElement("canvas");
canvas.width = 320;
canvas.height = 240;
document.body.appendChild(canvas);

const gl = canvas.getContext("webgl");

const surface = document.createElement("canvas");
surface.width = 64;
surface.height = 32;
const ctx = surface.getContext("2d");
ctx.fillStyle = "#ff8800";
ctx.font = "24px monospace";
ctx.textBaseline = "top";
ctx.textAlign = "left";
ctx.fillText("hi", 0, 0);
const pixels = ctx.getImageData(0, 0, 64, 32);
console.log("canvas2d pixel", pixels.data[0], pixels.width, pixels.height);

const texture = gl.createTexture();
gl.bindTexture(gl.TEXTURE_2D, texture);
gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, surface);

console.log("drawing buffer", gl.drawingBufferWidth, gl.drawingBufferHeight);

let frame = 0;
function draw() {
    frame++;
    gl.viewport(0, 0, 320, 240);
    gl.clearColor(0.1, 0.2, 0.3, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.enable(gl.BLEND);
    gl.blendFuncSeparate(
        gl.SRC_ALPHA,
        gl.ONE_MINUS_SRC_ALPHA,
        gl.ONE,
        gl.ONE_MINUS_SRC_ALPHA,
    );

    const vertices = new Float32Array([
        -1, -1, 0, 0,
        1, -1, 1, 0,
        -1, 1, 0, 1,
        1, 1, 1, 1,
    ]);
    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 16, 8);

    const vert = "attribute vec2 a_pos;attribute vec2 a_uv;varying vec2 v_uv;void main(){v_uv=a_uv;gl_Position=vec4(a_pos,0.0,1.0);}";
    const frag = "precision mediump float;varying vec2 v_uv;uniform float u_t;void main(){gl_FragColor=vec4(v_uv.x,u_t,v_uv.y,1.0);}";
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
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
        console.log("link error", gl.getShaderInfoLog(vs), gl.getShaderInfoLog(fs));
    }
    gl.useProgram(program);
    gl.uniform1f(gl.getUniformLocation(program, "u_t"), frame / 3);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
}

requestAnimationFrame(draw);
requestAnimationFrame(draw);
requestAnimationFrame(draw);
