//! `FieldTile`: a region-sized sample buffer plus the dependency hash it was
//! generated from (phase-2-plan.md §4.3, ADR 0008).
//!
//! Staleness is a pure integer comparison: a tile records the
//! [`crate::dephash::layer_dep_hash`] of the exact inputs it was built from,
//! so deciding whether it must regenerate never requires re-running
//! generation — and never over-invalidates, because the hash covers exactly
//! the inputs the producing layer declares. Samples are `f32` (or `u8` for
//! biome ids); a tile is pure presentation state, never an identity.

use crate::hash::mix;

/// Samples per region edge in the field cache: 32×32 per tile, ≈ 4 KB of `f32`
/// per channel (phase-2-plan.md §6.2 memory budget).
pub const FIELD_RES: u16 = 32;

/// A square buffer of per-cell samples for one region + one channel, tagged
/// with the dependency hash it was generated from.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldTile<T> {
    resolution: u16,
    /// The [`crate::dephash::layer_dep_hash`] of the inputs this tile was
    /// generated from. The tile is stale iff this differs from the freshly
    /// computed expected hash (ADR 0008).
    pub dep_hash: u64,
    samples: Vec<T>,
}

impl<T: Copy + Default> FieldTile<T> {
    /// A tile of `resolution × resolution` default-valued samples.
    #[must_use]
    pub fn new(resolution: u16, dep_hash: u64) -> Self {
        let n = resolution as usize * resolution as usize;
        Self {
            resolution,
            dep_hash,
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
    /// phase-2-plan.md §9.3).
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut h = self.provenance_hash();
        for s in &self.samples {
            h = mix(h, s.to_bits() as u64);
        }
        h
    }
}

impl FieldTile<u8> {
    /// Order-stable hash of a categorical (biome id) tile's contents and
    /// provenance — same replay role as the `f32` variant.
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut h = self.provenance_hash();
        for s in &self.samples {
            h = mix(h, *s as u64);
        }
        h
    }
}

impl<T> FieldTile<T> {
    fn provenance_hash(&self) -> u64 {
        let mut h: u64 = 0xF1E1_D000_C0FF_EE00;
        h = mix(h, self.dep_hash);
        h = mix(h, self.resolution as u64);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_round_trip() {
        let mut tile = FieldTile::<f32>::new(8, 1);
        tile.set(3, 5, 0.25);
        assert_eq!(tile.get(3, 5), 0.25);
        assert_eq!(tile.get(0, 0), 0.0);
        assert_eq!(tile.bytes(), 8 * 8 * 4);
    }

    #[test]
    fn u8_tile_round_trips_and_hashes() {
        // Biome tiles are honest u8 buffers (phase-2-plan.md §6.1, §12.4).
        let mut tile = FieldTile::<u8>::new(4, 7);
        tile.set(1, 2, 9);
        assert_eq!(tile.get(1, 2), 9);
        assert_eq!(tile.bytes(), 16);
        let same = tile.clone();
        assert_eq!(tile.content_hash(), same.content_hash());
        let mut other = tile.clone();
        other.set(0, 0, 1);
        assert_ne!(tile.content_hash(), other.content_hash());
    }

    #[test]
    fn content_hash_tracks_contents_and_provenance() {
        let mut a = FieldTile::<f32>::new(4, 10);
        let b = a.clone();
        assert_eq!(a.content_hash(), b.content_hash());
        a.set(1, 1, 0.5);
        assert_ne!(a.content_hash(), b.content_hash());
        let c = FieldTile::<f32>::new(4, 11);
        assert_ne!(b.content_hash(), c.content_hash());
    }
}
