//! Region lifecycle state (sections 6.4 and 6.5 of the plan).
//!
//! Each region distinguishes its *realized* possibility state (what the player
//! currently sees) from its *target* state (what it is converging toward). Nearby
//! regions stay pinned to their realized state; distant regions step toward their
//! target a little each update, which is the mechanism behind seamless
//! transformation without global regeneration.

use world_core::{PossibilityVector, RegionCoord};

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
    pub revision: u32,
    /// Bitset of procedural layers that need recomputation (indexed by layer id).
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
    /// not move. Returns `true` if the realized state changed, bumping `revision`.
    ///
    /// This is the core of continuous transformation: it never snaps, so callers
    /// can budget how much convergence happens per frame (section 6.6).
    pub fn converge(&mut self, rate: f32) -> bool {
        let t = (1.0 - self.stability) * rate.clamp(0.0, 1.0);
        if t <= 0.0 {
            return false;
        }
        let next = self.current.lerp(&self.target, t);
        if next != self.current {
            self.current = next;
            self.revision = self.revision.wrapping_add(1);
            // Any change to realized state dirties all dependent layers; a real
            // implementation will narrow this using the dependency graph
            // (section 6.5). For the bootstrap we conservatively mark everything.
            self.dirty_layers = u32::MAX;
            true
        } else {
            false
        }
    }
}
