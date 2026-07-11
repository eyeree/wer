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
//! `dirty_layers` is an exact optimization hint over it: converge, upstream
//! integration, macro regeneration, and revision bumps set bits precisely when
//! a layer's expected hash may have changed, so clean regions skip hash checks
//! entirely, and a result arriving for a re-dirtied layer is dropped as
//! superseded. Everything is budgeted (section 6.6), regeneration by declared
//! *cost* rather than count (phase-2-plan.md §8.2), so a big possibility
//! change ripples outward over several frames instead of hitching.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use world_core::layer::{
    all_layers_mask, dependents_closure, domain_dirty_mask, layer_bit, layer_decl, LAYER_COUNT,
    LAYER_DRAINAGE,
};
use world_core::{
    drainage_dep_hash, layer_dep_hash, macro_coord_for, project_plausible, steer, Anchor,
    DrainageTile, PossibilityField, PossibilityVector, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE,
};

use crate::budget::Budget;
use crate::generate::{generate_layer, layer_channels, GeneratedTile, LayerInputs, RegionCache};
use crate::macrocache::MacroCache;
use crate::region::{GenerationStatus, RegionState};
use crate::task::{TaskExecutor, TaskPriority};

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
        }
    }
}

/// Per-frame counters (phase-2-plan.md §8.2, §13). `deferred_*` report budget
/// backpressure — expected and healthy, not an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// The freshly computed expected dependency hash, or `None` while an
    /// input tile is missing or the macro input is stale.
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
    /// arrival.
    in_flight: BTreeMap<(RegionCoord, u16), u64>,
    next_job_id: u64,
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
            revision_bumps: [0; LAYER_COUNT as usize],
            results_tx,
            results_rx,
            in_flight: BTreeMap::new(),
            next_job_id: 0,
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
    ) -> FrameStats {
        let mut stats = FrameStats::default();
        self.integrate_finished(&mut stats);
        self.evict(player, &mut stats);
        self.load(player, field, anchors, bias, budget, &mut stats);
        self.retarget(player, field, anchors, bias);
        self.converge(player, travel, budget, &mut stats);
        self.dispatch_regen(player, field, budget, executor, &mut stats);
        // A synchronous executor (InlineExecutor) has already finished every
        // job dispatched above; integrating again here keeps the headless
        // replay settled within a single frame.
        self.integrate_finished(&mut stats);
        stats.active_regions = self.regions.len();
        stats.cache_bytes = self.cache.bytes();
        stats.macro_cache_bytes = self.macro_cache.bytes();
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

    /// The expected dependency hash of `(coord, layer)`, or `None` while any
    /// input tile is missing or the macro input is stale. Callers must ensure
    /// tile inputs are *fresh* (dirty bits clear, nothing in flight) before
    /// trusting the result — the fold consumes stored input hashes.
    fn expected_layer_hash(&self, coord: RegionCoord, layer: u16) -> Option<u64> {
        let decl = layer_decl(layer);
        let region = self.regions.get(&coord)?;
        let tiles = self.cache.get(coord);
        let mut input_hashes = Vec::with_capacity(decl.deps.len());
        for &dep in decl.deps {
            let hash = if dep == LAYER_DRAINAGE {
                let mc = macro_coord_for(coord);
                let tile = self.macro_cache.get(mc)?;
                if tile.dep_hash != self.expected_macro_hash(mc) {
                    return None;
                }
                tile.dep_hash
            } else {
                tiles?.layer_hash(dep)?
            };
            input_hashes.push(hash);
        }
        Some(layer_dep_hash(
            coord,
            layer,
            self.effective_revision(layer),
            &region.current.quantized_domains(decl.domains),
            &input_hashes,
            self.cfg.field_resolution,
        ))
    }

    /// Drain the results channel, integrating tiles that are still current.
    /// Arrival order does not matter: content is a pure function of the
    /// dependency key (ADR 0008), and superseded results — those whose layer
    /// was re-dirtied while they were in flight — are dropped, so a threaded
    /// executor converges to the same cache as an inline one.
    fn integrate_finished(&mut self, stats: &mut FrameStats) {
        while let Ok(result) = self.results_rx.try_recv() {
            match result {
                JobResult::Macro {
                    coord,
                    job_id,
                    tile,
                } => {
                    let key = (coord, LAYER_DRAINAGE);
                    if self.in_flight.get(&key) != Some(&job_id) {
                        continue; // superseded or evicted while in flight
                    }
                    self.in_flight.remove(&key);
                    self.macro_cache.insert(Arc::new(tile));
                    stats.macro_jobs += 1;
                    stats.layers_regenerated += 1;
                    stats.regenerated_by_layer[LAYER_DRAINAGE as usize] += 1;
                    // A regenerated macro tile changes its dependents' expected
                    // hydrology hashes; notify them so the hint stays exact
                    // (phase-2-plan.md §7.8).
                    for (&c, region) in self.regions.iter_mut() {
                        if macro_coord_for(c) == coord {
                            region.dirty_layers |= layer_bit(world_core::layer::LAYER_HYDROLOGY);
                            if region.status == GenerationStatus::Ready {
                                region.status = GenerationStatus::Generating;
                            }
                        }
                    }
                }
                JobResult::Tile(result) => {
                    let key = (result.coord, result.layer);
                    if self.in_flight.get(&key) != Some(&result.job_id) {
                        continue; // superseded or evicted while in flight
                    }
                    self.in_flight.remove(&key);
                    let Some(region) = self.regions.get_mut(&result.coord) else {
                        continue;
                    };
                    if region.dirty_layers & layer_bit(result.layer) != 0 {
                        // The layer was re-dirtied while this job flew (bucket
                        // flip, upstream regeneration, or revision bump): its
                        // expected hash moved on, so the result is stale.
                        // Dropping it leaves the dirty bit set, which forces a
                        // redispatch with fresh inputs.
                        continue;
                    }
                    for (channel, tile) in result.channels {
                        self.cache
                            .insert_channel(result.coord, channel, Arc::new(tile));
                    }
                    if let Some(biome) = result.biome {
                        self.cache.insert_biome(result.coord, Arc::new(biome));
                    }
                    // Downstream layers' expected hashes changed the moment the
                    // new tile landed: mark every transitive dependent.
                    region.dirty_layers |=
                        dependents_closure(result.layer) & !layer_bit(result.layer);
                    stats.layers_regenerated += 1;
                    stats.regenerated_by_layer[result.layer as usize] += 1;
                    let coord = result.coord;
                    self.refresh_status(coord);
                }
            }
        }
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
    /// any in-flight bookkeeping together, then sweep macro tiles (and macro
    /// jobs) that no resident region depends on any more (phase-2-plan.md
    /// §6.3).
    fn evict(&mut self, player: (f64, f64), stats: &mut FrameStats) {
        let unload = self.cfg.unload_radius;
        let gone: Vec<RegionCoord> = self
            .regions
            .keys()
            .copied()
            .filter(|c| center_distance(*c, player) > unload)
            .collect();
        if gone.is_empty() {
            return;
        }
        for coord in gone {
            self.regions.remove(&coord);
            self.cache.remove_region(coord);
            let keys: Vec<(RegionCoord, u16)> = self
                .in_flight
                .range((coord, 0)..=(coord, u16::MAX))
                .map(|(k, _)| *k)
                .collect();
            for k in keys {
                self.in_flight.remove(&k);
            }
            stats.evicted += 1;
        }
        self.macro_cache.evict_orphans(self.regions.keys());
        let needed: BTreeSet<RegionCoord> =
            self.regions.keys().map(|&c| macro_coord_for(c)).collect();
        self.in_flight
            .retain(|(c, _), _| c.level == 0 || needed.contains(c));
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
        for &(_, coord) in candidates.iter().take(budget.max_loads) {
            let mut region = RegionState::new(coord);
            region.target = self.target_for(coord, field, anchors, bias);
            region.current = region.target;
            region.stability = stability_for(&self.cfg, center_distance(coord, player));
            region.dirty_layers = all_layers_mask();
            self.regions.insert(coord, region);
            stats.loaded += 1;
        }
        stats.deferred_loads = candidates.len().saturating_sub(budget.max_loads);
    }

    /// Recompute stability and target for every resident region. Cheap (a few
    /// hundred bilinear samples), so it is not budgeted.
    fn retarget(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        bias: &[f32; POSSIBILITY_DIMS],
    ) {
        let coords: Vec<RegionCoord> = self.regions.keys().copied().collect();
        for coord in coords {
            let stability = stability_for(&self.cfg, center_distance(coord, player));
            let target = self.target_for(coord, field, anchors, bias);
            let region = self.regions.get_mut(&coord).expect("resident");
            region.stability = stability;
            region.target = target;
        }
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
        budget: &Budget,
        stats: &mut FrameStats,
    ) {
        let rate =
            (self.cfg.converge_per_unit * travel.max(0.0) as f32).min(self.cfg.converge_rate_cap);
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
            if let Some(flipped) = region.converge(rate) {
                stats.converged += 1;
                if flipped != 0 {
                    region.dirty_layers |= domain_dirty_mask(flipped);
                    if region.status == GenerationStatus::Ready {
                        region.status = GenerationStatus::Generating;
                    }
                }
            }
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

    /// Scan one region's dirty layers in topological (id) order, clearing
    /// false-positive hints and dispatching stale layers whose inputs are
    /// fresh. Returns whether anything was submitted.
    fn dispatch_region(
        &mut self,
        coord: RegionCoord,
        priority: TaskPriority,
        field: &PossibilityField,
        budget: &Budget,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) -> bool {
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
            if self.in_flight.contains_key(&(coord, layer)) {
                continue; // result pending; the dirty bit will drop it stale
            }
            if !self.inputs_fresh(coord, layer) {
                continue; // an earlier layer must land first
            }
            let Some(expected) = self.expected_layer_hash(coord, layer) else {
                continue;
            };
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
        if self.macro_fresh(coord) {
            self.clear_dirty(coord, LAYER_DRAINAGE);
            return;
        }
        if self.in_flight.contains_key(&(mc, LAYER_DRAINAGE)) {
            return;
        }
        let cost = layer_decl(LAYER_DRAINAGE).cost;
        if stats.regen_cost_spent.saturating_add(cost) > budget.max_regen_cost {
            stats.deferred_regens += 1;
            return;
        }
        let expected = self.expected_macro_hash(mc);
        let job_id = self.next_job_id;
        self.next_job_id += 1;
        self.in_flight.insert((mc, LAYER_DRAINAGE), job_id);
        let tx = self.results_tx.clone();
        let field = *field;
        executor.submit(
            priority,
            Box::new(move || {
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

    /// Whether every declared input of `(coord, layer)` is fresh: dirty bit
    /// clear, tile present, nothing in flight (macro input checked by hash).
    fn inputs_fresh(&self, coord: RegionCoord, layer: u16) -> bool {
        let region = &self.regions[&coord];
        let tiles = self.cache.get(coord);
        for &dep in layer_decl(layer).deps {
            if dep == LAYER_DRAINAGE {
                if !self.macro_fresh(coord) {
                    return false;
                }
                continue;
            }
            if region.dirty_layers & layer_bit(dep) != 0
                || self.in_flight.contains_key(&(coord, dep))
                || tiles.and_then(|t| t.layer_hash(dep)).is_none()
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

    /// Snapshot a layer's inputs and submit its generation job. Clears the
    /// dirty bit at dispatch: anything that re-dirties the layer while the job
    /// flies thereby marks the result superseded (see `integrate_finished`).
    fn submit_layer(
        &mut self,
        coord: RegionCoord,
        layer: u16,
        expected: u64,
        priority: TaskPriority,
        executor: &dyn TaskExecutor,
    ) {
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
        let region = self.regions.get_mut(&coord).expect("resident");
        let inputs = LayerInputs {
            quantized: region.current.quantized_domains(decl.domains),
            tiles: input_tiles,
            biome,
            drainage,
            dep_hash: expected,
        };
        region.dirty_layers &= !layer_bit(layer);
        region.status = GenerationStatus::Generating;

        let job_id = self.next_job_id;
        self.next_job_id += 1;
        self.in_flight.insert((coord, layer), job_id);
        let resolution = self.cfg.field_resolution;
        let tx = self.results_tx.clone();
        executor.submit(
            priority,
            Box::new(move || {
                let mut out = generate_layer(coord, layer, &inputs, resolution);
                out.job_id = job_id;
                // The receiver may be gone if the map was dropped; the job's
                // work is simply discarded then.
                let _ = tx.send(JobResult::Tile(out));
            }),
        );
    }
}
