use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use kaplay_native_audio::Audio;
use kaplay_native_canvas2d::Canvas2d;
use kaplay_native_gfx::GlState;
use kaplay_native_net::{HttpResponse, Network};
use kaplay_native_pak::AssetSource;
use kaplay_native_steam::SteamApi;
use winit::window::Window;

pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct PendingMessage {
    pub socket: u32,
    pub data: Vec<u8>,
}

pub struct HostState {
    pub gl: Rc<RefCell<GlState>>,
    pub canvas: Rc<RefCell<Canvas2d>>,
    pub images: RefCell<HashMap<u32, ImageData>>,
    pub next_image: Cell<u32>,
    pub assets: Rc<AssetSource>,
    pub window: Arc<Window>,
    pub quit: Cell<bool>,
    pub fullscreen: Cell<bool>,
    pub cursor_locked: Cell<bool>,
    pub cursor: RefCell<String>,
    pub start: Instant,
    pub save_path: PathBuf,
    pub platform: &'static str,
    pub host_version: &'static str,
    pub allow_network: bool,
    pub start_url: String,
    pub network: Option<Rc<Network>>,
    pub audio: Option<Rc<Audio>>,
    pub pending_response: RefCell<Option<HttpResponse>>,
    pub pending_message: RefCell<Option<PendingMessage>>,
    pub game_name: String,
    pub steam: Rc<RefCell<Option<SteamApi>>>,
}

impl HostState {
    pub fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    pub fn store_image(&self, width: u32, height: u32, pixels: Vec<u8>) -> u32 {
        let id = self.next_image.get();
        self.next_image.set(id + 1);
        self.images.borrow_mut().insert(
            id,
            ImageData {
                width,
                height,
                pixels,
            },
        );
        id
    }

    pub fn decode_image(&self, bytes: &[u8]) -> Option<u32> {
        let image = image::load_from_memory(bytes).ok()?;
        let rgba = image.to_rgba8();
        Some(self.store_image(rgba.width(), rgba.height(), rgba.into_raw()))
    }

    pub fn load_storage(&self) -> String {
        std::fs::read_to_string(&self.save_path).unwrap_or_default()
    }

    pub fn save_storage(&self, data: &str) {
        if let Some(parent) = self.save_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let temporary = self.save_path.with_extension("tmp");
        if std::fs::write(&temporary, data).is_ok() {
            let _ = std::fs::rename(&temporary, &self.save_path);
        }
    }

    pub fn set_cursor(&self, cursor: &str) {
        log::debug!("cursor: {cursor}");
        *self.cursor.borrow_mut() = cursor.to_string();
        if cursor == "none" {
            self.window.set_cursor_visible(false);
            return;
        }
        self.window.set_cursor_visible(true);
        self.window.set_cursor(cursor_icon(cursor));
    }

    pub fn set_cursor_locked(&self, locked: bool) {
        self.cursor_locked.set(locked);
        if locked {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });
            self.window.set_cursor_visible(false);
        } else {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
    }
}

fn cursor_icon(cursor: &str) -> winit::window::CursorIcon {
    use winit::window::CursorIcon;
    match cursor {
        "crosshair" => CursorIcon::Crosshair,
        "pointer" => CursorIcon::Pointer,
        "text" => CursorIcon::Text,
        "move" => CursorIcon::Move,
        "not-allowed" => CursorIcon::NotAllowed,
        "grab" => CursorIcon::Grab,
        "grabbing" => CursorIcon::Grabbing,
        "ew-resize" => CursorIcon::EwResize,
        "ns-resize" => CursorIcon::NsResize,
        "nesw-resize" => CursorIcon::NeswResize,
        "nwse-resize" => CursorIcon::NwseResize,
        "wait" => CursorIcon::Wait,
        "none" => CursorIcon::Default,
        _ => CursorIcon::Default,
    }
}
