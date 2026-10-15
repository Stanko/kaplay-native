import { red } from "./shared.js";

const lazy = await import("./lazy.js");
if (sharedLoads !== 1 || lazy.red !== red) throw new Error("module cache mismatch");
if (!import.meta.url.endsWith("/assets/main.js")) throw new Error("missing module URL");

const canvas = document.querySelector("canvas");
if (!canvas || !document.querySelector(".viewport")) throw new Error("missing HTML elements");
const gl = canvas.getContext("webgl");
const vertices = gl.createBuffer();
gl.bindBuffer(gl.ARRAY_BUFFER, vertices);
gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
const vertex = gl.createShader(gl.VERTEX_SHADER);
gl.shaderSource(vertex, "attribute vec2 a_pos; varying vec2 v_pos; void main(){v_pos=a_pos; gl_Position=vec4(a_pos,0.0,1.0);}");
gl.compileShader(vertex);
const fragment = gl.createShader(gl.FRAGMENT_SHADER);
gl.shaderSource(fragment, "precision mediump float; varying vec2 v_pos; void main(){gl_FragColor=v_pos.x < -0.33 ? vec4(1,0,0,1) : v_pos.x < 0.33 ? vec4(0,1,0,1) : vec4(0,0,1,1);}");
gl.compileShader(fragment);
const program = gl.createProgram();
gl.attachShader(program, vertex);
gl.attachShader(program, fragment);
gl.bindAttribLocation(program, 0, "a_pos");
gl.linkProgram(program);
if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error("shader link failed");
gl.useProgram(program);
gl.enableVertexAttribArray(0);
gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
requestAnimationFrame(() => {
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    console.log("MODULES OK");
});
