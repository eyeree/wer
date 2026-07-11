//! Criterion benchmarks for the Phase 3 runtime ecology work (phase-3-plan.md
//! §13): near-field organism realization over a dense region, and a full window
//! settle from cold including L8 and rosters. Built in CI, not timing-gated;
//! run with `cargo bench -p world-runtime` to size `max_realize_organisms` and
//! confirm `LAYER_ECOLOGY.cost`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{GenomeBias, PossibilityField, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{realize_region, Budget, InlineExecutor, RegionMap, StreamConfig};

fn config() -> StreamConfig {
    StreamConfig {
        near_radius: 2.0 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 5.0 * REGION_SIZE,
        unload_radius: 6.0 * REGION_SIZE,
        ..StreamConfig::default()
    }
}

fn settled_map(cfg: StreamConfig, field: &PossibilityField) -> RegionMap {
    let mut map = RegionMap::new(cfg);
    let bias = [0.0f32; POSSIBILITY_DIMS];
    for _ in 0..10 {
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

fn bench_ecology(c: &mut Criterion) {
    let field = PossibilityField::default();
    let cfg = config();
    let map = settled_map(cfg, &field);
    let coord = RegionCoord::new(0, 0);
    let tiles = map.cache().get(coord).expect("settled tiles");

    c.bench_function("realize_region", |b| {
        b.iter(|| {
            realize_region(
                black_box(coord),
                black_box(tiles),
                black_box(map.roster_cache()),
                black_box(GenomeBias::neutral()),
                black_box(0),
                black_box(cfg.field_resolution),
            )
        })
    });

    // Full window settle from cold, including ecology + rosters + realization.
    c.bench_function("window_settle_with_ecology", |b| {
        b.iter_batched(
            || RegionMap::new(cfg),
            |mut map| {
                let bias = [0.0f32; POSSIBILITY_DIMS];
                for _ in 0..10 {
                    map.update(
                        (0.0, 0.0),
                        0.0,
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

criterion_group!(benches, bench_ecology);
criterion_main!(benches);
