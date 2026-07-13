//! `platform-web` — the browser/wasm application shell (section 3.2 & Phase 7).
//!
//! For the bootstrap this is a **minimal WebGPU/wasm smoke target**: it exists so
//! `world-core` is exercised through a real `wasm32` entry point from the start,
//! before native-only assumptions accumulate (section 19). The full runtime
//! (Web Workers, browser storage, WebGPU tiers, suspend/resume) arrives in
//! Phase 7. Phase 2 grew the shell only by two parity exports: the lithology
//! seed and a drainage routing sample (phase-2-plan.md §12.5).

use world_core::{
    anchor::{
        anchor_set_signature, bound_target, domain_mask, project_plausible, steer, Anchor,
        AnchorKind, AnchorSource,
    },
    dephash::drainage_dep_hash_default,
    drainage::{drainage, MACRO_APRON, MACRO_LEVEL},
    feature_hash,
    foodweb::food_web,
    genome::Genome,
    geology::lithology_seed,
    habitat::HabitatSignature,
    hash::mix,
    possibility::PossibilityDomain,
    record::{
        encode_record, DiscoveryRecord, PossibilitySignature, RecordKind, RouteNode, RouteRecord,
    },
    route::attraction_anchors,
    species::{species_roster, species_seed},
    terrain, FeatureKey, PossibilityField, PossibilityVector, RegionCoord, WORLD_ALGORITHM_VERSION,
};

/// Shared native/wasm parity expectations. The wasm integration suite imports
/// these exact constants, so the two execution gates cannot drift apart.
pub const EXPECTED_ORIGIN_FEATURE_HASH: u64 = 0x4C6C_A5DE_38F9_0B17;
pub const EXPECTED_TERRAIN_GRADIENT_SEED: u64 = 0x557D_9B95_E708_EDFF;
pub const EXPECTED_CONTROL_POINT_SEED: u64 = 0xAAF0_551F_3E6F_1A1C;
pub const EXPECTED_LITHOLOGY_SEED: u64 = 0x61DD_60E4_EEF6_FD16;
pub const EXPECTED_DRAINAGE_ROUTING: u64 = 0x0000_0001_0000_000D;
pub const EXPECTED_DRAINAGE_TOPOLOGY: u64 = 0xB9FA_AD5C_9521_6B3F;
pub const EXPECTED_GENOME: u64 = 0x6023_7E3E_43E5_2590;
pub const EXPECTED_FOOD_WEB: u64 = 0x6272_09D2_6720_001B;
pub const EXPECTED_STEER: u64 = 0x9A4E_77F9_D151_9EC2;
pub const EXPECTED_CANONICAL_ANCHOR_SIGNATURE: u64 = 0xBDAA_C72D_CA08_3AF7;
pub const EXPECTED_RECORD_CODEC: u64 = 0xF42F_DCB5_3552_F850;
pub const EXPECTED_SHARED_STEER: u64 = 0xF0FB_820F_2030_1752;
pub const EXPECTED_ROUTE_ATTRACTION: u64 = 0x3D54_75F6_34AF_1C41;

/// The fixed habitat used by the Phase 3 parity exports. Only the (portable)
/// integer signature → seed → genome / roster / web chain is asserted
/// cross-platform; how a cell arrives at this signature is presentation-grade
/// (§9.3, ADR 0010).
const PARITY_SIGNATURE: HabitatSignature = HabitatSignature {
    biome: 6, // Biome::TemperateForest
    temperature_band: 3,
    moisture_band: 3,
    fertility_band: 2,
};

/// A portable smoke computation: the deterministic hash of the origin feature.
///
/// Must return the identical value on native and wasm — that equality is the
/// determinism guarantee the browser port depends on (section 23.5). It is also
/// covered by the native determinism golden test.
#[must_use]
pub fn origin_feature_hash() -> u64 {
    feature_hash(&FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region: RegionCoord::new(0, 0),
        layer: 0,
        feature_index: 0,
        possibility_revision: 0,
    })
}

/// Parity sample for the terrain identity layer: the integer seed that
/// selects the gradient at lattice corner `(3, -2)` of octave 1
/// (phase-1-plan.md section 11.2). Must equal the native value — float
/// elevation is presentation state and is *not* asserted bit-equal, but the
/// integer seeds that decide where mountains are must be.
#[must_use]
pub fn terrain_gradient_seed_sample() -> u64 {
    terrain::gradient_seed(3, -2, 1)
}

/// Parity sample for the possibility-field identity layer: the control-point
/// seed at lattice coordinate `(-5, 9)` with the default spacing.
#[must_use]
pub fn control_point_seed_sample() -> u64 {
    PossibilityField::default().control_point_seed(-5, 9)
}

/// Parity sample for the geology identity layer: the lithology seed of cell
/// `(3, -2)` (phase-2-plan.md §12.5).
#[must_use]
pub fn lithology_seed_sample() -> u64 {
    lithology_seed(3, -2)
}

/// Parity sample for the drainage identity layer: flow direction and
/// accumulation of a fixed cell of macro tile `(0, 0)`, packed as
/// `(dir << 32) | accum`. Routing is all-integer topology, so **full**
/// cross-platform equality is required here — not just seed equality
/// (phase-2-plan.md §12.5, ADR 0009).
#[must_use]
pub fn drainage_routing_sample() -> u64 {
    let field = PossibilityField::default();
    let mc = RegionCoord::at_level(0, 0, MACRO_LEVEL);
    let tile = drainage(mc, &field, drainage_dep_hash_default(mc));
    let apron = MACRO_APRON as usize;
    let (gx, gy) = (apron + 7, apron + 4);
    (u64::from(tile.flow_dir_at(gx, gy)) << 32) | u64::from(tile.accum_at(gx, gy))
}

/// Broad fixed-topology parity fold spanning signs, field lattice boundaries,
/// a non-power-of-two field recipe, and three complete macro tiles (ADR 0027).
#[must_use]
pub fn drainage_topology_sample() -> u64 {
    world_core::drainage_topology_sample()
}

/// Parity sample for the procedural-genetics identity layer (phase-3-plan.md
/// §12.5): the fingerprint of the genome of a fixed species seed. `genome(seed)`
/// is the *portable* Phase 3 surface — pure integer→integer hashing — so **full**
/// cross-platform equality is required. Signature derivation is deliberately not
/// exported: it reads `f32` tiles and is presentation-grade by decision
/// (§9.3, ADR 0010).
#[must_use]
pub fn genome_sample() -> u64 {
    Genome::from_seed(species_seed(PARITY_SIGNATURE, 0)).fingerprint()
}

/// Parity sample for the food-web layer (phase-3-plan.md §12.5): the tier
/// biomass of the fixed roster's web at a fixed productivity, folded to a
/// fingerprint. Tier biomass is portable `f32`, so full cross-platform equality
/// is required (§9.3). Signature derivation is not exported.
#[must_use]
pub fn food_web_sample() -> u64 {
    let roster = species_roster(PARITY_SIGNATURE);
    food_web(&roster, 0.7).tier_biomass_fingerprint()
}

/// Parity sample for the Phase 4 steering math (phase-4-plan.md §12.5): the
/// steered-and-projected possibility vector for a fixed base and a fixed
/// scripted anchor set (an Emphasize and a Suppress, overlapping on one domain),
/// folded to a fingerprint. `steer`/`project_plausible` are pure float-
/// deterministic functions of `(base, anchor set, position)`, so full
/// cross-platform equality is required. Live capture and resonance are *not*
/// exported — they read `f32` tiles/organisms and are presentation-grade by
/// decision (§9.2, ADR 0010/0011).
#[must_use]
pub fn steer_sample() -> u64 {
    let mask_a = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Aesthetics]);
    let mask_b = domain_mask(&[PossibilityDomain::Aesthetics, PossibilityDomain::Morphology]);
    let anchors = [
        Anchor {
            world_pos: (64.0, -32.0),
            target: bound_target(mask_a, 0.9),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.75,
            falloff_radius: 1500.0,
            source: AnchorSource::Manual,
        },
        Anchor {
            world_pos: (-100.0, 40.0),
            target: bound_target(mask_b, 0.2),
            mask: mask_b,
            kind: AnchorKind::Suppress,
            strength: 0.5,
            falloff_radius: 1200.0,
            source: AnchorSource::Manual,
        },
    ];
    let base = PossibilityVector::neutral();
    let v = project_plausible(steer(base, &anchors, (0.0, 0.0)));
    let mut h: u64 = 0x57EE_5000_0DE0_0004;
    for d in v.dims {
        h = mix(h, u64::from(d.to_bits()));
    }
    h
}

/// Parity sample for ADR 0025's canonical anchor-multiset signature. It
/// includes both polarities, different masks, an exact duplicate, and inert
/// unmasked target storage. Identical IEEE inputs must hash equally on native
/// and wasm regardless of slice order.
fn canonical_anchor_sample_anchors() -> [Anchor; 3] {
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
    [first, second, first]
}

/// Return the fixed canonical anchor-multiset signature parity probe.
#[must_use]
pub fn canonical_anchor_signature_sample() -> u64 {
    anchor_set_signature(&canonical_anchor_sample_anchors())
}

/// The fixed shareable record used by the Phase 5 parity exports: a discovery
/// of the parity habitat's first species, built entirely from integers, so it
/// is bit-identical on every platform (ADR 0013).
fn parity_discovery() -> DiscoveryRecord {
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    let mut target = PossibilitySignature {
        buckets: [2048; world_core::POSSIBILITY_DIMS],
    };
    target.buckets[PossibilityDomain::Morphology.index()] = 3600;
    target.buckets[PossibilityDomain::Aesthetics.index()] = 3300;
    DiscoveryRecord {
        id: 0,
        source: AnchorSource::Organism {
            species: species_seed(PARITY_SIGNATURE, 0),
        },
        signature_seed: PARITY_SIGNATURE.seed(),
        target,
        mask,
        kind: AnchorKind::Emphasize,
        strength_q: 3277,
        falloff_q: 1500,
        pos_q: (300, -10),
        sequence: 7,
        name: String::from("glowfin"),
        journal: String::new(),
    }
}

/// Parity sample for the Phase 5 record codec (phase-5-plan.md §12.5): the
/// byte fold of the canonical encoding of a fixed record. The wire format is
/// the interoperability surface native-written bundles and the future browser
/// runtime share, so **byte-level** cross-platform equality is required.
#[must_use]
pub fn record_codec_sample() -> u64 {
    let mut record = parity_discovery();
    record.id = record.content_id();
    let bytes = encode_record(RecordKind::Discovery, &record);
    let mut h: u64 = 0x5EC0_7D00_C0DE_0001;
    h = mix(h, bytes.len() as u64);
    for b in bytes {
        h = mix(h, u64::from(b));
    }
    h
}

/// Parity sample for shared-anchor steering (phase-5-plan.md §12.5): the
/// steered-and-projected vector produced by an anchor reconstructed from a
/// fixed [`DiscoveryRecord`]. The record carries only quantized integers and
/// `steer`/`project_plausible` are float-deterministic, so shared steering is
/// portable **end-to-end** — the shared-anchor guarantee (ADR 0013). Live
/// vault I/O is deliberately not exported (no browser storage until Phase 7).
#[must_use]
pub fn shared_steer_sample() -> u64 {
    let mut record = parity_discovery();
    record.id = record.content_id();
    let anchor = record.to_anchor();
    let base = PossibilityVector::neutral();
    let v = project_plausible(steer(base, &[anchor], (256.0, 0.0)));
    let mut h: u64 = 0x5A4E_D57E_E200_0005;
    h = mix(h, record.id);
    for d in v.dims {
        h = mix(h, u64::from(d.to_bits()));
    }
    h
}

/// Additive ADR 0026 parity probe for aggregate route normalization. The two
/// quantized routes contribute more candidates than are selected and more raw
/// peak pull than the global route-channel cap. Count, nearest-order strength
/// bits, and the projected steering result are folded into one portable value.
#[must_use]
pub fn route_attraction_sample() -> u64 {
    let mut signature = PossibilitySignature {
        buckets: [2048; world_core::POSSIBILITY_DIMS],
    };
    signature.buckets[PossibilityDomain::Ecology.index()] = 3900;
    signature.buckets[PossibilityDomain::Aesthetics.index()] = 3500;
    let node = |x, cost| RouteNode {
        pos_q: (x, 0),
        signature,
        current_signature: None,
        cost_q: cost,
        stability_q: 0,
        anchor_sig: 0,
        distance_q: 0,
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
    let mut h = mix(0xA77A_C710_0026_0001, anchors.len() as u64);
    for anchor in anchors {
        h = mix(h, u64::from(anchor.strength.to_bits()));
    }
    for dimension in value.dims {
        h = mix(h, u64::from(dimension.to_bits()));
    }
    h
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// wasm entry point, invoked automatically when the module is instantiated.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let hash = super::origin_feature_hash();
        web_sys::console::log_1(
            &format!("[wer] wasm smoke ok — origin feature hash: {hash:#018x}").into(),
        );
    }

    /// Exposed to JS so the host page can confirm the core computed the expected
    /// deterministic value.
    #[wasm_bindgen]
    #[must_use]
    pub fn origin_feature_hash() -> u64 {
        super::origin_feature_hash()
    }

    /// Terrain-gradient identity sample (phase-1-plan.md section 11.2).
    #[wasm_bindgen]
    #[must_use]
    pub fn terrain_gradient_seed_sample() -> u64 {
        super::terrain_gradient_seed_sample()
    }

    /// Possibility-field control-point identity sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn control_point_seed_sample() -> u64 {
        super::control_point_seed_sample()
    }

    /// Lithology identity sample (phase-2-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn lithology_seed_sample() -> u64 {
        super::lithology_seed_sample()
    }

    /// Drainage routing sample — all-integer topology, full equality required.
    #[wasm_bindgen]
    #[must_use]
    pub fn drainage_routing_sample() -> u64 {
        super::drainage_routing_sample()
    }

    /// Broad fixed-topology fold — complete macro content and signed samples.
    #[wasm_bindgen]
    #[must_use]
    pub fn drainage_topology_sample() -> u64 {
        super::drainage_topology_sample()
    }

    /// Procedural-genome identity sample (phase-3-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn genome_sample() -> u64 {
        super::genome_sample()
    }

    /// Food-web tier-biomass identity sample (phase-3-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn food_web_sample() -> u64 {
        super::food_web_sample()
    }

    /// Phase 4 steering-math identity sample (phase-4-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn steer_sample() -> u64 {
        super::steer_sample()
    }

    /// ADR 0025 canonical anchor-multiset signature sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn canonical_anchor_signature_sample() -> u64 {
        super::canonical_anchor_signature_sample()
    }

    /// Phase 5 record-codec byte-identity sample (phase-5-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn record_codec_sample() -> u64 {
        super::record_codec_sample()
    }

    /// Phase 5 shared-anchor steering identity sample (phase-5-plan.md §12.5).
    #[wasm_bindgen]
    #[must_use]
    pub fn shared_steer_sample() -> u64 {
        super::shared_steer_sample()
    }

    /// ADR 0026 aggregate route-attraction parity sample.
    #[wasm_bindgen]
    #[must_use]
    pub fn route_attraction_sample() -> u64 {
        super::route_attraction_sample()
    }
}

#[cfg(test)]
mod tests {
    //! Native side of the parity guarantee: the exact functions the wasm module
    //! exports are pinned here to the same golden constants asserted in
    //! `world-core`'s determinism suite. The wasm build compiles the identical
    //! pure code (CI's `wasm32` check), and the integer-only identity layer
    //! (ADR 0003) makes cross-platform agreement structural, not luck.

    #[test]
    fn parity_samples_match_goldens() {
        assert_eq!(
            super::origin_feature_hash(),
            super::EXPECTED_ORIGIN_FEATURE_HASH
        );
        assert_eq!(
            super::terrain_gradient_seed_sample(),
            super::EXPECTED_TERRAIN_GRADIENT_SEED
        );
        assert_eq!(
            super::control_point_seed_sample(),
            super::EXPECTED_CONTROL_POINT_SEED
        );
        assert_eq!(
            super::lithology_seed_sample(),
            super::EXPECTED_LITHOLOGY_SEED
        );
        assert_eq!(
            super::drainage_routing_sample(),
            super::EXPECTED_DRAINAGE_ROUTING
        );
        assert_eq!(
            super::drainage_topology_sample(),
            super::EXPECTED_DRAINAGE_TOPOLOGY
        );
        assert_eq!(super::genome_sample(), super::EXPECTED_GENOME);
        assert_eq!(super::food_web_sample(), super::EXPECTED_FOOD_WEB);
        assert_eq!(super::steer_sample(), super::EXPECTED_STEER);
        assert_eq!(
            super::canonical_anchor_signature_sample(),
            super::EXPECTED_CANONICAL_ANCHOR_SIGNATURE
        );
        assert_eq!(super::record_codec_sample(), super::EXPECTED_RECORD_CODEC);
        assert_eq!(super::shared_steer_sample(), super::EXPECTED_SHARED_STEER);
        assert_eq!(
            super::route_attraction_sample(),
            super::EXPECTED_ROUTE_ATTRACTION
        );
    }

    #[test]
    fn canonical_anchor_signature_is_permutation_invariant() {
        let mut anchors = super::canonical_anchor_sample_anchors();
        let forward = world_core::anchor_set_signature(&anchors);
        anchors.reverse();
        assert_eq!(forward, world_core::anchor_set_signature(&anchors));
        assert_eq!(forward, super::canonical_anchor_signature_sample());
    }
}
