//! Unit tests for the Phase 1 streaming window (phase-1-plan.md section 11.4):
//! load/evict hysteresis, stability ramp shape, budget enforcement, and the
//! narrowed dirty-layer policy.

use world_core::layer::{layer_bit, LAYER_CLIMATE, LAYER_ECOLOGY, LAYER_TERRAIN};
use world_core::{PossibilityDomain, PossibilityField, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{
    stability_for, Budget, GenerationStatus, InlineExecutor, RegionMap, RegionState, StreamConfig,
};

const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

fn small_config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 3.0 * REGION_SIZE,
        load_radius: 4.0 * REGION_SIZE,
        unload_radius: 5.0 * REGION_SIZE,
        converge_rate: 0.25,
        field_resolution: 4,
    }
}

fn settled_map(player: (f64, f64)) -> RegionMap {
    let mut map = RegionMap::new(small_config());
    let field = PossibilityField::default();
    for _ in 0..8 {
        map.update(
            player,
            &field,
            &[],
            &NO_BIAS,
            &Budget::unlimited(),
            &InlineExecutor,
        );
    }
    map
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
fn window_loads_within_radius_and_snaps_fresh_regions() {
    let map = settled_map((0.0, 0.0));
    assert!(!map.is_empty());
    for region in map.iter_active() {
        let (ox, oy) = region.coord.origin();
        let d = ((ox + REGION_SIZE * 0.5).powi(2) + (oy + REGION_SIZE * 0.5).powi(2)).sqrt();
        assert!(d <= small_config().load_radius);
        // Fresh regions realize their target immediately (no initial pop) and
        // finish generating under an unlimited budget.
        assert_eq!(region.current, region.target);
        assert_eq!(region.status, GenerationStatus::Ready);
        assert!(map.cache().get(region.coord).is_some());
    }
}

#[test]
fn eviction_has_hysteresis() {
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
        &field,
        &[],
        &NO_BIAS,
        &Budget::unlimited(),
        &InlineExecutor,
    );
    assert!(
        map.get(edge).is_some(),
        "hysteresis should retain the region"
    );

    // Move well past unload_radius: now it must be evicted with its tiles.
    let player = (-(4.0 * REGION_SIZE), 0.0);
    map.update(
        player,
        &field,
        &[],
        &NO_BIAS,
        &Budget::unlimited(),
        &InlineExecutor,
    );
    assert!(map.get(edge).is_none());
    assert!(map.cache().get(edge).is_none());
}

#[test]
fn budgets_are_enforced_per_frame() {
    let budget = Budget {
        max_loads: 5,
        max_converge_regions: 3,
        max_regen_layers: 4,
    };
    let mut map = RegionMap::new(small_config());
    let field = PossibilityField::default();
    let stats = map.update((0.0, 0.0), &field, &[], &NO_BIAS, &budget, &InlineExecutor);
    assert!(stats.loaded <= 5);
    assert!(stats.layers_dispatched <= 4);
    assert!(stats.deferred_loads > 0, "small budget must defer loads");
    let stats = map.update((0.0, 0.0), &field, &[], &NO_BIAS, &budget, &InlineExecutor);
    assert!(stats.loaded <= 5);
    assert!(stats.converged <= 3);
}

#[test]
fn drift_dirties_climate_and_ecology_but_never_terrain() {
    let mut region = RegionState::new(RegionCoord::new(0, 0));
    region.stability = 0.0;
    region.dirty_layers = 0;
    region.target.set(PossibilityDomain::Climate, 0.9);
    assert!(region.converge(0.5));
    assert_ne!(region.dirty_layers & layer_bit(LAYER_CLIMATE), 0);
    assert_ne!(region.dirty_layers & layer_bit(LAYER_ECOLOGY), 0);
    assert_eq!(
        region.dirty_layers & layer_bit(LAYER_TERRAIN),
        0,
        "possibility drift must never dirty terrain (phase-1-plan.md §6.4)"
    );
    assert_eq!(region.revision, 1);
}

#[test]
fn pinned_regions_never_move() {
    let mut region = RegionState::new(RegionCoord::new(0, 0));
    region.stability = 1.0;
    region.target.set(PossibilityDomain::Ecology, 1.0);
    let before = region.current;
    assert!(!region.converge(1.0));
    assert_eq!(region.current, before);
    assert_eq!(region.revision, 0);
    assert_eq!(region.dirty_layers, 0);
}

#[test]
fn distant_regions_converge_and_regenerate_only_drift_layers() {
    let mut map = settled_map((0.0, 0.0));
    let field = PossibilityField::default();

    // Snapshot the terrain tiles of a distant (unpinned) region, then push a
    // strong global bias so its target moves.
    let distant = RegionCoord::new(3, 1);
    let region = map.get(distant).expect("resident");
    assert!(region.stability < 1.0);
    let terrain_before = map
        .cache()
        .channel(distant, world_runtime::CHANNEL_ELEVATION)
        .expect("terrain generated")
        .content_hash();
    let veg_before = map
        .cache()
        .channel(distant, world_runtime::CHANNEL_VEGETATION)
        .expect("vegetation generated")
        .content_hash();

    let mut bias = NO_BIAS;
    bias[PossibilityDomain::Ecology.index()] = 0.4;
    bias[PossibilityDomain::Hydrology.index()] = 0.4;
    let mut moved = false;
    for _ in 0..6 {
        let stats = map.update(
            (0.0, 0.0),
            &field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
        );
        moved |= stats.converged > 0;
    }
    assert!(
        moved,
        "unpinned regions must converge toward the new target"
    );

    let region = map.get(distant).expect("resident");
    assert!(region.revision > 0);
    // Ecology regenerated with the drifted state; terrain untouched.
    let terrain_after = map
        .cache()
        .channel(distant, world_runtime::CHANNEL_ELEVATION)
        .expect("terrain cached")
        .content_hash();
    let veg_after = map
        .cache()
        .channel(distant, world_runtime::CHANNEL_VEGETATION)
        .expect("vegetation cached")
        .content_hash();
    assert_eq!(
        terrain_before, terrain_after,
        "terrain must not regenerate on drift"
    );
    assert_ne!(veg_before, veg_after, "ecology must regenerate on drift");
}

#[test]
fn staleness_is_tracked_per_tile() {
    let map = settled_map((0.0, 0.0));
    for region in map.iter_active() {
        if region.status != GenerationStatus::Ready {
            continue;
        }
        let tiles = map.cache().get(region.coord).expect("cached");
        for tile in tiles.channels.iter().flatten() {
            // Terrain tiles may carry an older revision (they are not
            // regenerated on drift); drift-layer tiles must be current.
            assert!(!tile.is_stale(world_core::WORLD_ALGORITHM_VERSION, tile.revision));
        }
        let veg = tiles.channels[world_runtime::CHANNEL_VEGETATION]
            .as_ref()
            .expect("vegetation cached");
        assert!(!veg.is_stale(world_core::WORLD_ALGORITHM_VERSION, region.revision));
    }
}
