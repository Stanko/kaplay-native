use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::Thread;
use crate::{Engine, BLOCK_FRAMES};

pub fn start(engine: Arc<Engine>) -> Thread {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut block = vec![0.0f32; BLOCK_FRAMES * 2];
        let sleep = std::time::Duration::from_micros(
            BLOCK_FRAMES as u64 * 1_000_000 / engine.sample_rate() as u64,
        );
        while !flag.load(Ordering::Relaxed) {
            engine.render(&mut block);
            std::thread::sleep(sleep);
        }
    });
    Thread::new(stop, handle)
}
