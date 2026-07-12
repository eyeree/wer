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

use crate::possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS};

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
/// Order-independent (ADR 0011): the result is a pure function of the *set* of
/// anchors and `at`, not of slice order. For each masked domain, the emphasize
/// anchors that reach `at` contribute a total-influence-weighted pull toward
/// their combined target, and the suppress anchors a weighted push away from
/// theirs (a reflection of the target about the base); the base is then blended
/// toward each combined desired value by a *saturating* weight `1 - ∏(1 - wₐ)`,
/// which keeps the result in `[0, 1]` without the Phase 1 sequential
/// contraction and prevents a single strong anchor from being diluted by many
/// weak far ones. Both the weighted means and the saturating products are
/// symmetric functions of the anchor set, so order cannot perturb the result
/// (§7.2, machine-checked by [`tests::steer_is_order_independent`]).
#[must_use]
pub fn steer(base: PossibilityVector, anchors: &[Anchor], at: (f64, f64)) -> PossibilityVector {
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
        for anchor in anchors {
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
        // The ADR 0011 property: the steered vector is a pure function of the
        // *set* of anchors, not slice order.
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
