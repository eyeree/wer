//! Routes end-to-end (phase-5-plan.md §7.3–7.4, §12.3; the M5 exit criteria):
//! a journey recorded through the real streaming pipeline persists, and its
//! attraction field bends targets near the corridor — softly, corridor-
//! bounded — through the unchanged steering machinery, surviving a vault
//! round trip.

use world_core::{
    anchor_influence_profile, attraction_anchors, encode_record, PossibilityField, RecordKind,
    RegionCoord, POSSIBILITY_DIMS, REGION_SIZE, ROUTE_CORRIDOR_RADIUS, ROUTE_PULL_CAP,
};
use world_runtime::{
    Budget, InlineExecutor, MemoryStorage, RegionMap, RouteRecorder, StreamConfig, Vault,
};

fn config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 3.0 * REGION_SIZE,
        load_radius: 4.0 * REGION_SIZE,
        unload_radius: 5.0 * REGION_SIZE,
        field_resolution: 8,
        ..StreamConfig::default()
    }
}

fn step(map: &mut RegionMap, player: (f64, f64), travel: f64) -> world_runtime::FrameStats {
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    map.update(
        player,
        travel,
        &field,
        &[],
        &bias,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    )
}

#[test]
fn a_recorded_route_persists_and_attracts_softly_within_its_corridor() {
    // Record a journey with the real recorder over the real pipeline.
    let mut map = RegionMap::new(config());
    let mut recorder = RouteRecorder::new();
    let mut player = (128.0, 128.0);
    let stats = step(&mut map, player, 0.0);
    recorder.observe(&map, player, 0.0, &[], stats.resonance_strength);
    for _ in 0..24 {
        player.0 += 100.0;
        let stats = step(&mut map, player, 100.0);
        recorder.observe(&map, player, 100.0, &[], stats.resonance_strength);
    }
    let (nodes, discoveries) = recorder.finish();
    assert_eq!(
        nodes.len(),
        13,
        "initial node plus every crossed 192-unit interval over 2400 units"
    );
    assert_eq!(nodes[0].distance_q, 0);
    for node in &nodes[1..] {
        assert_eq!(node.distance_q, 192);
        assert!(node.current_signature.is_some());
    }

    // Persist it and reopen — the record survives, difficulty is defined.
    let mut vault = Vault::open(MemoryStorage::new()).unwrap();
    let id = vault
        .record_route(nodes, discoveries, "trek".into())
        .unwrap();
    let encoded_before = encode_record(RecordKind::Route, &vault.routes()[&id]);
    vault.flush_all().unwrap();
    let mut vault = Vault::open(vault.store().clone()).unwrap();
    let route = vault.routes()[&id].clone();
    assert_eq!(encode_record(RecordKind::Route, &route), encoded_before);
    let difficulty = world_core::route_difficulty(&route.nodes);
    assert!((0.0..=1.0).contains(&difficulty));

    // Follow it in a fresh world: near the corridor the steered target
    // differs from the unattracted one; far from it, nothing changes.
    vault.bump_route_usage(id);
    let route = vault.routes()[&id].clone();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let on_route = (route.nodes[4].pos_q.0 as f64, route.nodes[4].pos_q.1 as f64);
    let far_off = (on_route.0, on_route.1 + 30_000.0);

    for (at, expect_pull) in [(on_route, true), (far_off, false)] {
        let anchors = attraction_anchors([&route], at, 32);
        assert_eq!(!anchors.is_empty(), expect_pull, "corridor bound at {at:?}");
        if expect_pull {
            let mut probes = vec![at];
            probes.extend(anchors.iter().map(|anchor| anchor.world_pos));
            for probe in probes {
                assert!(
                    anchor_influence_profile(&anchors, probe)
                        .into_iter()
                        .all(|pull| pull <= ROUTE_PULL_CAP),
                    "combined route channel exceeded its global cap at {probe:?}"
                );
            }
        }
        let mut plain = RegionMap::new(config());
        let mut pulled = RegionMap::new(config());
        for _ in 0..2 {
            plain.update(
                at,
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
            pulled.update(
                at,
                0.0,
                &field,
                &anchors,
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        let coord = RegionCoord::from_world(at.0, at.1);
        let plain_target = plain.get(coord).unwrap().target;
        let pulled_target = pulled.get(coord).unwrap().target;
        if expect_pull {
            assert_ne!(
                plain_target.dims, pulled_target.dims,
                "the corridor must bend the target"
            );
            // Soft: every domain stays strictly inside [0, 1] motion bounded
            // by the pull cap — the route biases, it never replaces.
            for i in 0..POSSIBILITY_DIMS {
                let moved = (pulled_target.dims[i] - plain_target.dims[i]).abs();
                assert!(
                    moved < 0.5,
                    "domain {i} moved {moved}, far beyond a soft pull"
                );
            }
        } else {
            assert_eq!(
                plain_target.dims, pulled_target.dims,
                "beyond the corridor the route must not steer at all"
            );
        }
    }
}

#[test]
fn multi_node_route_attracts_softly_near_distinct_segments() {
    let mut map = RegionMap::new(config());
    let mut recorder = RouteRecorder::new();
    let mut player = (128.0, 128.0);
    let stats = step(&mut map, player, 0.0);
    recorder.observe(&map, player, 0.0, &[], stats.resonance_strength);
    for i in 1..=30 {
        let next = if i <= 10 {
            (128.0 + f64::from(i) * 96.0, 128.0)
        } else if i <= 20 {
            (1088.0, 128.0 + f64::from(i - 10) * 96.0)
        } else {
            (1088.0 + f64::from(i - 20) * 96.0, 1088.0)
        };
        let travel = f64::hypot(next.0 - player.0, next.1 - player.1);
        player = next;
        let stats = step(&mut map, player, travel);
        recorder.observe(&map, player, travel, &[], stats.resonance_strength);
    }
    let (nodes, discoveries) = recorder.finish();
    assert!(
        nodes.len() >= 10,
        "fixture must produce several route nodes"
    );

    let mut vault = Vault::open(MemoryStorage::new()).unwrap();
    let id = vault
        .record_route(nodes, discoveries, "multi-node trek".into())
        .unwrap();
    for _ in 0..3 {
        vault.bump_route_usage(id);
    }
    vault.flush_all().unwrap();
    let vault = Vault::open(vault.store().clone()).unwrap();
    let route = vault.routes()[&id].clone();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];

    let node_pos = |index: usize| {
        (
            route.nodes[index].pos_q.0 as f64,
            route.nodes[index].pos_q.1 as f64,
        )
    };
    let midpoint = |a: usize, b: usize| {
        let a = node_pos(a);
        let b = node_pos(b);
        ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
    };
    let probes = [
        ("early node", node_pos(1), true),
        (
            "turn segment",
            midpoint(route.nodes.len() / 3, route.nodes.len() / 3 + 1),
            true,
        ),
        ("late node", node_pos(route.nodes.len() - 2), true),
        (
            "outside corridor",
            (node_pos(1).0, node_pos(1).1 + ROUTE_CORRIDOR_RADIUS + 1.0),
            false,
        ),
        ("far off", (node_pos(1).0, node_pos(1).1 + 30_000.0), false),
    ];

    for (label, at, expect_pull) in probes {
        let anchors = attraction_anchors([&route], at, 32);
        let anchors_again = attraction_anchors([&route], at, 32);
        assert_eq!(
            anchors, anchors_again,
            "route anchor ordering must be deterministic at {label}"
        );
        assert_eq!(
            !anchors.is_empty(),
            expect_pull,
            "corridor bound at {label}: {at:?}"
        );
        assert!(
            anchor_influence_profile(&anchors, at)
                .into_iter()
                .all(|pull| pull <= ROUTE_PULL_CAP),
            "route pull cap exceeded at {label}"
        );

        let mut plain = RegionMap::new(config());
        let mut pulled = RegionMap::new(config());
        for _ in 0..2 {
            plain.update(
                at,
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
            pulled.update(
                at,
                0.0,
                &field,
                &anchors,
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        let coord = RegionCoord::from_world(at.0, at.1);
        let plain_target = plain.get(coord).unwrap().target;
        let pulled_target = pulled.get(coord).unwrap().target;
        if expect_pull {
            assert_ne!(plain_target.dims, pulled_target.dims, "{label}");
            for i in 0..POSSIBILITY_DIMS {
                let moved = (pulled_target.dims[i] - plain_target.dims[i]).abs();
                assert!(
                    moved < ROUTE_PULL_CAP + 0.05,
                    "{label}: domain {i} moved {moved}, beyond soft route pull"
                );
            }
            for anchor in &anchors {
                assert_ne!(
                    pulled_target.dims, anchor.target.dims,
                    "{label}: route must not replace the target outright"
                );
            }
        } else {
            assert_eq!(plain_target.dims, pulled_target.dims, "{label}");
        }
    }
}
