//! Determinism tests — the golden fixtures that guard against accidental drift
//! in the world-generation algorithms (section 19 & 23.5 of the plan).
//!
//! If a change here fails, either it was unintended (a real determinism bug) or
//! it was intentional, in which case `WORLD_ALGORITHM_VERSION` must be bumped and
//! the golden constants below updated in the same commit.

use world_core::{
    feature_hash, splitmix64, FeatureKey, PossibilityDomain, PossibilityVector, RegionCoord, Rng,
    REGION_SIZE, WORLD_ALGORITHM_VERSION,
};

fn sample_key() -> FeatureKey {
    FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region: RegionCoord::new(-3, 7),
        layer: 4,
        feature_index: 42,
        possibility_revision: 2,
    }
}

// --- Golden fixtures (regenerate deliberately, never casually) --------------

#[test]
fn splitmix64_golden() {
    // Known-answer values for the splitmix64 finalizer.
    assert_eq!(splitmix64(0), 0xE220A8397B1DCDAF);
    assert_eq!(splitmix64(1), 0x910A2DEC89025CC1);
}

#[test]
fn feature_hash_golden() {
    assert_eq!(feature_hash(&sample_key()), 0x9758_0851_66D4_6452);
}

// --- Determinism / purity ---------------------------------------------------

#[test]
fn feature_hash_is_pure() {
    let k = sample_key();
    assert_eq!(feature_hash(&k), feature_hash(&k));
    assert_eq!(k.hash(), feature_hash(&k));
}

#[test]
fn feature_hash_separates_every_field() {
    let base = sample_key();
    let variants = [
        FeatureKey { world_version: base.world_version + 1, ..base },
        FeatureKey { region: RegionCoord::new(base.region.x + 1, base.region.y), ..base },
        FeatureKey { region: RegionCoord::new(base.region.x, base.region.y + 1), ..base },
        FeatureKey { region: RegionCoord::at_level(base.region.x, base.region.y, 1), ..base },
        FeatureKey { layer: base.layer + 1, ..base },
        FeatureKey { feature_index: base.feature_index + 1, ..base },
        FeatureKey { possibility_revision: base.possibility_revision + 1, ..base },
    ];
    let base_hash = feature_hash(&base);
    for v in variants {
        assert_ne!(feature_hash(&v), base_hash, "field change did not affect hash: {v:?}");
    }
}

#[test]
fn rng_from_key_is_reproducible() {
    let k = sample_key();
    let mut a = Rng::from_key(&k);
    let mut b = Rng::from_key(&k);
    for _ in 0..1000 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}

#[test]
fn rng_next_f32_in_unit_interval() {
    let mut rng = Rng::new(0xDEAD_BEEF);
    for _ in 0..100_000 {
        let x = rng.next_f32();
        assert!((0.0..1.0).contains(&x), "out of range: {x}");
    }
}

#[test]
fn rng_next_below_respects_bound() {
    let mut rng = Rng::new(12345);
    for _ in 0..100_000 {
        assert!(rng.next_below(10) < 10);
    }
    assert_eq!(rng.next_below(0), 0);
}

// --- Coordinate quantization ------------------------------------------------

#[test]
fn from_world_floors_toward_negative_infinity() {
    assert_eq!(RegionCoord::from_world(0.0, 0.0), RegionCoord::new(0, 0));
    assert_eq!(
        RegionCoord::from_world(REGION_SIZE - 0.5, 0.5),
        RegionCoord::new(0, 0)
    );
    assert_eq!(RegionCoord::from_world(REGION_SIZE, 0.0), RegionCoord::new(1, 0));
    // Negative positions must not round toward zero.
    assert_eq!(RegionCoord::from_world(-0.5, -REGION_SIZE), RegionCoord::new(-1, -1));
}

#[test]
fn parent_rounds_toward_negative_infinity() {
    assert_eq!(RegionCoord::new(2, 3).parent(), RegionCoord::at_level(1, 1, 1));
    assert_eq!(RegionCoord::new(-1, -1).parent(), RegionCoord::at_level(-1, -1, 1));
}

// --- Possibility vector -----------------------------------------------------

#[test]
fn lerp_hits_endpoints() {
    let a = PossibilityVector::neutral();
    let mut b = PossibilityVector::neutral();
    b.set(PossibilityDomain::Climate, 1.0);
    assert_eq!(a.lerp(&b, 0.0), a);
    assert_eq!(a.lerp(&b, 1.0), b);
}

#[test]
fn set_clamps_to_unit_interval() {
    let mut v = PossibilityVector::neutral();
    v.set(PossibilityDomain::Ecology, 5.0);
    assert_eq!(v.get(PossibilityDomain::Ecology), 1.0);
    v.set(PossibilityDomain::Ecology, -5.0);
    assert_eq!(v.get(PossibilityDomain::Ecology), 0.0);
}
