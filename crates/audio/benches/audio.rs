use std::path::{Path, PathBuf};
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use kaplay_native_audio::{decode, mixer::Mixer, AudioBuffer};
use kaplay_native_pak::AssetSource;

const MIX_FRAMES: usize = 512;
const VOICE_COUNTS: [usize; 5] = [0, 1, 8, 32, 128];

fn workspace_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn tone(sample_rate: u32, channels: u16, seconds: f64) -> Arc<AudioBuffer> {
    let frames = (sample_rate as f64 * seconds).max(1.0) as usize;
    let samples = (0..frames * channels as usize)
        .map(|index| {
            let phase = (index as f64 * 0.017) as f32;
            phase.sin() * 0.4
        })
        .collect();
    Arc::new(AudioBuffer::new(sample_rate, channels, samples))
}

fn mixer_render(c: &mut Criterion) {
    let buffer = tone(48000, 2, 1.0);
    let mut group = c.benchmark_group("audio/mixer/render");
    group.throughput(Throughput::Elements((MIX_FRAMES * 2) as u64));
    for voices in VOICE_COUNTS {
        let mut mixer = Mixer::new(48000);
        for index in 0..voices {
            let pan = if voices <= 1 {
                0.0
            } else {
                (index as f32 / (voices - 1) as f32) * 2.0 - 1.0
            };
            mixer.add_voice(buffer.clone(), 0.5, pan, 1.0, true, 0.0, None);
        }
        let mut out = vec![0.0f32; MIX_FRAMES * 2];
        let mut base = 0u64;
        group.bench_with_input(BenchmarkId::from_parameter(voices), &voices, |b, _| {
            b.iter(|| {
                mixer.render(&mut out, base);
                base += MIX_FRAMES as u64;
            });
        });
    }
    group.finish();
}

fn voice_lifecycle(c: &mut Criterion) {
    let buffer = tone(48000, 2, 0.25);
    let mut out = vec![0.0f32; MIX_FRAMES * 2];
    let mut mixer = Mixer::new(48000);
    c.bench_function("audio/mixer/voice-cycle", |b| {
        b.iter(|| {
            let voice = mixer.add_voice(buffer.clone(), 1.0, 0.0, 1.0, false, 0.0, None);
            mixer.stop(voice);
            mixer.render(&mut out, 0);
            mixer.take_finished();
        });
    });
}

fn decode_assets(c: &mut Criterion) {
    let assets = AssetSource::from_pak(&workspace_path("frax-kaplay.pak")).unwrap();
    let mut group = c.benchmark_group("audio/decode");
    for name in [
        "sounds/fx/03_Claw_03.opus",
        "sounds/music/title.opus",
        "sounds/music/Forest.ogg",
    ] {
        let bytes = assets.read(name).unwrap();
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(name, &bytes, |b, bytes| b.iter(|| decode(bytes)));
    }
    group.finish();
}

fn decode_synthetic(c: &mut Criterion) {
    let pcm: Vec<u8> = (0..48_000 * 2)
        .flat_map(|index| ((index as i16).wrapping_mul(311)).to_le_bytes())
        .collect();
    let mut wav = Vec::with_capacity(pcm.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&(48_000u32 * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend_from_slice(&pcm);

    let mut group = c.benchmark_group("audio/decode");
    group.throughput(Throughput::Bytes(wav.len() as u64));
    group.bench_function("synthetic-wav-1s-mono", |b| b.iter(|| decode(&wav)));
    group.finish();
}

criterion_group!(
    benches,
    mixer_render,
    voice_lifecycle,
    decode_assets,
    decode_synthetic
);
criterion_main!(benches);
