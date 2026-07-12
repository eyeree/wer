//! Layer regeneration and the region field cache (phase-2-plan.md §5.2, §6).
//!
//! [`generate_layer`] is a *pure* function of `(coord, layer, inputs,
//! resolution)`: it returns an owned [`GeneratedTile`] whose content depends
//! only on those inputs (the `&mut` on `inputs` exists solely to drain the
//! recycled output buffers, phase-6-plan.md §4.2 — never to vary content).
//! Unlike Phase 1 — where every job recomputed its inputs
//! per sample — jobs now receive cheap `Arc` snapshots of their input tiles
//! (phase-2-plan.md §6.2): with a six-deep graph the recomputation redundancy
//! compounds, and macro drainage cannot be recomputed per-sample at all. Tiles
//! are immutable once integrated, so sharing is safe, jobs stay pure, and
//! because content is a function of the dependency key (ADR 0008), results
//! remain order-independent — a *stronger* argument than Phase 1's.

use std::collections::BTreeMap;
use std::sync::Arc;

use world_core::layer::{
    LAYER_BIOME, LAYER_CLIMATE, LAYER_DRAINAGE, LAYER_ECOLOGY, LAYER_GEOLOGY, LAYER_HYDROLOGY,
    LAYER_SOILS, LAYER_TERRAIN, LAYER_VEGETATION,
};
use world_core::simd::{climate_row, elevation_row, hydrology_row, soils_row, vegetation_row};
use world_core::{
    classify, geology, population_from_table, Biome, Climate, DrainageTile, FieldTile,
    HabitatSignature, Hydrology, PossibilityDomain, PossibilityVector, RegionCoord, Soils,
    REGION_SIZE,
};

use crate::rostercache::RosterSnapshot;

/// Elevation channel (world units), produced by the terrain layer.
pub const CHANNEL_ELEVATION: usize = 0;
/// Rock hardness channel (`[0, 1]`), produced by the geology layer.
pub const CHANNEL_HARDNESS: usize = 1;
/// Temperature channel (°C), produced by the climate layer.
pub const CHANNEL_TEMPERATURE: usize = 2;
/// Moisture channel (`[0, 1]`), produced by the climate layer.
pub const CHANNEL_MOISTURE: usize = 3;
/// River presence channel (`[0, 1]`), produced by the hydrology layer.
pub const CHANNEL_RIVER: usize = 4;
/// Surface wetness channel (`[0, 1]`), produced by the hydrology layer.
pub const CHANNEL_WETNESS: usize = 5;
/// Soil depth channel (`[0, 1]`), produced by the soils layer.
pub const CHANNEL_SOIL_DEPTH: usize = 6;
/// Soil fertility channel (`[0, 1]`), produced by the soils layer.
pub const CHANNEL_FERTILITY: usize = 7;
/// Vegetation density channel (`[0, 1]`), produced by the vegetation layer.
pub const CHANNEL_VEGETATION: usize = 8;
/// Canopy height channel (world units), produced by the vegetation layer.
pub const CHANNEL_CANOPY: usize = 9;
/// Herbivore-pressure channel (`[0, 1]`), produced by the ecology layer
/// (phase-3-plan.md §6.1).
pub const CHANNEL_HERBIVORE: usize = 10;
/// Predator-pressure channel (`[0, 1]`), produced by the ecology layer.
pub const CHANNEL_PREDATOR: usize = 11;
/// Species-diversity channel (`[0, 1]`), produced by the ecology layer.
pub const CHANNEL_DIVERSITY: usize = 12;
/// Number of cached `f32` channels per region. Biome ids (`u8`) and the
/// dominant-species index (`u16`) live in honest integer tiles beside the
/// channels, not smuggled through f32 (phase-2-plan.md §6.1, phase-3-plan.md §6.1).
pub const CHANNEL_COUNT: usize = 13;

/// The `f32` channels a layer produces (empty for drainage, which produces a
/// macro tile, and for biome, which produces the u8 tile).
#[must_use]
pub const fn layer_channels(layer: u16) -> &'static [usize] {
    match layer {
        LAYER_TERRAIN => &[CHANNEL_ELEVATION],
        LAYER_GEOLOGY => &[CHANNEL_HARDNESS],
        LAYER_CLIMATE => &[CHANNEL_TEMPERATURE, CHANNEL_MOISTURE],
        LAYER_HYDROLOGY => &[CHANNEL_RIVER, CHANNEL_WETNESS],
        LAYER_SOILS => &[CHANNEL_SOIL_DEPTH, CHANNEL_FERTILITY],
        LAYER_VEGETATION => &[CHANNEL_VEGETATION, CHANNEL_CANOPY],
        LAYER_ECOLOGY => &[CHANNEL_HERBIVORE, CHANNEL_PREDATOR, CHANNEL_DIVERSITY],
        _ => &[],
    }
}

/// The cached sample tiles for one region: one optional shared tile per `f32`
/// channel, the biome id tile, and the dominant-species index tile.
#[derive(Debug, Default, Clone)]
pub struct RegionTiles {
    /// Indexed by the `CHANNEL_*` constants.
    pub channels: [Option<Arc<FieldTile<f32>>>; CHANNEL_COUNT],
    /// Biome classification ids (produced by the biome layer).
    pub biome: Option<Arc<FieldTile<u8>>>,
    /// Dominant-species index into the cell's roster (produced by the ecology
    /// layer). An index, not a global id — `--species` reconstructs the full
    /// identity via the signature (phase-3-plan.md §6.1).
    pub dominant: Option<Arc<FieldTile<u16>>>,
}

impl RegionTiles {
    /// Heap bytes held by this region's tiles.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.channels
            .iter()
            .flatten()
            .map(|t| t.bytes())
            .sum::<usize>()
            + self.biome.as_ref().map_or(0, |t| t.bytes())
            + self.dominant.as_ref().map_or(0, |t| t.bytes())
    }

    /// The stored dependency hash of a layer's output tiles, or `None` if any
    /// of the layer's tiles is missing. All tiles of one layer are integrated
    /// together, so the first channel's hash speaks for the set.
    #[must_use]
    pub fn layer_hash(&self, layer: u16) -> Option<u64> {
        if layer == LAYER_BIOME {
            return self.biome.as_ref().map(|t| t.dep_hash);
        }
        let channels = layer_channels(layer);
        let mut hash = None;
        for &channel in channels {
            let tile = self.channels[channel].as_ref()?;
            hash = Some(tile.dep_hash);
        }
        hash
    }
}

/// Field-tile cache for the active window, owned by
/// [`crate::stream::RegionMap`] and evicted together with region state.
///
/// A `BTreeMap` (not a hash map) so iteration order is deterministic — the
/// continuity replay asserts two runs produce identical caches, and budgeted
/// work must pick the same regions in the same order on every run.
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
    pub fn channel(&self, coord: RegionCoord, channel: usize) -> Option<&Arc<FieldTile<f32>>> {
        self.tiles.get(&coord)?.channels[channel].as_ref()
    }

    /// A region's biome tile.
    #[inline]
    #[must_use]
    pub fn biome(&self, coord: RegionCoord) -> Option<&Arc<FieldTile<u8>>> {
        self.tiles.get(&coord)?.biome.as_ref()
    }

    /// A region's dominant-species index tile.
    #[inline]
    #[must_use]
    pub fn dominant(&self, coord: RegionCoord) -> Option<&Arc<FieldTile<u16>>> {
        self.tiles.get(&coord)?.dominant.as_ref()
    }

    /// Store one channel's tile, creating the region entry as needed.
    /// Returns the superseded tile, if any, so the caller can reclaim its
    /// buffer through the pool (phase-6-plan.md §4.2).
    pub fn insert_channel(
        &mut self,
        coord: RegionCoord,
        channel: usize,
        tile: Arc<FieldTile<f32>>,
    ) -> Option<Arc<FieldTile<f32>>> {
        self.tiles.entry(coord).or_default().channels[channel].replace(tile)
    }

    /// Store a region's biome tile, returning the superseded one.
    pub fn insert_biome(
        &mut self,
        coord: RegionCoord,
        tile: Arc<FieldTile<u8>>,
    ) -> Option<Arc<FieldTile<u8>>> {
        self.tiles.entry(coord).or_default().biome.replace(tile)
    }

    /// Store a region's dominant-species index tile, returning the
    /// superseded one.
    pub fn insert_dominant(
        &mut self,
        coord: RegionCoord,
        tile: Arc<FieldTile<u16>>,
    ) -> Option<Arc<FieldTile<u16>>> {
        self.tiles.entry(coord).or_default().dominant.replace(tile)
    }

    /// Drop every tile for a region (eviction), returning them so their
    /// buffers can be reclaimed (phase-6-plan.md §4.2).
    pub fn remove_region(&mut self, coord: RegionCoord) -> Option<RegionTiles> {
        self.tiles.remove(&coord)
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

/// Recycled output buffers a generation job fills (phase-6-plan.md §4.2),
/// popped from the main-thread [`crate::pool::TilePool`] at dispatch. Empty
/// by default — a job with no pooled buffers allocates fresh ones, so the
/// pool is purely an optimization and content is identical either way.
#[derive(Debug, Default)]
pub struct TileBuffers {
    /// One buffer per `f32` output channel (order irrelevant — all same size).
    pub f32_bufs: Vec<Vec<f32>>,
    /// The biome layer's `u8` buffer.
    pub u8_buf: Option<Vec<u8>>,
    /// The ecology layer's dominant-index `u16` buffer.
    pub u16_buf: Option<Vec<u16>>,
}

/// Everything a layer generation job consumes, snapshotted at dispatch
/// (phase-2-plan.md §5.2). The job never touches shared mutable state.
#[derive(Debug)]
pub struct LayerInputs {
    /// Quantized buckets of the layer's directly-read domains, in stable
    /// domain order (matching `LayerDecl::domains`).
    pub quantized: Vec<u16>,
    /// Input `f32` channel tiles, by `CHANNEL_*` index.
    pub tiles: Vec<(usize, Arc<FieldTile<f32>>)>,
    /// The biome input tile, where declared.
    pub biome: Option<Arc<FieldTile<u8>>>,
    /// The macro drainage input tile, where declared.
    pub drainage: Option<Arc<DrainageTile>>,
    /// The rosters (and food webs) for the signatures this tile will encounter,
    /// resolved by the scheduler at L8 dispatch and keyed by signature — the
    /// Tier-A analogue of the drainage macro input (phase-3-plan.md §5.2, §6.3).
    /// The job looks each cell's signature up in it.
    pub rosters: Option<Arc<RosterSnapshot>>,
    /// The tile's provenance-to-be: the dependency hash the scheduler computed
    /// from exactly these inputs (ADR 0008).
    pub dep_hash: u64,
    /// Recycled output buffers, drained by [`generate_layer`]
    /// (phase-6-plan.md §4.2).
    pub buffers: TileBuffers,
}

impl LayerInputs {
    fn channel(&self, channel: usize) -> Option<&FieldTile<f32>> {
        self.tiles
            .iter()
            .find(|(c, _)| *c == channel)
            .map(|(_, t)| t.as_ref())
    }
}

/// The owned output of one region-layer generation job.
#[derive(Debug)]
pub struct GeneratedTile {
    /// Which region was generated.
    pub coord: RegionCoord,
    /// Which layer produced it.
    pub layer: u16,
    /// The dependency hash the tiles were generated from. Integration relies
    /// on the scheduler's dirty-bit bookkeeping to drop superseded results.
    pub dep_hash: u64,
    /// Dispatch identity assigned by the scheduler. Guards against a stale
    /// orphan (e.g. from an evicted-then-reloaded region) masquerading as the
    /// current job; [`generate_layer`] itself leaves it 0.
    pub job_id: u64,
    /// `(channel, tile)` pairs to store.
    pub channels: Vec<(usize, FieldTile<f32>)>,
    /// The biome tile, when the biome layer generated.
    pub biome: Option<FieldTile<u8>>,
    /// The dominant-species index tile, when the ecology layer generated.
    pub dominant: Option<FieldTile<u16>>,
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

/// Local terrain gradient magnitude (rise/run) at a cell, by central
/// differences over the elevation tile (one-sided at tile edges).
fn slope_at(elevation: &FieldTile<f32>, cx: u16, cy: u16, resolution: u16) -> f32 {
    let step = (REGION_SIZE / f64::from(resolution)) as f32;
    let max = resolution - 1;
    let x0 = cx.saturating_sub(1);
    let x1 = (cx + 1).min(max);
    let y0 = cy.saturating_sub(1);
    let y1 = (cy + 1).min(max);
    let dzdx =
        (elevation.get(x1, cy) - elevation.get(x0, cy)) / (f32::from(x1 - x0).max(1.0) * step);
    let dzdy =
        (elevation.get(cx, y1) - elevation.get(cx, y0)) / (f32::from(y1 - y0).max(1.0) * step);
    (dzdx * dzdx + dzdy * dzdy).sqrt()
}

/// Reconstruct the `Climate` a cell was generated with, from the climate
/// input tiles.
fn climate_at(
    temperature: &FieldTile<f32>,
    moisture: &FieldTile<f32>,
    cx: u16,
    cy: u16,
) -> Climate {
    Climate {
        temperature: temperature.get(cx, cy),
        moisture: moisture.get(cx, cy),
    }
}

/// Generate one layer of one region from a snapshot of its inputs.
///
/// Pure computation: no shared state, no platform services. The possibility
/// values consumed are the *dequantized* buckets of the layer's declared
/// domains — undeclared domains read as the neutral constant, so an undeclared
/// dependency cannot leak into tile content (phase-2-plan.md §4.2).
///
/// `inputs` is `&mut` only to drain its recycled output buffers
/// (phase-6-plan.md §4.2); everything consumed as *input* is untouched, and
/// output content is bit-identical with or without pooled buffers.
#[must_use]
pub fn generate_layer(
    coord: RegionCoord,
    layer: u16,
    inputs: &mut LayerInputs,
    resolution: u16,
) -> GeneratedTile {
    let decl = world_core::layer_decl(layer);
    let p = PossibilityVector::from_quantized(decl.domains, &inputs.quantized);
    let mut channels = Vec::new();
    let mut biome_tile = None;
    let mut dominant_tile = None;
    let mut buffers = core::mem::take(&mut inputs.buffers);
    let inputs = &*inputs;
    let u8_buf = buffers.u8_buf.take().unwrap_or_default();
    let u16_buf = buffers.u16_buf.take().unwrap_or_default();
    let mut new_tile = || {
        FieldTile::<f32>::from_buffer(
            resolution,
            inputs.dep_hash,
            buffers.f32_bufs.pop().unwrap_or_default(),
        )
    };

    match layer {
        LAYER_TERRAIN => {
            // Row-kernel path (phase-6-plan.md §6.1, ADR 0016): bit-identical
            // to the per-cell `elevation` loop, differential-tested.
            let mut tile = new_tile();
            let xs: Vec<f64> = (0..resolution)
                .map(|cx| cell_center(coord, resolution, cx, 0).0)
                .collect();
            for cy in 0..resolution {
                let y = cell_center(coord, resolution, 0, cy).1;
                elevation_row(&xs, y, &p, tile.row_mut(cy));
            }
            channels.push((CHANNEL_ELEVATION, tile));
        }
        LAYER_GEOLOGY => {
            let p_geology = p.get(PossibilityDomain::Geology);
            let mut tile = new_tile();
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    tile.set(cx, cy, geology(x, y, p_geology).hardness);
                }
            }
            channels.push((CHANNEL_HARDNESS, tile));
        }
        LAYER_CLIMATE => {
            let Some(elevation_tile) = inputs.channel(CHANNEL_ELEVATION) else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let mut temperature = new_tile();
            let mut moisture = new_tile();
            for cy in 0..resolution {
                climate_row(
                    elevation_tile.row(cy),
                    &p,
                    temperature.row_mut(cy),
                    moisture.row_mut(cy),
                );
            }
            channels.push((CHANNEL_TEMPERATURE, temperature));
            channels.push((CHANNEL_MOISTURE, moisture));
        }
        LAYER_HYDROLOGY => {
            let (Some(elevation_tile), Some(temperature), Some(moisture), Some(drainage)) = (
                inputs.channel(CHANNEL_ELEVATION),
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.drainage.as_deref(),
            ) else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let p_hydrology = p.get(PossibilityDomain::Hydrology);
            let p_planetary = p.get(PossibilityDomain::Planetary);
            let mut river = new_tile();
            let mut wetness = new_tile();
            let mut slope_row = vec![0f32; resolution as usize];
            let mut accum_row = vec![0f32; resolution as usize];
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    slope_row[cx as usize] = slope_at(elevation_tile, cx, cy, resolution);
                    accum_row[cx as usize] = drainage.accum_bilinear(x, y);
                }
                hydrology_row(
                    elevation_tile.row(cy),
                    &slope_row,
                    &accum_row,
                    temperature.row(cy),
                    moisture.row(cy),
                    p_hydrology,
                    p_planetary,
                    river.row_mut(cy),
                    wetness.row_mut(cy),
                );
            }
            channels.push((CHANNEL_RIVER, river));
            channels.push((CHANNEL_WETNESS, wetness));
        }
        LAYER_SOILS => {
            let (
                Some(elevation_tile),
                Some(hardness),
                Some(temperature),
                Some(moisture),
                Some(_river),
                Some(wetness),
            ) = (
                inputs.channel(CHANNEL_ELEVATION),
                inputs.channel(CHANNEL_HARDNESS),
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.channel(CHANNEL_RIVER),
                inputs.channel(CHANNEL_WETNESS),
            )
            else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let mut depth = new_tile();
            let mut fertility = new_tile();
            let mut slope_row = vec![0f32; resolution as usize];
            // Lithology ids are possibility-independent, so soils reads them
            // through the pure function rather than a cached channel
            // (phase-2-plan.md §6.1); hardness — the cached possibility-
            // dependent expression — comes from the tile.
            let mut lith_row = vec![0u8; resolution as usize];
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    slope_row[cx as usize] = slope_at(elevation_tile, cx, cy, resolution);
                    lith_row[cx as usize] = world_core::lithology_id(x, y);
                }
                soils_row(
                    elevation_tile.row(cy),
                    &slope_row,
                    hardness.row(cy),
                    &lith_row,
                    temperature.row(cy),
                    moisture.row(cy),
                    wetness.row(cy),
                    depth.row_mut(cy),
                    fertility.row_mut(cy),
                );
            }
            channels.push((CHANNEL_SOIL_DEPTH, depth));
            channels.push((CHANNEL_FERTILITY, fertility));
        }
        LAYER_BIOME => {
            let (
                Some(elevation_tile),
                Some(temperature),
                Some(moisture),
                Some(river),
                Some(wetness),
                Some(depth),
                Some(fertility),
            ) = (
                inputs.channel(CHANNEL_ELEVATION),
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.channel(CHANNEL_RIVER),
                inputs.channel(CHANNEL_WETNESS),
                inputs.channel(CHANNEL_SOIL_DEPTH),
                inputs.channel(CHANNEL_FERTILITY),
            )
            else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let mut tile = FieldTile::<u8>::from_buffer(resolution, inputs.dep_hash, u8_buf);
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let c = climate_at(temperature, moisture, cx, cy);
                    let h = Hydrology {
                        river: river.get(cx, cy),
                        wetness: wetness.get(cx, cy),
                    };
                    let s = Soils {
                        depth: depth.get(cx, cy),
                        fertility: fertility.get(cx, cy),
                    };
                    tile.set(
                        cx,
                        cy,
                        classify(elevation_tile.get(cx, cy), &c, &h, &s).id(),
                    );
                }
            }
            biome_tile = Some(tile);
        }
        LAYER_VEGETATION => {
            let (
                Some(temperature),
                Some(moisture),
                Some(depth),
                Some(fertility),
                Some(biome_input),
            ) = (
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.channel(CHANNEL_SOIL_DEPTH),
                inputs.channel(CHANNEL_FERTILITY),
                inputs.biome.as_deref(),
            )
            else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let p_ecology = p.get(PossibilityDomain::Ecology);
            let mut density = new_tile();
            let mut canopy = new_tile();
            for cy in 0..resolution {
                vegetation_row(
                    biome_input.row(cy),
                    temperature.row(cy),
                    moisture.row(cy),
                    depth.row(cy),
                    fertility.row(cy),
                    p_ecology,
                    density.row_mut(cy),
                    canopy.row_mut(cy),
                );
            }
            channels.push((CHANNEL_VEGETATION, density));
            channels.push((CHANNEL_CANOPY, canopy));
        }
        LAYER_ECOLOGY => {
            let (
                Some(temperature),
                Some(moisture),
                Some(fertility),
                Some(vegetation),
                Some(biome_input),
                Some(rosters),
            ) = (
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.channel(CHANNEL_FERTILITY),
                inputs.channel(CHANNEL_VEGETATION),
                inputs.biome.as_deref(),
                inputs.rosters.as_deref(),
            )
            else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            // Ecology is the sole direct reader of E/M/B/A. The aggregate fields
            // are Ecology-driven; M/B/A fold into the dependency hash (via
            // `decl.domains`) because near-field realization expresses genomes
            // under them, so steering M/B/A regenerates L8 and re-realizes its
            // organisms with new expression (phase-3-plan.md §7.5).
            let p_ecology = p.get(PossibilityDomain::Ecology);
            let mut herbivore = new_tile();
            let mut predator = new_tile();
            let mut diversity = new_tile();
            let mut dominant = FieldTile::<u16>::from_buffer(resolution, inputs.dep_hash, u16_buf);
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let c = climate_at(temperature, moisture, cx, cy);
                    // Only fertility feeds the signature; soil depth is not read
                    // by the classifier, so a zero-depth placeholder is exact.
                    let s = Soils {
                        depth: 0.0,
                        fertility: fertility.get(cx, cy),
                    };
                    let biome = Biome::from_id(biome_input.get(cx, cy));
                    let signature = HabitatSignature::of(biome, &c, &s);
                    let productivity = vegetation.get(cx, cy);
                    let sample = match rosters.get(&signature) {
                        // The hoisted table (phase-6-plan.md §6.3): identical
                        // values to the per-cell derivation, O(cells) not
                        // O(cells·roster²).
                        Some(entry) => population_from_table(&entry.table, productivity, p_ecology),
                        // The scheduler snapshots every signature a cell will
                        // encounter (§8.2); a miss is a scheduler bug, kept
                        // non-fatal on worker threads by emitting a barren cell.
                        None => {
                            debug_assert!(false, "ecology cell signature not in snapshot");
                            world_core::PopulationSample {
                                dominant: 0,
                                herbivore: 0.0,
                                predator: 0.0,
                                diversity: 0.0,
                            }
                        }
                    };
                    herbivore.set(cx, cy, sample.herbivore);
                    predator.set(cx, cy, sample.predator);
                    diversity.set(cx, cy, sample.diversity);
                    dominant.set(cx, cy, sample.dominant);
                }
            }
            channels.push((CHANNEL_HERBIVORE, herbivore));
            channels.push((CHANNEL_PREDATOR, predator));
            channels.push((CHANNEL_DIVERSITY, diversity));
            dominant_tile = Some(dominant);
        }
        other => {
            // Drainage generates through the macro path, never here; anything
            // else is a programmer error, kept non-fatal on worker threads.
            debug_assert!(
                other == LAYER_DRAINAGE,
                "generate_layer called with unknown layer {other}"
            );
        }
    }
    GeneratedTile {
        coord,
        layer,
        dep_hash: inputs.dep_hash,
        job_id: 0,
        channels,
        biome: biome_tile,
        dominant: dominant_tile,
    }
}

/// An empty result for a job whose input snapshot was incomplete — a
/// programmer error in the scheduler (it must only dispatch ready layers),
/// kept non-fatal on worker threads.
fn missing_inputs(coord: RegionCoord, layer: u16, dep_hash: u64) -> GeneratedTile {
    debug_assert!(false, "layer {layer} dispatched without its inputs");
    GeneratedTile {
        coord,
        layer,
        dep_hash,
        job_id: 0,
        channels: Vec::new(),
        biome: None,
        dominant: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::climate;
    use world_core::layer::LAYER_COUNT;

    fn terrain_inputs(p: &PossibilityVector) -> LayerInputs {
        let decl = world_core::layer_decl(LAYER_TERRAIN);
        LayerInputs {
            quantized: p.quantized_domains(decl.domains),
            tiles: Vec::new(),
            biome: None,
            drainage: None,
            rosters: None,
            dep_hash: 42,
            buffers: TileBuffers::default(),
        }
    }

    #[test]
    fn every_channel_has_exactly_one_producer() {
        // phase-2-plan.md §12.4.
        let mut producers = [0u32; CHANNEL_COUNT];
        for layer in 0..LAYER_COUNT {
            for &channel in layer_channels(layer) {
                producers[channel] += 1;
            }
        }
        assert_eq!(producers, [1; CHANNEL_COUNT]);
    }

    #[test]
    fn generation_is_pure_and_reproducible() {
        let coord = RegionCoord::new(3, -2);
        let p = PossibilityVector::neutral();
        let a = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(&p), 8);
        let b = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(&p), 8);
        assert_eq!(a.channels.len(), 1);
        for ((ca, ta), (cb, tb)) in a.channels.iter().zip(&b.channels) {
            assert_eq!(ca, cb);
            assert_eq!(ta.content_hash(), tb.content_hash());
        }
    }

    #[test]
    fn climate_consumes_the_terrain_input_tile() {
        let coord = RegionCoord::new(1, 1);
        let p = PossibilityVector::neutral();
        let terrain = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(&p), 8);
        let (_, elevation_tile) = terrain.channels.into_iter().next().unwrap();
        let decl = world_core::layer_decl(LAYER_CLIMATE);
        let mut inputs = LayerInputs {
            quantized: p.quantized_domains(decl.domains),
            tiles: vec![(CHANNEL_ELEVATION, Arc::new(elevation_tile.clone()))],
            biome: None,
            drainage: None,
            rosters: None,
            dep_hash: 7,
            buffers: TileBuffers::default(),
        };
        let out = generate_layer(coord, LAYER_CLIMATE, &mut inputs, 8);
        assert_eq!(out.channels.len(), 2);
        let (channel, temperature) = &out.channels[0];
        assert_eq!(*channel, CHANNEL_TEMPERATURE);
        // Spot-check one cell against the kernel run on the same inputs.
        let expected = climate(
            elevation_tile.get(3, 4),
            &PossibilityVector::from_quantized(decl.domains, &inputs.quantized),
        );
        assert_eq!(temperature.get(3, 4), expected.temperature);
        assert_eq!(temperature.dep_hash, 7);
    }

    #[test]
    fn pooled_buffers_produce_identical_tiles() {
        // The §4.2 plumbing pin: a dirty recycled buffer changes nothing —
        // same fill code, same content hash as a fresh allocation.
        let coord = RegionCoord::new(-4, 9);
        let p = PossibilityVector::neutral();
        let fresh = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(&p), 8);
        let mut pooled_inputs = terrain_inputs(&p);
        pooled_inputs.buffers.f32_bufs.push(vec![f32::NAN; 3]); // dirty, wrong-sized
        let pooled = generate_layer(coord, LAYER_TERRAIN, &mut pooled_inputs, 8);
        assert_eq!(
            fresh.channels[0].1.content_hash(),
            pooled.channels[0].1.content_hash()
        );
    }

    #[test]
    fn cache_round_trip_and_eviction() {
        let mut cache = RegionCache::default();
        let coord = RegionCoord::new(1, 1);
        let out = generate_layer(
            coord,
            LAYER_TERRAIN,
            &mut terrain_inputs(&PossibilityVector::neutral()),
            8,
        );
        for (channel, tile) in out.channels {
            cache.insert_channel(coord, channel, Arc::new(tile));
        }
        assert!(cache.channel(coord, CHANNEL_ELEVATION).is_some());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), 8 * 8 * 4);
        assert_eq!(
            cache.get(coord).unwrap().layer_hash(LAYER_TERRAIN),
            Some(42)
        );
        assert_eq!(cache.get(coord).unwrap().layer_hash(LAYER_CLIMATE), None);
        cache.remove_region(coord);
        assert!(cache.is_empty());
    }
}
