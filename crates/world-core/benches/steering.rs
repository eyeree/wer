//! Criterion benchmarks for the Phase 4 steering kernels (phase-4-plan.md §13).
//! Built in CI (clippy `--all-targets`) but not gated on timing; run locally
//! with `cargo bench -p world-core` to confirm steering stays inside the
//! unbudgeted `retarget` pass and to calibrate the anchor combination cost.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use world_core::{
    bound_target, capture_target, category_mask, domain_mask, project_plausible, steer, Anchor,
    AnchorKind, AnchorSource, PossibilityDomain, PossibilityVector, TraitCategory, TraitDeviation,
};

/// A representative anchor set: several overlapping emphasize/suppress anchors,
/// the shape `steer` combines each frame per region.
fn representative_anchors() -> Vec<Anchor> {
    let mask_a = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Aesthetics]);
    let mask_b = domain_mask(&[PossibilityDomain::Aesthetics, PossibilityDomain::Morphology]);
    let mask_c = domain_mask(&[PossibilityDomain::Climate, PossibilityDomain::Hydrology]);
    vec![
        Anchor {
            world_pos: (64.0, -32.0),
            target: bound_target(mask_a, 0.9),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.8,
            falloff_radius: 2000.0,
            source: AnchorSource::Manual,
        },
        Anchor {
            world_pos: (-120.0, 80.0),
            target: bound_target(mask_b, 0.2),
            mask: mask_b,
            kind: AnchorKind::Suppress,
            strength: 0.6,
            falloff_radius: 1600.0,
            source: AnchorSource::Manual,
        },
        Anchor {
            world_pos: (30.0, 200.0),
            target: bound_target(mask_c, 0.7),
            mask: mask_c,
            kind: AnchorKind::Emphasize,
            strength: 0.5,
            falloff_radius: 2400.0,
            source: AnchorSource::Manual,
        },
    ]
}

fn bench_steering(c: &mut Criterion) {
    let base = PossibilityVector::neutral();
    let anchors = representative_anchors();

    c.bench_function("steer", |b| {
        b.iter(|| steer(black_box(base), black_box(&anchors), black_box((0.0, 0.0))))
    });

    // A vector that trips every section-8 rule, so projection does real work.
    let mut wild = PossibilityVector::neutral();
    wild.set(PossibilityDomain::Hydrology, 1.0);
    wild.set(PossibilityDomain::Ecology, 1.0);
    wild.set(PossibilityDomain::Morphology, 1.0);
    c.bench_function("project_plausible", |b| {
        b.iter(|| project_plausible(black_box(wild)))
    });

    let mut deviation = TraitDeviation::zero();
    deviation.set(PossibilityDomain::Morphology, 0.8);
    deviation.set(PossibilityDomain::Aesthetics, -0.4);
    let mask = category_mask(&[TraitCategory::Morphology, TraitCategory::Coloration]);
    c.bench_function("capture_target", |b| {
        b.iter(|| {
            capture_target(
                black_box(base),
                black_box(deviation),
                black_box(mask),
                black_box(0.5),
            )
        })
    });
}

criterion_group!(benches, bench_steering);
criterion_main!(benches);
