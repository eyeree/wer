//! The vault harness — the Phase 5 sign-off tool (phase-5-plan.md §12.3),
//! alongside the invalidation ledger, the ecology harness, and the anchor
//! harness. Machine-checks the Phase 5 success criterion over scripted
//! journeys on `MemoryStorage` + `InlineExecutor`:
//!
//! - **Durable** — save→load→settle reproduces the uninterrupted run's state
//!   hash exactly; a crash mid-flush leaves a store that opens clean.
//! - **Sparse** — the store holds records in the declared namespaces only,
//!   with bytes bounded by player actions, never geometry.
//! - **Shareable** — bundles merge commutatively/associatively/idempotently,
//!   imported anchors steer identically, tampering is rejected.
//! - **Preserves** — a preserve holds bit-identical tiles under steering and
//!   eviction, and realizes identically from a bundle in a fresh world.
//! - **Routes** — attraction is soft, corridor-bounded, monotone-saturating
//!   in usage; traversal bumps once per leg.
//! - **Precision preserved** — persisted influence (summoned discovery
//!   anchors, route attraction) never regenerates the stable trio.

use world_core::{
    anchor_influence_profile, attraction_anchors, bound_target, domain_mask, route_pull, Anchor,
    AnchorKind, AnchorSource, PossibilityDomain, PossibilityField, PossibilitySignature,
    RegionCoord, RouteNode, RouteRecord, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    apply_session_regions, Budget, FrameStats, InlineExecutor, MemoryStorage, RegionMap,
    RouteTracker, Storage, StorageError, StreamConfig, Vault, CHANNEL_COUNT, CHANNEL_ELEVATION,
    CHANNEL_HARDNESS,
};

use crate::replay::state_hash;

/// Outcome of one vault-harness scenario.
#[derive(Debug)]
pub struct VaultReport {
    /// Scenario name.
    pub name: &'static str,
    /// Violations found (empty = passed).
    pub violations: Vec<String>,
    /// One-line context for the log.
    pub summary: String,
}

impl VaultReport {
    /// Whether the scenario passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

const MAX_VIOLATIONS: usize = 16;

fn record(violations: &mut Vec<String>, message: String) {
    if violations.len() < MAX_VIOLATIONS {
        violations.push(message);
    }
}

#[derive(Debug, Default)]
struct FaultState {
    inner: MemoryStorage,
    fail_store: Option<Vec<u8>>,
    fail_remove: bool,
    stores: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
struct FaultStorage(std::rc::Rc<std::cell::RefCell<FaultState>>);

impl Storage for FaultStorage {
    fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError> {
        self.0.borrow().inner.load(key)
    }

    fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        let mut state = self.0.borrow_mut();
        state.stores.push(key.to_vec());
        if state.fail_store.as_deref() == Some(key) {
            return Err(StorageError::Backend("harness store fault".into()));
        }
        state.inner.store(key, value)
    }

    fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
        let mut state = self.0.borrow_mut();
        if state.fail_remove {
            return Err(StorageError::Backend("harness remove fault".into()));
        }
        state.inner.remove(key)
    }

    fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError> {
        self.0.borrow().inner.keys_with_prefix(prefix)
    }
}

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

/// One scripted frame. Generation dispatch/integration is unbudgeted and
/// inline; fixed canonical publication may still lag one region per frame, so
/// the durability scenario explicitly checks its save-point precondition.
fn step(
    map: &mut RegionMap,
    player: (f64, f64),
    travel: f64,
    anchors: &[Anchor],
    bias: &[f32; POSSIBILITY_DIMS],
) -> FrameStats {
    let field = PossibilityField::default();
    map.update(
        player,
        travel,
        &field,
        anchors,
        bias,
        &Budget::unlimited(),
        &InlineExecutor,
        false,
    )
}

/// The deterministic journey script shared by the durability and sparsity
/// scenarios.
fn script_pos(frame: u32) -> (f64, f64) {
    (f64::from(frame) * 17.0, f64::from(frame) * 11.0)
}

fn script_bias(frame: u32, frames: u32) -> [f32; POSSIBILITY_DIMS] {
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    let t = frame as f32 / frames as f32;
    bias[PossibilityDomain::Ecology.index()] = 0.25 * (t * 2.0).min(1.0);
    bias
}

fn script_anchors(frame: u32) -> Vec<Anchor> {
    if frame < 30 {
        return Vec::new();
    }
    let mask = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Morphology]);
    vec![Anchor {
        world_pos: script_pos(30),
        target: bound_target(mask, 0.95),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.7,
        falloff_radius: 1200.0,
        source: AnchorSource::Manual,
    }]
}

/// A capture-shaped anchor for discovery records.
fn discovery_anchor(x: f64, strength: f32) -> Anchor {
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    Anchor {
        world_pos: (x, -40.0),
        target: bound_target(mask, 0.85),
        mask,
        kind: AnchorKind::Emphasize,
        strength,
        falloff_radius: 1400.0,
        source: AnchorSource::Organism {
            species: 0x2340_6061_75CD_D2D2,
        },
    }
}

/// Durable: save mid-journey, reload into a fresh map, settle, continue —
/// the state hash must equal the uninterrupted run's (§12.2). Loading is not
/// an event.
fn scenario_durable() -> VaultReport {
    const FRAMES: u32 = 90;
    const SAVE: u32 = 45;
    let mut violations = Vec::new();
    let run = |map: &mut RegionMap, from: u32, to: u32| {
        for frame in from..to {
            let travel = if frame == 0 {
                0.0
            } else {
                f64::hypot(17.0, 11.0)
            };
            step(
                map,
                script_pos(frame),
                travel,
                &script_anchors(frame),
                &script_bias(frame, FRAMES),
            );
        }
    };

    let mut uninterrupted = RegionMap::new(config());
    run(&mut uninterrupted, 0, FRAMES);
    let expected = state_hash(&uninterrupted);

    let mut before = RegionMap::new(config());
    run(&mut before, 0, SAVE + 1);
    if !before.authoritative_realization_complete(script_pos(SAVE)) {
        record(
            &mut violations,
            "save-point source map lacked complete canonical near state".into(),
        );
    }
    let mut vault = Vault::open(MemoryStorage::new()).expect("fresh store");
    vault
        .snapshot_session(
            &before,
            script_pos(SAVE),
            script_pos(SAVE - 1),
            &script_bias(SAVE, FRAMES),
            false,
            &script_anchors(SAVE),
        )
        .expect("sequence available");
    vault.flush_all().expect("memory store flush");
    let reopened = Vault::open(vault.store().clone()).expect("reopen");
    let snap = reopened.session().expect("session persisted").clone();

    let mut restored = RegionMap::new(config());
    apply_session_regions(&mut restored, &snap);
    let anchors: Vec<Anchor> = snap.anchors.iter().map(|a| a.to_anchor()).collect();
    let field = PossibilityField::default();
    let settle_limit = restored.len() + 1;
    for _ in 0..settle_limit {
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
        if settle.converged != 0 || settle.loaded != 0 {
            record(
                &mut violations,
                format!(
                    "restore settled with {} converged / {} loaded regions (loading must not be an event)",
                    settle.converged, settle.loaded
                ),
            );
        }
        if restored.authoritative_realization_complete(snap.player) {
            break;
        }
    }
    if !restored.authoritative_realization_complete(snap.player) {
        record(
            &mut violations,
            "restore did not drain fixed canonical publication at zero travel".into(),
        );
    }
    run(&mut restored, SAVE + 1, FRAMES);
    let actual = state_hash(&restored);
    if actual != expected {
        record(
            &mut violations,
            format!("save→load→settle hash {actual:#018x} != uninterrupted {expected:#018x}"),
        );
    }

    VaultReport {
        name: "durable: save→load→settle is state-hash exact",
        violations,
        summary: format!("{FRAMES} frames, saved at {SAVE}"),
    }
}

/// Durable: every crash point mid-flush leaves a store that opens clean with
/// whole records only.
fn scenario_crash_consistency() -> VaultReport {
    let mut violations = Vec::new();
    let build = || {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        for i in 0..4 {
            vault
                .record_discovery(
                    &discovery_anchor(f64::from(i) * 120.0, 0.7),
                    0,
                    format!("d{i}"),
                )
                .expect("sequence available");
        }
        vault
    };
    let total = build().flush_all().expect("memory store flush").flushed;
    for cut in 0..total {
        let mut vault = build();
        vault
            .flush(&Budget {
                max_persist_ops: cut,
                ..Budget::unlimited()
            })
            .expect("memory store flush");
        match Vault::open(vault.store().clone()) {
            Err(e) => record(
                &mut violations,
                format!("cut {cut}: store failed to open: {e}"),
            ),
            Ok(survivor) => {
                if survivor.issue_count() > 0 {
                    record(
                        &mut violations,
                        format!(
                            "cut {cut}: issues {:?}",
                            survivor.issues().collect::<Vec<_>>()
                        ),
                    );
                }
                for r in survivor.discoveries().values() {
                    if r.id != r.content_id() {
                        record(
                            &mut violations,
                            format!("cut {cut}: torn record {:#018x}", r.id),
                        );
                    }
                }
            }
        }
    }
    VaultReport {
        name: "durable: a crash mid-flush never corrupts the store",
        violations,
        summary: format!("{total} crash points exercised"),
    }
}

/// Contract hardening (ADR 0022): failures remain explicit/retryable, data
/// precedes metadata, delete is commit-after-remove, diagnostics stay bounded,
/// and import advances the live local sequence immediately.
fn scenario_persistence_failures() -> VaultReport {
    let mut violations = Vec::new();
    let storage = FaultStorage::default();
    let control = storage.clone();
    let mut vault = Vault::open(storage).expect("fault store opens");
    let id = vault
        .record_discovery(&discovery_anchor(12.0, 0.7), 0, "fault".into())
        .expect("sequence available");
    let key = world_runtime::vault::discovery_key(id);
    control.0.borrow_mut().fail_store = Some(key.clone());

    for attempt in 1..=70 {
        match vault.flush_all() {
            Ok(stats) => record(
                &mut violations,
                format!("failure attempt {attempt} reported clean success: {stats:?}"),
            ),
            Err(error) => {
                if error.progress().is_clean() || error.progress().dirty != 2 {
                    record(
                        &mut violations,
                        format!("failure attempt {attempt} reported wrong dirtiness: {error}"),
                    );
                }
            }
        }
    }
    if control
        .0
        .borrow()
        .stores
        .iter()
        .any(|stored| stored.as_slice() == world_runtime::vault::KEY_META)
    {
        record(
            &mut violations,
            "metadata overtook a failed discovery write".into(),
        );
    }
    if vault.issue_count() != 1
        || vault
            .active_persistence_issue()
            .is_none_or(|issue| issue.occurrences() != 70)
    {
        record(
            &mut violations,
            format!(
                "repeated failure diagnostics were not bounded: {} retained",
                vault.issue_count()
            ),
        );
    }
    control.0.borrow_mut().fail_store = None;
    match vault.flush_all() {
        Ok(stats) if stats.is_clean() && vault.active_persistence_issue().is_none() => {}
        result => record(
            &mut violations,
            format!("store retry did not recover cleanly: {result:?}"),
        ),
    }

    let signature = PossibilitySignature::of(world_core::PossibilityVector::neutral());
    let preserve = vault
        .record_preserve(
            vec![(RegionCoord::new(5, 6), signature)],
            "retry-delete".into(),
        )
        .expect("sequence available");
    vault.flush_all().expect("fault disabled");
    control.0.borrow_mut().fail_remove = true;
    if vault.remove_preserve(preserve).is_ok() || !vault.preserves().contains_key(&preserve) {
        record(
            &mut violations,
            "failed delete removed the logical preserve".into(),
        );
    }
    control.0.borrow_mut().fail_remove = false;
    if !matches!(vault.remove_preserve(preserve), Ok(true))
        || vault.preserves().contains_key(&preserve)
    {
        record(
            &mut violations,
            "successful delete retry did not commit logical absence".into(),
        );
    }

    let mut remote = Vault::open(MemoryStorage::new()).unwrap();
    remote
        .record_discovery(&discovery_anchor(800.0, 0.8), 1, "remote".into())
        .expect("sequence available");
    let mut bundle = remote.export();
    bundle.discoveries[0].sequence = 500;
    let stats = vault.import(&bundle);
    let local = vault
        .record_discovery(&discovery_anchor(1600.0, 0.8), 1, "local".into())
        .expect("sequence available");
    if stats.added != 1 || vault.discoveries()[&local].sequence <= 500 {
        record(
            &mut violations,
            "post-import local edit was not strictly newer in the same process".into(),
        );
    }

    VaultReport {
        name: "durable: persistence failures are explicit and retry-safe",
        violations,
        summary: "70 retries, data/meta ordering, delete, and import sequence".into(),
    }
}

/// Sparse: a long journey with a handful of actions stores bytes bounded by
/// the actions — in the declared namespaces only, and no value remotely
/// tile-sized.
fn scenario_sparse() -> VaultReport {
    const FRAMES: u32 = 240;
    let mut violations = Vec::new();
    let mut map = RegionMap::new(config());
    let mut vault = Vault::open(MemoryStorage::new()).unwrap();

    // Travel-only baseline: only session + seen-set bytes may grow.
    for frame in 0..FRAMES {
        let travel = if frame == 0 {
            0.0
        } else {
            f64::hypot(17.0, 11.0)
        };
        step(
            &mut map,
            script_pos(frame),
            travel,
            &[],
            &[0.0; POSSIBILITY_DIMS],
        );
        vault.mark_seen(RegionCoord::from_world(
            script_pos(frame).0,
            script_pos(frame).1,
        ));
    }
    vault.flush_all().expect("memory store flush");
    let travel_only = vault.store().bytes();

    // A handful of actions.
    const ACTIONS: usize = 6;
    for i in 0..ACTIONS {
        vault
            .record_discovery(
                &discovery_anchor(f64::from(i as u32) * 200.0, 0.7),
                0,
                format!("d{i}"),
            )
            .expect("sequence available");
    }
    let sig = PossibilitySignature::of(map.iter_active().next().unwrap().current);
    vault
        .record_preserve(vec![(RegionCoord::new(0, 0), sig)], "glade".into())
        .expect("sequence available");
    vault
        .snapshot_session(
            &map,
            script_pos(FRAMES - 1),
            script_pos(FRAMES - 2),
            &[0.0; POSSIBILITY_DIMS],
            false,
            &[],
        )
        .expect("sequence available");
    vault.flush_all().expect("memory store flush");
    let total = vault.store().bytes();

    // Namespace check + per-value size bound (a tile at 8×8 f32 alone would
    // be 256 bytes of samples per channel across ~13 channels; a whole-region
    // leak would show as multi-KB values).
    let allowed = ["meta/", "session/", "disc/", "route/", "pres/", "seen/"];
    for (key, value) in vault.store().iter() {
        let key_str = String::from_utf8_lossy(key);
        if !allowed.iter().any(|p| key_str.starts_with(p)) {
            record(&mut violations, format!("undeclared namespace: {key_str}"));
        }
        let cap = if key_str.starts_with("session/") {
            // Session scales with the resident window (bounded by config),
            // not with travel: ~50 bytes per resident region.
            50 * map.len() + 512
        } else {
            512
        };
        if value.len() > cap {
            record(
                &mut violations,
                format!(
                    "{key_str} holds {} bytes (cap {cap}) — geometry leak?",
                    value.len()
                ),
            );
        }
    }
    // Affine bound in actions: the actions added a bounded number of bytes.
    let action_bytes = total.saturating_sub(travel_only);
    if action_bytes > (ACTIONS + 1) * 512 {
        record(
            &mut violations,
            format!("{ACTIONS} actions grew the store by {action_bytes} bytes"),
        );
    }
    // The seen-set stays compact: a few bytes per region ever visited.
    let seen = vault.seen_count();
    if seen == 0 {
        record(&mut violations, "journey marked nothing discovered".into());
    }

    VaultReport {
        name: "sparse: store bytes are O(actions), never geometry",
        violations,
        summary: format!(
            "{FRAMES} frames, {seen} regions seen, {travel_only}B travel-only, {total}B after {ACTIONS}+1 actions"
        ),
    }
}

/// Shareable: merge laws hold, import order is irrelevant to steering, and
/// tampering is rejected without partial application.
fn scenario_shareable() -> VaultReport {
    let mut violations = Vec::new();

    let mut a = Vault::open(MemoryStorage::new()).unwrap();
    a.record_discovery(&discovery_anchor(0.0, 0.8), 1, "shared".into())
        .expect("sequence available");
    a.record_discovery(&discovery_anchor(400.0, 0.6), 1, "only-a".into())
        .expect("sequence available");
    let mut b = Vault::open(MemoryStorage::new()).unwrap();
    b.record_discovery(&discovery_anchor(0.0, 0.8), 2, "shared-renamed".into())
        .expect("sequence available");
    b.record_discovery(&discovery_anchor(800.0, 0.5), 1, "only-b".into())
        .expect("sequence available");
    let mut c = Vault::open(MemoryStorage::new()).unwrap();
    c.record_discovery(&discovery_anchor(1200.0, 0.4), 1, "only-c".into())
        .expect("sequence available");
    let (ea, eb, ec) = (a.export(), b.export(), c.export());

    let merged = |bundles: &[&world_core::AtlasBundle]| {
        let mut v = Vault::open(MemoryStorage::new()).unwrap();
        for bundle in bundles {
            v.import(bundle);
        }
        v.export()
    };
    if merged(&[&ea, &eb]) != merged(&[&eb, &ea]) {
        record(&mut violations, "merge is not commutative".into());
    }
    let left = merged(&[&merged(&[&ea, &eb]), &ec]);
    let right = merged(&[&ea, &merged(&[&eb, &ec])]);
    if left != right {
        record(&mut violations, "merge is not associative".into());
    }
    let mut idem = Vault::open(MemoryStorage::new()).unwrap();
    idem.import(&ea);
    let before = idem.export();
    let stats = idem.import(&ea);
    if stats.added + stats.merged + stats.rejected != 0 || idem.export() != before {
        record(&mut violations, "merge is not idempotent".into());
    }

    // Import order cannot perturb steering (ADR 0011 cashed in).
    let steer_with = |bundles: &[&world_core::AtlasBundle]| {
        let mut v = Vault::open(MemoryStorage::new()).unwrap();
        for bundle in bundles {
            v.import(bundle);
        }
        let anchors: Vec<Anchor> = v.discoveries().values().map(|r| r.to_anchor()).collect();
        world_core::steer(
            world_core::PossibilityVector::neutral(),
            &anchors,
            (200.0, -40.0),
        )
    };
    if steer_with(&[&ea, &eb, &ec]).dims != steer_with(&[&ec, &ea, &eb]).dims {
        record(&mut violations, "import order changed steering".into());
    }

    // Tampering: rejected, never partially applied.
    let mut tampered = ea.clone();
    tampered.discoveries[0].strength_q ^= 1;
    let mut victim = Vault::open(MemoryStorage::new()).unwrap();
    let stats = victim.import(&tampered);
    if stats.rejected != 1 || victim.discoveries().len() != tampered.discoveries.len() - 1 {
        record(
            &mut violations,
            "tampered record was not cleanly rejected".into(),
        );
    }

    VaultReport {
        name: "shareable: CRDT merge laws + order-free steering",
        violations,
        summary: "3 stores, overlapping records, tamper check".into(),
    }
}

/// Preserves: hold bit-identical under steering + eviction; realize
/// identically from a bundle in a fresh world.
fn scenario_preserve() -> VaultReport {
    let mut violations = Vec::new();
    let target = RegionCoord::new(0, 0);
    let anchors = script_anchors(30);
    let bias = [0.0f32; POSSIBILITY_DIMS];

    let mut map = RegionMap::new(config());
    for _ in 0..4 {
        step(&mut map, (128.0, 128.0), 0.0, &[], &bias);
    }
    let sig = PossibilitySignature::of(map.get(target).expect("resident").current);
    map.apply_preserve_contribution(1, target, sig);
    step(&mut map, (128.0, 128.0), 0.0, &[], &bias);
    let hashes = |m: &RegionMap| -> Vec<(usize, u64)> {
        (0..CHANNEL_COUNT)
            .filter_map(|ch| {
                m.cache()
                    .channel(target, ch)
                    .map(|t| (ch, t.content_hash()))
            })
            .collect()
    };
    let held = hashes(&map);

    // Steer + travel; then walk away (evict) and back (reload).
    for stepi in 1..=40 {
        let along = 128.0 + f64::from(stepi) * 60.0;
        step(&mut map, (along, along), 60.0, &anchors, &bias);
    }
    if map.get(target).is_some() {
        record(
            &mut violations,
            "preserved region failed to evict when far".into(),
        );
    }
    for stepi in (0..=40).rev() {
        let along = 128.0 + f64::from(stepi) * 60.0;
        step(&mut map, (along, along), 60.0, &anchors, &bias);
    }
    for _ in 0..2 {
        step(&mut map, (128.0, 128.0), 30.0, &anchors, &bias);
    }
    if hashes(&map) != held {
        record(
            &mut violations,
            "preserved tiles changed across steering + eviction + reload".into(),
        );
    }

    // Import into a fresh world.
    let mut vault = Vault::open(MemoryStorage::new()).unwrap();
    vault
        .record_preserve(vec![(target, sig)], "glade".into())
        .expect("sequence available");
    let bundle = vault.export();
    let mut fresh_vault = Vault::open(MemoryStorage::new()).unwrap();
    fresh_vault.import(&bundle);
    let mut fresh = RegionMap::new(config());
    let contributions = fresh_vault.preserves().iter().flat_map(|(&id, preserve)| {
        preserve
            .regions
            .iter()
            .map(move |&(coord, signature)| (id, coord, signature))
    });
    fresh.apply_preserve_contributions(contributions);
    for _ in 0..4 {
        step(&mut fresh, (128.0, 128.0), 0.0, &[], &bias);
    }
    if hashes(&fresh) != held {
        record(
            &mut violations,
            "imported preserve did not realize identical tiles".into(),
        );
    }

    // Overlap ownership (ADR 0020): real content-derived ids applied in
    // opposite orders must settle identically, and winner deletion must reveal
    // the retained successor with exactly one resident revision bump.
    let mut overlap_vault = Vault::open(MemoryStorage::new()).unwrap();
    overlap_vault
        .record_preserve(vec![(target, sig)], "first".into())
        .expect("sequence available");
    let mut alternate = sig;
    alternate.buckets[world_core::PossibilityDomain::Aesthetics.index()] =
        if alternate.buckets[world_core::PossibilityDomain::Aesthetics.index()] < 2048 {
            4095
        } else {
            0
        };
    overlap_vault
        .record_preserve(vec![(target, alternate)], "second".into())
        .expect("sequence available");
    let (&winner_id, winner) = overlap_vault.preserves().first_key_value().unwrap();
    let (&successor_id, successor) = overlap_vault.preserves().last_key_value().unwrap();
    let winner_sig = winner.regions[0].1;
    let successor_sig = successor.regions[0].1;
    let mut ascending = RegionMap::new(config());
    let mut descending = RegionMap::new(config());
    let ascending_batch = overlap_vault
        .preserves()
        .iter()
        .flat_map(|(&id, preserve)| {
            preserve
                .regions
                .iter()
                .map(move |&(coord, signature)| (id, coord, signature))
        });
    ascending.apply_preserve_contributions(ascending_batch);
    let descending_batch = overlap_vault
        .preserves()
        .iter()
        .rev()
        .flat_map(|(&id, preserve)| {
            preserve
                .regions
                .iter()
                .map(move |&(coord, signature)| (id, coord, signature))
        });
    descending.apply_preserve_contributions(descending_batch);
    for _ in 0..4 {
        step(&mut ascending, (128.0, 128.0), 0.0, &[], &bias);
        step(&mut descending, (128.0, 128.0), 0.0, &[], &bias);
    }
    if ascending.effective_preserve(target) != Some((winner_id, winner_sig))
        || descending.effective_preserve(target) != Some((winner_id, winner_sig))
        || hashes(&ascending) != hashes(&descending)
    {
        record(
            &mut violations,
            "overlapping preserves depended on application order".into(),
        );
    }
    let revision = ascending.get(target).expect("overlap resident").revision;
    ascending.remove_preserve_contribution(winner_id, target);
    if ascending.effective_preserve(target) != Some((successor_id, successor_sig))
        || ascending.get(target).expect("successor resident").revision != revision.wrapping_add(1)
    {
        record(
            &mut violations,
            "winner deletion did not select the successor with one revision bump".into(),
        );
    }

    VaultReport {
        name: "preserve: deterministic ownership, holds under pressure, ships whole",
        violations,
        summary: format!("{} channels held; 2-owner overlap checked", held.len()),
    }
}

/// Routes: soft corridor-bounded attraction, monotone in usage; traversal
/// fires once per leg.
fn scenario_routes() -> VaultReport {
    let mut violations = Vec::new();
    let mut sig = PossibilitySignature::of(world_core::PossibilityVector::neutral());
    sig.buckets[PossibilityDomain::Ecology.index()] = 3600;
    let nodes: Vec<RouteNode> = (0..6)
        .map(|i| RouteNode {
            pos_q: (i * 300, 0),
            signature: sig,
            cost_q: 40,
            stability_q: 0,
            anchor_sig: 0,
        })
        .collect();
    let route = RouteRecord::new(nodes, vec![], 1, "trail".into());

    let base = world_core::PossibilityVector::neutral();
    let at = (450.0, 0.0);
    let pulled = |r: &RouteRecord| {
        let anchors = attraction_anchors([r], at, 32);
        world_core::steer(base, &anchors, at).get(PossibilityDomain::Ecology)
    };
    let fresh_pull = pulled(&route);
    let node_value = world_core::PossibilityVector::dequantize(3600);
    if fresh_pull <= 0.5 {
        record(
            &mut violations,
            "route did not pull toward its recorded state".into(),
        );
    }
    if fresh_pull >= node_value {
        record(
            &mut violations,
            "route forced its recorded state (not soft)".into(),
        );
    }
    // Usage monotonicity is strict below aggregate saturation; isolate one
    // candidate so group normalization cannot turn it into a plateau.
    let mut singleton = RouteRecord::new(vec![route.nodes[1]], vec![], 2, "single".into());
    let singleton_fresh = pulled(&singleton);
    singleton.usage = 40;
    let worn_pull = pulled(&singleton);
    if worn_pull <= singleton_fresh {
        record(
            &mut violations,
            "singleton usage did not strengthen attraction".into(),
        );
    }
    if route_pull(route.usage) > world_core::ROUTE_PULL_CAP {
        record(&mut violations, "route pull exceeded its cap".into());
    }

    // Every selected occurrence across route ids shares one worst-case
    // aggregate budget. Co-located nodes attain the peak exactly.
    let dense_nodes: Vec<_> = (0..16)
        .map(|_| RouteNode {
            pos_q: (450, 0),
            signature: sig,
            cost_q: 40,
            stability_q: 0,
            anchor_sig: 0,
        })
        .collect();
    let mut dense_nodes_b = dense_nodes.clone();
    for node in &mut dense_nodes_b {
        node.cost_q = 41;
    }
    let mut dense_a = RouteRecord::new(dense_nodes, vec![], 3, "dense-a".into());
    let mut dense_b = RouteRecord::new(dense_nodes_b, vec![], 4, "dense-b".into());
    if dense_a.id == dense_b.id {
        record(
            &mut violations,
            "dense multi-route fixture collapsed to one content id".into(),
        );
    }
    dense_a.usage = u32::MAX;
    dense_b.usage = u32::MAX - 1;
    let dense = attraction_anchors([&dense_a, &dense_b], (450.0, 0.0), 32);
    let dense_peak = anchor_influence_profile(&dense, (450.0, 0.0))
        .into_iter()
        .fold(0.0f32, f32::max);
    if dense_peak > world_core::ROUTE_PULL_CAP {
        record(
            &mut violations,
            format!(
                "dense multi-route channel exceeded global cap ({dense_peak:?} > {:?})",
                world_core::ROUTE_PULL_CAP
            ),
        );
    }
    let far = (450.0, world_core::ROUTE_CORRIDOR_RADIUS * 3.0);
    if !attraction_anchors([&route], far, 32).is_empty() {
        record(
            &mut violations,
            "attraction leaked beyond the corridor".into(),
        );
    }

    // Traversal: once per leg, on exit.
    let mut tracker = RouteTracker::new();
    let mut bumps = 0;
    for i in 0..6 {
        bumps += tracker.observe([&route], (f64::from(i) * 300.0, 0.0)).len();
    }
    bumps += tracker.observe([&route], far).len();
    bumps += tracker.observe([&route], far).len();
    if bumps != 1 {
        record(
            &mut violations,
            format!("traversal fired {bumps} times for one leg"),
        );
    }

    VaultReport {
        name: "routes: globally bounded attraction, singleton usage, one bump per leg",
        violations,
        summary: format!(
            "route {fresh_pull:.3}; singleton {singleton_fresh:.3} → {worn_pull:.3}; dense peak {dense_peak:.3}"
        ),
    }
}

/// Precision preserved: persisted influence (a summoned discovery anchor and
/// a route corridor over fast domains) never regenerates the stable trio.
fn scenario_precision() -> VaultReport {
    let mut violations = Vec::new();
    let bias = [0.0f32; POSSIBILITY_DIMS];

    // Records: a fast-domain discovery and a route, persisted then reloaded.
    let mut vault = Vault::open(MemoryStorage::new()).unwrap();
    vault
        .record_discovery(&discovery_anchor(600.0, 0.9), 0, "pull".into())
        .expect("sequence available");
    let mut sig = PossibilitySignature::of(world_core::PossibilityVector::neutral());
    sig.buckets[PossibilityDomain::Ecology.index()] = 3400;
    let nodes: Vec<RouteNode> = (0..5)
        .map(|i| RouteNode {
            pos_q: (i * 250, 100),
            signature: sig,
            cost_q: 20,
            stability_q: 0,
            anchor_sig: 0,
        })
        .collect();
    vault
        .record_route(nodes, vec![], "corridor".into())
        .expect("sequence available");
    vault.flush_all().expect("memory store flush");
    let vault = Vault::open(vault.store().clone()).unwrap();

    let mut map = RegionMap::new(config());
    for _ in 0..4 {
        step(&mut map, (128.0, 128.0), 0.0, &[], &bias);
    }
    // Stable-trio ledger: content hashes fixed while resident.
    let mut trio: std::collections::BTreeMap<RegionCoord, (u64, u64)> = map
        .iter_active()
        .filter_map(|r| {
            let e = map
                .cache()
                .channel(r.coord, CHANNEL_ELEVATION)?
                .content_hash();
            let h = map
                .cache()
                .channel(r.coord, CHANNEL_HARDNESS)?
                .content_hash();
            Some((r.coord, (e, h)))
        })
        .collect();

    // Travel under the persisted influence. Macro drainage stability rides
    // the same ledger: a macro tile regenerating under fast-domain steering
    // would change its dependents' river tiles, which the trio check below
    // would surface via hydrology's *inputs* staying untouched (terrain and
    // geology hashes fixed while resident).
    let summoned: Vec<Anchor> = vault
        .discoveries()
        .values()
        .map(|r| r.to_anchor())
        .collect();
    for frame in 1..=30 {
        let player = (f64::from(frame) * 35.0, f64::from(frame) * 10.0);
        let mut anchors = summoned.clone();
        anchors.extend(attraction_anchors(vault.routes().values(), player, 32));
        step(&mut map, player, 40.0, &anchors, &bias);
        trio.retain(|c, _| map.get(*c).is_some());
        for (&coord, &(e, h)) in &trio {
            let now_e = map
                .cache()
                .channel(coord, CHANNEL_ELEVATION)
                .map(|t| t.content_hash());
            let now_h = map
                .cache()
                .channel(coord, CHANNEL_HARDNESS)
                .map(|t| t.content_hash());
            if now_e.is_some_and(|v| v != e) || now_h.is_some_and(|v| v != h) {
                record(
                    &mut violations,
                    format!(
                        "stable trio regenerated at ({}, {}) under persisted steering",
                        coord.x, coord.y
                    ),
                );
            }
        }
    }

    VaultReport {
        name: "precision: persisted influence touches only declared readers",
        violations,
        summary: "30 frames under discovery + route steering".into(),
    }
}

/// Run every vault-harness scenario (the Phase 5 sign-off, §12.3).
#[must_use]
pub fn run_vault_harness() -> Vec<VaultReport> {
    vec![
        scenario_durable(),
        scenario_crash_consistency(),
        scenario_persistence_failures(),
        scenario_sparse(),
        scenario_shareable(),
        scenario_preserve(),
        scenario_routes(),
        scenario_precision(),
    ]
}
