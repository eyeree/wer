//! Criterion benchmarks for the Phase 3 procedural-genetics kernels
//! (phase-3-plan.md §13): genome derivation, roster construction, food-web
//! projection, and aggregate population sampling. Built in CI but not gated on
//! timing; run with `cargo bench -p world-core` to calibrate `LAYER_ECOLOGY.cost`
//! and the roster-cache sizing.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{biome::Biome, food_web, population, species_roster, Genome, HabitatSignature};

fn rainforest() -> HabitatSignature {
    HabitatSignature {
        biome: Biome::Rainforest.id(),
        temperature_band: 5,
        moisture_band: 4,
        fertility_band: 3,
    }
}

fn bench_ecology(c: &mut Criterion) {
    c.bench_function("genome_from_seed", |b| {
        b.iter(|| Genome::from_seed(black_box(0x1234_5678_9ABC_DEF0)))
    });

    let sig = rainforest();
    c.bench_function("species_roster", |b| {
        b.iter(|| species_roster(black_box(sig)))
    });

    let roster = species_roster(sig);
    c.bench_function("food_web", |b| {
        b.iter(|| food_web(black_box(&roster), black_box(0.8)))
    });

    let web = food_web(&roster, 0.8);
    c.bench_function("population_sample", |b| {
        b.iter(|| {
            population(
                black_box(&roster),
                black_box(&web),
                black_box(0.7),
                black_box(0.6),
            )
        })
    });

    // The M4 hoist pair (phase-6-plan.md §6.3): table build once per
    // signature vs the per-cell remainder.
    let table = world_core::population_table(&roster, &web);
    c.bench_function("population_from_table", |b| {
        b.iter(|| {
            world_core::population_from_table(
                criterion::black_box(&table),
                criterion::black_box(0.7),
                criterion::black_box(0.6),
            )
        })
    });
}

criterion_group!(benches, bench_ecology);
criterion_main!(benches);
