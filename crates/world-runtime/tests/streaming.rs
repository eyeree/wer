//! Unit tests for the streaming window (phase-2-plan.md §12.4): load/evict
//! hysteresis, stability ramp shape, cost-budget enforcement, topological
//! dispatch, dep-hash staleness precision, and macro cache lifecycle.

use world_core::layer::{
    LAYER_BIOME, LAYER_CLIMATE, LAYER_DRAINAGE, LAYER_GEOLOGY, LAYER_HYDROLOGY, LAYER_SOILS,
    LAYER_TERRAIN, LAYER_VEGETATION,
};
use world_core::{
    macro_coord_for, PossibilityDomain, PossibilityField, RegionCoord, POSSIBILITY_DIMS,
    REGION_SIZE,
};
use world_runtime::{
    stability_for, Budget, GenerationStatus, InlineExecutor, RegionMap, StreamConfig,
    CHANNEL_ELEVATION, CHANNEL_HARDNESS, CHANNEL_RIVER, CHANNEL_VEGETATION,
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
        max_regen_cost: 12,
        max_realize_organisms: usize::MAX,
        max_resonance_nodes: usize::MAX,
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
    assert!(stats.regen_cost_spent <= 12);
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
    assert!(stats.regen_cost_spent <= 12);

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
        assert!(stats.regen_cost_spent <= 12);
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
fn dispatch_is_topological_under_tiny_budgets() {
    // With a budget so small only one job fits per frame, layers must still
    // appear strictly bottom-up: no layer generates before its inputs.
    let budget = Budget {
        max_loads: usize::MAX,
        max_converge_regions: usize::MAX,
        max_regen_cost: 10, // one drainage job, or a few cheap layers
        max_realize_organisms: usize::MAX,
        max_resonance_nodes: usize::MAX,
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
