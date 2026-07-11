//! Region streaming: the moving window of active regions, the distance-driven
//! stability ramp, budgeted convergence, and incremental regeneration
//! (phase-1-plan.md sections 4.2 and 7; milestone M4).
//!
//! [`RegionMap::update`] is the once-per-frame heart of the Phase 1 prototype:
//!
//! 1. integrate finished generation jobs,
//! 2. evict regions beyond `unload_radius` (hysteresis vs `load_radius`),
//! 3. load missing regions nearest-first,
//! 4. recompute every region's stability and steered target,
//! 5. converge distant regions toward their targets (farthest-first),
//! 6. dispatch stale region-layers to the [`TaskExecutor`] nearest-first.
//!
//! Everything is budgeted (section 6.6) so a big possibility change ripples
//! outward over several frames instead of hitching.

use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};

use world_core::layer::{layer_bit, ALL_LAYERS, LAYER_COUNT};
use world_core::{
    project_plausible, steer, Anchor, PossibilityField, PossibilityVector, RegionCoord,
    POSSIBILITY_DIMS, REGION_SIZE,
};

use crate::budget::Budget;
use crate::generate::{generate_layer, GeneratedTile, RegionCache};
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
    /// Per-frame convergence fraction fed to [`RegionState::converge`].
    pub converge_rate: f32,
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
            converge_rate: 0.15,
            field_resolution: world_core::FIELD_RES,
        }
    }
}

/// Per-frame counters (phase-1-plan.md section 12). `deferred_*` report
/// budget backpressure — expected and healthy, not an error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Regions inserted this frame.
    pub loaded: usize,
    /// Regions evicted this frame.
    pub evicted: usize,
    /// Regions whose realized state actually moved this frame.
    pub converged: usize,
    /// Generation jobs dispatched this frame.
    pub layers_dispatched: usize,
    /// Finished region-layers integrated into the cache this frame.
    pub layers_regenerated: usize,
    /// Loads that missed the per-frame budget.
    pub deferred_loads: usize,
    /// Converge candidates that missed the budget.
    pub deferred_converges: usize,
    /// Stale region-layers that missed the dispatch budget.
    pub deferred_regens: usize,
    /// Resident regions after this frame.
    pub active_regions: usize,
    /// Heap bytes held by the field cache after this frame.
    pub cache_bytes: usize,
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

/// The active window of regions plus their field cache.
///
/// Region state lives in a `BTreeMap`, not a hash map: iteration order is part
/// of the determinism contract. Budgeted work (loads, convergence, regen) must
/// pick the same regions in the same order on every run for the continuity
/// replay's two-run equality check to hold (phase-1-plan.md section 11.3).
#[derive(Debug)]
pub struct RegionMap {
    cfg: StreamConfig,
    regions: BTreeMap<RegionCoord, RegionState>,
    cache: RegionCache,
    /// Completed generation jobs flow back through this channel; jobs are pure
    /// and send owned tiles, so the main thread is the only cache writer
    /// (phase-1-plan.md section 9).
    results_tx: Sender<GeneratedTile>,
    results_rx: Receiver<GeneratedTile>,
    /// One entry per region-layer job in flight, keyed to the job id it was
    /// dispatched as. Results whose id no longer matches (superseded, evicted,
    /// or from an evicted-then-reloaded region) are dropped on arrival.
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
    /// (the live task-queue depth; telemetry, phase-1-plan.md section 12).
    #[inline]
    #[must_use]
    pub fn jobs_in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// One frame of streaming work (see module docs for the step order).
    ///
    /// `bias` is a per-dimension offset the player steers directly (the
    /// keyboard "nudge" input), applied to the field sample before anchors.
    pub fn update(
        &mut self,
        player: (f64, f64),
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
        self.converge(player, budget, &mut stats);
        self.dispatch_regen(player, budget, executor, &mut stats);
        // A synchronous executor (InlineExecutor) has already finished every
        // job dispatched above; integrating again here keeps the headless
        // replay settled within a single frame.
        self.integrate_finished(&mut stats);
        stats.active_regions = self.regions.len();
        stats.cache_bytes = self.cache.bytes();
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

    /// Drain the results channel, integrating tiles whose job id and revision
    /// still match. Arrival order does not matter: content is a pure function
    /// of the job inputs, and mismatched results are dropped, so a threaded
    /// executor converges to the same cache as an inline one.
    fn integrate_finished(&mut self, stats: &mut FrameStats) {
        while let Ok(result) = self.results_rx.try_recv() {
            let key = (result.coord, result.layer);
            if self.in_flight.get(&key) != Some(&result.job_id) {
                continue; // superseded or evicted while in flight
            }
            self.in_flight.remove(&key);
            let Some(region) = self.regions.get_mut(&result.coord) else {
                continue;
            };
            if result.revision != region.revision {
                continue; // realized state moved on; dirty bit still set
            }
            for (channel, tile) in result.channels {
                self.cache.insert_channel(result.coord, channel, tile);
            }
            region.dirty_layers &= !layer_bit(result.layer);
            stats.layers_regenerated += 1;
            let coord = result.coord;
            if self.regions[&coord].dirty_layers == 0
                && self
                    .in_flight
                    .range((coord, 0)..=(coord, u16::MAX))
                    .next()
                    .is_none()
            {
                self.regions.get_mut(&coord).expect("resident").status = GenerationStatus::Ready;
            }
        }
    }

    /// Evict regions beyond `unload_radius`, dropping state, cache tiles, and
    /// any in-flight bookkeeping together (phase-1-plan.md section 7.4).
    fn evict(&mut self, player: (f64, f64), stats: &mut FrameStats) {
        let unload = self.cfg.unload_radius;
        let gone: Vec<RegionCoord> = self
            .regions
            .keys()
            .copied()
            .filter(|c| center_distance(*c, player) > unload)
            .collect();
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
            region.dirty_layers = ALL_LAYERS;
            self.regions.insert(coord, region);
            stats.loaded += 1;
        }
        stats.deferred_loads = candidates.len().saturating_sub(budget.max_loads);
    }

    /// Recompute stability and target for every resident region. Cheap (a few
    /// hundred bilinear samples), so it is not budgeted in Phase 1.
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
    fn converge(&mut self, player: (f64, f64), budget: &Budget, stats: &mut FrameStats) {
        let mut eligible: Vec<(u64, RegionCoord)> = self
            .regions
            .iter()
            .filter(|(_, r)| r.stability < 1.0 && r.current != r.target)
            .map(|(c, _)| (center_distance(*c, player).to_bits(), *c))
            .collect();
        // Farthest first: descending distance, coord tiebreak for determinism.
        eligible.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let rate = self.cfg.converge_rate;
        for &(_, coord) in eligible.iter().take(budget.max_converge_regions) {
            let region = self.regions.get_mut(&coord).expect("resident");
            if region.converge(rate) {
                stats.converged += 1;
            }
        }
        stats.deferred_converges = eligible.len().saturating_sub(budget.max_converge_regions);
    }

    /// Dispatch stale region-layers to the executor, nearest-first, up to the
    /// budget. Jobs snapshot `(current, revision)`; a region whose state moves
    /// again before its job lands simply drops the stale result and redispatches
    /// next frame (safe supersession, phase-1-plan.md section 7.3).
    fn dispatch_regen(
        &mut self,
        player: (f64, f64),
        budget: &Budget,
        executor: &dyn TaskExecutor,
        stats: &mut FrameStats,
    ) {
        let mut stale: Vec<(u64, RegionCoord, u16)> = Vec::new();
        for (&coord, region) in &self.regions {
            for layer in 0..LAYER_COUNT {
                let missing = self.cache.get(coord).is_none_or(|tiles| match layer {
                    world_core::layer::LAYER_TERRAIN => {
                        tiles.channels[crate::generate::CHANNEL_ELEVATION].is_none()
                    }
                    world_core::layer::LAYER_CLIMATE => {
                        tiles.channels[crate::generate::CHANNEL_TEMPERATURE].is_none()
                            || tiles.channels[crate::generate::CHANNEL_MOISTURE].is_none()
                    }
                    _ => tiles.channels[crate::generate::CHANNEL_VEGETATION].is_none(),
                });
                if (region.dirty_layers & layer_bit(layer) != 0 || missing)
                    && !self.in_flight.contains_key(&(coord, layer))
                {
                    stale.push((center_distance(coord, player).to_bits(), coord, layer));
                }
            }
        }
        // Nearest first: what the player is close to regenerates soonest.
        stale.sort_unstable_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        let resolution = self.cfg.field_resolution;
        for &(dist_bits, coord, layer) in stale.iter().take(budget.max_regen_layers) {
            let region = self.regions.get_mut(&coord).expect("resident");
            let current = region.current;
            let revision = region.revision;
            region.status = GenerationStatus::Generating;
            let job_id = self.next_job_id;
            self.next_job_id += 1;
            self.in_flight.insert((coord, layer), job_id);

            let distance = f64::from_bits(dist_bits);
            let priority = if distance <= self.cfg.near_radius {
                TaskPriority::Critical
            } else if distance <= self.cfg.far_radius {
                TaskPriority::Normal
            } else {
                TaskPriority::Background
            };

            let tx = self.results_tx.clone();
            executor.submit(
                priority,
                Box::new(move || {
                    let mut out = generate_layer(coord, layer, &current, revision, resolution);
                    out.job_id = job_id;
                    // The receiver may be gone if the map was dropped; the
                    // job's work is simply discarded then.
                    let _ = tx.send(out);
                }),
            );
            stats.layers_dispatched += 1;
        }
        stats.deferred_regens = stale.len().saturating_sub(budget.max_regen_layers);
    }
}
