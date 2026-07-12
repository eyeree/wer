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
    simd::{climate_row, elevation_row, vegetation_row},
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

/// Row-shaped kernel benches (phase-6-plan.md §12): the vectorization unit is
/// a 32-cell row (the tile width), so the scalar row cost recorded here at M1
/// is the denominator of every M4 SIMD ledger entry. M4 adds the `simd` twin
/// of each entry via `world_core::simd`; the pair's ratio is the per-kernel
/// speedup the baseline document records.
fn bench_rows(c: &mut Criterion) {
    const ROW: u16 = 32;
    let p = PossibilityVector::neutral();
    let mut group = c.benchmark_group("row32");

    group.bench_function("elevation/scalar", |b| {
        b.iter(|| {
            let mut acc = 0.0f32;
            for cx in 0..ROW {
                acc += elevation(black_box(f64::from(cx) * 8.0), black_box(-678.9), &p);
            }
            acc
        })
    });

    let xs: Vec<f64> = (0..ROW).map(|cx| f64::from(cx) * 8.0).collect();
    group.bench_function("elevation/simd", |b| {
        let mut out = vec![0f32; ROW as usize];
        b.iter(|| {
            elevation_row(black_box(&xs), black_box(-678.9), &p, &mut out);
            out[0]
        })
    });

    let e: Vec<f32> = (0..ROW)
        .map(|cx| elevation(f64::from(cx) * 8.0, -678.9, &p))
        .collect();
    group.bench_function("climate/scalar", |b| {
        b.iter(|| {
            let mut acc = 0.0f32;
            for &ev in &e {
                acc += climate(black_box(ev), &p).temperature;
            }
            acc
        })
    });

    group.bench_function("climate/simd", |b| {
        let mut ts = vec![0f32; ROW as usize];
        let mut ms = vec![0f32; ROW as usize];
        b.iter(|| {
            climate_row(black_box(&e), &p, &mut ts, &mut ms);
            ts[0]
        })
    });

    group.bench_function("vegetation/scalar", |b| {
        let cl = climate(e[0], &p);
        let g = geology(1234.5, -678.9, 0.5);
        let h = hydrology(e[0], 0.03, 150.0, &cl, 0.5, 0.5);
        let s = soils(e[0], 0.03, &g, &cl, &h);
        let biome = classify(e[0], &cl, &h, &s);
        b.iter(|| {
            let mut acc = 0.0f32;
            for _ in 0..ROW {
                acc += vegetation(black_box(biome), &cl, &s, black_box(0.5)).density;
            }
            acc
        })
    });

    group.bench_function("vegetation/simd", |b| {
        let cl = climate(e[0], &p);
        let g = geology(1234.5, -678.9, 0.5);
        let h = hydrology(e[0], 0.03, 150.0, &cl, 0.5, 0.5);
        let s = soils(e[0], 0.03, &g, &cl, &h);
        let biome = classify(e[0], &cl, &h, &s).id();
        let biomes = vec![biome; ROW as usize];
        let ts = vec![cl.temperature; ROW as usize];
        let ms = vec![cl.moisture; ROW as usize];
        let ds = vec![s.depth; ROW as usize];
        let fs = vec![s.fertility; ROW as usize];
        let mut dens = vec![0f32; ROW as usize];
        let mut cans = vec![0f32; ROW as usize];
        b.iter(|| {
            vegetation_row(
                black_box(&biomes),
                &ts,
                &ms,
                &ds,
                &fs,
                black_box(0.5),
                &mut dens,
                &mut cans,
            );
            dens[0]
        })
    });

    group.finish();
}

criterion_group!(benches, bench_generation, bench_rows);
criterion_main!(benches);
