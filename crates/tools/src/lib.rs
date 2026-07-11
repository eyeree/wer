//! `tools` — developer-facing utilities for inspecting and validating the
//! deterministic world model (section 5, `tools/`).
//!
//! The bootstrap ships a single command-line inspector (`wer-inspect`); world
//! visualizers, atlas tools, profiling harnesses, and deterministic-replay tools
//! will land here as their subsystems come online.

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
