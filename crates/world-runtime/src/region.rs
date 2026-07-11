//! Region lifecycle state (sections 6.4 and 6.5 of the plan).
//!
//! Each region distinguishes its *realized* possibility state (what the player
//! currently sees) from its *target* state (what it is converging toward). Nearby
//! regions stay pinned to their realized state; distant regions step toward their
//! target a little each update, which is the mechanism behind seamless
//! transformation without global regeneration.

use world_core::{PossibilityDomain, PossibilityVector, RegionCoord};

/// Where a region is in the generation pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationStatus {
    /// Known to exist but nothing generated yet.
    Unloaded,
    /// One or more layers are queued or in flight.
    Generating,
    /// All required layers for the current LOD are up to date.
    Ready,
}

/// The tracked state of a single region (section 6.4 lists the required fields).
#[derive(Debug, Clone)]
pub struct RegionState {
    /// Stable integer identity.
    pub coord: RegionCoord,
    /// Currently realized possibility state.
    pub current: PossibilityVector,
    /// Possibility state the region is converging toward.
    pub target: PossibilityVector,
    /// 0 = free to transform, 1 = fully pinned (near the player).
    pub stability: f32,
    /// Monotonic revision, bumped whenever `current` changes materially.
    /// Kept for the pinned-stability contract and the continuity replay; it
    /// no longer drives staleness — dependency hashes do (phase-2-plan.md
    /// §4.3, ADR 0008).
    pub revision: u32,
    /// Bitset of layers whose dependency hash may have changed (indexed by
    /// layer id). An optimization hint over the dep-hash ground truth: regions
    /// with no set bits skip hash checks entirely (phase-2-plan.md §7.8).
    pub dirty_layers: u32,
    /// Pipeline status.
    pub status: GenerationStatus,
}

impl RegionState {
    /// A freshly-known region: realized == target == neutral, nothing generated.
    #[must_use]
    pub fn new(coord: RegionCoord) -> Self {
        Self {
            coord,
            current: PossibilityVector::neutral(),
            target: PossibilityVector::neutral(),
            stability: 1.0,
            revision: 0,
            dirty_layers: 0,
            status: GenerationStatus::Unloaded,
        }
    }

    /// Step `current` toward `target`, scaled by how *unstable* the region is and
    /// by `rate` (a per-update fraction). Pinned regions (`stability == 1.0`) do
    /// not move.
    ///
    /// Returns `None` if the realized state did not change; otherwise the
    /// bitmask of possibility domains whose *quantized bucket* flipped (bit =
    /// `domain.index()`), which may be empty — sub-bucket drift costs zero
    /// regeneration (phase-2-plan.md §4.2). Callers translate flipped buckets
    /// into dirty layers via the declared graph
    /// ([`world_core::layer::domain_dirty_mask`]); this module deliberately
    /// knows nothing about layers (ADR 0007 superseded the static drift mask).
    ///
    /// This is the core of continuous transformation: it never snaps, so callers
    /// can budget how much convergence happens per frame (section 6.6).
    pub fn converge(&mut self, rate: f32) -> Option<u8> {
        let t = (1.0 - self.stability) * rate.clamp(0.0, 1.0);
        if t <= 0.0 {
            return None;
        }
        let next = self.current.lerp(&self.target, t);
        if next == self.current {
            return None;
        }
        let mut flipped = 0u8;
        for (i, domain) in PossibilityDomain::ALL.iter().enumerate() {
            if self.current.quantized(*domain) != next.quantized(*domain) {
                flipped |= 1 << i;
            }
        }
        self.current = next;
        self.revision = self.revision.wrapping_add(1);
        Some(flipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converge_reports_flipped_buckets() {
        let mut region = RegionState::new(RegionCoord::new(0, 0));
        region.stability = 0.0;
        region.target.set(PossibilityDomain::Climate, 0.9);
        let flipped = region.converge(0.5).expect("state moved");
        assert_ne!(
            flipped & (1 << PossibilityDomain::Climate.index()),
            0,
            "a 0.2 step must cross climate buckets"
        );
        assert_eq!(
            flipped & (1 << PossibilityDomain::Geology.index()),
            0,
            "untouched domains must not flip"
        );
        assert_eq!(region.revision, 1);
    }

    #[test]
    fn sub_bucket_drift_reports_no_flips() {
        let mut region = RegionState::new(RegionCoord::new(0, 0));
        region.stability = 0.0;
        // A target offset far below one bucket (1/4096).
        let base = region.current.get(PossibilityDomain::Ecology);
        region
            .target
            .set(PossibilityDomain::Ecology, base + 0.00001);
        if let Some(flipped) = region.converge(1.0) {
            assert_eq!(flipped, 0, "sub-bucket drift must not flip buckets");
        }
    }

    #[test]
    fn pinned_regions_never_move() {
        let mut region = RegionState::new(RegionCoord::new(0, 0));
        region.stability = 1.0;
        region.target.set(PossibilityDomain::Ecology, 1.0);
        let before = region.current;
        assert!(region.converge(1.0).is_none());
        assert_eq!(region.current, before);
        assert_eq!(region.revision, 0);
    }
}
