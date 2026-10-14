#[cfg(feature = "quickjs")]
pub mod quickjs;
#[cfg(feature = "v8")]
pub mod v8_engine;

use crate::state::HostState;

pub(crate) fn handle(value: Option<u32>) -> i32 {
    value.map_or(-1, |value| value as i32)
}

pub(crate) fn option_handle(handle: i32) -> Option<u32> {
    if handle < 0 {
        None
    } else {
        Some(handle as u32)
    }
}

pub(crate) fn source_size(state: &HostState, kind: i32, id: u32) -> (u32, u32) {
    if kind == 0 {
        state
            .images
            .borrow()
            .get(&id)
            .map_or((0, 0), |image| (image.width, image.height))
    } else {
        state.canvas.borrow().canvas_size(id).unwrap_or((0, 0))
    }
}

pub(crate) fn parse_headers(json: &str) -> Vec<(String, String)> {
    serde_json::from_str(json).unwrap_or_default()
}
