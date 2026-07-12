//! The anchor harness: the machine check of the Phase 4 success criterion
//! (phase-4-plan.md §12.3), alongside the still-passing invalidation ledger and
//! ecology harness.
//!
//! It settles steered streaming windows and asserts the scenario families of
//! §12.3:
//!
//! - **Intentional / selective** — an emphasized captured anchor moves the
//!   far-field target *toward* the capture in the masked domains (monotone in
//!   strength) and leaves unmasked domains untouched; a suppress anchor moves it
//!   *away*; two anchors blend emergently and order-independently; a region
//!   beyond the falloff radius is untouched.
//! - **Coherence** — every steered region's target is a plausibility fixed point
//!   (satisfies all section-8 rules), the Phase 3 ecology coherence bounds hold
//!   in the steered world, the diversity floor is retained, and emphasizing a
//!   fast (E/M/B/A) domain never moves the stable trio.
//! - **Transition / resonance** — a stationary player produces zero convergence,
//!   anchor-compatible surroundings resonate more than incompatible ones, and
//!   the canonical graph stays within its fixed semantic ceiling. Density
//!   monotonicity is pinned separately by the pure resonance unit test.
//!
//! Regeneration *precision* under steering (which layers regenerate for which
//! domain flip) is asserted by the invalidation ledger; this harness checks the
//! steering *content* those regenerations produce.

use world_core::foodweb::{CARNIVORE_EFFICIENCY, HERBIVORE_EFFICIENCY};
use world_core::{
    anchor_set_signature, bound_target, category_mask, project_plausible, steer, Anchor,
    AnchorKind, AnchorSource, PossibilityDomain, PossibilityField, PossibilityVector, RegionCoord,
    TraitCategory, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    Budget, InlineExecutor, RegionMap, StreamConfig, CHANNEL_ELEVATION, CHANNEL_HARDNESS,
    CHANNEL_HERBIVORE, CHANNEL_PREDATOR, CHANNEL_VEGETATION,
};

/// Outcome of one anchor-harness scenario.
#[derive(Debug)]
pub struct AnchorReport {
    /// Scenario name (matches the §12.3 families).
    pub name: &'static str,
    /// Violations found (capped; empty means the scenario passed).
    pub violations: Vec<String>,
    /// A short metric summary for logging.
    pub summary: String,
}

impl AnchorReport {
    /// Whether the scenario held.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

const MAX_VIOLATIONS: usize = 16;

fn record(violations: &mut Vec<String>, message: String) {
    if violations.len() < MAX_VIOLATIONS {
        violations.push(message);
    }
}

fn harness_config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 6.0 * REGION_SIZE,
        unload_radius: 7.5 * REGION_SIZE,
        converge_per_unit: 0.02,
        converge_rate_cap: 0.25,
        field_resolution: 8,
        ..StreamConfig::default()
    }
}

const PLAYER: (f64, f64) = (128.0, 128.0);

/// Distance from a region's center to the player.
fn center_distance(coord: RegionCoord) -> f64 {
    let (ox, oy) = coord.origin();
    let cx = ox + REGION_SIZE * 0.5;
    let cy = oy + REGION_SIZE * 0.5;
    f64::hypot(cx - PLAYER.0, cy - PLAYER.1)
}

/// Settle a window (fresh regions snap to their steered target at load, so the
/// window settles to the steered state without needing travel).
fn settled(anchors: &[Anchor], budget: &Budget) -> RegionMap {
    let mut map = RegionMap::new(harness_config());
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    for _ in 0..12 {
        map.update(
            PLAYER,
            0.0,
            &field,
            anchors,
            &bias,
            budget,
            &InlineExecutor,
            false,
        );
    }
    map
}

/// A far, unpinned region resident in the window and within an anchor's reach —
/// where steering the far field is observable. Deterministic (nearest such in
/// coordinate order).
fn far_region(map: &RegionMap, anchor: &Anchor) -> Option<RegionCoord> {
    let mut best: Option<(u64, RegionCoord)> = None;
    for region in map.iter_active() {
        if region.stability >= 1.0 {
            continue; // pinned regions do not steer their far field
        }
        let (ox, oy) = region.coord.origin();
        let center = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
        if anchor.influence(center) <= 0.0 {
            continue; // beyond the falloff radius
        }
        let d = center_distance(region.coord).to_bits();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, region.coord));
        }
    }
    best.map(|(_, c)| c)
}

/// An Emphasize/Suppress anchor at the player over the given categories.
fn anchor(categories: &[TraitCategory], kind: AnchorKind, target: f32, strength: f32) -> Anchor {
    let mask = category_mask(categories);
    Anchor {
        world_pos: PLAYER,
        target: bound_target(mask, target),
        mask,
        kind,
        strength,
        falloff_radius: 5.0 * REGION_SIZE,
        source: AnchorSource::Manual,
    }
}

/// Intentional + selective steering (phase-4-plan.md §12.3): an emphasized anchor
/// moves the far-field target toward the capture in the masked domains, monotone
/// in strength, and leaves unmasked domains untouched.
fn intentional_selective_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let budget = Budget::unlimited();
    // Aesthetics is never re-capped by projection, so it is the clean channel to
    // read "moves toward the target"; Morphology is masked too but may be
    // projection-limited (that is the "surprising" property, checked elsewhere).
    let strong = anchor(
        &[TraitCategory::Coloration, TraitCategory::Morphology],
        AnchorKind::Emphasize,
        0.9,
        0.9,
    );
    let weak = anchor(
        &[TraitCategory::Coloration, TraitCategory::Morphology],
        AnchorKind::Emphasize,
        0.9,
        0.3,
    );

    let base_map = settled(&[], &budget);
    let strong_map = settled(&[strong], &budget);
    let weak_map = settled(&[weak], &budget);

    let Some(coord) = far_region(&strong_map, &strong) else {
        return AnchorReport {
            name: "intentional + selective steering",
            violations: vec!["no far unpinned region within the anchor reach".into()],
            summary: String::new(),
        };
    };

    let base = base_map.get(coord).expect("resident").target;
    let strong_t = strong_map.get(coord).expect("resident").target;
    let weak_t = weak_map.get(coord).expect("resident").target;

    let aes = PossibilityDomain::Aesthetics;
    let toward_strong = (strong_t.get(aes) - 0.9).abs();
    let toward_base = (base.get(aes) - 0.9).abs();
    let toward_weak = (weak_t.get(aes) - 0.9).abs();

    // Moves toward the target.
    if toward_strong >= toward_base - 1e-4 {
        record(
            &mut violations,
            format!(
                "aesthetics did not move toward the capture (base gap {toward_base:.4}, steered gap {toward_strong:.4})"
            ),
        );
    }
    // Monotone in strength: the strong anchor moves more than the weak one.
    if toward_strong >= toward_weak + 1e-4 {
        record(
            &mut violations,
            format!(
                "response not monotone in strength (strong gap {toward_strong:.4} >= weak gap {toward_weak:.4})"
            ),
        );
    }
    // Selective: unmasked domains are untouched.
    for domain in [
        PossibilityDomain::Planetary,
        PossibilityDomain::Climate,
        PossibilityDomain::Geology,
        PossibilityDomain::Hydrology,
        PossibilityDomain::Ecology,
        PossibilityDomain::Behavior,
    ] {
        if (strong_t.get(domain) - base.get(domain)).abs() > 1e-4 {
            record(
                &mut violations,
                format!(
                    "unmasked {domain:?} moved under steering ({:.4} -> {:.4})",
                    base.get(domain),
                    strong_t.get(domain)
                ),
            );
        }
    }

    AnchorReport {
        name: "intentional + selective steering",
        violations,
        summary: format!(
            "far region ({}, {}): aesthetics {:.3} -> {:.3} (target 0.9)",
            coord.x,
            coord.y,
            base.get(aes),
            strong_t.get(aes)
        ),
    }
}

/// A suppress anchor moves the far-field target *away* from its target in the
/// masked domains (phase-4-plan.md §12.3).
fn suppress_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let budget = Budget::unlimited();
    let target = 0.9;
    let supp = anchor(
        &[TraitCategory::Coloration],
        AnchorKind::Suppress,
        target,
        0.9,
    );
    let base_map = settled(&[], &budget);
    let supp_map = settled(&[supp], &budget);
    let Some(coord) = far_region(&supp_map, &supp) else {
        return AnchorReport {
            name: "suppress anchor (anti-anchor) moves away",
            violations: vec!["no far unpinned region within the anchor reach".into()],
            summary: String::new(),
        };
    };
    let aes = PossibilityDomain::Aesthetics;
    let base = base_map.get(coord).expect("resident").target.get(aes);
    let suppressed = supp_map.get(coord).expect("resident").target.get(aes);
    // Farther from the (high) target than the base was — i.e. pushed down.
    if suppressed >= base - 1e-4 {
        record(
            &mut violations,
            format!("suppress did not push aesthetics away from {target} (base {base:.4}, steered {suppressed:.4})"),
        );
    }
    AnchorReport {
        name: "suppress anchor (anti-anchor) moves away",
        violations,
        summary: format!("aesthetics {base:.3} -> {suppressed:.3} (away from {target})"),
    }
}

/// Two anchors blend emergently and order-independently (phase-4-plan.md §12.3).
fn combination_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let budget = Budget::unlimited();
    let a = anchor(
        &[TraitCategory::Coloration],
        AnchorKind::Emphasize,
        0.9,
        0.8,
    );
    let mut b = anchor(
        &[TraitCategory::Coloration],
        AnchorKind::Emphasize,
        0.1,
        0.8,
    );
    // Offset b so the two anchors weight the far field differently.
    b.world_pos = (PLAYER.0 + 2.0 * REGION_SIZE, PLAYER.1);

    let map_a = settled(std::slice::from_ref(&a), &budget);
    let map_b = settled(std::slice::from_ref(&b), &budget);
    let map_ab = settled(&[a, b], &budget);
    let map_ba = settled(&[b, a], &budget);

    let Some(coord) = far_region(&map_ab, &a) else {
        return AnchorReport {
            name: "two anchors blend emergently and order-independently",
            violations: vec!["no far unpinned region within reach".into()],
            summary: String::new(),
        };
    };
    let aes = PossibilityDomain::Aesthetics;
    let only_a = map_a.get(coord).expect("resident").target.get(aes);
    let only_b = map_b.get(coord).expect("resident").target.get(aes);
    let ab = map_ab.get(coord).expect("resident").target.get(aes);
    let ba = map_ba.get(coord).expect("resident").target.get(aes);

    // Emergent: the combined steer differs from either alone.
    if (ab - only_a).abs() < 1e-4 || (ab - only_b).abs() < 1e-4 {
        record(
            &mut violations,
            format!("combined steer {ab:.4} equals a single anchor (a {only_a:.4}, b {only_b:.4})"),
        );
    }
    // Order-independent: placement order does not change the result.
    if (ab - ba).abs() > 1e-5 {
        record(
            &mut violations,
            format!("combination is order-dependent ({ab:.6} vs {ba:.6})"),
        );
    }
    AnchorReport {
        name: "two anchors blend emergently and order-independently",
        violations,
        summary: format!("a {only_a:.3}, b {only_b:.3}, a+b {ab:.3}, b+a {ba:.3}"),
    }
}

/// ADR 0025's exact numerical contract: a multiset has one raw-bit reduction
/// order, duplicates remain influential, and Suppress is the final blend.
fn canonical_multiset_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let mask = 1 << PossibilityDomain::Ecology.index() as u8;
    let make = |kind, target, strength, x| Anchor {
        world_pos: (x, 0.0),
        target: bound_target(mask, target),
        mask,
        kind,
        strength,
        falloff_radius: 1000.0,
        source: AnchorSource::Manual,
    };
    let a = make(AnchorKind::Emphasize, 0.91, 0.500_000_06, -30.0);
    let b = make(AnchorKind::Suppress, 0.83, 0.37, 80.0);
    let c = make(AnchorKind::Emphasize, 0.07, 0.000_976_562_5, 0.25);
    let fixture = [a, b, a, c];
    let base = PossibilityVector::neutral();
    let expected = steer(base, &fixture, PLAYER).dims.map(f32::to_bits);
    let expected_sig = anchor_set_signature(&fixture);
    let permutations = [
        [a, b, a, c],
        [c, a, b, a],
        [b, a, c, a],
        [a, c, a, b],
        [a, a, b, c],
        [c, b, a, a],
    ];
    for (index, permutation) in permutations.iter().enumerate() {
        if steer(base, permutation, PLAYER).dims.map(f32::to_bits) != expected {
            record(
                &mut violations,
                format!("permutation {index} changed steering bits"),
            );
        }
        if anchor_set_signature(permutation) != expected_sig {
            record(
                &mut violations,
                format!("permutation {index} changed anchor signature"),
            );
        }
    }
    if anchor_set_signature(&[a]) == anchor_set_signature(&[a, a]) {
        record(
            &mut violations,
            "duplicate anchor occurrence vanished from signature".into(),
        );
    }
    let once = steer(base, &[a], a.world_pos).get(PossibilityDomain::Ecology);
    let twice = steer(base, &[a, a], a.world_pos).get(PossibilityDomain::Ecology);
    if twice <= once {
        record(
            &mut violations,
            "duplicate anchor occurrence did not strengthen steering".into(),
        );
    }

    let emphasize_target = 0.9f32;
    let suppress_target = 0.8f32;
    let polarity_base_i = 0.4f32;
    let emphasize = make(AnchorKind::Emphasize, emphasize_target, 0.5, 0.0);
    let suppress = make(AnchorKind::Suppress, suppress_target, 0.25, 0.0);
    let mut polarity_base = PossibilityVector::neutral();
    polarity_base.set(PossibilityDomain::Ecology, polarity_base_i);
    let actual =
        steer(polarity_base, &[suppress, emphasize], (0.0, 0.0)).get(PossibilityDomain::Ecology);
    let emphasized = polarity_base_i + (emphasize_target - polarity_base_i) * 0.5;
    let reflected = (2.0 * polarity_base_i - suppress_target).clamp(0.0, 1.0);
    let suppress_final = emphasized + (reflected - emphasized) * 0.25;
    if actual.to_bits() != suppress_final.to_bits() {
        record(
            &mut violations,
            "polarity blend was not Emphasize-first/Suppress-final".into(),
        );
    }

    AnchorReport {
        name: "canonical anchor multiset (exact permutations/duplicates/polarity)",
        violations,
        summary: format!(
            "{} permutations, signature {expected_sig:#018x}, duplicate pull {:.3}->{:.3}",
            permutations.len(),
            once,
            twice
        ),
    }
}

/// Falloff: a region beyond the falloff radius is untouched, and influence
/// decreases monotonically with distance (phase-4-plan.md §12.3).
fn falloff_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let budget = Budget::unlimited();
    // A tight anchor that reaches only the pinned core, leaving far regions free.
    let mask = category_mask(&[TraitCategory::Coloration]);
    let tight = Anchor {
        world_pos: PLAYER,
        target: bound_target(mask, 0.9),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.9,
        falloff_radius: 1.0 * REGION_SIZE,
        source: AnchorSource::Manual,
    };
    let base_map = settled(&[], &budget);
    let steered_map = settled(&[tight], &budget);
    let aes = PossibilityDomain::Aesthetics;
    let mut checked = 0usize;
    for region in steered_map.iter_active() {
        let coord = region.coord;
        let (ox, oy) = coord.origin();
        let center = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
        if tight.influence(center) > 0.0 {
            continue; // inside the falloff, expected to move
        }
        // Beyond the radius: identical to the anchor-free target.
        let base = base_map.get(coord).map(|r| r.target.get(aes));
        let steered = region.target.get(aes);
        if let Some(base) = base {
            checked += 1;
            if (base - steered).abs() > 1e-6 {
                record(
                    &mut violations,
                    format!(
                        "region ({}, {}) beyond falloff moved ({base:.5} -> {steered:.5})",
                        coord.x, coord.y
                    ),
                );
            }
        }
    }
    // Influence decreases monotonically with distance.
    let mut last = f32::INFINITY;
    for step in 0..10 {
        let at = (PLAYER.0 + f64::from(step) * 0.1 * REGION_SIZE, PLAYER.1);
        let inf = tight.influence(at);
        if inf > last + 1e-6 {
            record(&mut violations, "influence increased with distance".into());
            break;
        }
        last = inf;
    }
    AnchorReport {
        name: "falloff: beyond-radius untouched, influence monotone",
        violations,
        summary: format!("{checked} far regions verified untouched"),
    }
}

/// Coherence: every steered region's target is a plausibility fixed point, so it
/// satisfies all section-8 rules (phase-4-plan.md §12.3, §7.3).
fn plausible_target_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    // A deliberately extreme anchor set across many domains.
    let wild = Anchor {
        world_pos: PLAYER,
        target: bound_target(0xFF, 1.0),
        mask: 0xFF,
        kind: AnchorKind::Emphasize,
        strength: 1.0,
        falloff_radius: 6.0 * REGION_SIZE,
        source: AnchorSource::Manual,
    };
    let map = settled(&[wild], &Budget::unlimited());
    let mut checked = 0usize;
    for region in map.iter_active() {
        checked += 1;
        let target = region.target;
        let projected = project_plausible(target);
        if projected.dims != target.dims {
            record(
                &mut violations,
                format!(
                    "region ({}, {}) target is not a plausibility fixed point",
                    region.coord.x, region.coord.y
                ),
            );
        }
    }
    AnchorReport {
        name: "every steered target satisfies the section-8 rules",
        violations,
        summary: format!("{checked} regions verified plausible"),
    }
}

/// Coherence + diversity retained: the Phase 3 ecology bounds hold in a strongly
/// steered world, and the window still carries many species (phase-4-plan.md
/// §12.3).
fn coherence_and_diversity_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    // Steer ecology hard in both directions across the window via a strong
    // ecology emphasize; the food web must still be coherent.
    let eco = anchor(
        &[TraitCategory::Ecological],
        AnchorKind::Emphasize,
        0.9,
        0.9,
    );
    let map = settled(&[eco], &Budget::unlimited());
    let res = harness_config().field_resolution;
    for region in map.iter_active() {
        let coord = region.coord;
        let Some(tiles) = map.cache().get(coord) else {
            continue;
        };
        let (Some(veg), Some(herb), Some(pred)) = (
            tiles.channels[CHANNEL_VEGETATION].as_deref(),
            tiles.channels[CHANNEL_HERBIVORE].as_deref(),
            tiles.channels[CHANNEL_PREDATOR].as_deref(),
        ) else {
            continue;
        };
        for cy in 0..res {
            for cx in 0..res {
                let pp = veg.get(cx, cy);
                if herb.get(cx, cy) > HERBIVORE_EFFICIENCY * pp + 1e-5 {
                    record(
                        &mut violations,
                        format!(
                            "({}, {}) herbivore exceeds productivity bound",
                            coord.x, coord.y
                        ),
                    );
                }
                if pred.get(cx, cy) > CARNIVORE_EFFICIENCY * herb.get(cx, cy) + 1e-5 {
                    record(
                        &mut violations,
                        format!("({}, {}) predator exceeds trophic bound", coord.x, coord.y),
                    );
                }
            }
        }
    }
    // Diversity floor: the steered window still carries many species.
    let mut species = std::collections::BTreeSet::new();
    for (_, entry) in map.roster_cache().iter() {
        for sp in &entry.roster.species {
            species.insert(sp.id);
        }
    }
    if species.len() < 8 {
        record(
            &mut violations,
            format!(
                "over-steering flattened diversity to {} species (< 8)",
                species.len()
            ),
        );
    }
    AnchorReport {
        name: "steered world stays ecologically coherent and diverse",
        violations,
        summary: format!(
            "{} habitats, {} species",
            map.roster_cache().len(),
            species.len()
        ),
    }
}

/// No stable-trio steer: emphasizing fast (E/M/B/A) domains never moves terrain
/// or geology (phase-4-plan.md §12.3).
fn stable_trio_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let fast = anchor(
        &[
            TraitCategory::Ecological,
            TraitCategory::Morphology,
            TraitCategory::Behavior,
            TraitCategory::Coloration,
        ],
        AnchorKind::Emphasize,
        0.9,
        0.9,
    );
    let base_map = settled(&[], &Budget::unlimited());
    let steered_map = settled(&[fast], &Budget::unlimited());
    let res = harness_config().field_resolution;
    let mut checked = 0usize;
    for region in steered_map.iter_active() {
        let coord = region.coord;
        let (Some(base), Some(steered)) =
            (base_map.cache().get(coord), steered_map.cache().get(coord))
        else {
            continue;
        };
        for channel in [CHANNEL_ELEVATION, CHANNEL_HARDNESS] {
            let (Some(b), Some(s)) = (
                base.channels[channel].as_deref(),
                steered.channels[channel].as_deref(),
            ) else {
                continue;
            };
            for cy in 0..res {
                for cx in 0..res {
                    checked += 1;
                    if b.get(cx, cy) != s.get(cx, cy) {
                        record(
                            &mut violations,
                            format!(
                                "stable-trio channel {channel} of ({}, {}) moved under an E/M/B/A anchor",
                                coord.x, coord.y
                            ),
                        );
                    }
                }
            }
        }
    }
    AnchorReport {
        name: "E/M/B/A steering never moves the stable trio",
        violations,
        summary: format!("{checked} terrain/geology samples verified identical"),
    }
}

/// Transition / resonance (phase-4-plan.md §12.3): a stationary player produces
/// zero convergence; anchor-compatible surroundings resonate more than
/// incompatible ones; the canonical graph respects its fixed semantic ceiling.
fn resonance_scenario() -> AnchorReport {
    let mut violations = Vec::new();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let mut map = settled(&[], &Budget::unlimited());

    // Stationary player: zero travel ⇒ no realized state moves (ADR 0006/0012).
    let before: std::collections::BTreeMap<RegionCoord, PossibilityVector> =
        map.iter_active().map(|r| (r.coord, r.current)).collect();
    map.update(
        PLAYER,
        0.0,
        &field,
        &[],
        &bias,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    );
    for region in map.iter_active() {
        if before.get(&region.coord) != Some(&region.current) {
            record(
                &mut violations,
                format!(
                    "stationary player moved region ({}, {}) — convergence not travel-gated",
                    region.coord.x, region.coord.y
                ),
            );
        }
    }

    // The authoritative graph has one semantic ceiling, independent of frame
    // budgets and resource tiers (ADR 0024).
    let resonance = map.resonance_at(PLAYER, &[]);
    if resonance.nodes.len() > world_runtime::MAX_RESONANCE_NODES {
        record(
            &mut violations,
            format!(
                "resonance graph overran its fixed ceiling ({} > {})",
                resonance.nodes.len(),
                world_runtime::MAX_RESONANCE_NODES
            ),
        );
    }

    // Anchor compatibility: an anchor targeting the local realized state
    // resonates more than one targeting its opposite.
    if let Some(region) = map.get(RegionCoord::from_world(PLAYER.0, PLAYER.1)) {
        let current = region.current;
        let mask = category_mask(&[TraitCategory::Coloration]);
        let aes = PossibilityDomain::Aesthetics;
        let mut compatible_target = PossibilityVector::neutral();
        compatible_target.set(aes, current.get(aes));
        let mut opposite_target = PossibilityVector::neutral();
        opposite_target.set(aes, 1.0 - current.get(aes));
        let mk = |target| Anchor {
            world_pos: PLAYER,
            target,
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.9,
            falloff_radius: 3.0 * REGION_SIZE,
            source: AnchorSource::Manual,
        };
        let compatible = map.resonance_at(PLAYER, &[mk(compatible_target)]);
        let opposite = map.resonance_at(PLAYER, &[mk(opposite_target)]);
        if compatible.strength < opposite.strength - 1e-4 {
            record(
                &mut violations,
                format!(
                    "anchor-compatible surroundings resonated less ({:.4} < {:.4})",
                    compatible.strength, opposite.strength
                ),
            );
        }
    }

    AnchorReport {
        name: "resonance gates transition (stationary/compatibility/fixed cap)",
        violations,
        summary: format!(
            "resonance {:.3} over {} nodes",
            resonance.strength,
            resonance.nodes.len()
        ),
    }
}

/// Run the full §12.3 anchor-harness scenario set.
#[must_use]
pub fn run_anchor_harness() -> Vec<AnchorReport> {
    vec![
        intentional_selective_scenario(),
        suppress_scenario(),
        combination_scenario(),
        canonical_multiset_scenario(),
        falloff_scenario(),
        plausible_target_scenario(),
        coherence_and_diversity_scenario(),
        stable_trio_scenario(),
        resonance_scenario(),
    ]
}
