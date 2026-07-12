//! The vault: the sparse record store layered on [`Storage`]
//! (implementation-plan.md section 18; phase-5-plan.md §5.2, §7.6–7.7).
//!
//! The vault is the first (and only) user of the `Storage` trait. It owns no
//! world state — it holds loaded record indexes, a dirty-record queue, and the
//! store-local sequence counter, and it orchestrates encode/decode through the
//! `world-core` record codec (ADR 0013). Persistence obeys temporal budgeting
//! like every other subsystem: mutating actions mark records dirty in O(1),
//! and [`Vault::flush`] writes at most `Budget::max_persist_ops` records per
//! call, each key atomically (the `Storage` contract), so a crash mid-flush
//! loses at most un-flushed dirtiness and never corrupts a record.
//!
//! What the vault stores is *deviations only* (ADR 0014): quantized intents
//! and identities — never tiles, organisms, or geometry. The key namespace
//! (phase-5-plan.md §6.1):
//!
//! ```text
//! meta/store         store header (sequence counter; versions in the envelope)
//! session/current    the run-local SessionSnapshot (bit-exact, never shared)
//! disc/<id:016x>     one DiscoveryRecord per named discovery
//! route/<id:016x>    one RouteRecord per expedition
//! pres/<id:016x>     one PreserveRecord per preserve
//! seen/<x:08x><y:08x> one SeenRecord per visited 16×16-region chunk
//! ```
//!
//! Load order is irrelevant by construction: records are independent keys,
//! anchors combine order-independently (ADR 0011), and every index is keyed.

use std::collections::{BTreeMap, BTreeSet};

use world_core::{
    decode_record, encode_record, Anchor, AnchorSnapshot, AtlasBundle, DiscoveryRecord,
    PossibilitySignature, PreserveRecord, RecordError, RecordKind, RegionCoord,
    RegionSnapshotRecord, RouteNode, RouteRecord, SeenRecord, SessionSnapshot, StoreMeta,
    POSSIBILITY_DIMS, WORLD_ALGORITHM_VERSION,
};

use crate::budget::Budget;
use crate::storage::{Storage, StorageError};
use crate::stream::RegionMap;

/// The store-header key.
pub const KEY_META: &[u8] = b"meta/store";
/// The session-snapshot key (overwritten in place).
pub const KEY_SESSION: &[u8] = b"session/current";

/// Errors opening or flushing a vault. Per-record decode problems during open
/// are *not* errors — they are skipped and reported as [`Vault::issues`]
/// (reject the record, never the store) — but an unreadable or future-format
/// store header refuses to open rather than risk corrupting it.
#[derive(Debug)]
pub enum VaultError {
    /// The backend failed.
    Storage(StorageError),
    /// The store header is corrupt or from a newer format.
    Meta(RecordError),
}

impl core::fmt::Display for VaultError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VaultError::Storage(e) => write!(f, "vault storage error: {e}"),
            VaultError::Meta(e) => write!(f, "vault store header: {e}"),
        }
    }
}

impl core::error::Error for VaultError {}

impl From<StorageError> for VaultError {
    fn from(e: StorageError) -> Self {
        VaultError::Storage(e)
    }
}

/// Which record needs flushing. The `Ord` puts the meta header *after* the
/// records of a batch, so a budget-split flush persists data before advancing
/// the on-disk sequence (a stale counter heals on open; a dangling one would
/// not).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DirtyKey {
    Session,
    Discovery(u64),
    Route(u64),
    Preserve(u64),
    Seen(RegionCoord),
    Meta,
}

/// Counters returned by [`Vault::import`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeStats {
    /// Records inserted (unknown id).
    pub added: usize,
    /// Records whose mutable fields changed under the merge rules.
    pub merged: usize,
    /// Records already present and identical (idempotence in action).
    pub unchanged: usize,
    /// Records rejected (id mismatch) — reported in [`Vault::issues`].
    pub rejected: usize,
}

/// Counters returned by [`Vault::flush`] — the panel/harness telemetry
/// (phase-5-plan.md §8.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VaultStats {
    /// Records encoded and written by this call.
    pub flushed: usize,
    /// Bytes written by this call.
    pub bytes: usize,
    /// Records still dirty after this call (backpressure, not an error).
    pub dirty: usize,
}

/// The persistence orchestrator (phase-5-plan.md §5.2). Generic over
/// [`Storage`], so harnesses use `MemoryStorage` and the native shell a file
/// tree. See the module docs for the namespace and budgeting contract.
#[derive(Debug)]
pub struct Vault<S: Storage> {
    store: S,
    meta: StoreMeta,
    session: Option<SessionSnapshot>,
    discoveries: BTreeMap<u64, DiscoveryRecord>,
    routes: BTreeMap<u64, RouteRecord>,
    preserves: BTreeMap<u64, PreserveRecord>,
    seen: BTreeMap<RegionCoord, SeenRecord>,
    dirty: BTreeSet<DirtyKey>,
    /// Non-fatal problems found while opening (skipped records, world-version
    /// mismatches) — surfaced by the panel and `wer-atlas check`.
    issues: Vec<String>,
}

impl<S: Storage> Vault<S> {
    /// Open a store: read the header (refusing a future format), load every
    /// record, skip-and-report anything corrupt, and heal the sequence
    /// counter. A missing header means a fresh store.
    pub fn open(store: S) -> Result<Self, VaultError> {
        let mut vault = Self {
            store,
            meta: StoreMeta::default(),
            session: None,
            discoveries: BTreeMap::new(),
            routes: BTreeMap::new(),
            preserves: BTreeMap::new(),
            seen: BTreeMap::new(),
            dirty: BTreeSet::new(),
            issues: Vec::new(),
        };

        match vault.store.load(KEY_META) {
            Ok(bytes) => {
                let (envelope, meta): (world_core::Envelope, StoreMeta) =
                    decode_record(&bytes, RecordKind::Meta).map_err(VaultError::Meta)?;
                vault.meta = meta;
                if envelope.world_version != WORLD_ALGORITHM_VERSION {
                    vault.issues.push(format!(
                        "store was written under world algorithm v{} (this build is v{}): \
                         records keep their meaning, but the same buckets realize a different world",
                        envelope.world_version, WORLD_ALGORITHM_VERSION
                    ));
                }
            }
            Err(StorageError::NotFound) => {} // fresh store
            Err(e) => return Err(e.into()),
        }

        match vault.store.load(KEY_SESSION) {
            Ok(bytes) => match decode_record::<SessionSnapshot>(&bytes, RecordKind::Session) {
                Ok((_, snap)) => vault.session = Some(snap),
                Err(e) => vault.issues.push(format!("session snapshot skipped: {e}")),
            },
            Err(StorageError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }

        vault.load_namespace(b"disc/", RecordKind::Discovery, |v, r: DiscoveryRecord| {
            if r.id != r.content_id() {
                return Err("content id mismatch (corrupt or tampered)".into());
            }
            v.discoveries.insert(r.id, r);
            Ok(())
        })?;
        vault.load_namespace(b"route/", RecordKind::Route, |v, r: RouteRecord| {
            if r.id != r.content_id() {
                return Err("content id mismatch (corrupt or tampered)".into());
            }
            v.routes.insert(r.id, r);
            Ok(())
        })?;
        vault.load_namespace(b"pres/", RecordKind::Preserve, |v, r: PreserveRecord| {
            if r.id != r.content_id() {
                return Err("content id mismatch (corrupt or tampered)".into());
            }
            v.preserves.insert(r.id, r);
            Ok(())
        })?;
        vault.load_namespace(b"seen/", RecordKind::Seen, |v, r: SeenRecord| {
            v.seen.insert(r.chunk, r);
            Ok(())
        })?;

        // Heal a stale sequence counter (crash between record and meta writes):
        // the counter only needs monotonicity.
        let max_seen = vault
            .discoveries
            .values()
            .map(|r| r.sequence)
            .chain(vault.routes.values().map(|r| r.sequence))
            .chain(vault.preserves.values().map(|r| r.sequence))
            .chain(vault.session.iter().map(|s| s.sequence))
            .max()
            .unwrap_or(0);
        vault.meta.sequence = vault.meta.sequence.max(max_seen);

        Ok(vault)
    }

    /// Load one record namespace, skipping (and reporting) undecodable or
    /// invalid entries.
    fn load_namespace<T: serde::de::DeserializeOwned>(
        &mut self,
        prefix: &[u8],
        kind: RecordKind,
        mut insert: impl FnMut(&mut Self, T) -> Result<(), String>,
    ) -> Result<(), VaultError> {
        for key in self.store.keys_with_prefix(prefix)? {
            let bytes = match self.store.load(&key) {
                Ok(b) => b,
                Err(StorageError::NotFound) => continue, // raced away; harmless
                Err(e) => return Err(e.into()),
            };
            let display_key = String::from_utf8_lossy(&key).into_owned();
            match decode_record::<T>(&bytes, kind) {
                Ok((_, record)) => {
                    if let Err(why) = insert(self, record) {
                        self.issues.push(format!("{display_key} skipped: {why}"));
                    }
                }
                Err(e) => self.issues.push(format!("{display_key} skipped: {e}")),
            }
        }
        Ok(())
    }

    /// The backing store (telemetry, tests).
    #[inline]
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }

    /// Non-fatal problems found while opening.
    #[inline]
    #[must_use]
    pub fn issues(&self) -> &[String] {
        &self.issues
    }

    /// Records currently waiting to be flushed.
    #[inline]
    #[must_use]
    pub fn dirty_records(&self) -> usize {
        self.dirty.len()
    }

    /// The loaded discoveries, keyed by content id.
    #[inline]
    #[must_use]
    pub const fn discoveries(&self) -> &BTreeMap<u64, DiscoveryRecord> {
        &self.discoveries
    }

    /// The loaded routes, keyed by content id.
    #[inline]
    #[must_use]
    pub const fn routes(&self) -> &BTreeMap<u64, RouteRecord> {
        &self.routes
    }

    /// The loaded preserves, keyed by content id.
    #[inline]
    #[must_use]
    pub const fn preserves(&self) -> &BTreeMap<u64, PreserveRecord> {
        &self.preserves
    }

    /// The loaded session snapshot, if one was saved.
    #[inline]
    #[must_use]
    pub const fn session(&self) -> Option<&SessionSnapshot> {
        self.session.as_ref()
    }

    /// Whether a level-0 region has been marked discovered.
    #[must_use]
    pub fn is_seen(&self, region: RegionCoord) -> bool {
        self.seen
            .get(&SeenRecord::chunk_of(region))
            .is_some_and(|c| c.contains(region))
    }

    /// Total discovered regions.
    #[must_use]
    pub fn seen_count(&self) -> u64 {
        self.seen.values().map(|c| u64::from(c.count())).sum()
    }

    /// Hand out the next store sequence number.
    fn next_sequence(&mut self) -> u64 {
        self.meta.sequence += 1;
        self.dirty.insert(DirtyKey::Meta);
        self.meta.sequence
    }

    /// Persist a captured anchor as a named discovery (phase-5-plan.md §7.1):
    /// quantized at this boundary (ADR 0013), content-id keyed (ADR 0014).
    /// Re-recording an identical capture merges into the existing record
    /// instead of duplicating. Returns the record id.
    pub fn record_discovery(&mut self, anchor: &Anchor, signature_seed: u64, name: String) -> u64 {
        let sequence = self.next_sequence();
        let record = DiscoveryRecord::from_anchor(anchor, signature_seed, sequence, name);
        let id = record.id;
        match self.discoveries.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing.get_mut().merge_from(&record) {
                    self.dirty.insert(DirtyKey::Discovery(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Discovery(id));
            }
        }
        id
    }

    /// Persist a preserve: each region's possibility state, quantized
    /// (phase-5-plan.md §7.5). Returns the record id.
    pub fn record_preserve(
        &mut self,
        regions: Vec<(RegionCoord, PossibilitySignature)>,
        name: String,
    ) -> u64 {
        let sequence = self.next_sequence();
        let record = PreserveRecord::new(regions, sequence, name);
        let id = record.id;
        match self.preserves.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing.get_mut().merge_from(&record) {
                    self.dirty.insert(DirtyKey::Preserve(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Preserve(id));
            }
        }
        id
    }

    /// Delete a preserve record. Returns whether it existed. (The caller
    /// clears the map's overrides; the regions rejoin normal steering from
    /// their preserved state — no snap, phase-5-plan.md §7.5.)
    pub fn remove_preserve(&mut self, id: u64) -> bool {
        if self.preserves.remove(&id).is_some() {
            self.dirty.remove(&DirtyKey::Preserve(id));
            // Remove eagerly: deletion is rare and leaving the key would
            // resurrect the record on next open.
            let _ = self.store.remove(&preserve_key(id));
            return true;
        }
        false
    }

    /// Delete a route record. Returns whether it existed. (Mirrors
    /// [`Self::remove_preserve`]: deletion is rare, so the stored key is
    /// removed eagerly rather than resurrecting the record on next open.)
    pub fn remove_route(&mut self, id: u64) -> bool {
        if self.routes.remove(&id).is_some() {
            self.dirty.remove(&DirtyKey::Route(id));
            let _ = self.store.remove(&route_key(id));
            return true;
        }
        false
    }

    /// Persist a recorded expedition (phase-5-plan.md §7.3). Returns the id.
    pub fn record_route(
        &mut self,
        nodes: Vec<RouteNode>,
        discoveries: Vec<u64>,
        name: String,
    ) -> u64 {
        let sequence = self.next_sequence();
        let record = RouteRecord::new(nodes, discoveries, sequence, name);
        let id = record.id;
        match self.routes.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing.get_mut().merge_from(&record) {
                    self.dirty.insert(DirtyKey::Route(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Route(id));
            }
        }
        id
    }

    /// Bump a route's traversal count (phase-5-plan.md §7.4).
    pub fn bump_route_usage(&mut self, id: u64) {
        if let Some(route) = self.routes.get_mut(&id) {
            route.usage = route.usage.saturating_add(1);
            self.dirty.insert(DirtyKey::Route(id));
        }
    }

    /// Mark a level-0 region discovered. Cheap and idempotent; only a new bit
    /// dirties its chunk.
    pub fn mark_seen(&mut self, region: RegionCoord) {
        let chunk = SeenRecord::chunk_of(region);
        let record = self
            .seen
            .entry(chunk)
            .or_insert_with(|| SeenRecord::empty(chunk));
        if record.mark(region) {
            self.dirty.insert(DirtyKey::Seen(chunk));
        }
    }

    /// Snapshot the run-local session tier (phase-5-plan.md §4.5): player,
    /// bias, live anchors bit-exact, and every resident region's authoritative
    /// state. Queued for flushing; overwrites the previous snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn snapshot_session(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        last_player: (f64, f64),
        bias: &[f32; POSSIBILITY_DIMS],
        transition_mode: bool,
        anchors: &[Anchor],
    ) {
        let sequence = self.next_sequence();
        let snap = SessionSnapshot {
            player,
            last_player,
            bias: *bias,
            transition_mode,
            anchors: anchors.iter().map(AnchorSnapshot::from_anchor).collect(),
            regions: map
                .iter_active()
                .map(|r| RegionSnapshotRecord {
                    coord: r.coord,
                    current: r.current.dims,
                    stability: r.stability,
                    revision: r.revision,
                })
                .collect(),
            sequence,
        };
        self.session = Some(snap);
        self.dirty.insert(DirtyKey::Session);
    }

    /// Export the shareable tier as a canonical bundle (phase-5-plan.md §4.5):
    /// discoveries, routes, and preserves, sorted by id — never the session
    /// tier or the seen-set.
    #[must_use]
    pub fn export(&self) -> AtlasBundle {
        let mut bundle = AtlasBundle {
            discoveries: self.discoveries.values().cloned().collect(),
            routes: self.routes.values().cloned().collect(),
            preserves: self.preserves.values().cloned().collect(),
        };
        bundle.canonicalize();
        bundle
    }

    /// Merge a bundle into this store (phase-5-plan.md §7.6, ADR 0014): union
    /// by content id; a record whose stored id mismatches its recomputed fold
    /// is rejected with a report, never partially applied. Commutative,
    /// associative, idempotent — re-importing the same bundle changes nothing
    /// and never double-counts usage.
    pub fn import(&mut self, bundle: &AtlasBundle) -> MergeStats {
        use std::collections::btree_map::Entry;
        let mut stats = MergeStats::default();

        macro_rules! merge_namespace {
            ($records:expr, $index:expr, $dirty:expr) => {
                for record in $records {
                    if record.id != record.content_id() {
                        stats.rejected += 1;
                        self.issues.push(format!(
                            "import rejected {:#018x}: content id mismatch (corrupt or tampered)",
                            record.id
                        ));
                        continue;
                    }
                    match $index.entry(record.id) {
                        Entry::Occupied(mut existing) => {
                            if existing.get_mut().merge_from(record) {
                                self.dirty.insert($dirty(record.id));
                                stats.merged += 1;
                            } else {
                                stats.unchanged += 1;
                            }
                        }
                        Entry::Vacant(slot) => {
                            slot.insert(record.clone());
                            self.dirty.insert($dirty(record.id));
                            stats.added += 1;
                        }
                    }
                }
            };
        }

        merge_namespace!(&bundle.discoveries, self.discoveries, DirtyKey::Discovery);
        merge_namespace!(&bundle.routes, self.routes, DirtyKey::Route);
        merge_namespace!(&bundle.preserves, self.preserves, DirtyKey::Preserve);
        stats
    }

    /// Write dirty records, at most `budget.max_persist_ops` per call, in
    /// deterministic order (records before the meta header). Deferred work is
    /// healthy backpressure; each key write is atomic (§7.7).
    pub fn flush(&mut self, budget: &Budget) -> VaultStats {
        let mut stats = VaultStats::default();
        while stats.flushed < budget.max_persist_ops {
            let Some(&key) = self.dirty.iter().next() else {
                break;
            };
            let bytes = match key {
                DirtyKey::Session => self
                    .session
                    .as_ref()
                    .map(|s| (KEY_SESSION.to_vec(), encode_record(RecordKind::Session, s))),
                DirtyKey::Discovery(id) => self
                    .discoveries
                    .get(&id)
                    .map(|r| (discovery_key(id), encode_record(RecordKind::Discovery, r))),
                DirtyKey::Route(id) => self
                    .routes
                    .get(&id)
                    .map(|r| (route_key(id), encode_record(RecordKind::Route, r))),
                DirtyKey::Preserve(id) => self
                    .preserves
                    .get(&id)
                    .map(|r| (preserve_key(id), encode_record(RecordKind::Preserve, r))),
                DirtyKey::Seen(chunk) => self
                    .seen
                    .get(&chunk)
                    .map(|r| (seen_key(chunk), encode_record(RecordKind::Seen, r))),
                DirtyKey::Meta => Some((
                    KEY_META.to_vec(),
                    encode_record(RecordKind::Meta, &self.meta),
                )),
            };
            if let Some((store_key, bytes)) = bytes {
                if let Err(e) = self.store.store(&store_key, &bytes) {
                    // Keep the record dirty for a later retry, but stop this
                    // call: a full disk or a permissions change must not wedge
                    // the frame loop hammering a dead backend.
                    self.issues.push(format!(
                        "flush of {} failed: {e}",
                        String::from_utf8_lossy(&store_key)
                    ));
                    break;
                }
                stats.flushed += 1;
                stats.bytes += bytes.len();
            }
            self.dirty.remove(&key);
        }
        stats.dirty = self.dirty.len();
        stats
    }

    /// Flush everything (tools, shutdown).
    pub fn flush_all(&mut self) -> VaultStats {
        let mut total = VaultStats::default();
        loop {
            let stats = self.flush(&Budget::unlimited());
            total.flushed += stats.flushed;
            total.bytes += stats.bytes;
            total.dirty = stats.dirty;
            if stats.dirty == 0 || stats.flushed == 0 {
                break;
            }
        }
        total
    }
}

/// Restore a session's resident window into a fresh [`RegionMap`]
/// (phase-5-plan.md §12.2). Every region comes back with its bit-exact
/// `current`, stability, and revision, all layers dirty; follow with one
/// settle update (travel = 0, the session's anchors/bias) so caches, rosters,
/// and organisms re-derive deterministically before the journey continues —
/// loading is not an event (no convergence, no target motion).
pub fn apply_session_regions(map: &mut RegionMap, snap: &SessionSnapshot) {
    for region in &snap.regions {
        map.restore_region(region);
    }
}

/// The storage key of a discovery record.
#[must_use]
pub fn discovery_key(id: u64) -> Vec<u8> {
    format!("disc/{id:016x}").into_bytes()
}

/// The storage key of a route record.
#[must_use]
pub fn route_key(id: u64) -> Vec<u8> {
    format!("route/{id:016x}").into_bytes()
}

/// The storage key of a preserve record.
#[must_use]
pub fn preserve_key(id: u64) -> Vec<u8> {
    format!("pres/{id:016x}").into_bytes()
}

/// The storage key of a seen chunk (coordinates as unsigned bit patterns).
#[must_use]
pub fn seen_key(chunk: RegionCoord) -> Vec<u8> {
    format!("seen/{:08x}{:08x}", chunk.x as u32, chunk.y as u32).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use world_core::{bound_target, domain_mask, AnchorKind, AnchorSource, PossibilityDomain};

    fn sample_anchor() -> Anchor {
        let mask = domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Morphology]);
        Anchor {
            world_pos: (300.0, -10.0),
            target: bound_target(mask, 0.9),
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.8,
            falloff_radius: 1500.0,
            source: AnchorSource::Landform,
        }
    }

    #[test]
    fn records_survive_reopen() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        let disc = vault.record_discovery(&sample_anchor(), 0xABCD, "spire".into());
        let pres = vault.record_preserve(
            vec![(
                RegionCoord::new(1, 2),
                PossibilitySignature::of(world_core::PossibilityVector::neutral()),
            )],
            "glade".into(),
        );
        vault.mark_seen(RegionCoord::new(1, 2));
        vault.mark_seen(RegionCoord::new(-40, 7));
        let stats = vault.flush_all();
        assert_eq!(stats.dirty, 0);
        assert!(stats.flushed >= 5); // 2 records + 2 seen chunks + meta

        let reopened = Vault::open(vault.store().clone()).unwrap();
        assert!(reopened.issues().is_empty(), "{:?}", reopened.issues());
        assert!(reopened.discoveries().contains_key(&disc));
        assert_eq!(reopened.discoveries()[&disc].name, "spire");
        assert!(reopened.preserves().contains_key(&pres));
        assert!(reopened.is_seen(RegionCoord::new(1, 2)));
        assert!(reopened.is_seen(RegionCoord::new(-40, 7)));
        assert!(!reopened.is_seen(RegionCoord::new(2, 2)));
        assert_eq!(reopened.seen_count(), 2);
    }

    #[test]
    fn rerecording_the_same_capture_deduplicates() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        let a = vault.record_discovery(&sample_anchor(), 0xABCD, "first".into());
        let b = vault.record_discovery(&sample_anchor(), 0xABCD, "second".into());
        assert_eq!(a, b, "same capture ⇒ same content id");
        assert_eq!(vault.discoveries().len(), 1);
        // The later name wins (higher sequence).
        assert_eq!(vault.discoveries()[&a].name, "second");
    }

    #[test]
    fn flush_is_budgeted_and_makes_progress() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        for i in 0..10 {
            let mut anchor = sample_anchor();
            anchor.world_pos.0 += f64::from(i) * 10.0;
            vault.record_discovery(&anchor, 0, format!("d{i}"));
        }
        let budget = Budget {
            max_persist_ops: 3,
            ..Budget::unlimited()
        };
        let first = vault.flush(&budget);
        assert_eq!(first.flushed, 3);
        assert!(first.dirty > 0);
        let mut guard = 0;
        while vault.dirty_records() > 0 {
            vault.flush(&budget);
            guard += 1;
            assert!(guard < 100, "flush must drain");
        }
        let reopened = Vault::open(vault.store().clone()).unwrap();
        assert_eq!(reopened.discoveries().len(), 10);
    }

    #[test]
    fn corrupt_records_are_skipped_with_a_report_never_a_panic() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        let id = vault.record_discovery(&sample_anchor(), 0xABCD, "ok".into());
        vault.flush_all();
        let mut store = vault.store().clone();
        // Tamper one record and drop garbage into the namespace.
        let key = discovery_key(id);
        let mut bytes = store.load(&key).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        store.store(&key, &bytes).unwrap();
        store.store(b"disc/nonsense", b"not a record").unwrap();

        let reopened = Vault::open(store).unwrap();
        assert_eq!(reopened.discoveries().len(), 0, "tampered record rejected");
        assert_eq!(reopened.issues().len(), 2, "{:?}", reopened.issues());
    }

    /// A small distinct anchor for merge tests.
    fn anchor_at(x: f64) -> Anchor {
        Anchor {
            world_pos: (x, 0.0),
            ..sample_anchor()
        }
    }

    #[test]
    fn merge_laws_hold_at_the_vault_level() {
        // Three stores with overlapping and disjoint records.
        let mut a = Vault::open(MemoryStorage::new()).unwrap();
        a.record_discovery(&anchor_at(0.0), 1, "shared".into());
        a.record_discovery(&anchor_at(100.0), 1, "only-a".into());
        let mut b = Vault::open(MemoryStorage::new()).unwrap();
        b.record_discovery(&anchor_at(0.0), 1, "shared-renamed".into());
        b.record_discovery(&anchor_at(200.0), 1, "only-b".into());
        let mut c = Vault::open(MemoryStorage::new()).unwrap();
        c.record_discovery(&anchor_at(300.0), 1, "only-c".into());

        let (ea, eb, ec) = (a.export(), b.export(), c.export());

        // Commutative: a←b equals b←a.
        let mut ab = Vault::open(MemoryStorage::new()).unwrap();
        ab.import(&ea);
        ab.import(&eb);
        let mut ba = Vault::open(MemoryStorage::new()).unwrap();
        ba.import(&eb);
        ba.import(&ea);
        assert_eq!(ab.export(), ba.export());

        // Associative: (a∪b)∪c equals a∪(b∪c).
        let mut bc = Vault::open(MemoryStorage::new()).unwrap();
        bc.import(&eb);
        bc.import(&ec);
        let mut left = Vault::open(MemoryStorage::new()).unwrap();
        left.import(&ab.export());
        left.import(&ec);
        let mut right = Vault::open(MemoryStorage::new()).unwrap();
        right.import(&ea);
        right.import(&bc.export());
        assert_eq!(left.export(), right.export());

        // Idempotent: re-importing changes nothing.
        let before = ab.export();
        let stats = ab.import(&before);
        assert_eq!(stats.added + stats.merged + stats.rejected, 0);
        assert_eq!(ab.export(), before);
        assert_eq!(ab.export().discoveries.len(), 3);
    }

    #[test]
    fn import_rejects_tampered_records_without_partial_apply() {
        let mut a = Vault::open(MemoryStorage::new()).unwrap();
        a.record_discovery(&anchor_at(0.0), 1, "good".into());
        let mut bundle = a.export();
        bundle.discoveries[0].strength_q ^= 1; // immutable field no longer matches id
        let mut b = Vault::open(MemoryStorage::new()).unwrap();
        let stats = b.import(&bundle);
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.added, 0);
        assert!(b.discoveries().is_empty());
        assert_eq!(b.issues().len(), 1);
    }

    #[test]
    fn route_usage_never_double_counts_across_import() {
        let mut a = Vault::open(MemoryStorage::new()).unwrap();
        let node = RouteNode {
            pos_q: (0, 0),
            signature: PossibilitySignature::of(world_core::PossibilityVector::neutral()),
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
        };
        let id = a.record_route(vec![node], vec![], "trail".into());
        a.bump_route_usage(id);
        a.bump_route_usage(id);
        let bundle = a.export();
        let mut b = Vault::open(MemoryStorage::new()).unwrap();
        b.import(&bundle);
        b.import(&bundle);
        b.import(&bundle);
        assert_eq!(b.routes()[&id].usage, 2, "max-merge, not sum");
    }

    #[test]
    fn sequence_heals_from_records_after_a_stale_meta() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        vault.record_discovery(&sample_anchor(), 0xABCD, "one".into());
        // Flush only the record batch's first entries, never re-flushing meta
        // (simulates a crash between record and meta writes): record sequence
        // is 1 but the stored meta still says 0.
        let budget = Budget {
            max_persist_ops: 1,
            ..Budget::unlimited()
        };
        vault.flush(&budget); // writes the discovery (records sort before meta)
        let reopened = Vault::open(vault.store().clone()).unwrap();
        // Healed: the next sequence must exceed the record's.
        let mut reopened = reopened;
        let mut anchor = sample_anchor();
        anchor.world_pos.0 += 1000.0;
        let id = reopened.record_discovery(&anchor, 0, "two".into());
        assert!(reopened.discoveries()[&id].sequence > 1);
    }
}
