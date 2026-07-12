//! Unit tests for the streaming window (phase-2-plan.md §12.4): load/evict
//! hysteresis, stability ramp shape, cost-budget enforcement, topological
//! dispatch, dep-hash staleness precision, and macro cache lifecycle.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};

use world_core::habitat::{FERTILITY_BANDS, MOISTURE_BANDS, TEMPERATURE_BANDS};
use world_core::layer::{
    LAYER_BIOME, LAYER_CLIMATE, LAYER_DRAINAGE, LAYER_ECOLOGY, LAYER_GEOLOGY, LAYER_HYDROLOGY,
    LAYER_SOILS, LAYER_TERRAIN, LAYER_VEGETATION,
};
use world_core::{
    macro_coord_for, HabitatSignature, PossibilityDomain, PossibilityField, RegionCoord,
    BIOME_COUNT, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    layer_channels, stability_for, Budget, GenerationStatus, InlineExecutor, RegionMap,
    RosterCache, StreamConfig, TaskExecutor, TaskPriority, CHANNEL_ELEVATION, CHANNEL_HARDNESS,
    CHANNEL_RIVER, CHANNEL_VEGETATION,
};

const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

fn small_config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 3.0 * REGION_SIZE,
        load_radius: 4.0 * REGION_SIZE,
        unload_radius: 5.0 * REGION_SIZE,
        converge_per_unit: 0.01,
        converge_rate_cap: 0.25,
        field_resolution: 4,
        ..StreamConfig::default()
    }
}

fn settle(map: &mut RegionMap, player: (f64, f64)) {
    let field = PossibilityField::default();
    for _ in 0..8 {
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
    }
}

fn settled_map(player: (f64, f64)) -> RegionMap {
    let mut map = RegionMap::new(small_config());
    settle(&mut map, player);
    map
}

#[derive(Debug, PartialEq, Eq)]
struct LayerFingerprint {
    dependency_hash: u64,
    content_hashes: Vec<u64>,
}

#[derive(Debug, PartialEq, Eq)]
struct RegionFingerprint {
    coord: RegionCoord,
    layers: Vec<LayerFingerprint>,
}

type TestJob = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct ManualExecutor {
    queued: RefCell<VecDeque<TestJob>>,
    run_inline: Cell<bool>,
}

impl ManualExecutor {
    fn run_next(&self) {
        self.queued
            .borrow_mut()
            .pop_front()
            .expect("queued generation job")();
    }

    fn queue_len(&self) -> usize {
        self.queued.borrow().len()
    }

    fn run_submissions_inline(&self) {
        self.run_inline.set(true);
    }
}

impl TaskExecutor for ManualExecutor {
    fn submit(&self, _priority: TaskPriority, job: TestJob) {
        if self.run_inline.get() {
            job();
        } else {
            self.queued.borrow_mut().push_back(job);
        }
    }

    fn parallelism(&self) -> usize {
        1
    }
}

fn is_current_fixed_point(map: &RegionMap) -> bool {
    !map.is_empty()
        && map.jobs_in_flight() == 0
        && map.iter_active().all(|region| {
            region.status == GenerationStatus::Ready
                && region.dirty_layers == 0
                && map
                    .layer_diagnostics(region.coord)
                    .is_some_and(|diagnostics| {
                        diagnostics
                            .iter()
                            .all(|diagnostic| diagnostic.stored == diagnostic.expected)
                    })
        })
}

fn update_until_current(
    map: &mut RegionMap,
    player: (f64, f64),
    budget: &Budget,
    max_updates: usize,
) -> [usize; world_core::LAYER_COUNT as usize] {
    let field = PossibilityField::default();
    let mut regenerated = [0; world_core::LAYER_COUNT as usize];
    for _ in 0..max_updates {
        let stats = map.update(
            player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            budget,
            &InlineExecutor,
            false,
        );
        for (total, frame) in regenerated.iter_mut().zip(stats.regenerated_by_layer) {
            *total += frame;
        }
        if is_current_fixed_point(map) {
            return regenerated;
        }
    }
    panic!(
        "window did not reach a current fixed point in {max_updates} updates ({} jobs remain)",
        map.jobs_in_flight()
    );
}

fn layer_content_hashes(map: &RegionMap, coord: RegionCoord, layer: u16) -> Vec<u64> {
    if layer == LAYER_DRAINAGE {
        return vec![map
            .macro_cache()
            .get(macro_coord_for(coord))
            .expect("current drainage tile")
            .content_hash()];
    }

    let tiles = map.cache().get(coord).expect("current region tiles");
    let mut hashes: Vec<u64> = layer_channels(layer)
        .iter()
        .map(|&channel| {
            tiles.channels[channel]
                .as_ref()
                .expect("current layer channel")
                .content_hash()
        })
        .collect();
    if layer == LAYER_BIOME {
        hashes.push(
            tiles
                .biome
                .as_ref()
                .expect("current biome tile")
                .content_hash(),
        );
    } else if layer == LAYER_ECOLOGY {
        hashes.push(
            tiles
                .dominant
                .as_ref()
                .expect("current dominant-species tile")
                .content_hash(),
        );
    }
    assert!(
        !hashes.is_empty(),
        "layer {layer} has no fingerprinted output"
    );
    hashes
}

fn world_fingerprint(map: &RegionMap) -> Vec<RegionFingerprint> {
    map.iter_active()
        .map(|region| {
            let diagnostics = map
                .layer_diagnostics(region.coord)
                .expect("resident diagnostics");
            let layers = diagnostics
                .iter()
                .map(|diagnostic| {
                    assert_eq!(
                        diagnostic.stored, diagnostic.expected,
                        "region {:?} layer {} is not current",
                        region.coord, diagnostic.layer
                    );
                    LayerFingerprint {
                        dependency_hash: diagnostic.stored.expect("settled layer hash"),
                        content_hashes: layer_content_hashes(map, region.coord, diagnostic.layer),
                    }
                })
                .collect();
            RegionFingerprint {
                coord: region.coord,
                layers,
            }
        })
        .collect()
}

fn resident_cell_signatures(map: &RegionMap) -> BTreeSet<HabitatSignature> {
    let resolution = map.config().field_resolution;
    let mut signatures = BTreeSet::new();
    for region in map.iter_active() {
        assert_eq!(region.status, GenerationStatus::Ready);
        for cy in 0..resolution {
            for cx in 0..resolution {
                let signature = map
                    .cell_signature(region.coord, cx, cy)
                    .expect("settled cell signature");
                assert!(
                    map.roster_cache().get(signature).is_some(),
                    "settled cell {:?} ({cx}, {cy}) lost roster {signature:?}",
                    region.coord
                );
                let ecology = map
                    .cell_ecology(region.coord, cx, cy)
                    .expect("settled cell ecology");
                assert_eq!(ecology.signature, signature);
                assert!(ecology.herbivore.is_some());
                assert!(ecology.predator.is_some());
                assert!(ecology.diversity.is_some());
                signatures.insert(signature);
            }
        }
    }
    signatures
}

fn assert_roster_floor(map: &RegionMap, required: &BTreeSet<HabitatSignature>) -> usize {
    let required_bytes: usize = required
        .iter()
        .map(|&signature| {
            map.roster_cache()
                .get(signature)
                .expect("required roster")
                .bytes()
        })
        .sum();
    assert_eq!(map.roster_cache().len(), required.len());
    assert_eq!(map.roster_cache().bytes(), required_bytes);
    required_bytes
}

fn unused_valid_signature(used: &BTreeSet<HabitatSignature>) -> HabitatSignature {
    for biome in 0..BIOME_COUNT {
        let biome = u8::try_from(biome).expect("biome id fits in u8");
        for temperature_band in 0..TEMPERATURE_BANDS {
            for moisture_band in 0..MOISTURE_BANDS {
                for fertility_band in 0..FERTILITY_BANDS {
                    let candidate = HabitatSignature {
                        biome,
                        temperature_band,
                        moisture_band,
                        fertility_band,
                    };
                    if !used.contains(&candidate) {
                        return candidate;
                    }
                }
            }
        }
    }
    panic!("tiny fixture unexpectedly covered the complete habitat signature space");
}

#[test]
fn stability_ramp_endpoints_and_monotonicity() {
    let cfg = small_config();
    assert_eq!(stability_for(&cfg, 0.0), 1.0);
    assert_eq!(stability_for(&cfg, cfg.near_radius), 1.0);
    assert_eq!(stability_for(&cfg, cfg.far_radius), 0.0);
    assert_eq!(stability_for(&cfg, cfg.far_radius * 10.0), 0.0);
    let mut last = 1.0f32;
    let steps = 100;
    for i in 0..=steps {
        let d =
            cfg.near_radius + (cfg.far_radius - cfg.near_radius) * f64::from(i) / f64::from(steps);
        let s = stability_for(&cfg, d);
        assert!(
            s <= last,
            "ramp not monotonic at distance {d}: {s} > {last}"
        );
        assert!((0.0..=1.0).contains(&s));
        last = s;
    }
}

#[test]
fn window_loads_and_settles_every_layer_bottom_up() {
    let map = settled_map((0.0, 0.0));
    assert!(!map.is_empty());
    for region in map.iter_active() {
        let (ox, oy) = region.coord.origin();
        let d = ((ox + REGION_SIZE * 0.5).powi(2) + (oy + REGION_SIZE * 0.5).powi(2)).sqrt();
        assert!(d <= small_config().load_radius);
        // Fresh regions realize their target immediately (no initial pop) and
        // finish the whole eight-layer stack under an unlimited budget.
        assert_eq!(region.current, region.target);
        assert_eq!(region.status, GenerationStatus::Ready);
        assert_eq!(region.dirty_layers, 0);
        let tiles = map.cache().get(region.coord).expect("cached");
        for layer in [
            LAYER_TERRAIN,
            LAYER_GEOLOGY,
            LAYER_CLIMATE,
            LAYER_HYDROLOGY,
            LAYER_SOILS,
            LAYER_BIOME,
            LAYER_VEGETATION,
        ] {
            assert!(
                tiles.layer_hash(layer).is_some(),
                "layer {layer} missing for region {:?}",
                region.coord
            );
        }
        // The covering macro drainage tile is resident and fresh.
        assert!(map
            .macro_cache()
            .get(macro_coord_for(region.coord))
            .is_some());
    }
    assert_eq!(map.jobs_in_flight(), 0);
}

#[test]
fn eviction_has_hysteresis_and_sweeps_macro_orphans() {
    let cfg = small_config();
    let mut map = settled_map((0.0, 0.0));
    let field = PossibilityField::default();

    // A region on the far edge of the initial window.
    let edge = RegionCoord::new(3, 0);
    assert!(map.get(edge).is_some());

    // Move so the edge region's center distance (~1143 units) lands between
    // load_radius (1024) and unload_radius (1280): outside the load zone but
    // inside the hysteresis band, so it must stay resident.
    assert!(cfg.load_radius < 1143.0 && 1143.0 < cfg.unload_radius);
    let player = (-240.0, 0.0);
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
    assert!(
        map.get(edge).is_some(),
        "hysteresis should retain the region"
    );

    // Move far enough west that every region under macro tile (0,0) unloads:
    // the macro tile must be swept with its last dependent.
    let player = (-(8.0 * REGION_SIZE), 0.0);
    for _ in 0..4 {
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
    }
    assert!(map.get(edge).is_none());
    assert!(map.cache().get(edge).is_none());
    let mc = macro_coord_for(RegionCoord::new(0, 0));
    assert!(
        map.iter_active().all(|r| macro_coord_for(r.coord) != mc),
        "test setup: no dependent regions should remain"
    );
    assert!(
        map.macro_cache().get(mc).is_none(),
        "orphaned macro tile must be evicted"
    );
}

#[test]
fn cost_budgets_are_enforced_per_frame() {
    let budget = Budget {
        max_loads: 5,
        max_converge_regions: 3,
        // One drainage job costs 17 after the M4 recalibration
        // (phase-6-plan.md §7.2); 20 admits it plus a cheap layer while
        // still deferring most of a fresh window.
        max_regen_cost: 20,
        max_realize_organisms: usize::MAX,
        max_resonance_nodes: usize::MAX,
        max_persist_ops: usize::MAX,
        max_route_attraction_nodes: usize::MAX,
        max_retarget_regions: usize::MAX,
    };
    let mut map = RegionMap::new(small_config());
    let field = PossibilityField::default();
    let stats = map.update(
        (0.0, 0.0),
        10.0,
        &field,
        &[],
        &NO_BIAS,
        &budget,
        &InlineExecutor,
        false,
    );
    assert!(stats.loaded <= 5);
    assert!(stats.regen_cost_spent <= 20);
    assert!(stats.deferred_loads > 0, "small budget must defer loads");
    assert!(stats.deferred_regens > 0, "small budget must defer regens");
    let stats = map.update(
        (0.0, 0.0),
        10.0,
        &field,
        &[],
        &NO_BIAS,
        &budget,
        &InlineExecutor,
        false,
    );
    assert!(stats.loaded <= 5);
    assert!(stats.converged <= 3);
    assert!(stats.regen_cost_spent <= 20);

    // The budget throttles but never starves: the window must fully settle.
    for _ in 0..600 {
        let stats = map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &budget,
            &InlineExecutor,
            false,
        );
        assert!(stats.regen_cost_spent <= 20);
        if stats.regen_cost_spent == 0 && stats.loaded == 0 && map.jobs_in_flight() == 0 {
            break;
        }
    }
    assert!(map
        .iter_active()
        .all(|r| r.status == GenerationStatus::Ready));
}

#[test]
fn drift_regenerates_declared_readers_and_never_the_stable_trio() {
    let mut map = settled_map((0.0, 0.0));
    let field = PossibilityField::default();

    // Snapshot the stable-trio and expression tiles of a distant (unpinned)
    // region, then push a strong fast-domain bias so its target moves.
    let distant = RegionCoord::new(3, 1);
    let region = map.get(distant).expect("resident");
    assert!(region.stability < 1.0);
    let hash_of = |map: &RegionMap, channel: usize| {
        map.cache()
            .channel(distant, channel)
            .expect("cached")
            .content_hash()
    };
    let terrain_before = hash_of(&map, CHANNEL_ELEVATION);
    let hardness_before = hash_of(&map, CHANNEL_HARDNESS);
    let veg_before = hash_of(&map, CHANNEL_VEGETATION);
    let macro_before = map
        .macro_cache()
        .get(macro_coord_for(distant))
        .expect("macro tile")
        .content_hash();

    let mut bias = NO_BIAS;
    bias[PossibilityDomain::Ecology.index()] = 0.4;
    bias[PossibilityDomain::Hydrology.index()] = 0.4;

    // Convergence is travel-fueled (ADR 0006): with the player stationary,
    // the new target must not be realized anywhere.
    for _ in 0..3 {
        let stats = map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert_eq!(stats.converged, 0, "no travel, no convergence");
    }
    assert_eq!(map.get(distant).expect("resident").revision, 0);

    // Travel (without net displacement — pacing in place still counts as
    // movement to the runtime; the app derives travel from real motion).
    let mut moved = false;
    for _ in 0..6 {
        let stats = map.update(
            (0.0, 0.0),
            25.0,
            &field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        moved |= stats.converged > 0;
    }
    assert!(
        moved,
        "unpinned regions must converge toward the new target"
    );

    let region = map.get(distant).expect("resident");
    assert!(region.revision > 0);
    // Expression layers regenerated with the drifted buckets; the stable trio
    // never moved (section 9; ADR 0007/0009).
    assert_eq!(
        terrain_before,
        hash_of(&map, CHANNEL_ELEVATION),
        "terrain must not regenerate on fast drift"
    );
    assert_eq!(
        hardness_before,
        hash_of(&map, CHANNEL_HARDNESS),
        "geology must not regenerate on fast drift"
    );
    assert_eq!(
        macro_before,
        map.macro_cache()
            .get(macro_coord_for(distant))
            .expect("macro tile")
            .content_hash(),
        "drainage topology must never move"
    );
    assert_ne!(
        veg_before,
        hash_of(&map, CHANNEL_VEGETATION),
        "vegetation must regenerate on ecology drift"
    );
}

#[test]
fn revision_bump_invalidates_the_layer_and_its_dependents_only() {
    let mut map = settled_map((0.0, 0.0));

    let probe = RegionCoord::new(1, 1);
    let before: Vec<Option<u64>> = (0..world_core::LAYER_COUNT)
        .map(|l| map.cache().get(probe).and_then(|t| t.layer_hash(l)))
        .collect();

    map.bump_layer_revision(LAYER_SOILS);
    settle(&mut map, (0.0, 0.0));

    let after: Vec<Option<u64>> = (0..world_core::LAYER_COUNT)
        .map(|l| map.cache().get(probe).and_then(|t| t.layer_hash(l)))
        .collect();
    for layer in [LAYER_TERRAIN, LAYER_GEOLOGY, LAYER_CLIMATE, LAYER_HYDROLOGY] {
        assert_eq!(
            before[layer as usize], after[layer as usize],
            "layer {layer} must not regenerate on a soils revision bump"
        );
    }
    for layer in [LAYER_SOILS, LAYER_BIOME, LAYER_VEGETATION] {
        assert_ne!(
            before[layer as usize], after[layer as usize],
            "layer {layer} must regenerate on a soils revision bump"
        );
    }
}

#[test]
fn zero_macro_target_recovers_hydrology_chain_to_roomy_fixed_point() {
    let player = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    let tight_config = StreamConfig {
        near_radius: 0.1 * REGION_SIZE,
        far_radius: 0.2 * REGION_SIZE,
        load_radius: 0.25 * REGION_SIZE,
        unload_radius: 0.5 * REGION_SIZE,
        field_resolution: 2,
        max_macro_cache_bytes: 0,
        ..StreamConfig::default()
    };
    let mut roomy_config = tight_config;
    roomy_config.max_macro_cache_bytes = usize::MAX;

    let mut tight = RegionMap::new(tight_config);
    let mut roomy = RegionMap::new(roomy_config);
    update_until_current(&mut tight, player, &Budget::unlimited(), 8);
    update_until_current(&mut roomy, player, &Budget::unlimited(), 8);
    assert_eq!(tight.iter_active().count(), 1, "test window must stay tiny");

    // The first idle capacity pass may discard Drainage while leaving the
    // already-fresh Hydrology output and its clean scheduling hints alone.
    let field = PossibilityField::default();
    tight.update(
        player,
        0.0,
        &field,
        &[],
        &NO_BIAS,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    );
    roomy.update(
        player,
        0.0,
        &field,
        &[],
        &NO_BIAS,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    );
    let coord = RegionCoord::new(0, 0);
    let macro_coord = macro_coord_for(coord);
    assert!(tight.macro_cache().get(macro_coord).is_none());
    let diagnostics = tight
        .layer_diagnostics(coord)
        .expect("resident diagnostics");
    assert!(!diagnostics[LAYER_DRAINAGE as usize].dirty);
    assert!(diagnostics[LAYER_DRAINAGE as usize].stored.is_none());
    assert_eq!(
        diagnostics[LAYER_HYDROLOGY as usize].stored,
        diagnostics[LAYER_HYDROLOGY as usize].expected,
        "capacity eviction must leave fresh Hydrology intact"
    );

    let drainage_revision = tight.effective_revision(LAYER_DRAINAGE);
    let hydrology_revision = tight.effective_revision(LAYER_HYDROLOGY);
    tight.bump_layer_revision(LAYER_HYDROLOGY);
    roomy.bump_layer_revision(LAYER_HYDROLOGY);
    assert_eq!(
        tight.effective_revision(LAYER_DRAINAGE),
        drainage_revision,
        "the reproduction must invalidate Hydrology only"
    );
    assert_eq!(
        tight.effective_revision(LAYER_HYDROLOGY),
        hydrology_revision.wrapping_add(1)
    );
    let diagnostics = tight
        .layer_diagnostics(coord)
        .expect("resident diagnostics");
    assert!(!diagnostics[LAYER_DRAINAGE as usize].dirty);
    assert!(diagnostics[LAYER_HYDROLOGY as usize].dirty);

    // Seventeen units admit exactly the missing macro job in the first
    // recovery frame. Hold it in a manual queue so its result reaches the
    // integrator at the start of the next update, immediately before the
    // zero-target capacity pass. Hydrology is still dirty at that pass, so
    // the just-integrated macro must survive long enough to be snapshotted.
    let finite_budget = Budget {
        max_regen_cost: 17,
        ..Budget::unlimited()
    };
    let executor = ManualExecutor::default();
    let queued_stats = tight.update(
        player,
        0.0,
        &field,
        &[],
        &NO_BIAS,
        &finite_budget,
        &executor,
        false,
    );
    assert_eq!(queued_stats.regen_cost_spent, 17);
    assert_eq!(executor.queue_len(), 1);
    assert_eq!(tight.jobs_in_flight(), 1);
    assert!(tight.macro_cache().get(macro_coord).is_none());
    executor.run_next();
    assert!(
        tight.macro_cache().get(macro_coord).is_none(),
        "workers never write the cache directly"
    );

    executor.run_submissions_inline();
    let recovered_stats = tight.update(
        player,
        0.0,
        &field,
        &[],
        &NO_BIAS,
        &finite_budget,
        &executor,
        false,
    );
    assert!(is_current_fixed_point(&tight));
    assert_eq!(executor.queue_len(), 0);
    assert!(
        tight.macro_cache().get(macro_coord).is_some(),
        "demanded macro must cross the capacity pass before Hydrology snapshots it"
    );

    let mut regenerated = queued_stats.regenerated_by_layer;
    for (total, frame) in regenerated
        .iter_mut()
        .zip(recovered_stats.regenerated_by_layer)
    {
        *total += frame;
    }
    for layer in [
        LAYER_DRAINAGE,
        LAYER_HYDROLOGY,
        LAYER_SOILS,
        LAYER_BIOME,
        LAYER_VEGETATION,
        LAYER_ECOLOGY,
    ] {
        assert!(
            regenerated[layer as usize] > 0,
            "layer {layer} made no recovery progress"
        );
    }

    update_until_current(&mut roomy, player, &finite_budget, 8);
    assert_eq!(tight.jobs_in_flight(), 0);
    assert!(tight
        .iter_active()
        .all(|region| region.status == GenerationStatus::Ready && region.dirty_layers == 0));
    // Capture this demanded fixed point now: a later idle update is allowed to
    // evict Drainage again under the zero target.
    assert_eq!(world_fingerprint(&tight), world_fingerprint(&roomy));
}

#[test]
fn dispatch_is_topological_under_tiny_budgets() {
    // With a budget so small only one job fits per frame, layers must still
    // appear strictly bottom-up: no layer generates before its inputs.
    let budget = Budget {
        max_loads: usize::MAX,
        max_converge_regions: usize::MAX,
        max_regen_cost: 17, // exactly one drainage job (M4 costs), or a few cheap layers
        max_realize_organisms: usize::MAX,
        max_resonance_nodes: usize::MAX,
        max_persist_ops: usize::MAX,
        max_route_attraction_nodes: usize::MAX,
        max_retarget_regions: usize::MAX,
    };
    let mut map = RegionMap::new(StreamConfig {
        load_radius: 1.0 * REGION_SIZE,
        unload_radius: 2.0 * REGION_SIZE,
        ..small_config()
    });
    let field = PossibilityField::default();
    for _ in 0..200 {
        map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &budget,
            &InlineExecutor,
            false,
        );
        // Invariant: whenever a layer's tiles exist, its inputs' tiles exist
        // and carry exactly the hashes folded into the layer's dep hash —
        // checked indirectly: an output present implies inputs present.
        for region in map.iter_active() {
            let Some(tiles) = map.cache().get(region.coord) else {
                continue;
            };
            for layer in 0..world_core::LAYER_COUNT {
                if tiles.layer_hash(layer).is_none() {
                    continue;
                }
                for &dep in world_core::layer_decl(layer).deps {
                    if dep == LAYER_DRAINAGE {
                        assert!(
                            map.macro_cache()
                                .get(macro_coord_for(region.coord))
                                .is_some(),
                            "layer {layer} exists without its macro input"
                        );
                    } else {
                        assert!(
                            tiles.layer_hash(dep).is_some(),
                            "layer {layer} exists without input {dep}"
                        );
                    }
                }
            }
        }
        if map.iter_active().count() > 0
            && map
                .iter_active()
                .all(|r| r.status == GenerationStatus::Ready)
        {
            return; // settled bottom-up under the tiny budget
        }
    }
    panic!("window never settled under the tiny budget");
}

#[test]
fn river_expression_reads_the_macro_topology() {
    // Hydrology tiles must exist and reflect drainage: somewhere in the window
    // a river cell should express (the fixture window spans a full macro
    // catchment, so a channel is statistically certain; if this ever flakes
    // the window moved — pick a different origin).
    let map = settled_map((0.0, 0.0));
    let mut max_river = 0.0f32;
    for region in map.iter_active() {
        if let Some(tile) = map.cache().channel(region.coord, CHANNEL_RIVER) {
            for &v in tile.samples() {
                max_river = max_river.max(v);
            }
        }
    }
    assert!(
        max_river > 0.05,
        "no river expression anywhere in the window (max {max_river})"
    );
}

#[test]
fn near_field_organisms_are_stable_and_preserve_the_aggregate() {
    // M4 exit (phase-3-plan.md §16): near-field organism counts preserve the
    // aggregate, and organism ids are stable across frames and a two-run replay.
    let field = PossibilityField::default();
    let run = || {
        let mut map = RegionMap::new(small_config());
        for _ in 0..8 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &NO_BIAS,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        map
    };
    let map = run();
    // The pinned near region hosts organisms.
    let near = RegionCoord::new(0, 0);
    let organisms = map.organisms_in(near).expect("near region realized");
    assert!(!organisms.is_empty(), "near region should host organisms");

    // Coverage preserves the aggregate: organism count ≈ sum of vegetation
    // density over the region's cells (one per cell with probability = density).
    let res = small_config().field_resolution;
    let veg = map
        .cache()
        .channel(near, CHANNEL_VEGETATION)
        .expect("veg tile");
    let expected: f32 = (0..res)
        .flat_map(|cy| (0..res).map(move |cx| (cx, cy)))
        .map(|(cx, cy)| veg.get(cx, cy))
        .sum();
    let realized = organisms.len() as f32;
    assert!(
        (realized - expected).abs() <= expected.max(4.0) * 0.6 + 8.0,
        "realized {realized} far from aggregate {expected}"
    );

    // Ids are stable across further frames while the region stays pinned.
    let ids_before: Vec<u64> = organisms.iter().map(|o| o.id).collect();
    let mut map = map;
    for _ in 0..5 {
        map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
    }
    let ids_after: Vec<u64> = map
        .organisms_in(near)
        .unwrap()
        .iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(
        ids_before, ids_after,
        "pinned organism ids must not flicker"
    );

    // Two-run replay: an independent run realizes bit-identical organisms.
    let other = run();
    let a: Vec<_> = map.organisms_in(near).unwrap().to_vec();
    let b: Vec<_> = other.organisms_in(near).unwrap().to_vec();
    assert_eq!(a, b, "two runs must realize identical organisms");
}

#[test]
fn zero_roster_target_preserves_resident_ecology_and_realization() {
    let initial_player = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    let tight_config = StreamConfig {
        near_radius: 0.25 * REGION_SIZE,
        far_radius: 0.75 * REGION_SIZE,
        load_radius: 1.05 * REGION_SIZE,
        unload_radius: 2.25 * REGION_SIZE,
        field_resolution: 4,
        max_roster_cache_bytes: 0,
        ..StreamConfig::default()
    };
    let mut roomy_config = tight_config;
    roomy_config.max_roster_cache_bytes = usize::MAX;
    let mut tight = RegionMap::new(tight_config);
    let mut roomy = RegionMap::new(roomy_config);
    update_until_current(&mut tight, initial_player, &Budget::unlimited(), 8);
    update_until_current(&mut roomy, initial_player, &Budget::unlimited(), 8);

    let approached = RegionCoord::new(1, 0);
    assert!(
        tight.get(approached).is_some(),
        "approach target must be resident"
    );
    assert!(
        tight.organisms_in(approached).is_none(),
        "approach target must initially be outside the near window"
    );

    let initial_required = resident_cell_signatures(&tight);
    assert!(
        initial_required.len() > 1,
        "fixture must exercise multiple resident habitat signatures"
    );
    let initial_floor = assert_roster_floor(&tight, &initial_required);
    assert!(
        initial_floor > tight.config().max_roster_cache_bytes,
        "required roster working set must exceed the zero-byte target"
    );

    // Repeated idle updates each run the capacity pass. The protected cache
    // must stabilize at its indispensable floor instead of rebuilding or
    // shedding an entry needed by a settled L8 cell.
    let field = PossibilityField::default();
    for _ in 0..4 {
        let tight_stats = tight.update(
            initial_player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        roomy.update(
            initial_player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert_eq!(tight_stats.rosters_built, 0, "protected rosters churned");
        assert_eq!(tight_stats.roster_cache_bytes, initial_floor);
        assert_eq!(resident_cell_signatures(&tight), initial_required);
        assert_eq!(
            assert_roster_floor(&tight, &initial_required),
            initial_floor
        );
    }

    // Move the already-settled cardinal neighbor into the near window without
    // unloading it. Realization must read the retained roster set and match a
    // roomy-cache replay exactly, including ids, species, and expressions.
    let (ox, oy) = approached.origin();
    let approached_player = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
    update_until_current(&mut tight, approached_player, &Budget::unlimited(), 8);
    update_until_current(&mut roomy, approached_player, &Budget::unlimited(), 8);
    let tight_organisms = tight
        .organisms_in(approached)
        .expect("approached resident realized")
        .to_vec();
    let roomy_organisms = roomy
        .organisms_in(approached)
        .expect("roomy approached resident realized")
        .to_vec();
    assert!(
        !tight_organisms.is_empty(),
        "fixture must realize at least one organism"
    );
    assert_eq!(
        tight_organisms, roomy_organisms,
        "zero roster target changed organism ids, species, or expressions"
    );
    for organism in &tight_organisms {
        let signature = tight
            .cell_signature(approached, organism.cell.cx, organism.cell.cy)
            .expect("organism cell signature");
        let roster = tight
            .roster_cache()
            .get(signature)
            .expect("organism cell roster");
        assert!(
            roster
                .roster
                .species
                .iter()
                .any(|species| species.id == organism.species),
            "organism species is absent from its retained cell roster"
        );
    }

    let final_required = resident_cell_signatures(&tight);
    assert_eq!(final_required, resident_cell_signatures(&roomy));
    let final_floor = assert_roster_floor(&tight, &final_required);
    assert!(final_floor > tight.config().max_roster_cache_bytes);
    for &signature in &final_required {
        assert_eq!(
            tight
                .roster_cache()
                .get(signature)
                .expect("tight roster")
                .as_ref(),
            roomy
                .roster_cache()
                .get(signature)
                .expect("roomy roster")
                .as_ref(),
            "roster content differs for {signature:?}"
        );
    }
    assert_eq!(
        world_fingerprint(&tight),
        world_fingerprint(&roomy),
        "zero roster target changed L8 dependency or content hashes"
    );

    for _ in 0..4 {
        let tight_stats = tight.update(
            approached_player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        roomy.update(
            approached_player,
            0.0,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        assert_eq!(tight_stats.rosters_built, 0, "protected rosters churned");
        assert_eq!(tight_stats.roster_cache_bytes, final_floor);
        assert_eq!(resident_cell_signatures(&tight), final_required);
        assert_eq!(assert_roster_floor(&tight, &final_required), final_floor);
        assert_eq!(
            tight.organisms_in(approached),
            roomy.organisms_in(approached),
            "realized organisms changed on a later capacity pass"
        );
    }

    // The public cache policy still removes an unprotected entry at a zero
    // target; protection is a working-set floor, not a blanket no-eviction
    // mode. RegionMap intentionally exposes its live cache read-only, so use a
    // standalone public cache to exercise the mutation contract.
    let protected_signature = *final_required.iter().next().expect("required signature");
    let disposable_signature = unused_valid_signature(&final_required);
    let mut capacity_probe = RosterCache::default();
    let protected_entry = capacity_probe.ensure(protected_signature);
    capacity_probe.ensure(disposable_signature);
    let protected: BTreeSet<_> = [protected_signature].into_iter().collect();
    let eviction = capacity_probe.evict_to_bytes(0, &protected);
    assert_eq!(eviction.entries_removed, 1);
    assert!(eviction.bytes_removed > 0);
    assert_eq!(
        capacity_probe
            .get(protected_signature)
            .expect("protected entry retained")
            .as_ref(),
        protected_entry.as_ref()
    );
    assert!(capacity_probe.get(disposable_signature).is_none());
}

#[test]
fn staleness_is_tracked_per_tile_by_dep_hash() {
    let map = settled_map((0.0, 0.0));
    for region in map.iter_active() {
        if region.status != GenerationStatus::Ready {
            continue;
        }
        let tiles = map.cache().get(region.coord).expect("cached");
        // A settled region's biome and channel tiles share their layer's
        // dependency hash; different layers have different hashes.
        let climate = tiles.layer_hash(LAYER_CLIMATE).unwrap();
        let veg = tiles.layer_hash(LAYER_VEGETATION).unwrap();
        assert_ne!(climate, veg);
        let biome = tiles.biome.as_ref().expect("biome tile");
        assert_eq!(tiles.layer_hash(LAYER_BIOME), Some(biome.dep_hash));
    }
}

/// Phase 6 (§6.4): under unchanged steering the retarget pass round-robins
/// `max_retarget_regions` per frame (deferral reported), while any steering
/// change refreshes the whole window that same frame — dirty-first.
#[test]
fn retarget_amortizes_and_refreshes_on_steering_change() {
    use world_runtime::InlineExecutor;
    let field = world_core::PossibilityField::default();
    let mut map = RegionMap::new(small_config());
    let neutral = [0.0f32; world_core::POSSIBILITY_DIMS];
    let budget = Budget {
        max_retarget_regions: 1,
        ..Budget::unlimited()
    };
    // Settle a window (steering unchanged after the first frame).
    let mut stats = world_runtime::FrameStats::default();
    for _ in 0..6 {
        stats = map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &[],
            &neutral,
            &budget,
            &InlineExecutor,
            false,
        );
    }
    // Amortized: all but one region deferred.
    assert!(stats.active_regions > 2);
    assert_eq!(stats.retarget_deferred, stats.active_regions - 1);

    // A bias change forces a full refresh this frame: no deferral.
    let mut bias = neutral;
    bias[world_core::PossibilityDomain::Ecology.index()] = 0.3;
    let stats = map.update(
        (0.0, 0.0),
        0.0,
        &field,
        &[],
        &bias,
        &budget,
        &InlineExecutor,
        false,
    );
    assert_eq!(stats.retarget_deferred, 0);
    // The refresh took effect: some unpinned region's target moved up in
    // Ecology versus its raw field sample (the plausibility projection may
    // damp individual regions, so the assertion is existential).
    let moved = map.iter_active().any(|region| {
        region.stability < 1.0
            && region.target.get(world_core::PossibilityDomain::Ecology)
                > field
                    .sample(region.coord)
                    .get(world_core::PossibilityDomain::Ecology)
    });
    assert!(moved, "bias change did not reach any target this frame");
}
