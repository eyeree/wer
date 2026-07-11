//! Criterion benchmarks for the per-sample generation kernels
//! (phase-1-plan.md section 12). Built in CI (clippy `--all-targets`) but not
//! gated on timing; run locally with `cargo bench -p world-core` to size the
//! per-frame budgets.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{
    climate, elevation, vegetation_density, PossibilityField, PossibilityVector, RegionCoord,
};

fn bench_generation(c: &mut Criterion) {
    let p = PossibilityVector::neutral();
    let field = PossibilityField::default();

    c.bench_function("elevation", |b| {
        b.iter(|| elevation(black_box(1234.5), black_box(-678.9), black_box(&p)))
    });

    let e = elevation(1234.5, -678.9, &p);
    c.bench_function("climate", |b| {
        b.iter(|| climate(black_box(e), black_box(&p)))
    });

    let cl = climate(e, &p);
    c.bench_function("vegetation_density", |b| {
        b.iter(|| vegetation_density(black_box(e), black_box(&cl), black_box(&p)))
    });

    c.bench_function("possibility_field_sample", |b| {
        b.iter(|| field.sample(black_box(RegionCoord::new(-37, 74))))
    });
}

criterion_group!(benches, bench_generation);
criterion_main!(benches);
