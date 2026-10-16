use std::io::Write;
use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use kaplay_native_pak::{normalize, AssetSource};

fn workspace_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn synthetic_pak(entries: usize, entry_bytes: usize, method: zip::CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(method);
    for index in 0..entries {
        writer
            .start_file(format!("assets/file-{index:05}.bin"), options)
            .unwrap();
        writer
            .write_all(&vec![(index % 251) as u8; entry_bytes])
            .unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn open(c: &mut Criterion) {
    let frax = workspace_path("frax-kaplay.pak");
    let frax_size = std::fs::metadata(&frax).unwrap().len();
    let mut group = c.benchmark_group("pak/open");
    group.throughput(Throughput::Bytes(frax_size));
    group.bench_function("frax", |b| {
        b.iter(|| AssetSource::from_pak(std::hint::black_box(&frax)).unwrap());
    });
    group.finish();

    let stored = synthetic_pak(512, 4096, zip::CompressionMethod::Stored);
    let deflated = synthetic_pak(512, 4096, zip::CompressionMethod::Deflated);
    let mut group = c.benchmark_group("pak/open/synthetic-512x4k");
    group.throughput(Throughput::Bytes((512 * 4096) as u64));
    group.bench_function("store", |b| {
        b.iter(|| AssetSource::from_pak_bytes(stored.clone()).unwrap());
    });
    group.bench_function("deflate", |b| {
        b.iter(|| AssetSource::from_pak_bytes(deflated.clone()).unwrap());
    });
    group.finish();
}

fn access(c: &mut Criterion) {
    let assets = AssetSource::from_pak(&workspace_path("frax-kaplay.pak")).unwrap();
    let hit = "sounds/fx/03_Claw_03.opus";
    let mut group = c.benchmark_group("pak/lookup");
    group.bench_function("exists-hit", |b| b.iter(|| assets.exists(hit)));
    group.bench_function("exists-miss", |b| {
        b.iter(|| assets.exists("assets/missing.png"))
    });
    group.bench_function("size-hit", |b| b.iter(|| assets.size(hit)));
    group.finish();

    let mut group = c.benchmark_group("pak/read");
    group.throughput(Throughput::Bytes(assets.size(hit).unwrap() as u64));
    group.bench_function("cached", |b| b.iter(|| assets.read(hit)));
    group.bench_function("decompress", |b| {
        b.iter_batched(
            || AssetSource::from_pak(&workspace_path("frax-kaplay.pak")).unwrap(),
            |assets| assets.read(hit),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn paths(c: &mut Criterion) {
    let long = "/generated/kaplay-sprite-atlas/../atlas-default-0-c0fbc27934.png?rev=2";
    c.bench_function("pak/normalize", |b| {
        b.iter(|| normalize(std::hint::black_box(long)));
    });
}

criterion_group!(benches, open, access, paths);
criterion_main!(benches);
