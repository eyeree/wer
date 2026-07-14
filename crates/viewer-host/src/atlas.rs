//! Value types for shared GPU-atlas preparation. Atlas assignment and packing
//! move from the native shell in Milestone 4.

use world_core::RegionCoord;

/// Index of one region tile in the renderer-owned atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AtlasSlot(pub u32);

/// CPU-authoritative presentation currency for one region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasRegionKey {
    /// Region represented by the atlas tile.
    pub region: RegionCoord,
    /// Dependency-hash-derived presentation key.
    pub presentation_key: u64,
}

/// A region-to-slot assignment ready for upload planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasAssignment {
    /// Source region and version.
    pub key: AtlasRegionKey,
    /// Destination slot.
    pub slot: AtlasSlot,
}

/// Refinement request; renderer-facing octave upload structs remain in
/// `renderer` to preserve dependency direction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RefinementRequest {
    /// Whether presentation-only refinement is enabled.
    pub enabled: bool,
    /// Maximum continuation octaves (currently at most three).
    pub octave_count: u8,
}
