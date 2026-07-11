//! `FieldTile`: a region-sized sample buffer plus the provenance it was
//! generated from (phase-1-plan.md sections 4.1, 5, and 8).
//!
//! Staleness is a pure comparison: a tile records the
//! `(world_version, revision)` of the realized state it was built from, so
//! deciding whether it must regenerate never requires re-running generation.
//! Samples are `f32` for Phase 1 clarity; packing to `u8`/`u16`
//! (implementation-plan.md section 15) is a later win once profiling justifies
//! it. Identity inputs (region coords, layer indices) stay integers elsewhere —
//! a tile is pure presentation state.

use crate::hash::mix;

/// Samples per region edge in the Phase 1 field cache: 32×32 per tile,
/// ≈ 4 KB of `f32` per channel (phase-1-plan.md section 5 memory budget).
pub const FIELD_RES: u16 = 32;

/// A square buffer of per-cell samples for one region + one channel, tagged
/// with the provenance it was generated from.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldTile<T> {
    resolution: u16,
    /// [`crate::WORLD_ALGORITHM_VERSION`] at generation time.
    pub world_version: u32,
    /// The region's realized-state revision at generation time.
    pub revision: u32,
    samples: Vec<T>,
}

impl<T: Copy + Default> FieldTile<T> {
    /// A tile of `resolution × resolution` default-valued samples.
    #[must_use]
    pub fn new(resolution: u16, world_version: u32, revision: u32) -> Self {
        let n = resolution as usize * resolution as usize;
        Self {
            resolution,
            world_version,
            revision,
            samples: vec![T::default(); n],
        }
    }

    /// Samples per edge.
    #[inline]
    #[must_use]
    pub const fn resolution(&self) -> u16 {
        self.resolution
    }

    /// Sample at cell `(cx, cy)`. Panics on out-of-range cells (debug tooling
    /// indexes tiles it just created, so this is a programmer error).
    #[inline]
    #[must_use]
    pub fn get(&self, cx: u16, cy: u16) -> T {
        self.samples[cy as usize * self.resolution as usize + cx as usize]
    }

    /// Write the sample at cell `(cx, cy)`.
    #[inline]
    pub fn set(&mut self, cx: u16, cy: u16, value: T) {
        self.samples[cy as usize * self.resolution as usize + cx as usize] = value;
    }

    /// The whole sample buffer, row-major.
    #[inline]
    #[must_use]
    pub fn samples(&self) -> &[T] {
        &self.samples
    }

    /// Whether this tile is out of date for a region currently at
    /// `(world_version, revision)` — a pure comparison, no generation needed.
    #[inline]
    #[must_use]
    pub const fn is_stale(&self, world_version: u32, revision: u32) -> bool {
        self.world_version != world_version || self.revision != revision
    }

    /// Heap bytes held by the sample buffer (cache telemetry, section 12).
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.samples.len() * core::mem::size_of::<T>()
    }
}

impl FieldTile<f32> {
    /// Order-stable hash of the tile's contents and provenance.
    ///
    /// Folds sample *bit patterns*, so it is exact for the platform that
    /// produced the tile. Used by the continuity replay to assert two runs of
    /// the same script produce identical caches — it is not a cross-platform
    /// identity (float presentation state is allowed to differ across targets,
    /// phase-1-plan.md section 8).
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xF1E1_D000_C0FF_EE00;
        h = mix(h, self.world_version as u64);
        h = mix(h, self.revision as u64);
        h = mix(h, self.resolution as u64);
        for s in &self.samples {
            h = mix(h, s.to_bits() as u64);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staleness_is_a_pure_comparison() {
        let tile = FieldTile::<f32>::new(4, 1, 7);
        assert!(!tile.is_stale(1, 7));
        assert!(tile.is_stale(1, 8));
        assert!(tile.is_stale(2, 7));
    }

    #[test]
    fn get_set_round_trip() {
        let mut tile = FieldTile::<f32>::new(8, 1, 0);
        tile.set(3, 5, 0.25);
        assert_eq!(tile.get(3, 5), 0.25);
        assert_eq!(tile.get(0, 0), 0.0);
        assert_eq!(tile.bytes(), 8 * 8 * 4);
    }

    #[test]
    fn content_hash_tracks_contents() {
        let mut a = FieldTile::<f32>::new(4, 1, 0);
        let b = a.clone();
        assert_eq!(a.content_hash(), b.content_hash());
        a.set(1, 1, 0.5);
        assert_ne!(a.content_hash(), b.content_hash());
    }
}
