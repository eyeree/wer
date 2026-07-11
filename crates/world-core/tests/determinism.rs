//! Determinism tests — the golden fixtures that guard against accidental drift
//! in the world-generation algorithms (section 19 & 23.5 of the plan).
//!
//! If a change here fails, either it was unintended (a real determinism bug) or
//! it was intentional, in which case `WORLD_ALGORITHM_VERSION` must be bumped and
//! the golden constants below updated in the same commit.

use world_core::{
    anchor::{domain_mask, project_plausible, steer, Anchor, AnchorKind},
    climate::climate,
    ecology::vegetation_density,
    elevation, feature_hash,
    possibility_field::PossibilityField,
    splitmix64,
    terrain::gradient_seed,
    FeatureKey, PossibilityDomain, PossibilityVector, RegionCoord, Rng, REGION_SIZE,
    WORLD_ALGORITHM_VERSION,
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

/// A deliberately non-neutral vector used by the Phase 1 generation fixtures.
fn skewed_vector() -> PossibilityVector {
    let mut v = PossibilityVector::neutral();
    v.set(PossibilityDomain::Geology, 0.9);
    v.set(PossibilityDomain::Planetary, 0.2);
    v.set(PossibilityDomain::Climate, 0.8);
    v.set(PossibilityDomain::Hydrology, 0.3);
    v.set(PossibilityDomain::Ecology, 0.7);
    v
}

#[test]
fn terrain_gradient_seed_golden() {
    // Integer identities behind topology — must agree bit-for-bit on native
    // and wasm (phase-1-plan.md sections 8 and 11.2).
    assert_eq!(gradient_seed(0, 0, 0), 0xFD7A_DE10_8EE7_E882);
    assert_eq!(gradient_seed(3, -2, 1), 0xB630_958A_7BD1_F867);
    assert_eq!(gradient_seed(-7, 11, 4), 0x1F51_B981_FACB_44B7);
}

#[test]
fn elevation_golden() {
    let neutral = PossibilityVector::neutral();
    let skew = skewed_vector();
    assert_eq!(elevation(0.0, 0.0, &neutral), 37.29465);
    assert_eq!(elevation(300.0, -10.0, &neutral), 53.016655);
    assert_eq!(elevation(-12800.0, 7040.0, &skew), -150.01616);
}

#[test]
fn climate_and_ecology_golden() {
    let neutral = PossibilityVector::neutral();
    let skew = skewed_vector();

    let e0 = elevation(300.0, -10.0, &neutral);
    let c0 = climate(e0, &neutral);
    assert_eq!(c0.temperature, 12.155392);
    assert_eq!(c0.moisture, 0.5325867);
    assert_eq!(vegetation_density(e0, &c0, &neutral), 0.35104322);

    // The skewed fixture point is open water: saturated moisture, no plants.
    let e1 = elevation(-12800.0, 7040.0, &skew);
    let c1 = climate(e1, &skew);
    assert_eq!(c1.temperature, 23.0);
    assert_eq!(c1.moisture, 1.0);
    assert_eq!(vegetation_density(e1, &c1, &skew), 0.0);
}

#[test]
fn possibility_field_golden() {
    let f = PossibilityField::new(8);
    // Integer control-point identities (native↔wasm parity surface).
    assert_eq!(f.control_point_seed(0, 0), 0x6226_D400_B167_0C47);
    assert_eq!(f.control_point_seed(-5, 9), 0xEAFE_6C24_2F6B_03F3);

    // Sampling exactly at a control point reproduces it.
    let cp = f.control_point(-5, 9);
    assert_eq!(f.sample(RegionCoord::new(-40, 72)), cp);
    assert_eq!(
        cp.dims,
        [
            0.61419696, 0.65296286, 0.6780351, 0.9583414, 0.30520362, 0.49428523, 0.17672008,
            0.7128106
        ]
    );

    // An interpolated (off-lattice) sample.
    assert_eq!(
        f.sample(RegionCoord::new(-37, 74)).dims,
        [
            0.5988153, 0.73600864, 0.5436786, 0.7237021, 0.37882164, 0.69600093, 0.32387576,
            0.5879861
        ]
    );
}

#[test]
fn steer_and_project_golden() {
    let anchors = [
        Anchor {
            world_pos: (100.0, 100.0),
            mask: domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Hydrology]),
            kind: AnchorKind::Emphasize,
            strength: 0.8,
            falloff_radius: 2048.0,
        },
        Anchor {
            world_pos: (-500.0, 300.0),
            mask: domain_mask(&[PossibilityDomain::Climate]),
            kind: AnchorKind::Suppress,
            strength: 0.6,
            falloff_radius: 1024.0,
        },
    ];
    let steered = steer(PossibilityVector::neutral(), &anchors, (0.0, 0.0));
    assert_eq!(
        steered.dims,
        [0.5, 0.36300826, 0.5, 0.8961944, 0.8961944, 0.5, 0.5, 0.5]
    );

    let mut wild = PossibilityVector::neutral();
    wild.set(PossibilityDomain::Planetary, 0.1);
    wild.set(PossibilityDomain::Hydrology, 0.9);
    wild.set(PossibilityDomain::Ecology, 1.0);
    assert_eq!(
        project_plausible(wild).dims,
        [0.1, 0.5, 0.5, 0.56, 0.81, 0.5, 0.5, 0.5]
    );
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
        FeatureKey {
            world_version: base.world_version + 1,
            ..base
        },
        FeatureKey {
            region: RegionCoord::new(base.region.x + 1, base.region.y),
            ..base
        },
        FeatureKey {
            region: RegionCoord::new(base.region.x, base.region.y + 1),
            ..base
        },
        FeatureKey {
            region: RegionCoord::at_level(base.region.x, base.region.y, 1),
            ..base
        },
        FeatureKey {
            layer: base.layer + 1,
            ..base
        },
        FeatureKey {
            feature_index: base.feature_index + 1,
            ..base
        },
        FeatureKey {
            possibility_revision: base.possibility_revision + 1,
            ..base
        },
    ];
    let base_hash = feature_hash(&base);
    for v in variants {
        assert_ne!(
            feature_hash(&v),
            base_hash,
            "field change did not affect hash: {v:?}"
        );
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
    assert_eq!(
        RegionCoord::from_world(REGION_SIZE, 0.0),
        RegionCoord::new(1, 0)
    );
    // Negative positions must not round toward zero.
    assert_eq!(
        RegionCoord::from_world(-0.5, -REGION_SIZE),
        RegionCoord::new(-1, -1)
    );
}

#[test]
fn parent_rounds_toward_negative_infinity() {
    assert_eq!(
        RegionCoord::new(2, 3).parent(),
        RegionCoord::at_level(1, 1, 1)
    );
    assert_eq!(
        RegionCoord::new(-1, -1).parent(),
        RegionCoord::at_level(-1, -1, 1)
    );
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
