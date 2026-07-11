//! Criterion benchmarks for the per-sample generation kernels and the macro
//! drainage tile (phase-2-plan.md §13). Built in CI (clippy `--all-targets`)
//! but not gated on timing; run locally with `cargo bench -p world-core` to
//! calibrate `LayerDecl.cost` and the per-frame cost budget.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{
    biome::classify,
    climate,
    drainage::{drainage, MACRO_LEVEL},
    elevation,
    geology::geology,
    hydrology::hydrology,
    soils::soils,
    vegetation::vegetation,
    PossibilityField, PossibilityVector, RegionCoord,
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

    c.bench_function("geology", |b| {
        b.iter(|| geology(black_box(1234.5), black_box(-678.9), black_box(0.5)))
    });

    let cl = climate(e, &p);
    let g = geology(1234.5, -678.9, 0.5);
    c.bench_function("hydrology", |b| {
        b.iter(|| {
            hydrology(
                black_box(e),
                black_box(0.03),
                black_box(150.0),
                black_box(&cl),
                black_box(0.5),
                black_box(0.5),
            )
        })
    });

    let h = hydrology(e, 0.03, 150.0, &cl, 0.5, 0.5);
    c.bench_function("soils", |b| {
        b.iter(|| {
            soils(
                black_box(e),
                black_box(0.03),
                black_box(&g),
                black_box(&cl),
                black_box(&h),
            )
        })
    });

    let s = soils(e, 0.03, &g, &cl, &h);
    c.bench_function("classify", |b| {
        b.iter(|| classify(black_box(e), black_box(&cl), black_box(&h), black_box(&s)))
    });

    let biome = classify(e, &cl, &h, &s);
    c.bench_function("vegetation", |b| {
        b.iter(|| {
            vegetation(
                black_box(biome),
                black_box(&cl),
                black_box(&s),
                black_box(0.5),
            )
        })
    });

    c.bench_function("possibility_field_sample", |b| {
        b.iter(|| field.sample(black_box(RegionCoord::new(-37, 74))))
    });

    // The expensive macro job (calibrates LayerDecl::cost for drainage).
    c.bench_function("drainage_macro_tile", |b| {
        b.iter(|| {
            drainage(
                black_box(RegionCoord::at_level(0, 0, MACRO_LEVEL)),
                black_box(&field),
                black_box(0),
            )
        })
    });
}

criterion_group!(benches, bench_generation);
criterion_main!(benches);
