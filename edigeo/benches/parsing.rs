//! Benchmarks pour le parsing EDIGEO

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::path::Path;

fn find_fixtures() -> Vec<std::path::PathBuf> {
    let fixtures_dir = Path::new("../../fixtures");
    if !fixtures_dir.exists() {
        return vec![];
    }

    let mut archives = Vec::new();
    for entry in walkdir::WalkDir::new(fixtures_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "bz2") {
            archives.push(path.to_path_buf());
        }
    }
    archives
}

fn bench_parse_single(c: &mut Criterion) {
    let fixtures = find_fixtures();
    if fixtures.is_empty() {
        eprintln!("No fixtures found, skipping benchmark");
        return;
    }

    let archive = &fixtures[0];
    let file_size = std::fs::metadata(archive).map(|m| m.len()).unwrap_or(0);

    let mut group = c.benchmark_group("parse_single");
    group.throughput(Throughput::Bytes(file_size));

    group.bench_with_input(
        BenchmarkId::from_parameter(archive.file_name().unwrap().to_string_lossy()),
        archive,
        |b, path| {
            b.iter(|| {
                let result = edigeo::parse(black_box(path)).unwrap();
                black_box(result)
            })
        },
    );

    group.finish();
}

fn bench_parse_batch(c: &mut Criterion) {
    let fixtures = find_fixtures();
    if fixtures.is_empty() {
        eprintln!("No fixtures found, skipping benchmark");
        return;
    }

    let total_size: u64 = fixtures
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();

    let mut group = c.benchmark_group("parse_batch");
    group.throughput(Throughput::Bytes(total_size));
    group.sample_size(10);

    group.bench_function("all_fixtures", |b| {
        b.iter(|| {
            let mut total_features = 0;
            for archive in &fixtures {
                if let Ok(result) = edigeo::parse(black_box(archive)) {
                    total_features += result.features.values().map(|v| v.len()).sum::<usize>();
                }
            }
            black_box(total_features)
        })
    });

    group.finish();
}

fn bench_parse_parallel(c: &mut Criterion) {
    use rayon::prelude::*;

    let fixtures = find_fixtures();
    if fixtures.is_empty() {
        eprintln!("No fixtures found, skipping benchmark");
        return;
    }

    let total_size: u64 = fixtures
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();

    let mut group = c.benchmark_group("parse_parallel");
    group.throughput(Throughput::Bytes(total_size));
    group.sample_size(10);

    group.bench_function("all_fixtures_parallel", |b| {
        b.iter(|| {
            let total_features: usize = fixtures
                .par_iter()
                .filter_map(|archive| edigeo::parse(black_box(archive)).ok())
                .map(|result| result.features.values().map(|v| v.len()).sum::<usize>())
                .sum();
            black_box(total_features)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse_single,
    bench_parse_batch,
    bench_parse_parallel
);
criterion_main!(benches);
