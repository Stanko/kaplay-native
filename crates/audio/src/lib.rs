mod backend;
mod buffer;
mod decode;
mod mixer;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use buffer::AudioBuffer;
pub use decode::{decode, output_sample_rate, DecodeError};

#[derive(Debug, Clone)]
pub enum AudioEvent {
    Decoded {
        request: u32,
        buffer: u32,
        error: Option<String>,
    },
    VoiceEnded {
        voice: u32,
    },
}

pub const BLOCK_FRAMES: usize = 512;

pub struct Engine {
    mixer: Mutex<mixer::Mixer>,
    events: Mutex<Vec<AudioEvent>>,
    clock: AtomicU64,
    sample_rate: u32,
    paused: AtomicBool,
}

impl Engine {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            mixer: Mutex::new(mixer::Mixer::new(sample_rate)),
            events: Mutex::new(Vec::new()),
            clock: AtomicU64::new(0),
            sample_rate: sample_rate.max(1),
            paused: AtomicBool::new(false),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn current_time(&self) -> f64 {
        self.clock.load(Ordering::Relaxed) as f64 / self.sample_rate as f64
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn push_event(&self, event: AudioEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    pub fn take_events(&self) -> Vec<AudioEvent> {
        self.events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }

    pub fn render(&self, out: &mut [f32]) {
        if self.paused.load(Ordering::Relaxed) {
            out.fill(0.0);
            return;
        }
        let base = self.clock.load(Ordering::Relaxed);
        let finished = {
            let mut mixer = match self.mixer.lock() {
                Ok(mixer) => mixer,
                Err(_) => {
                    out.fill(0.0);
                    return;
                }
            };
            mixer.render(out, base);
            mixer.take_finished()
        };
        self.clock
            .fetch_add((out.len() / 2) as u64, Ordering::Relaxed);
        if !finished.is_empty() {
            if let Ok(mut events) = self.events.lock() {
                events.extend(
                    finished
                        .into_iter()
                        .map(|voice| AudioEvent::VoiceEnded { voice }),
                );
            }
        }
    }

    fn with_mixer<T>(&self, f: impl FnOnce(&mut mixer::Mixer) -> T) -> Option<T> {
        self.mixer.lock().ok().map(|mut mixer| f(&mut mixer))
    }
}

struct Shared {
    engine: Arc<Engine>,
    buffers: Mutex<HashMap<u32, Arc<AudioBuffer>>>,
    next_buffer: AtomicU32,
}

pub struct Audio {
    shared: Arc<Shared>,
    _backend: backend::Handle,
    has_device: bool,
}

impl Audio {
    pub fn new() -> Self {
        let sample_rate = backend::preferred_sample_rate().unwrap_or_else(output_sample_rate);
        let engine = Arc::new(Engine::new(sample_rate));
        let (handle, has_device) = match backend::start(engine.clone()) {
            Some(handle) => (handle, true),
            None => (backend::silent(engine.clone()), false),
        };
        Self {
            shared: Arc::new(Shared {
                engine,
                buffers: Mutex::new(HashMap::new()),
                next_buffer: AtomicU32::new(1),
            }),
            _backend: handle,
            has_device,
        }
    }

    pub fn silent() -> Self {
        let engine = Arc::new(Engine::new(output_sample_rate()));
        let handle = backend::silent(engine.clone());
        Self {
            shared: Arc::new(Shared {
                engine,
                buffers: Mutex::new(HashMap::new()),
                next_buffer: AtomicU32::new(1),
            }),
            _backend: handle,
            has_device: false,
        }
    }

    pub fn has_device(&self) -> bool {
        self.has_device
    }

    pub fn sample_rate(&self) -> u32 {
        self.shared.engine.sample_rate()
    }

    pub fn current_time(&self) -> f64 {
        self.shared.engine.current_time()
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.engine.set_paused(paused);
    }

    pub fn take_events(&self) -> Vec<AudioEvent> {
        self.shared.engine.take_events()
    }

    pub fn decode_async(&self, request: u32, bytes: Vec<u8>) {
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let (buffer, error) = match decode::decode(&bytes) {
                Ok(buffer) => {
                    let id = shared.next_buffer.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut buffers) = shared.buffers.lock() {
                        buffers.insert(id, Arc::new(buffer));
                    }
                    (id, None)
                }
                Err(error) => (0, Some(error.to_string())),
            };
            shared.engine.push_event(AudioEvent::Decoded {
                request,
                buffer,
                error,
            });
        });
    }

    pub fn upload_buffer(&self, sample_rate: u32, channels: u16, samples: Vec<f32>) -> u32 {
        let buffer = Arc::new(AudioBuffer::new(sample_rate, channels, samples));
        let id = self.shared.next_buffer.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut buffers) = self.shared.buffers.lock() {
            buffers.insert(id, buffer);
        }
        id
    }

    pub fn buffer_info(&self, id: u32) -> Option<(u32, u16, usize)> {
        self.shared
            .buffers
            .lock()
            .ok()?
            .get(&id)
            .map(|buffer| (buffer.sample_rate, buffer.channels, buffer.frames))
    }

    pub fn read_channel(&self, id: u32, channel: usize, out: &mut [f32]) -> usize {
        let Some(buffer) = self
            .shared
            .buffers
            .lock()
            .ok()
            .and_then(|b| b.get(&id).cloned())
        else {
            return 0;
        };
        let Some(samples) = buffer.channel(channel) else {
            return 0;
        };
        let count = samples.len().min(out.len());
        out[..count].copy_from_slice(&samples[..count]);
        count
    }

    pub fn free_buffer(&self, id: u32) {
        if let Ok(mut buffers) = self.shared.buffers.lock() {
            buffers.remove(&id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn play_voice(
        &self,
        buffer: u32,
        gain: f32,
        pan: f32,
        rate: f64,
        looping: bool,
        offset_seconds: f64,
        start_at_frame: Option<u64>,
    ) -> Option<u32> {
        let buffer = self
            .shared
            .buffers
            .lock()
            .ok()
            .and_then(|buffers| buffers.get(&buffer).cloned())?;
        self.shared.engine.with_mixer(|mixer| {
            mixer.add_voice(
                buffer,
                gain,
                pan,
                rate,
                looping,
                offset_seconds,
                start_at_frame,
            )
        })
    }

    pub fn set_voice_gain(&self, voice: u32, gain: f32) {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.set_gain(voice, gain));
    }

    pub fn set_voice_pan(&self, voice: u32, pan: f32) {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.set_pan(voice, pan));
    }

    pub fn set_voice_rate(&self, voice: u32, rate: f64) {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.set_rate(voice, rate));
    }

    pub fn set_voice_loop(&self, voice: u32, looping: bool) {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.set_loop(voice, looping));
    }

    pub fn voice_position(&self, voice: u32) -> Option<f64> {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.position(voice))?
    }

    pub fn voice_duration(&self, voice: u32) -> Option<f64> {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.duration(voice))?
    }

    pub fn stop_voice(&self, voice: u32) {
        self.shared.engine.with_mixer(|mixer| mixer.stop(voice));
    }

    pub fn stop_voice_at(&self, voice: u32, frame: u64) {
        self.shared
            .engine
            .with_mixer(|mixer| mixer.stop_at(voice, frame));
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}
