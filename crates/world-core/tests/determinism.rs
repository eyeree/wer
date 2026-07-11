//! Determinism tests — the golden fixtures that guard against accidental drift
//! in the world-generation algorithms (section 19 & 23.5 of the plan).
//!
//! If a change here fails, either it was unintended (a real determinism bug) or
//! it was intentional, in which case `WORLD_ALGORITHM_VERSION` must be bumped
//! (or, for a single layer's tuning, that layer's `algorithm_revision` —
//! phase-2-plan.md §9.2) and the golden constants below updated in the same
//! commit. All fixtures were re-blessed exactly once at the Phase 2 M1 version
//! bump 1 → 2 (phase-2-plan.md §9.1).

use world_core::{
    anchor::{domain_mask, project_plausible, steer, Anchor, AnchorKind},
    biome::{classify, Biome},
    climate::climate,
    dephash::{drainage_dep_hash_default, layer_dep_hash},
    drainage::{drainage, tiebreak_hash, MACRO_APRON, MACRO_LEVEL},
    elevation, feature_hash,
    geology::{geology, lithology_seed},
    hydrology::hydrology,
    layer::LAYER_CLIMATE,
    possibility_field::PossibilityField,
    soils::soils,
    splitmix64,
    terrain::gradient_seed,
    vegetation::vegetation,
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
    assert_eq!(feature_hash(&sample_key()), 0xEA82_857D_C015_4ED2);
}

/// A deliberately non-neutral vector used by the generation fixtures.
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
    // and wasm (phase-2-plan.md §9.3 and §12.5).
    assert_eq!(gradient_seed(0, 0, 0), 0x1993_0AB0_C271_3FAC);
    assert_eq!(gradient_seed(3, -2, 1), 0x557D_9B95_E708_EDFF);
    assert_eq!(gradient_seed(-7, 11, 4), 0x57D0_A861_F9D8_56E4);
}

#[test]
fn elevation_golden() {
    let neutral = PossibilityVector::neutral();
    let skew = skewed_vector();
    assert_eq!(elevation(0.0, 0.0, &neutral), 27.215008);
    assert_eq!(elevation(300.0, -10.0, &neutral), 31.078772);
    assert_eq!(elevation(-12800.0, 7040.0, &skew), 105.49782);
}

#[test]
fn climate_golden() {
    let neutral = PossibilityVector::neutral();
    let skew = skewed_vector();

    let e0 = elevation(300.0, -10.0, &neutral);
    let c0 = climate(e0, &neutral);
    assert_eq!(c0.temperature, 12.297988);
    assert_eq!(c0.moisture, 0.55013704);

    let e1 = elevation(-12800.0, 7040.0, &skew);
    let c1 = climate(e1, &skew);
    assert_eq!(c1.temperature, 22.314264);
    assert_eq!(c1.moisture, 0.29060173);

    // The water branch saturates.
    assert_eq!(climate(-25.0, &neutral).moisture, 1.0);
}

#[test]
fn possibility_field_golden() {
    let f = PossibilityField::new(8);
    // Integer control-point identities (native↔wasm parity surface).
    assert_eq!(f.control_point_seed(0, 0), 0x3BCA_A0D4_114B_8B01);
    assert_eq!(f.control_point_seed(-5, 9), 0xAAF0_551F_3E6F_1A1C);

    // Sampling exactly at a control point reproduces it.
    let cp = f.control_point(-5, 9);
    assert_eq!(f.sample(RegionCoord::new(-40, 72)), cp);
    assert_eq!(
        cp.dims,
        [
            0.39362913,
            0.075211644,
            0.8385061,
            0.93864495,
            0.566788,
            0.84250957,
            0.8172563,
            0.13781202
        ]
    );

    // An interpolated (off-lattice) sample.
    assert_eq!(
        f.sample(RegionCoord::new(-37, 74)).dims,
        [
            0.33075798, 0.35141626, 0.5980482, 0.62507087, 0.41861656, 0.5644254, 0.8817282,
            0.18277985
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

// --- Phase 2 golden fixtures (phase-2-plan.md §12.1) -------------------------

#[test]
fn lithology_seed_golden() {
    // Integer identities behind rock provinces (parity-tested on wasm).
    assert_eq!(lithology_seed(0, 0), 0x0DB8_938A_789C_D47B);
    assert_eq!(lithology_seed(3, -2), 0x61DD_60E4_EEF6_FD16);
    assert_eq!(lithology_seed(-11, 7), 0xBDE3_EA42_4CE8_839D);
}

#[test]
fn geology_golden() {
    let g0 = geology(300.0, -10.0, 0.5);
    assert_eq!(g0.lithology, 3);
    assert_eq!(g0.hardness, 0.6288269);
    let g1 = geology(-9000.0, 4500.0, 0.9);
    assert_eq!(g1.lithology, 1);
    assert_eq!(g1.hardness, 0.99771243);
}

#[test]
fn drainage_tiebreak_golden() {
    // Integer identities behind routing tie-breaks (parity-tested on wasm).
    assert_eq!(tiebreak_hash(0, 0), 0xBB1D_6718_89AD_3CF2);
    assert_eq!(tiebreak_hash(-3, 17), 0xE78D_737F_316B_00E9);
}

#[test]
fn drainage_routing_golden() {
    // The full routing outcome of one fixed macro tile: flow directions of the
    // 8×8 core corner printed as a fixture, one accumulation row, and the
    // order-stable hash of the whole tile. Routing is all-integer, so these
    // must be exactly reproducible (ADR 0009).
    let field = PossibilityField::default();
    let mc = RegionCoord::at_level(0, 0, MACRO_LEVEL);
    let tile = drainage(mc, &field, drainage_dep_hash_default(mc));

    let a = MACRO_APRON as usize;
    let mut dirs = String::new();
    for gy in a..a + 8 {
        for gx in a..a + 8 {
            dirs.push(char::from(b'0' + tile.flow_dir_at(gx, gy)));
        }
        dirs.push('|');
    }
    assert_eq!(
        dirs,
        "84655322|64422222|65121122|11211212|10220201|01221270|11223127|11222321|"
    );

    let accum_row: Vec<u32> = (a..a + 8).map(|gx| tile.accum_at(gx, a + 4)).collect();
    assert_eq!(accum_row, [1, 2, 5, 1, 5, 11, 1, 13]);

    assert_eq!(tile.content_hash(), 0x3D3C_D818_31BC_900B);

    // Quantized routing elevations are integer identities too (§9.3).
    assert_eq!(
        world_core::drainage::routing_elevation_cm(&field, 0, 0),
        5120
    );
    assert_eq!(
        world_core::drainage::routing_elevation_cm(&field, -37, 74),
        2208
    );
}

#[test]
fn hydrology_soils_biome_vegetation_golden() {
    let neutral = PossibilityVector::neutral();
    let e0 = elevation(300.0, -10.0, &neutral);
    let c0 = climate(e0, &neutral);
    let g0 = geology(300.0, -10.0, 0.5);

    let h0 = hydrology(e0, 0.03, 150.0, &c0, 0.55, 0.45);
    assert_eq!(h0.river, 0.7307622);
    assert_eq!(h0.wetness, 0.5990911);

    let s0 = soils(e0, 0.03, &g0, &c0, &h0);
    assert_eq!(s0.depth, 0.6676073);
    assert_eq!(s0.fertility, 0.54137903);

    // A 150-region catchment is a real channel at this moisture.
    assert_eq!(classify(e0, &c0, &h0, &s0), Biome::River);

    let v0 = vegetation(Biome::TemperateForest, &c0, &s0, 0.7);
    assert_eq!(v0.density, 0.65013707);
    assert_eq!(v0.canopy_height, 20.266144);
}

#[test]
fn classify_override_branches_golden() {
    // Every override branch of the classifier (phase-2-plan.md §12.1).
    let c = |t: f32, m: f32| world_core::Climate {
        temperature: t,
        moisture: m,
    };
    let h = |r: f32, w: f32| world_core::Hydrology {
        river: r,
        wetness: w,
    };
    let s = |d: f32, f: f32| world_core::Soils {
        depth: d,
        fertility: f,
    };
    let mid_h = h(0.0, 0.3);
    let mid_s = s(0.6, 0.5);
    assert_eq!(classify(-1.0, &c(20.0, 1.0), &mid_h, &mid_s), Biome::Ocean);
    assert_eq!(
        classify(100.0, &c(-15.0, 0.5), &h(0.9, 0.9), &mid_s),
        Biome::Ice
    );
    assert_eq!(
        classify(100.0, &c(20.0, 0.5), &h(0.7, 0.5), &mid_s),
        Biome::River
    );
    assert_eq!(
        classify(100.0, &c(20.0, 0.5), &h(0.1, 0.8), &mid_s),
        Biome::Wetland
    );
    assert_eq!(
        classify(100.0, &c(-5.0, 0.5), &mid_h, &mid_s),
        Biome::Tundra
    );
    assert_eq!(classify(900.0, &c(10.0, 0.5), &mid_h, &mid_s), Biome::Bare);
    assert_eq!(
        classify(100.0, &c(25.0, 0.1), &mid_h, &mid_s),
        Biome::Desert
    );
    assert_eq!(classify(100.0, &c(0.0, 0.5), &mid_h, &mid_s), Biome::Taiga);
    assert_eq!(
        classify(100.0, &c(25.0, 0.85), &mid_h, &mid_s),
        Biome::Rainforest
    );
    assert_eq!(
        classify(100.0, &c(12.0, 0.6), &mid_h, &mid_s),
        Biome::TemperateForest
    );
    assert_eq!(
        classify(100.0, &c(12.0, 0.35), &mid_h, &mid_s),
        Biome::Shrubland
    );
    assert_eq!(
        classify(100.0, &c(12.0, 0.22), &mid_h, &mid_s),
        Biome::Grassland
    );
    // Shallow soil demotes forest.
    assert_eq!(
        classify(100.0, &c(12.0, 0.6), &mid_h, &s(0.1, 0.2)),
        Biome::Shrubland
    );
}

#[test]
fn layer_dep_hash_golden() {
    // A fixed dependency chain (phase-2-plan.md §12.1): fold order is part of
    // the stable contract.
    assert_eq!(
        layer_dep_hash(
            RegionCoord::new(-3, 7),
            LAYER_CLIMATE,
            0,
            &[100, 2000, 3000],
            &[0x1111_2222_3333_4444],
            32,
        ),
        0xBEFC_4F8C_B321_8610
    );
    assert_eq!(
        drainage_dep_hash_default(RegionCoord::at_level(0, 0, MACRO_LEVEL)),
        0x1902_F38A_1E6A_30A6
    );
}

#[test]
fn quantization_round_trips_at_bucket_edges() {
    use world_core::POSSIBILITY_QUANT;
    for bucket in [
        0u16,
        1,
        2047,
        2048,
        POSSIBILITY_QUANT - 2,
        POSSIBILITY_QUANT - 1,
    ] {
        let v = PossibilityVector::dequantize(bucket);
        let mut p = PossibilityVector::neutral();
        p.set(PossibilityDomain::Hydrology, v);
        assert_eq!(p.quantized(PossibilityDomain::Hydrology), bucket);
    }
    // Exact bucket boundaries land in the upper bucket (floor semantics).
    let mut p = PossibilityVector::neutral();
    p.set(PossibilityDomain::Hydrology, 0.5);
    assert_eq!(
        p.quantized(PossibilityDomain::Hydrology),
        POSSIBILITY_QUANT / 2
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
