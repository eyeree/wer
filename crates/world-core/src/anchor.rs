//! Anchors: player-placed influences that steer the possibility target
//! (implementation-plan.md section 8; phase-1-plan.md section 6.3, milestone M2).
//!
//! Phase 1 supports the two simplest anchor kinds — Emphasize and Suppress —
//! plus a tiny rule-based plausibility projection. The point is to prove the
//! *seam* between steering and constraints exists, not to model an ecosystem.

use crate::possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS};

/// What an anchor does to the possibility dimensions it touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    /// Push masked dimensions toward 1 (make the tendency more present).
    Emphasize,
    /// Push masked dimensions toward 0 (make the tendency less present).
    Suppress,
}

/// A placed steering influence with smooth radial falloff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Where the anchor sits in continuous world space.
    pub world_pos: (f64, f64),
    /// Bitmask of affected [`PossibilityDomain`]s (bit = `domain.index()`).
    pub mask: u8,
    /// Direction of influence.
    pub kind: AnchorKind,
    /// Peak influence at the anchor's center, `0..=1`.
    pub strength: f32,
    /// World-space radius beyond which the anchor has no effect.
    pub falloff_radius: f64,
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
/// Each anchor moves its masked dimensions proportionally toward 1
/// (Emphasize) or 0 (Suppress) by its influence at `at`. Anchors are applied
/// in slice order; because each step is a contraction toward a bound the
/// result stays in `[0, 1]` and the order sensitivity is mild, but the order
/// is still deterministic (callers keep anchor lists in placement order).
#[must_use]
pub fn steer(base: PossibilityVector, anchors: &[Anchor], at: (f64, f64)) -> PossibilityVector {
    let mut v = base;
    for anchor in anchors {
        let influence = anchor.influence(at);
        if influence <= 0.0 {
            continue;
        }
        for i in 0..POSSIBILITY_DIMS {
            if anchor.mask & (1 << i as u8) == 0 {
                continue;
            }
            match anchor.kind {
                AnchorKind::Emphasize => v.dims[i] += influence * (1.0 - v.dims[i]),
                AnchorKind::Suppress => v.dims[i] -= influence * v.dims[i],
            }
        }
    }
    v
}

/// Project a steered vector back inside plausible bounds
/// (implementation-plan.md section 8: rule-based constraints and iterative
/// relaxation, not machine learning).
///
/// Phase 1 keeps this tiny: clamp to `[0, 1]`, then two ordered relaxation
/// rules. Rule order is part of the deterministic contract (golden-fixtured).
#[must_use]
pub fn project_plausible(mut v: PossibilityVector) -> PossibilityVector {
    for d in v.dims.iter_mut() {
        *d = d.clamp(0.0, 1.0);
    }
    // Rule 1: surface wetness can only mildly exceed what the planetary ocean
    // fraction supplies (a dry world cannot be steered fully swampy).
    let hydrology_cap = (0.5 + 0.6 * v.get(PossibilityDomain::Planetary)).min(1.0);
    if v.get(PossibilityDomain::Hydrology) > hydrology_cap {
        v.set(PossibilityDomain::Hydrology, hydrology_cap);
    }
    // Rule 2: vegetation density is capped by available moisture ("vegetation
    // density versus rainfall"), evaluated after rule 1 so it sees the relaxed
    // hydrology value.
    let ecology_cap = (0.25 + v.get(PossibilityDomain::Hydrology)).min(1.0);
    if v.get(PossibilityDomain::Ecology) > ecology_cap {
        v.set(PossibilityDomain::Ecology, ecology_cap);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(kind: AnchorKind) -> Anchor {
        Anchor {
            world_pos: (0.0, 0.0),
            mask: domain_mask(&[PossibilityDomain::Ecology]),
            kind,
            strength: 0.8,
            falloff_radius: 1000.0,
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
    }

    #[test]
    fn projection_caps_vegetation_by_moisture() {
        let mut v = PossibilityVector::neutral();
        v.set(PossibilityDomain::Planetary, 0.0); // dry world
        v.set(PossibilityDomain::Hydrology, 1.0); // steered fully wet
        v.set(PossibilityDomain::Ecology, 1.0); // steered fully lush
        let p = project_plausible(v);
        assert!(p.get(PossibilityDomain::Hydrology) <= 0.5 + 1e-6);
        assert!(p.get(PossibilityDomain::Ecology) <= 0.75 + 1e-6);
    }
}
