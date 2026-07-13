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

/// Stored dependency keys for every declared layer of one resident region.
fn region_dep_hashes(map: &RegionMap, coord: RegionCoord) -> Vec<(u16, Option<u64>)> {
    map.layer_diagnostics(coord)
        .expect("resident diagnostics")
        .into_iter()
        .map(|diagnostic| (diagnostic.layer, diagnostic.stored))
        .collect()
}

fn apply_vault_preserves(map: &mut RegionMap, vault: &Vault<MemoryStorage>, reverse: bool) {
    let mut records: Vec<_> = vault.preserves().iter().collect();
    if reverse {
        records.reverse();
    }
    let contributions = records.into_iter().flat_map(|(&id, record)| {
        record
            .regions
            .iter()
            .map(move |&(coord, signature)| (id, coord, signature))
    });
    map.apply_preserve_contributions(contributions);
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
    map.apply_preserve_contribution(1, target, sig);
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
    map.remove_preserve_contribution(1, target);
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
    let id = vault_a
        .record_preserve(vec![(target, sig)], "glade".into())
        .unwrap();
    map_a.apply_preserve_contribution(id, target, sig);
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
    let contributions = vault_b.preserves().iter().flat_map(|(&id, record)| {
        record
            .regions
            .iter()
            .map(move |&(coord, signature)| (id, coord, signature))
    });
    map_b.apply_preserve_contributions(contributions);
    for _ in 0..4 {
        settle(&mut map_b, (128.0, 128.0), 0.0, &[]);
    }

    // Same buckets ⇒ same dependency hashes ⇒ bit-identical tiles (same
    // platform), with zero geometry in the record (ADR 0013/0014).
    let region_b = map_b.get(target).expect("resident");
    assert_eq!(PossibilitySignature::of(region_b.current), sig);
    assert_eq!(region_tile_hashes(&map_b, target), hashes_a);
}

#[test]
fn overlap_order_winner_recovery_and_evicted_deletion_are_deterministic() {
    let target = RegionCoord::new(0, 0);
    let neutral = PossibilitySignature::of(world_core::PossibilityVector::neutral());
    let mut alternate = neutral;
    alternate.buckets[PossibilityDomain::Aesthetics.index()] = 4000;

    // Use real content-derived ids. Which signature receives the lower id is
    // intentionally not assumed; the ordered vault map defines the oracle.
    let mut vault = Vault::open(MemoryStorage::new()).unwrap();
    let first = vault
        .record_preserve(vec![(target, neutral)], "neutral".into())
        .unwrap();
    let second = vault
        .record_preserve(vec![(target, alternate)], "alternate".into())
        .unwrap();
    assert_ne!(first, second);
    let (&low_id, low_record) = vault.preserves().first_key_value().unwrap();
    let (&high_id, high_record) = vault.preserves().last_key_value().unwrap();
    let low_signature = low_record.regions[0].1;
    let high_signature = high_record.regions[0].1;
    assert_ne!(low_signature, high_signature);

    // Startup/import traversal in either direction, including an idempotent
    // second synchronization, must produce the same authoritative world.
    let mut forward = RegionMap::new(config());
    apply_vault_preserves(&mut forward, &vault, false);
    apply_vault_preserves(&mut forward, &vault, false);
    let mut reverse = RegionMap::new(config());
    apply_vault_preserves(&mut reverse, &vault, true);
    for _ in 0..4 {
        settle(&mut forward, (128.0, 128.0), 0.0, &[]);
        settle(&mut reverse, (128.0, 128.0), 0.0, &[]);
    }
    assert_eq!(
        forward.effective_preserve(target),
        Some((low_id, low_signature))
    );
    assert_eq!(
        reverse.effective_preserve(target),
        Some((low_id, low_signature))
    );
    assert_eq!(
        forward.get(target).unwrap().current,
        low_signature.dequantize()
    );
    assert_eq!(
        reverse.get(target).unwrap().current,
        low_signature.dequantize()
    );
    assert_eq!(
        region_dep_hashes(&forward, target),
        region_dep_hashes(&reverse, target)
    );
    assert_eq!(
        region_tile_hashes(&forward, target),
        region_tile_hashes(&reverse, target)
    );
    assert_eq!(forward.organisms_in(target), reverse.organisms_in(target));

    // Deleting the resident winner reveals the successor and advances the
    // realized-state revision exactly once. The successor differs only in
    // Aesthetics, so A.9 keeps stable organism identity resident until the
    // expression refresh publishes.
    let old_revision = forward.get(target).unwrap().revision;
    let old_organisms = forward.organisms_in(target).unwrap().to_vec();
    assert!(forward.remove_preserve_contribution(low_id, target));
    assert_eq!(
        forward.effective_preserve(target),
        Some((high_id, high_signature))
    );
    assert_eq!(
        forward.get(target).unwrap().revision,
        old_revision.wrapping_add(1)
    );
    assert_eq!(forward.organisms_in(target).unwrap(), old_organisms);
    for _ in 0..4 {
        settle(&mut forward, (128.0, 128.0), 0.0, &[]);
    }

    let mut successor_oracle = RegionMap::new(config());
    successor_oracle.apply_preserve_contribution(high_id, target, high_signature);
    for _ in 0..4 {
        settle(&mut successor_oracle, (128.0, 128.0), 0.0, &[]);
    }
    assert_eq!(
        forward.get(target).unwrap().current,
        high_signature.dequantize()
    );
    assert_eq!(
        region_dep_hashes(&forward, target),
        region_dep_hashes(&successor_oracle, target)
    );
    assert_eq!(
        region_tile_hashes(&forward, target),
        region_tile_hashes(&successor_oracle, target)
    );
    assert_eq!(
        forward.organisms_in(target),
        successor_oracle.organisms_in(target)
    );

    // Contributor ownership outlives the resident. Removing a winner while
    // evicted still selects the successor used on the later reload.
    let mut evicted = RegionMap::new(config());
    apply_vault_preserves(&mut evicted, &vault, true);
    for _ in 0..4 {
        settle(&mut evicted, (128.0, 128.0), 0.0, &[]);
    }
    for _ in 0..4 {
        settle(&mut evicted, (5000.0, 5000.0), 0.0, &[]);
    }
    assert!(evicted.get(target).is_none());
    assert!(evicted.remove_preserve_contribution(low_id, target));
    assert_eq!(
        evicted.effective_preserve(target),
        Some((high_id, high_signature))
    );
    for _ in 0..4 {
        settle(&mut evicted, (128.0, 128.0), 0.0, &[]);
    }
    assert_eq!(evicted.get(target).unwrap().revision, 0);
    assert_eq!(
        evicted.get(target).unwrap().current,
        high_signature.dequantize()
    );
    assert_eq!(
        region_tile_hashes(&evicted, target),
        region_tile_hashes(&successor_oracle, target)
    );
}
