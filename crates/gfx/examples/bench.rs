use std::time::{Duration, Instant};

use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::SwapInterval;
use glutin_winit::{DisplayBuilder, GlWindow};
use kaplay_native_gfx::pixels::UploadOptions;
use kaplay_native_gfx::{GlState, FRAGMENT_SHADER, VERTEX_SHADER};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowAttributes;

const WIDTH: i32 = 1280;
const HEIGHT: i32 = 720;
const WARMUP: u64 = 5;

const VERTEX_SOURCE: &str = "attribute vec2 a_pos;void main(){gl_Position=vec4(a_pos,0.0,1.0);}";
const FRAGMENT_SOURCE: &str =
    "precision mediump float;void main(){gl_FragColor=vec4(0.2,0.6,0.9,1.0);}";

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn report(name: &str, iterations: u64, elapsed: Duration, bytes_per_op: u64) {
    let ns = elapsed.as_secs_f64() * 1e9 / iterations as f64;
    let gib = bytes_per_op as f64 / (1u64 << 30) as f64;
    let gib_per_s = if bytes_per_op > 0 {
        gib / (ns / 1e9)
    } else {
        0.0
    };
    println!(
        "GFX_BENCH {{\"bench\":\"{name}\",\"iterations\":{iterations},\"ns_per_op\":{ns:.1},\"bytes_per_op\":{bytes_per_op},\"gib_per_s\":{gib_per_s:.3}}}"
    );
}

fn measure(name: &str, iterations: u64, bytes_per_op: u64, mut op: impl FnMut()) {
    for _ in 0..WARMUP {
        op();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        op();
    }
    report(name, iterations, start.elapsed(), bytes_per_op);
}

fn rgba_bytes(width: usize, height: usize, alpha: bool) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let a = if alpha { ((x + y) % 256) as u8 } else { 255 };
            data.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 0x88, a]);
        }
    }
    data
}

fn run(event_loop: &ActiveEventLoop) -> Result<(), Box<dyn std::error::Error>> {
    let attributes = WindowAttributes::default().with_visible(false);
    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_depth_size(24)
        .with_stencil_size(8);
    let (window, gl_config) = DisplayBuilder::new()
        .with_window_attributes(Some(attributes))
        .build(event_loop, template, |mut configs| configs.next().unwrap())?;
    let window = window.ok_or("no window")?;
    let raw_window_handle = window.window_handle()?.as_raw();
    let display = gl_config.display();
    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
        .build(Some(raw_window_handle));
    let not_current = unsafe { display.create_context(&gl_config, &context_attributes)? };
    let surface_attributes = window.build_surface_attributes(Default::default())?;
    let surface = unsafe { display.create_window_surface(&gl_config, &surface_attributes)? };
    let gl_context = not_current.make_current(&surface)?;
    let _ = surface.set_swap_interval(&gl_context, SwapInterval::DontWait);

    let glow_context = unsafe {
        glow::Context::from_loader_function_cstr(|symbol| {
            display.get_proc_address(symbol) as *const _
        })
    };
    let renderer = unsafe { glow_context.get_parameter_string(glow::RENDERER) };
    let vendor = unsafe { glow_context.get_parameter_string(glow::VENDOR) };
    let version = unsafe { glow_context.get_parameter_string(glow::VERSION) };
    println!(
        "GFX_RENDERER {{\"renderer\":\"{}\",\"vendor\":\"{}\",\"version\":\"{}\"}}",
        escape(&renderer),
        escape(&vendor),
        escape(&version)
    );

    let mut gl = GlState::new(glow_context);
    gl.resize_drawing_buffer(WIDTH, HEIGHT);
    gl.viewport(0, 0, WIDTH, HEIGHT);

    let pixels = rgba_bytes(512, 512, false);
    let texture = gl.create_texture().ok_or("texture")?;
    gl.active_texture(glow::TEXTURE0);
    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA as i32,
        512,
        512,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        Some(&pixels),
        UploadOptions::default(),
    );
    measure("texture_upload/512", 500, pixels.len() as u64, || {
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            512,
            512,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(&pixels),
            UploadOptions::default(),
        );
    });
    measure("tex_sub_image_2d/512", 500, pixels.len() as u64, || {
        gl.tex_sub_image_2d(
            glow::TEXTURE_2D,
            0,
            0,
            0,
            512,
            512,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            &pixels,
            UploadOptions::default(),
        );
    });

    let buffer_bytes = vec![0x5au8; 64 * 1024];
    let buffer = gl.create_buffer().ok_or("buffer")?;
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
    measure("buffer_data/64k", 2000, buffer_bytes.len() as u64, || {
        gl.buffer_data_bytes(glow::ARRAY_BUFFER, &buffer_bytes, glow::DYNAMIC_DRAW);
    });
    measure(
        "buffer_sub_data/64k",
        2000,
        buffer_bytes.len() as u64,
        || {
            gl.buffer_sub_data(glow::ARRAY_BUFFER, 0, &buffer_bytes);
        },
    );

    let program = gl.create_program().ok_or("program")?;
    let vertex = gl.create_shader(VERTEX_SHADER).ok_or("vertex")?;
    gl.shader_source(vertex, VERTEX_SOURCE)?;
    gl.compile_shader(vertex);
    let fragment = gl.create_shader(FRAGMENT_SHADER).ok_or("fragment")?;
    gl.shader_source(fragment, FRAGMENT_SOURCE)?;
    gl.compile_shader(fragment);
    gl.attach_shader(program, vertex);
    gl.attach_shader(program, fragment);
    gl.bind_attrib_location(program, 0, "a_pos");
    gl.link_program(program);
    if !gl.get_link_status(program) {
        return Err(format!("program link failed: {}", gl.get_program_info_log(program)).into());
    }
    gl.use_program(Some(program));
    let quad = gl.create_buffer().ok_or("quad")?;
    gl.bind_buffer(glow::ARRAY_BUFFER, Some(quad));
    let vertices: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
    let vertex_bytes = unsafe {
        std::slice::from_raw_parts(
            vertices.as_ptr().cast::<u8>(),
            std::mem::size_of_val(&vertices),
        )
    };
    gl.buffer_data_bytes(glow::ARRAY_BUFFER, vertex_bytes, glow::STATIC_DRAW);
    gl.enable_vertex_attrib_array(0);
    gl.vertex_attrib_pointer(0, 2, glow::FLOAT, false, 0, 0);
    measure("draw_arrays/4-verts", 100_000, 0, || {
        gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    });

    measure("state/bind-texture", 200_000, 0, || {
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.bind_texture(glow::TEXTURE_2D, None);
    });
    measure("state/blend-func", 200_000, 0, || {
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
    });
    measure("state/viewport", 200_000, 0, || {
        gl.viewport(0, 0, WIDTH, HEIGHT);
    });

    measure("clear/1280x720", 1000, (WIDTH * HEIGHT * 4) as u64, || {
        gl.clear_color(0.05, 0.06, 0.08, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);
    });

    let frame_bytes = (WIDTH * HEIGHT * 4) as u64;
    measure("read_pixels/1280x720", 100, frame_bytes, || {
        let _ = gl.read_default_pixels();
    });

    {
        for _ in 0..WARMUP {
            gl.present(WIDTH, HEIGHT, || {
                let _ = surface.swap_buffers(&gl_context);
            });
        }
        let start = Instant::now();
        let iterations = 240u64;
        for _ in 0..iterations {
            gl.present(WIDTH, HEIGHT, || {
                let _ = surface.swap_buffers(&gl_context);
            });
        }
        report("present/1280x720", iterations, start.elapsed(), frame_bytes);
    }

    let _ = window;
    Ok(())
}

struct BenchApp {
    ran: bool,
}

impl ApplicationHandler for BenchApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.ran {
            return;
        }
        self.ran = true;
        if let Err(error) = run(event_loop) {
            eprintln!("gfx bench failed: {error}");
        }
        event_loop.exit();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        _event: WindowEvent,
    ) {
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = BenchApp { ran: false };
    event_loop.run_app(&mut app).expect("run");
}
