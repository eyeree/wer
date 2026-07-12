//! Anchors: player-placed influences that steer the possibility target
//! (implementation-plan.md section 8; phase-4-plan.md §4, §7.2).
//!
//! Phase 1 shipped a deliberately blunt sketch — two kinds pulling toward the
//! fixed bounds `1`/`0`, a per-domain mask, radial falloff, and a two-rule
//! projection — to prove *the seam between steering and constraints exists*.
//! Phase 4 generalizes it into the full section 8 shape (phase-4-plan.md §4.1):
//! a captured possibility `target` the masked dimensions are pulled toward
//! (Emphasize) or pushed away from (Suppress, the anti-anchor), a `source`
//! recording what the anchor was captured from, and an **order-independent**
//! combination rule (ADR 0011) that retires the Phase 1 sequential-contraction
//! order caveat.
//!
//! The Phase 1 `Emphasize`/`Suppress`-toward-a-bound behaviour is the special
//! case `target = 1.0` across the mask: Emphasize toward the bound pulls a
//! dimension up, Suppress away from it pushes it down (§4.1), so the Phase 1
//! debug keys keep working by constructing `Anchor { target: bound, source:
//! Manual, .. }`. The [`project_plausible`] rule set grows in Phase 4 M2.

use serde::{Deserialize, Serialize};

use crate::hash::mix;
use crate::possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS};

/// Fixed basis separating canonical anchor-multiset signatures from every
/// other stable hash domain (ADR 0025).
const ANCHOR_SET_BASIS: u64 = 0xA5E7_51B0_39C4_D6F2;

/// What an anchor does to the masked dimensions relative to its captured target.
///
/// Serialized inside Phase 5 records; the variant order is part of the record
/// format contract (`record::RECORD_FORMAT_VERSION`) and is golden-fixtured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorKind {
    /// Pull masked dimensions toward `target` (make the remembered trait more
    /// present). Phase 1 "emphasize" is this with `target = 1.0`.
    Emphasize,
    /// Push masked dimensions away from `target` — the anti-anchor. Phase 1
    /// "suppress" is this with `target = 1.0` (push away from the upper bound).
    Suppress,
}

/// Where an anchor was captured from — legibility metadata in Phase 4, and the
/// identity core of a persistent/shareable [`crate::record::DiscoveryRecord`]
/// in Phase 5. Serialized inside records; the variant order is part of the
/// record format contract and is golden-fixtured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorSource {
    /// A captured creature, carrying its stable [`crate::species::Species::id`].
    Organism { species: u64 },
    /// A rock formation / terrain character.
    Landform,
    /// A hydrology feature (river, wetland).
    River,
    /// A climate / sky / weather condition.
    Atmosphere,
    /// Debug-placed with no discovery (the Phase 1 behaviour).
    Manual,
}

/// A placed steering influence: a captured trait target with smooth radial
/// falloff (phase-4-plan.md §4.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Where the anchor sits in continuous world space.
    pub world_pos: (f64, f64),
    /// The captured possibility target the masked dimensions are pulled toward
    /// (Emphasize) or away from (Suppress). Only masked dimensions are read.
    pub target: PossibilityVector,
    /// Bitmask of affected [`PossibilityDomain`]s (bit = `domain.index()`).
    pub mask: u8,
    /// Direction of influence relative to `target`.
    pub kind: AnchorKind,
    /// Peak influence at the anchor's center, `0..=1`.
    pub strength: f32,
    /// World-space radius beyond which the anchor has no effect (its scope).
    pub falloff_radius: f64,
    /// What this anchor was captured from (metadata; run-local in Phase 4).
    pub source: AnchorSource,
}

impl Anchor {
    /// Influence of this anchor at a world position: `strength` at the center
    /// falling smoothly (C1) to zero at `falloff_radius`.
    #[must_use]
    pub fn influence(&self, at: (f64, f64)) -> f32 {
        if self.falloff_radius <= 0.0 {
            return 0.0;
        }
        let dx = at.0 - self.world_pos.0;
        let dy = at.1 - self.world_pos.1;
        let d2 = dx * dx + dy * dy;
        let r2 = self.falloff_radius * self.falloff_radius;
        if d2 >= r2 {
            return 0.0;
        }
        // (1 - (d/r)^2)^2: smooth at both the center and the rim, so anchors
        // never introduce a seam of their own.
        let t = (1.0 - d2 / r2) as f32;
        self.strength.clamp(0.0, 1.0) * t * t
    }

    /// Whether this anchor touches `domain`.
    #[inline]
    #[must_use]
    pub const fn affects(&self, domain: PossibilityDomain) -> bool {
        self.mask & (1 << domain.index() as u8) != 0
    }
}

/// The complete steering-semantic projection of an anchor (ADR 0025).
///
/// Field order is both the raw-bit lexicographic reduction order and the
/// signature fold order. Source metadata and unmasked target storage are
/// deliberately normalized out because steering never reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct AnchorSteeringKey {
    world_x_bits: u64,
    world_y_bits: u64,
    mask: u8,
    kind_tag: u8,
    strength_bits: u32,
    falloff_radius_bits: u64,
    masked_target_bits: [u32; POSSIBILITY_DIMS],
}

impl AnchorSteeringKey {
    fn of(anchor: &Anchor) -> Self {
        let mut masked_target_bits = [0; POSSIBILITY_DIMS];
        for (i, bits) in masked_target_bits.iter_mut().enumerate() {
            if anchor.mask & (1 << i as u8) != 0 {
                *bits = anchor.target.dims[i].to_bits();
            }
        }
        Self {
            world_x_bits: anchor.world_pos.0.to_bits(),
            world_y_bits: anchor.world_pos.1.to_bits(),
            mask: anchor.mask,
            kind_tag: match anchor.kind {
                AnchorKind::Emphasize => 0,
                AnchorKind::Suppress => 1,
            },
            strength_bits: anchor.strength.to_bits(),
            falloff_radius_bits: anchor.falloff_radius.to_bits(),
            masked_target_bits,
        }
    }
}

/// Project and sort every occurrence without deduplicating equal keys.
fn canonical_anchors(anchors: &[Anchor]) -> Vec<(AnchorSteeringKey, &Anchor)> {
    let mut canonical: Vec<_> = anchors
        .iter()
        .map(|anchor| (AnchorSteeringKey::of(anchor), anchor))
        .collect();
    canonical.sort_unstable_by_key(|(key, _)| *key);
    canonical
}

/// A canonical signature of an anchor steering multiset (ADR 0025).
///
/// Every occurrence is folded in raw-bit key order after the cardinality, so
/// duplicate anchors retain their steering multiplicity. Fields that steering
/// does not read—source metadata and unmasked target values—are excluded.
#[must_use]
pub fn anchor_set_signature(anchors: &[Anchor]) -> u64 {
    let canonical = canonical_anchors(anchors);
    let mut h = mix(ANCHOR_SET_BASIS, canonical.len() as u64);
    for (key, _) in canonical {
        h = mix(h, key.world_x_bits);
        h = mix(h, key.world_y_bits);
        h = mix(h, u64::from(key.mask));
        h = mix(h, u64::from(key.kind_tag));
        h = mix(h, u64::from(key.strength_bits));
        h = mix(h, key.falloff_radius_bits);
        for target_bits in key.masked_target_bits {
            h = mix(h, u64::from(target_bits));
        }
    }
    h
}

/// A target that carries `value` on every domain in `mask` and neutral
/// elsewhere — the Phase 1 "bound" target (`value = 1.0`) and the shape a
/// debug/manual anchor uses when it has no captured discovery (§4.1).
#[must_use]
pub fn bound_target(mask: u8, value: f32) -> PossibilityVector {
    let mut v = PossibilityVector::neutral();
    let value = value.clamp(0.0, 1.0);
    for i in 0..POSSIBILITY_DIMS {
        if mask & (1 << i as u8) != 0 {
            v.dims[i] = value;
        }
    }
    v
}

/// Build an anchor mask from a set of domains.
#[must_use]
pub fn domain_mask(domains: &[PossibilityDomain]) -> u8 {
    let mut mask = 0u8;
    for d in domains {
        mask |= 1 << d.index() as u8;
    }
    mask
}

/// Combine a base field sample with every nearby anchor into a steered vector.
///
/// Bitwise order-independent (ADR 0011, refined by ADR 0025): every occurrence
/// is sorted by its complete raw-bit steering key, so the result is a pure
/// function of the anchor *multiset* and `at`, not of slice order. For each
/// masked domain, the emphasize
/// anchors that reach `at` contribute a total-influence-weighted pull toward
/// their combined target, and the suppress anchors a weighted push away from
/// theirs (a reflection of the target about the base); the base is then blended
/// toward each combined desired value by a *saturating* weight `1 - ∏(1 - wₐ)`,
/// which keeps the result in `[0, 1]` without the Phase 1 sequential
/// contraction and prevents a single strong anchor from being diluted by many
/// weak far ones. Emphasize is blended first; Suppress is deliberately blended
/// last, while its reflected targets remain relative to the unsteered base.
/// This fixed polarity priority is not a simultaneous solve (§7.2, ADR 0025).
#[must_use]
pub fn steer(base: PossibilityVector, anchors: &[Anchor], at: (f64, f64)) -> PossibilityVector {
    let canonical = canonical_anchors(anchors);
    let mut out = base;
    for i in 0..POSSIBILITY_DIMS {
        let base_i = base.dims[i];
        // Emphasize accumulators: influence-weighted target mean + saturating
        // product. Suppress mirrors them against a reflected target.
        let mut emp_num = 0.0f32;
        let mut emp_den = 0.0f32;
        let mut emp_keep = 1.0f32;
        let mut sup_num = 0.0f32;
        let mut sup_den = 0.0f32;
        let mut sup_keep = 1.0f32;
        for (_, anchor) in &canonical {
            if anchor.mask & (1 << i as u8) == 0 {
                continue;
            }
            let w = anchor.influence(at);
            if w <= 0.0 {
                continue;
            }
            let target_i = anchor.target.dims[i];
            match anchor.kind {
                AnchorKind::Emphasize => {
                    emp_num += w * target_i;
                    emp_den += w;
                    emp_keep *= 1.0 - w;
                }
                AnchorKind::Suppress => {
                    // Push away: reflect the target about the base, so a
                    // suppress toward the same target moves the opposite way.
                    let reflected = (2.0 * base_i - target_i).clamp(0.0, 1.0);
                    sup_num += w * reflected;
                    sup_den += w;
                    sup_keep *= 1.0 - w;
                }
            }
        }
        let mut val = base_i;
        if emp_den > 0.0 {
            let desired = emp_num / emp_den;
            val += (desired - val) * (1.0 - emp_keep);
        }
        if sup_den > 0.0 {
            let desired = sup_num / sup_den;
            val += (desired - val) * (1.0 - sup_keep);
        }
        out.dims[i] = val.clamp(0.0, 1.0);
    }
    out
}

/// Project a steered vector back inside plausible bounds
/// (implementation-plan.md section 8: rule-based constraints and iterative
/// relaxation, not machine learning; phase-4-plan.md §7.3).
///
/// The full section 8 rule set, applied as a **fixed, ordered, bounded
/// relaxation** — a single pass in *topological* order, so each domain is only
/// ever capped by domains already at their final value. That makes projection
/// **idempotent** (projecting a projected vector is a fixed point) and keeps the
/// neutral vector a fixed point (a bias-free world never drifts under the rules)
/// without any convergence loop, mirroring the food web's single-pass discipline
/// (phase-3-plan.md §7.4). Rule order is part of the deterministic contract and
/// is golden-fixtured.
///
/// The rules (each an *upper* cap, so steering can always suppress a domain but
/// not conjure an implausible abundance):
///
/// 1. **Wetland vs hydrology** — surface wetness (Hydrology) is capped by the
///    planetary ocean fraction (a dry world cannot be steered fully swampy).
/// 5. **Ice vs temperature** — a cold (low Climate), ocean-poor (low Planetary)
///    world holds little liquid water, jointly bounding Hydrology.
/// 2. **Vegetation vs rainfall** — Ecology is capped by available moisture
///    (the now-final Hydrology plus Climate).
/// 4. **Canopy vs soil depth and wind** — Ecology's canopy component is capped
///    by a Geology-derived soil/exposure proxy (active, eroded ground shelters
///    less canopy).
/// 3. **Animal scale vs primary productivity** — Morphology (body scale) is
///    capped by a function of the now-final Ecology (productivity).
///
/// Rules 1 and 5 both bound Hydrology, so both run before the Ecology rules
/// (2, 4) that read it; rule 3 reads the fully-capped Ecology last (§7.3).
#[must_use]
pub fn project_plausible(mut v: PossibilityVector) -> PossibilityVector {
    for d in v.dims.iter_mut() {
        *d = d.clamp(0.0, 1.0);
    }
    let planetary = v.get(PossibilityDomain::Planetary);
    let climate = v.get(PossibilityDomain::Climate);
    let geology = v.get(PossibilityDomain::Geology);

    // Rule 1 + Rule 5: bound Hydrology by ocean supply and by liquid-water
    // availability (warmth + ocean), taking the tighter of the two.
    let hydrology_cap = (0.5 + 0.6 * planetary).min(1.0);
    let liquid_cap = (0.2 + 0.5 * climate + 0.4 * planetary).min(1.0);
    let hydrology = v
        .get(PossibilityDomain::Hydrology)
        .min(hydrology_cap)
        .min(liquid_cap);
    v.set(PossibilityDomain::Hydrology, hydrology);

    // Rule 2 + Rule 4: bound Ecology by available moisture and by the
    // soil/exposure proxy, both reading now-final inputs.
    let moisture = 0.5 * hydrology + 0.5 * climate;
    let vegetation_cap = (0.2 + 0.8 * moisture).min(1.0);
    let canopy_cap = (0.4 + 0.6 * (1.0 - geology)).min(1.0);
    let ecology = v
        .get(PossibilityDomain::Ecology)
        .min(vegetation_cap)
        .min(canopy_cap);
    v.set(PossibilityDomain::Ecology, ecology);

    // Rule 3: bound Morphology (body scale) by the now-final productivity.
    let body_cap = (0.3 + 0.7 * ecology).min(1.0);
    let morphology = v.get(PossibilityDomain::Morphology).min(body_cap);
    v.set(PossibilityDomain::Morphology, morphology);

    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(kind: AnchorKind) -> Anchor {
        let mask = domain_mask(&[PossibilityDomain::Ecology]);
        Anchor {
            world_pos: (0.0, 0.0),
            target: bound_target(mask, 1.0),
            mask,
            kind,
            strength: 0.8,
            falloff_radius: 1000.0,
            source: AnchorSource::Manual,
        }
    }

    #[test]
    fn influence_is_bounded_and_fades_to_zero() {
        let a = anchor(AnchorKind::Emphasize);
        assert!((a.influence((0.0, 0.0)) - 0.8).abs() < 1e-6);
        assert_eq!(a.influence((1000.0, 0.0)), 0.0);
        assert_eq!(a.influence((5000.0, 0.0)), 0.0);
        let near = a.influence((100.0, 0.0));
        let far = a.influence((900.0, 0.0));
        assert!(near > far && far > 0.0);
    }

    #[test]
    fn emphasize_raises_and_suppress_lowers_only_masked_dims() {
        let base = PossibilityVector::neutral();
        // Emphasize toward the bound 1.0 pulls up; Suppress away from it pushes
        // down (the Phase 1 special case, §4.1).
        let up = steer(base, &[anchor(AnchorKind::Emphasize)], (0.0, 0.0));
        let down = steer(base, &[anchor(AnchorKind::Suppress)], (0.0, 0.0));
        assert!(up.get(PossibilityDomain::Ecology) > 0.5);
        assert!(down.get(PossibilityDomain::Ecology) < 0.5);
        // Unmasked dimensions untouched.
        assert_eq!(up.get(PossibilityDomain::Climate), 0.5);
        assert_eq!(down.get(PossibilityDomain::Climate), 0.5);
    }

    #[test]
    fn steer_stays_in_unit_range() {
        let mut base = PossibilityVector::neutral();
        base.set(PossibilityDomain::Ecology, 0.95);
        let anchors = [anchor(AnchorKind::Emphasize), anchor(AnchorKind::Emphasize)];
        let v = steer(base, &anchors, (0.0, 0.0));
        assert!(v.get(PossibilityDomain::Ecology) <= 1.0);
        assert!(v.get(PossibilityDomain::Ecology) >= 0.0);
    }

    #[test]
    fn steer_is_order_independent() {
        // The ADR 0011/0025 property: the steered vector is a pure function of
        // the anchor multiset, not slice order.
        let mask_a = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Aesthetics]);
        let mask_b = domain_mask(&[PossibilityDomain::Aesthetics, PossibilityDomain::Morphology]);
        let a = Anchor {
            world_pos: (50.0, 0.0),
            target: bound_target(mask_a, 0.9),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.7,
            falloff_radius: 800.0,
            source: AnchorSource::Manual,
        };
        let b = Anchor {
            world_pos: (-30.0, 20.0),
            target: bound_target(mask_b, 0.2),
            mask: mask_b,
            kind: AnchorKind::Suppress,
            strength: 0.6,
            falloff_radius: 900.0,
            source: AnchorSource::Manual,
        };
        let c = Anchor {
            world_pos: (10.0, -10.0),
            target: bound_target(mask_a, 0.4),
            mask: mask_a,
            kind: AnchorKind::Emphasize,
            strength: 0.5,
            falloff_radius: 700.0,
            source: AnchorSource::Manual,
        };
        let base = PossibilityVector::neutral();
        let at = (5.0, 5.0);
        let forward = steer(base, &[a, b, c], at);
        let reversed = steer(base, &[c, b, a], at);
        let shuffled = steer(base, &[b, a, c], at);
        assert_eq!(forward.dims, reversed.dims);
        assert_eq!(forward.dims, shuffled.dims);
    }

    fn adversarial_anchors() -> [Anchor; 6] {
        let ecology = domain_mask(&[PossibilityDomain::Ecology]);
        let ecology_aesthetics =
            domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Aesthetics]);
        let morphology_aesthetics =
            domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
        let mut first_target = PossibilityVector::neutral();
        first_target.set(PossibilityDomain::Ecology, f32::from_bits(0x3f6c_cccd));
        // This unmasked value is deliberately non-neutral and must be inert.
        first_target.set(PossibilityDomain::Climate, 0.91);
        let first = Anchor {
            world_pos: (-17.25, 9.5),
            target: first_target,
            mask: ecology,
            kind: AnchorKind::Emphasize,
            strength: f32::from_bits(0x3e80_0001),
            falloff_radius: 911.125,
            source: AnchorSource::Landform,
        };
        let mut equivalent = first;
        equivalent.source = AnchorSource::River;
        equivalent.target.set(PossibilityDomain::Climate, 0.03);

        let mut second_target = PossibilityVector::neutral();
        second_target.set(PossibilityDomain::Ecology, f32::from_bits(0x3d00_0001));
        second_target.set(PossibilityDomain::Aesthetics, 0.77);
        let second = Anchor {
            world_pos: (81.0, -63.5),
            target: second_target,
            mask: ecology_aesthetics,
            kind: AnchorKind::Suppress,
            strength: f32::from_bits(0x3f19_999a),
            falloff_radius: 1200.25,
            source: AnchorSource::Atmosphere,
        };

        let mut third_target = PossibilityVector::neutral();
        third_target.set(PossibilityDomain::Morphology, 0.88);
        third_target.set(PossibilityDomain::Aesthetics, f32::from_bits(0x3e4c_cccd));
        let third = Anchor {
            world_pos: (-400.0, -11.0),
            target: third_target,
            mask: morphology_aesthetics,
            kind: AnchorKind::Emphasize,
            strength: f32::from_bits(0x3580_0001),
            falloff_radius: 777.75,
            source: AnchorSource::Manual,
        };

        let mut fourth_target = PossibilityVector::neutral();
        fourth_target.set(PossibilityDomain::Ecology, f32::from_bits(0x3f7f_fffe));
        let fourth = Anchor {
            world_pos: (0.125, 0.25),
            target: fourth_target,
            mask: ecology,
            kind: AnchorKind::Emphasize,
            strength: f32::from_bits(0x3a80_0001),
            falloff_radius: 32.0,
            source: AnchorSource::Manual,
        };

        let mut fifth_target = PossibilityVector::neutral();
        fifth_target.set(PossibilityDomain::Ecology, 0.31);
        fifth_target.set(PossibilityDomain::Aesthetics, 0.69);
        let fifth = Anchor {
            world_pos: (2048.0, -1024.0),
            target: fifth_target,
            mask: ecology_aesthetics,
            kind: AnchorKind::Suppress,
            strength: 0.42,
            falloff_radius: 2500.0,
            source: AnchorSource::Manual,
        };

        [first, equivalent, second, third, fourth, fifth]
    }

    fn for_each_permutation(values: &mut [Anchor], start: usize, f: &mut impl FnMut(&[Anchor])) {
        if start == values.len() {
            f(values);
            return;
        }
        for i in start..values.len() {
            values.swap(start, i);
            for_each_permutation(values, start + 1, f);
            values.swap(start, i);
        }
    }

    #[test]
    fn adversarial_multiset_is_bitwise_equal_across_all_permutations() {
        let fixture = adversarial_anchors();
        let base = PossibilityVector {
            dims: [0.17, 0.29, 0.43, 0.59, 0.61, 0.73, 0.83, 0.97],
        };
        let positions = [(0.0, 0.0), (300.0, -100.0), (3000.0, 3000.0)];
        let expected: Vec<[u32; POSSIBILITY_DIMS]> = positions
            .iter()
            .map(|&at| steer(base, &fixture, at).dims.map(f32::to_bits))
            .collect();
        let expected_signature = anchor_set_signature(&fixture);
        let mut permutation = fixture;
        let mut count = 0;
        for_each_permutation(&mut permutation, 0, &mut |anchors| {
            count += 1;
            assert_eq!(anchor_set_signature(anchors), expected_signature);
            for (&at, expected_bits) in positions.iter().zip(&expected) {
                assert_eq!(
                    steer(base, anchors, at).dims.map(f32::to_bits),
                    *expected_bits
                );
            }
        });
        assert_eq!(count, 720);
    }

    #[test]
    fn canonical_signature_tracks_semantics_and_multiplicity() {
        let [a, equivalent, ..] = adversarial_anchors();
        assert_eq!(
            AnchorSteeringKey::of(&a),
            AnchorSteeringKey::of(&equivalent)
        );
        assert_eq!(
            anchor_set_signature(&[a]),
            anchor_set_signature(&[equivalent])
        );
        assert_eq!(
            steer(PossibilityVector::neutral(), &[a], a.world_pos)
                .dims
                .map(f32::to_bits),
            steer(
                PossibilityVector::neutral(),
                &[equivalent],
                equivalent.world_pos
            )
            .dims
            .map(f32::to_bits)
        );

        let empty = anchor_set_signature(&[]);
        let singleton = anchor_set_signature(&[a]);
        let pair = anchor_set_signature(&[a, equivalent]);
        let triple = anchor_set_signature(&[a, equivalent, a]);
        assert_ne!(empty, singleton);
        assert_ne!(singleton, pair);
        assert_ne!(pair, triple);

        let base = PossibilityVector::neutral();
        let once = steer(base, &[a], a.world_pos).get(PossibilityDomain::Ecology);
        let twice = steer(base, &[a, equivalent], a.world_pos).get(PossibilityDomain::Ecology);
        assert!(
            twice > once,
            "a duplicate occurrence must strengthen the pull"
        );

        let mut changed = a;
        changed.target.dims[PossibilityDomain::Ecology.index()] = 0.25;
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.mask |= 1 << PossibilityDomain::Climate.index() as u8;
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.kind = AnchorKind::Suppress;
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.world_pos.0 = f64::from_bits(changed.world_pos.0.to_bits() + 1);
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.world_pos.1 = f64::from_bits(changed.world_pos.1.to_bits() + 1);
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.strength = f32::from_bits(changed.strength.to_bits() + 1);
        assert_ne!(singleton, anchor_set_signature(&[changed]));
        changed = a;
        changed.falloff_radius = f64::from_bits(changed.falloff_radius.to_bits() + 1);
        assert_ne!(singleton, anchor_set_signature(&[changed]));

        let mut all_masked = a;
        all_masked.mask = u8::MAX;
        all_masked.target = PossibilityVector {
            dims: [0.11, 0.22, 0.33, 0.44, 0.55, 0.66, 0.77, 0.88],
        };
        let all_signature = anchor_set_signature(&[all_masked]);
        for i in 0..POSSIBILITY_DIMS {
            let mut one_target_changed = all_masked;
            one_target_changed.target.dims[i] =
                f32::from_bits(one_target_changed.target.dims[i].to_bits() + 1);
            assert_ne!(
                all_signature,
                anchor_set_signature(&[one_target_changed]),
                "masked target {i} must be covered"
            );
        }
    }

    #[test]
    fn raw_float_bit_keys_sort_without_partial_comparisons() {
        let mut anchors = adversarial_anchors();
        anchors[0].world_pos.0 = -0.0;
        anchors[1].world_pos.0 = 0.0;
        anchors[2].strength = f32::from_bits(0x7fc0_0001);
        anchors[3].falloff_radius = f64::from_bits(0x7ff8_0000_0000_0001);
        let canonical = canonical_anchors(&anchors);
        assert_eq!(canonical.len(), anchors.len());
        let _ = anchor_set_signature(&anchors);
    }

    #[test]
    fn suppress_has_final_blend_priority() {
        let mask = domain_mask(&[PossibilityDomain::Ecology]);
        let make = |kind, target, strength| Anchor {
            world_pos: (0.0, 0.0),
            target: bound_target(mask, target),
            mask,
            kind,
            strength,
            falloff_radius: 100.0,
            source: AnchorSource::Manual,
        };
        let base_i = 0.4f32;
        let mut base = PossibilityVector::neutral();
        base.set(PossibilityDomain::Ecology, base_i);
        let emphasize = make(AnchorKind::Emphasize, 0.9, 0.5);
        let suppress = make(AnchorKind::Suppress, 0.8, 0.25);
        let actual =
            steer(base, &[suppress, emphasize], (0.0, 0.0)).get(PossibilityDomain::Ecology);
        let emphasized = base_i + (0.9 - base_i) * 0.5;
        let reflected = (2.0 * base_i - 0.8).clamp(0.0, 1.0);
        let suppress_final = emphasized + (reflected - emphasized) * 0.25;
        let suppressed = base_i + (reflected - base_i) * 0.25;
        let emphasize_final = suppressed + (0.9 - suppressed) * 0.5;
        assert_eq!(actual.to_bits(), suppress_final.to_bits());
        assert_ne!(actual.to_bits(), emphasize_final.to_bits());
    }

    #[test]
    fn projection_caps_vegetation_by_moisture() {
        let mut v = PossibilityVector::neutral();
        v.set(PossibilityDomain::Planetary, 0.0); // dry, ocean-poor world
        v.set(PossibilityDomain::Hydrology, 1.0); // steered fully wet
        v.set(PossibilityDomain::Ecology, 1.0); // steered fully lush
        let p = project_plausible(v);
        // Rule 1: hydrology capped by ocean supply (0.5 + 0.6·0 = 0.5).
        assert!(p.get(PossibilityDomain::Hydrology) <= 0.5 + 1e-6);
        // Rule 2: ecology capped below the fully-lush steer.
        assert!(p.get(PossibilityDomain::Ecology) < 1.0);
    }

    #[test]
    fn projection_is_idempotent_and_neutral_is_a_fixed_point() {
        // The neutral world sits inside every cap: projection must not move it,
        // or a bias-free settle would drift.
        let neutral = PossibilityVector::neutral();
        assert_eq!(project_plausible(neutral).dims, neutral.dims);

        // Projecting a projected vector is a fixed point (single-pass topological
        // relaxation, §7.3) — sweep a range of wild inputs.
        for p in [0.0f32, 0.3, 0.7, 1.0] {
            for c in [0.0f32, 0.5, 1.0] {
                for g in [0.0f32, 0.5, 1.0] {
                    let mut v = PossibilityVector::neutral();
                    v.set(PossibilityDomain::Planetary, p);
                    v.set(PossibilityDomain::Climate, c);
                    v.set(PossibilityDomain::Geology, g);
                    v.set(PossibilityDomain::Hydrology, 1.0);
                    v.set(PossibilityDomain::Ecology, 1.0);
                    v.set(PossibilityDomain::Morphology, 1.0);
                    let once = project_plausible(v);
                    let twice = project_plausible(once);
                    assert_eq!(once.dims, twice.dims, "projection not idempotent");
                }
            }
        }
    }

    #[test]
    fn projection_bounds_body_scale_by_productivity() {
        // Rule 3: a barren (low-Ecology) world caps Morphology (body scale).
        let mut v = PossibilityVector::neutral();
        v.set(PossibilityDomain::Planetary, 0.0);
        v.set(PossibilityDomain::Climate, 0.0);
        v.set(PossibilityDomain::Ecology, 0.0);
        v.set(PossibilityDomain::Morphology, 1.0);
        let p = project_plausible(v);
        // body_cap = 0.3 + 0.7·ecology, and ecology is heavily capped here.
        assert!(p.get(PossibilityDomain::Morphology) <= 0.3 + 1e-6);
    }

    #[test]
    fn projection_ice_rule_bounds_hydrology_in_a_cold_dry_world() {
        // Rule 5: cold + ocean-poor bounds liquid water tighter than rule 1.
        let mut v = PossibilityVector::neutral();
        v.set(PossibilityDomain::Planetary, 0.2);
        v.set(PossibilityDomain::Climate, 0.0); // frozen
        v.set(PossibilityDomain::Hydrology, 1.0);
        let p = project_plausible(v);
        // liquid_cap = 0.2 + 0.5·0 + 0.4·0.2 = 0.28.
        assert!(p.get(PossibilityDomain::Hydrology) <= 0.28 + 1e-6);
    }
}
