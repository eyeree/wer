//! Region streaming: the moving window of active regions, the distance-driven
//! stability ramp, budgeted convergence, and dependency-precise incremental
//! regeneration (phase-1-plan.md sections 4.2 and 7; phase-2-plan.md §7.8, §8).
//!
//! [`RegionMap::update`] is the once-per-frame heart of the runtime:
//!
//! 1. integrate finished generation jobs,
//! 2. remove authority beyond `unload_radius` and park disposable fields under
//!    capacity pressure, sweeping orphaned derived inputs with them,
//! 3. create missing authority and admit eligible field working sets,
//! 4. recompute every region's stability, then budget steered targets,
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
    LAYER_DRAINAGE, LAYER_ECOLOGY, LAYER_TERRAIN,
};
use world_core::{
    anchor_influence_profile, anchor_set_signature, capture_target, domain_mask, drainage_dep_hash,
    layer_dep_hash, macro_coord_for, mix, organism_trait_deviation, project_plausible, steer,
    terrain_dep_hash, Anchor, AnchorKind, AnchorSource, Biome, Climate, DrainageTile, Genome,
    GenomeBias, HabitatSignature, PossibilityDomain, PossibilityField, PossibilitySignature,
    PossibilityVector, RegionCoord, Soils, TraitDeviation, POSSIBILITY_DIMS, REGION_SIZE,
};

use crate::budget::Budget;
use crate::generate::{
    full_region_payload_bytes, generate_layer, layer_channels, GeneratedTile, LayerInputs,
    RegionCache, RegionTiles, TerrainPossibilityHalo, TileBuffers, CHANNEL_DIVERSITY,
    CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_HERBIVORE, CHANNEL_MOISTURE, CHANNEL_PREDATOR,
    CHANNEL_RIVER, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};
use crate::macrocache::MacroCache;
use crate::pool::TilePool;
use crate::realize::{realize_region_into, Organism};
use crate::region::{GenerationStatus, RegionState};
use crate::resonance::{
    combine_resonance, density_term, gated_rate, species_entropy, Resonance, ResonanceNode,
    MAX_RESONANCE_NODES,
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
    /// Regions within this radius are admitted to the authoritative window.
    pub load_radius: f64,
    /// Regions beyond this radius are removed from the authoritative window.
    /// Must exceed `load_radius`; the gap is the hysteresis that prevents
    /// thrashing at the boundary.
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
    /// Byte target for disposable field payload (phase-6-plan.md §4.3): after
    /// the radius sweep, capacity pressure parks farthest field working sets
    /// until their full-payload reservations fit, exempting preserved regions
    /// and everything inside `near_radius`. Authority and its transformation
    /// history remain resident (ADR 0023); every tile re-derives from its
    /// dependency hash (ADR 0008).
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
    /// hold. Slot 0 remains the canonical gameplay sample; higher slots are
    /// presentation-only (ADRs 0010 and 0024), so no persisted or shared byte
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
    /// Authoritative regions inserted this frame.
    pub loaded: usize,
    /// Authoritative regions removed beyond `unload_radius` this frame.
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
    /// Authoritative resident regions after this frame, parked entries included.
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
    /// Canonical slot-0 organisms instantiated by the fixed one-region
    /// authoritative pass this frame (ADR 0024). This semantic work is not
    /// charged to a resource-tier budget.
    pub authoritative_organisms_realized: usize,
    /// Presentation organisms instantiated while expanding canonical vectors
    /// to the configured visual density this frame (≤
    /// `max_realize_organisms` modulo a final region overshoot).
    pub organisms_realized: usize,
    /// Total near-field organisms resident after this frame.
    pub organisms: usize,
    /// Transition capability at the player this frame, `0..=1` — the resonance
    /// gate multiplier folded into convergence (phase-4-plan.md §8.3, ADR 0012).
    pub resonance_strength: f32,
    /// Contributing canonical nodes in this frame's resonance graph (≤
    /// [`MAX_RESONANCE_NODES`]).
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
    /// Region field working sets parked by the byte-capacity target (beyond
    /// the radius sweep; ADR 0023).
    pub evicted_for_capacity: usize,
    /// Authoritative regions whose steered target calculation was deferred by
    /// `max_retarget_regions` (phase-6-plan.md §6.4). Geometric stability is
    /// never deferred (ADR 0023).
    pub retarget_deferred: usize,
}

/// Canonical hash of the steering inputs (bias + anchor multiset) — a change
/// forces a full retarget instead of the amortized round-robin
/// (phase-6-plan.md §6.4; ADR 0025). The core signature owns the one complete
/// steering-field list, retains duplicate occurrences, and ignores metadata.
fn steering_signature(anchors: &[Anchor], bias: &[f32; POSSIBILITY_DIMS]) -> u64 {
    let mut h: u64 = 0x5EED_5163_0000_0006;
    for b in bias {
        h = mix(h, u64::from(b.to_bits()));
    }
    mix(h, anchor_set_signature(anchors))
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

/// The authoritative streaming window plus its disposable field/macro caches.
///
/// Region state lives in a `BTreeMap`, not a hash map: iteration order is part
/// of the determinism contract. Budgeted work (loads, convergence, regen) must
/// pick the same regions in the same order on every run for the continuity
/// replay's two-run equality check to hold.
#[derive(Debug)]
pub struct RegionMap {
    cfg: StreamConfig,
    /// Active field recipe used by fallback Terrain halos and macro topology.
    /// Synchronized before any result integrates in an update (ADR 0027).
    field_recipe: PossibilityField,
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
    /// The L8 dependency hash each region's canonical slot-0 organisms were
    /// realized from. This is gameplay availability, independent of visual
    /// density (ADR 0024).
    authoritative_organism_keys: BTreeMap<RegionCoord, u64>,
    /// The L8 dependency hash and slot count represented by the presentation
    /// vector. Empty realizations retain a key so barrenness is complete work.
    presentation_organism_keys: BTreeMap<RegionCoord, (u64, u16)>,
    /// Every preserve contribution covering a region, keyed by immutable
    /// content id (ADR 0020). The lowest id is the effective owner. The nested
    /// map is deliberately retained across region eviction so a later load
    /// uses the then-current winner and deleting a winner can reveal its
    /// successor without depending on application order.
    preserve_contributors: BTreeMap<RegionCoord, BTreeMap<u64, PossibilitySignature>>,
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
    /// Hash of the steering inputs (bias + anchors) at the last target pass. A
    /// change forces every target to refresh; otherwise the pass round-robins under
    /// `max_retarget_regions` (phase-6-plan.md §6.4).
    steer_signature: u64,
    /// Round-robin position of the amortized target calculation.
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
            field_recipe: PossibilityField::default(),
            regions: BTreeMap::new(),
            cache: RegionCache::default(),
            macro_cache: MacroCache::default(),
            roster_cache: RosterCache::default(),
            region_signatures: BTreeMap::new(),
            organisms: BTreeMap::new(),
            authoritative_organism_keys: BTreeMap::new(),
            presentation_organism_keys: BTreeMap::new(),
            preserve_contributors: BTreeMap::new(),
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

    /// Whether a resident authority currently owns an admitted field working
    /// set. `Unloaded` is capacity-parked authority, not absence (ADR 0023).
    #[inline]
    const fn field_active(region: &RegionState) -> bool {
        !matches!(region.status, GenerationStatus::Unloaded)
    }

    fn fallback_terrain_pair(&self, coord: RegionCoord) -> [u16; 2] {
        let base = project_plausible(self.field_recipe.sample(coord)).requantized();
        [
            base.quantized(PossibilityDomain::Planetary),
            base.quantized(PossibilityDomain::Geology),
        ]
    }

    fn effective_terrain_pair(&self, coord: RegionCoord) -> [u16; 2] {
        self.regions.get(&coord).map_or_else(
            || self.fallback_terrain_pair(coord),
            |region| {
                [
                    region.current.quantized(PossibilityDomain::Planetary),
                    region.current.quantized(PossibilityDomain::Geology),
                ]
            },
        )
    }

    fn terrain_halo(&self, center: RegionCoord) -> TerrainPossibilityHalo {
        debug_assert_eq!(center.level, 0);
        let mut buckets = [[[0; 2]; 3]; 3];
        for dy in -1..=1 {
            for dx in -1..=1 {
                let coord = RegionCoord::new(center.x + dx, center.y + dy);
                buckets[(dy + 1) as usize][(dx + 1) as usize] = self.effective_terrain_pair(coord);
            }
        }
        TerrainPossibilityHalo::new(center, buckets)
    }

    /// Invalidate every admitted Terrain consumer whose 3×3 halo includes the
    /// changed absolute source. Parked authority remains a source but has no
    /// disposable closure to dirty (ADR 0027).
    fn invalidate_terrain_consumers(&mut self, source: RegionCoord, stats: &mut FrameStats) {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let consumer = RegionCoord::new(source.x + dx, source.y + dy);
                if self.regions.get(&consumer).is_some_and(Self::field_active) {
                    self.mark_dirty_closure(consumer, LAYER_TERRAIN, stats);
                }
            }
        }
    }

    /// Full-payload reservations currently charged to the disposable field
    /// target. Near and contributor-covered field sets are explicit
    /// exemptions and therefore form a floor above the configured target.
    fn disposable_field_reservations(&self, player: (f64, f64)) -> usize {
        self.regions
            .values()
            .filter(|region| {
                Self::field_active(region)
                    && center_distance(region.coord, player) > self.cfg.near_radius
                    && !self.preserve_contributors.contains_key(&region.coord)
            })
            .count()
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

    /// Recover every buffer of a parked or removed region's tiles whose `Arc` the map
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

    /// The disposable field cache. Capacity-parked authoritative residents are
    /// intentionally absent from this traversal (ADR 0023).
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

    /// All realized near-field organisms in the window, including additive
    /// presentation slots (phase-3-plan.md §8.3; ADR 0024).
    pub fn organisms(&self) -> impl Iterator<Item = &Organism> {
        self.organisms.values().flatten()
    }

    /// Canonical slot-0 organisms available to gameplay in deterministic
    /// region/vector order (ADR 0024).
    pub fn authoritative_organisms(&self) -> impl Iterator<Item = &Organism> {
        self.organisms().filter(|organism| organism.slot == 0)
    }

    /// A pinned near region's realized organisms, if any.
    #[inline]
    #[must_use]
    pub fn organisms_in(&self, coord: RegionCoord) -> Option<&[Organism]> {
        self.organisms.get(&coord).map(Vec::as_slice)
    }

    /// Canonical slot-0 organisms for one region. The iterator may be empty for
    /// a completed barren realization.
    pub fn authoritative_organisms_in(
        &self,
        coord: RegionCoord,
    ) -> impl Iterator<Item = &Organism> {
        self.organisms
            .get(&coord)
            .into_iter()
            .flatten()
            .filter(|organism| organism.slot == 0)
    }

    /// Total near-field organisms currently resident.
    #[inline]
    #[must_use]
    pub fn organism_count(&self) -> usize {
        self.organisms.values().map(Vec::len).sum()
    }

    /// Whether every field-active region in the current near window has a
    /// fresh, roster-complete canonical slot-0 publication. This is a settling
    /// observation for harnesses and session restore; it does not advance work
    /// or require optional visual-density expansion (ADR 0024).
    #[must_use]
    pub fn authoritative_realization_complete(&self, player: (f64, f64)) -> bool {
        self.near_realization_coords(player)
            .into_iter()
            .all(|coord| {
                let Some(hash) = self.fresh_ecology_hash(coord) else {
                    return false;
                };
                self.realization_rosters_complete(coord)
                    && self.organisms.contains_key(&coord)
                    && self.authoritative_organism_keys.get(&coord) == Some(&hash)
            })
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
    /// authoritative slot-0 organism (for an organism capture) or the terrain/hydrology/
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

        // Organism capture: the nearest authoritative slot-0 organism drives
        // the M/B/A/E deviation (§7.1; ADR 0024). Its genome is reconstructed
        // from its species id
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

    /// The authoritative slot-0 organism nearest a world position within its
    /// region, if any are resident there (the near window only).
    fn nearest_organism(&self, coord: RegionCoord, world_pos: (f64, f64)) -> Option<&Organism> {
        let dist2 = |p: (f64, f64)| {
            let dx = p.0 - world_pos.0;
            let dy = p.1 - world_pos.1;
            dx * dx + dy * dy
        };
        self.authoritative_organisms_in(coord).min_by(|a, b| {
            dist2(a.world_pos)
                .partial_cmp(&dist2(b.world_pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Build the transient resonance graph at the player and its gate strength
    /// (phase-4-plan.md §7.5). A pure read of the settled near-window caches:
    /// canonical slot-0 organisms within `near_radius` become nodes
    /// (nearest-first, capped at [`MAX_RESONANCE_NODES`]), and their
    /// count/diversity/distance,
    /// the local anchor compatibility, and a canopy occlusion proxy combine into
    /// a bounded `strength`. Order-independent and deterministic; never stored.
    ///
    /// `anchors` must be the same effective multiset (explicit plus normalized
    /// route-derived anchors) that produced the resident authoritative targets.
    /// This direct-call precondition keeps the active-domain profile coherent
    /// with the final projected desire being scored (ADR 0026).
    #[must_use]
    pub fn resonance_at(&self, player: (f64, f64), anchors: &[Anchor]) -> Resonance {
        let radius = self.cfg.near_radius;
        if radius <= 0.0 {
            return Resonance::empty();
        }
        let radius2 = radius * radius;
        // Collect near organisms with their squared distance for a stable sort.
        let mut candidates: Vec<(u64, ResonanceNode)> = Vec::new();
        for org in self.authoritative_organisms() {
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
        candidates.truncate(MAX_RESONANCE_NODES);
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

    /// How well the covering region's authoritative current state agrees with
    /// its authoritative final projected target over actively steered domains
    /// (ADR 0026). Domain relevance is the canonical saturating influence of
    /// the same effective anchor multiset at the region center. Missing
    /// authority, an effective preserve, or no active influence is neutral.
    fn anchor_compatibility(&self, player: (f64, f64), anchors: &[Anchor]) -> f32 {
        let coord = RegionCoord::from_world(player.0, player.1);
        if self.preserve_contributors.contains_key(&coord) {
            return 1.0;
        }
        let Some(region) = self.regions.get(&coord) else {
            return 1.0;
        };
        let (ox, oy) = coord.origin();
        let center = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
        let profile = anchor_influence_profile(anchors, center);
        let mut weight_sum = 0.0f32;
        let mut diff_sum = 0.0f32;
        for (domain, &weight) in profile.iter().enumerate() {
            diff_sum += weight * (region.current.dims[domain] - region.target.dims[domain]).abs();
            weight_sum += weight;
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

    /// An authoritative resident's state, whether field-active or parked.
    #[inline]
    #[must_use]
    pub fn get(&self, coord: RegionCoord) -> Option<&RegionState> {
        self.regions.get(&coord)
    }

    /// Owned Terrain P/G halo for a resident level-0 region. Presentation
    /// workers use this same authoritative snapshot rather than reconstructing
    /// a region-constant vector (ADR 0027).
    #[must_use]
    pub fn terrain_possibility_halo(&self, coord: RegionCoord) -> Option<TerrainPossibilityHalo> {
        self.regions
            .contains_key(&coord)
            .then(|| self.terrain_halo(coord))
    }

    /// Number of authoritative residents, parked entries included.
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

    /// Iterate authoritative residents in deterministic coordinate order,
    /// parked entries included. The historical name remains API-compatible;
    /// use [`RegionMap::cache`] for derived field residency.
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

    /// Add or replace one preserve's contribution to a region (ADR 0020).
    ///
    /// All contributors are retained and the numerically lowest content id is
    /// authoritative. Contributor-only churn is inert. A newly effective
    /// signature snaps a resident to its bucket centers; an exact vector
    /// change advances the region revision and retires old-revision organisms,
    /// while only quantized bucket changes dirty generated layers (ADR 0007).
    /// Applying the same `(preserve_id, coord, signature)` is idempotent.
    pub fn apply_preserve_contribution(
        &mut self,
        preserve_id: u64,
        coord: RegionCoord,
        signature: PossibilitySignature,
    ) {
        self.apply_preserve_contributions([(preserve_id, coord, signature)]);
    }

    /// Atomically apply a synchronization batch of preserve contributions.
    ///
    /// The complete batch is installed before each touched resident coordinate
    /// reconciles exactly once from its pre-batch winner to its final lowest-id
    /// winner. This is the startup/session/import seam: reversing distinct
    /// preserve records in one batch cannot create intermediate revision or
    /// organism epochs. Separate calls remain separate material history and may
    /// legitimately advance revision once per effective-winner change.
    ///
    /// Repeated entries for the same `(preserve_id, coord)` retain the last
    /// signature in the supplied canonical record traversal. Duplicate
    /// coordinate policy is finding 25 and is deliberately not changed here.
    pub fn apply_preserve_contributions(
        &mut self,
        contributions: impl IntoIterator<Item = (u64, RegionCoord, PossibilitySignature)>,
    ) {
        let mut old_winners = BTreeMap::new();
        for (preserve_id, coord, signature) in contributions {
            if let std::collections::btree_map::Entry::Vacant(entry) = old_winners.entry(coord) {
                entry.insert(self.effective_preserve(coord));
            }
            self.preserve_contributors
                .entry(coord)
                .or_default()
                .insert(preserve_id, signature);
        }
        for (coord, old) in old_winners {
            let new = self.effective_preserve(coord);
            self.reconcile_preserve_winner(coord, old, new);
        }
    }

    /// Remove exactly one preserve's contribution from a region.
    ///
    /// Removing a non-winner is bookkeeping-only. Removing the winner applies
    /// the lowest-id successor, if any. Removing the final contributor releases
    /// the region without snapping or changing its revision, tiles, jobs, or
    /// organisms; ordinary retargeting resumes on a later update (ADR 0020).
    /// Returns whether the contribution existed.
    pub fn remove_preserve_contribution(&mut self, preserve_id: u64, coord: RegionCoord) -> bool {
        let old = self.effective_preserve(coord);
        let removed = self
            .preserve_contributors
            .get_mut(&coord)
            .is_some_and(|contributors| contributors.remove(&preserve_id).is_some());
        if !removed {
            return false;
        }
        if self
            .preserve_contributors
            .get(&coord)
            .is_some_and(BTreeMap::is_empty)
        {
            self.preserve_contributors.remove(&coord);
        }
        let new = self.effective_preserve(coord);
        self.reconcile_preserve_winner(coord, old, new);
        true
    }

    /// The deterministic effective preserve owner and signature for a region.
    /// The inner ordered map is the sole source of truth; no cached winner can
    /// drift from the contributor set (ADR 0020).
    #[must_use]
    pub fn effective_preserve(&self, coord: RegionCoord) -> Option<(u64, PossibilitySignature)> {
        self.preserve_contributors
            .get(&coord)
            .and_then(BTreeMap::first_key_value)
            .map(|(&id, &signature)| (id, signature))
    }

    /// Whether a region has at least one preserve contributor.
    #[inline]
    #[must_use]
    pub fn is_overridden(&self, coord: RegionCoord) -> bool {
        self.preserve_contributors.contains_key(&coord)
    }

    /// Reconcile one contributor mutation against its old and new effective
    /// owner. Owner changes with an equal signature are deliberately inert.
    fn reconcile_preserve_winner(
        &mut self,
        coord: RegionCoord,
        old: Option<(u64, PossibilitySignature)>,
        new: Option<(u64, PossibilitySignature)>,
    ) {
        let old_signature = old.map(|(_, signature)| signature);
        let new_signature = new.map(|(_, signature)| signature);
        if let Some(signature) = new_signature {
            if Some(signature) != old_signature {
                self.apply_effective_preserve_signature(coord, signature);
            }
        }
        // `Some -> None` is the intentional no-snap release. Retargeting on a
        // subsequent update will restore ordinary stability and target state.
    }

    /// Snap a newly effective signature into an already resident region.
    /// Exact vector and bucket changes are separate contracts: the former
    /// advances identity epoch and retires organisms, the latter invalidates
    /// only declared layer readers.
    fn apply_effective_preserve_signature(
        &mut self,
        coord: RegionCoord,
        signature: PossibilitySignature,
    ) {
        let Some(region) = self.regions.get_mut(&coord) else {
            return;
        };
        let old_current = region.current;
        let snapped = signature.dequantize();
        let mut flipped = 0u8;
        for (i, domain) in PossibilityDomain::ALL.iter().enumerate() {
            if old_current.quantized(*domain) != snapped.quantized(*domain) {
                flipped |= 1 << i;
            }
        }
        let material = old_current != snapped;
        region.current = snapped;
        region.target = snapped;
        region.stability = 1.0;
        if material {
            region.revision = region.revision.wrapping_add(1);
        }
        let dirtied = domain_dirty_mask(flipped);
        if dirtied != 0 {
            region.dirty_layers |= dirtied;
            if Self::field_active(region) && region.status == GenerationStatus::Ready {
                region.status = GenerationStatus::Generating;
            }
        }
        if material {
            self.retire_organisms(coord);
        }
        // Superseded in-flight work stops only for layers whose quantized
        // inputs changed. Same-bucket normalization leaves all jobs intact.
        self.cancel_in_flight(coord, dirtied);
        let slow_mask =
            (1 << PossibilityDomain::Planetary.index()) | (1 << PossibilityDomain::Geology.index());
        if flipped & slow_mask != 0 {
            self.invalidate_terrain_consumers(coord, &mut FrameStats::default());
        }
    }

    /// Restore one region from a session snapshot (phase-5-plan.md §12.2):
    /// bit-exact `current`, stability, and revision, target = current (the
    /// next field admission recomputes it from the live inputs). Restored
    /// authority begins parked; admission later dirties every layer so caches,
    /// rosters, and organisms re-derive deterministically from the restored
    /// possibility state (ADRs 0008 and 0023). Loading is not an event: nothing
    /// converges and no target moves beyond what the live run would compute.
    pub fn restore_region(&mut self, snap: &world_core::RegionSnapshotRecord) {
        let old_pair = self.effective_terrain_pair(snap.coord);
        if self.regions.contains_key(&snap.coord) {
            self.park_region_fields(snap.coord, &mut FrameStats::default());
        } else {
            self.retire_organisms(snap.coord);
        }
        let mut region = RegionState::new(snap.coord);
        region.current = PossibilityVector { dims: snap.current };
        region.target = region.current;
        region.stability = snap.stability;
        region.revision = snap.revision;
        region.dirty_layers = 0;
        region.status = GenerationStatus::Unloaded;
        self.regions.insert(snap.coord, region);
        if self.effective_terrain_pair(snap.coord) != old_pair {
            self.invalidate_terrain_consumers(snap.coord, &mut FrameStats::default());
        }
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
            if Self::field_active(region) && region.status == GenerationStatus::Ready {
                region.status = GenerationStatus::Generating;
            }
        }
        if mask & layer_bit(LAYER_ECOLOGY) != 0 {
            let coords: Vec<_> = self.organisms.keys().copied().collect();
            for coord in coords {
                self.retire_organisms(coord);
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
        self.synchronize_field_recipe(*field, anchors, bias, &mut stats);
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
        // Publish at most one nearest fresh canonical slot-0 region before any
        // gameplay read. This fixed semantic pass is independent of visual
        // density and resource budgets (ADR 0024).
        timings.time(Pass::Realize, || {
            self.realize_authoritative_near_window(player, &mut stats);
        });
        // Resonance is a pure read of the current canonical near-window view,
        // computed after retarget with the same effective slice so its active
        // profile scores this frame's authoritative final targets (ADR 0026),
        // and before converge so the rate sees this frame's gate (§8.2).
        let resonance = self.resonance_at(player, anchors);
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
        // Expand already-canonical regions to the tier's optional visual
        // density after integration. A newly ready L8 waits until next frame's
        // fixed authoritative pass, so presentation capacity cannot accelerate
        // gameplay availability (ADR 0024).
        timings.time(Pass::Realize, || {
            self.expand_visual_near_window(player, budget, &mut stats);
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

    /// Make a field-recipe transition atomic with provenance validation. Macro
    /// jobs, fallback-sensitive Terrain closures, and every ordinary target are
    /// refreshed before an old result can integrate (ADR 0027).
    fn synchronize_field_recipe(
        &mut self,
        field: PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
        stats: &mut FrameStats,
    ) {
        if self.field_recipe == field {
            return;
        }
        self.field_recipe = field;

        let macro_jobs: Vec<_> = self
            .in_flight
            .keys()
            .filter(|(_, layer)| *layer == LAYER_DRAINAGE)
            .copied()
            .collect();
        for key in macro_jobs {
            stats.jobs_cancelled += self.retire_in_flight(key);
        }

        let coords: Vec<_> = self.regions.keys().copied().collect();
        for &coord in &coords {
            if self.regions.get(&coord).is_some_and(Self::field_active) {
                self.mark_dirty_closure(coord, LAYER_TERRAIN, stats);
            }
        }
        // The field recipe is a target input as well as a fallback/topology
        // input. Refresh all authority immediately, bypassing amortized target
        // budgets so no region can converge toward the old recipe.
        for coord in coords {
            if self.preserve_contributors.contains_key(&coord) {
                continue;
            }
            let target = self.target_for(coord, &field, anchors, bias);
            self.regions.get_mut(&coord).expect("resident").target = target;
        }
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
            self.field_recipe.cell_regions,
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
        if layer == LAYER_TERRAIN {
            let buckets = self.terrain_halo(coord).dependency_buckets();
            let hash = terrain_dep_hash(
                coord,
                self.effective_revision(layer),
                &buckets,
                self.cfg.field_resolution,
            );
            memo[layer as usize] = Some(hash);
            return hash;
        }
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
                                if Self::field_active(region)
                                    && region.status == GenerationStatus::Ready
                                {
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
    /// region and retire obsolete dependent dispatches. Parked authority keeps
    /// `Unloaded`; only admitted fields may become visibly generating.
    fn mark_dirty_closure(&mut self, coord: RegionCoord, layer: u16, stats: &mut FrameStats) {
        if !self.regions.contains_key(&coord) {
            return;
        }
        let mask = dependents_closure(layer);
        if let Some(region) = self.regions.get_mut(&coord) {
            region.dirty_layers |= mask;
            if Self::field_active(region) {
                region.status = GenerationStatus::Generating;
            }
        }
        stats.jobs_cancelled += self.cancel_in_flight(coord, mask);
    }

    /// Recompute an admitted region's `GenerationStatus` from its dirty bits
    /// and in-flight jobs. Parking is changed only by field admission.
    fn refresh_status(&mut self, coord: RegionCoord) {
        let in_flight = self
            .in_flight
            .range((coord, 0)..=(coord, u16::MAX))
            .next()
            .is_some();
        if let Some(region) = self.regions.get_mut(&coord) {
            if !Self::field_active(region) {
                return;
            }
            region.status = if region.dirty_layers == 0 && !in_flight {
                GenerationStatus::Ready
            } else {
                GenerationStatus::Generating
            };
        }
    }

    /// Remove authority beyond `unload_radius`, park disposable field working
    /// sets under capacity pressure, then sweep derived inputs against the
    /// remaining field-active dependents (ADR 0023).
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
        let capacity_parked = self.enforce_capacity(player, stats);
        if radius_evicted || capacity_parked {
            self.sweep_dependent_caches(stats);
        }
    }

    /// Park one resident's disposable fields while retaining its authoritative
    /// possibility history. Late results lose dispatch identity and therefore
    /// cannot recreate cache entries (ADR 0023).
    fn park_region_fields(&mut self, coord: RegionCoord, stats: &mut FrameStats) {
        if let Some(tiles) = self.cache.remove_region(coord) {
            self.reclaim_tiles(tiles);
        }
        self.region_signatures.remove(&coord);
        self.retire_organisms(coord);
        let keys: Vec<(RegionCoord, u16)> = self
            .in_flight
            .range((coord, 0)..=(coord, u16::MAX))
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            stats.jobs_cancelled += self.retire_in_flight(k);
        }
        if let Some(region) = self.regions.get_mut(&coord) {
            region.dirty_layers = 0;
            region.status = GenerationStatus::Unloaded;
        }
    }

    /// Forget one resident completely after it crosses `unload_radius`.
    fn drop_region(&mut self, coord: RegionCoord, stats: &mut FrameStats) {
        let old_pair = self.effective_terrain_pair(coord);
        self.park_region_fields(coord, stats);
        self.regions.remove(&coord);
        if self.effective_terrain_pair(coord) != old_pair {
            self.invalidate_terrain_consumers(coord, stats);
        }
    }

    /// Retire one region's near-field realization and recycle its allocation.
    /// Preserve-driven revision changes use this immediately, before a later
    /// realization pass can publish identities from the old epoch (ADR 0020).
    fn retire_organisms(&mut self, coord: RegionCoord) {
        if let Some(mut organisms) = self.organisms.remove(&coord) {
            organisms.clear();
            if self.organism_pool.len() < 256 {
                self.organism_pool.push(organisms);
            }
        }
        self.authoritative_organism_keys.remove(&coord);
        self.presentation_organism_keys.remove(&coord);
    }

    /// Sweep dependent-tracked caches against field-active consumers. Parked
    /// authority can compute expected keys but cannot consume macro/roster
    /// allocations, so it does not pin them (ADR 0023).
    fn sweep_dependent_caches(&mut self, stats: &mut FrameStats) {
        let active: BTreeSet<RegionCoord> = self
            .regions
            .iter()
            .filter(|(_, region)| Self::field_active(region))
            .map(|(&coord, _)| coord)
            .collect();
        self.macro_cache.evict_orphans(active.iter());
        let needed: BTreeSet<RegionCoord> = active.iter().map(|&c| macro_coord_for(c)).collect();
        let cancellation = self.cancellation;
        self.in_flight.retain(|(c, _), job| {
            let keep = c.level == 0 || needed.contains(c);
            if !keep && cancellation {
                job.cancel.store(true, Ordering::Relaxed);
                stats.jobs_cancelled += 1;
            }
            keep
        });
        // `park_region_fields` removes the parked region's signature set.
        let needed_signatures = self.required_roster_signatures();
        self.roster_cache.evict_unused(&needed_signatures);
    }

    /// The indispensable roster working set: the deterministic union of every
    /// field-active region's current or in-flight Ecology input signatures.
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

    /// Enforce derived byte-capacity targets. Field pressure parks active,
    /// non-exempt working sets farthest-first according to their full eventual
    /// payload reservation; it never removes authority (ADR 0023).
    fn enforce_capacity(&mut self, player: (f64, f64), stats: &mut FrameStats) -> bool {
        let mut parked_any = false;
        let payload = full_region_payload_bytes(self.cfg.field_resolution);
        let mut reserved = self.disposable_field_reservations(player) * payload;
        if reserved > self.cfg.max_field_cache_bytes {
            let mut order: Vec<(u64, RegionCoord)> = self
                .regions
                .iter()
                .filter(|(_, region)| Self::field_active(region))
                .filter(|(coord, _)| !self.preserve_contributors.contains_key(coord))
                .map(|(&coord, _)| (center_distance(coord, player).to_bits(), coord))
                .filter(|&(d, _)| f64::from_bits(d) > self.cfg.near_radius)
                .collect();
            // Farthest first, coord tiebreak for determinism.
            order.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            for (_, coord) in order {
                if reserved <= self.cfg.max_field_cache_bytes {
                    break;
                }
                self.park_region_fields(coord, stats);
                reserved = reserved.saturating_sub(payload);
                stats.evicted_for_capacity += 1;
                parked_any = true;
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
                    Self::field_active(region)
                        && region.dirty_layers & layer_bit(world_core::layer::LAYER_HYDROLOGY) != 0
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
                }
            }
        }

        // Roster target: repair the resident working set, then evict only
        // disposable entries in deterministic reverse-signature order. The
        // indispensable floor may exceed the configured target (ADR 0019).
        let protected = self.maintain_roster_working_set();
        self.roster_cache
            .evict_to_bytes(self.cfg.max_roster_cache_bytes, &protected);
        parked_any
    }

    /// Insert missing authority within `load_radius`, nearest-first and under
    /// `max_loads`, independently of field capacity. Then admit eligible
    /// parked working sets under the disposable full-payload target. A fresh
    /// region snaps `current = target`; reactivation never does (ADR 0023).
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
        let created = candidates.len().min(budget.max_loads);
        for &(_, coord) in candidates.iter().take(created) {
            let old_pair = self.fallback_terrain_pair(coord);
            let mut region = RegionState::new(coord);
            if let Some((_, sig)) = self.effective_preserve(coord) {
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
            // Authority is created parked. Admission below is the sole path
            // that allocates a field working set and marks its layers dirty.
            self.regions.insert(coord, region);
            if self.effective_terrain_pair(coord) != old_pair {
                self.invalidate_terrain_consumers(coord, stats);
            }
            stats.loaded += 1;
        }
        stats.deferred_loads = candidates.len().saturating_sub(created);
        self.admit_fields(player, field, anchors, bias);
    }

    /// Admit parked fields nearest-first. Near and contributor-covered
    /// residents are exemptions; ordinary residents reserve one complete
    /// eventual payload below the configured target.
    fn admit_fields(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) {
        let mut candidates: Vec<(u64, RegionCoord)> = self
            .regions
            .iter()
            .filter(|(_, region)| !Self::field_active(region))
            .filter(|(&coord, _)| {
                center_distance(coord, player) <= self.cfg.load_radius
                    || self.preserve_contributors.contains_key(&coord)
            })
            .map(|(&coord, _)| (center_distance(coord, player).to_bits(), coord))
            .collect();
        candidates.sort_unstable_by(|a, b| a.cmp(b).then_with(|| a.1.cmp(&b.1)));

        let payload = full_region_payload_bytes(self.cfg.field_resolution);
        let mut projected = self.disposable_field_reservations(player) * payload;
        for (distance_bits, coord) in candidates {
            let exempt = f64::from_bits(distance_bits) <= self.cfg.near_radius
                || self.preserve_contributors.contains_key(&coord);
            if !exempt && projected.saturating_add(payload) > self.cfg.max_field_cache_bytes {
                continue;
            }
            self.activate_region(coord, player, field, anchors, bias);
            if !exempt {
                projected = projected.saturating_add(payload);
            }
        }
    }

    /// Turn retained authority into a field-active generation epoch without
    /// resetting its realized history or revision.
    fn activate_region(
        &mut self,
        coord: RegionCoord,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) {
        let preserved = self.preserve_contributors.contains_key(&coord);
        let target = (!preserved).then(|| self.target_for(coord, field, anchors, bias));
        let region = self.regions.get_mut(&coord).expect("resident admission");
        if preserved {
            region.target = region.current;
            region.stability = 1.0;
        } else {
            region.target = target.expect("ordinary target");
            region.stability = stability_for(&self.cfg, center_distance(coord, player));
        }
        region.dirty_layers = all_layers_mask();
        region.status = GenerationStatus::Generating;
    }

    /// Refresh geometric stability for every authoritative region, then budget
    /// only steered target calculation. A steering change still refreshes all
    /// targets immediately; unchanged steering round-robins over every
    /// coordinate, parked authority included (ADR 0023).
    fn retarget(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        self.refresh_stability(player);

        let signature = steering_signature(anchors, bias);
        let steering_changed = signature != self.steer_signature;
        self.steer_signature = signature;

        let coords: Vec<RegionCoord> = self.regions.keys().copied().collect();
        if steering_changed || coords.len() <= budget.max_retarget_regions {
            for coord in coords {
                self.refresh_target(coord, field, anchors, bias);
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
            self.refresh_target(coord, field, anchors, bias);
            self.retarget_cursor = Some(coord);
            processed += 1;
        }
        stats.retarget_deferred = coords.len() - processed;
    }

    /// Refresh every resident's cheap distance-derived stability in ordered
    /// coordinate traversal. Preserves stay fully pinned and self-targeted.
    fn refresh_stability(&mut self, player: (f64, f64)) {
        for (&coord, region) in &mut self.regions {
            if self.preserve_contributors.contains_key(&coord) {
                region.stability = 1.0;
                region.target = region.current;
            } else {
                region.stability = stability_for(&self.cfg, center_distance(coord, player));
            }
        }
    }

    /// Refresh one authoritative region's steered target.
    fn refresh_target(
        &mut self,
        coord: RegionCoord,
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) {
        if self.preserve_contributors.contains_key(&coord) {
            // Preserved: pinned to its buckets; neither the distance ramp
            // nor steering moves it (phase-5-plan.md §7.5).
            let region = self.regions.get_mut(&coord).expect("resident");
            region.stability = 1.0;
            region.target = region.current;
            return;
        }
        let target = self.target_for(coord, field, anchors, bias);
        let region = self.regions.get_mut(&coord).expect("resident");
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
        let mut terrain_sources = Vec::new();
        for &(_, coord) in eligible.iter().take(budget.max_converge_regions) {
            let region = self.regions.get_mut(&coord).expect("resident");
            let mut dirtied = 0u32;
            if let Some(flipped) = region.converge(rate) {
                stats.converged += 1;
                if flipped != 0 {
                    dirtied = domain_dirty_mask(flipped);
                    region.dirty_layers |= dirtied;
                    if Self::field_active(region) && region.status == GenerationStatus::Ready {
                        region.status = GenerationStatus::Generating;
                    }
                    let slow_mask = (1 << PossibilityDomain::Planetary.index())
                        | (1 << PossibilityDomain::Geology.index());
                    if flipped & slow_mask != 0 {
                        terrain_sources.push(coord);
                    }
                }
            }
            // Bucket flips supersede any in-flight job of the dirtied layers:
            // its expected hash moved on while it flew (§6.2).
            stats.jobs_cancelled += self.cancel_in_flight(coord, dirtied);
        }
        terrain_sources.sort_unstable();
        terrain_sources.dedup();
        for source in terrain_sources {
            self.invalidate_terrain_consumers(source, stats);
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
            .iter()
            .filter(|(_, region)| Self::field_active(region))
            .map(|(&coord, _)| (center_distance(coord, player).to_bits(), coord))
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
        if !self.regions.get(&coord).is_some_and(Self::field_active) {
            return false;
        }
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
                        if Self::field_active(region) {
                            region.status = GenerationStatus::Generating;
                        }
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
        debug_assert_eq!(*field, self.field_recipe);
        let field = self.field_recipe;
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

    /// Field-active regions in the near window, in coordinate order.
    fn near_realization_coords(&self, player: (f64, f64)) -> BTreeSet<RegionCoord> {
        self.regions
            .iter()
            .filter(|(_, region)| Self::field_active(region))
            .map(|(&coord, _)| coord)
            .filter(|&coord| center_distance(coord, player) <= self.cfg.near_radius)
            .collect()
    }

    /// A region's current, recursively fresh Ecology key. Any dirty hint,
    /// pending job, missing tile, or key mismatch makes it unavailable.
    fn fresh_ecology_hash(&self, coord: RegionCoord) -> Option<u64> {
        let region = self.regions.get(&coord)?;
        let stored = self.cache.get(coord)?.layer_hash(LAYER_ECOLOGY)?;
        if region.dirty_layers & layer_bit(LAYER_ECOLOGY) != 0
            || self.in_flight.contains_key(&(coord, LAYER_ECOLOGY))
            || self.expected_layer_hash(coord, LAYER_ECOLOGY) != Some(stored)
        {
            return None;
        }
        Some(stored)
    }

    fn realization_rosters_complete(&self, coord: RegionCoord) -> bool {
        self.region_signatures
            .get(&coord)
            .is_some_and(|signatures| {
                signatures
                    .iter()
                    .all(|&signature| self.roster_cache.get(signature).is_some())
            })
    }

    /// Retire offscreen or stale presentation vectors and both currency keys
    /// before any gameplay read. Empty vectors count as valid completed
    /// realizations when their two keys are current (ADR 0024).
    fn retire_invalid_realizations(&mut self, near: &BTreeSet<RegionCoord>) {
        let mut tracked: BTreeSet<RegionCoord> = self.organisms.keys().copied().collect();
        tracked.extend(self.authoritative_organism_keys.keys().copied());
        tracked.extend(self.presentation_organism_keys.keys().copied());
        let stale: Vec<_> = tracked
            .into_iter()
            .filter(|&coord| {
                if !near.contains(&coord) || !self.organisms.contains_key(&coord) {
                    return true;
                }
                let Some(hash) = self.fresh_ecology_hash(coord) else {
                    return true;
                };
                !self.realization_rosters_complete(coord)
                    || self.authoritative_organism_keys.get(&coord) != Some(&hash)
                    || self
                        .presentation_organism_keys
                        .get(&coord)
                        .is_none_or(|&(presentation_hash, _)| presentation_hash != hash)
            })
            .collect();
        for coord in stale {
            self.retire_organisms(coord);
        }
    }

    fn realization_order(
        near: &BTreeSet<RegionCoord>,
        player: (f64, f64),
    ) -> Vec<(u64, RegionCoord)> {
        let mut order: Vec<_> = near
            .iter()
            .map(|&coord| (center_distance(coord, player).to_bits(), coord))
            .collect();
        order.sort_unstable();
        order
    }

    fn realize_slots(&mut self, coord: RegionCoord, slots: u16) -> Vec<Organism> {
        let region = &self.regions[&coord];
        let bias = GenomeBias {
            morphology: region.current.get(PossibilityDomain::Morphology),
            behavior: region.current.get(PossibilityDomain::Behavior),
            aesthetics: region.current.get(PossibilityDomain::Aesthetics),
        };
        let revision = region.revision;
        let mut organisms = self.organism_pool.pop().unwrap_or_default();
        realize_region_into(
            coord,
            self.cache.get(coord).expect("fresh ecology implies tiles"),
            &self.roster_cache,
            bias,
            revision,
            self.cfg.field_resolution,
            slots,
            &mut organisms,
        );
        organisms
    }

    fn publish_organisms(&mut self, coord: RegionCoord, organisms: Vec<Organism>) {
        if let Some(mut old) = self.organisms.insert(coord, organisms) {
            old.clear();
            if self.organism_pool.len() < 256 {
                self.organism_pool.push(old);
            }
        }
    }

    /// Fixed semantic realization pass: publish slot 0 for at most one nearest
    /// eligible whole region before capture/resonance can read it. Resource
    /// tiers and temporal budgets cannot change this admission (ADR 0024).
    fn realize_authoritative_near_window(&mut self, player: (f64, f64), stats: &mut FrameStats) {
        let near = self.near_realization_coords(player);
        self.retire_invalid_realizations(&near);
        for (_, coord) in Self::realization_order(&near, player) {
            let Some(l8_hash) = self.fresh_ecology_hash(coord) else {
                continue;
            };
            if self.authoritative_organism_keys.get(&coord) == Some(&l8_hash) {
                continue;
            }
            if !self.realization_rosters_complete(coord) {
                continue;
            }
            let organisms = self.realize_slots(coord, 1);
            stats.authoritative_organisms_realized += organisms.len();
            self.publish_organisms(coord, organisms);
            self.authoritative_organism_keys.insert(coord, l8_hash);
            self.presentation_organism_keys.insert(coord, (l8_hash, 1));
            break;
        }
        stats.organisms = self.organism_count();
    }

    /// Budgeted visual expansion pass. Only a current canonical vector may be
    /// replaced by the full tier density, and slot 0 is recomputed through the
    /// identical pure realization path (ADR 0024).
    fn expand_visual_near_window(
        &mut self,
        player: (f64, f64),
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        let near = self.near_realization_coords(player);
        self.retire_invalid_realizations(&near);
        let target_slots = self.cfg.organisms_per_cell.max(1);
        for (_, coord) in Self::realization_order(&near, player) {
            let Some(l8_hash) = self.fresh_ecology_hash(coord) else {
                continue;
            };
            if self.authoritative_organism_keys.get(&coord) != Some(&l8_hash) {
                continue;
            }
            if self.presentation_organism_keys.get(&coord) == Some(&(l8_hash, target_slots)) {
                continue;
            }
            if stats.organisms_realized >= budget.max_realize_organisms {
                break;
            }
            if !self.realization_rosters_complete(coord) {
                continue;
            }
            let organisms = self.realize_slots(coord, target_slots);
            stats.organisms_realized += organisms.len();
            self.publish_organisms(coord, organisms);
            self.presentation_organism_keys
                .insert(coord, (l8_hash, target_slots));
        }
        stats.organisms = self.organism_count();
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
        let terrain_halo = (layer == LAYER_TERRAIN).then(|| self.terrain_halo(coord));
        let dispatch_hash = terrain_halo.as_ref().map_or(expected, |halo| {
            terrain_dep_hash(
                coord,
                self.effective_revision(LAYER_TERRAIN),
                &halo.dependency_buckets(),
                self.cfg.field_resolution,
            )
        });
        debug_assert_eq!(dispatch_hash, expected, "owned Terrain halo/key skew");

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
            terrain_halo,
            tiles: input_tiles,
            biome,
            drainage,
            rosters,
            dep_hash: dispatch_hash,
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
                expected_hash: dispatch_hash,
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
mod preserve_tests {
    use super::*;
    use crate::budget::Budget;
    use crate::InlineExecutor;

    const PLAYER: (f64, f64) = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

    fn coord() -> RegionCoord {
        RegionCoord::new(0, 0)
    }

    fn config() -> StreamConfig {
        StreamConfig {
            near_radius: REGION_SIZE * 0.125,
            far_radius: REGION_SIZE * 0.25,
            load_radius: REGION_SIZE * 0.375,
            unload_radius: REGION_SIZE * 0.5,
            field_resolution: 2,
            organisms_per_cell: 4,
            ..StreamConfig::default()
        }
    }

    fn settle(map: &mut RegionMap) {
        for _ in 0..4 {
            map.update(
                PLAYER,
                0.0,
                &PossibilityField::default(),
                &[],
                &NO_BIAS,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
            let region = map.get(coord()).expect("center resident");
            if region.status == GenerationStatus::Ready
                && region.dirty_layers == 0
                && map.jobs_in_flight() == 0
                && map.organisms_in(coord()).is_some()
            {
                return;
            }
        }
        panic!("preserve fixture did not settle");
    }

    fn settled_map() -> RegionMap {
        let mut map = RegionMap::new(config());
        settle(&mut map);
        assert!(
            !map.organisms_in(coord())
                .expect("realized organisms")
                .is_empty(),
            "four slots keep the identity assertions non-vacuous"
        );
        map
    }

    fn changed_signature(
        base: PossibilitySignature,
        domain: PossibilityDomain,
    ) -> PossibilitySignature {
        let mut changed = base;
        let bucket = &mut changed.buckets[domain.index()];
        *bucket = if *bucket < 2048 { 4095 } else { 0 };
        changed
    }

    fn tile_keys(map: &RegionMap) -> Vec<(u16, Option<u64>)> {
        map.layer_diagnostics(coord())
            .expect("resident diagnostics")
            .into_iter()
            .map(|diagnostic| (diagnostic.layer, diagnostic.stored))
            .collect()
    }

    fn tile_content_hashes(map: &RegionMap) -> Vec<u64> {
        let tiles = map.cache.get(coord()).expect("resident tiles");
        let mut hashes: Vec<u64> = tiles
            .channels
            .iter()
            .flatten()
            .map(|tile| tile.content_hash())
            .collect();
        if let Some(tile) = &tiles.biome {
            hashes.push(tile.content_hash());
        }
        if let Some(tile) = &tiles.dominant {
            hashes.push(tile.content_hash());
        }
        hashes
    }

    #[test]
    fn lowest_content_id_wins_in_both_application_orders() {
        let low = PossibilitySignature::of(PossibilityVector::neutral());
        let high = changed_signature(low, PossibilityDomain::Climate);

        for order in [[(20, high), (10, low)], [(10, low), (20, high)]] {
            let mut map = RegionMap::new(config());
            for (id, signature) in order {
                map.apply_preserve_contribution(id, coord(), signature);
            }
            assert_eq!(map.effective_preserve(coord()), Some((10, low)));
            assert!(map.is_overridden(coord()));
        }
    }

    #[test]
    fn resident_batch_order_reconciles_once_to_identical_realized_state() {
        let mut forward = settled_map();
        let mut reverse = settled_map();
        let base = PossibilitySignature::of(forward.get(coord()).unwrap().current);
        let low_signature = changed_signature(base, PossibilityDomain::Aesthetics);
        let high_signature = changed_signature(base, PossibilityDomain::Climate);
        let old_revision = forward.get(coord()).unwrap().revision;
        assert_eq!(reverse.get(coord()).unwrap().revision, old_revision);

        forward.apply_preserve_contributions([
            (10, coord(), low_signature),
            (20, coord(), high_signature),
        ]);
        reverse.apply_preserve_contributions([
            (20, coord(), high_signature),
            (10, coord(), low_signature),
        ]);

        for map in [&forward, &reverse] {
            assert_eq!(map.effective_preserve(coord()), Some((10, low_signature)));
            assert_eq!(
                map.get(coord()).unwrap().current,
                low_signature.dequantize()
            );
            assert_eq!(
                map.get(coord()).unwrap().revision,
                old_revision.wrapping_add(1),
                "one synchronization batch is one material revision event"
            );
            assert!(map.organisms_in(coord()).is_none());
        }

        settle(&mut forward);
        settle(&mut reverse);
        assert_eq!(
            forward.get(coord()).unwrap().current,
            reverse.get(coord()).unwrap().current
        );
        assert_eq!(
            forward.get(coord()).unwrap().revision,
            reverse.get(coord()).unwrap().revision
        );
        assert_eq!(tile_keys(&forward), tile_keys(&reverse));
        assert_eq!(tile_content_hashes(&forward), tile_content_hashes(&reverse));
        assert_eq!(forward.organisms_in(coord()), reverse.organisms_in(coord()));
    }

    #[test]
    fn same_bucket_snap_bumps_revision_and_rebuilds_only_organisms() {
        let mut map = settled_map();
        let signature = PossibilitySignature::of(map.get(coord()).unwrap().current);
        let snapped = signature.dequantize();
        assert_ne!(
            map.get(coord()).unwrap().current,
            snapped,
            "fixture must begin away from at least one bucket center"
        );
        let old_revision = map.get(coord()).unwrap().revision;
        let old_tiles = tile_keys(&map);
        let old_organisms = map.organisms_in(coord()).unwrap().to_vec();
        let old_key = map.authoritative_organism_keys[&coord()];

        // A same-bucket normalization must not retire unrelated work.
        let cancel = Arc::new(AtomicBool::new(false));
        map.in_flight.insert(
            (coord(), LAYER_ECOLOGY),
            InFlightJob {
                id: u64::MAX,
                expected_hash: old_key,
                cancel: Arc::clone(&cancel),
            },
        );
        map.apply_preserve_contribution(10, coord(), signature);

        let region = map.get(coord()).unwrap();
        assert_eq!(region.current, snapped);
        assert_eq!(region.target, snapped);
        assert_eq!(region.revision, old_revision.wrapping_add(1));
        assert_eq!(region.dirty_layers, 0);
        assert_eq!(region.status, GenerationStatus::Ready);
        assert_eq!(tile_keys(&map), old_tiles);
        assert!(map.in_flight.contains_key(&(coord(), LAYER_ECOLOGY)));
        assert!(!cancel.load(Ordering::Relaxed));
        assert!(map.organisms_in(coord()).is_none());
        assert!(!map.authoritative_organism_keys.contains_key(&coord()));
        assert!(!map.presentation_organism_keys.contains_key(&coord()));

        // Remove the synthetic job; normal realization can now rebuild from
        // the unchanged fresh L8 key using the incremented revision.
        map.in_flight.remove(&(coord(), LAYER_ECOLOGY));
        settle(&mut map);
        assert_eq!(tile_keys(&map), old_tiles);
        assert_eq!(map.authoritative_organism_keys[&coord()], old_key);
        let new_organisms = map.organisms_in(coord()).unwrap();
        assert!(!new_organisms.is_empty());
        assert_ne!(new_organisms, old_organisms);
        assert!(
            new_organisms
                .iter()
                .all(|new| old_organisms.iter().all(|old| new.id != old.id)),
            "the new possibility revision must define a new feature-id epoch"
        );
    }

    #[test]
    fn bucket_changing_snap_cancels_exactly_the_declared_in_flight_closure() {
        let mut map = settled_map();
        let base = PossibilitySignature::of(map.get(coord()).unwrap().current);
        let changed = changed_signature(base, PossibilityDomain::Climate);
        let expected_dirty = domain_dirty_mask(1 << PossibilityDomain::Climate.index());
        let mut tokens = BTreeMap::new();
        for layer in 0..LAYER_COUNT {
            let token = Arc::new(AtomicBool::new(false));
            map.in_flight.insert(
                (coord(), layer),
                InFlightJob {
                    id: u64::from(layer) + 1,
                    expected_hash: u64::from(layer),
                    cancel: Arc::clone(&token),
                },
            );
            tokens.insert(layer, token);
        }

        map.apply_preserve_contribution(10, coord(), changed);

        assert_eq!(map.get(coord()).unwrap().dirty_layers, expected_dirty);
        for (layer, token) in tokens {
            let must_cancel = expected_dirty & layer_bit(layer) != 0;
            assert_eq!(
                token.load(Ordering::Relaxed),
                must_cancel,
                "layer {layer} cancellation token"
            );
            assert_eq!(
                map.in_flight.contains_key(&(coord(), layer)),
                !must_cancel,
                "layer {layer} in-flight bookkeeping"
            );
        }
    }

    #[test]
    fn session_restore_reconciles_an_overlap_batch_once() {
        let mut map = RegionMap::new(config());
        let snapshot_current = PossibilityVector::neutral();
        let snapshot = world_core::RegionSnapshotRecord {
            coord: coord(),
            current: snapshot_current.dims,
            stability: 0.25,
            revision: 41,
        };
        let session = world_core::SessionSnapshot {
            player: PLAYER,
            last_player: PLAYER,
            bias: NO_BIAS,
            transition_mode: false,
            anchors: Vec::new(),
            regions: vec![snapshot],
            sequence: 1,
        };
        crate::vault::apply_session_regions(&mut map, &session);
        let base = PossibilitySignature::of(snapshot_current);
        let winner = changed_signature(base, PossibilityDomain::Aesthetics);
        let nonwinner = changed_signature(base, PossibilityDomain::Climate);

        map.apply_preserve_contributions([(20, coord(), nonwinner), (10, coord(), winner)]);

        assert_eq!(map.effective_preserve(coord()), Some((10, winner)));
        let restored = map.get(coord()).unwrap();
        assert_eq!(restored.current, winner.dequantize());
        assert_eq!(restored.target, winner.dequantize());
        assert_eq!(restored.stability, 1.0);
        assert_eq!(restored.revision, 42);
        settle(&mut map);
        assert_eq!(map.get(coord()).unwrap().revision, 42);
        assert!(map.organisms_in(coord()).is_some());
    }

    #[test]
    fn winner_nonwinner_and_final_deletions_follow_the_transition_table() {
        let mut map = settled_map();
        let low_signature = PossibilitySignature::of(map.get(coord()).unwrap().current);
        let high_signature = changed_signature(low_signature, PossibilityDomain::Aesthetics);

        map.apply_preserve_contribution(20, coord(), high_signature);
        settle(&mut map);
        map.apply_preserve_contribution(10, coord(), low_signature);
        settle(&mut map);
        assert_eq!(map.effective_preserve(coord()), Some((10, low_signature)));

        // A non-winning deletion is completely inert outside bookkeeping.
        let before_nonwinner = map.get(coord()).unwrap().clone();
        let before_tiles = tile_keys(&map);
        let before_key = map.authoritative_organism_keys[&coord()];
        let before_organisms = map.organisms_in(coord()).unwrap().to_vec();
        assert!(map.remove_preserve_contribution(20, coord()));
        assert_eq!(map.effective_preserve(coord()), Some((10, low_signature)));
        let after_nonwinner = map.get(coord()).unwrap();
        assert_eq!(after_nonwinner.current, before_nonwinner.current);
        assert_eq!(after_nonwinner.target, before_nonwinner.target);
        assert_eq!(after_nonwinner.revision, before_nonwinner.revision);
        assert_eq!(after_nonwinner.dirty_layers, before_nonwinner.dirty_layers);
        assert_eq!(after_nonwinner.status, before_nonwinner.status);
        assert_eq!(tile_keys(&map), before_tiles);
        assert_eq!(map.authoritative_organism_keys[&coord()], before_key);
        assert_eq!(map.organisms_in(coord()).unwrap(), before_organisms);

        // Re-add then reveal the successor. Its changed Aesthetics bucket
        // dirties exactly the declared closure and advances the epoch once.
        map.apply_preserve_contribution(20, coord(), high_signature);
        let revision_before_winner_delete = map.get(coord()).unwrap().revision;
        assert!(map.remove_preserve_contribution(10, coord()));
        assert_eq!(map.effective_preserve(coord()), Some((20, high_signature)));
        let expected_dirty = domain_dirty_mask(1 << PossibilityDomain::Aesthetics.index());
        let changed = map.get(coord()).unwrap();
        assert_eq!(changed.current, high_signature.dequantize());
        assert_eq!(
            changed.revision,
            revision_before_winner_delete.wrapping_add(1)
        );
        assert_eq!(changed.dirty_layers, expected_dirty);
        assert_eq!(changed.status, GenerationStatus::Generating);
        assert!(map.organisms_in(coord()).is_none());
        assert!(!map.authoritative_organism_keys.contains_key(&coord()));
        assert!(!map.presentation_organism_keys.contains_key(&coord()));
        settle(&mut map);

        // Final deletion is a no-snap release: all resident derived state is
        // byte-for-byte untouched until a future travel-fueled convergence.
        let before_final = map.get(coord()).unwrap().clone();
        let before_final_tiles = tile_keys(&map);
        let before_final_key = map.authoritative_organism_keys[&coord()];
        let before_final_organisms = map.organisms_in(coord()).unwrap().to_vec();
        assert!(map.remove_preserve_contribution(20, coord()));
        assert_eq!(map.effective_preserve(coord()), None);
        assert!(!map.is_overridden(coord()));
        let after_final = map.get(coord()).unwrap();
        assert_eq!(after_final.current, before_final.current);
        assert_eq!(after_final.target, before_final.target);
        assert_eq!(after_final.revision, before_final.revision);
        assert_eq!(after_final.dirty_layers, before_final.dirty_layers);
        assert_eq!(after_final.status, before_final.status);
        assert_eq!(tile_keys(&map), before_final_tiles);
        assert_eq!(map.authoritative_organism_keys[&coord()], before_final_key);
        assert_eq!(map.organisms_in(coord()).unwrap(), before_final_organisms);
    }

    #[test]
    fn equal_signature_owner_change_updates_only_the_reported_owner() {
        let mut map = settled_map();
        let signature = PossibilitySignature::of(map.get(coord()).unwrap().current);
        map.apply_preserve_contribution(20, coord(), signature);
        settle(&mut map);
        let before = map.get(coord()).unwrap().clone();
        let before_tiles = tile_keys(&map);
        let before_key = map.authoritative_organism_keys[&coord()];
        let before_organisms = map.organisms_in(coord()).unwrap().to_vec();

        map.apply_preserve_contribution(10, coord(), signature);
        assert_eq!(map.effective_preserve(coord()), Some((10, signature)));
        let after = map.get(coord()).unwrap();
        assert_eq!(after.current, before.current);
        assert_eq!(after.target, before.target);
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.dirty_layers, before.dirty_layers);
        assert_eq!(after.status, before.status);
        assert_eq!(tile_keys(&map), before_tiles);
        assert_eq!(map.authoritative_organism_keys[&coord()], before_key);
        assert_eq!(map.organisms_in(coord()).unwrap(), before_organisms);
    }

    #[test]
    fn contributor_exempts_a_far_region_from_disposable_field_target() {
        let target = RegionCoord::new(1, 0);
        let mut cfg = config();
        cfg.near_radius = REGION_SIZE * 0.125;
        cfg.far_radius = REGION_SIZE * 1.25;
        cfg.load_radius = REGION_SIZE * 1.5;
        cfg.unload_radius = REGION_SIZE * 2.0;
        cfg.max_field_cache_bytes = 0;
        let mut map = RegionMap::new(cfg);
        let signature = PossibilitySignature::of(PossibilityVector::neutral());
        map.apply_preserve_contribution(20, target, signature);
        map.apply_preserve_contribution(10, target, signature);

        for _ in 0..3 {
            map.update(
                PLAYER,
                0.0,
                &PossibilityField::default(),
                &[],
                &NO_BIAS,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        assert_eq!(map.effective_preserve(target), Some((10, signature)));
        assert!(
            map.get(target).is_some(),
            "a contributor-covered non-near region must retain authority"
        );
        assert_ne!(
            map.get(target).unwrap().status,
            GenerationStatus::Unloaded,
            "a contributor-covered resident must be field-active"
        );
        assert!(map.cache.get(target).is_some());

        // Non-winner removal leaves the exemption and resident state intact.
        assert!(map.remove_preserve_contribution(20, target));
        map.update(
            PLAYER,
            0.0,
            &PossibilityField::default(),
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert!(map.get(target).is_some());
        assert_eq!(map.effective_preserve(target), Some((10, signature)));
    }

    #[test]
    fn parked_preserve_winners_and_release_keep_one_authoritative_epoch() {
        let mut map = settled_map();
        let mut parking_stats = FrameStats::default();
        map.park_region_fields(coord(), &mut parking_stats);
        assert_eq!(map.get(coord()).unwrap().status, GenerationStatus::Unloaded);
        assert!(map.cache.get(coord()).is_none());

        // Force a noncanonical value inside the same bucket, then apply that
        // bucket as a preserve while parked. This is a material revision and
        // organism epoch, but not a tile-key change.
        let base = PossibilitySignature::of(map.get(coord()).unwrap().current);
        let snapped = base.dequantize();
        let mut same_bucket = snapped;
        let climate = snapped.get(PossibilityDomain::Climate);
        same_bucket.set(
            PossibilityDomain::Climate,
            if climate < 1.0 {
                climate + 0.000001
            } else {
                climate - 0.000001
            },
        );
        assert_eq!(PossibilitySignature::of(same_bucket), base);
        {
            let region = map.regions.get_mut(&coord()).unwrap();
            region.current = same_bucket;
            region.target = same_bucket;
        }
        let normalization_revision = map.get(coord()).unwrap().revision;
        map.apply_preserve_contribution(30, coord(), base);
        let preserved_revision = normalization_revision + 1;
        assert_eq!(map.get(coord()).unwrap().current, snapped);
        assert_eq!(map.get(coord()).unwrap().revision, preserved_revision);
        assert_eq!(map.get(coord()).unwrap().dirty_layers, 0);
        assert_eq!(map.get(coord()).unwrap().status, GenerationStatus::Unloaded);

        // Contributor admission rebuilds every field without inventing another
        // regional epoch.
        settle(&mut map);
        assert_eq!(map.get(coord()).unwrap().revision, preserved_revision);
        assert_eq!(map.get(coord()).unwrap().current, snapped);
        assert!(map
            .layer_diagnostics(coord())
            .unwrap()
            .iter()
            .all(|diagnostic| diagnostic.stored == diagnostic.expected));
        map.park_region_fields(coord(), &mut parking_stats);
        assert!(map.remove_preserve_contribution(30, coord()));
        assert_eq!(map.get(coord()).unwrap().status, GenerationStatus::Unloaded);
        assert_eq!(map.get(coord()).unwrap().revision, preserved_revision);

        let high = changed_signature(base, PossibilityDomain::Climate);
        let low = changed_signature(base, PossibilityDomain::Aesthetics);
        let start_revision = map.get(coord()).unwrap().revision;

        map.apply_preserve_contribution(20, coord(), high);
        assert_eq!(map.get(coord()).unwrap().revision, start_revision + 1);
        assert_eq!(map.get(coord()).unwrap().status, GenerationStatus::Unloaded);
        map.apply_preserve_contribution(10, coord(), low);
        assert_eq!(map.get(coord()).unwrap().revision, start_revision + 2);
        assert_eq!(map.get(coord()).unwrap().status, GenerationStatus::Unloaded);
        assert!(map.remove_preserve_contribution(10, coord()));
        assert_eq!(map.get(coord()).unwrap().current, high.dequantize());
        assert_eq!(map.get(coord()).unwrap().revision, start_revision + 3);

        let before_release = map.get(coord()).unwrap().clone();
        assert!(map.remove_preserve_contribution(20, coord()));
        let after_release = map.get(coord()).unwrap();
        assert_eq!(after_release.current, before_release.current);
        assert_eq!(after_release.target, before_release.target);
        assert_eq!(after_release.revision, before_release.revision);
        assert_eq!(after_release.status, GenerationStatus::Unloaded);

        settle(&mut map);
        let reactivated = map.get(coord()).unwrap();
        assert_eq!(reactivated.revision, before_release.revision);
        assert_eq!(reactivated.current, before_release.current);
        assert_eq!(reactivated.status, GenerationStatus::Ready);
        assert!(map.cache.get(coord()).is_some());
    }

    #[test]
    fn session_snapshot_includes_parked_authority_and_restore_starts_parked() {
        use crate::{MemoryStorage, Vault};

        let mut map = settled_map();
        {
            let region = map.regions.get_mut(&coord()).unwrap();
            region.stability = 0.0;
            let current = region.current.get(PossibilityDomain::Ecology);
            region.target.set(
                PossibilityDomain::Ecology,
                if current < 0.5 { 1.0 } else { 0.0 },
            );
            assert!(region.converge(0.5).is_some());
            assert!(region.revision > 0);
        }
        let mut parking_stats = FrameStats::default();
        map.park_region_fields(coord(), &mut parking_stats);
        let before = map.get(coord()).unwrap().clone();
        assert_eq!(before.status, GenerationStatus::Unloaded);

        let mut vault = Vault::open(MemoryStorage::new()).expect("memory vault");
        vault
            .snapshot_session(&map, PLAYER, PLAYER, &NO_BIAS, false, &[])
            .expect("session sequence");
        let session = vault.session().expect("session").clone();
        assert_eq!(session.regions.len(), 1);

        let mut restored = RegionMap::new(config());
        crate::vault::apply_session_regions(&mut restored, &session);
        let parked = restored.get(coord()).expect("restored authority");
        assert_eq!(parked.status, GenerationStatus::Unloaded);
        assert_eq!(parked.current, before.current);
        assert_eq!(parked.stability.to_bits(), before.stability.to_bits());
        assert_eq!(parked.revision, before.revision);
        assert!(restored.cache.get(coord()).is_none());

        settle(&mut restored);
        let active = restored.get(coord()).unwrap();
        assert_eq!(active.current, before.current);
        assert_eq!(active.revision, before.revision);
        assert_eq!(active.status, GenerationStatus::Ready);
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
mod compatibility_tests {
    use super::*;
    use crate::budget::Budget;
    use crate::InlineExecutor;
    use world_core::{bound_target, domain_mask};

    const PLAYER: (f64, f64) = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

    fn config() -> StreamConfig {
        StreamConfig {
            near_radius: REGION_SIZE * 0.125,
            far_radius: REGION_SIZE * 0.25,
            load_radius: REGION_SIZE * 0.375,
            unload_radius: REGION_SIZE * 0.5,
            field_resolution: 2,
            ..StreamConfig::default()
        }
    }

    fn anchor(kind: AnchorKind, target: f32, strength: f32) -> Anchor {
        let mask = domain_mask(&[PossibilityDomain::Ecology]);
        Anchor {
            world_pos: PLAYER,
            target: bound_target(mask, target),
            mask,
            kind,
            strength,
            falloff_radius: REGION_SIZE * 2.0,
            source: AnchorSource::Manual,
        }
    }

    fn retargeted(anchors: &[Anchor]) -> RegionMap {
        let field = PossibilityField::default();
        let budget = Budget::unlimited();
        let mut map = RegionMap::new(config());
        // Establish the authoritative unsteered current first, then refresh its
        // final target with the exact effective slice scored below.
        map.update(
            PLAYER,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &budget,
            &InlineExecutor,
            false,
        );
        map.update(
            PLAYER,
            0.0,
            &field,
            anchors,
            &NO_BIAS,
            &budget,
            &InlineExecutor,
            false,
        );
        map
    }

    #[test]
    fn suppress_compatibility_scores_final_desire_not_literal_target() {
        let suppress = anchor(AnchorKind::Suppress, 1.0, 0.85);
        let mut map = retargeted(&[suppress]);
        let coord = RegionCoord::from_world(PLAYER.0, PLAYER.1);
        let final_target = map.regions[&coord].target;

        map.regions.get_mut(&coord).unwrap().current = final_target;
        let at_final = map.anchor_compatibility(PLAYER, &[suppress]);
        map.regions.get_mut(&coord).unwrap().current = suppress.target;
        let at_literal = map.anchor_compatibility(PLAYER, &[suppress]);
        assert_eq!(at_final.to_bits(), 1.0f32.to_bits());
        assert!(at_final > at_literal);
    }

    #[test]
    fn mixed_polarity_compatibility_is_exact_across_permutations() {
        let emphasize = anchor(AnchorKind::Emphasize, 0.95, 0.63);
        let suppress = anchor(AnchorKind::Suppress, 0.72, 0.41);
        let duplicate = Anchor {
            source: AnchorSource::River,
            ..emphasize
        };
        let permutations = [
            vec![emphasize, suppress, duplicate],
            vec![duplicate, emphasize, suppress],
            vec![suppress, duplicate, emphasize],
            vec![suppress, emphasize, duplicate],
        ];
        let mut expected = None;
        for anchors in permutations {
            let map = retargeted(&anchors);
            let coord = RegionCoord::from_world(PLAYER.0, PLAYER.1);
            let image = (
                map.regions[&coord].target.dims.map(f32::to_bits),
                anchor_influence_profile(&anchors, PLAYER).map(f32::to_bits),
                map.anchor_compatibility(PLAYER, &anchors).to_bits(),
                map.resonance_at(PLAYER, &anchors).strength.to_bits(),
            );
            if let Some(expected) = expected {
                assert_eq!(image, expected);
            } else {
                expected = Some(image);
            }
        }
    }

    #[test]
    fn compatibility_neutral_cases_and_center_evaluation_are_explicit() {
        let active = anchor(AnchorKind::Emphasize, 1.0, 0.8);
        let mut map = retargeted(&[active]);
        let coord = RegionCoord::from_world(PLAYER.0, PLAYER.1);
        assert_eq!(
            RegionMap::new(config()).anchor_compatibility(PLAYER, &[active]),
            1.0
        );
        assert_eq!(map.anchor_compatibility(PLAYER, &[]), 1.0);
        assert_eq!(
            map.anchor_compatibility(PLAYER, &[Anchor { mask: 0, ..active }]),
            1.0
        );

        // Ordinary near authority is pinned, but unlike a preserve it still
        // has an active final desire and therefore non-neutral disagreement.
        map.regions.get_mut(&coord).unwrap().current = PossibilityVector::neutral();
        assert_eq!(map.regions[&coord].stability, 1.0);
        assert!(map.anchor_compatibility(PLAYER, &[active]) < 1.0);

        let signature = PossibilitySignature::of(map.regions[&coord].current);
        map.apply_preserve_contribution(7, coord, signature);
        assert_eq!(map.anchor_compatibility(PLAYER, &[active]), 1.0);

        // The anchor reaches the queried player at a region corner, but not
        // the center where this region-level target/profile is defined.
        let corner = (0.0, 0.0);
        let corner_only = Anchor {
            world_pos: corner,
            falloff_radius: REGION_SIZE * 0.1,
            ..active
        };
        let mut center_map = RegionMap::new(config());
        let mut region = RegionState::new(RegionCoord::new(0, 0));
        region.target = bound_target(corner_only.mask, 1.0);
        center_map.regions.insert(region.coord, region);
        assert!(corner_only.influence(corner) > 0.0);
        assert_eq!(center_map.anchor_compatibility(corner, &[corner_only]), 1.0);
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use crate::budget::Budget;
    use crate::generate::{
        RegionTiles, CHANNEL_COUNT, CHANNEL_ELEVATION, CHANNEL_SLOPE, CHANNEL_SOIL_DEPTH,
    };
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
        // Canonical publication intentionally follows prerequisite readiness
        // on the next frame (ADR 0024).
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
        assert_eq!(map.regions.len(), 1, "fixture must load one region only");
        map
    }

    fn differing_extra_probe(map: &RegionMap, minimum_slot: u16) -> (Organism, Organism) {
        map.organisms()
            .filter(|organism| organism.slot >= minimum_slot)
            .find_map(|extra| {
                let coord = RegionCoord::from_world(extra.world_pos.0, extra.world_pos.1);
                let distance2 = |organism: &Organism| {
                    let dx = organism.world_pos.0 - extra.world_pos.0;
                    let dy = organism.world_pos.1 - extra.world_pos.1;
                    dx * dx + dy * dy
                };
                let canonical = map.authoritative_organisms_in(coord).min_by(|a, b| {
                    distance2(a)
                        .partial_cmp(&distance2(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })?;
                (canonical.species != extra.species).then_some((*extra, *canonical))
            })
            .expect("extra-slot probe must differ from its nearest canonical species")
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
        let mut config = tiny_config();
        config.field_resolution = 16;
        let mut map = RegionMap::new(config);
        settle_inline(&mut map);
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
        let old_organisms = map
            .organisms
            .get(&coord())
            .expect("near region realization")
            .clone();
        assert!(
            !old_organisms.is_empty(),
            "four realization slots should make the fixture non-vacuous"
        );
        let old_canonical: Vec<_> = old_organisms
            .iter()
            .copied()
            .filter(|organism| organism.slot == 0)
            .collect();
        assert!(!old_canonical.is_empty(), "canonical fixture population");
        let old_key = map.authoritative_organism_keys[&coord()];
        let other_realizations: Vec<_> = map
            .organisms
            .keys()
            .copied()
            .filter(|&candidate| candidate != coord())
            .collect();
        for candidate in other_realizations {
            map.retire_organisms(candidate);
        }

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

        // Losing a roster underneath a current-key vector fails closed before
        // resonance/capture: the vector and both currencies retire together.
        map.roster_cache = RosterCache::default();
        assert!(map.roster_cache.is_empty());
        assert_eq!(map.region_signatures[&coord()], required);

        let mut failed_stats = FrameStats::default();
        map.realize_authoritative_near_window(PLAYER, &mut failed_stats);
        assert_eq!(failed_stats.authoritative_organisms_realized, 0);
        assert_eq!(failed_stats.organisms_realized, 0);
        assert!(!map.organisms.contains_key(&coord()));
        assert!(!map.authoritative_organism_keys.contains_key(&coord()));
        assert!(!map.presentation_organism_keys.contains_key(&coord()));
        assert!(map
            .nearest_organism(coord(), old_canonical[0].world_pos)
            .is_none());
        assert!(map.resonance_at(PLAYER, &[]).nodes.is_empty());
        let capture = map
            .capture_at(
                old_canonical[0].world_pos,
                1 << PossibilityDomain::Morphology.index(),
                AnchorKind::Emphasize,
                1.0,
                REGION_SIZE,
            )
            .expect("resident region remains environmentally capturable");
        assert!(!matches!(capture.source, AnchorSource::Organism { .. }));

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
        map.realize_authoritative_near_window(PLAYER, &mut retry_stats);
        assert_eq!(map.authoritative_organism_keys[&coord()], old_key);
        assert_eq!(
            map.authoritative_organisms_in(coord())
                .copied()
                .collect::<Vec<_>>(),
            old_canonical,
            "same inputs and exact roster rebuild must realize identically"
        );
        let mut expansion_stats = FrameStats::default();
        map.expand_visual_near_window(PLAYER, &Budget::unlimited(), &mut expansion_stats);
        assert_eq!(map.organisms[&coord()], old_organisms);

        // Revision invalidation also retires authority synchronously. A newly
        // landed L8 cannot republish while its replacement rosters are absent.
        map.bump_layer_revision(LAYER_ECOLOGY);
        assert!(!map.organisms.contains_key(&coord()));
        assert!(!map.authoritative_organism_keys.contains_key(&coord()));
        assert!(!map.presentation_organism_keys.contains_key(&coord()));
        assert!(map.resonance_at(PLAYER, &[]).nodes.is_empty());

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
        let new_required = map
            .region_signatures
            .get(&coord())
            .expect("replacement ecology signatures")
            .clone();
        map.roster_cache = RosterCache::default();
        let mut blocked_stats = FrameStats::default();
        map.realize_authoritative_near_window(PLAYER, &mut blocked_stats);
        assert!(!map.organisms.contains_key(&coord()));
        assert!(map.resonance_at(PLAYER, &[]).nodes.is_empty());

        assert_eq!(map.maintain_roster_working_set(), new_required);
        let mut repaired_stats = FrameStats::default();
        map.realize_authoritative_near_window(PLAYER, &mut repaired_stats);
        assert_eq!(map.authoritative_organism_keys[&coord()], new_key);
        assert_eq!(
            map.authoritative_organisms_in(coord())
                .copied()
                .collect::<Vec<_>>(),
            old_canonical,
            "algorithm revision changes provenance, not realization math"
        );
    }

    #[test]
    fn canonical_admission_is_fixed_while_visual_expansion_obeys_budget() {
        let mut config = tiny_config();
        config.field_resolution = 16;
        let mut blocked = RegionMap::new(config);
        let mut expanded = RegionMap::new(config);
        let no_visual = Budget {
            max_realize_organisms: 0,
            ..Budget::unlimited()
        };

        // First frame settles prerequisites after the pre-resonance canonical
        // pass, so neither map can publish early through visual capacity.
        for (map, budget) in [
            (&mut blocked, &no_visual),
            (&mut expanded, &Budget::unlimited()),
        ] {
            let stats = map.update(
                PLAYER,
                0.0,
                &field(),
                &[],
                &NO_BIAS,
                budget,
                &InlineExecutor,
                false,
            );
            assert_eq!(stats.authoritative_organisms_realized, 0);
            assert!(map.authoritative_organism_keys.is_empty());
        }

        let blocked_stats = blocked.update(
            PLAYER,
            0.0,
            &field(),
            &[],
            &NO_BIAS,
            &no_visual,
            &InlineExecutor,
            false,
        );
        let expanded_stats = expanded.update(
            PLAYER,
            0.0,
            &field(),
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert_eq!(
            blocked_stats.authoritative_organisms_realized,
            expanded_stats.authoritative_organisms_realized
        );
        assert_eq!(blocked_stats.organisms_realized, 0);
        assert!(expanded_stats.organisms_realized > 0);
        assert_eq!(
            blocked.authoritative_organism_keys,
            expanded.authoritative_organism_keys
        );
        assert_eq!(
            blocked
                .authoritative_organisms()
                .copied()
                .collect::<Vec<_>>(),
            expanded
                .authoritative_organisms()
                .copied()
                .collect::<Vec<_>>()
        );
        assert!(expanded.organism_count() > blocked.organism_count());
        assert_eq!(
            blocked_stats.resonance_strength.to_bits(),
            expanded_stats.resonance_strength.to_bits()
        );
        let (extra, canonical) = differing_extra_probe(&expanded, 2);
        let mask = domain_mask(&[
            PossibilityDomain::Morphology,
            PossibilityDomain::Behavior,
            PossibilityDomain::Aesthetics,
            PossibilityDomain::Ecology,
        ]);
        let blocked_capture = blocked
            .capture_at(
                extra.world_pos,
                mask,
                AnchorKind::Emphasize,
                0.8,
                REGION_SIZE,
            )
            .expect("canonical capture under visual backpressure");
        let expanded_capture = expanded
            .capture_at(
                extra.world_pos,
                mask,
                AnchorKind::Emphasize,
                0.8,
                REGION_SIZE,
            )
            .expect("canonical capture after visual expansion");
        assert_eq!(blocked_capture, expanded_capture);
        assert!(matches!(
            blocked_capture.source,
            AnchorSource::Organism { species } if species == canonical.species
        ));
        assert_ne!(extra.species, canonical.species);
    }

    #[test]
    fn density_changes_only_presentation_not_capture_or_resonance() {
        let settled_density = |slots| {
            let mut config = tiny_config();
            config.field_resolution = 24;
            config.organisms_per_cell = slots;
            let mut map = RegionMap::new(config);
            settle_inline(&mut map);
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
            map
        };
        let one = settled_density(1);
        let four = settled_density(4);
        assert_eq!(
            one.authoritative_organisms().copied().collect::<Vec<_>>(),
            four.authoritative_organisms().copied().collect::<Vec<_>>()
        );
        assert!(four.organism_count() > one.organism_count());

        let (extra, canonical) = differing_extra_probe(&four, 2);
        let mask = domain_mask(&[
            PossibilityDomain::Morphology,
            PossibilityDomain::Behavior,
            PossibilityDomain::Aesthetics,
            PossibilityDomain::Ecology,
        ]);
        let low_capture = one
            .capture_at(
                extra.world_pos,
                mask,
                AnchorKind::Emphasize,
                0.8,
                REGION_SIZE,
            )
            .expect("density-one canonical capture");
        let high_capture = four
            .capture_at(
                extra.world_pos,
                mask,
                AnchorKind::Emphasize,
                0.8,
                REGION_SIZE,
            )
            .expect("density-four canonical capture");
        assert_eq!(low_capture, high_capture);
        assert!(matches!(
            high_capture.source,
            AnchorSource::Organism { species } if species == canonical.species
        ));
        assert_ne!(extra.species, canonical.species);

        let low = one.resonance_at(PLAYER, &[]);
        let high = four.resonance_at(PLAYER, &[]);
        assert_eq!(low.strength.to_bits(), high.strength.to_bits());
        assert_eq!(
            low.anchor_compatibility.to_bits(),
            high.anchor_compatibility.to_bits()
        );
        assert_eq!(low.nodes, high.nodes);
    }

    #[test]
    fn canonical_resonance_uses_the_fixed_semantic_ceiling() {
        let mut config = tiny_config();
        config.field_resolution = 32;
        config.organisms_per_cell = 4;
        config.near_radius = 0.75 * REGION_SIZE;
        config.far_radius = 0.9 * REGION_SIZE;
        config.load_radius = 0.8 * REGION_SIZE;
        config.unload_radius = 1.1 * REGION_SIZE;
        let mut map = RegionMap::new(config);
        settle_inline(&mut map);
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
        let radius2 = map.config().near_radius * map.config().near_radius;
        let mut expected: Vec<(u64, ResonanceNode)> = map
            .authoritative_organisms()
            .filter_map(|organism| {
                let dx = organism.world_pos.0 - PLAYER.0;
                let dy = organism.world_pos.1 - PLAYER.1;
                let distance2 = dx * dx + dy * dy;
                (distance2 <= radius2).then_some((
                    distance2.to_bits(),
                    ResonanceNode {
                        world_pos: organism.world_pos,
                        species: organism.species,
                        distance: distance2.sqrt(),
                    },
                ))
            })
            .collect();
        expected.sort_unstable_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.species.cmp(&b.1.species))
                .then_with(|| a.1.world_pos.0.to_bits().cmp(&b.1.world_pos.0.to_bits()))
                .then_with(|| a.1.world_pos.1.to_bits().cmp(&b.1.world_pos.1.to_bits()))
        });
        assert!(expected.len() > MAX_RESONANCE_NODES);
        let expected: Vec<_> = expected
            .into_iter()
            .take(MAX_RESONANCE_NODES)
            .map(|(_, node)| node)
            .collect();
        assert_eq!(map.resonance_at(PLAYER, &[]).nodes, expected);
    }

    #[test]
    fn parking_retains_history_while_radius_drop_forgets_authority() {
        let mut map = settled_map();
        {
            let region = map.regions.get_mut(&coord()).unwrap();
            region.stability = 0.0;
            let current = region.current.get(PossibilityDomain::Ecology);
            region.target.set(PossibilityDomain::Ecology, 1.0 - current);
            assert!(region.converge(0.5).is_some());
            region.stability = 0.375;
        }
        let before = map.regions[&coord()].clone();
        let mut stats = FrameStats::default();
        map.park_region_fields(coord(), &mut stats);

        let parked = &map.regions[&coord()];
        assert_eq!(parked.current, before.current);
        assert_eq!(parked.target, before.target);
        assert_eq!(parked.stability.to_bits(), before.stability.to_bits());
        assert_eq!(parked.revision, before.revision);
        assert_eq!(parked.status, GenerationStatus::Unloaded);
        assert_eq!(parked.dirty_layers, 0);
        assert!(map.cache.get(coord()).is_none());
        assert!(!map.region_signatures.contains_key(&coord()));
        assert!(!map.organisms.contains_key(&coord()));
        assert!(!map.authoritative_organism_keys.contains_key(&coord()));
        assert!(!map.presentation_organism_keys.contains_key(&coord()));

        map.sweep_dependent_caches(&mut stats);
        assert!(map.macro_cache.is_empty());
        assert!(map.roster_cache.is_empty());
        map.drop_region(coord(), &mut stats);
        assert!(!map.regions.contains_key(&coord()));
    }

    #[test]
    fn parked_region_rejects_late_results_with_and_without_cancellation() {
        for cancellation in [false, true] {
            let mut map = settled_map();
            map.set_cancellation_enabled(cancellation);
            map.bump_layer_revision(LAYER_TERRAIN);
            let executor = ManualExecutor::default();
            let expected = map
                .expected_layer_hash(coord(), LAYER_TERRAIN)
                .expect("terrain key");
            map.submit_layer(
                coord(),
                LAYER_TERRAIN,
                expected,
                TaskPriority::Critical,
                &executor,
            );
            assert_eq!(executor.len(), 1);
            assert_eq!(map.jobs_in_flight(), 1);

            let mut parking_stats = FrameStats::default();
            map.park_region_fields(coord(), &mut parking_stats);
            assert_eq!(
                parking_stats.jobs_cancelled,
                usize::from(cancellation),
                "token count must reflect the cancellation mode"
            );
            assert_eq!(map.jobs_in_flight(), 0);
            assert_eq!(map.regions[&coord()].status, GenerationStatus::Unloaded);
            assert!(map.cache.get(coord()).is_none());

            executor.run_next();
            let mut integration_stats = FrameStats::default();
            map.integrate_finished(&mut integration_stats);
            assert_eq!(
                integration_stats.results_dropped,
                usize::from(!cancellation),
                "cancellation-off work runs and is rejected; cancellation-on work is a no-op"
            );
            assert_eq!(map.regions[&coord()].status, GenerationStatus::Unloaded);
            assert!(map.cache.get(coord()).is_none());
        }
    }

    #[test]
    fn unchanged_steering_round_robin_refreshes_parked_targets() {
        let player = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
        let cfg = StreamConfig {
            near_radius: 0.1 * REGION_SIZE,
            far_radius: 1.5 * REGION_SIZE,
            load_radius: 1.1 * REGION_SIZE,
            unload_radius: 2.5 * REGION_SIZE,
            field_resolution: 2,
            max_field_cache_bytes: 0,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let mut map = RegionMap::new(cfg);
        map.update(
            player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        let expected: BTreeMap<_, _> = map
            .regions
            .keys()
            .map(|&coord| (coord, map.target_for(coord, &field, &[], &NO_BIAS)))
            .collect();
        let parked: Vec<_> = map
            .regions
            .iter()
            .filter(|(_, region)| region.status == GenerationStatus::Unloaded)
            .map(|(&coord, _)| coord)
            .collect();
        assert!(!parked.is_empty());
        for (&coord, region) in &mut map.regions {
            let mut stale = expected[&coord];
            let value = stale.get(PossibilityDomain::Behavior);
            stale.set(
                PossibilityDomain::Behavior,
                if value < 0.5 { 1.0 } else { 0.0 },
            );
            assert_ne!(stale, expected[&coord]);
            region.target = stale;
        }

        let budget = Budget {
            max_retarget_regions: 1,
            max_regen_cost: 0,
            ..Budget::unlimited()
        };
        let count = map.regions.len();
        for _ in 0..count {
            let stats = map.update(
                player,
                0.0,
                &field,
                &[],
                &NO_BIAS,
                &budget,
                &InlineExecutor,
                false,
            );
            assert_eq!(stats.retarget_deferred, count - 1);
        }
        for (&coord, target) in &expected {
            assert_eq!(map.regions[&coord].target, *target);
        }
        for coord in parked {
            assert_eq!(map.regions[&coord].status, GenerationStatus::Unloaded);
        }
    }

    #[test]
    fn field_recipe_change_refreshes_all_targets_before_amortized_work() {
        let player = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
        let cfg = StreamConfig {
            near_radius: 0.1 * REGION_SIZE,
            far_radius: 1.5 * REGION_SIZE,
            load_radius: 1.1 * REGION_SIZE,
            unload_radius: 2.5 * REGION_SIZE,
            field_resolution: 2,
            max_field_cache_bytes: 0,
            ..StreamConfig::default()
        };
        let mut map = RegionMap::new(cfg);
        map.update(
            player,
            0.0,
            &PossibilityField::default(),
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert!(map
            .regions
            .values()
            .any(|region| region.status == GenerationStatus::Unloaded));
        let old_macro = map.expected_macro_hash(macro_coord_for(coord()));
        let changed_field = PossibilityField::new(7);
        let expected: BTreeMap<_, _> = map
            .regions
            .keys()
            .map(|&coord| (coord, map.target_for(coord, &changed_field, &[], &NO_BIAS)))
            .collect();
        let budget = Budget {
            max_loads: 0,
            max_retarget_regions: 0,
            max_regen_cost: 0,
            ..Budget::unlimited()
        };
        map.update(
            player,
            0.0,
            &changed_field,
            &[],
            &NO_BIAS,
            &budget,
            &InlineExecutor,
            false,
        );

        assert_eq!(map.field_recipe, changed_field);
        assert_ne!(map.expected_macro_hash(macro_coord_for(coord())), old_macro);
        for (coord, target) in expected {
            assert_eq!(map.regions[&coord].target, target);
        }
    }

    fn ordinary_divergent_map(reverse_insertion: bool) -> RegionMap {
        let mut map = RegionMap::new(StreamConfig {
            near_radius: REGION_SIZE * 0.25,
            far_radius: REGION_SIZE * 2.0,
            load_radius: REGION_SIZE * 2.0,
            unload_radius: REGION_SIZE * 3.0,
            field_resolution: 8,
            ..StreamConfig::default()
        });
        let left = RegionCoord::new(0, 0);
        let right = RegionCoord::new(1, 0);
        let make = |coord: RegionCoord, planetary: f32, geology: f32| {
            let mut region = RegionState::new(coord);
            region.current = project_plausible(field().sample(coord)).requantized();
            region.current.set(PossibilityDomain::Planetary, planetary);
            region.current.set(PossibilityDomain::Geology, geology);
            region.target = region.current;
            region.stability = 0.5;
            region.dirty_layers = all_layers_mask();
            region.status = GenerationStatus::Generating;
            region
        };
        let left_region = make(left, 0.08, 0.92);
        let right_region = make(right, 0.91, 0.12);
        if reverse_insertion {
            map.regions.insert(right, right_region);
            map.regions.insert(left, left_region);
        } else {
            map.regions.insert(left, left_region);
            map.regions.insert(right, right_region);
        }

        for _ in 0..12 {
            let mut stats = FrameStats::default();
            map.dispatch_regen(
                PLAYER,
                &field(),
                &Budget::unlimited(),
                &InlineExecutor,
                &mut stats,
            );
            map.integrate_finished(&mut stats);
            if [left, right].iter().all(|coord| {
                map.regions[coord].dirty_layers == 0
                    && map.regions[coord].status == GenerationStatus::Ready
            }) && map.in_flight.is_empty()
            {
                return map;
            }
        }
        panic!(
            "ordinary divergent fixture did not settle: left={:?}, right={:?}",
            map.layer_diagnostics(left),
            map.layer_diagnostics(right)
        );
    }

    fn tile_image_at(map: &RegionMap, coord: RegionCoord) -> Vec<(u64, u64, Vec<u32>)> {
        map.cache
            .get(coord)
            .expect("settled ordinary tiles")
            .channels
            .iter()
            .map(|tile| {
                let tile = tile.as_ref().expect("every channel settled");
                (
                    tile.dep_hash,
                    tile.content_hash(),
                    tile.samples().iter().map(|value| value.to_bits()).collect(),
                )
            })
            .collect()
    }

    fn canonical_cell_world(coord: i32, resolution: u16, cell: i32) -> f64 {
        let global = i64::from(coord) * i64::from(resolution) + i64::from(cell);
        (global as f64 + 0.5) * (REGION_SIZE / f64::from(resolution))
    }

    fn halo_elevation(
        coord: RegionCoord,
        halo: &TerrainPossibilityHalo,
        resolution: u16,
        cx: i32,
        cy: i32,
    ) -> f32 {
        world_core::elevation(
            canonical_cell_world(coord.x, resolution, cx),
            canonical_cell_world(coord.y, resolution, cy),
            &halo.sample_cell(resolution, cx, cy),
        )
    }

    #[test]
    fn ordinary_divergent_history_seam_is_exact_through_biome_and_parking() {
        let left = RegionCoord::new(0, 0);
        let right = RegionCoord::new(1, 0);
        let mut forward = ordinary_divergent_map(false);
        let reverse = ordinary_divergent_map(true);
        assert!(!forward.is_overridden(left) && !forward.is_overridden(right));

        for coord in [left, right] {
            assert_eq!(
                tile_image_at(&forward, coord),
                tile_image_at(&reverse, coord)
            );
            assert_eq!(
                forward
                    .cache
                    .get(coord)
                    .unwrap()
                    .biome
                    .as_ref()
                    .unwrap()
                    .samples(),
                reverse
                    .cache
                    .get(coord)
                    .unwrap()
                    .biome
                    .as_ref()
                    .unwrap()
                    .samples(),
            );
        }

        let left_halo = forward.terrain_halo(left);
        let right_halo = forward.terrain_halo(right);
        let left_pair = forward.effective_terrain_pair(left);
        let right_pair = forward.effective_terrain_pair(right);
        assert_ne!(left_pair, right_pair, "fixture histories must be divergent");
        assert_eq!(left_halo.buckets_at(right), Some(right_pair));
        assert_eq!(right_halo.buckets_at(left), Some(left_pair));

        let resolution = 8;
        let cy = 4;
        let shared_left = left_halo.sample_cell(resolution, i32::from(resolution), cy);
        let shared_right = right_halo.sample_cell(resolution, 0, cy);
        assert_eq!(
            shared_left.dims.map(f32::to_bits),
            shared_right.dims.map(f32::to_bits)
        );
        let shared_elevation =
            halo_elevation(left, &left_halo, resolution, i32::from(resolution), cy);
        let right_elevation = forward.cache.channel(right, CHANNEL_ELEVATION).unwrap();
        assert_eq!(
            shared_elevation.to_bits(),
            right_elevation.get(0, cy as u16).to_bits()
        );

        let cx = i32::from(resolution) - 1;
        let step = (REGION_SIZE / f64::from(resolution)) as f32;
        let dx = (halo_elevation(left, &left_halo, resolution, cx + 1, cy)
            - halo_elevation(left, &left_halo, resolution, cx - 1, cy))
            / (2.0 * step);
        let dy = (halo_elevation(left, &left_halo, resolution, cx, cy + 1)
            - halo_elevation(left, &left_halo, resolution, cx, cy - 1))
            / (2.0 * step);
        let expected_slope = (dx * dx + dy * dy).sqrt();
        let tiles = forward.cache.get(left).unwrap();
        let elevation = tiles.channels[CHANNEL_ELEVATION].as_ref().unwrap();
        let slope = tiles.channels[CHANNEL_SLOPE].as_ref().unwrap();
        assert_eq!(
            slope.get(cx as u16, cy as u16).to_bits(),
            expected_slope.to_bits()
        );
        let old_dx =
            (elevation.get(cx as u16, cy as u16) - elevation.get(cx as u16 - 1, cy as u16)) / step;
        assert_ne!(
            expected_slope.to_bits(),
            (old_dx * old_dx + dy * dy).sqrt().to_bits(),
            "fixture must detect the old one-sided edge derivative"
        );

        let x = canonical_cell_world(left.x, resolution, cx);
        let y = canonical_cell_world(left.y, resolution, cy);
        let current = forward.regions[&left].current;
        let climate = Climate {
            temperature: tiles.channels[CHANNEL_TEMPERATURE]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16),
            moisture: tiles.channels[CHANNEL_MOISTURE]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16),
        };
        let drainage = forward
            .macro_cache
            .get(macro_coord_for(left))
            .expect("drainage settled");
        let hydrology_p = PossibilityVector::from_quantized(
            layer_decl(world_core::layer::LAYER_HYDROLOGY).domains,
            &current.quantized_domains(layer_decl(world_core::layer::LAYER_HYDROLOGY).domains),
        );
        let expected_hydrology = world_core::hydrology(
            elevation.get(cx as u16, cy as u16),
            expected_slope,
            drainage.accum_bilinear(x, y),
            &climate,
            hydrology_p.get(PossibilityDomain::Hydrology),
            hydrology_p.get(PossibilityDomain::Planetary),
        );
        assert_eq!(
            tiles.channels[CHANNEL_RIVER]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16)
                .to_bits(),
            expected_hydrology.river.to_bits()
        );
        assert_eq!(
            tiles.channels[CHANNEL_WETNESS]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16)
                .to_bits(),
            expected_hydrology.wetness.to_bits()
        );
        let geology_p = PossibilityVector::from_quantized(
            layer_decl(world_core::layer::LAYER_GEOLOGY).domains,
            &current.quantized_domains(layer_decl(world_core::layer::LAYER_GEOLOGY).domains),
        );
        let geology = world_core::geology(x, y, geology_p.get(PossibilityDomain::Geology));
        assert_eq!(
            geology.hardness.to_bits(),
            tiles.channels[CHANNEL_HARDNESS]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16)
                .to_bits()
        );
        let expected_soils = world_core::soils(
            elevation.get(cx as u16, cy as u16),
            expected_slope,
            &geology,
            &climate,
            &expected_hydrology,
        );
        assert_eq!(
            tiles.channels[CHANNEL_SOIL_DEPTH]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16)
                .to_bits(),
            expected_soils.depth.to_bits()
        );
        assert_eq!(
            tiles.channels[CHANNEL_FERTILITY]
                .as_ref()
                .unwrap()
                .get(cx as u16, cy as u16)
                .to_bits(),
            expected_soils.fertility.to_bits()
        );
        let expected_biome = world_core::classify(
            elevation.get(cx as u16, cy as u16),
            &climate,
            &expected_hydrology,
            &expected_soils,
        );
        assert_eq!(
            tiles.biome.as_ref().unwrap().get(cx as u16, cy as u16),
            expected_biome.id()
        );

        let left_before_parking = tile_image_at(&forward, left);
        let left_key_before = forward.expected_layer_hash(left, LAYER_TERRAIN);
        forward.park_region_fields(right, &mut FrameStats::default());
        assert_eq!(forward.regions[&right].status, GenerationStatus::Unloaded);
        assert_eq!(forward.effective_terrain_pair(right), right_pair);
        assert_eq!(
            forward.expected_layer_hash(left, LAYER_TERRAIN),
            left_key_before
        );
        assert_eq!(tile_image_at(&forward, left), left_before_parking);
    }

    #[test]
    fn queued_old_neighbor_halo_never_publishes_with_or_without_cancellation() {
        for cancellation in [false, true] {
            let left = RegionCoord::new(0, 0);
            let right = RegionCoord::new(1, 0);
            let mut map = RegionMap::new(StreamConfig {
                field_resolution: 8,
                ..tiny_config()
            });
            map.cancellation = cancellation;
            for coord in [left, right] {
                let mut region = RegionState::new(coord);
                region.current = project_plausible(field().sample(coord)).requantized();
                region.target = region.current;
                region.status = GenerationStatus::Generating;
                region.dirty_layers = layer_bit(LAYER_TERRAIN);
                map.regions.insert(coord, region);
            }
            let executor = ManualExecutor::default();
            let old_hash = map.expected_layer_hash(left, LAYER_TERRAIN).unwrap();
            map.submit_layer(
                left,
                LAYER_TERRAIN,
                old_hash,
                TaskPriority::Critical,
                &executor,
            );
            assert_eq!(executor.len(), 1);

            map.regions
                .get_mut(&right)
                .unwrap()
                .current
                .set(PossibilityDomain::Geology, 0.99);
            let mut stats = FrameStats::default();
            map.invalidate_terrain_consumers(right, &mut stats);
            let corrected_hash = map.expected_layer_hash(left, LAYER_TERRAIN).unwrap();
            assert_ne!(corrected_hash, old_hash);
            assert!(!map.in_flight.contains_key(&(left, LAYER_TERRAIN)));

            executor.run_next();
            map.integrate_finished(&mut stats);
            assert!(map.cache.get(left).is_none());
            assert_eq!(stats.results_dropped, usize::from(!cancellation));

            map.submit_layer(
                left,
                LAYER_TERRAIN,
                corrected_hash,
                TaskPriority::Critical,
                &executor,
            );
            executor.run_next();
            map.integrate_finished(&mut stats);
            assert_eq!(
                map.cache
                    .get(left)
                    .and_then(|tiles| tiles.layer_hash(LAYER_TERRAIN)),
                Some(corrected_hash)
            );
        }
    }

    fn clean_active_region(coord: RegionCoord, current: PossibilityVector) -> RegionState {
        let mut region = RegionState::new(coord);
        region.current = current;
        region.target = current;
        region.status = GenerationStatus::Ready;
        region.dirty_layers = 0;
        region
    }

    #[test]
    fn neighbor_halo_lifecycle_paths_notify_only_material_pg_changes() {
        let field = field();
        let neighbor = RegionCoord::new(0, 0);
        let source = RegionCoord::new(1, 0);

        // Actual authority insertion replaces fallback when steering makes the
        // loaded current P/G differ; radius-drop restoration returns to the
        // exact fallback and fans out through the same production helper.
        let mut map = RegionMap::new(StreamConfig {
            near_radius: REGION_SIZE * 0.05,
            far_radius: REGION_SIZE * 0.1,
            load_radius: REGION_SIZE * 0.1,
            unload_radius: REGION_SIZE * 4.0,
            field_resolution: 8,
            ..StreamConfig::default()
        });
        map.regions.insert(
            neighbor,
            clean_active_region(
                neighbor,
                project_plausible(field.sample(neighbor)).requantized(),
            ),
        );
        let fallback_key = map.expected_layer_hash(neighbor, LAYER_TERRAIN).unwrap();
        let mut bias = NO_BIAS;
        bias[PossibilityDomain::Planetary.index()] = 0.4;
        bias[PossibilityDomain::Geology.index()] = -0.4;
        let mut stats = FrameStats::default();
        map.load(
            (
                (f64::from(source.x) + 0.5) * REGION_SIZE,
                (f64::from(source.y) + 0.5) * REGION_SIZE,
            ),
            &field,
            &[],
            &bias,
            &Budget {
                max_loads: 1,
                ..Budget::unlimited()
            },
            &mut stats,
        );
        assert!(map.regions.contains_key(&source));
        let authority_key = map.expected_layer_hash(neighbor, LAYER_TERRAIN).unwrap();
        assert_ne!(authority_key, fallback_key);
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );
        map.regions.get_mut(&neighbor).unwrap().dirty_layers = 0;
        map.regions.get_mut(&neighbor).unwrap().status = GenerationStatus::Ready;
        map.drop_region(source, &mut stats);
        assert_eq!(
            map.expected_layer_hash(neighbor, LAYER_TERRAIN),
            Some(fallback_key)
        );
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );

        // Preserve winner P/G snaps fan out. A later winner which changes only
        // Climate while retaining the same P/G buckets leaves the neighbor's
        // Terrain key and work untouched.
        let mut map = RegionMap::new(tiny_config());
        let base = project_plausible(field.sample(source)).requantized();
        map.regions
            .insert(neighbor, clean_active_region(neighbor, base));
        map.regions
            .insert(source, clean_active_region(source, base));
        let mut changed = PossibilitySignature::of(base);
        changed.buckets[PossibilityDomain::Planetary.index()] = 300;
        changed.buckets[PossibilityDomain::Geology.index()] = 3_700;
        map.apply_preserve_contribution(20, source, changed);
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );
        let preserve_key = map.expected_layer_hash(neighbor, LAYER_TERRAIN).unwrap();
        map.regions.get_mut(&neighbor).unwrap().dirty_layers = 0;
        map.regions.get_mut(&neighbor).unwrap().status = GenerationStatus::Ready;
        let mut climate_only = changed;
        climate_only.buckets[PossibilityDomain::Climate.index()] = 123;
        map.apply_preserve_contribution(10, source, climate_only);
        assert_eq!(
            map.expected_layer_hash(neighbor, LAYER_TERRAIN),
            Some(preserve_key)
        );
        assert_eq!(map.regions[&neighbor].dirty_layers, 0);

        // Session restoration replaces source authority with parked authority,
        // but a material P/G pair still invalidates its active neighbor.
        let restored = world_core::RegionSnapshotRecord {
            coord: source,
            current: PossibilitySignature {
                buckets: [1_900; POSSIBILITY_DIMS],
            }
            .dequantize()
            .dims,
            stability: 0.25,
            revision: 9,
        };
        map.restore_region(&restored);
        assert_eq!(map.regions[&source].status, GenerationStatus::Unloaded);
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );

        // Actual convergence of slow buckets fans out, while a Climate-only
        // crossing dirties only the source's declared readers.
        let mut map = RegionMap::new(StreamConfig {
            converge_per_unit: 1.0,
            converge_rate_cap: 1.0,
            ..tiny_config()
        });
        let mut slow = base;
        slow.set(PossibilityDomain::Planetary, 0.1);
        slow.set(PossibilityDomain::Geology, 0.1);
        let mut slow_target = slow;
        slow_target.set(PossibilityDomain::Planetary, 0.9);
        slow_target.set(PossibilityDomain::Geology, 0.9);
        let mut source_region = clean_active_region(source, slow);
        source_region.target = slow_target;
        source_region.stability = 0.0;
        map.regions.insert(source, source_region);
        map.regions
            .insert(neighbor, clean_active_region(neighbor, base));
        let mut stats = FrameStats::default();
        map.converge(PLAYER, 1.0, 1.0, 1.0, &Budget::unlimited(), &mut stats);
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );
        map.regions.get_mut(&neighbor).unwrap().dirty_layers = 0;
        map.regions.get_mut(&neighbor).unwrap().status = GenerationStatus::Ready;
        let terrain_key = map.expected_layer_hash(neighbor, LAYER_TERRAIN).unwrap();
        let source_region = map.regions.get_mut(&source).unwrap();
        source_region.current = source_region.target;
        let climate_target = if source_region.current.get(PossibilityDomain::Climate) < 0.5 {
            0.99
        } else {
            0.01
        };
        source_region
            .target
            .set(PossibilityDomain::Climate, climate_target);
        source_region.stability = 0.0;
        source_region.dirty_layers = 0;
        source_region.status = GenerationStatus::Ready;
        map.converge(PLAYER, 1.0, 1.0, 1.0, &Budget::unlimited(), &mut stats);
        assert_eq!(
            map.expected_layer_hash(neighbor, LAYER_TERRAIN),
            Some(terrain_key)
        );
        assert_eq!(map.regions[&neighbor].dirty_layers, 0);

        // A recipe change through `update` invalidates a fallback-sensitive
        // Terrain job and refreshes the key before old work can integrate.
        let mut map = RegionMap::new(tiny_config());
        map.regions
            .insert(neighbor, clean_active_region(neighbor, base));
        map.regions.get_mut(&neighbor).unwrap().dirty_layers = layer_bit(LAYER_TERRAIN);
        let executor = ManualExecutor::default();
        let old_key = map.expected_layer_hash(neighbor, LAYER_TERRAIN).unwrap();
        map.submit_layer(
            neighbor,
            LAYER_TERRAIN,
            old_key,
            TaskPriority::Critical,
            &executor,
        );
        let changed_field = PossibilityField::new(7);
        let stats = map.update(
            PLAYER,
            0.0,
            &changed_field,
            &[],
            &NO_BIAS,
            &Budget {
                max_loads: 0,
                max_regen_cost: 0,
                ..Budget::unlimited()
            },
            &executor,
            false,
        );
        assert_eq!(stats.jobs_cancelled, 1);
        assert!(!map.in_flight.contains_key(&(neighbor, LAYER_TERRAIN)));
        assert_ne!(
            map.expected_layer_hash(neighbor, LAYER_TERRAIN),
            Some(old_key)
        );
        assert_ne!(
            map.regions[&neighbor].dirty_layers & layer_bit(LAYER_TERRAIN),
            0
        );
    }

    #[test]
    fn ordinary_history_halos_use_parked_authority_and_fan_out_nine_consumers() {
        let mut map = RegionMap::new(StreamConfig {
            field_resolution: 8,
            ..tiny_config()
        });
        let source = RegionCoord::new(0, 0);
        let mut source_region = RegionState::new(source);
        source_region.current.set(PossibilityDomain::Planetary, 0.1);
        source_region.current.set(PossibilityDomain::Geology, 0.9);
        source_region.target = source_region.current;
        source_region.status = GenerationStatus::Unloaded;
        source_region.dirty_layers = 0;
        let expected_pair = [
            source_region
                .current
                .quantized(PossibilityDomain::Planetary),
            source_region.current.quantized(PossibilityDomain::Geology),
        ];
        map.regions.insert(source, source_region);

        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let coord = RegionCoord::new(dx, dy);
                let mut region = RegionState::new(coord);
                region.status = GenerationStatus::Ready;
                region.dirty_layers = 0;
                map.regions.insert(coord, region);
            }
        }
        let outside = RegionCoord::new(2, 0);
        let mut outside_region = RegionState::new(outside);
        outside_region.status = GenerationStatus::Ready;
        outside_region.dirty_layers = 0;
        map.regions.insert(outside, outside_region);

        let neighbor_halo = map.terrain_halo(RegionCoord::new(1, 0));
        assert_eq!(neighbor_halo.buckets_at(source), Some(expected_pair));
        let mut stats = FrameStats::default();
        map.invalidate_terrain_consumers(source, &mut stats);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let coord = RegionCoord::new(dx, dy);
                if coord == source {
                    assert_eq!(map.regions[&coord].status, GenerationStatus::Unloaded);
                } else {
                    assert_ne!(
                        map.regions[&coord].dirty_layers & dependents_closure(LAYER_TERRAIN),
                        0
                    );
                }
            }
        }
        assert_eq!(map.regions[&outside].dirty_layers, 0);
    }
}
