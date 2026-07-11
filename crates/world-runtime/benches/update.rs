//! Criterion benchmarks for `RegionMap::update` (phase-2-plan.md §13) — the
//! numbers that size the per-frame cost budget: the steady-state floor, the
//! drifting worst case, a full window settle from cold, and the cost of a
//! world-scale Climate flip rippling through the expression layers.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{PossibilityDomain, PossibilityField, POSSIBILITY_DIMS};
use world_runtime::{Budget, InlineExecutor, RegionMap, StreamConfig};

fn settled_map(cfg: StreamConfig, field: &PossibilityField) -> RegionMap {
    let mut map = RegionMap::new(cfg);
    let bias = [0.0f32; POSSIBILITY_DIMS];
    // Unbudgeted warm-up frames: fill and generate the whole window.
    for _ in 0..8 {
        map.update(
            (0.0, 0.0),
            0.0,
            field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
    }
    map
}

fn bench_update(c: &mut Criterion) {
    let field = PossibilityField::default();
    let cfg = StreamConfig::default();
    let budget = Budget::per_frame(16.6);

    // Steady state: player idle, targets unchanged — the per-frame floor
    // (this is also where the dep-hash check cost would show up if it ever
    // stopped being negligible, phase-2-plan.md §13).
    let mut map = settled_map(cfg, &field);
    let bias = [0.0f32; POSSIBILITY_DIMS];
    c.bench_function("region_map_update_steady", |b| {
        b.iter(|| {
            black_box(map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &budget,
                &InlineExecutor,
                false,
            ))
        })
    });

    // Drifting: a standing bias plus walking-speed travel keeps distant
    // regions converging and their expression layers regenerating — the
    // budgeted worst case (convergence is travel-fueled, ADR 0006).
    let mut map = settled_map(cfg, &field);
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Ecology.index()] = 0.4;
    bias[PossibilityDomain::Climate.index()] = -0.3;
    c.bench_function("region_map_update_drifting", |b| {
        b.iter(|| {
            black_box(map.update(
                (0.0, 0.0),
                8.0,
                &field,
                &[],
                &bias,
                &budget,
                &InlineExecutor,
                false,
            ))
        })
    });

    // Full window settle from cold, unbudgeted (throughput of the whole
    // eight-layer pipeline including macro drainage).
    c.bench_function("region_map_settle_cold", |b| {
        b.iter(|| black_box(settled_map(cfg, &field)))
    });

    // A world-scale Climate bucket flip over a settled window: the §12.3
    // throughput scenario (climate → hydrology → soils → biome → vegetation).
    c.bench_function("climate_flip_ripple", |b| {
        b.iter_batched(
            || settled_map(cfg, &field),
            |mut map| {
                let mut bias = [0.0f32; POSSIBILITY_DIMS];
                bias[PossibilityDomain::Climate.index()] = 0.3;
                for _ in 0..40 {
                    map.update(
                        (0.0, 0.0),
                        25.0,
                        &field,
                        &[],
                        &bias,
                        &Budget::unlimited(),
                        &InlineExecutor,
                        false,
                    );
                }
                black_box(map)
            },
            criterion::BatchSize::LargeInput,
        )
    });
}

criterion_group!(benches, bench_update);
criterion_main!(benches);
