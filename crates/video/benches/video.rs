use std::path::PathBuf;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use kaplay_native_video::Video;

const FPS: f64 = 30.0;

fn sample_bytes() -> Arc<[u8]> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/video/assets/sample.mp4");
    Arc::from(std::fs::read(path).expect("sample.mp4"))
}

fn decode_sequence(video: &mut Video, duration: f64) {
    let mut time = 0.0;
    while time <= duration {
        for _ in 0..200_000 {
            if video.frame_ready(time) {
                break;
            }
            std::thread::yield_now();
        }
        video.frame_at(time);
        time += 1.0 / FPS;
    }
}

fn benchmarks(c: &mut Criterion) {
    let bytes = sample_bytes();
    let probe = Video::open(bytes.clone()).expect("video");
    let duration = probe.duration();
    let width = probe.width();
    let height = probe.height();
    drop(probe);
    let frame_count = ((duration * FPS).floor() as u64).max(1);

    let mut group = c.benchmark_group("video");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("open", |b| {
        b.iter(|| Video::open(bytes.clone()).expect("video"));
    });

    group.throughput(Throughput::Elements(frame_count));
    group.bench_function("decode-30fps", |b| {
        b.iter_batched(
            || Video::open(bytes.clone()).expect("video"),
            |mut video| decode_sequence(&mut video, duration),
            BatchSize::PerIteration,
        );
    });

    group.throughput(Throughput::Bytes((width as u64) * (height as u64) * 4));
    group.bench_function("frame_at/copy", |b| {
        let mut video = Video::open(bytes.clone()).expect("video");
        while !video.frame_ready(0.0) {
            std::thread::yield_now();
        }
        video.frame_at(0.0);
        b.iter(|| {
            video.frame_at(0.0);
        });
    });

    group.bench_function("frame_ready", |b| {
        let mut video = Video::open(bytes.clone()).expect("video");
        b.iter(|| video.frame_ready(0.5));
    });
    group.finish();
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
