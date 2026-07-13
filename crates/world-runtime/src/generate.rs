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
use world_core::simd::{climate_row, fbm_row, hydrology_row, soils_row, vegetation_row};
use world_core::{
    classify, geology, population_from_table, Biome, Climate, DrainageTile, FieldTile,
    HabitatSignature, Hydrology, PossibilityDomain, PossibilityVector, RegionCoord, Soils,
    POSSIBILITY_QUANT, REGION_SIZE,
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
/// Terrain slope magnitude (rise/run), produced atomically with Elevation and
/// consumed by Hydrology and Soils. It remains CPU-only presentation state.
pub const CHANNEL_SLOPE: usize = 13;
/// Number of cached `f32` channels per region. Biome ids (`u8`) and the
/// dominant-species index (`u16`) live in honest integer tiles beside the
/// channels, not smuggled through f32 (phase-2-plan.md §6.1, phase-3-plan.md §6.1).
pub const CHANNEL_COUNT: usize = 14;

/// Logical bytes in one fully materialized region field at `resolution`.
///
/// This is the shared admission unit for the field working set: every region
/// eventually owns all `f32` channels plus one biome byte and one dominant
/// species index per cell. It deliberately measures payload bytes rather than
/// allocator overhead (ADR 0023).
#[must_use]
pub const fn full_region_payload_bytes(resolution: u16) -> usize {
    let cells = resolution as usize * resolution as usize;
    cells
        * (CHANNEL_COUNT * std::mem::size_of::<f32>()
            + std::mem::size_of::<u8>()
            + std::mem::size_of::<u16>())
}

/// The `f32` channels a layer produces (empty for drainage, which produces a
/// macro tile, and for biome, which produces the u8 tile).
#[must_use]
pub const fn layer_channels(layer: u16) -> &'static [usize] {
    match layer {
        LAYER_TERRAIN => &[CHANNEL_ELEVATION, CHANNEL_SLOPE],
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

/// Disposable field-tile cache for field-active regions, owned by
/// [`crate::stream::RegionMap`]. Capacity pressure may park these tiles while
/// retaining the region's authoritative possibility history (ADR 0023).
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

/// Ordered Planetary/Geology buckets at the 3×3 absolute region-center halo
/// around one level-0 Terrain tile (ADR 0027).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainPossibilityHalo {
    center: RegionCoord,
    // Row-major dy=-1..=1, dx=-1..=1, [Planetary, Geology].
    buckets: [[[u16; 2]; 3]; 3],
}

impl TerrainPossibilityHalo {
    /// Construct a complete level-0 halo from row-major bucket pairs.
    #[must_use]
    pub fn new(center: RegionCoord, buckets: [[[u16; 2]; 3]; 3]) -> Self {
        assert_eq!(center.level, 0, "Terrain halo center must be level 0");
        debug_assert!(buckets
            .iter()
            .flatten()
            .flatten()
            .all(|bucket| *bucket < POSSIBILITY_QUANT));
        Self { center, buckets }
    }

    /// Center coordinate this snapshot belongs to.
    #[must_use]
    pub const fn center(&self) -> RegionCoord {
        self.center
    }

    /// Planetary/Geology pair for an absolute coordinate inside the halo.
    #[must_use]
    pub fn buckets_at(&self, coord: RegionCoord) -> Option<[u16; 2]> {
        if coord.level != 0 {
            return None;
        }
        let dx = coord.x.checked_sub(self.center.x)?;
        let dy = coord.y.checked_sub(self.center.y)?;
        if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dy) {
            return None;
        }
        Some(self.buckets[(dy + 1) as usize][(dx + 1) as usize])
    }

    /// Exact typed fold sequence: row-major pair order, Planetary then Geology.
    #[must_use]
    pub fn dependency_buckets(&self) -> [u16; 18] {
        let mut out = [0; 18];
        let mut next = 0;
        for row in self.buckets {
            for pair in row {
                out[next] = pair[0];
                out[next + 1] = pair[1];
                next += 2;
            }
        }
        out
    }

    fn axis_sample(base: i32, numerator: i64, denominator: i64) -> (i32, i32, f32) {
        let lower = numerator.div_euclid(denominator);
        let remainder = numerator.rem_euclid(denominator);
        let x0 = i32::try_from(lower).expect("level-0 sample coordinate fits i32");
        let x1 = if remainder == 0 { x0 } else { x0 + 1 };
        debug_assert!((base - 1..=base + 1).contains(&x0));
        debug_assert!((base - 1..=base + 1).contains(&x1));
        (x0, x1, remainder as f32 / denominator as f32)
    }

    fn sample_axes(
        &self,
        x0: i32,
        x1: i32,
        fx: f32,
        y0: i32,
        y1: i32,
        fy: f32,
    ) -> PossibilityVector {
        let pair = |x, y| {
            self.buckets_at(RegionCoord::new(x, y))
                .expect("core/ghost Terrain sample stays within its halo")
        };
        let p00 = pair(x0, y0);
        let p10 = pair(x1, y0);
        let p01 = pair(x0, y1);
        let p11 = pair(x1, y1);
        let sample = |index: usize| {
            let a = PossibilityVector::dequantize(p00[index]);
            let b = PossibilityVector::dequantize(p10[index]);
            let c = PossibilityVector::dequantize(p01[index]);
            let d = PossibilityVector::dequantize(p11[index]);
            let top = a + (b - a) * fx;
            let bottom = c + (d - c) * fx;
            top + (bottom - top) * fy
        };
        let mut out = PossibilityVector::neutral();
        out.set(PossibilityDomain::Planetary, sample(0));
        out.set(PossibilityDomain::Geology, sample(1));
        out
    }

    /// Sample a world position through the same absolute center lattice used
    /// by core/ghost generation. Exact center axes do not fetch an unused
    /// endpoint. This is the presentation-mesher surface (ADR 0027).
    #[must_use]
    pub fn sample_world(&self, world_x: f64, world_y: f64) -> PossibilityVector {
        let axis = |base: i32, world: f64| {
            let lattice = world / REGION_SIZE - 0.5;
            let lower = lattice.floor();
            let fraction = (lattice - lower) as f32;
            let x0 = lower as i32;
            let x1 = if fraction == 0.0 { x0 } else { x0 + 1 };
            debug_assert!((base - 1..=base + 1).contains(&x0));
            debug_assert!((base - 1..=base + 1).contains(&x1));
            (x0, x1, fraction)
        };
        let (x0, x1, fx) = axis(self.center.x, world_x);
        let (y0, y1, fy) = axis(self.center.y, world_y);
        self.sample_axes(x0, x1, fx, y0, y1, fy)
    }

    /// Sample P/G at a core or one-cell ghost position using an exact rational
    /// global cell coordinate. This avoids side-dependent reconstruction of a
    /// shared negative or positive world position.
    #[must_use]
    pub fn sample_cell(&self, resolution: u16, cx: i32, cy: i32) -> PossibilityVector {
        assert!(resolution > 0, "Terrain resolution must be nonzero");
        let n = i64::from(resolution);
        let denominator = 2 * n;
        let x_numerator = (2 * i64::from(self.center.x) - 1) * n + 2 * i64::from(cx) + 1;
        let y_numerator = (2 * i64::from(self.center.y) - 1) * n + 2 * i64::from(cy) + 1;
        let (x0, x1, fx) = Self::axis_sample(self.center.x, x_numerator, denominator);
        let (y0, y1, fy) = Self::axis_sample(self.center.y, y_numerator, denominator);
        self.sample_axes(x0, x1, fx, y0, y1, fy)
    }
}

/// Everything a layer generation job consumes, snapshotted at dispatch
/// (phase-2-plan.md §5.2). The job never touches shared mutable state.
#[derive(Debug)]
pub struct LayerInputs {
    /// Quantized buckets of the layer's directly-read domains, in stable
    /// domain order (matching `LayerDecl::domains`).
    pub quantized: Vec<u16>,
    /// Absolute realized-current/fallback P/G snapshot used only by Terrain.
    pub terrain_halo: Option<TerrainPossibilityHalo>,
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
    /// The dependency hash the tiles were generated from. Integration checks
    /// it against both the dispatch key and the recursively current expected
    /// key; dirty bits are scheduling hints, not provenance (ADR 0019).
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

/// Canonical world coordinate for a core or ghost cell center. The global
/// integer cell index makes overlapping tiles construct identical `f64` bits,
/// including across negative coordinates.
fn cell_center_extended(coord: i32, resolution: u16, cell: i32) -> f64 {
    let global = i64::from(coord) * i64::from(resolution) + i64::from(cell);
    (global as f64 + 0.5) * (REGION_SIZE / f64::from(resolution))
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
            let Some(halo) = inputs.terrain_halo.as_ref() else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            debug_assert_eq!(halo.center(), coord);
            let n = i32::from(resolution);
            let xs: Vec<f64> = (-1..=n)
                .map(|cx| cell_center_extended(coord.x, resolution, cx))
                .collect();
            let mut elevation = new_tile();
            let mut slope = new_tile();
            let mut rows = [
                vec![0.0; n as usize + 2],
                vec![0.0; n as usize + 2],
                vec![0.0; n as usize + 2],
            ];
            let fill_row = |cy: i32, row: &mut [f32]| {
                let y = cell_center_extended(coord.y, resolution, cy);
                fbm_row(&xs, y, row);
                for (index, value) in row.iter_mut().enumerate() {
                    let p = halo.sample_cell(resolution, index as i32 - 1, cy);
                    *value = world_core::terrain::elevation_from_relief(*value, &p);
                }
            };
            fill_row(-1, &mut rows[0]);
            fill_row(0, &mut rows[1]);
            fill_row(1, &mut rows[2]);
            let two_step = 2.0 * (REGION_SIZE / f64::from(resolution)) as f32;
            for cy in 0..n {
                for cx in 0..n {
                    let index = cx as usize + 1;
                    let value = rows[1][index];
                    let dx = (rows[1][index + 1] - rows[1][index - 1]) / two_step;
                    let dy = (rows[2][index] - rows[0][index]) / two_step;
                    elevation.set(cx as u16, cy as u16, value);
                    slope.set(cx as u16, cy as u16, (dx * dx + dy * dy).sqrt());
                }
                if cy + 1 < n {
                    rows.rotate_left(1);
                    fill_row(cy + 2, &mut rows[2]);
                }
            }
            channels.push((CHANNEL_ELEVATION, elevation));
            channels.push((CHANNEL_SLOPE, slope));
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
            let (
                Some(elevation_tile),
                Some(slope),
                Some(temperature),
                Some(moisture),
                Some(drainage),
            ) = (
                inputs.channel(CHANNEL_ELEVATION),
                inputs.channel(CHANNEL_SLOPE),
                inputs.channel(CHANNEL_TEMPERATURE),
                inputs.channel(CHANNEL_MOISTURE),
                inputs.drainage.as_deref(),
            )
            else {
                return missing_inputs(coord, layer, inputs.dep_hash);
            };
            let p_hydrology = p.get(PossibilityDomain::Hydrology);
            let p_planetary = p.get(PossibilityDomain::Planetary);
            let mut river = new_tile();
            let mut wetness = new_tile();
            let mut accum_row = vec![0f32; resolution as usize];
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    accum_row[cx as usize] = drainage.accum_bilinear(x, y);
                }
                hydrology_row(
                    elevation_tile.row(cy),
                    slope.row(cy),
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
                Some(slope),
                Some(hardness),
                Some(temperature),
                Some(moisture),
                Some(_river),
                Some(wetness),
            ) = (
                inputs.channel(CHANNEL_ELEVATION),
                inputs.channel(CHANNEL_SLOPE),
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
            // Lithology ids are possibility-independent, so soils reads them
            // through the pure function rather than a cached channel
            // (phase-2-plan.md §6.1); hardness — the cached possibility-
            // dependent expression — comes from the tile.
            let mut lith_row = vec![0u8; resolution as usize];
            for cy in 0..resolution {
                for cx in 0..resolution {
                    let (x, y) = cell_center(coord, resolution, cx, cy);
                    lith_row[cx as usize] = world_core::lithology_id(x, y);
                }
                soils_row(
                    elevation_tile.row(cy),
                    slope.row(cy),
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
            // Aggregate Ecology directly reads only E. M/B/A are
            // expression-only inputs tracked by the near-field realization key,
            // so they do not regenerate this tile (A.9).
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

    fn patterned_halo(center: RegionCoord) -> TerrainPossibilityHalo {
        let mut buckets = [[[0; 2]; 3]; 3];
        for dy in -1..=1 {
            for dx in -1..=1 {
                let x = center.x + dx;
                let y = center.y + dy;
                buckets[(dy + 1) as usize][(dx + 1) as usize] = [
                    (900 + (x + 8) * 113 + (y + 8) * 31) as u16,
                    (2800 - (x + 8) * 97 + (y + 8) * 43) as u16,
                ];
            }
        }
        TerrainPossibilityHalo::new(center, buckets)
    }

    fn direct_elevation(
        coord: RegionCoord,
        halo: &TerrainPossibilityHalo,
        resolution: u16,
        cx: i32,
        cy: i32,
    ) -> f32 {
        let x = cell_center_extended(coord.x, resolution, cx);
        let y = cell_center_extended(coord.y, resolution, cy);
        let p = halo.sample_cell(resolution, cx, cy);
        world_core::elevation(x, y, &p)
    }

    fn terrain_inputs(coord: RegionCoord, p: &PossibilityVector) -> LayerInputs {
        let decl = world_core::layer_decl(LAYER_TERRAIN);
        let pair = [
            p.quantized(PossibilityDomain::Planetary),
            p.quantized(PossibilityDomain::Geology),
        ];
        LayerInputs {
            quantized: p.quantized_domains(decl.domains),
            terrain_halo: Some(TerrainPossibilityHalo::new(coord, [[pair; 3]; 3])),
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
        assert_eq!(full_region_payload_bytes(32), 60_416);
    }

    #[test]
    fn generation_is_pure_and_reproducible() {
        let coord = RegionCoord::new(3, -2);
        let p = PossibilityVector::neutral();
        let a = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(coord, &p), 8);
        let b = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(coord, &p), 8);
        assert_eq!(a.channels.len(), 2);
        for ((ca, ta), (cb, tb)) in a.channels.iter().zip(&b.channels) {
            assert_eq!(ca, cb);
            assert_eq!(ta.content_hash(), tb.content_hash());
        }
    }

    #[test]
    fn overlapping_halos_sample_shared_positions_bit_exactly() {
        let left_coord = RegionCoord::new(-2, -3);
        let right_coord = RegionCoord::new(-1, -3);
        let left = patterned_halo(left_coord);
        let right = patterned_halo(right_coord);
        for resolution in [1, 7, 8, 32] {
            for cy in [-1, 0, i32::from(resolution)] {
                let a = left.sample_cell(resolution, i32::from(resolution), cy);
                let b = right.sample_cell(resolution, 0, cy);
                for domain in [PossibilityDomain::Planetary, PossibilityDomain::Geology] {
                    assert_eq!(a.get(domain).to_bits(), b.get(domain).to_bits());
                }
                assert_eq!(
                    direct_elevation(left_coord, &left, resolution, i32::from(resolution), cy,)
                        .to_bits(),
                    direct_elevation(right_coord, &right, resolution, 0, cy).to_bits(),
                );
            }
        }

        let diagonal_coord = RegionCoord::new(-1, -2);
        let diagonal = patterned_halo(diagonal_coord);
        let a = left.sample_cell(8, 8, 8);
        let b = diagonal.sample_cell(8, 0, 0);
        assert_eq!(a.dims.map(f32::to_bits), b.dims.map(f32::to_bits));
    }

    #[test]
    fn terrain_stores_centered_ghost_slope_at_edges() {
        let coord = RegionCoord::new(-2, 1);
        let resolution = 8;
        let halo = patterned_halo(coord);
        let mut inputs = terrain_inputs(coord, &PossibilityVector::neutral());
        inputs.terrain_halo = Some(halo.clone());
        let out = generate_layer(coord, LAYER_TERRAIN, &mut inputs, resolution);
        let elevation = &out.channels[0].1;
        let slope = &out.channels[1].1;
        let cx = i32::from(resolution) - 1;
        let cy = 3;
        let step = (REGION_SIZE / f64::from(resolution)) as f32;
        let left = direct_elevation(coord, &halo, resolution, cx - 1, cy);
        let right_ghost = direct_elevation(coord, &halo, resolution, cx + 1, cy);
        let down = direct_elevation(coord, &halo, resolution, cx, cy - 1);
        let up = direct_elevation(coord, &halo, resolution, cx, cy + 1);
        let dx = (right_ghost - left) / (2.0 * step);
        let dy = (up - down) / (2.0 * step);
        let expected = (dx * dx + dy * dy).sqrt();
        assert_eq!(
            slope.get(cx as u16, cy as u16).to_bits(),
            expected.to_bits()
        );

        let old_dx =
            (elevation.get(cx as u16, cy as u16) - elevation.get(cx as u16 - 1, cy as u16)) / step;
        let old_dy = (up - down) / (2.0 * step);
        let old_one_sided = (old_dx * old_dx + old_dy * old_dy).sqrt();
        assert_ne!(expected.to_bits(), old_one_sided.to_bits());
    }

    #[test]
    fn climate_consumes_the_terrain_input_tile() {
        let coord = RegionCoord::new(1, 1);
        let p = PossibilityVector::neutral();
        let terrain = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(coord, &p), 8);
        let (_, elevation_tile) = terrain.channels.into_iter().next().unwrap();
        let decl = world_core::layer_decl(LAYER_CLIMATE);
        let mut inputs = LayerInputs {
            quantized: p.quantized_domains(decl.domains),
            terrain_halo: None,
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
        let fresh = generate_layer(coord, LAYER_TERRAIN, &mut terrain_inputs(coord, &p), 8);
        let mut pooled_inputs = terrain_inputs(coord, &p);
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
            &mut terrain_inputs(coord, &PossibilityVector::neutral()),
            8,
        );
        for (channel, tile) in out.channels {
            cache.insert_channel(coord, channel, Arc::new(tile));
        }
        assert!(cache.channel(coord, CHANNEL_ELEVATION).is_some());
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes(), 8 * 8 * 4 * 2);
        assert_eq!(
            cache.get(coord).unwrap().layer_hash(LAYER_TERRAIN),
            Some(42)
        );
        assert_eq!(cache.get(coord).unwrap().layer_hash(LAYER_CLIMATE), None);
        cache.remove_region(coord);
        assert!(cache.is_empty());
    }
}
