//! Criterion benchmark for a full `RegionMap::update` tick over a fixed window
//! (phase-1-plan.md section 12) — the number that sizes the per-frame budgets.

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
            field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
        );
    }
    map
}

fn bench_update(c: &mut Criterion) {
    let field = PossibilityField::default();
    let cfg = StreamConfig::default();
    let budget = Budget::per_frame(16.6);

    // Steady state: nothing moves, targets unchanged — the per-frame floor.
    let mut map = settled_map(cfg, &field);
    let bias = [0.0f32; POSSIBILITY_DIMS];
    c.bench_function("region_map_update_steady", |b| {
        b.iter(|| black_box(map.update((0.0, 0.0), &field, &[], &bias, &budget, &InlineExecutor)))
    });

    // Drifting: a standing bias keeps distant regions converging and their
    // climate/ecology layers regenerating — the budgeted worst case.
    let mut map = settled_map(cfg, &field);
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Ecology.index()] = 0.4;
    bias[PossibilityDomain::Climate.index()] = -0.3;
    c.bench_function("region_map_update_drifting", |b| {
        b.iter(|| black_box(map.update((0.0, 0.0), &field, &[], &bias, &budget, &InlineExecutor)))
    });
}

criterion_group!(benches, bench_update);
criterion_main!(benches);
