//! Region streaming: the moving window of active regions, the distance-driven
//! stability ramp, budgeted convergence, and dependency-precise incremental
//! regeneration (phase-1-plan.md sections 4.2 and 7; phase-2-plan.md §7.8, §8).
//!
//! [`RegionMap::update`] is the once-per-frame heart of the runtime:
//!
//! 1. integrate finished generation jobs,
//! 2. evict regions beyond `unload_radius` (hysteresis vs `load_radius`),
//!    sweeping orphaned macro drainage tiles with them,
//! 3. load missing regions nearest-first,
//! 4. recompute every region's stability and steered target,
//! 5. converge distant regions toward their targets (farthest-first); flipped
//!    possibility buckets dirty exactly the declared reader layers and their
//!    transitive dependents (ADR 0007),
//! 6. dispatch stale region-layers topologically (phase-2-plan.md §8.1): a
//!    fixed-point loop that, per pass, submits every layer whose dependency
//!    hash mismatches and whose inputs are fresh, then integrates and goes
//!    again — so a synchronous executor settles a whole region bottom-up in
//!    one frame, while a threaded executor settles it over a handful of
//!    frames, deepest layers last.
//!
//! Staleness is dependency-hash comparison — the ground truth (ADR 0008).
//! `dirty_layers` is a conservative scheduling hint over it: convergence,
//! upstream integration, macro regeneration, and revision bumps set bits when
//! a layer's expected hash may have changed, while dependency repair restores
//! omitted hints on demand. Job ids protect dispatch identity; dispatch and
//! current dependency keys protect content provenance at integration (ADR
//! 0019). Everything is budgeted (section 6.6), regeneration by declared
//! *cost* rather than count (phase-2-plan.md §8.2), so a big possibility
//! change ripples outward over several frames instead of hitching.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use world_core::layer::{
    all_layers_mask, dependents_closure, domain_dirty_mask, layer_bit, layer_decl, LAYER_COUNT,
    LAYER_DRAINAGE, LAYER_ECOLOGY,
};
use world_core::{
    capture_target, domain_mask, drainage_dep_hash, layer_dep_hash, macro_coord_for, mix,
    organism_trait_deviation, project_plausible, steer, Anchor, AnchorKind, AnchorSource, Biome,
    Climate, DrainageTile, Genome, GenomeBias, HabitatSignature, PossibilityDomain,
    PossibilityField, PossibilityVector, RegionCoord, Soils, TraitDeviation, POSSIBILITY_DIMS,
    REGION_SIZE,
};

use crate::budget::Budget;
use crate::generate::{
    generate_layer, layer_channels, GeneratedTile, LayerInputs, RegionCache, RegionTiles,
    TileBuffers, CHANNEL_DIVERSITY, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_HERBIVORE,
    CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION,
    CHANNEL_WETNESS,
};
use crate::macrocache::MacroCache;
use crate::pool::TilePool;
use crate::realize::{realize_region_into, Organism};
use crate::region::{GenerationStatus, RegionState};
use crate::resonance::{
    combine_resonance, density_term, gated_rate, species_entropy, Resonance, ResonanceNode,
};
use crate::rostercache::{RosterCache, RosterEntry, RosterSnapshot};
use crate::task::{TaskExecutor, TaskPriority};
use crate::timing::{Pass, PassTimings, PASS_COUNT};

/// How far a capture may pull the world past its habitat baseline
/// (phase-4-plan.md §7.1). Bounds the "distinctiveness" of a captured anchor:
/// even a wildly atypical discovery moves the target a bounded step, so steering
/// stays inside the plausibility projection's reach.
const CAPTURE_GAIN: f32 = 0.5;

/// Convergence-rate scale in transition mode versus free movement
/// (phase-4-plan.md §7.6, §8.2): deliberate reality-transition travel steers
/// slowly and precisely; free exploration surveys at the full rate.
const TRANSITION_CONVERGE_SCALE: f32 = 0.35;

/// Distance thresholds and rates for the streaming window. All radii are world
/// units measured from the player to a region's center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Inside this radius regions are fully pinned (`stability = 1`): the
    /// ground the player can clearly see never rewrites itself.
    pub near_radius: f64,
    /// Beyond this radius regions are free (`stability = 0`); between near and
    /// far a smoothstep ramp blends the two (phase-1-plan.md section 7.2).
    pub far_radius: f64,
    /// Regions within this radius are kept resident.
    pub load_radius: f64,
    /// Regions beyond this radius are evicted. Must exceed `load_radius`; the
    /// gap is the hysteresis that prevents thrashing at the boundary.
    pub unload_radius: f64,
    /// Convergence fraction per world unit of player travel. Transformation is
    /// fueled by the journey, not by wall-clock time: a stationary player's
    /// world holds perfectly still, so change can never silently bank up into
    /// an old/new cliff at the pinned boundary while they stand and look
    /// (ADR 0006). The per-update rate is `converge_per_unit * travel`,
    /// clamped to `converge_rate_cap`.
    pub converge_per_unit: f32,
    /// Upper bound on the effective per-update convergence rate, so sprinting
    /// (or a scripted fast camera) accelerates transformation only up to a
    /// still-smooth step size.
    pub converge_rate_cap: f32,
    /// Samples per region edge for generated field tiles.
    pub field_resolution: u16,
    /// Byte ceiling for the field-tile cache (phase-6-plan.md §4.3): after
    /// the radius sweep, the capacity evictor removes farthest-first until
    /// under this, exempting preserved regions and everything inside
    /// `near_radius`; the loader defers non-near loads that would exceed it.
    /// Always safe — every tile re-derives from its dependency hash
    /// (ADR 0008) — so a ceiling costs recompute, never correctness.
    pub max_field_cache_bytes: usize,
    /// Byte target for the macro drainage cache. Evicted tiles re-derive
    /// lazily the next time a dependent's hydrology goes stale; a demanded
    /// result is retained transiently until Hydrology snapshots it.
    pub max_macro_cache_bytes: usize,
    /// Byte target for the roster cache. Disposable entries evict
    /// deterministically; the required resident working set is repaired and
    /// may exceed this target (`RosterCache::ensure`, ADR 0019).
    pub max_roster_cache_bytes: usize,
    /// Near-field organisms realized per cell (phase-6-plan.md §6.6): the
    /// High-tier density lever. Slot 0 keeps the exact Phase 5 identities;
    /// slots 1.. derive additive identities (`feature_index = cell +
    /// slot·res²`) from the same scheme, each independently density-gated so
    /// expected population scales linearly and the aggregate↔entity ratios
    /// hold. Presentation state only (ADR 0010): no persisted or shared byte
    /// changes with this knob.
    pub organisms_per_cell: u16,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            near_radius: 3.0 * REGION_SIZE,
            far_radius: 9.0 * REGION_SIZE,
            load_radius: 12.0 * REGION_SIZE,
            unload_radius: 14.0 * REGION_SIZE,
            // Walking speed (~8 units/frame at 60 fps) yields a rate ≈ 0.08;
            // a 4× sprint saturates at the cap.
            converge_per_unit: 0.01,
            converge_rate_cap: 0.2,
            field_resolution: world_core::FIELD_RES,
            // Low-tier ceilings (§7.4). The Phase 5 default window uses
            // ~34 MB of field tiles, so these change nothing until a tier
            // (or a misconfigured window) pushes past them.
            max_field_cache_bytes: 48 * 1024 * 1024,
            max_macro_cache_bytes: 12 * 1024 * 1024,
            max_roster_cache_bytes: 8 * 1024 * 1024,
            // One per cell: the proven Phase 5 realization density; tiers
            // opt in to more (phase-6-plan.md §6.6).
            organisms_per_cell: 1,
        }
    }
}

/// Per-frame counters (phase-2-plan.md §8.2, §13). `deferred_*` report budget
/// backpressure — expected and healthy, not an error.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameStats {
    /// Regions inserted this frame.
    pub loaded: usize,
    /// Regions evicted this frame.
    pub evicted: usize,
    /// Regions whose realized state actually moved this frame.
    pub converged: usize,
    /// Generation jobs dispatched this frame (macro jobs included).
    pub layers_dispatched: usize,
    /// Finished region-layers integrated into the cache this frame.
    pub layers_regenerated: usize,
    /// Finished region-layers integrated, by layer id.
    pub regenerated_by_layer: [usize; LAYER_COUNT as usize],
    /// Macro drainage tiles integrated this frame.
    pub macro_jobs: usize,
    /// Generation cost units dispatched this frame (≤ `max_regen_cost`).
    pub regen_cost_spent: u32,
    /// Loads that missed the per-frame budget.
    pub deferred_loads: usize,
    /// Converge candidates that missed the budget.
    pub deferred_converges: usize,
    /// Stale, ready region-layers that missed the dispatch cost budget.
    pub deferred_regens: usize,
    /// Resident regions after this frame.
    pub active_regions: usize,
    /// Heap bytes held by the field cache after this frame.
    pub cache_bytes: usize,
    /// Heap bytes held by the macro drainage cache after this frame.
    pub macro_cache_bytes: usize,
    /// `(roster, food web)` entries built (not cache-served) this frame
    /// (phase-3-plan.md §8.4).
    pub rosters_built: usize,
    /// Heap bytes held by the roster cache after this frame.
    pub roster_cache_bytes: usize,
    /// Near-field organisms instantiated this frame (≤ `max_realize_organisms`
    /// modulo a final region overshoot, phase-3-plan.md §8.4).
    pub organisms_realized: usize,
    /// Total near-field organisms resident after this frame.
    pub organisms: usize,
    /// Transition capability at the player this frame, `0..=1` — the resonance
    /// gate multiplier folded into convergence (phase-4-plan.md §8.3, ADR 0012).
    pub resonance_strength: f32,
    /// Contributing nodes in this frame's resonance graph (≤
    /// `max_resonance_nodes`).
    pub resonance_nodes: usize,
    /// Active anchors this frame.
    pub anchors_active: usize,
    /// Milliseconds per update pass, indexed by [`Pass::index`]
    /// (phase-6-plan.md §5.2). Telemetry only — never gated, never hashed
    /// (§12.6); all zeros without the `pass-timing` feature. The `Flush`
    /// slot is filled by the shell around its vault flush.
    pub pass_ms: [f32; PASS_COUNT],
    /// Jobs whose cancellation token was flipped (superseded or evicted)
    /// before their kernel ran — worker time saved, not an error
    /// (phase-6-plan.md §6.2).
    pub jobs_cancelled: usize,
    /// Results that arrived but were dropped as superseded/orphaned. With
    /// cancellation on, doomed jobs mostly never run, so this falls toward
    /// zero; the pair (`jobs_cancelled`, `results_dropped`) is the §11.4
    /// "cancellation reduces jobs-run" counter gate.
    pub results_dropped: usize,
    /// Tile-pool buffers served from the pool at dispatch (phase-6-plan.md
    /// §4.2).
    pub pool_hits: usize,
    /// Tile-pool requests that fell back to a fresh allocation.
    pub pool_misses: usize,
    /// Heap bytes idling in the tile pool after this frame.
    pub pool_bytes: usize,
    /// Regions evicted by the byte-capacity ceiling (beyond the radius sweep;
    /// phase-6-plan.md §4.3).
    pub evicted_for_capacity: usize,
    /// Resident regions whose retarget was deferred to a later frame by
    /// `max_retarget_regions` (phase-6-plan.md §6.4) — round-robin
    /// backpressure, not an error.
    pub retarget_deferred: usize,
}

/// Order-stable hash of the steering inputs (bias + anchors) — a change
/// forces a full retarget instead of the amortized round-robin
/// (phase-6-plan.md §6.4). Bit-exact over every field an anchor steers with.
fn steering_signature(anchors: &[Anchor], bias: &[f32; POSSIBILITY_DIMS]) -> u64 {
    let mut h: u64 = 0x5EED_5163_0000_0006;
    for b in bias {
        h = mix(h, u64::from(b.to_bits()));
    }
    for anchor in anchors {
        h = mix(h, anchor.world_pos.0.to_bits());
        h = mix(h, anchor.world_pos.1.to_bits());
        for d in anchor.target.dims {
            h = mix(h, u64::from(d.to_bits()));
        }
        h = mix(h, u64::from(anchor.mask));
        h = mix(
            h,
            match anchor.kind {
                AnchorKind::Emphasize => 1,
                AnchorKind::Suppress => 2,
            },
        );
        h = mix(h, u64::from(anchor.strength.to_bits()));
        h = mix(h, anchor.falloff_radius.to_bits());
    }
    h
}

/// Distance from the player to a region's center.
fn center_distance(coord: RegionCoord, player: (f64, f64)) -> f64 {
    let (ox, oy) = coord.origin();
    let cx = ox + REGION_SIZE * 0.5;
    let cy = oy + REGION_SIZE * 0.5;
    let dx = cx - player.0;
    let dy = cy - player.1;
    (dx * dx + dy * dy).sqrt()
}

/// The distance→stability ramp (phase-1-plan.md section 7.2): 1 inside
/// `near_radius`, 0 beyond `far_radius`, smoothstep between.
#[must_use]
pub fn stability_for(cfg: &StreamConfig, distance: f64) -> f32 {
    if distance <= cfg.near_radius {
        1.0
    } else if distance >= cfg.far_radius {
        0.0
    } else {
        let t = ((distance - cfg.near_radius) / (cfg.far_radius - cfg.near_radius)) as f32;
        let s = t * t * (3.0 - 2.0 * t);
        1.0 - s
    }
}

/// One layer's staleness explanation for one region — the data behind
/// `wer-inspect --layers`, which makes invalidation *legible*
/// (phase-2-plan.md §11).
#[derive(Debug, Clone)]
pub struct LayerDiagnostic {
    /// The layer id.
    pub layer: u16,
    /// Quantized buckets of the layer's directly-read domains, in stable
    /// domain order.
    pub buckets: Vec<u16>,
    /// The dependency hash recursively expected from current authoritative
    /// state. `None` only when the region is no longer resident; cache absence
    /// is a readiness concern, not a provenance gap (ADR 0019).
    pub expected: Option<u64>,
    /// The stored dependency hash of the cached tiles, if generated.
    pub stored: Option<u64>,
    /// Whether a job for this layer is currently in flight.
    pub in_flight: bool,
    /// Whether the scheduler's dirty hint is set.
    pub dirty: bool,
}

impl LayerDiagnostic {
    /// The staleness verdict: stale unless the stored hash equals a computable
    /// expected hash.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match (self.stored, self.expected) {
            (Some(stored), Some(expected)) => stored != expected,
            _ => true,
        }
    }
}

/// The ecology readout for one cell (phase-3-plan.md §11): the habitat
/// signature, its memoized roster/web, the dominant species, the trophic
/// breakdown, and the aggregate L8 field values. Behind `wer-inspect
/// --species` / `--ecology` and the info panel.
#[derive(Debug, Clone)]
pub struct CellEcology {
    /// The cell's habitat signature.
    pub signature: HabitatSignature,
    /// The memoized roster and food web for the signature.
    pub roster: Arc<RosterEntry>,
    /// Index of the dominant species within the roster.
    pub dominant_index: u16,
    /// Stable id of the dominant species (`0` if the roster is empty).
    pub dominant_id: u64,
    /// Species count per [`world_core::Trophic`] variant (indexed by its
    /// discriminant).
    pub trophic_counts: [usize; 5],
    /// Herbivore pressure at the cell, if the L8 tile exists.
    pub herbivore: Option<f32>,
    /// Predator pressure at the cell.
    pub predator: Option<f32>,
    /// Species diversity at the cell.
    pub diversity: Option<f32>,
}

/// A finished job flowing back to the integrator.
#[derive(Debug)]
enum JobResult {
    /// A region-layer tile set.
    Tile(GeneratedTile),
    /// A macro drainage tile.
    Macro {
        coord: RegionCoord,
        job_id: u64,
        tile: DrainageTile,
    },
}

/// The active window of regions plus their field and macro caches.
///
/// Region state lives in a `BTreeMap`, not a hash map: iteration order is part
/// of the determinism contract. Budgeted work (loads, convergence, regen) must
/// pick the same regions in the same order on every run for the continuity
/// replay's two-run equality check to hold.
#[derive(Debug)]
pub struct RegionMap {
    cfg: StreamConfig,
    regions: BTreeMap<RegionCoord, RegionState>,
    cache: RegionCache,
    macro_cache: MacroCache,
    /// Memoized `(roster, food web)` per habitat signature (phase-3-plan.md §6.3).
    roster_cache: RosterCache,
    /// The signatures each region's L8 tile currently uses, recorded at L8
    /// dispatch — the dependent set the roster cache evicts against.
    region_signatures: BTreeMap<RegionCoord, BTreeSet<HabitatSignature>>,
    /// Realized near-field organisms per pinned near region (Tier B, transient
    /// and un-cached, phase-3-plan.md §8.3). Rebuilt when a region's L8 tile
    /// changes, dropped when it leaves the near window.
    organisms: BTreeMap<RegionCoord, Vec<Organism>>,
    /// The L8 dependency hash each region's organisms were realized from, so
    /// re-realization triggers exactly when the aggregate changes.
    organism_keys: BTreeMap<RegionCoord, u64>,
    /// Persistent possibility overrides (phase-5-plan.md §7.5): regions pinned
    /// to a quantized possibility state by a preserve. Consulted by the load
    /// path (current/target from the buckets, stability 1) and honoured by
    /// retarget/converge; deliberately **not** cleared on eviction — surviving
    /// eviction is the override's job. A few dozen bytes per preserved region.
    overrides: BTreeMap<RegionCoord, world_core::PossibilitySignature>,
    /// Run-local additions to each layer's declared `algorithm_revision`
    /// (see [`RegionMap::bump_layer_revision`]).
    revision_bumps: [u16; LAYER_COUNT as usize],
    /// Completed generation jobs flow back through this channel; jobs are pure
    /// and send owned tiles, so the main thread is the only cache writer.
    results_tx: Sender<JobResult>,
    results_rx: Receiver<JobResult>,
    /// One entry per job in flight, keyed to the job id it was dispatched as.
    /// Keys are `(region, layer)` for tile jobs and `(macro coord,
    /// LAYER_DRAINAGE)` for macro jobs — macro coords are just `RegionCoord`s
    /// at a higher level. Results whose id no longer matches (superseded,
    /// evicted, or from an evicted-then-reloaded region) are dropped on
    /// arrival. Matching ids establish dispatch identity; the recorded and
    /// recursively current dependency keys separately gate content provenance
    /// (ADR 0019). Cancellation is only a worker-time optimization layered on
    /// top (phase-6-plan.md §6.2).
    in_flight: BTreeMap<(RegionCoord, u16), InFlightJob>,
    /// Whether supersession/eviction flips cancellation tokens (default on).
    /// A harness A/B hook (like [`RegionMap::bump_layer_revision`]): settled
    /// state must be identical either way (ADR 0018), only worker time spent
    /// on doomed jobs differs.
    cancellation: bool,
    /// Recycles tile sample buffers through dispatch→integrate→evict
    /// (phase-6-plan.md §4.2). Main-thread only.
    pool: TilePool,
    /// Recycled organism vectors for the realizer (same story, smaller).
    organism_pool: Vec<Vec<Organism>>,
    /// Hash of the steering inputs (bias + anchors) at the last retarget. A
    /// change forces a full retarget; otherwise the pass round-robins under
    /// `max_retarget_regions` (phase-6-plan.md §6.4).
    steer_signature: u64,
    /// Round-robin position of the amortized retarget.
    retarget_cursor: Option<RegionCoord>,
    next_job_id: u64,
}

/// Bookkeeping for one dispatched generation job: its dispatch identity, the
/// authoritative dependency key captured at submission, and its cancellation
/// token (the optimization — flipped when the job is superseded or its region
/// evicted, checked once by the job closure on dequeue, phase-6-plan.md §6.2).
#[derive(Debug)]
struct InFlightJob {
    id: u64,
    expected_hash: u64,
    cancel: Arc<AtomicBool>,
}

impl RegionMap {
    /// An empty window with the given streaming configuration.
    #[must_use]
    pub fn new(cfg: StreamConfig) -> Self {
        let (results_tx, results_rx) = channel();
        Self {
            cfg,
            regions: BTreeMap::new(),
            cache: RegionCache::default(),
            macro_cache: MacroCache::default(),
            roster_cache: RosterCache::default(),
            region_signatures: BTreeMap::new(),
            organisms: BTreeMap::new(),
            organism_keys: BTreeMap::new(),
            overrides: BTreeMap::new(),
            revision_bumps: [0; LAYER_COUNT as usize],
            results_tx,
            results_rx,
            in_flight: BTreeMap::new(),
            cancellation: true,
            pool: TilePool::default(),
            organism_pool: Vec::new(),
            steer_signature: 0,
            retarget_cursor: None,
            next_job_id: 0,
        }
    }

    /// Enable or disable cancellation-token flipping (default: enabled). An
    /// A/B hook for the scale harness's schedule-independence gates
    /// (ADR 0018): cancellation may only save worker time, never change the
    /// settled world.
    pub fn set_cancellation_enabled(&mut self, enabled: bool) {
        self.cancellation = enabled;
    }

    /// Forget the in-flight jobs of `coord` whose layer is in `mask` — they
    /// were dispatched against inputs that just changed, so their results are
    /// already doomed to be dropped. When cancellation is enabled their token
    /// also lets them skip the kernel work; when disabled they deliberately run
    /// and are rejected by dispatch identity. Removing the entry always lets
    /// fresh work redispatch immediately. Returns how many tokens were flipped.
    fn cancel_in_flight(&mut self, coord: RegionCoord, mask: u32) -> usize {
        if mask == 0 {
            return 0;
        }
        let keys: Vec<(RegionCoord, u16)> = self
            .in_flight
            .range((coord, 0)..=(coord, u16::MAX))
            .filter(|((_, layer), _)| mask & layer_bit(*layer) != 0)
            .map(|(k, _)| *k)
            .collect();
        let mut cancelled = 0;
        for k in keys {
            cancelled += self.retire_in_flight(k);
        }
        cancelled
    }

    /// Retire one dispatch, flipping its token only when cancellation is
    /// enabled. The entry is removed in either mode so correctness and retry
    /// pacing never depend on whether workers honor cancellation (ADR 0018).
    fn retire_in_flight(&mut self, key: (RegionCoord, u16)) -> usize {
        let Some(job) = self.in_flight.remove(&key) else {
            return 0;
        };
        if self.cancellation {
            job.cancel.store(true, Ordering::Relaxed);
            1
        } else {
            0
        }
    }

    /// Recover the buffers of a dropped (superseded/orphaned) generation
    /// result into the pool — the tiles never entered the cache, so they are
    /// solely owned (phase-6-plan.md §4.2).
    fn reclaim_generated(&mut self, result: GeneratedTile) {
        for (_, tile) in result.channels {
            self.pool.reclaim_f32(tile.into_samples());
        }
        if let Some(tile) = result.biome {
            self.pool.reclaim_u8(tile.into_samples());
        }
        if let Some(tile) = result.dominant {
            self.pool.reclaim_u16(tile.into_samples());
        }
    }

    /// Recover every buffer of an evicted region's tiles whose `Arc` the map
    /// held the last reference to; in-flight readers just delay reclaim
    /// (§4.2 — the pool falls back to allocation, never blocks).
    fn reclaim_tiles(&mut self, tiles: RegionTiles) {
        for tile in tiles.channels.into_iter().flatten() {
            if let Ok(t) = Arc::try_unwrap(tile) {
                self.pool.reclaim_f32(t.into_samples());
            }
        }
        if let Some(tile) = tiles.biome {
            if let Ok(t) = Arc::try_unwrap(tile) {
                self.pool.reclaim_u8(t.into_samples());
            }
        }
        if let Some(tile) = tiles.dominant {
            if let Ok(t) = Arc::try_unwrap(tile) {
                self.pool.reclaim_u16(t.into_samples());
            }
        }
    }

    /// The streaming configuration.
    #[inline]
    #[must_use]
    pub const fn config(&self) -> &StreamConfig {
        &self.cfg
    }

    /// The field cache for the active window.
    #[inline]
    #[must_use]
    pub const fn cache(&self) -> &RegionCache {
        &self.cache
    }

    /// The macro drainage cache.
    #[inline]
    #[must_use]
    pub const fn macro_cache(&self) -> &MacroCache {
        &self.macro_cache
    }

    /// The roster cache (memoized `(roster, food web)` per signature).
    #[inline]
    #[must_use]
    pub const fn roster_cache(&self) -> &RosterCache {
        &self.roster_cache
    }

    /// All realized near-field organisms in the window (phase-3-plan.md §8.3).
    pub fn organisms(&self) -> impl Iterator<Item = &Organism> {
        self.organisms.values().flatten()
    }

    /// A pinned near region's realized organisms, if any.
    #[inline]
    #[must_use]
    pub fn organisms_in(&self, coord: RegionCoord) -> Option<&[Organism]> {
        self.organisms.get(&coord).map(Vec::as_slice)
    }

    /// Total near-field organisms currently resident.
    #[inline]
    #[must_use]
    pub fn organism_count(&self) -> usize {
        self.organisms.values().map(Vec::len).sum()
    }

    /// Classify a cell's habitat signature from its settled biome/climate/soil
    /// tiles — the same classification L8 runs (phase-3-plan.md §7.1). `None`
    /// until those input tiles exist.
    #[must_use]
    pub fn cell_signature(&self, coord: RegionCoord, cx: u16, cy: u16) -> Option<HabitatSignature> {
        let tiles = self.cache.get(coord)?;
        let temperature = tiles.channels[CHANNEL_TEMPERATURE].as_ref()?;
        let moisture = tiles.channels[CHANNEL_MOISTURE].as_ref()?;
        let fertility = tiles.channels[CHANNEL_FERTILITY].as_ref()?;
        let biome = tiles.biome.as_ref()?;
        let c = Climate {
            temperature: temperature.get(cx, cy),
            moisture: moisture.get(cx, cy),
        };
        let s = Soils {
            depth: 0.0,
            fertility: fertility.get(cx, cy),
        };
        Some(HabitatSignature::of(
            Biome::from_id(biome.get(cx, cy)),
            &c,
            &s,
        ))
    }

    /// The stable id of a cell's dominant species — its signature classified,
    /// then `species_seed(signature, dominant_index)`. Cheap (no roster cache
    /// lookup), for the categorical dominant-species map channel. `None` until
    /// the ecology tile and its inputs exist.
    #[must_use]
    pub fn dominant_species_id(&self, coord: RegionCoord, cx: u16, cy: u16) -> Option<u64> {
        let index = self.cache.dominant(coord)?.get(cx, cy);
        let signature = self.cell_signature(coord, cx, cy)?;
        Some(world_core::species_seed(signature, u32::from(index)))
    }

    /// The full ecology readout for a cell: signature, roster, dominant species,
    /// trophic breakdown, and the aggregate L8 field values — the data behind
    /// `wer-inspect --species` / `--ecology` and the info panel
    /// (phase-3-plan.md §11). `None` until L8 has settled for the cell.
    #[must_use]
    pub fn cell_ecology(&self, coord: RegionCoord, cx: u16, cy: u16) -> Option<CellEcology> {
        let signature = self.cell_signature(coord, cx, cy)?;
        let entry = Arc::clone(self.roster_cache.get(signature)?);
        let dominant_index = self.cache.dominant(coord)?.get(cx, cy);
        let tiles = self.cache.get(coord)?;
        let sample = |channel: usize| tiles.channels[channel].as_ref().map(|t| t.get(cx, cy));
        let dominant_id = entry
            .roster
            .species
            .get(dominant_index as usize)
            .map_or(0, |s| s.id);
        let mut trophic_counts = [0usize; 5];
        for s in &entry.roster.species {
            trophic_counts[s.trophic as usize] += 1;
        }
        Some(CellEcology {
            signature,
            roster: entry,
            dominant_index,
            dominant_id,
            trophic_counts,
            herbivore: sample(CHANNEL_HERBIVORE),
            predator: sample(CHANNEL_PREDATOR),
            diversity: sample(CHANNEL_DIVERSITY),
        })
    }

    /// Capture the feature at a world position into a run-local
    /// [`Anchor`](world_core::Anchor) (phase-4-plan.md §4.2, §7.1). The runtime
    /// gatherer for the pure `world-core` capture math: it reads the covering
    /// region's `current` possibility vector (the habitat baseline), the nearest
    /// realized organism (for an organism capture) or the terrain/hydrology/
    /// climate channels (for an environmental capture), builds a bounded
    /// [`TraitDeviation`](world_core::TraitDeviation), and calls
    /// [`capture_target`](world_core::capture_target). `None` if nothing
    /// capturable is resident there or the mask is empty.
    ///
    /// Presentation-grade throughout (reads `f32` tiles/organisms, ADR 0010/0011);
    /// a pure read of the resident caches — it never mutates the map.
    #[must_use]
    pub fn capture_at(
        &self,
        world_pos: (f64, f64),
        category_mask: u8,
        kind: AnchorKind,
        strength: f32,
        falloff_radius: f64,
    ) -> Option<Anchor> {
        if category_mask == 0 {
            return None;
        }
        let coord = RegionCoord::from_world(world_pos.0, world_pos.1);
        let baseline = self.regions.get(&coord)?.current;
        let res = self.cfg.field_resolution;
        let (ox, oy) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);
        let cx = (((world_pos.0 - ox) / cell) as u16).min(res - 1);
        let cy = (((world_pos.1 - oy) / cell) as u16).min(res - 1);
        let tiles = self.cache.get(coord);
        let sample = |channel: usize| {
            tiles
                .and_then(|t| t.channels[channel].as_ref())
                .map(|t| t.get(cx, cy))
        };

        let organism_mask = domain_mask(&[
            PossibilityDomain::Morphology,
            PossibilityDomain::Behavior,
            PossibilityDomain::Aesthetics,
            PossibilityDomain::Ecology,
        ]);

        let mut deviation = TraitDeviation::zero();
        let mut organism_species = None;

        // Organism capture: the nearest realized organism drives the M/B/A/E
        // deviation (§7.1). Its genome is reconstructed from its species id
        // (`Genome::from_seed`, the same derivation realization used).
        if category_mask & organism_mask != 0 {
            if let Some(org) = self.nearest_organism(coord, world_pos) {
                let genome = Genome::from_seed(org.species);
                deviation = organism_trait_deviation(org.expressed, genome, baseline);
                organism_species = Some(org.species);
            } else if category_mask & (1 << PossibilityDomain::Ecology.index() as u8) != 0 {
                // No organism: read Ecology distinctiveness from aggregate
                // vegetation density instead.
                if let Some(v) = sample(CHANNEL_VEGETATION) {
                    deviation.set(
                        PossibilityDomain::Ecology,
                        v - baseline.get(PossibilityDomain::Ecology),
                    );
                }
            }
        }

        // Environmental deviations: each masked landscape/water/climate domain
        // departs from the baseline by its channel value.
        if category_mask & (1 << PossibilityDomain::Geology.index() as u8) != 0 {
            if let Some(h) = sample(CHANNEL_HARDNESS) {
                deviation.set(
                    PossibilityDomain::Geology,
                    h - baseline.get(PossibilityDomain::Geology),
                );
            }
        }
        if category_mask & (1 << PossibilityDomain::Hydrology.index() as u8) != 0 {
            let river = sample(CHANNEL_RIVER).unwrap_or(0.0);
            let wetness = sample(CHANNEL_WETNESS).unwrap_or(0.0);
            deviation.set(
                PossibilityDomain::Hydrology,
                river.max(wetness) - baseline.get(PossibilityDomain::Hydrology),
            );
        }
        if category_mask & (1 << PossibilityDomain::Climate.index() as u8) != 0 {
            if let Some(t) = sample(CHANNEL_TEMPERATURE) {
                let norm = ((t + 15.0) / 50.0).clamp(0.0, 1.0);
                deviation.set(
                    PossibilityDomain::Climate,
                    norm - baseline.get(PossibilityDomain::Climate),
                );
            }
        }

        // Source metadata: organism if one drove the capture, else the dominant
        // masked environmental category (legibility only in Phase 4).
        let source = if let Some(species) = organism_species {
            AnchorSource::Organism { species }
        } else if category_mask & (1 << PossibilityDomain::Hydrology.index() as u8) != 0 {
            AnchorSource::River
        } else if category_mask
            & ((1 << PossibilityDomain::Climate.index() as u8)
                | (1 << PossibilityDomain::Planetary.index() as u8))
            != 0
        {
            AnchorSource::Atmosphere
        } else if category_mask & (1 << PossibilityDomain::Geology.index() as u8) != 0 {
            AnchorSource::Landform
        } else {
            AnchorSource::Manual
        };

        let target = capture_target(baseline, deviation, category_mask, CAPTURE_GAIN);
        Some(Anchor {
            world_pos,
            target,
            mask: category_mask,
            kind,
            strength,
            falloff_radius,
            source,
        })
    }

    /// The realized organism nearest a world position within its region, if any
    /// are resident there (the near window only).
    fn nearest_organism(&self, coord: RegionCoord, world_pos: (f64, f64)) -> Option<&Organism> {
        let dist2 = |p: (f64, f64)| {
            let dx = p.0 - world_pos.0;
            let dy = p.1 - world_pos.1;
            dx * dx + dy * dy
        };
        self.organisms.get(&coord)?.iter().min_by(|a, b| {
            dist2(a.world_pos)
                .partial_cmp(&dist2(b.world_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Build the transient resonance graph at the player and its gate strength
    /// (phase-4-plan.md §7.5). A pure read of the settled near-window caches:
    /// the realized organisms within `near_radius` become nodes (nearest-first,
    /// capped at `max_resonance_nodes`), and their count/diversity/distance,
    /// the local anchor compatibility, and a canopy occlusion proxy combine into
    /// a bounded `strength`. Order-independent and deterministic; never stored.
    #[must_use]
    pub fn resonance_at(
        &self,
        player: (f64, f64),
        anchors: &[Anchor],
        budget: &Budget,
    ) -> Resonance {
        let radius = self.cfg.near_radius;
        if radius <= 0.0 {
            return Resonance::empty();
        }
        let radius2 = radius * radius;
        // Collect near organisms with their squared distance for a stable sort.
        let mut candidates: Vec<(u64, ResonanceNode)> = Vec::new();
        for org in self.organisms() {
            let dx = org.world_pos.0 - player.0;
            let dy = org.world_pos.1 - player.1;
            let d2 = dx * dx + dy * dy;
            if d2 <= radius2 {
                candidates.push((
                    d2.to_bits(),
                    ResonanceNode {
                        world_pos: org.world_pos,
                        species: org.species,
                        distance: d2.sqrt(),
                    },
                ));
            }
        }
        // Nearest-first, with a total deterministic tiebreak so the capped set
        // is order-independent (§7.5).
        candidates.sort_unstable_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.species.cmp(&b.1.species))
                .then_with(|| a.1.world_pos.0.to_bits().cmp(&b.1.world_pos.0.to_bits()))
                .then_with(|| a.1.world_pos.1.to_bits().cmp(&b.1.world_pos.1.to_bits()))
        });
        candidates.truncate(budget.max_resonance_nodes);
        let nodes: Vec<ResonanceNode> = candidates.into_iter().map(|(_, n)| n).collect();
        if nodes.is_empty() {
            return Resonance::empty();
        }

        let density = density_term(nodes.len());
        let diversity = species_entropy(&nodes);
        // Distance term: mean smooth falloff of the nodes' reach.
        let distance = {
            let sum: f32 = nodes
                .iter()
                .map(|n| {
                    let t = (1.0 - (n.distance / radius) as f32).clamp(0.0, 1.0);
                    t * t
                })
                .sum();
            sum / nodes.len() as f32
        };
        let anchor_compatibility = self.anchor_compatibility(player, anchors);
        let occlusion = self.occlusion_proxy(player);
        let strength = combine_resonance(
            density,
            diversity,
            distance,
            anchor_compatibility,
            occlusion,
        );
        Resonance {
            strength,
            nodes,
            anchor_compatibility,
        }
    }

    /// How well the local ecology at the player matches the active anchor set,
    /// `0..=1` — steering toward a world the player is *near an example of*
    /// resonates more strongly (phase-4-plan.md §7.5). The influence-weighted
    /// mean agreement between the player cell's realized possibility vector and
    /// each anchor's masked target. Neutral (`1.0`) when no anchor reaches here.
    fn anchor_compatibility(&self, player: (f64, f64), anchors: &[Anchor]) -> f32 {
        if anchors.is_empty() {
            return 1.0;
        }
        let coord = RegionCoord::from_world(player.0, player.1);
        let Some(region) = self.regions.get(&coord) else {
            return 1.0;
        };
        let current = region.current;
        let mut weight_sum = 0.0f32;
        let mut diff_sum = 0.0f32;
        for anchor in anchors {
            let w = anchor.influence(player);
            if w <= 0.0 {
                continue;
            }
            let mut masked = 0u32;
            let mut diff = 0.0f32;
            for i in 0..POSSIBILITY_DIMS {
                if anchor.mask & (1 << i as u8) != 0 {
                    diff += (current.dims[i] - anchor.target.dims[i]).abs();
                    masked += 1;
                }
            }
            if masked > 0 {
                diff_sum += w * (diff / masked as f32);
                weight_sum += w;
            }
        }
        if weight_sum <= 0.0 {
            return 1.0;
        }
        (1.0 - diff_sum / weight_sum).clamp(0.0, 1.0)
    }

    /// The line-of-sight occlusion proxy at the player (phase-4-plan.md §7.5):
    /// dense canopy attenuates resonance. A mild factor from the player cell's
    /// vegetation density, floored so it never dominates.
    fn occlusion_proxy(&self, player: (f64, f64)) -> f32 {
        let coord = RegionCoord::from_world(player.0, player.1);
        let res = self.cfg.field_resolution;
        let (ox, oy) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);
        let cx = (((player.0 - ox) / cell) as u16).min(res - 1);
        let cy = (((player.1 - oy) / cell) as u16).min(res - 1);
        let veg = self
            .cache
            .get(coord)
            .and_then(|t| t.channels[CHANNEL_VEGETATION].as_ref())
            .map_or(0.0, |t| t.get(cx, cy));
        (1.0 - 0.25 * veg).clamp(0.7, 1.0)
    }

    /// A resident region's state.
    #[inline]
    #[must_use]
    pub fn get(&self, coord: RegionCoord) -> Option<&RegionState> {
        self.regions.get(&coord)
    }

    /// Number of resident regions.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// Whether the window is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Iterate resident regions in deterministic coordinate order.
    pub fn iter_active(&self) -> impl Iterator<Item = &RegionState> {
        self.regions.values()
    }

    /// Number of generation jobs currently dispatched but not yet integrated
    /// (the live task-queue depth; telemetry).
    #[inline]
    #[must_use]
    pub fn jobs_in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// A layer's effective algorithm revision: the declared table value plus
    /// any run-local bumps.
    #[inline]
    #[must_use]
    pub fn effective_revision(&self, layer: u16) -> u16 {
        layer_decl(layer)
            .algorithm_revision
            .wrapping_add(self.revision_bumps[layer as usize])
    }

    /// The full dependency-hash chain of one resident region: every layer's
    /// buckets, expected vs stored hash, and pipeline state — the
    /// "why did this regenerate" debugging story (phase-2-plan.md §5.3).
    /// The drainage entry reports the covering macro tile.
    #[must_use]
    pub fn layer_diagnostics(&self, coord: RegionCoord) -> Option<Vec<LayerDiagnostic>> {
        let region = self.regions.get(&coord)?;
        let mut out = Vec::with_capacity(LAYER_COUNT as usize);
        for layer in 0..LAYER_COUNT {
            let decl = layer_decl(layer);
            let (expected, stored, in_flight) = if layer == LAYER_DRAINAGE {
                let mc = macro_coord_for(coord);
                (
                    Some(self.expected_macro_hash(mc)),
                    self.macro_cache.get(mc).map(|t| t.dep_hash),
                    self.in_flight.contains_key(&(mc, LAYER_DRAINAGE)),
                )
            } else {
                (
                    self.expected_layer_hash(coord, layer),
                    self.cache.get(coord).and_then(|t| t.layer_hash(layer)),
                    self.in_flight.contains_key(&(coord, layer)),
                )
            };
            out.push(LayerDiagnostic {
                layer,
                buckets: region.current.quantized_domains(decl.domains),
                expected,
                stored,
                in_flight,
                dirty: region.dirty_layers & layer_bit(layer) != 0,
            });
        }
        Some(out)
    }

    /// Pin a region to a quantized possibility state (phase-5-plan.md §7.5) —
    /// the runtime half of a preserve. The override survives eviction: a
    /// preserved region always reloads from its buckets, never from the field,
    /// and neither steering nor convergence moves it while overridden.
    ///
    /// Applying an override to a resident region snaps `current`/`target` to
    /// the signature's bucket centers. Creating a preserve *from* the region's
    /// own state flips no bucket, so it dirties nothing and its tiles hold
    /// bit-identical; importing a foreign preserve may flip buckets, which
    /// dirties exactly the declared reader layers (ADR 0007) — regeneration to
    /// the preserved landscape, not a snap of the realized one.
    pub fn set_override(&mut self, coord: RegionCoord, sig: world_core::PossibilitySignature) {
        self.overrides.insert(coord, sig);
        let mut dirtied = 0u32;
        if let Some(region) = self.regions.get_mut(&coord) {
            let snapped = sig.dequantize();
            let mut flipped = 0u8;
            for (i, domain) in PossibilityDomain::ALL.iter().enumerate() {
                if region.current.quantized(*domain) != snapped.quantized(*domain) {
                    flipped |= 1 << i;
                }
            }
            region.current = snapped;
            region.target = snapped;
            region.stability = 1.0;
            if flipped != 0 {
                dirtied = domain_dirty_mask(flipped);
                region.dirty_layers |= dirtied;
                if region.status == GenerationStatus::Ready {
                    region.status = GenerationStatus::Generating;
                }
            }
        }
        // Superseded in-flight work for the flipped layers stops early (§6.2).
        self.cancel_in_flight(coord, dirtied);
    }

    /// Release a region's possibility override (deleting a preserve). No
    /// snap: `current` stays where the preserve held it, and the next
    /// retarget/converge resumes normal travel-fueled steering from there
    /// (phase-5-plan.md §7.5).
    pub fn clear_override(&mut self, coord: RegionCoord) {
        self.overrides.remove(&coord);
    }

    /// Whether a region is currently pinned by a possibility override.
    #[inline]
    #[must_use]
    pub fn is_overridden(&self, coord: RegionCoord) -> bool {
        self.overrides.contains_key(&coord)
    }

    /// Restore one region from a session snapshot (phase-5-plan.md §12.2):
    /// bit-exact `current`, stability, and revision, target = current (the
    /// next retarget recomputes it from the same inputs), every layer dirty so
    /// caches, rosters, and organisms re-derive deterministically from the
    /// restored possibility state (ADR 0008). Loading is not an event: nothing
    /// converges and no target moves beyond what the live run would compute.
    pub fn restore_region(&mut self, snap: &world_core::RegionSnapshotRecord) {
        let mut region = RegionState::new(snap.coord);
        region.current = PossibilityVector { dims: snap.current };
        region.target = region.current;
        region.stability = snap.stability;
        region.revision = snap.revision;
        region.dirty_layers = all_layers_mask();
        region.status = GenerationStatus::Generating;
        self.regions.insert(snap.coord, region);
    }

    /// Apply a run-local bump to one layer's algorithm revision, invalidating
    /// that layer and its transitive dependents everywhere (phase-2-plan.md
    /// §9.2). A debug/testing hook: the invalidation-precision harness uses it
    /// to machine-check the "revision bump" scenario without recompiling; a
    /// real algorithm change edits the declaration table instead.
    pub fn bump_layer_revision(&mut self, layer: u16) {
        self.revision_bumps[layer as usize] = self.revision_bumps[layer as usize].wrapping_add(1);
        let mask = dependents_closure(layer);
        for region in self.regions.values_mut() {
            region.dirty_layers |= mask;
            if region.status == GenerationStatus::Ready {
                region.status = GenerationStatus::Generating;
            }
        }
        // Every in-flight job of an invalidated layer — macro drainage
        // included (its keys carry LAYER_DRAINAGE) — is now stale (§6.2).
        // Entries retire in both cancellation modes so fresh keys can dispatch
        // immediately; cancellation-off workers still run and are rejected by
        // their old job ids at integration.
        let obsolete: Vec<(RegionCoord, u16)> = self
            .in_flight
            .keys()
            .filter(|(_, layer)| mask & layer_bit(*layer) != 0)
            .copied()
            .collect();
        for key in obsolete {
            self.retire_in_flight(key);
        }
    }

    /// One frame of streaming work (see module docs for the step order).
    ///
    /// `travel` is how far, in world units, the player moved since the
    /// previous update. It fuels convergence (ADR 0006): zero travel means no
    /// realized state moves anywhere, so a stationary player's world is
    /// perfectly still and change can never bank up out of sight. Streaming,
    /// retargeting, and regeneration of already-dirty layers are *not* gated
    /// on travel — only the act of drifting `current` toward `target` is.
    ///
    /// `bias` is a per-dimension offset the player steers directly (the
    /// keyboard "nudge" input), applied to the field sample before anchors.
    ///
    /// `transition_mode` selects the deliberate slow-steering movement mode over
    /// fast free exploration (phase-4-plan.md §8.2): it scales the convergence
    /// coefficient down so transition travel shapes the world precisely while
    /// free travel surveys it. Convergence is additionally **resonance-gated**
    /// (ADR 0012): the rate is multiplied by the transition capability at the
    /// player, so barren surroundings hold the world still no matter how far the
    /// player travels.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        player: (f64, f64),
        travel: f64,
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
        budget: &Budget,
        executor: &dyn TaskExecutor,
        transition_mode: bool,
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        // Per-pass wall-clock (phase-6-plan.md §5.2): telemetry only, all
        // zeros without the `pass-timing` feature. Timing changes no behavior.
        let mut timings = PassTimings::default();
        timings.time(Pass::Integrate, || self.integrate_finished(&mut stats));
        timings.time(Pass::Evict, || self.evict(player, &mut stats));
        timings.time(Pass::Load, || {
            self.load(player, field, anchors, bias, budget, &mut stats);
        });
        timings.time(Pass::Retarget, || {
            self.retarget(player, field, anchors, bias, budget, &mut stats);
        });
        // Resonance is a pure read of the settled near-window caches, computed
        // between retarget and converge so the rate sees the current frame's
        // gate (phase-4-plan.md §8.2). It reads the previous frame's realized
        // organisms — a transient one-frame view, never stored.
        let resonance = self.resonance_at(player, anchors, budget);
        let transition_scale = if transition_mode {
            TRANSITION_CONVERGE_SCALE
        } else {
            1.0
        };
        timings.time(Pass::Converge, || {
            self.converge(
                player,
                travel,
                resonance.strength,
                transition_scale,
                budget,
                &mut stats,
            );
        });
        stats.resonance_strength = resonance.strength;
        stats.resonance_nodes = resonance.nodes.len();
        stats.anchors_active = anchors.len();
        timings.time(Pass::Dispatch, || {
            self.dispatch_regen(player, field, budget, executor, &mut stats);
        });
        // A synchronous executor (InlineExecutor) has already finished every
        // job dispatched above; integrating again here keeps the headless
        // replay settled within a single frame.
        timings.time(Pass::Integrate, || self.integrate_finished(&mut stats));
        // Near-field realization: a pure read of the settled caches over the
        // pinned near window only (phase-3-plan.md §8.3), run after dispatch
        // settles so it never sees a stale L8 tile.
        timings.time(Pass::Realize, || {
            self.realize_near_window(player, budget, &mut stats);
        });
        stats.pass_ms = timings.ms;
        stats.active_regions = self.regions.len();
        stats.cache_bytes = self.cache.bytes();
        stats.macro_cache_bytes = self.macro_cache.bytes();
        stats.rosters_built = self.roster_cache.take_builds();
        stats.roster_cache_bytes = self.roster_cache.bytes();
        let (pool_hits, pool_misses) = self.pool.take_stats();
        stats.pool_hits = pool_hits;
        stats.pool_misses = pool_misses;
        stats.pool_bytes = self.pool.bytes();
        stats
    }

    /// The steered, projected target vector for a region.
    fn target_for(
        &self,
        coord: RegionCoord,
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) -> PossibilityVector {
        let mut base = field.sample(coord);
        for (dim, offset) in base.dims.iter_mut().zip(bias) {
            *dim = (*dim + offset).clamp(0.0, 1.0);
        }
        let (ox, oy) = coord.origin();
        let center = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
        project_plausible(steer(base, anchors, center))
    }

    /// The expected dependency hash of a macro drainage tile (ADR 0009).
    fn expected_macro_hash(&self, macro_coord: RegionCoord) -> u64 {
        drainage_dep_hash(
            macro_coord,
            self.effective_revision(LAYER_DRAINAGE),
            self.effective_revision(world_core::layer::LAYER_TERRAIN),
        )
    }

    /// Whether the macro drainage tile covering `coord` is present and fresh.
    fn macro_fresh(&self, coord: RegionCoord) -> bool {
        let mc = macro_coord_for(coord);
        self.macro_cache
            .get(mc)
            .is_some_and(|t| t.dep_hash == self.expected_macro_hash(mc))
    }

    /// The expected dependency hash of `(coord, layer)` recursively derived
    /// from current authoritative region state, effective revisions, field
    /// resolution, and the expected keys of declared inputs. Materialized tile
    /// presence is deliberately irrelevant here: dispatch readiness checks it
    /// separately, while integration can always validate provenance (ADR 0019).
    fn expected_layer_hash(&self, coord: RegionCoord, layer: u16) -> Option<u64> {
        let region = self.regions.get(&coord)?;
        let mut memo = [None; LAYER_COUNT as usize];
        Some(self.expected_layer_hash_inner(coord, layer, &region.current, &mut memo))
    }

    fn expected_layer_hash_inner(
        &self,
        coord: RegionCoord,
        layer: u16,
        current: &PossibilityVector,
        memo: &mut [Option<u64>; LAYER_COUNT as usize],
    ) -> u64 {
        debug_assert_ne!(layer, LAYER_DRAINAGE, "macro drainage has its own key");
        if let Some(hash) = memo[layer as usize] {
            return hash;
        }
        let decl = layer_decl(layer);
        let mut input_hashes = Vec::with_capacity(decl.deps.len());
        for &dep in decl.deps {
            let hash = if dep == LAYER_DRAINAGE {
                self.expected_macro_hash(macro_coord_for(coord))
            } else {
                self.expected_layer_hash_inner(coord, dep, current, memo)
            };
            input_hashes.push(hash);
        }
        let hash = layer_dep_hash(
            coord,
            layer,
            self.effective_revision(layer),
            &current.quantized_domains(decl.domains),
            &input_hashes,
            self.cfg.field_resolution,
        );
        memo[layer as usize] = Some(hash);
        hash
    }

    /// Drain the results channel, integrating only results whose job id,
    /// dispatch dependency key, result key, and recursively current expected
    /// key all agree (ADR 0019). Dirty bits and cancellation may avoid work,
    /// but neither is trusted as content provenance. Rejected current
    /// dispatches leave the cache untouched and restore the affected closure
    /// to retryable state.
    fn integrate_finished(&mut self, stats: &mut FrameStats) {
        while let Ok(result) = self.results_rx.try_recv() {
            match result {
                JobResult::Macro {
                    coord,
                    job_id,
                    tile,
                } => {
                    let key = (coord, LAYER_DRAINAGE);
                    let Some((current_id, dispatch_hash)) = self
                        .in_flight
                        .get(&key)
                        .map(|job| (job.id, job.expected_hash))
                    else {
                        stats.results_dropped += 1;
                        continue; // superseded or evicted while in flight
                    };
                    if current_id != job_id {
                        stats.results_dropped += 1;
                        continue; // a newer dispatch owns this key
                    }
                    let current_hash = self.expected_macro_hash(coord);
                    if tile.dep_hash != dispatch_hash || tile.dep_hash != current_hash {
                        self.reject_macro_result(coord, stats);
                        continue;
                    }
                    self.in_flight.remove(&key);
                    let replaced_hash = self.macro_cache.get(coord).map(|old| old.dep_hash);
                    let tile_hash = tile.dep_hash;
                    self.macro_cache.insert(Arc::new(tile));
                    stats.macro_jobs += 1;
                    stats.layers_regenerated += 1;
                    stats.regenerated_by_layer[LAYER_DRAINAGE as usize] += 1;
                    // A regenerated macro tile changes its dependents' expected
                    // hydrology hashes; notify them so the hint stays exact
                    // (phase-2-plan.md §7.8).
                    if replaced_hash != Some(tile_hash) {
                        let mut dirtied: Vec<RegionCoord> = Vec::new();
                        for (&c, region) in &mut self.regions {
                            if macro_coord_for(c) == coord {
                                region.dirty_layers |=
                                    layer_bit(world_core::layer::LAYER_HYDROLOGY);
                                if region.status == GenerationStatus::Ready {
                                    region.status = GenerationStatus::Generating;
                                }
                                dirtied.push(c);
                            }
                        }
                        // In-flight hydrology jobs of those dependents were built
                        // from the superseded macro tile: cancel them (§6.2).
                        for c in dirtied {
                            stats.jobs_cancelled += self
                                .cancel_in_flight(c, layer_bit(world_core::layer::LAYER_HYDROLOGY));
                        }
                    }
                }
                JobResult::Tile(result) => {
                    let key = (result.coord, result.layer);
                    let Some((current_id, dispatch_hash)) = self
                        .in_flight
                        .get(&key)
                        .map(|job| (job.id, job.expected_hash))
                    else {
                        stats.results_dropped += 1;
                        self.reclaim_generated(result);
                        continue; // superseded or evicted while in flight
                    };
                    if current_id != result.job_id {
                        stats.results_dropped += 1;
                        self.reclaim_generated(result);
                        continue; // a newer dispatch owns this key
                    }
                    let current_hash = self.expected_layer_hash(result.coord, result.layer);
                    if result.dep_hash != dispatch_hash || current_hash != Some(result.dep_hash) {
                        self.reject_generated_result(result, stats);
                        continue;
                    }
                    self.in_flight.remove(&key);
                    let coord = result.coord;
                    let layer = result.layer;
                    let replaced_hash = self.cache.get(coord).and_then(|t| t.layer_hash(layer));
                    let result_hash = result.dep_hash;
                    // Superseded predecessors' buffers go back to the pool
                    // the moment the map holds their last reference (§4.2).
                    for (channel, tile) in result.channels {
                        if let Some(old) = self.cache.insert_channel(coord, channel, Arc::new(tile))
                        {
                            if let Ok(t) = Arc::try_unwrap(old) {
                                self.pool.reclaim_f32(t.into_samples());
                            }
                        }
                    }
                    if let Some(biome) = result.biome {
                        if let Some(old) = self.cache.insert_biome(coord, Arc::new(biome)) {
                            if let Ok(t) = Arc::try_unwrap(old) {
                                self.pool.reclaim_u8(t.into_samples());
                            }
                        }
                    }
                    if let Some(dominant) = result.dominant {
                        if let Some(old) = self.cache.insert_dominant(coord, Arc::new(dominant)) {
                            if let Ok(t) = Arc::try_unwrap(old) {
                                self.pool.reclaim_u16(t.into_samples());
                            }
                        }
                    }
                    let dependents = dependents_closure(layer) & !layer_bit(layer);
                    if let Some(region) = self.regions.get_mut(&coord) {
                        // A valid result clears even a false-positive dirty hint.
                        // Only a changed key invalidates downstream work.
                        region.dirty_layers &= !layer_bit(layer);
                        if replaced_hash != Some(result_hash) {
                            region.dirty_layers |= dependents;
                        }
                    }
                    stats.layers_regenerated += 1;
                    stats.regenerated_by_layer[layer as usize] += 1;
                    if replaced_hash != Some(result_hash) {
                        stats.jobs_cancelled += self.cancel_in_flight(coord, dependents);
                    }
                    self.refresh_status(coord);
                }
            }
        }
    }

    /// Reject one current macro dispatch whose content provenance failed,
    /// leaving the old macro tile untouched and making every covered resident
    /// Drainage-dependent closure retryable.
    fn reject_macro_result(&mut self, macro_coord: RegionCoord, stats: &mut FrameStats) {
        self.in_flight.remove(&(macro_coord, LAYER_DRAINAGE));
        stats.results_dropped += 1;
        self.mark_macro_dependents_dirty(macro_coord, stats);
    }

    /// Apply the Drainage dependent closure to every resident level-0 region
    /// covered by one macro coordinate.
    fn mark_macro_dependents_dirty(&mut self, macro_coord: RegionCoord, stats: &mut FrameStats) {
        let covered: Vec<RegionCoord> = self
            .regions
            .keys()
            .filter(|&&coord| macro_coord_for(coord) == macro_coord)
            .copied()
            .collect();
        for coord in covered {
            self.mark_dirty_closure(coord, LAYER_DRAINAGE, stats);
        }
    }

    /// Reject one current region dispatch whose content provenance failed,
    /// reclaiming all owned buffers and restoring deterministic retry state.
    fn reject_generated_result(&mut self, result: GeneratedTile, stats: &mut FrameStats) {
        let coord = result.coord;
        let layer = result.layer;
        self.in_flight.remove(&(coord, layer));
        stats.results_dropped += 1;
        self.reclaim_generated(result);
        self.mark_dirty_closure(coord, layer, stats);
    }

    /// Mark a layer and all transitive dependents dirty for one resident
    /// region, retire any now-obsolete dependent dispatches, and leave its
    /// status visibly generating. False-positive hints are harmless because
    /// dispatch clears a matching stored-versus-expected key without work.
    fn mark_dirty_closure(&mut self, coord: RegionCoord, layer: u16, stats: &mut FrameStats) {
        if !self.regions.contains_key(&coord) {
            return;
        }
        let mask = dependents_closure(layer);
        if let Some(region) = self.regions.get_mut(&coord) {
            region.dirty_layers |= mask;
            region.status = GenerationStatus::Generating;
        }
        stats.jobs_cancelled += self.cancel_in_flight(coord, mask);
    }

    /// Recompute a region's `GenerationStatus` from its dirty bits and
    /// in-flight jobs.
    fn refresh_status(&mut self, coord: RegionCoord) {
        let in_flight = self
            .in_flight
            .range((coord, 0)..=(coord, u16::MAX))
            .next()
            .is_some();
        if let Some(region) = self.regions.get_mut(&coord) {
            region.status = if region.dirty_layers == 0 && !in_flight {
                GenerationStatus::Ready
            } else {
                GenerationStatus::Generating
            };
        }
    }

    /// Evict regions beyond `unload_radius`, dropping state, cache tiles, and
    /// any in-flight bookkeeping together, then enforce the byte-capacity
    /// ceilings (phase-6-plan.md §4.3), then sweep macro tiles (and macro
    /// jobs) and roster entries that no resident region depends on any more
    /// (phase-2-plan.md §6.3).
    fn evict(&mut self, player: (f64, f64), stats: &mut FrameStats) {
        let unload = self.cfg.unload_radius;
        let gone: Vec<RegionCoord> = self
            .regions
            .keys()
            .copied()
            .filter(|c| center_distance(*c, player) > unload)
            .collect();
        let radius_evicted = !gone.is_empty();
        for coord in gone {
            self.drop_region(coord, stats);
            stats.evicted += 1;
        }
        let capacity_evicted = self.enforce_capacity(player, stats);
        if radius_evicted || capacity_evicted {
            self.sweep_dependent_caches(stats);
        }
    }

    /// Drop one resident region completely: state, tiles (buffers back to
    /// the pool), organisms (vector recycled), signature bookkeeping, and
    /// in-flight jobs (cancelled — they stop costing worker time, §6.2).
    fn drop_region(&mut self, coord: RegionCoord, stats: &mut FrameStats) {
        self.regions.remove(&coord);
        if let Some(tiles) = self.cache.remove_region(coord) {
            self.reclaim_tiles(tiles);
        }
        self.region_signatures.remove(&coord);
        if let Some(mut organisms) = self.organisms.remove(&coord) {
            organisms.clear();
            if self.organism_pool.len() < 256 {
                self.organism_pool.push(organisms);
            }
        }
        self.organism_keys.remove(&coord);
        let keys: Vec<(RegionCoord, u16)> = self
            .in_flight
            .range((coord, 0)..=(coord, u16::MAX))
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            if let Some(job) = self.in_flight.remove(&k) {
                if self.cancellation {
                    job.cancel.store(true, Ordering::Relaxed);
                    stats.jobs_cancelled += 1;
                }
            }
        }
    }

    /// Sweep the dependent-tracked caches after any region left the window:
    /// orphaned macro tiles and jobs, and roster entries no resident L8 tile
    /// references any more (phase-2-plan.md §6.3).
    fn sweep_dependent_caches(&mut self, stats: &mut FrameStats) {
        self.macro_cache.evict_orphans(self.regions.keys());
        let needed: BTreeSet<RegionCoord> =
            self.regions.keys().map(|&c| macro_coord_for(c)).collect();
        let cancellation = self.cancellation;
        self.in_flight.retain(|(c, _), job| {
            let keep = c.level == 0 || needed.contains(c);
            if !keep && cancellation {
                job.cancel.store(true, Ordering::Relaxed);
                stats.jobs_cancelled += 1;
            }
            keep
        });
        // Sweep roster entries no resident region's L8 tile references any more
        // (dependent-tracked, the macro cache's shape, §6.3).
        let needed_signatures = self.required_roster_signatures();
        self.roster_cache.evict_unused(&needed_signatures);
    }

    /// The indispensable roster working set: the deterministic union of every
    /// resident region's current or in-flight Ecology input signatures.
    fn required_roster_signatures(&self) -> BTreeSet<HabitatSignature> {
        self.region_signatures.values().flatten().copied().collect()
    }

    /// Repair every indispensable roster entry before capacity eviction or a
    /// synchronous reader can observe the cache. Returns the same protected
    /// set used by orphan sweeping so the two policies cannot diverge.
    fn maintain_roster_working_set(&mut self) -> BTreeSet<HabitatSignature> {
        let required = self.required_roster_signatures();
        for &signature in &required {
            self.roster_cache.ensure(signature);
        }
        required
    }

    /// Enforce the byte-capacity ceilings (phase-6-plan.md §4.3): after the
    /// radius sweep, remove farthest-first — deterministic (distance bits,
    /// then coord order) — until under ceiling, exempting preserved regions
    /// and everything inside `near_radius`. Always safe: every evicted tile
    /// re-derives bit-identically from its dependency hash (ADR 0008), so a
    /// ceiling costs recompute on revisit, never correctness. Returns
    /// whether anything was evicted.
    fn enforce_capacity(&mut self, player: (f64, f64), stats: &mut FrameStats) -> bool {
        let mut evicted_any = false;
        let mut field_bytes = self.cache.bytes();
        if field_bytes > self.cfg.max_field_cache_bytes {
            let mut order: Vec<(u64, RegionCoord)> = self
                .regions
                .keys()
                .filter(|c| !self.overrides.contains_key(c))
                .map(|&c| (center_distance(c, player).to_bits(), c))
                .filter(|&(d, _)| f64::from_bits(d) > self.cfg.near_radius)
                .collect();
            // Farthest first, coord tiebreak for determinism.
            order.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            for (_, coord) in order {
                if field_bytes <= self.cfg.max_field_cache_bytes {
                    break;
                }
                let freed = self.cache.get(coord).map_or(0, RegionTiles::bytes);
                self.drop_region(coord, stats);
                field_bytes -= freed;
                stats.evicted_for_capacity += 1;
                evicted_any = true;
            }
        }

        // Macro target: farthest macro tiles go first; dependents keep their
        // fresh hydrology tiles, and a macro re-derives lazily when Hydrology
        // next needs it. A freshly integrated macro is temporarily
        // indispensable while any covered Hydrology bit remains dirty — an
        // asynchronous result must survive this pre-dispatch capacity pass
        // long enough to be snapshotted. Once Hydrology dispatches, its Arc
        // snapshot is independent and the cache entry becomes disposable.
        if self.macro_cache.bytes() > self.cfg.max_macro_cache_bytes {
            let pmc = macro_coord_for(RegionCoord::from_world(player.0, player.1));
            let protected: BTreeSet<RegionCoord> = self
                .regions
                .iter()
                .filter(|(_, region)| {
                    region.dirty_layers & layer_bit(world_core::layer::LAYER_HYDROLOGY) != 0
                })
                .map(|(&coord, _)| macro_coord_for(coord))
                .collect();
            let mut tiles: Vec<(u64, RegionCoord)> = self
                .macro_cache
                .iter()
                .filter(|(coord, _)| !protected.contains(coord))
                .map(|(&c, _)| {
                    let dx = i64::from(c.x) - i64::from(pmc.x);
                    let dy = i64::from(c.y) - i64::from(pmc.y);
                    ((dx * dx + dy * dy) as u64, c)
                })
                .collect();
            tiles.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let mut macro_bytes = self.macro_cache.bytes();
            for (_, coord) in tiles {
                if macro_bytes <= self.cfg.max_macro_cache_bytes {
                    break;
                }
                if let Some(tile) = self.macro_cache.remove(coord) {
                    macro_bytes -= tile.bytes();
                    stats.evicted_for_capacity += 1;
                }
            }
        }

        // Roster target: repair the resident working set, then evict only
        // disposable entries in deterministic reverse-signature order. The
        // indispensable floor may exceed the configured target (ADR 0019).
        let protected = self.maintain_roster_working_set();
        self.roster_cache
            .evict_to_bytes(self.cfg.max_roster_cache_bytes, &protected);
        evicted_any
    }

    /// Insert missing regions within `load_radius`, nearest-first, up to the
    /// budget. A fresh region snaps `current = target`: it was never realized
    /// before, so first realization is not a pop (and at startup this fills
    /// the pinned zone already settled).
    fn load(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        let radius_regions = (self.cfg.load_radius / REGION_SIZE).ceil() as i32;
        let center = RegionCoord::from_world(player.0, player.1);
        let mut candidates: Vec<(u64, RegionCoord)> = Vec::new();
        for dy in -radius_regions..=radius_regions {
            for dx in -radius_regions..=radius_regions {
                let coord = RegionCoord::new(center.x + dx, center.y + dy);
                if self.regions.contains_key(&coord) {
                    continue;
                }
                let d = center_distance(coord, player);
                if d <= self.cfg.load_radius {
                    // Distance bits sort identically to distance (both are
                    // non-negative), and the coord tiebreak keeps the order
                    // total and deterministic.
                    candidates.push((d.to_bits(), coord));
                }
            }
        }
        candidates.sort_unstable_by(|a, b| a.cmp(b).then_with(|| a.1.cmp(&b.1)));
        // Capacity awareness (phase-6-plan.md §4.3): a non-near load that
        // would push the field cache past its ceiling defers instead — the
        // deterministic complement of the capacity evictor, so the two never
        // thrash against each other. Near regions always load (the visible
        // zone is exempt on both sides).
        let per_region_bytes = {
            let res = usize::from(self.cfg.field_resolution);
            res * res * (crate::generate::CHANNEL_COUNT * 4 + 1 + 2)
        };
        let mut projected = self.cache.bytes();
        for &(dist_bits, coord) in candidates.iter().take(budget.max_loads) {
            let near = f64::from_bits(dist_bits) <= self.cfg.near_radius;
            if !near && projected + per_region_bytes > self.cfg.max_field_cache_bytes {
                continue; // deferred by the ceiling; counted below
            }
            projected += per_region_bytes;
            let mut region = RegionState::new(coord);
            if let Some(sig) = self.overrides.get(&coord) {
                // Preserved region: reload from its persisted buckets, pinned
                // (phase-5-plan.md §7.5) — never from the field.
                region.current = sig.dequantize();
                region.target = region.current;
                region.stability = 1.0;
            } else {
                region.target = self.target_for(coord, field, anchors, bias);
                region.current = region.target;
                region.stability = stability_for(&self.cfg, center_distance(coord, player));
            }
            region.dirty_layers = all_layers_mask();
            self.regions.insert(coord, region);
            stats.loaded += 1;
        }
        stats.deferred_loads = candidates.len().saturating_sub(stats.loaded);
    }

    /// Recompute stability and target for resident regions — amortized
    /// (phase-6-plan.md §6.4): a steering change (bias or anchor set) always
    /// refreshes the whole window this frame, because every target may have
    /// moved; under unchanged steering the pass round-robins
    /// `max_retarget_regions` per frame in coord order, so the window
    /// refreshes over a few frames. Settled fixed points are amortization-
    /// invariant (ADR 0018) — targets are pure functions of unchanged
    /// inputs, so a deferred refresh recomputes the same values later.
    fn retarget(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        let signature = steering_signature(anchors, bias);
        let steering_changed = signature != self.steer_signature;
        self.steer_signature = signature;

        let coords: Vec<RegionCoord> = self.regions.keys().copied().collect();
        if steering_changed || coords.len() <= budget.max_retarget_regions {
            for coord in coords {
                self.retarget_one(coord, player, field, anchors, bias);
            }
            self.retarget_cursor = None;
            return;
        }

        // Round-robin from the cursor in coord order (deterministic).
        let split = self
            .retarget_cursor
            .map_or(0, |c| coords.partition_point(|&x| x <= c));
        let mut processed = 0usize;
        for &coord in coords[split..].iter().chain(coords[..split].iter()) {
            if processed >= budget.max_retarget_regions {
                break;
            }
            self.retarget_one(coord, player, field, anchors, bias);
            self.retarget_cursor = Some(coord);
            processed += 1;
        }
        stats.retarget_deferred = coords.len() - processed;
    }

    /// Refresh one region's stability and steered target.
    fn retarget_one(
        &mut self,
        coord: RegionCoord,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) {
        if self.overrides.contains_key(&coord) {
            // Preserved: pinned to its buckets; neither the distance ramp
            // nor steering moves it (phase-5-plan.md §7.5).
            let region = self.regions.get_mut(&coord).expect("resident");
            region.stability = 1.0;
            region.target = region.current;
            return;
        }
        let stability = stability_for(&self.cfg, center_distance(coord, player));
        let target = self.target_for(coord, field, anchors, bias);
        let region = self.regions.get_mut(&coord).expect("resident");
        region.stability = stability;
        region.target = target;
    }

    /// Step unpinned regions toward their targets, farthest-first (near
    /// regions are pinned anyway, and the far field is where transformation
    /// should visibly happen), up to the budget.
    ///
    /// The rate is fueled by `travel` (ADR 0006). A step that flips quantized
    /// buckets dirties exactly the layers that declare those domains, plus
    /// their transitive dependents (ADR 0007); sub-bucket drift dirties
    /// nothing and costs zero regeneration (phase-2-plan.md §4.2).
    fn converge(
        &mut self,
        player: (f64, f64),
        travel: f64,
        resonance: f32,
        transition_scale: f32,
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        // Resonance-gated, travel-fueled rate (ADR 0012): zero when either
        // travel or resonance is zero, so a stationary player or a barren
        // neighbourhood holds the world perfectly still.
        let rate = gated_rate(
            self.cfg.converge_per_unit,
            travel,
            resonance,
            transition_scale,
            self.cfg.converge_rate_cap,
        );
        if rate <= 0.0 {
            return;
        }
        let mut eligible: Vec<(u64, RegionCoord)> = self
            .regions
            .iter()
            .filter(|(_, r)| r.stability < 1.0 && r.current != r.target)
            .map(|(c, _)| (center_distance(*c, player).to_bits(), *c))
            .collect();
        // Farthest first: descending distance, coord tiebreak for determinism.
        eligible.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for &(_, coord) in eligible.iter().take(budget.max_converge_regions) {
            let region = self.regions.get_mut(&coord).expect("resident");
            let mut dirtied = 0u32;
            if let Some(flipped) = region.converge(rate) {
                stats.converged += 1;
                if flipped != 0 {
                    dirtied = domain_dirty_mask(flipped);
                    region.dirty_layers |= dirtied;
                    if region.status == GenerationStatus::Ready {
                        region.status = GenerationStatus::Generating;
                    }
                }
            }
            // Bucket flips supersede any in-flight job of the dirtied layers:
            // its expected hash moved on while it flew (§6.2).
            stats.jobs_cancelled += self.cancel_in_flight(coord, dirtied);
        }
        stats.deferred_converges = eligible.len().saturating_sub(budget.max_converge_regions);
    }

    /// Topological dispatch (phase-2-plan.md §8.1): a fixed-point loop that
    /// integrates finished results, then submits every stale layer whose
    /// inputs are fresh — nearest region first, layer id (topological) order
    /// within a region — until the cost budget is spent or nothing new became
    /// ready.
    fn dispatch_regen(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        budget: &Budget,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) {
        // Nearest-first region order, fixed for the frame.
        let mut order: Vec<(u64, RegionCoord)> = self
            .regions
            .keys()
            .map(|&c| (center_distance(c, player).to_bits(), c))
            .collect();
        order.sort_unstable_by(|a, b| a.cmp(b).then_with(|| a.1.cmp(&b.1)));

        loop {
            self.integrate_finished(stats);
            let mut dispatched_this_pass = false;
            // Deferral is the *end-of-frame* backlog: each pass recounts, and
            // the final (dispatch-free) pass's count is what stands.
            stats.deferred_regens = 0;
            for &(dist_bits, coord) in &order {
                if self.regions[&coord].dirty_layers == 0 {
                    continue; // clean region: skip hash checks entirely (§7.8)
                }
                let distance = f64::from_bits(dist_bits);
                let priority = if distance <= self.cfg.near_radius {
                    TaskPriority::Critical
                } else if distance <= self.cfg.far_radius {
                    TaskPriority::Normal
                } else {
                    TaskPriority::Background
                };
                dispatched_this_pass |=
                    self.dispatch_region(coord, priority, field, budget, executor, stats);
            }
            if !dispatched_this_pass {
                break;
            }
        }
    }

    /// Scan one region's dirty layers in topological (id) order. Before the
    /// scan, close its work set over missing/stale materialized dependencies;
    /// then clear false-positive hints and dispatch stale layers whose actual
    /// inputs match their authoritative keys. Returns whether anything was
    /// submitted.
    fn dispatch_region(
        &mut self,
        coord: RegionCoord,
        priority: TaskPriority,
        field: &PossibilityField,
        budget: &Budget,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) -> bool {
        self.repair_missing_dependencies(coord, stats);
        let mut dispatched = false;
        for layer in 0..LAYER_COUNT {
            let dirty = self.regions[&coord].dirty_layers;
            if dirty & layer_bit(layer) == 0 {
                continue;
            }
            if layer == LAYER_DRAINAGE {
                self.check_macro(coord, priority, field, budget, executor, stats);
                continue;
            }
            let expected = self
                .expected_layer_hash(coord, layer)
                .expect("dispatch region is resident");
            if let Some(job_hash) = self
                .in_flight
                .get(&(coord, layer))
                .map(|job| job.expected_hash)
            {
                if job_hash == expected {
                    continue; // current result pending; do not self-supersede
                }
                // A bookkeeping omission left obsolete work attached to this
                // dirty layer. Retire it and the dependent closure now.
                self.mark_dirty_closure(coord, layer, stats);
            }
            if !self.inputs_fresh(coord, layer) {
                continue; // a repaired lower layer must land first
            }
            let tiles = self.cache.get(coord);
            if tiles.and_then(|t| t.layer_hash(layer)) == Some(expected) {
                // False-positive hint: the inputs settled back to exactly the
                // state this tile was generated from.
                self.clear_dirty(coord, layer);
                continue;
            }
            let cost = layer_decl(layer).cost;
            if stats.regen_cost_spent.saturating_add(cost) > budget.max_regen_cost {
                stats.deferred_regens += 1;
                continue;
            }
            self.submit_layer(coord, layer, expected, priority, executor);
            stats.regen_cost_spent += cost;
            stats.layers_dispatched += 1;
            dispatched = true;
        }
        dispatched
    }

    /// Restore omitted dirty hints for every missing or stale materialized
    /// dependency of a dirty consumer. Matching-key in-flight work is a valid
    /// pending input; obsolete work retires before the producer is re-dirtied.
    /// Repeating reaches the finite declared-DAG closure in one dispatch pass.
    fn repair_missing_dependencies(&mut self, coord: RegionCoord, stats: &mut FrameStats) {
        loop {
            let mut changed = false;
            for consumer in 0..LAYER_COUNT {
                if consumer == LAYER_DRAINAGE
                    || self.regions[&coord].dirty_layers & layer_bit(consumer) == 0
                {
                    continue;
                }
                for &dependency in layer_decl(consumer).deps {
                    let (job_key, expected, stored) = if dependency == LAYER_DRAINAGE {
                        let macro_coord = macro_coord_for(coord);
                        (
                            (macro_coord, LAYER_DRAINAGE),
                            self.expected_macro_hash(macro_coord),
                            self.macro_cache.get(macro_coord).map(|tile| tile.dep_hash),
                        )
                    } else {
                        (
                            (coord, dependency),
                            self.expected_layer_hash(coord, dependency)
                                .expect("repair region is resident"),
                            self.cache
                                .get(coord)
                                .and_then(|tiles| tiles.layer_hash(dependency)),
                        )
                    };
                    let pending = self.in_flight.get(&job_key).map(|job| job.expected_hash);

                    if stored == Some(expected) {
                        // A current cached input is ready unless a replacement
                        // is pending. Obsolete replacement work cannot improve
                        // it and must not strand the consumer.
                        if pending.is_some_and(|hash| hash != expected) {
                            stats.jobs_cancelled += self.retire_in_flight(job_key);
                            changed = true;
                        }
                        continue;
                    }
                    if pending == Some(expected) {
                        continue; // current producer work is already on its way
                    }
                    if pending.is_some() {
                        stats.jobs_cancelled += self.retire_in_flight(job_key);
                        if dependency == LAYER_DRAINAGE {
                            self.mark_macro_dependents_dirty(job_key.0, stats);
                        }
                        changed = true;
                    }
                    let bit = layer_bit(dependency);
                    let region = self.regions.get_mut(&coord).expect("resident");
                    if region.dirty_layers & bit == 0 {
                        region.dirty_layers |= bit;
                        region.status = GenerationStatus::Generating;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Handle a region's drainage dirty bit: clear it if the covering macro
    /// tile is fresh, otherwise make sure a macro job is on its way (riding
    /// the queue at the priority of its nearest dependent, phase-2-plan.md
    /// §8.1).
    #[allow(clippy::too_many_arguments)]
    fn check_macro(
        &mut self,
        coord: RegionCoord,
        priority: TaskPriority,
        field: &PossibilityField,
        budget: &Budget,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) {
        let mc = macro_coord_for(coord);
        let expected = self.expected_macro_hash(mc);
        if let Some(job_hash) = self
            .in_flight
            .get(&(mc, LAYER_DRAINAGE))
            .map(|job| job.expected_hash)
        {
            if job_hash == expected {
                return;
            }
            stats.jobs_cancelled += self.retire_in_flight((mc, LAYER_DRAINAGE));
            self.mark_macro_dependents_dirty(mc, stats);
        }
        if self.macro_fresh(coord) {
            self.clear_dirty(coord, LAYER_DRAINAGE);
            return;
        }
        let cost = layer_decl(LAYER_DRAINAGE).cost;
        if stats.regen_cost_spent.saturating_add(cost) > budget.max_regen_cost {
            stats.deferred_regens += 1;
            return;
        }
        let job_id = self.next_job_id;
        self.next_job_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.in_flight.insert(
            (mc, LAYER_DRAINAGE),
            InFlightJob {
                id: job_id,
                expected_hash: expected,
                cancel: Arc::clone(&cancel),
            },
        );
        let tx = self.results_tx.clone();
        let field = *field;
        executor.submit(
            priority,
            Box::new(move || {
                // Checked once, on dequeue: a superseded/evicted job becomes
                // a no-op before its kernel runs (phase-6-plan.md §6.2). The
                // integration still checks job id and both dependency keys.
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let tile = world_core::drainage(mc, &field, expected);
                // The receiver may be gone if the map was dropped; the job's
                // work is simply discarded then.
                let _ = tx.send(JobResult::Macro {
                    coord: mc,
                    job_id,
                    tile,
                });
            }),
        );
        stats.regen_cost_spent += cost;
        stats.layers_dispatched += 1;
    }

    /// Whether every declared input of `(coord, layer)` is materialized with
    /// its recursively authoritative key and has no pending replacement.
    /// Dirty bits are scheduling hints and therefore not evidence either way.
    fn inputs_fresh(&self, coord: RegionCoord, layer: u16) -> bool {
        let tiles = self.cache.get(coord);
        for &dep in layer_decl(layer).deps {
            if dep == LAYER_DRAINAGE {
                let mc = macro_coord_for(coord);
                if self.in_flight.contains_key(&(mc, LAYER_DRAINAGE))
                    || self.macro_cache.get(mc).map(|tile| tile.dep_hash)
                        != Some(self.expected_macro_hash(mc))
                {
                    return false;
                }
                continue;
            }
            if self.in_flight.contains_key(&(coord, dep))
                || tiles.and_then(|t| t.layer_hash(dep)) != self.expected_layer_hash(coord, dep)
            {
                return false;
            }
        }
        true
    }

    /// Clear one dirty bit and refresh the region's status.
    fn clear_dirty(&mut self, coord: RegionCoord, layer: u16) {
        if let Some(region) = self.regions.get_mut(&coord) {
            region.dirty_layers &= !layer_bit(layer);
        }
        self.refresh_status(coord);
    }

    /// Classify every cell of a region's settled inputs into a habitat
    /// signature, ensure each signature's `(roster, food web)` is cached, and
    /// snapshot them for the L8 job (phase-3-plan.md §6.3, §8.2). Records the
    /// region's signature set for dependent-tracked roster eviction. The
    /// classification here must match `generate_layer`'s exactly, so the
    /// snapshot covers every signature the job will look up.
    fn build_ecology_rosters(&mut self, coord: RegionCoord) -> Option<Arc<RosterSnapshot>> {
        let tiles = self.cache.get(coord)?;
        // Clone the input Arcs so the immutable cache borrow ends before we
        // mutate the roster cache below.
        let temperature = tiles.channels[CHANNEL_TEMPERATURE].clone()?;
        let moisture = tiles.channels[CHANNEL_MOISTURE].clone()?;
        let fertility = tiles.channels[CHANNEL_FERTILITY].clone()?;
        let biome = tiles.biome.clone()?;

        let res = self.cfg.field_resolution;
        let mut signatures = BTreeSet::new();
        for cy in 0..res {
            for cx in 0..res {
                let c = Climate {
                    temperature: temperature.get(cx, cy),
                    moisture: moisture.get(cx, cy),
                };
                // Only fertility feeds the signature (matches generate_layer).
                let s = Soils {
                    depth: 0.0,
                    fertility: fertility.get(cx, cy),
                };
                let b = Biome::from_id(biome.get(cx, cy));
                signatures.insert(HabitatSignature::of(b, &c, &s));
            }
        }

        let mut snapshot: RosterSnapshot = BTreeMap::new();
        for &signature in &signatures {
            snapshot.insert(signature, self.roster_cache.ensure(signature));
        }
        self.region_signatures.insert(coord, signatures);
        Some(Arc::new(snapshot))
    }

    /// The near-field realization pass (phase-3-plan.md §8.3): (re)build the
    /// organism list of each pinned near region whose L8 tile and complete
    /// resident roster set are fresh, drop lists of regions that left the near
    /// window, and cap per-frame organism instantiation by whole regions. A
    /// pure read of the settled caches — it never mutates tiles and never
    /// enters the results channel.
    ///
    /// Re-realization is keyed on a region's L8 dependency hash: an organism
    /// list is rebuilt exactly when the aggregate it was sampled from changes
    /// (distance-based regeneration, §7.6), and is otherwise reused untouched,
    /// so a pinned region's organism ids hold still across frames.
    fn realize_near_window(&mut self, player: (f64, f64), budget: &Budget, stats: &mut FrameStats) {
        let near_radius = self.cfg.near_radius;
        let near: BTreeSet<RegionCoord> = self
            .regions
            .keys()
            .copied()
            .filter(|&c| center_distance(c, player) <= near_radius)
            .collect();
        // Offscreen replacement: discard organisms of regions no longer near
        // (their vectors recycle through the pool, phase-6-plan.md §4.2).
        let stale: Vec<RegionCoord> = self
            .organisms
            .keys()
            .filter(|c| !near.contains(c))
            .copied()
            .collect();
        for coord in stale {
            if let Some(mut organisms) = self.organisms.remove(&coord) {
                organisms.clear();
                if self.organism_pool.len() < 256 {
                    self.organism_pool.push(organisms);
                }
            }
        }
        self.organism_keys.retain(|c, _| near.contains(c));

        // Nearest-first, deterministic order.
        let mut order: Vec<(u64, RegionCoord)> = near
            .iter()
            .map(|&c| (center_distance(c, player).to_bits(), c))
            .collect();
        order.sort_unstable_by(|a, b| a.cmp(b).then_with(|| a.1.cmp(&b.1)));

        let resolution = self.cfg.field_resolution;
        for (_, coord) in order {
            // L8 must be fresh: present, dirty bit clear, no job in flight.
            let Some(l8_hash) = self
                .cache
                .get(coord)
                .and_then(|t| t.layer_hash(LAYER_ECOLOGY))
            else {
                continue;
            };
            let region = &self.regions[&coord];
            if region.dirty_layers & layer_bit(LAYER_ECOLOGY) != 0
                || self.in_flight.contains_key(&(coord, LAYER_ECOLOGY))
                || self.expected_layer_hash(coord, LAYER_ECOLOGY) != Some(l8_hash)
            {
                continue;
            }
            if self.organism_keys.get(&coord) == Some(&l8_hash) {
                continue; // organisms already reflect the current aggregate
            }
            if stats.organisms_realized >= budget.max_realize_organisms {
                break; // defer the rest to a later frame (§8.4)
            }
            let Some(signatures) = self.region_signatures.get(&coord) else {
                continue;
            };
            if signatures
                .iter()
                .any(|&signature| self.roster_cache.get(signature).is_none())
            {
                // Fail closed: preserve the old organism vector and key. The
                // next capacity pass repairs the required roster set, after
                // which ordinary realization retries (ADR 0019).
                continue;
            }
            let bias = GenomeBias {
                morphology: region.current.get(PossibilityDomain::Morphology),
                behavior: region.current.get(PossibilityDomain::Behavior),
                aesthetics: region.current.get(PossibilityDomain::Aesthetics),
            };
            let revision = region.revision;
            let mut organisms = self.organism_pool.pop().unwrap_or_default();
            {
                let tiles = self.cache.get(coord).expect("l8 hash implies tiles");
                realize_region_into(
                    coord,
                    tiles,
                    &self.roster_cache,
                    bias,
                    revision,
                    resolution,
                    self.cfg.organisms_per_cell,
                    &mut organisms,
                );
            }
            stats.organisms_realized += organisms.len();
            if let Some(mut old) = self.organisms.insert(coord, organisms) {
                old.clear();
                if self.organism_pool.len() < 256 {
                    self.organism_pool.push(old);
                }
            }
            self.organism_keys.insert(coord, l8_hash);
        }
        stats.organisms = self.organisms.values().map(Vec::len).sum();
    }

    /// Snapshot a layer's inputs and submit its generation job. Clears the
    /// dirty bit at dispatch as a scheduling hint; integration independently
    /// validates the recorded dispatch key and recursively current key, so a
    /// later false-positive dirty bit neither accepts nor rejects content.
    fn submit_layer(
        &mut self,
        coord: RegionCoord,
        layer: u16,
        expected: u64,
        priority: TaskPriority,
        executor: &dyn TaskExecutor,
    ) {
        // L8 resolves the rosters for the signatures its cells will produce
        // (from the settled biome/climate/soil tiles) before the mutable
        // borrows below, ensuring each is cached and snapshotting them into the
        // job's inputs — exactly as hydrology snapshots the drainage macro tile
        // (phase-3-plan.md §6.3, §8.2).
        let rosters = if layer == LAYER_ECOLOGY {
            self.build_ecology_rosters(coord)
        } else {
            None
        };

        let decl = layer_decl(layer);
        let tiles = self.cache.get(coord);
        let mut input_tiles = Vec::new();
        let mut biome = None;
        let mut drainage = None;
        for &dep in decl.deps {
            match dep {
                LAYER_DRAINAGE => {
                    drainage = self.macro_cache.get(macro_coord_for(coord)).cloned();
                }
                world_core::layer::LAYER_BIOME => {
                    biome = tiles.and_then(|t| t.biome.clone());
                }
                _ => {
                    for &channel in layer_channels(dep) {
                        let tile = tiles
                            .and_then(|t| t.channels[channel].clone())
                            .expect("inputs_fresh checked");
                        input_tiles.push((channel, tile));
                    }
                }
            }
        }
        // Recycled output buffers ride into the job (phase-6-plan.md §4.2):
        // the main thread is the only pool toucher, workers fill what they
        // were given.
        let mut buffers = TileBuffers::default();
        for _ in layer_channels(layer) {
            buffers.f32_bufs.push(self.pool.take_f32());
        }
        if layer == world_core::layer::LAYER_BIOME {
            buffers.u8_buf = Some(self.pool.take_u8());
        }
        if layer == LAYER_ECOLOGY {
            buffers.u16_buf = Some(self.pool.take_u16());
        }
        let region = self.regions.get_mut(&coord).expect("resident");
        let inputs = LayerInputs {
            quantized: region.current.quantized_domains(decl.domains),
            tiles: input_tiles,
            biome,
            drainage,
            rosters,
            dep_hash: expected,
            buffers,
        };
        region.dirty_layers &= !layer_bit(layer);
        region.status = GenerationStatus::Generating;

        let job_id = self.next_job_id;
        self.next_job_id += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.in_flight.insert(
            (coord, layer),
            InFlightJob {
                id: job_id,
                expected_hash: expected,
                cancel: Arc::clone(&cancel),
            },
        );
        let resolution = self.cfg.field_resolution;
        let tx = self.results_tx.clone();
        executor.submit(
            priority,
            Box::new(move || {
                // Checked once, on dequeue: a superseded/evicted job becomes
                // a no-op before its kernel runs (phase-6-plan.md §6.2). The
                // integration still checks job id and both dependency keys.
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let mut inputs = inputs;
                let mut out = generate_layer(coord, layer, &mut inputs, resolution);
                out.job_id = job_id;
                // The receiver may be gone if the map was dropped; the job's
                // work is simply discarded then.
                let _ = tx.send(JobResult::Tile(out));
            }),
        );
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use crate::budget::Budget;
    use crate::InlineExecutor;
    use world_core::{category_mask, TraitCategory};

    fn settled_map() -> RegionMap {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 16,
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
    fn capture_outside_the_window_is_none() {
        let map = settled_map();
        assert!(map
            .capture_at((1.0e9, 1.0e9), 0xFF, AnchorKind::Emphasize, 0.8, 1000.0)
            .is_none());
    }

    #[test]
    fn empty_mask_captures_nothing() {
        let map = settled_map();
        assert!(map
            .capture_at((0.0, 0.0), 0, AnchorKind::Emphasize, 0.8, 1000.0)
            .is_none());
    }

    #[test]
    fn capture_targets_only_masked_domains_and_is_bounded() {
        let map = settled_map();
        let mask = category_mask(&[TraitCategory::Morphology, TraitCategory::Coloration]);
        let anchor = map
            .capture_at((0.0, 0.0), mask, AnchorKind::Emphasize, 0.8, 2000.0)
            .expect("center region resident");
        assert_eq!(anchor.mask, mask);
        // Unmasked domains stay neutral (never read by `steer`).
        assert_eq!(anchor.target.get(PossibilityDomain::Hydrology), 0.5);
        assert_eq!(anchor.target.get(PossibilityDomain::Climate), 0.5);
        // Masked domains are a bounded nudge of the baseline, in range.
        for domain in [PossibilityDomain::Morphology, PossibilityDomain::Aesthetics] {
            let v = anchor.target.get(domain);
            assert!((0.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn organism_capture_records_its_species() {
        let map = settled_map();
        // Find a near region that realized organisms, and capture at one.
        let with_org = map
            .iter_active()
            .map(|r| r.coord)
            .find(|&c| map.organisms_in(c).is_some_and(|o| !o.is_empty()));
        let Some(coord) = with_org else {
            return; // no organisms in this tiny window; nothing to assert
        };
        let org = map.organisms_in(coord).unwrap()[0];
        let mask = category_mask(&[TraitCategory::Morphology]);
        let anchor = map
            .capture_at(org.world_pos, mask, AnchorKind::Emphasize, 0.8, 2000.0)
            .expect("resident");
        match anchor.source {
            AnchorSource::Organism { species } => {
                // The nearest organism to its own position is itself.
                assert_eq!(species, org.species);
            }
            other => panic!("expected an organism capture, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::budget::Budget;
    use crate::generate::{RegionTiles, CHANNEL_COUNT};
    use crate::InlineExecutor;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use world_core::layer::{LAYER_BIOME, LAYER_TERRAIN};

    const PLAYER: (f64, f64) = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

    type QueuedJob = (TaskPriority, Box<dyn FnOnce() + Send>);

    /// A single-threaded executor whose jobs run only when a test explicitly
    /// pumps them. This gives the recovery tests a deterministic gap between
    /// dispatch, authoritative-state mutation, execution, and integration.
    #[derive(Default)]
    struct ManualExecutor {
        queue: RefCell<VecDeque<QueuedJob>>,
    }

    impl ManualExecutor {
        fn len(&self) -> usize {
            self.queue.borrow().len()
        }

        fn run_next(&self) {
            let (_, job) = self
                .queue
                .borrow_mut()
                .pop_front()
                .expect("a queued generation job");
            job();
        }

        fn run_all(&self) {
            loop {
                let job = self.queue.borrow_mut().pop_front();
                let Some((_, job)) = job else {
                    break;
                };
                job();
            }
        }
    }

    impl TaskExecutor for ManualExecutor {
        fn submit(&self, priority: TaskPriority, job: Box<dyn FnOnce() + Send>) {
            self.queue.borrow_mut().push_back((priority, job));
        }

        fn parallelism(&self) -> usize {
            1
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct F32TileImage {
        dep_hash: u64,
        content_hash: u64,
        samples: Vec<u32>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct U8TileImage {
        dep_hash: u64,
        content_hash: u64,
        samples: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct U16TileImage {
        dep_hash: u64,
        content_hash: u64,
        samples: Vec<u16>,
    }

    /// Exact cached presentation state for the only resident region. Keeping
    /// both samples and their folded hashes makes atomic non-publication and
    /// oracle equality failures easy to diagnose.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RegionImage {
        channels: [Option<F32TileImage>; CHANNEL_COUNT],
        biome: Option<U8TileImage>,
        dominant: Option<U16TileImage>,
        drainage: DrainageTile,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ProducerJobState {
        None,
        Current,
        Obsolete,
    }

    fn coord() -> RegionCoord {
        RegionCoord::new(0, 0)
    }

    fn field() -> PossibilityField {
        PossibilityField::default()
    }

    fn tiny_config() -> StreamConfig {
        StreamConfig {
            near_radius: REGION_SIZE * 0.125,
            far_radius: REGION_SIZE * 0.25,
            load_radius: REGION_SIZE * 0.375,
            unload_radius: REGION_SIZE * 0.5,
            field_resolution: 2,
            // A few slots make the fail-closed realization assertion
            // non-vacuous while retaining only sixteen possible organisms.
            organisms_per_cell: 4,
            ..StreamConfig::default()
        }
    }

    fn is_fully_settled(map: &RegionMap) -> bool {
        if map.regions.len() != 1 || !map.in_flight.is_empty() {
            return false;
        }
        let Some(region) = map.regions.get(&coord()) else {
            return false;
        };
        if region.dirty_layers != 0 || region.status != GenerationStatus::Ready {
            return false;
        }
        map.layer_diagnostics(coord()).is_some_and(|diagnostics| {
            diagnostics.iter().all(|diagnostic| {
                diagnostic.expected.is_some()
                    && diagnostic.stored == diagnostic.expected
                    && !diagnostic.dirty
                    && !diagnostic.in_flight
            })
        })
    }

    fn settle_inline(map: &mut RegionMap) {
        for _ in 0..4 {
            map.update(
                PLAYER,
                0.0,
                &field(),
                &[],
                &NO_BIAS,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
            if is_fully_settled(map) {
                return;
            }
        }
        panic!(
            "tiny fixture did not settle: {:?}",
            map.layer_diagnostics(coord())
        );
    }

    fn settled_map() -> RegionMap {
        let mut map = RegionMap::new(tiny_config());
        settle_inline(&mut map);
        assert_eq!(map.regions.len(), 1, "fixture must load one region only");
        map
    }

    fn assert_fully_settled(map: &RegionMap) {
        assert!(
            is_fully_settled(map),
            "expected every stored key to equal its authoritative key: {:?}",
            map.layer_diagnostics(coord())
        );
    }

    fn region_image(map: &RegionMap) -> RegionImage {
        let tiles = map.cache.get(coord()).expect("settled region tiles");
        let channels = std::array::from_fn(|channel| {
            tiles.channels[channel].as_ref().map(|tile| F32TileImage {
                dep_hash: tile.dep_hash,
                content_hash: tile.content_hash(),
                samples: tile
                    .samples()
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect(),
            })
        });
        let biome = tiles.biome.as_ref().map(|tile| U8TileImage {
            dep_hash: tile.dep_hash,
            content_hash: tile.content_hash(),
            samples: tile.samples().to_vec(),
        });
        let dominant = tiles.dominant.as_ref().map(|tile| U16TileImage {
            dep_hash: tile.dep_hash,
            content_hash: tile.content_hash(),
            samples: tile.samples().to_vec(),
        });
        let drainage = map
            .macro_cache
            .get(macro_coord_for(coord()))
            .expect("macro tile");
        RegionImage {
            channels,
            biome,
            dominant,
            drainage: drainage.as_ref().clone(),
        }
    }

    fn dispatch_key(layer: u16) -> (RegionCoord, u16) {
        if layer == LAYER_DRAINAGE {
            (macro_coord_for(coord()), layer)
        } else {
            (coord(), layer)
        }
    }

    fn generated_payload_bytes(layer: u16) -> usize {
        let samples = usize::from(tiny_config().field_resolution).pow(2);
        let f32_bytes = samples * layer_channels(layer).len() * core::mem::size_of::<f32>();
        let categorical_bytes = match layer {
            LAYER_BIOME => samples * core::mem::size_of::<u8>(),
            LAYER_ECOLOGY => samples * core::mem::size_of::<u16>(),
            _ => 0,
        };
        f32_bytes + categorical_bytes
    }

    fn dispatch_dirty_region(
        map: &mut RegionMap,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) {
        map.dispatch_region(
            coord(),
            TaskPriority::Critical,
            &field(),
            &Budget::unlimited(),
            executor,
            stats,
        );
    }

    fn remove_layer_output(map: &mut RegionMap, layer: u16) {
        let mut tiles = map.cache.remove_region(coord()).expect("region tiles");
        for &channel in layer_channels(layer) {
            assert!(
                tiles.channels[channel].take().is_some(),
                "layer {layer} channel {channel} must start resident"
            );
        }
        if layer == LAYER_BIOME {
            assert!(tiles.biome.take().is_some(), "biome output must exist");
        }
        if layer == LAYER_ECOLOGY {
            assert!(
                tiles.dominant.take().is_some(),
                "ecology dominant output must exist"
            );
        }

        let RegionTiles {
            channels,
            biome,
            dominant,
        } = tiles;
        for (channel, tile) in channels.into_iter().enumerate() {
            if let Some(tile) = tile {
                map.cache.insert_channel(coord(), channel, tile);
            }
        }
        if let Some(tile) = biome {
            map.cache.insert_biome(coord(), tile);
        }
        if let Some(tile) = dominant {
            map.cache.insert_dominant(coord(), tile);
        }
        assert_eq!(
            map.cache
                .get(coord())
                .and_then(|resident| resident.layer_hash(layer)),
            None,
            "only the producer's atomic output set should be absent"
        );
    }

    fn materialized_declared_edges() -> Vec<(u16, u16)> {
        let mut edges = Vec::new();
        for consumer in 0..LAYER_COUNT {
            if consumer == LAYER_DRAINAGE {
                continue; // Drainage -> Terrain is an algorithm edge only.
            }
            for &producer in layer_decl(consumer).deps {
                if producer != LAYER_DRAINAGE {
                    edges.push((producer, consumer));
                }
            }
        }
        edges
    }

    #[test]
    fn integration_revalidates_every_result_shape_with_and_without_cancellation() {
        for cancellation in [true, false] {
            for layer in 0..LAYER_COUNT {
                let mut map = settled_map();
                map.set_cancellation_enabled(cancellation);
                let old_image = region_image(&map);

                // Revision 1 legitimately invalidates and dispatches this
                // result. Revision 2 is then installed behind the dirty-bit
                // scheduler's back, reproducing the bookkeeping omission the
                // integration provenance gate must independently catch.
                map.revision_bumps[layer as usize] =
                    map.revision_bumps[layer as usize].wrapping_add(1);
                let region = map.regions.get_mut(&coord()).expect("resident");
                region.dirty_layers = layer_bit(layer);
                region.status = GenerationStatus::Generating;
                let executor = ManualExecutor::default();
                let mut dispatch_stats = FrameStats::default();
                dispatch_dirty_region(&mut map, &executor, &mut dispatch_stats);
                assert_eq!(
                    executor.len(),
                    1,
                    "layer {layer}, cancellation={cancellation}: one legitimate result"
                );
                assert_eq!(dispatch_stats.layers_dispatched, 1);
                let key = dispatch_key(layer);
                assert!(map.in_flight.contains_key(&key));
                let pool_bytes_after_dispatch = map.pool.bytes();

                let dirty_before_omission = map.regions[&coord()].dirty_layers;
                map.revision_bumps[layer as usize] =
                    map.revision_bumps[layer as usize].wrapping_add(1);
                assert_eq!(
                    map.regions[&coord()].dirty_layers,
                    dirty_before_omission,
                    "authoritative revision changed without dirty bookkeeping"
                );

                executor.run_next();
                let mut integration_stats = FrameStats::default();
                map.integrate_finished(&mut integration_stats);

                assert_eq!(
                    integration_stats.results_dropped, 1,
                    "layer {layer}, cancellation={cancellation}"
                );
                assert_eq!(integration_stats.layers_regenerated, 0);
                if layer != LAYER_DRAINAGE {
                    assert!(
                        map.pool.bytes()
                            >= pool_bytes_after_dispatch + generated_payload_bytes(layer),
                        "layer {layer}, cancellation={cancellation}: every rejected output buffer must return to the pool"
                    );
                }
                assert_eq!(
                    region_image(&map),
                    old_image,
                    "layer {layer}, cancellation={cancellation}: rejection must publish no channel"
                );
                assert!(
                    !map.in_flight.contains_key(&key),
                    "matching completed dispatch must retire"
                );
                let closure = dependents_closure(layer);
                assert_eq!(
                    map.regions[&coord()].dirty_layers,
                    closure,
                    "layer {layer}, cancellation={cancellation}: exact retry closure"
                );
                assert_eq!(map.regions[&coord()].status, GenerationStatus::Generating);

                settle_inline(&mut map);
                assert_fully_settled(&map);
            }
        }
    }

    #[test]
    fn dispatch_key_mismatch_is_rejected_before_any_channel_is_published() {
        let layer = LAYER_TERRAIN;
        let mut map = settled_map();
        let old_image = region_image(&map);
        map.revision_bumps[layer as usize] = map.revision_bumps[layer as usize].wrapping_add(1);
        let region = map.regions.get_mut(&coord()).expect("resident");
        region.dirty_layers = layer_bit(layer);
        region.status = GenerationStatus::Generating;

        let executor = ManualExecutor::default();
        let mut dispatch_stats = FrameStats::default();
        dispatch_dirty_region(&mut map, &executor, &mut dispatch_stats);
        let key = dispatch_key(layer);
        let current = map
            .expected_layer_hash(coord(), layer)
            .expect("resident expected key");
        let job = map.in_flight.get_mut(&key).expect("terrain dispatch");
        assert_eq!(job.expected_hash, current);
        job.expected_hash ^= 0xA11C_EBAD_D15C_A7C1;
        assert_ne!(job.expected_hash, current);
        let pool_bytes_after_dispatch = map.pool.bytes();

        executor.run_next();
        let mut integration_stats = FrameStats::default();
        map.integrate_finished(&mut integration_stats);

        assert_eq!(integration_stats.results_dropped, 1);
        assert_eq!(integration_stats.layers_regenerated, 0);
        assert!(
            map.pool.bytes() >= pool_bytes_after_dispatch + generated_payload_bytes(layer),
            "dispatch-key rejection must reclaim the generated output buffer"
        );
        assert_eq!(region_image(&map), old_image);
        assert!(!map.in_flight.contains_key(&key));
        assert_eq!(
            map.regions[&coord()].dirty_layers,
            dependents_closure(layer)
        );
        assert_eq!(map.regions[&coord()].status, GenerationStatus::Generating);

        settle_inline(&mut map);
        assert_fully_settled(&map);
    }

    #[test]
    fn rejected_macro_marks_every_covered_resident_retryable() {
        let mut map = settled_map();
        let second = RegionCoord::new(1, 0);
        let macro_coord = macro_coord_for(coord());
        assert_eq!(macro_coord_for(second), macro_coord);

        // A second level-0 resident is enough to exercise the macro fan-out;
        // its field cache is irrelevant because rejection must only restore
        // scheduling state and leave every cache untouched.
        let mut second_region = RegionState::new(second);
        second_region.current = field().sample(second);
        second_region.target = second_region.current;
        second_region.status = GenerationStatus::Ready;
        map.regions.insert(second, second_region);
        let old_macro = map
            .macro_cache
            .get(macro_coord)
            .expect("settled macro")
            .as_ref()
            .clone();

        map.revision_bumps[LAYER_DRAINAGE as usize] =
            map.revision_bumps[LAYER_DRAINAGE as usize].wrapping_add(1);
        let dispatch_region = map.regions.get_mut(&coord()).expect("dispatch resident");
        dispatch_region.dirty_layers = layer_bit(LAYER_DRAINAGE);
        dispatch_region.status = GenerationStatus::Generating;

        let executor = ManualExecutor::default();
        let mut dispatch_stats = FrameStats::default();
        dispatch_dirty_region(&mut map, &executor, &mut dispatch_stats);
        let key = (macro_coord, LAYER_DRAINAGE);
        assert!(map.in_flight.contains_key(&key));
        assert_eq!(executor.len(), 1);

        // Make the queued result stale without repairing either resident's
        // dirty bookkeeping, then execute it so integration must fan out.
        map.revision_bumps[LAYER_DRAINAGE as usize] =
            map.revision_bumps[LAYER_DRAINAGE as usize].wrapping_add(1);
        executor.run_next();
        let mut integration_stats = FrameStats::default();
        map.integrate_finished(&mut integration_stats);

        assert_eq!(integration_stats.results_dropped, 1);
        assert!(!map.in_flight.contains_key(&key));
        assert_eq!(
            map.macro_cache
                .get(macro_coord)
                .expect("old macro retained")
                .as_ref(),
            &old_macro
        );
        let closure = dependents_closure(LAYER_DRAINAGE);
        for resident in [coord(), second] {
            assert_eq!(
                map.regions[&resident].dirty_layers, closure,
                "covered resident {resident:?} must receive the exact closure"
            );
            assert_eq!(map.regions[&resident].status, GenerationStatus::Generating);
        }
    }

    #[test]
    fn every_materialized_declared_edge_repairs_missing_producers() {
        let edges = materialized_declared_edges();
        assert_eq!(edges.len(), 18, "the declared cached-edge matrix changed");

        let base_oracle = region_image(&settled_map());
        let unique_producers: BTreeSet<u16> = edges.iter().map(|(producer, _)| *producer).collect();
        let mut revised_oracles = BTreeMap::new();
        for producer in unique_producers {
            let mut oracle = settled_map();
            oracle.bump_layer_revision(producer);
            settle_inline(&mut oracle);
            revised_oracles.insert(producer, region_image(&oracle));
        }

        for (producer, consumer) in edges {
            for job_state in [
                ProducerJobState::None,
                ProducerJobState::Current,
                ProducerJobState::Obsolete,
            ] {
                let mut map = settled_map();
                remove_layer_output(&mut map, producer);
                let region = map.regions.get_mut(&coord()).expect("resident");
                region.dirty_layers = layer_bit(consumer);
                region.status = GenerationStatus::Generating;
                assert_eq!(
                    region.dirty_layers & layer_bit(producer),
                    0,
                    "producer hint must remain omitted before repair"
                );

                let executor = ManualExecutor::default();
                let expected_at_dispatch = map
                    .expected_layer_hash(coord(), producer)
                    .expect("producer expected key");
                let mut old_job = None;
                if job_state != ProducerJobState::None {
                    map.submit_layer(
                        coord(),
                        producer,
                        expected_at_dispatch,
                        TaskPriority::Critical,
                        &executor,
                    );
                    let job = map
                        .in_flight
                        .get(&(coord(), producer))
                        .expect("manual producer dispatch");
                    old_job = Some((job.id, job.expected_hash, Arc::clone(&job.cancel)));
                }
                if job_state == ProducerJobState::Obsolete {
                    let dirty_before_omission = map.regions[&coord()].dirty_layers;
                    map.revision_bumps[producer as usize] =
                        map.revision_bumps[producer as usize].wrapping_add(1);
                    assert_eq!(map.regions[&coord()].dirty_layers, dirty_before_omission);
                }

                let mut repair_stats = FrameStats::default();
                dispatch_dirty_region(&mut map, &executor, &mut repair_stats);
                let current_expected = map
                    .expected_layer_hash(coord(), producer)
                    .expect("current producer key");
                let repaired_job = map
                    .in_flight
                    .get(&(coord(), producer))
                    .expect("producer must be pending after same-pass repair");
                assert_eq!(
                    repaired_job.expected_hash, current_expected,
                    "{producer}->{consumer}, {job_state:?}: repaired key"
                );

                match job_state {
                    ProducerJobState::None => {
                        assert_eq!(executor.len(), 1);
                    }
                    ProducerJobState::Current => {
                        let (old_id, old_hash, old_cancel) = old_job.expect("old job");
                        assert_eq!(repaired_job.id, old_id, "current job self-superseded");
                        assert_eq!(repaired_job.expected_hash, old_hash);
                        assert!(!old_cancel.load(Ordering::Relaxed));
                        assert_eq!(repair_stats.jobs_cancelled, 0);
                        assert_eq!(executor.len(), 1);
                    }
                    ProducerJobState::Obsolete => {
                        let (old_id, old_hash, old_cancel) = old_job.expect("old job");
                        assert_ne!(old_hash, current_expected);
                        assert_ne!(repaired_job.id, old_id, "obsolete job was not replaced");
                        assert!(old_cancel.load(Ordering::Relaxed));
                        assert!(repair_stats.jobs_cancelled >= 1);
                        assert!(
                            executor.len() >= 2,
                            "old and replacement closures remain queued"
                        );
                    }
                }

                executor.run_all();
                let mut integration_stats = FrameStats::default();
                map.integrate_finished(&mut integration_stats);
                settle_inline(&mut map);
                assert_fully_settled(&map);
                let oracle = if job_state == ProducerJobState::Obsolete {
                    &revised_oracles[&producer]
                } else {
                    &base_oracle
                };
                assert_eq!(
                    &region_image(&map),
                    oracle,
                    "{producer}->{consumer}, {job_state:?}: recovered world differs from oracle"
                );
            }
        }
    }

    #[test]
    fn realization_fails_closed_until_required_rosters_are_rebuilt() {
        let mut map = settled_map();
        let old_organisms = map
            .organisms
            .get(&coord())
            .expect("near region realization")
            .clone();
        assert!(
            !old_organisms.is_empty(),
            "four realization slots should make the fixture non-vacuous"
        );
        let old_key = map.organism_keys[&coord()];

        // Land a new L8 key without running capacity maintenance or the
        // realization pass. This leaves a real old organism vector/key that a
        // retry is expected to replace.
        map.bump_layer_revision(LAYER_ECOLOGY);
        let mut generation_stats = FrameStats::default();
        map.dispatch_regen(
            PLAYER,
            &field(),
            &Budget::unlimited(),
            &InlineExecutor,
            &mut generation_stats,
        );
        let new_key = map
            .cache
            .get(coord())
            .and_then(|tiles| tiles.layer_hash(LAYER_ECOLOGY))
            .expect("new ecology output");
        assert_ne!(new_key, old_key);
        assert_eq!(map.organism_keys[&coord()], old_key);
        assert_eq!(map.organisms[&coord()], old_organisms);

        let required = map
            .region_signatures
            .get(&coord())
            .expect("ecology signature bookkeeping")
            .clone();
        assert!(!required.is_empty());
        let exact_entries: BTreeMap<_, _> = required
            .iter()
            .map(|&signature| {
                (
                    signature,
                    map.roster_cache
                        .get(signature)
                        .expect("pre-eviction required roster")
                        .as_ref()
                        .clone(),
                )
            })
            .collect();

        // Simulate legacy capacity behavior: all entries disappear, but the
        // resident Ecology dependent set remains authoritative.
        map.roster_cache = RosterCache::default();
        assert!(map.roster_cache.is_empty());
        assert_eq!(map.region_signatures[&coord()], required);

        let mut failed_stats = FrameStats::default();
        map.realize_near_window(PLAYER, &Budget::unlimited(), &mut failed_stats);
        assert_eq!(failed_stats.organisms_realized, 0);
        assert_eq!(map.organism_keys[&coord()], old_key);
        assert_eq!(map.organisms[&coord()], old_organisms);

        let protected = map.maintain_roster_working_set();
        assert_eq!(protected, required);
        assert_eq!(map.roster_cache.len(), required.len());
        for (&signature, expected) in &exact_entries {
            assert_eq!(
                map.roster_cache
                    .get(signature)
                    .expect("maintenance rebuilt required roster")
                    .as_ref(),
                expected
            );
        }

        let mut retry_stats = FrameStats::default();
        map.realize_near_window(PLAYER, &Budget::unlimited(), &mut retry_stats);
        assert_eq!(map.organism_keys[&coord()], new_key);
        assert_eq!(
            map.organisms[&coord()],
            old_organisms,
            "same inputs and exact roster rebuild must realize identically"
        );
    }
}
