//! `tools` — developer-facing utilities for inspecting and validating the
//! deterministic world model (section 5, `tools/`).
//!
//! Phase 1 ships two: `wer-inspect` (position → region, hashes, and generated
//! field samples) and `wer-replay` (the headless continuity replay of
//! phase-1-plan.md section 11.3).

pub mod replay;

pub use replay::{run_continuity_replay, ReplayConfig, ReplayReport};

use world_core::{feature_hash, FeatureKey, RegionCoord, WORLD_ALGORITHM_VERSION};

/// Resolve a world-space position into its region and the deterministic hash of
/// that region's first feature — a minimal, scriptable determinism probe.
#[must_use]
pub fn probe_world_position(world_x: f64, world_y: f64) -> (RegionCoord, u64) {
    let region = RegionCoord::from_world(world_x, world_y);
    let key = FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region,
        layer: 0,
        feature_index: 0,
        possibility_revision: 0,
    };
    (region, feature_hash(&key))
}

/// Everything `wer-inspect` reports for one world position: the region and
/// feature-hash identity plus the Phase 1 generated samples, evaluated at the
/// region's own (anchor-free) possibility target.
#[derive(Debug)]
pub struct PositionReport {
    /// The level-0 region containing the position.
    pub region: RegionCoord,
    /// Deterministic origin-feature hash of that region.
    pub feature_hash: u64,
    /// The region's target vector: field sample → plausibility projection.
    pub target: world_core::PossibilityVector,
    /// Elevation at the position, generated from `target`.
    pub elevation: f32,
    /// Climate at the position.
    pub climate: world_core::Climate,
    /// Aggregate vegetation density at the position.
    pub vegetation: f32,
}

/// Generate the full Phase 1 sample stack for one position (no anchors, no
/// bias — the pure deterministic base the runtime steers from).
#[must_use]
pub fn inspect_world_position(world_x: f64, world_y: f64) -> PositionReport {
    let (region, hash) = probe_world_position(world_x, world_y);
    let field = world_core::PossibilityField::default();
    let target = world_core::project_plausible(field.sample(region));
    let elevation = world_core::elevation(world_x, world_y, &target);
    let climate = world_core::climate(elevation, &target);
    let vegetation = world_core::vegetation_density(elevation, &climate, &target);
    PositionReport {
        region,
        feature_hash: hash,
        target,
        elevation,
        climate,
        vegetation,
    }
}
