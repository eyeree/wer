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
        bound_target, domain_mask, project_plausible, steer, Anchor, AnchorKind, AnchorSource,
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
    species::{species_roster, species_seed},
    terrain, FeatureKey, PossibilityField, PossibilityVector, RegionCoord, WORLD_ALGORITHM_VERSION,
};

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
        assert_eq!(super::origin_feature_hash(), 0x4C6C_A5DE_38F9_0B17);
        assert_eq!(super::terrain_gradient_seed_sample(), 0x557D_9B95_E708_EDFF);
        assert_eq!(super::control_point_seed_sample(), 0xAAF0_551F_3E6F_1A1C);
        assert_eq!(super::lithology_seed_sample(), 0x61DD_60E4_EEF6_FD16);
        assert_eq!(super::drainage_routing_sample(), 0x0000_0001_0000_000D);
        assert_eq!(super::genome_sample(), 0x6023_7E3E_43E5_2590);
        assert_eq!(super::food_web_sample(), 0x6272_09D2_6720_001B);
        assert_eq!(super::steer_sample(), 0x9A4E_77F9_D151_9EC2);
    }
}
