//! Succession and response tests (phase-3-plan.md §7.6, M5): distance-based
//! regeneration, offscreen replacement, distant ecosystems shifting under
//! steering, and coherent re-realization as the player approaches — no pop, no
//! stored organism state.

use world_core::layer::LAYER_ECOLOGY;
use world_core::{PossibilityDomain, PossibilityField, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{
    Budget, InlineExecutor, RegionMap, StreamConfig, CHANNEL_HARDNESS, CHANNEL_HERBIVORE,
};

const NO_BIAS: [f32; POSSIBILITY_DIMS] = [0.0; POSSIBILITY_DIMS];

fn config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 3.0 * REGION_SIZE,
        load_radius: 5.0 * REGION_SIZE,
        unload_radius: 6.0 * REGION_SIZE,
        converge_per_unit: 0.02,
        converge_rate_cap: 0.25,
        field_resolution: 8,
        ..StreamConfig::default()
    }
}

fn step(map: &mut RegionMap, player: (f64, f64), travel: f64, bias: &[f32; POSSIBILITY_DIMS]) {
    let field = PossibilityField::default();
    map.update(
        player,
        travel,
        &field,
        &[],
        bias,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    );
}

fn settle(map: &mut RegionMap, player: (f64, f64)) {
    for _ in 0..8 {
        step(map, player, 0.0, &NO_BIAS);
    }
}

#[test]
fn organisms_are_dropped_offscreen_and_rerealized_on_return() {
    // Offscreen replacement: a region's organisms exist only while it is in the
    // near window; leaving discards them, returning re-realizes from scratch —
    // no stored entity state (§7.6).
    let mut map = RegionMap::new(config());
    let origin = (0.0, 0.0);
    settle(&mut map, origin);

    let near = RegionCoord::new(0, 0);
    assert!(
        map.organisms_in(near).is_some_and(|o| !o.is_empty()),
        "near region should host organisms"
    );

    // Move so (0,0) leaves the near window but stays loaded.
    let away = (3.0 * REGION_SIZE, 0.0);
    settle(&mut map, away);
    assert!(map.get(near).is_some(), "region should still be resident");
    assert!(
        map.organisms_in(near).is_none(),
        "organisms must be discarded once the region leaves the near window"
    );

    // Return: the region re-realizes coherently.
    settle(&mut map, origin);
    assert!(
        map.organisms_in(near).is_some_and(|o| !o.is_empty()),
        "returning to the region must re-realize its organisms"
    );
}

#[test]
fn distant_ecosystem_shifts_under_steering_while_the_stable_trio_holds() {
    // A converging far region regenerates its L8 aggregate under Ecology
    // steering (its pressures shift) while terrain/geology never move.
    let mut map = RegionMap::new(config());
    let player = (128.0, 128.0);
    settle(&mut map, player);

    // A loaded, unpinned region.
    let far = RegionCoord::new(2, 2);
    let region = map.get(far).expect("resident");
    assert!(region.stability < 1.0, "target region must be unpinned");

    let herb_before = map
        .cache()
        .channel(far, CHANNEL_HERBIVORE)
        .expect("L8 herbivore tile")
        .content_hash();
    let geology_before = map
        .cache()
        .channel(far, CHANNEL_HARDNESS)
        .expect("geology tile")
        .content_hash();

    // Steer Ecology up and travel in place so far regions converge.
    let mut bias = NO_BIAS;
    bias[PossibilityDomain::Ecology.index()] = 0.4;
    for _ in 0..60 {
        step(&mut map, player, 25.0, &bias);
    }

    let herb_after = map
        .cache()
        .channel(far, CHANNEL_HERBIVORE)
        .expect("L8 herbivore tile")
        .content_hash();
    let geology_after = map
        .cache()
        .channel(far, CHANNEL_HARDNESS)
        .expect("geology tile")
        .content_hash();

    assert_ne!(
        herb_before, herb_after,
        "far ecology must shift as the region converges under steering"
    );
    assert_eq!(
        geology_before, geology_after,
        "the stable trio must never move under ecology steering"
    );
}

#[test]
fn converged_region_realizes_coherently_on_approach() {
    // As the player approaches a region that converged under steering, it
    // realizes organisms consistent with its (new) aggregate: every organism's
    // species belongs to its cell's roster — no organism contradicts the field.
    let mut map = RegionMap::new(config());
    let player = (128.0, 128.0);
    settle(&mut map, player);

    let mut bias = NO_BIAS;
    bias[PossibilityDomain::Climate.index()] = -0.3;
    bias[PossibilityDomain::Ecology.index()] = 0.3;
    for _ in 0..80 {
        step(&mut map, player, 25.0, &bias);
    }

    // Approach a converged region so it becomes near and realizes.
    let target = RegionCoord::new(2, 2);
    let (ox, oy) = target.origin();
    let center = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
    for _ in 0..8 {
        // Keep steering so the region's realized state stays the converged one.
        step(&mut map, center, 0.0, &bias);
    }

    // Ecology tile is fresh and organisms exist.
    assert!(
        map.cache()
            .get(target)
            .and_then(|t| t.layer_hash(LAYER_ECOLOGY))
            .is_some(),
        "L8 must be settled for the approached region"
    );
    let organisms = map
        .organisms_in(target)
        .expect("approached region must realize organisms");
    assert!(!organisms.is_empty());

    // Coherence: every realized organism's species is in its cell's roster.
    for org in organisms {
        let sig = map
            .cell_signature(target, org.cell.cx, org.cell.cy)
            .expect("cell classified");
        let entry = map.roster_cache().get(sig).expect("roster cached");
        assert!(
            entry.roster.species.iter().any(|s| s.id == org.species),
            "organism species {:#x} not in its cell's roster",
            org.species
        );
    }
}
