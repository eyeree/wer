//! Preserves (phase-5-plan.md §7.5, §12.2–12.3; the M4 exit criteria): a
//! preserved window holds bit-identical tiles across travel, steering, and
//! eviction/reload while unpreserved neighbours keep transforming; an
//! imported preserve realizes the identical buckets and tiles in a fresh
//! world; and deleting a preserve produces no snap.

use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, PossibilityDomain,
    PossibilityField, PossibilitySignature, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    Budget, InlineExecutor, MemoryStorage, RegionMap, StreamConfig, Vault, CHANNEL_COUNT,
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

/// A strong wide anchor pushing the fast domains — pressure a preserve must
/// hold against.
fn steering_anchor() -> Anchor {
    let mask = domain_mask(&[
        PossibilityDomain::Ecology,
        PossibilityDomain::Hydrology,
        PossibilityDomain::Climate,
    ]);
    Anchor {
        world_pos: (0.0, 0.0),
        target: bound_target(mask, 1.0),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.9,
        falloff_radius: 4000.0,
        source: AnchorSource::Manual,
    }
}

fn settle(map: &mut RegionMap, player: (f64, f64), travel: f64, anchors: &[Anchor]) {
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    map.update(
        player,
        travel,
        &field,
        anchors,
        &bias,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    );
}

/// Every generated channel hash of one region (None entries skipped).
fn region_tile_hashes(map: &RegionMap, coord: RegionCoord) -> Vec<(usize, u64)> {
    let mut hashes = Vec::new();
    for channel in 0..CHANNEL_COUNT {
        if let Some(tile) = map.cache().channel(coord, channel) {
            hashes.push((channel, tile.content_hash()));
        }
    }
    assert!(!hashes.is_empty(), "region has no tiles yet");
    hashes
}

#[test]
fn a_preserve_holds_under_steering_and_eviction_and_releases_without_a_snap() {
    let target = RegionCoord::new(0, 0);
    let mut map = RegionMap::new(config());
    for _ in 0..4 {
        settle(&mut map, (128.0, 128.0), 0.0, &[]);
    }

    // Preserve the settled center region from its own state: flips no bucket,
    // so its tiles hold bit-identical from this exact moment.
    let sig = PossibilitySignature::of(map.get(target).expect("resident").current);
    map.set_override(target, sig);
    settle(&mut map, (128.0, 128.0), 0.0, &[]);
    let preserved_hashes = region_tile_hashes(&map, target);

    // Travel around under a strong anchor: neighbours transform, the
    // preserve does not.
    let far = RegionCoord::new(2, 2);
    let far_before = map.get(far).expect("resident").current;
    let anchors = [steering_anchor()];
    for step in 1..=20 {
        let along = f64::from(step) * 40.0;
        settle(
            &mut map,
            (128.0 + along, 128.0 + along * 0.5),
            60.0,
            &anchors,
        );
        assert_eq!(
            region_tile_hashes(&map, target),
            preserved_hashes,
            "preserved tiles changed at step {step}"
        );
        assert_eq!(map.get(target).unwrap().stability, 1.0);
    }
    let far_after = map.get(far).map(|r| r.current);
    if let Some(after) = far_after {
        assert_ne!(
            far_before.dims, after.dims,
            "unpreserved neighbour should have transformed under travel + anchor"
        );
    }

    // Walk far enough that the preserved region evicts, then come back: it
    // reloads from its buckets and regenerates the identical tiles.
    for step in 0..30 {
        let away = 128.0 + 800.0 + f64::from(step) * 60.0;
        settle(&mut map, (away, away), 60.0, &anchors);
    }
    assert!(
        map.get(target).is_none(),
        "preserved region should have evicted"
    );
    for step in (0..30).rev() {
        let back = 128.0 + 800.0 + f64::from(step) * 60.0;
        settle(&mut map, (back, back), 60.0, &anchors);
    }
    for _ in 0..4 {
        settle(&mut map, (128.0, 128.0), 60.0, &anchors);
    }
    assert_eq!(
        region_tile_hashes(&map, target),
        preserved_hashes,
        "preserve did not survive eviction + reload"
    );

    // Delete the preserve: no snap — the region resumes steering from where
    // the preserve held it, converging gradually (bounded per-frame motion).
    let held = map.get(target).unwrap().current;
    map.clear_override(target);
    let cap = map.config().converge_rate_cap;
    let mut moved_total = 0.0f32;
    let mut previous = held;
    for _ in 0..10 {
        settle(&mut map, (700.0, 700.0), 60.0, &anchors);
        let Some(region) = map.get(target) else { break };
        let now = region.current;
        for i in 0..POSSIBILITY_DIMS {
            let step = (now.dims[i] - previous.dims[i]).abs();
            assert!(
                step <= cap + 1e-6,
                "post-delete step {step} exceeds the convergence cap {cap}"
            );
            moved_total += step;
        }
        previous = now;
    }
    assert!(
        moved_total > 0.0,
        "released region should resume converging toward the steered target"
    );
}

#[test]
fn an_imported_preserve_realizes_identical_tiles_in_a_fresh_world() {
    let target = RegionCoord::new(0, 0);

    // World A: settle, steer a little so the center is off the pure field
    // value, preserve it, and export the bundle.
    let mut map_a = RegionMap::new(config());
    let anchors = [steering_anchor()];
    settle(&mut map_a, (128.0, 128.0), 0.0, &[]);
    for step in 1..=6 {
        settle(
            &mut map_a,
            (128.0 + f64::from(step) * 30.0, 128.0),
            45.0,
            &anchors,
        );
    }
    for _ in 0..2 {
        settle(&mut map_a, (128.0, 128.0), 30.0, &anchors);
    }
    let sig = PossibilitySignature::of(map_a.get(target).expect("resident").current);
    let mut vault_a = Vault::open(MemoryStorage::new()).unwrap();
    vault_a.record_preserve(vec![(target, sig)], "glade".into());
    map_a.set_override(target, sig);
    for _ in 0..2 {
        settle(&mut map_a, (128.0, 128.0), 0.0, &[]);
    }
    let hashes_a = region_tile_hashes(&map_a, target);
    let bundle = vault_a.export();

    // World B: a fresh explorer imports the preserve before ever visiting.
    let mut vault_b = Vault::open(MemoryStorage::new()).unwrap();
    let stats = vault_b.import(&bundle);
    assert_eq!(stats.added, 1);
    let mut map_b = RegionMap::new(config());
    for record in vault_b.preserves().values() {
        for &(coord, sig) in &record.regions {
            map_b.set_override(coord, sig);
        }
    }
    for _ in 0..4 {
        settle(&mut map_b, (128.0, 128.0), 0.0, &[]);
    }

    // Same buckets ⇒ same dependency hashes ⇒ bit-identical tiles (same
    // platform), with zero geometry in the record (ADR 0013/0014).
    let region_b = map_b.get(target).expect("resident");
    assert_eq!(PossibilitySignature::of(region_b.current), sig);
    assert_eq!(region_tile_hashes(&map_b, target), hashes_a);
}
