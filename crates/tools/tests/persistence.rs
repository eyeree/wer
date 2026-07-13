//! Session durability (phase-5-plan.md §12.2, the M2 exit criterion): a run
//! saved mid-journey and reloaded into a fresh process settles to the *same*
//! world — the state hash of the save→load→settle run equals the
//! uninterrupted run's, bit for bit. Also asserts that loading is not an
//! event: the bounded zero-travel settle updates after a restore converge
//! nothing and load nothing beyond what the session captured.

use tools::state_hash;
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, PossibilityDomain,
    PossibilityField, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    apply_session_regions, session_runtime_record, Budget, FrameStats, InlineExecutor,
    MemoryStorage, RegionMap, SessionSnapshotInput, StreamConfig, Vault,
};

const FRAMES: u32 = 90;
const SAVE_FRAME: u32 = 45;
const ANCHOR_FRAME: u32 = 30;
const VELOCITY: (f64, f64) = (17.0, 11.0);

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

fn pos(frame: u32) -> (f64, f64) {
    (f64::from(frame) * VELOCITY.0, f64::from(frame) * VELOCITY.1)
}

/// A deterministic bias script: an Ecology/Hydrology ramp through the middle.
fn bias_at(frame: u32) -> [f32; POSSIBILITY_DIMS] {
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    let t = frame as f32 / FRAMES as f32;
    let ramp = (t * 2.0).min(1.0);
    bias[PossibilityDomain::Ecology.index()] = 0.25 * ramp;
    bias[PossibilityDomain::Hydrology.index()] = 0.15 * ramp;
    bias
}

/// A deterministic anchor script: one Emphasize anchor dropped a third of the
/// way in, frozen at that frame's player position.
fn anchors_at(frame: u32) -> Vec<Anchor> {
    if frame < ANCHOR_FRAME {
        return Vec::new();
    }
    let mask = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Morphology]);
    vec![Anchor {
        world_pos: pos(ANCHOR_FRAME),
        target: bound_target(mask, 0.95),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.7,
        falloff_radius: 1200.0,
        source: AnchorSource::Manual,
    }]
}

fn step(map: &mut RegionMap, frame: u32, field: &PossibilityField) -> FrameStats {
    let travel = if frame == 0 {
        0.0
    } else {
        f64::hypot(VELOCITY.0, VELOCITY.1)
    };
    map.update(
        pos(frame),
        travel,
        field,
        &anchors_at(frame),
        &bias_at(frame),
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    )
}

#[test]
fn save_load_settle_matches_the_uninterrupted_run() {
    let field = PossibilityField::default();

    // The uninterrupted run.
    let mut uninterrupted = RegionMap::new(config());
    for frame in 0..FRAMES {
        step(&mut uninterrupted, frame, &field);
    }
    let expected = state_hash(&uninterrupted);

    // The interrupted run: play to the save point, snapshot, drop everything.
    let mut before = RegionMap::new(config());
    for frame in 0..=SAVE_FRAME {
        step(&mut before, frame, &field);
    }
    assert!(
        before.authoritative_realization_complete(pos(SAVE_FRAME)),
        "save-point precondition: the scripted source map must have complete canonical near state"
    );
    let mut vault = Vault::open(MemoryStorage::new()).expect("fresh store opens");
    let anchors = anchors_at(SAVE_FRAME);
    vault
        .snapshot_session(SessionSnapshotInput {
            map: &before,
            player: pos(SAVE_FRAME),
            last_player: pos(SAVE_FRAME - 1),
            bias: &bias_at(SAVE_FRAME),
            transition_mode: false,
            anchors: &anchors,
            runtime: session_runtime_record(
                before.config(),
                &Budget::unlimited(),
                None,
                false,
                false,
            ),
            recorder: None,
            tracker: world_core::RouteTrackerSnapshot::default(),
        })
        .unwrap();
    let stats = vault.flush_all().unwrap();
    assert_eq!(stats.dirty, 0);
    let store = vault.store().clone();
    drop(before);
    drop(vault);

    // A "fresh process": reopen the store, restore, settle, continue.
    let reopened = Vault::open(store).expect("store reopens");
    assert_eq!(reopened.issue_count(), 0);
    let snap = reopened.session().expect("session persisted").clone();
    assert_eq!(snap.player, pos(SAVE_FRAME));

    let mut restored = RegionMap::new(config());
    apply_session_regions(&mut restored, &snap);
    for region in &snap.regions {
        assert_eq!(
            restored.get(region.coord).unwrap().target.dims,
            region.target,
            "session restore must preserve region target bit-exactly"
        );
    }
    let anchors: Vec<Anchor> = snap.anchors.iter().map(|a| a.to_anchor()).collect();

    // The settle phase (phase-5-plan.md §12.2; ADR 0024): travel = 0 at the
    // save-point inputs rebuilds caches and rosters, then drains the fixed
    // one-canonical-region-per-frame publication schedule without moving any
    // state. Loading is not an event. One pass per captured authority plus a
    // final observation is a conservative bound for the smaller near subset.
    let settle_frames = restored.len() + 1;
    for _ in 0..settle_frames {
        let settle = restored.update(
            snap.player,
            0.0,
            &field,
            &anchors,
            &snap.bias,
            &Budget::unlimited(),
            &InlineExecutor,
            snap.transition_mode,
        );
        assert_eq!(settle.converged, 0, "restore must not converge anything");
        assert_eq!(settle.loaded, 0, "the session captured the whole window");
        if restored.authoritative_realization_complete(snap.player) {
            break;
        }
    }
    assert!(
        restored.authoritative_realization_complete(snap.player),
        "zero-travel restore settling must publish every ready canonical near region"
    );

    for frame in SAVE_FRAME + 1..FRAMES {
        step(&mut restored, frame, &field);
    }
    assert_eq!(
        state_hash(&restored),
        expected,
        "save→load→settle must reproduce the uninterrupted world exactly"
    );
}

#[test]
fn crash_consistency_a_partial_flush_still_opens_clean() {
    // Simulate a crash after every possible prefix of a flush batch: each
    // partial store must open without error and contain only whole records
    // (phase-5-plan.md §12.3, durability family).
    let anchor = anchors_at(ANCHOR_FRAME).remove(0);
    let build_vault = || {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        for i in 0..4 {
            let mut a = anchor;
            a.world_pos.0 += f64::from(i) * 100.0;
            vault.record_discovery(&a, 0, format!("d{i}")).unwrap();
        }
        vault
    };

    // Count the total writes a full flush performs.
    let mut full = build_vault();
    let total = full.flush_all().unwrap().flushed;

    for cut in 0..total {
        let mut vault = build_vault();
        // Flush exactly `cut` ops, then "crash" (drop the rest).
        let budget = Budget {
            max_persist_ops: cut,
            ..Budget::unlimited()
        };
        vault.flush(&budget).unwrap();
        let survivor = Vault::open(vault.store().clone()).expect("partial store opens");
        assert!(
            survivor.issue_count() == 0,
            "cut {cut}: {:?}",
            survivor.issues().collect::<Vec<_>>()
        );
        // Every record that made it is whole and valid.
        for record in survivor.discoveries().values() {
            assert_eq!(record.id, record.content_id());
        }
        assert!(survivor.discoveries().len() <= 4);
    }
}
