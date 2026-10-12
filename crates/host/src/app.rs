use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::prelude::{GlSurface, NotCurrentGlContext};
use glutin::surface::{Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use kaplay_native_canvas2d::Canvas2d;
use kaplay_native_gfx::GlState;
use kaplay_native_pak::AssetSource;
use kaplay_native_steam::SteamApi;
use rquickjs::{Context, Function, Runtime};
use serde_json::json;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::Window;

use crate::bridge;
use crate::config::HostConfig;
use crate::keys::KeyState;
use crate::state::HostState;

const SHIM: &str = include_str!("../../../shim/src/shim.js");
const DEFAULT_FONT: &[u8] = include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf");

pub struct HostApp {
    config: HostConfig,
    assets: Rc<AssetSource>,
    steam: Rc<std::cell::RefCell<Option<SteamApi>>>,
    inner: Option<Inner>,
    frames: u32,
}

struct Inner {
    window: Arc<Window>,
    gl_context: PossiblyCurrentContext,
    gl_surface: Surface<WindowSurface>,
    _runtime: Runtime,
    js: Context,
    state: Rc<HostState>,
    keys: KeyState,
    caps_lock: Cell<bool>,
    modifiers: ModifiersState,
    cursor_position: Cell<(f64, f64)>,
}

impl Inner {
    fn drain(&self) {
        self.js.with(|ctx| while ctx.execute_pending_job() {});
    }

    fn dispatch(&self, event: &str, payload: serde_json::Value) {
        let payload = payload.to_string();
        let event = event.to_string();
        let result = self.js.with(|ctx| -> Result<(), String> {
            let dispatch: Function = ctx
                .globals()
                .get("__kapNative_dispatch")
                .map_err(|error| js_error(&ctx, error))?;
            dispatch
                .call::<(String, String), ()>((event.clone(), payload))
                .map_err(|error| js_error(&ctx, error))
        });
        if let Err(error) = result {
            log::error!("dispatch {event}: {error}");
        }
        self.drain();
    }

    fn frame(&self) {
        let now = self.state.now_ms();
        let result = self.js.with(|ctx| -> Result<(), String> {
            let frame: Function = ctx
                .globals()
                .get("__kapNative_onFrame")
                .map_err(|error| js_error(&ctx, error))?;
            frame
                .call::<(f64,), ()>((now,))
                .map_err(|error| js_error(&ctx, error))
        });
        if let Err(error) = result {
            log::error!("frame: {error}");
        }
        self.drain();

        if let Some(api) = self.state.steam.borrow().as_ref() {
            api.run_callbacks();
        }

        let size = self.window.inner_size();
        self.state
            .gl
            .borrow_mut()
            .present(size.width as i32, size.height as i32);
        if let Err(error) = self.gl_surface.swap_buffers(&self.gl_context) {
            log::error!("swap buffers: {error}");
        }
    }
}

fn js_error(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> String {
    let pending: rquickjs::Value = ctx.catch();
    if let Some(exception) = pending.as_exception() {
        let message = exception.message().unwrap_or_default();
        let stack = exception.stack().unwrap_or_default();
        if stack.is_empty() {
            return message;
        }
        return format!("{message}\n{stack}");
    }
    error.to_string()
}

impl HostApp {
    pub fn new(config: HostConfig, assets: Rc<AssetSource>) -> anyhow::Result<Self> {
        let steam = if config.steam { SteamApi::load() } else { None };
        if let (Some(api), Some(app_id)) = (steam.as_ref(), config.app_id) {
            if api.restart_app_if_necessary(app_id) {
                log::info!("relaunching through Steam");
                std::process::exit(0);
            }
        }
        if let Some(api) = steam.as_ref() {
            log::info!(
                "steam library loaded (client running: {})",
                api.is_client_running()
            );
        }
        Ok(Self {
            config,
            assets,
            steam: Rc::new(std::cell::RefCell::new(steam)),
            inner: None,
            frames: 0,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Poll);
        let result = event_loop.run_app(&mut self);
        if let Some(api) = self.steam.borrow_mut().as_mut() {
            api.shutdown();
        }
        result?;
        Ok(())
    }

    fn create(&mut self, event_loop: &ActiveEventLoop) -> Option<Inner> {
        if let Some(api) = self.steam.borrow_mut().as_mut() {
            match api.init() {
                Ok(()) => log::info!("steam initialised"),
                Err(error) => log::warn!("steam unavailable: {error}"),
            }
        }
        let attributes = Window::default_attributes()
            .with_title(self.config.game_name.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.config.window.0 as f64,
                self.config.window.1 as f64,
            ))
            .with_visible(!self.config.headless);

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_depth_size(24)
            .with_stencil_size(8);

        let (window, gl_config) = DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(event_loop, template, |mut configs| configs.next().unwrap())
            .ok()?;
        let window = Arc::new(window?);
        let raw_window_handle = window.window_handle().ok()?.as_raw();
        let display = gl_config.display();

        let context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
            .build(Some(raw_window_handle));
        let not_current =
            unsafe { display.create_context(&gl_config, &context_attributes) }.ok()?;
        let surface_attributes = window.build_surface_attributes(Default::default()).ok()?;
        let surface =
            unsafe { display.create_window_surface(&gl_config, &surface_attributes) }.ok()?;
        let gl_context = not_current.make_current(&surface).ok()?;
        let _ =
            surface.set_swap_interval(&gl_context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()));

        let glow_context = unsafe {
            glow::Context::from_loader_function_cstr(|symbol| {
                display.get_proc_address(symbol) as *const _
            })
        };
        let mut gl = GlState::new(glow_context);
        let size = window.inner_size();
        gl.resize_drawing_buffer(size.width as i32, size.height as i32);

        let save_path = self.config.save_dir.join("storage.json");
        let state = Rc::new(HostState {
            gl: Rc::new(std::cell::RefCell::new(gl)),
            canvas: Rc::new(std::cell::RefCell::new(Canvas2d::new(
                DEFAULT_FONT.to_vec(),
            ))),
            images: std::cell::RefCell::new(std::collections::HashMap::new()),
            next_image: Cell::new(1),
            assets: self.assets.clone(),
            window: window.clone(),
            quit: Cell::new(false),
            fullscreen: Cell::new(self.config.fullscreen),
            cursor_locked: Cell::new(false),
            cursor: std::cell::RefCell::new("default".to_string()),
            start: std::time::Instant::now(),
            save_path,
            platform: std::env::consts::OS,
            host_version: env!("CARGO_PKG_VERSION"),
            allow_network: self.config.allow_network,
            game_name: self.config.game_name.clone(),
            steam: self.steam.clone(),
        });

        if self.config.fullscreen {
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }

        let runtime = Runtime::new().ok()?;
        let js = Context::full(&runtime).ok()?;
        if let Some(app_id) = self.config.app_id {
            log::info!("steam app id: {app_id}");
        }
        let installed = js.with(|ctx| {
            bridge::install(&ctx, state.clone(), SHIM).map_err(|error| error.to_string())
        });
        if let Err(error) = installed {
            log::error!("failed to initialise runtime: {error}");
            event_loop.exit();
            return None;
        }

        let preloads = self.config.preload.clone();
        let script_source = if let Some(script) = &self.config.script {
            std::fs::read_to_string(script).ok()
        } else {
            ["main.js", "game/main.js", "index.js"]
                .iter()
                .find_map(|path| state.assets.read_text(path))
        };
        let mut sources: Vec<String> = Vec::new();
        for preload in &preloads {
            match std::fs::read_to_string(preload) {
                Ok(source) => sources.push(source),
                Err(error) => log::error!("failed to read preload {}: {error}", preload.display()),
            }
        }
        if let Some(source) = script_source {
            sources.push(source);
        }
        for source in sources {
            let result = js.with(|ctx| -> Result<(), String> {
                ctx.eval::<(), _>(source.as_bytes())
                    .map_err(|error| js_error(&ctx, error))
            });
            if let Err(error) = result {
                log::error!("script error: {error}");
            }
            js.with(|ctx| while ctx.execute_pending_job() {});
        }

        Some(Inner {
            window,
            gl_context,
            gl_surface: surface,
            _runtime: runtime,
            js,
            state,
            keys: KeyState::new(),
            caps_lock: Cell::new(false),
            modifiers: ModifiersState::empty(),
            cursor_position: Cell::new((0.0, 0.0)),
        })
    }

    fn capture_screenshot(&self, path: &std::path::Path) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let (width, height, pixels) = inner.state.gl.borrow_mut().read_default_pixels_top_down();
        match crate::screenshots::encode_bytes(&pixels, width, height) {
            Some(png) => {
                if let Err(error) = std::fs::write(path, png) {
                    log::error!("failed to write screenshot: {error}");
                }
            }
            None => log::error!("failed to encode screenshot"),
        }
    }

    fn maybe_exit(&mut self, event_loop: &ActiveEventLoop) {
        let Some(inner) = self.inner.as_ref() else {
            return;
        };
        let finished =
            inner.state.quit.get() || self.config.frames.is_some_and(|limit| self.frames >= limit);
        if finished {
            if let Some(path) = self.config.screenshot.clone() {
                self.capture_screenshot(&path);
            }
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for HostApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.inner.is_some() {
            return;
        }
        self.inner = self.create(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(inner) = self.inner.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let scale = inner.window.scale_factor();
                let logical = size.to_logical::<f64>(scale);
                inner.dispatch(
                    "resize",
                    json!({
                        "width": logical.width,
                        "height": logical.height,
                        "framebufferWidth": size.width,
                        "framebufferHeight": size.height,
                        "dpr": scale,
                    }),
                );
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = inner.window.inner_size();
                let scale = inner.window.scale_factor();
                let logical = size.to_logical::<f64>(scale);
                inner.dispatch(
                    "resize",
                    json!({
                        "width": logical.width,
                        "height": logical.height,
                        "framebufferWidth": size.width,
                        "framebufferHeight": size.height,
                        "dpr": scale,
                    }),
                );
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.logical_key
                    == winit::keyboard::Key::Named(winit::keyboard::NamedKey::CapsLock)
                    && event.state == ElementState::Pressed
                    && !event.repeat
                {
                    inner.caps_lock.set(!inner.caps_lock.get());
                }
                let payload = inner.keys.event(&event);
                let event_name = match event.state {
                    ElementState::Pressed => "keydown",
                    ElementState::Released => "keyup",
                };
                inner.dispatch(
                    event_name,
                    json!({
                        "key": payload.key,
                        "code": payload.code,
                        "repeat": payload.repeat,
                        "shiftKey": inner.modifiers.shift_key(),
                        "ctrlKey": inner.modifiers.control_key(),
                        "altKey": inner.modifiers.alt_key(),
                        "metaKey": inner.modifiers.super_key(),
                        "capsLock": inner.caps_lock.get(),
                    }),
                );
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                inner.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = inner.window.scale_factor();
                let logical = position.to_logical::<f64>(scale);
                let last = inner.cursor_position.get();
                inner.cursor_position.set((logical.x, logical.y));
                inner.dispatch(
                    "mousemove",
                    json!({
                        "clientX": logical.x,
                        "clientY": logical.y,
                        "movementX": logical.x - last.0,
                        "movementY": logical.y - last.1,
                        "dpr": scale,
                    }),
                );
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (name, button) = match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => ("mousedown", 0),
                    (ElementState::Released, MouseButton::Left) => ("mouseup", 0),
                    (ElementState::Pressed, MouseButton::Middle) => ("mousedown", 1),
                    (ElementState::Released, MouseButton::Middle) => ("mouseup", 1),
                    (ElementState::Pressed, MouseButton::Right) => ("mousedown", 2),
                    (ElementState::Released, MouseButton::Right) => ("mouseup", 2),
                    (ElementState::Pressed, MouseButton::Back) => ("mousedown", 3),
                    (ElementState::Released, MouseButton::Back) => ("mouseup", 3),
                    (ElementState::Pressed, MouseButton::Forward) => ("mousedown", 4),
                    (ElementState::Released, MouseButton::Forward) => ("mouseup", 4),
                    (ElementState::Pressed, MouseButton::Other(value)) => {
                        ("mousedown", value as i32)
                    }
                    (ElementState::Released, MouseButton::Other(value)) => {
                        ("mouseup", value as i32)
                    }
                };
                inner.dispatch(name, json!({ "button": button }));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x as f64 * 100.0, y as f64 * 100.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(position) => {
                        (position.x, position.y)
                    }
                };
                inner.dispatch("wheel", json!({ "deltaX": x, "deltaY": y }));
            }
            WindowEvent::Focused(focused) => {
                inner.dispatch(if focused { "focus" } else { "blur" }, json!({}));
            }
            WindowEvent::Occluded(occluded) => {
                inner.dispatch("visibilitychange", json!({ "hidden": occluded }));
            }
            WindowEvent::RedrawRequested => {
                inner.frame();
                self.frames += 1;
                self.maybe_exit(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(inner) = self.inner.as_ref() {
            if inner.state.quit.get() {
                self.maybe_exit(event_loop);
                return;
            }
            inner.window.request_redraw();
        }
    }
}
