//! Shell-side state for the GPU-composed map (phase-6-plan.md §6.5): atlas
//! slot assignment, dependency-hash-keyed delta uploads, and the refinement
//! octave parameters.
//!
//! The shell is the only writer of atlas contents, and everything it uploads
//! is a copy of CPU-authoritative tiles — the GPU path is derived
//! presentation with no way back (ADR 0017).

use std::collections::HashMap;

use renderer::{MapTileUpload, RefineOctaveParams};
use world_core::{RegionCoord, REGION_SIZE};
use world_runtime::{RegionMap, CHANNEL_SLOPE};

use crate::viz::Channel;

/// The GPU channel selector for a [`Channel`], or `None` when the channel is
/// CPU-only (lithology hashes, per-pixel anchor influence, region-state
/// visualizations) and the shell falls back to the CPU composer this frame.
#[must_use]
pub fn gpu_channel(channel: Channel) -> Option<u32> {
    match channel {
        Channel::Composite => Some(0),
        Channel::Elevation => Some(1),
        Channel::Temperature => Some(2),
        Channel::Moisture => Some(3),
        Channel::River => Some(4),
        Channel::Wetness => Some(5),
        Channel::Soil => Some(6),
        Channel::Biome => Some(7),
        Channel::Vegetation => Some(8),
        Channel::Herbivore => Some(9),
        Channel::Predator => Some(10),
        Channel::Diversity => Some(11),
        Channel::Geology
        | Channel::DominantSpecies
        | Channel::Influence
        | Channel::Stability
        | Channel::Revision
        | Channel::Residual => None,
    }
}

/// Assigns visible regions to atlas slots and produces delta uploads: a
/// region re-uploads exactly when its dependency-hash key changes (a tile
/// regenerated or arrived) — steady-state upload traffic is ~zero
/// (phase-6-plan.md §6.5).
#[derive(Debug, Default)]
pub struct AtlasManager {
    slots: HashMap<RegionCoord, (u32, u64)>,
    free: Vec<u32>,
    capacity: u32,
}

impl AtlasManager {
    /// The dependency-hash key of a region's current tiles: which tiles are
    /// present and which inputs they were generated from. Regenerating a tile
    /// changes its dep hash; settling back to identical inputs reproduces the
    /// same key — and, by ADR 0008, the same bytes, so skipping the upload is
    /// exact, not approximate. Hosted by the neutral `RegionMap` (so the
    /// browser shell keys its presentation caches identically); the POV chunk
    /// key folds this tile provenance together with its coherently captured
    /// current Terrain halo (3d-phase-1-plan.md §7.1).
    pub(crate) fn region_key(map: &RegionMap, coord: RegionCoord) -> Option<u64> {
        map.presentation_key(coord)
    }

    /// Sync the atlas to the visible window: assign/evict slots, build the
    /// window slot lookup, and pack uploads for changed regions.
    pub fn sync(
        &mut self,
        map: &RegionMap,
        center: RegionCoord,
        half: i32,
        resolution: u16,
    ) -> (Vec<i32>, Vec<MapTileUpload>) {
        let span = 2 * half + 1;
        let capacity = (span * span) as u32;
        if capacity != self.capacity {
            self.slots.clear();
            self.free = (0..capacity).rev().collect();
            self.capacity = capacity;
        }

        // Evict slots for regions that left the view.
        let visible = |c: &RegionCoord| {
            (c.x - center.x).abs() <= half && (c.y - center.y).abs() <= half && c.level == 0
        };
        let gone: Vec<RegionCoord> = self.slots.keys().filter(|c| !visible(c)).copied().collect();
        for coord in gone {
            if let Some((slot, _)) = self.slots.remove(&coord) {
                self.free.push(slot);
            }
        }

        let mut lookup = vec![-1i32; (span * span) as usize];
        let mut uploads = Vec::new();
        for row in 0..span {
            let ry = center.y + half - row;
            for col in 0..span {
                let rx = center.x - half + col;
                let coord = RegionCoord::new(rx, ry);
                let Some(key) = Self::region_key(map, coord) else {
                    continue;
                };
                let (slot, changed) = match self.slots.get(&coord) {
                    Some(&(slot, old_key)) => (slot, old_key != key),
                    None => {
                        let Some(slot) = self.free.pop() else {
                            continue; // capacity exhausted (cannot happen: window-sized)
                        };
                        (slot, true)
                    }
                };
                if changed {
                    self.slots.insert(coord, (slot, key));
                    if let Some(upload) = pack_region(map, coord, slot, resolution) {
                        uploads.push(upload);
                    }
                }
                lookup[(row * span + col) as usize] = slot as i32;
            }
        }
        (lookup, uploads)
    }
}

/// Pack one region's tiles into the atlas plane layout (§6.5): four
/// rgba32float planes plus the (biome, dominant) integer plane, with a
/// presence bitmask riding in plane3.g so the shader can paint the
/// missing-tile checker exactly like the CPU composer.
fn pack_region(
    map: &RegionMap,
    coord: RegionCoord,
    slot: u32,
    resolution: u16,
) -> Option<MapTileUpload> {
    let tiles = map.cache().get(coord)?;
    let res = usize::from(resolution);
    let texels = res * res;
    let mut planes = [
        vec![0f32; texels * 4],
        vec![0f32; texels * 4],
        vec![0f32; texels * 4],
        vec![0f32; texels * 4],
    ];
    let mut ints = vec![0u16; texels * 2];

    let mut presence = 0u32;
    for (i, tile) in tiles.channels.iter().enumerate() {
        if i == CHANNEL_SLOPE {
            continue;
        }
        if tile.is_some() {
            presence |= 1 << i;
        }
    }
    if tiles.biome.is_some() {
        presence |= 1 << 13;
    }
    if tiles.dominant.is_some() {
        presence |= 1 << 14;
    }

    // (plane, component) per CHANNEL_* index — the shader's layout.
    const SLOT_OF: [(usize, usize); 13] = [
        (0, 0), // elevation
        (0, 1), // hardness
        (0, 2), // temperature
        (0, 3), // moisture
        (1, 0), // river
        (1, 1), // wetness
        (1, 2), // soil depth
        (1, 3), // fertility
        (2, 0), // vegetation
        (2, 1), // canopy
        (2, 2), // herbivore
        (2, 3), // predator
        (3, 0), // diversity
    ];
    for (channel, tile) in tiles.channels.iter().enumerate() {
        if channel == CHANNEL_SLOPE {
            continue;
        }
        let Some(tile) = tile else { continue };
        let (plane, component) = SLOT_OF[channel];
        for (t, &v) in tile.samples().iter().enumerate() {
            planes[plane][t * 4 + component] = v;
        }
    }
    let presence_f = presence as f32;
    for t in 0..texels {
        planes[3][t * 4 + 1] = presence_f;
    }
    if let Some(biome) = &tiles.biome {
        for (t, &b) in biome.samples().iter().enumerate() {
            ints[t * 2] = u16::from(b);
        }
    }
    if let Some(dominant) = &tiles.dominant {
        for (t, &d) in dominant.samples().iter().enumerate() {
            ints[t * 2 + 1] = d;
        }
    }

    Some(MapTileUpload { slot, planes, ints })
}

/// The refinement octave parameters for the current view (phase-6-plan.md
/// §6.5): continue the terrain gradient spectrum above the authoritative
/// resolution, anchored at the view's NW corner in f64 so the shader only
/// needs in-window f32 precision. Presentation-only (ADR 0017).
#[must_use]
pub fn refinement_octaves(
    view_west: f64,
    view_north: f64,
    resolution: u16,
    count: u32,
) -> ([RefineOctaveParams; 3], u32) {
    use world_core::terrain::{octave_offset, BASE_AMPLITUDE, BASE_WAVELENGTH, OCTAVES};
    let mut out = [RefineOctaveParams::default(); 3];
    let count = count.min(3);
    // The scalar fBm norm for the authoritative octaves; refined octaves keep
    // the same halving spectrum, so display amplitude continues it exactly.
    let norm: f32 = (0..OCTAVES).map(|k| 0.5f32.powi(k as i32)).sum();
    let cell = REGION_SIZE / f64::from(resolution);
    for (i, slot) in out.iter_mut().take(count as usize).enumerate() {
        let octave = OCTAVES + i as u32;
        let wavelength = BASE_WAVELENGTH / f64::from(1u32 << octave);
        let (ox, oy) = octave_offset(octave);
        let u0 = view_west / wavelength + ox;
        let v0 = view_north / wavelength + oy;
        let base_ix = u0.floor() as i64;
        let base_iy = v0.floor() as i64;
        *slot = RefineOctaveParams {
            base_ix: base_ix as u64,
            base_iy: base_iy as u64,
            frac: [(u0 - u0.floor()) as f32, (v0 - v0.floor()) as f32],
            inv_wavelength_cells: (cell / wavelength) as f32,
            amplitude: BASE_AMPLITUDE * 0.5f32.powi(octave as i32) / norm,
            octave,
        };
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::{PossibilityField, POSSIBILITY_DIMS};
    use world_runtime::{Budget, InlineExecutor, StreamConfig};

    fn settled_map() -> RegionMap {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 8,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let mut map = RegionMap::new(cfg);
        for _ in 0..6 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        map
    }

    #[test]
    fn delta_uploads_stop_when_nothing_changes() {
        let map = settled_map();
        let center = RegionCoord::new(0, 0);
        let mut atlas = AtlasManager::default();
        let (lookup, uploads) = atlas.sync(&map, center, 2, 8);
        assert!(!uploads.is_empty(), "first sync uploads the window");
        assert!(lookup.iter().any(|&s| s >= 0));
        // Same settled map: nothing changed, nothing uploads.
        let (lookup2, uploads2) = atlas.sync(&map, center, 2, 8);
        assert_eq!(lookup, lookup2, "stable window keeps stable slots");
        assert!(
            uploads2.is_empty(),
            "steady state must upload zero tiles ({} uploaded)",
            uploads2.len()
        );
    }

    #[test]
    fn slots_recycle_when_the_window_moves() {
        let map = settled_map();
        let mut atlas = AtlasManager::default();
        let (_, first) = atlas.sync(&map, RegionCoord::new(0, 0), 2, 8);
        assert!(!first.is_empty());
        // Move the window: departed regions free slots; new ones claim them.
        let (lookup, _) = atlas.sync(&map, RegionCoord::new(1, 0), 2, 8);
        let used: std::collections::BTreeSet<i32> =
            lookup.iter().copied().filter(|&s| s >= 0).collect();
        assert_eq!(used.len(), lookup.iter().filter(|&&s| s >= 0).count());
        let capacity = 5 * 5;
        assert!(used.iter().all(|&s| s < capacity));
    }

    #[test]
    fn packed_presence_mask_matches_tiles() {
        let map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let upload = pack_region(&map, coord, 0, 8).expect("settled region packs");
        let presence = upload.planes[3][1] as u32;
        // A settled region has 13 presented channels plus CPU-only Slope,
        // biome, and dominant.
        assert_eq!(presence & 0x1FFF, 0x1FFF, "all f32 channels present");
        assert_ne!(presence & (1 << 13), 0, "biome present");
        assert_ne!(presence & (1 << 14), 0, "dominant present");
        // Spot-check the elevation texel packing (row-major, plane0.r).
        let tiles = map.cache().get(coord).unwrap();
        let elev = tiles.channels[0].as_ref().unwrap();
        assert_eq!(upload.planes[0][(3 * 8 + 5) * 4], elev.get(5, 3));
        let slope = tiles.channels[CHANNEL_SLOPE].as_ref().unwrap();
        assert!(slope.samples().iter().any(|value| *value > 0.0));
        for texel in 0..64 {
            assert_eq!(upload.planes[3][texel * 4 + 2], 0.0);
            assert_eq!(upload.planes[3][texel * 4 + 3], 0.0);
        }
    }
}
