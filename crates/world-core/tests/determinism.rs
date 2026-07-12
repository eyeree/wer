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
    anchor::{
        anchor_set_signature, bound_target, domain_mask, project_plausible, steer, Anchor,
        AnchorKind, AnchorSource,
    },
    attraction_anchors,
    biome::{classify, Biome},
    capture::{capture_target, category_mask, TraitCategory, TraitDeviation},
    climate::climate,
    dephash::{drainage_dep_hash_default, layer_dep_hash},
    drainage::{drainage, tiebreak_hash, MACRO_APRON, MACRO_LEVEL},
    elevation, feature_hash,
    foodweb::food_web,
    genome::Genome,
    geology::{geology, lithology_seed},
    habitat::HabitatSignature,
    hydrology::hydrology,
    layer::LAYER_CLIMATE,
    mix,
    possibility_field::PossibilityField,
    soils::soils,
    species::{species_roster, species_seed, Trophic},
    splitmix64,
    terrain::gradient_seed,
    vegetation::vegetation,
    FeatureKey, PossibilityDomain, PossibilitySignature, PossibilityVector, RegionCoord, Rng,
    RouteNode, RouteRecord, REGION_SIZE, WORLD_ALGORITHM_VERSION,
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
    let mask_a = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Hydrology]);
    let mask_b = domain_mask(&[PossibilityDomain::Climate]);
    let anchors = [
        Anchor {
            world_pos: (100.0, 100.0),
            target: bound_target(mask_a, 1.0),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.8,
            falloff_radius: 2048.0,
            source: AnchorSource::Manual,
        },
        Anchor {
            world_pos: (-500.0, 300.0),
            target: bound_target(mask_b, 1.0),
            mask: mask_b,
            kind: AnchorKind::Suppress,
            strength: 0.6,
            falloff_radius: 1024.0,
            source: AnchorSource::Manual,
        },
    ];
    let steered = steer(PossibilityVector::neutral(), &anchors, (0.0, 0.0));
    // Order-independent combination golden (ADR 0011). For one anchor per domain
    // the saturating blend reduces to the Phase 1 values, so this fixture is
    // stable across the M1 `steer` rewrite (a presentation fixture, not a world
    // identity, §9.1); the order-independence property itself is unit-tested.
    assert_eq!(
        steered.dims,
        [0.5, 0.36300826, 0.5, 0.8961944, 0.8961944, 0.5, 0.5, 0.5]
    );

    let mut wild = PossibilityVector::neutral();
    wild.set(PossibilityDomain::Planetary, 0.1);
    wild.set(PossibilityDomain::Hydrology, 0.9);
    wild.set(PossibilityDomain::Ecology, 1.0);
    // Section-8 rule set (phase-4-plan.md §7.3): rule 1 caps Hydrology by ocean
    // supply (0.56) then rule 5 tightens it to 0.49 in this cool ocean-poor
    // world; rule 2 then caps Ecology by the now-final moisture. A Phase 4
    // presentation fixture (§9.1), not a re-blessed world identity.
    assert_eq!(
        project_plausible(wild).dims,
        [0.1, 0.5, 0.5, 0.48999998, 0.596, 0.5, 0.5, 0.5]
    );
}

#[test]
fn canonical_anchor_set_signature_golden() {
    // Additive ADR 0025 fixture: the exact live steering fields, cardinality,
    // duplicate occurrence, and canonical fold—not a generator or wire golden.
    let ecology = domain_mask(&[PossibilityDomain::Ecology]);
    let living = domain_mask(&[
        PossibilityDomain::Ecology,
        PossibilityDomain::Morphology,
        PossibilityDomain::Aesthetics,
    ]);
    let mut first_target = bound_target(ecology, 0.875);
    first_target.set(PossibilityDomain::Climate, 0.9375);
    let first = Anchor {
        world_pos: (-320.5, 144.25),
        target: first_target,
        mask: ecology,
        kind: AnchorKind::Emphasize,
        strength: 0.625,
        falloff_radius: 1536.0,
        source: AnchorSource::Landform,
    };
    let mut second_target = bound_target(living, 0.1875);
    second_target.set(PossibilityDomain::Climate, 0.03125);
    let second = Anchor {
        world_pos: (96.0, -48.0),
        target: second_target,
        mask: living,
        kind: AnchorKind::Suppress,
        strength: 0.3125,
        falloff_radius: 768.5,
        source: AnchorSource::Atmosphere,
    };
    assert_eq!(
        anchor_set_signature(&[first, second, first]),
        0xBDAA_C72D_CA08_3AF7
    );
}

#[test]
fn aggregate_route_attraction_golden() {
    // Additive ADR 0026 presentation/interop fixture. Existing generator,
    // steering, record-wire, and content-id goldens remain untouched.
    let mut signature = PossibilitySignature {
        buckets: [2048; world_core::POSSIBILITY_DIMS],
    };
    signature.buckets[PossibilityDomain::Ecology.index()] = 3900;
    signature.buckets[PossibilityDomain::Aesthetics.index()] = 3500;
    let node = |x, cost| RouteNode {
        pos_q: (x, 0),
        signature,
        cost_q: cost,
        stability_q: 0,
        anchor_sig: 0,
    };
    let mut first = RouteRecord::new(
        vec![node(0, 10), node(32, 11), node(64, 12), node(96, 13)],
        vec![],
        1,
        String::from("parity-a"),
    );
    first.usage = 3;
    let mut second = RouteRecord::new(
        vec![node(0, 20), node(16, 21), node(48, 22), node(80, 23)],
        vec![],
        2,
        String::from("parity-b"),
    );
    second.usage = 19;
    let anchors = attraction_anchors([&second, &first], (0.0, 0.0), 5);
    let value = project_plausible(steer(PossibilityVector::neutral(), &anchors, (24.0, 0.0)));
    let mut hash = mix(0xA77A_C710_0026_0001, anchors.len() as u64);
    for anchor in anchors {
        hash = mix(hash, u64::from(anchor.strength.to_bits()));
    }
    for dimension in value.dims {
        hash = mix(hash, u64::from(dimension.to_bits()));
    }
    assert_eq!(hash, 0x3D54_75F6_34AF_1C41);
}

#[test]
fn capture_target_golden() {
    // Phase 4 M1 (phase-4-plan.md §12.1): the pure capture math is
    // float-deterministic and parity-exported (via `steer_sample`). A capture
    // nudges the baseline toward the deviation on the masked domains only.
    let baseline = PossibilityVector::neutral();
    let mut deviation = TraitDeviation::zero();
    deviation.set(PossibilityDomain::Morphology, 0.8);
    deviation.set(PossibilityDomain::Aesthetics, -0.4);
    let mask = category_mask(&[TraitCategory::Morphology, TraitCategory::Coloration]);
    let target = capture_target(baseline, deviation, mask, 0.5);
    assert_eq!(target.dims, [0.5, 0.5, 0.5, 0.5, 0.5, 0.9, 0.5, 0.3]);
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

// --- Phase 3 golden fixtures (phase-3-plan.md §12.1) ------------------------

/// A fixed habitat signature used across the genetics/roster fixtures.
fn golden_signature() -> HabitatSignature {
    HabitatSignature {
        biome: Biome::TemperateForest.id(),
        temperature_band: 3,
        moisture_band: 3,
        fertility_band: 2,
    }
}

#[test]
fn habitat_and_species_seed_golden() {
    // Integer identities behind species rosters: portable given a signature
    // (the signature *derivation* is presentation-grade, §9.3, ADR 0010).
    let sig = golden_signature();
    assert_eq!(sig.seed(), 0x4204_1386_32E9_C315);
    assert_eq!(species_seed(sig, 0), 0x2340_6061_75CD_D2D2);
    assert_eq!(species_seed(sig, 5), 0x8FFA_4BC8_DED4_E2BF);
}

#[test]
fn genome_from_seed_golden() {
    // The portable, cross-platform genome surface (§9.3): integer trait words
    // for a fixed seed, plus the fold-order fingerprint parity-tested on wasm.
    let g = Genome::from_seed(0x1234_5678_9ABC_DEF0);
    assert_eq!(g.appearance.hue, 201);
    assert_eq!(g.appearance.luminance, 242);
    assert_eq!(g.appearance.size_class, 0);
    assert_eq!(g.appearance.form, 11);
    assert_eq!(g.behavior.activity, 76);
    assert_eq!(g.behavior.aggression, 9);
    assert_eq!(g.behavior.sociality, 162);
    assert_eq!(g.niche.trophic_tendency, 54);
    assert_eq!(g.niche.diet_breadth, 15);
    assert_eq!(g.niche.temperature_tolerance, 50);
    assert_eq!(g.niche.moisture_tolerance, 50);
    assert_eq!(g.fingerprint(), 0xE76D_2D5A_4C1F_C16B);
}

#[test]
fn species_roster_composition_golden() {
    // Roster size and each species' trophic role for a fixed signature
    // (phase-3-plan.md §12.1). Roles are trophic-sorted: Producer(0),
    // Herbivore(1), Omnivore(2), Carnivore(3), Decomposer(4).
    let roster = species_roster(golden_signature());
    assert_eq!(roster.len(), 10);
    let roles: Vec<u8> = roster.species.iter().map(|s| s.trophic as u8).collect();
    assert_eq!(
        roles,
        vec![
            Trophic::Producer as u8,
            Trophic::Producer as u8,
            Trophic::Producer as u8,
            Trophic::Producer as u8,
            Trophic::Herbivore as u8,
            Trophic::Herbivore as u8,
            Trophic::Herbivore as u8,
            Trophic::Omnivore as u8,
            Trophic::Carnivore as u8,
            Trophic::Decomposer as u8,
        ]
    );
    // Each species' genome derives from its own species seed.
    assert_eq!(roster.species[0].id, species_seed(golden_signature(), 0));
    assert_eq!(
        roster.species[0].genome,
        Genome::from_seed(roster.species[0].id)
    );
}

#[test]
fn food_web_golden() {
    // Tier biomass for a fixed roster is portable `f32` (pure IEEE arithmetic
    // over integer-derived inputs), so it is a cross-platform identity
    // (phase-3-plan.md §12.5). The rainforest roster is the richest, so this
    // exercises a full four-tier web.
    let sig = HabitatSignature {
        biome: Biome::Rainforest.id(),
        temperature_band: 5,
        moisture_band: 4,
        fertility_band: 3,
    };
    let roster = species_roster(sig);
    let web = food_web(&roster, 0.8);
    assert_eq!(
        web.tier_biomass,
        [0.7936508, 0.07936508, 0.007936508, 0.11904762]
    );
    assert_eq!(web.tier_biomass_fingerprint(), 0xDD32_EC75_48F5_38A6);
    assert_eq!(web.edges.len(), 15);
    assert!(web.pruned.is_empty());
    assert_eq!(web.max_body_size, 10.34);
}

// --- Phase 5 golden fixtures (phase-5-plan.md §12.1) ------------------------
//
// These pin the record format: the exact wire bytes (envelope + postcard body,
// including every serde field/variant order), the content-id fold orders, and
// the possibility-signature seed fold. They are NEW fixtures — no Phase 2–4
// identity fixture re-blesses in Phase 5 (§9.1). Changing any of these values
// is a format change and MUST bump RECORD_FORMAT_VERSION with a migration.

/// The fixed discovery used across the record fixtures: a capture of the
/// golden species in the golden habitat, quantized.
fn golden_discovery() -> world_core::DiscoveryRecord {
    use world_core::{bound_target, domain_mask, Anchor, DiscoveryRecord};
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    let anchor = Anchor {
        world_pos: (300.0, -10.0),
        target: bound_target(mask, 0.9),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.8,
        falloff_radius: 1500.0,
        source: AnchorSource::Organism {
            species: species_seed(golden_signature(), 0),
        },
    };
    DiscoveryRecord::from_anchor(
        &anchor,
        golden_signature().seed(),
        7,
        String::from("glowfin"),
    )
}

#[test]
fn possibility_signature_golden() {
    use world_core::PossibilitySignature;
    let sig = PossibilitySignature::of(skewed_vector());
    // The quantized buckets of the skewed vector (the record vocabulary)…
    assert_eq!(sig.buckets, [819, 3276, 3686, 1228, 2867, 2048, 2048, 2048]);
    // …and the portable integer seed the route graph keys possibility space by.
    assert_eq!(sig.seed(), 0x0F0B_E580_4857_720B);
}

#[test]
fn record_content_id_golden() {
    use world_core::{PossibilitySignature, PreserveRecord, RouteNode, RouteRecord};
    let disc = golden_discovery();
    assert_eq!(disc.id, 0x0414_A7BC_1E1E_F0B4);

    let sig = PossibilitySignature::of(skewed_vector());
    let node = RouteNode {
        pos_q: (300, -10),
        signature: sig,
        cost_q: 40,
        stability_q: 255,
        anchor_sig: 0x1234_5678_9ABC_DEF0,
    };
    let route = RouteRecord::new(vec![node, node], vec![disc.id], 3, String::from("trek"));
    assert_eq!(route.id, 0x5E9E_962B_E0DE_86D0);

    let preserve = PreserveRecord::new(
        vec![
            (RegionCoord::new(-3, 7), sig),
            (RegionCoord::new(-2, 7), sig),
        ],
        4,
        String::from("glade"),
    );
    assert_eq!(preserve.id, 0x21EC_5BC1_8A38_95BC);
}

#[test]
fn record_wire_bytes_golden() {
    // The canonical encoded bytes of the golden discovery — the byte-level
    // format contract (envelope, postcard varints, field order). If this fails
    // the wire format changed: bump RECORD_FORMAT_VERSION and add a migration;
    // never silently re-bless.
    use world_core::{decode_record, encode_record, DiscoveryRecord, RecordKind};
    let disc = golden_discovery();
    let bytes = encode_record(RecordKind::Discovery, &disc);
    assert_eq!(
        bytes,
        [
            0x01, 0x02, 0x02, 0xB4, 0xE1, 0xFB, 0xF0, 0xC1, 0xF7, 0xA9, 0x8A, 0x04, 0x00, 0xD2,
            0xA5, 0xB7, 0xAE, 0x97, 0x8C, 0x98, 0xA0, 0x23, 0x95, 0x86, 0xA7, 0x97, 0xE3, 0xF0,
            0x84, 0x82, 0x42, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0x80, 0x10, 0xE6,
            0x1C, 0x80, 0x10, 0xE6, 0x1C, 0xA0, 0x00, 0xCC, 0x19, 0xDC, 0x0B, 0xD8, 0x04, 0x13,
            0x07, 0x07, 0x67, 0x6C, 0x6F, 0x77, 0x66, 0x69, 0x6E, 0x00,
        ]
    );
    // The v1 archive floor (phase-5-plan.md §12.1): these exact bytes must
    // decode forever, migrations included.
    let (envelope, decoded): (world_core::Envelope, DiscoveryRecord) =
        decode_record(&bytes, RecordKind::Discovery).expect("v1 archive bytes decode");
    assert_eq!(envelope.format_version, 1);
    assert_eq!(decoded, disc);
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
