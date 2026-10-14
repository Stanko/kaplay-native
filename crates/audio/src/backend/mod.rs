use std::sync::Arc;

use crate::Engine;

#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;

#[cfg(target_os = "linux")]
mod alsa;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod cpal_backend;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use cpal::traits::StreamTrait;

#[cfg(target_os = "linux")]
pub struct Thread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Thread {
    pub fn new(stop: Arc<AtomicBool>, handle: JoinHandle<()>) -> Self {
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub enum Handle {
    #[cfg(target_os = "linux")]
    Alsa(Thread),
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    Cpal(Box<cpal::Stream>),
    Silent,
}

impl Drop for Handle {
    fn drop(&mut self) {
        match self {
            Handle::Silent => {}
            #[cfg(target_os = "linux")]
            Handle::Alsa(thread) => thread.shutdown(),
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            Handle::Cpal(stream) => {
                let _ = stream.pause();
            }
        }
    }
}

pub fn preferred_sample_rate() -> Option<u32> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        cpal_backend::preferred_sample_rate()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

pub fn start(engine: Arc<Engine>) -> Option<Handle> {
    #[cfg(target_os = "linux")]
    {
        match alsa::start(engine.clone()) {
            Ok(thread) => return Some(Handle::Alsa(thread)),
            Err(error) => log::warn!("audio output unavailable: {error}"),
        }
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        match cpal_backend::start(engine) {
            Ok(stream) => return Some(Handle::Cpal(Box::new(stream))),
            Err(error) => log::warn!("audio output unavailable: {error}"),
        }
    }
    let _ = engine;
    None
}

pub fn silent() -> Handle {
    Handle::Silent
}
