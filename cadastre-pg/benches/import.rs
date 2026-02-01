//! Benchmarks pour l'import PostGIS

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_placeholder(c: &mut Criterion) {
    c.bench_function("import_placeholder", |b| {
        b.iter(|| {
            // TODO: Ajouter les vrais benchmarks d'import
            black_box(1 + 1)
        })
    });
}

criterion_group!(benches, bench_placeholder);
criterion_main!(benches);
