//! Layer regeneration and the region field cache (phase-1-plan.md sections
//! 4.2, 5, and 8; milestone M4).
//!
//! [`generate_layer`] is a *pure* function of `(coord, realized vector,
//! resolution)`: it takes immutable inputs and returns an owned
//! [`GeneratedTile`]. That purity is what makes generation jobs safe to run on
//! any thread, safe to supersede, and — critically — independent of completion
//! order (phase-1-plan.md section 9), the same property a future Web Worker
//! executor needs.

use std::collections::BTreeMap;

use world_core::layer::{LAYER_CLIMATE, LAYER_ECOLOGY, LAYER_TERRAIN};
use world_core::{
    climate, elevation, vegetation_density, FieldTile, PossibilityVector, RegionCoord, REGION_SIZE,
    WORLD_ALGORITHM_VERSION,
};

/// Scalar channels cached per region. Layers map onto channels: terrain owns
/// elevation; climate owns temperature and moisture; ecology owns vegetation.
pub const CHANNEL_ELEVATION: usize = 0;
/// Temperature channel (°C), produced by the climate layer.
pub const CHANNEL_TEMPERATURE: usize = 1;
/// Moisture channel (`[0, 1]`), produced by the climate layer.
pub const CHANNEL_MOISTURE: usize = 2;
/// Vegetation-density channel (`[0, 1]`), produced by the ecology layer.
pub const CHANNEL_VEGETATION: usize = 3;
/// Number of cached channels per region.
pub const CHANNEL_COUNT: usize = 4;

/// The cached sample tiles for one region, one optional tile per channel.
#[derive(Debug, Default, Clone)]
pub struct RegionTiles {
    /// Indexed by the `CHANNEL_*` constants.
    pub channels: [Option<FieldTile<f32>>; CHANNEL_COUNT],
}

impl RegionTiles {
    /// Heap bytes held by this region's tiles.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.channels.iter().flatten().map(FieldTile::bytes).sum()
    }
}

/// Field-tile cache for the active window, owned by
/// [`crate::stream::RegionMap`] and evicted together with region state.
///
/// A `BTreeMap` (not a hash map) so iteration order is deterministic — the
/// continuity replay asserts two runs produce identical caches, and budgeted
/// work must pick the same regions in the same order on every run
/// (phase-1-plan.md section 11.3).
#[derive(Debug, Default)]
pub struct RegionCache {
    tiles: BTreeMap<RegionCoord, RegionTiles>,
}

impl RegionCache {
    /// All tiles for a region, if any have been generated.
    #[inline]
    #[must_use]
    pub fn get(&self, coord: RegionCoord) -> Option<&RegionTiles> {
        self.tiles.get(&coord)
    }

    /// One channel's tile for a region.
    #[inline]
    #[must_use]
    pub fn channel(&self, coord: RegionCoord, channel: usize) -> Option<&FieldTile<f32>> {
        self.tiles.get(&coord)?.channels[channel].as_ref()
    }

    /// Store one channel's tile, creating the region entry as needed.
    pub fn insert_channel(&mut self, coord: RegionCoord, channel: usize, tile: FieldTile<f32>) {
        self.tiles.entry(coord).or_default().channels[channel] = Some(tile);
    }

    /// Drop every tile for a region (eviction).
    pub fn remove_region(&mut self, coord: RegionCoord) {
        self.tiles.remove(&coord);
    }

    /// Number of regions with at least one cached tile.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether the cache is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Total heap bytes held by cached tiles (memory telemetry, section 12).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.tiles.values().map(RegionTiles::bytes).sum()
    }

    /// Iterate all cached regions in deterministic coordinate order.
    pub fn iter(&self) -> impl Iterator<Item = (&RegionCoord, &RegionTiles)> {
        self.tiles.iter()
    }
}

/// The owned output of one region-layer generation job.
#[derive(Debug)]
pub struct GeneratedTile {
    /// Which region was generated.
    pub coord: RegionCoord,
    /// Which layer produced it.
    pub layer: u16,
    /// The region revision the job snapshot was taken at. Integration drops
    /// the result if the region has since moved on (supersession).
    pub revision: u32,
    /// Dispatch identity assigned by the scheduler. Guards against a stale
    /// orphan (e.g. from an evicted-then-reloaded region) masquerading as the
    /// current job; [`generate_layer`] itself leaves it 0.
    pub job_id: u64,
    /// `(channel, tile)` pairs to store — climate produces two.
    pub channels: Vec<(usize, FieldTile<f32>)>,
}

/// World-space center of the `(cx, cy)` cell of a region sampled at
/// `resolution`.
#[inline]
fn cell_center(coord: RegionCoord, resolution: u16, cx: u16, cy: u16) -> (f64, f64) {
    let (ox, oy) = coord.origin();
    let step = REGION_SIZE / f64::from(resolution);
    (
        ox + (f64::from(cx) + 0.5) * step,
        oy + (f64::from(cy) + 0.5) * step,
    )
}

/// Generate one layer of one region from a snapshot of its realized state.
///
/// Pure computation: no shared state, no platform services. Climate and
/// ecology recompute elevation from [`elevation`] directly rather than reading
/// the cached terrain tile, so every layer job is independent of every other
/// job's completion (phase-1-plan.md section 9) at the cost of some redundant
/// arithmetic — a deliberate Phase 1 trade.
#[must_use]
pub fn generate_layer(
    coord: RegionCoord,
    layer: u16,
    current: &PossibilityVector,
    revision: u32,
    resolution: u16,
) -> GeneratedTile {
    let mut channels = Vec::new();
    match layer {
        LAYER_TERRAIN => {
            let mut tile = FieldTile::new(resolution, WORLD_ALGORITHM_VERSION, revision);
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    tile.set(cx, cy, elevation(x, y, current));
                }
            }
            channels.push((CHANNEL_ELEVATION, tile));
        }
        LAYER_CLIMATE => {
            let mut temperature = FieldTile::new(resolution, WORLD_ALGORITHM_VERSION, revision);
            let mut moisture = FieldTile::new(resolution, WORLD_ALGORITHM_VERSION, revision);
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    let c = climate(elevation(x, y, current), current);
                    temperature.set(cx, cy, c.temperature);
                    moisture.set(cx, cy, c.moisture);
                }
            }
            channels.push((CHANNEL_TEMPERATURE, temperature));
            channels.push((CHANNEL_MOISTURE, moisture));
        }
        LAYER_ECOLOGY => {
            let mut vegetation = FieldTile::new(resolution, WORLD_ALGORITHM_VERSION, revision);
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    let e = elevation(x, y, current);
                    let c = climate(e, current);
                    vegetation.set(cx, cy, vegetation_density(e, &c, current));
                }
            }
            channels.push((CHANNEL_VEGETATION, vegetation));
        }
        other => {
            // Unknown layer: return an empty result rather than panicking on a
            // worker thread; the integrator treats it as a no-op.
            debug_assert!(false, "generate_layer called with unknown layer {other}");
        }
    }
    GeneratedTile {
        coord,
        layer,
        revision,
        job_id: 0,
        channels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_pure_and_reproducible() {
        let coord = RegionCoord::new(3, -2);
        let p = PossibilityVector::neutral();
        let a = generate_layer(coord, LAYER_CLIMATE, &p, 5, 8);
        let b = generate_layer(coord, LAYER_CLIMATE, &p, 5, 8);
        assert_eq!(a.channels.len(), 2);
        for ((ca, ta), (cb, tb)) in a.channels.iter().zip(&b.channels) {
            assert_eq!(ca, cb);
            assert_eq!(ta.content_hash(), tb.content_hash());
        }
    }

    #[test]
    fn cache_round_trip_and_eviction() {
        let mut cache = RegionCache::default();
        let coord = RegionCoord::new(1, 1);
        let out = generate_layer(coord, LAYER_TERRAIN, &PossibilityVector::neutral(), 0, 8);
        for (channel, tile) in out.channels {
            cache.insert_channel(coord, channel, tile);
        }
        assert!(cache.channel(coord, CHANNEL_ELEVATION).is_some());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), 8 * 8 * 4);
        cache.remove_region(coord);
        assert!(cache.is_empty());
    }
}
