use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use libloading::Library;

use super::Thread;
use crate::{Engine, BLOCK_FRAMES};

const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_FORMAT_S16_LE: c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;
const EPIPE: c_int = -32;
const EAGAIN: c_int = -11;
const EBUSY: c_int = -16;

type Pcm = *mut c_void;

type OpenFn = unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int;
type SetParamsFn = unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int;
type WriteFn = unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long;
type RecoverFn = unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int;
type DrainFn = unsafe extern "C" fn(Pcm) -> c_int;
type CloseFn = unsafe extern "C" fn(Pcm) -> c_int;
type StrErrorFn = unsafe extern "C" fn(c_int) -> *const c_char;

struct Api {
    _library: Library,
    open: OpenFn,
    set_params: SetParamsFn,
    write: WriteFn,
    recover: RecoverFn,
    drain: DrainFn,
    close: CloseFn,
    strerror: StrErrorFn,
}

struct PcmHandle(Pcm);

unsafe impl Send for PcmHandle {}

impl Api {
    fn load() -> Result<Api, String> {
        unsafe {
            let library = Library::new("libasound.so.2")
                .map_err(|error| format!("libasound is not available: {error}"))?;
            let open = *library
                .get::<OpenFn>(b"snd_pcm_open\0")
                .map_err(|error| error.to_string())?;
            let set_params = *library
                .get::<SetParamsFn>(b"snd_pcm_set_params\0")
                .map_err(|error| error.to_string())?;
            let write = *library
                .get::<WriteFn>(b"snd_pcm_writei\0")
                .map_err(|error| error.to_string())?;
            let recover = *library
                .get::<RecoverFn>(b"snd_pcm_recover\0")
                .map_err(|error| error.to_string())?;
            let drain = *library
                .get::<DrainFn>(b"snd_pcm_drain\0")
                .map_err(|error| error.to_string())?;
            let close = *library
                .get::<CloseFn>(b"snd_pcm_close\0")
                .map_err(|error| error.to_string())?;
            let strerror = *library
                .get::<StrErrorFn>(b"snd_strerror\0")
                .map_err(|error| error.to_string())?;
            Ok(Api {
                _library: library,
                open,
                set_params,
                write,
                recover,
                drain,
                close,
                strerror,
            })
        }
    }

    fn message(&self, code: c_int) -> String {
        unsafe {
            let pointer = (self.strerror)(code);
            if pointer.is_null() {
                return format!("alsa error {code}");
            }
            CStr::from_ptr(pointer).to_string_lossy().into_owned()
        }
    }
}

pub fn start(engine: Arc<Engine>) -> Result<Thread, String> {
    let api = Api::load()?;
    let pcm = unsafe {
        let mut pcm: Pcm = std::ptr::null_mut();
        let name = c"default";
        let result = (api.open)(&mut pcm, name.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0);
        if result < 0 {
            return Err(format!(
                "cannot open default device: {}",
                api.message(result)
            ));
        }
        let result = (api.set_params)(
            pcm,
            SND_PCM_FORMAT_S16_LE,
            SND_PCM_ACCESS_RW_INTERLEAVED,
            2,
            engine.sample_rate(),
            1,
            100_000,
        );
        if result < 0 {
            let _ = (api.close)(pcm);
            return Err(format!("cannot configure device: {}", api.message(result)));
        }
        PcmHandle(pcm)
    };

    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::spawn(move || run(api, pcm, engine, flag));
    Ok(Thread::new(stop, handle))
}

fn run(api: Api, pcm: PcmHandle, engine: Arc<Engine>, stop: Arc<AtomicBool>) {
    let mut block = vec![0.0f32; BLOCK_FRAMES * 2];
    let mut pcm16 = vec![0i16; BLOCK_FRAMES * 2];
    while !stop.load(Ordering::Relaxed) {
        engine.render(&mut block);
        for (target, sample) in pcm16.iter_mut().zip(block.iter()) {
            *target = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
        }
        let mut offset = 0usize;
        while offset < BLOCK_FRAMES {
            let written = unsafe {
                (api.write)(
                    pcm.0,
                    pcm16.as_ptr().add(offset * 2).cast(),
                    (BLOCK_FRAMES - offset) as c_ulong,
                )
            };
            if written < 0 {
                let code = written as c_int;
                if code == EPIPE || code == EAGAIN || code == EBUSY {
                    unsafe {
                        (api.recover)(pcm.0, code, 1);
                    }
                    continue;
                }
                log::warn!("audio write failed: {}", api.message(code));
                break;
            }
            offset += written as usize;
        }
    }
    unsafe {
        (api.drain)(pcm.0);
        (api.close)(pcm.0);
    }
}
