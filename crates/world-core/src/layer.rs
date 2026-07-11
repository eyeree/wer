//! The Phase 1 procedural layer stack (phase-1-plan.md section 4.1 and 8).
//!
//! Phase 1 deliberately uses a short, hard-coded 3-layer stack — terrain,
//! climate, ecology — instead of the full layered dependency graph of
//! `implementation-plan.md` section 6.5 (that generalization is Phase 2 work).
//! Layer ids are stable integers because they participate in feature identity
//! hashing ([`crate::FeatureKey::layer`]).

/// Stable topology (elevation). Depends only on world position and the slow
/// Geology/Planetary possibility dimensions, so possibility *drift* never
/// dirties it — this is the core continuity commitment (plan section 6.1).
pub const LAYER_TERRAIN: u16 = 0;

/// Temperature and moisture. Cheap, recomputed whenever a region's realized
/// possibility state changes.
pub const LAYER_CLIMATE: u16 = 1;

/// Aggregate vegetation density. Recomputed alongside climate on drift.
pub const LAYER_ECOLOGY: u16 = 2;

/// Number of layers in the Phase 1 stack.
pub const LAYER_COUNT: u16 = 3;

/// The dirty-bitset bit for a layer id.
#[inline]
#[must_use]
pub const fn layer_bit(layer: u16) -> u32 {
    1 << layer
}

/// Layers that depend on the realized possibility state and therefore go stale
/// when a region's `current` vector drifts. Terrain is deliberately excluded:
/// possibility drift moves climate and ecology, never the mountains
/// (phase-1-plan.md section 6.4 — the incremental-regeneration narrowing).
pub const DRIFT_LAYERS: u32 = layer_bit(LAYER_CLIMATE) | layer_bit(LAYER_ECOLOGY);

/// Every layer in the Phase 1 stack — the dirty mask for a freshly loaded
/// region that has generated nothing yet.
pub const ALL_LAYERS: u32 = (1 << LAYER_COUNT) - 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_excludes_terrain() {
        assert_eq!(DRIFT_LAYERS & layer_bit(LAYER_TERRAIN), 0);
        assert_ne!(DRIFT_LAYERS & layer_bit(LAYER_CLIMATE), 0);
        assert_ne!(DRIFT_LAYERS & layer_bit(LAYER_ECOLOGY), 0);
    }

    #[test]
    fn all_layers_covers_the_stack() {
        assert_eq!(ALL_LAYERS, 0b111);
        assert_eq!(ALL_LAYERS & DRIFT_LAYERS, DRIFT_LAYERS);
    }
}
