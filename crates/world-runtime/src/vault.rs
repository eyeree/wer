//! The vault: the sparse record store layered on [`Storage`]
//! (implementation-plan.md section 18; phase-5-plan.md §5.2, §7.6–7.7).
//!
//! The vault is the first (and only) user of the `Storage` trait. It owns no
//! world state — it holds loaded record indexes, a dirty-record queue, and the
//! store-local sequence counter, and it orchestrates encode/decode through the
//! `world-core` record codec (ADR 0013). Persistence obeys temporal budgeting
//! like every other subsystem: mutating actions insert ordered dirty keys, and
//! [`Vault::flush`] durably writes at most `Budget::max_persist_ops` records
//! per call. Each key retires only after backend success, data precedes
//! metadata, and failures return structured retryable state (ADR 0022).
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
    decode_record, encode_record, Anchor, AnchorSnapshot, AtlasBundle, BudgetRecord,
    DiscoveryRecord, LegacyTargetPolicy, PossibilitySignature, PreserveRecord,
    RecordCanonicalError, RecordError, RecordKind, RegionCoord, RegionSnapshotRecord, RouteNode,
    RouteRecord, RouteRecorderSnapshot, RouteTrackerSnapshot, SeenRecord, SessionRuntimeRecord,
    SessionSnapshot, SessionTierRecord, StoreMeta, StreamConfigRecord, POSSIBILITY_DIMS,
    WORLD_ALGORITHM_VERSION,
};

use crate::budget::Budget;
use crate::storage::{Storage, StorageError};
use crate::stream::RegionMap;
use crate::tier::ResourceTier;

/// The store-header key.
pub const KEY_META: &[u8] = b"meta/store";
/// The session-snapshot key (overwritten in place).
pub const KEY_SESSION: &[u8] = b"session/current";

/// Maximum number of nonfatal/active issue entries retained by a vault.
pub const MAX_VAULT_ISSUES: usize = 64;

/// Fully-typed input for a run-local session snapshot. Adding a persisted
/// session field should extend this struct, not widen a positional API.
#[derive(Debug)]
pub struct SessionSnapshotInput<'a> {
    pub map: &'a RegionMap,
    pub player: (f64, f64),
    pub last_player: (f64, f64),
    pub bias: &'a [f32; POSSIBILITY_DIMS],
    pub transition_mode: bool,
    pub anchors: &'a [Anchor],
    pub runtime: SessionRuntimeRecord,
    pub recorder: Option<RouteRecorderSnapshot>,
    pub tracker: RouteTrackerSnapshot,
}

/// Owned, action-ordered session values captured by a shared viewer reducer.
///
/// Unlike [`SessionSnapshotInput`], this form does not re-read a live
/// [`RegionMap`] when a platform processes the persistence effect later. The
/// vault still assigns the authoritative store sequence at write time.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSnapshotOwnedInput {
    pub runtime: SessionRuntimeRecord,
    pub player: (f64, f64),
    pub last_player: (f64, f64),
    pub bias: [f32; POSSIBILITY_DIMS],
    pub transition_mode: bool,
    pub anchors: Vec<AnchorSnapshot>,
    pub regions: Vec<RegionSnapshotRecord>,
    pub recorder: Option<RouteRecorderSnapshot>,
    pub tracker: RouteTrackerSnapshot,
}

/// Session metadata comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCompatibility {
    Exact,
    CompatibleNotExact,
    Incompatible,
}

/// Runtime metadata could not be represented on this platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionConfigError {
    UsizeOverflow,
}

#[must_use]
pub fn stream_config_record(cfg: &crate::stream::StreamConfig) -> StreamConfigRecord {
    StreamConfigRecord {
        near_radius: cfg.near_radius,
        far_radius: cfg.far_radius,
        load_radius: cfg.load_radius,
        unload_radius: cfg.unload_radius,
        converge_per_unit: cfg.converge_per_unit,
        converge_rate_cap: cfg.converge_rate_cap,
        field_resolution: cfg.field_resolution,
        max_field_cache_bytes: cfg.max_field_cache_bytes as u64,
        max_macro_cache_bytes: cfg.max_macro_cache_bytes as u64,
        max_roster_cache_bytes: cfg.max_roster_cache_bytes as u64,
        organisms_per_cell: cfg.organisms_per_cell,
    }
}

pub fn stream_config_from_record(
    record: &StreamConfigRecord,
) -> Result<crate::stream::StreamConfig, SessionConfigError> {
    Ok(crate::stream::StreamConfig {
        near_radius: record.near_radius,
        far_radius: record.far_radius,
        load_radius: record.load_radius,
        unload_radius: record.unload_radius,
        converge_per_unit: record.converge_per_unit,
        converge_rate_cap: record.converge_rate_cap,
        field_resolution: record.field_resolution,
        max_field_cache_bytes: usize::try_from(record.max_field_cache_bytes)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_macro_cache_bytes: usize::try_from(record.max_macro_cache_bytes)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_roster_cache_bytes: usize::try_from(record.max_roster_cache_bytes)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        organisms_per_cell: record.organisms_per_cell,
    })
}

#[must_use]
pub fn budget_record(budget: &Budget) -> BudgetRecord {
    BudgetRecord {
        max_loads: budget.max_loads as u64,
        max_converge_regions: budget.max_converge_regions as u64,
        max_regen_cost: budget.max_regen_cost,
        max_realize_organisms: budget.max_realize_organisms as u64,
        max_persist_ops: budget.max_persist_ops as u64,
        max_route_attraction_nodes: budget.max_route_attraction_nodes as u64,
        max_retarget_regions: budget.max_retarget_regions as u64,
    }
}

pub fn budget_from_record(record: &BudgetRecord) -> Result<Budget, SessionConfigError> {
    Ok(Budget {
        max_loads: usize::try_from(record.max_loads)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_converge_regions: usize::try_from(record.max_converge_regions)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_regen_cost: record.max_regen_cost,
        max_realize_organisms: usize::try_from(record.max_realize_organisms)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_persist_ops: usize::try_from(record.max_persist_ops)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_route_attraction_nodes: usize::try_from(record.max_route_attraction_nodes)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
        max_retarget_regions: usize::try_from(record.max_retarget_regions)
            .map_err(|_| SessionConfigError::UsizeOverflow)?,
    })
}

#[must_use]
pub const fn tier_record(tier: Option<ResourceTier>) -> SessionTierRecord {
    match tier {
        Some(ResourceTier::Low) => SessionTierRecord::Low,
        Some(ResourceTier::Mid) => SessionTierRecord::Mid,
        Some(ResourceTier::High) => SessionTierRecord::High,
        None => SessionTierRecord::Unknown,
    }
}

#[must_use]
pub fn session_runtime_record(
    stream: &crate::stream::StreamConfig,
    budget: &Budget,
    tier: Option<ResourceTier>,
    path_tracking: bool,
    route_attraction: bool,
) -> SessionRuntimeRecord {
    SessionRuntimeRecord {
        stream: stream_config_record(stream),
        budget: budget_record(budget),
        tier: tier_record(tier),
        path_tracking,
        route_attraction,
        legacy_target_policy: LegacyTargetPolicy::ExactTargetStored,
    }
}

#[must_use]
pub fn compare_session_runtime(
    snapshot: &SessionRuntimeRecord,
    stream: &crate::stream::StreamConfig,
    budget: &Budget,
    tier: Option<ResourceTier>,
    path_tracking: bool,
    route_attraction: bool,
) -> SessionCompatibility {
    let current = session_runtime_record(stream, budget, tier, path_tracking, route_attraction);
    if snapshot == &current {
        return SessionCompatibility::Exact;
    }
    let mut pacing_only_budget = current.budget;
    pacing_only_budget.max_persist_ops = snapshot.budget.max_persist_ops;
    if snapshot.legacy_target_policy == LegacyTargetPolicy::TargetEqualsCurrent
        || snapshot.stream != current.stream
        || snapshot.budget != pacing_only_budget
        || snapshot.tier != current.tier
        || snapshot.path_tracking != current.path_tracking
        || snapshot.route_attraction != current.route_attraction
    {
        return SessionCompatibility::Incompatible;
    }
    SessionCompatibility::CompatibleNotExact
}

/// The storage mutation which failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceOperation {
    /// A key/value write.
    Store,
    /// A key removal (or durable confirmation of absence).
    Remove,
}

/// The store-local sequence counter has no strictly newer `u64` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct VaultSequenceError {
    last_sequence: u64,
}

impl VaultSequenceError {
    /// The exhausted counter value (currently always [`u64::MAX`]).
    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}

impl core::fmt::Display for VaultSequenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "vault sequence exhausted at {}; no strictly newer local edit is representable",
            self.last_sequence
        )
    }
}

impl core::error::Error for VaultSequenceError {}

impl core::fmt::Display for PersistenceOperation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Store => f.write_str("store"),
            Self::Remove => f.write_str("remove"),
        }
    }
}

/// A structured, retryable vault persistence failure (ADR 0022).
#[derive(Debug)]
#[must_use]
pub struct VaultPersistenceError {
    operation: PersistenceOperation,
    key: Vec<u8>,
    source: StorageError,
    occurrences: u64,
}

impl VaultPersistenceError {
    /// The mutation attempted by the vault.
    #[must_use]
    pub const fn operation(&self) -> PersistenceOperation {
        self.operation
    }

    /// The exact storage key involved in the failure.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// The backend failure.
    #[must_use]
    pub const fn source_error(&self) -> &StorageError {
        &self.source
    }

    /// Consecutive reports for this active operation/key.
    #[must_use]
    pub const fn occurrences(&self) -> u64 {
        self.occurrences
    }
}

impl core::fmt::Display for VaultPersistenceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "vault {} of {} failed: {} (occurrence {})",
            self.operation,
            String::from_utf8_lossy(&self.key),
            self.source,
            self.occurrences
        )
    }
}

impl core::error::Error for VaultPersistenceError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// A failed budgeted flush together with the progress made before it stopped.
#[derive(Debug)]
#[must_use]
pub struct VaultFlushError {
    progress: VaultStats,
    error: VaultPersistenceError,
}

impl VaultFlushError {
    /// Successful work and remaining dirtiness at the failure boundary.
    #[must_use]
    pub const fn progress(&self) -> VaultStats {
        self.progress
    }

    /// The first backend failure.
    #[must_use = "the persistence failure carries operation and key context"]
    pub const fn persistence_error(&self) -> &VaultPersistenceError {
        &self.error
    }

    fn into_parts(self) -> (VaultStats, VaultPersistenceError) {
        (self.progress, self.error)
    }
}

impl core::fmt::Display for VaultFlushError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{} after flushing {} record(s) / {} byte(s); {} record(s) remain dirty",
            self.error, self.progress.flushed, self.progress.bytes, self.progress.dirty
        )
    }
}

impl core::error::Error for VaultFlushError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// One retained, deduplicated nonfatal or active persistence diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultIssue {
    message: String,
    occurrences: u64,
    identity: IssueIdentity,
}

impl VaultIssue {
    /// Human-readable diagnostic text.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Number of reports deduplicated into this entry.
    #[must_use]
    pub const fn occurrences(&self) -> u64 {
        self.occurrences
    }
}

impl core::fmt::Display for VaultIssue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.message)?;
        if self.occurrences > 1 {
            write!(f, " (repeated {} times)", self.occurrences)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum IssueIdentity {
    Stored(Vec<u8>),
    Import(RecordKind, u64, &'static str),
    Persistence(PersistenceOperation, Vec<u8>),
}

/// Errors opening a vault. Per-record decode problems during open
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

impl VaultStats {
    /// Whether no queued mutation remains.
    #[must_use]
    pub const fn is_clean(self) -> bool {
        self.dirty == 0
    }
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
    issues: Vec<VaultIssue>,
    suppressed_issues: u64,
}

impl<S: Storage> Vault<S> {
    fn record_issue(&mut self, identity: IssueIdentity, message: String) {
        if let Some(issue) = self
            .issues
            .iter_mut()
            .find(|issue| issue.identity == identity)
        {
            issue.occurrences = issue.occurrences.saturating_add(1);
            issue.message = message;
            return;
        }
        if self.issues.len() == MAX_VAULT_ISSUES {
            self.suppressed_issues = self.suppressed_issues.saturating_add(1);
            return;
        }
        self.issues.push(VaultIssue {
            message,
            occurrences: 1,
            identity,
        });
    }

    fn record_persistence_failure(
        &mut self,
        operation: PersistenceOperation,
        key: Vec<u8>,
        source: StorageError,
    ) -> VaultPersistenceError {
        let identity = IssueIdentity::Persistence(operation, key.clone());
        if let Some(index) = self
            .issues
            .iter()
            .position(|issue| matches!(issue.identity, IssueIdentity::Persistence(_, _)))
        {
            if self.issues[index].identity == identity {
                let issue = &mut self.issues[index];
                issue.occurrences = issue.occurrences.saturating_add(1);
                issue.message = format!(
                    "vault {operation} of {} failed: {source}",
                    String::from_utf8_lossy(&key)
                );
                return VaultPersistenceError {
                    operation,
                    key,
                    source,
                    occurrences: issue.occurrences,
                };
            }
            self.issues.remove(index);
        }

        let message = format!(
            "vault {operation} of {} failed: {source}",
            String::from_utf8_lossy(&key)
        );
        let issue = VaultIssue {
            message,
            occurrences: 1,
            identity,
        };
        if self.issues.len() == MAX_VAULT_ISSUES {
            // Persistence must always remain visible. With no previous active
            // entry, every retained entry is nonfatal; replace the newest.
            self.issues[MAX_VAULT_ISSUES - 1] = issue;
            self.suppressed_issues = self.suppressed_issues.saturating_add(1);
        } else {
            self.issues.push(issue);
        }
        VaultPersistenceError {
            operation,
            key,
            source,
            occurrences: 1,
        }
    }

    fn clear_persistence_issue(&mut self, operation: PersistenceOperation, key: &[u8]) -> bool {
        let Some(index) = self.issues.iter().position(|issue| {
            matches!(
                &issue.identity,
                IssueIdentity::Persistence(found_operation, found_key)
                    if *found_operation == operation && found_key == key
            )
        }) else {
            return false;
        };
        self.issues.remove(index);
        true
    }

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
            suppressed_issues: 0,
        };

        match vault.store.load(KEY_META) {
            Ok(bytes) => {
                let (envelope, meta): (world_core::Envelope, StoreMeta) =
                    decode_record(&bytes, RecordKind::Meta).map_err(VaultError::Meta)?;
                vault.meta = meta;
                if envelope.world_version != WORLD_ALGORITHM_VERSION {
                    vault.record_issue(IssueIdentity::Stored(KEY_META.to_vec()), format!(
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
                Err(e) => vault.record_issue(
                    IssueIdentity::Stored(KEY_SESSION.to_vec()),
                    format!("session snapshot skipped: {e}"),
                ),
            },
            Err(StorageError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }

        vault.load_namespace(
            b"disc/",
            RecordKind::Discovery,
            |v, key, r: DiscoveryRecord| {
                require_record_key(key, b"disc/", r.id)?;
                r.validate_canonical().map_err(|e| e.to_string())?;
                v.discoveries.insert(r.id, r);
                Ok(())
            },
        )?;
        vault.load_namespace(b"route/", RecordKind::Route, |v, key, r: RouteRecord| {
            require_record_key(key, b"route/", r.id)?;
            r.validate_canonical().map_err(|e| e.to_string())?;
            v.routes.insert(r.id, r);
            Ok(())
        })?;
        vault.load_namespace(
            b"pres/",
            RecordKind::Preserve,
            |v, key, r: PreserveRecord| {
                require_record_key(key, b"pres/", r.id)?;
                r.validate_canonical().map_err(|e| e.to_string())?;
                v.preserves.insert(r.id, r);
                Ok(())
            },
        )?;
        vault.load_namespace(b"seen/", RecordKind::Seen, |v, _key, r: SeenRecord| {
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
    fn load_namespace<T: serde::de::DeserializeOwned + 'static>(
        &mut self,
        prefix: &[u8],
        kind: RecordKind,
        mut insert: impl FnMut(&mut Self, &[u8], T) -> Result<(), String>,
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
                    if let Err(why) = insert(self, &key, record) {
                        self.record_issue(
                            IssueIdentity::Stored(key.clone()),
                            format!("{display_key} skipped: {why}"),
                        );
                    }
                }
                Err(e) => self.record_issue(
                    IssueIdentity::Stored(key),
                    format!("{display_key} skipped: {e}"),
                ),
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
    pub fn issues(&self) -> impl ExactSizeIterator<Item = &VaultIssue> {
        self.issues.iter()
    }

    /// Number of retained diagnostics.
    #[inline]
    #[must_use]
    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Number of new diagnostic identities displaced or omitted at the cap.
    #[inline]
    #[must_use]
    pub const fn suppressed_issue_count(&self) -> u64 {
        self.suppressed_issues
    }

    /// The one active persistence diagnostic, if a retryable mutation is
    /// currently failing.
    #[must_use]
    pub fn active_persistence_issue(&self) -> Option<&VaultIssue> {
        self.issues
            .iter()
            .find(|issue| matches!(issue.identity, IssueIdentity::Persistence(_, _)))
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
    fn next_sequence(&mut self) -> Result<u64, VaultSequenceError> {
        let Some(next) = self.meta.sequence.checked_add(1) else {
            return Err(VaultSequenceError {
                last_sequence: self.meta.sequence,
            });
        };
        self.meta.sequence = next;
        self.dirty.insert(DirtyKey::Meta);
        Ok(self.meta.sequence)
    }

    /// Persist a captured anchor as a named discovery (phase-5-plan.md §7.1):
    /// quantized at this boundary (ADR 0013), content-id keyed (ADR 0014).
    /// Re-recording an identical capture merges into the existing record
    /// instead of duplicating. Returns the record id, or
    /// [`VaultSequenceError`] before mutation when the local counter is
    /// exhausted.
    pub fn record_discovery(
        &mut self,
        anchor: &Anchor,
        signature_seed: u64,
        name: String,
    ) -> Result<u64, VaultSequenceError> {
        let sequence = self.next_sequence()?;
        let record = DiscoveryRecord::from_anchor(anchor, signature_seed, sequence, name);
        let id = record.id;
        match self.discoveries.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing
                    .get_mut()
                    .merge_from(&record)
                    .expect("same constructor output must have equal immutable body")
                {
                    self.dirty.insert(DirtyKey::Discovery(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Discovery(id));
            }
        }
        Ok(id)
    }

    /// Persist a preserve: each region's possibility state, quantized
    /// (phase-5-plan.md §7.5). Returns the record id, or
    /// [`VaultSequenceError`] before mutation when exhausted.
    pub fn record_preserve(
        &mut self,
        regions: Vec<(RegionCoord, PossibilitySignature)>,
        name: String,
    ) -> Result<u64, VaultSequenceError> {
        let sequence = self.next_sequence()?;
        let record = PreserveRecord::new(regions, sequence, name);
        let id = record.id;
        match self.preserves.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing
                    .get_mut()
                    .merge_from(&record)
                    .expect("same constructor output must have equal immutable body")
                {
                    self.dirty.insert(DirtyKey::Preserve(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Preserve(id));
            }
        }
        Ok(id)
    }

    /// Delete a preserve record. Returns whether it existed. The caller removes
    /// this id's runtime contributions; overlapping regions select their next
    /// lowest-id owner, while regions with no contributor rejoin normal
    /// steering without a snap (ADR 0020).
    #[must_use = "a preserve is removed only after durable backend success"]
    pub fn remove_preserve(&mut self, id: u64) -> Result<bool, VaultPersistenceError> {
        if !self.preserves.contains_key(&id) {
            return Ok(false);
        }
        let key = preserve_key(id);
        if let Err(source) = self.store.remove(&key) {
            return Err(self.record_persistence_failure(PersistenceOperation::Remove, key, source));
        }
        self.clear_persistence_issue(PersistenceOperation::Remove, &key);
        // A durable delete also cancels a pending write of this same logical
        // record. Its failed-store diagnostic is no longer active.
        self.clear_persistence_issue(PersistenceOperation::Store, &key);
        self.preserves.remove(&id);
        self.dirty.remove(&DirtyKey::Preserve(id));
        Ok(true)
    }

    /// Delete a route record. Returns whether it existed. (Mirrors
    /// [`Self::remove_preserve`]: deletion is rare, so the stored key is
    /// removed eagerly rather than resurrecting the record on next open.)
    #[must_use = "a route is removed only after durable backend success"]
    pub fn remove_route(&mut self, id: u64) -> Result<bool, VaultPersistenceError> {
        if !self.routes.contains_key(&id) {
            return Ok(false);
        }
        let key = route_key(id);
        if let Err(source) = self.store.remove(&key) {
            return Err(self.record_persistence_failure(PersistenceOperation::Remove, key, source));
        }
        self.clear_persistence_issue(PersistenceOperation::Remove, &key);
        self.clear_persistence_issue(PersistenceOperation::Store, &key);
        self.routes.remove(&id);
        self.dirty.remove(&DirtyKey::Route(id));
        Ok(true)
    }

    /// Persist a recorded expedition (phase-5-plan.md §7.3). Returns the id,
    /// or [`VaultSequenceError`] before mutation when exhausted.
    pub fn record_route(
        &mut self,
        nodes: Vec<RouteNode>,
        discoveries: Vec<u64>,
        name: String,
    ) -> Result<u64, VaultSequenceError> {
        let sequence = self.next_sequence()?;
        let record = RouteRecord::new(nodes, discoveries, sequence, name);
        let id = record.id;
        match self.routes.entry(id) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                if existing
                    .get_mut()
                    .merge_from(&record)
                    .expect("same constructor output must have equal immutable body")
                {
                    self.dirty.insert(DirtyKey::Route(id));
                }
            }
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(record);
                self.dirty.insert(DirtyKey::Route(id));
            }
        }
        Ok(id)
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
    /// state, capacity-parked entries included. Queued for flushing; overwrites
    /// the previous snapshot. Sequence exhaustion returns
    /// [`VaultSequenceError`] without replacing it.
    pub fn snapshot_session(
        &mut self,
        input: SessionSnapshotInput<'_>,
    ) -> Result<(), VaultSequenceError> {
        self.snapshot_session_owned(SessionSnapshotOwnedInput {
            runtime: input.runtime,
            player: input.player,
            last_player: input.last_player,
            bias: *input.bias,
            transition_mode: input.transition_mode,
            anchors: input
                .anchors
                .iter()
                .map(AnchorSnapshot::from_anchor)
                .collect(),
            regions: input
                .map
                .iter_active()
                .map(|r| RegionSnapshotRecord {
                    coord: r.coord,
                    current: r.current.dims,
                    target: r.target.dims,
                    stability: r.stability,
                    revision: r.revision,
                })
                .collect(),
            recorder: input.recorder,
            tracker: input.tracker,
        })
    }

    /// Snapshot already-captured, action-ordered session values.
    ///
    /// The platform may process this input after the controller tick without
    /// accidentally persisting later actions or continuous movement.
    pub fn snapshot_session_owned(
        &mut self,
        input: SessionSnapshotOwnedInput,
    ) -> Result<(), VaultSequenceError> {
        let sequence = self.next_sequence()?;
        let snap = SessionSnapshot {
            runtime: input.runtime,
            player: input.player,
            last_player: input.last_player,
            bias: input.bias,
            transition_mode: input.transition_mode,
            anchors: input.anchors,
            regions: input.regions,
            recorder: input.recorder,
            tracker: input.tracker,
            sequence,
        };
        self.session = Some(snap);
        self.dirty.insert(DirtyKey::Session);
        Ok(())
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
    /// by content id; records pass canonical body validation and same-id
    /// immutable equality before any mutable fields merge. Rejected records are
    /// reported and never partially applied.
    pub fn import(&mut self, bundle: &AtlasBundle) -> MergeStats {
        use std::collections::btree_map::Entry;
        let mut stats = MergeStats::default();
        let mut valid_result_sequence_max = self.meta.sequence;

        macro_rules! merge_namespace {
            ($records:expr, $index:expr, $dirty:expr, $kind:expr, $validate:expr) => {
                for record in $records {
                    if let Err(error) = $validate(record) {
                        stats.rejected += 1;
                        let reason = import_reason(&error);
                        self.record_issue(
                            IssueIdentity::Import($kind, record.id, reason),
                            format!("import rejected {:?} {:#018x}: {}", $kind, record.id, error),
                        );
                        continue;
                    }
                    let resulting_sequence = match $index.entry(record.id) {
                        Entry::Occupied(mut existing) => {
                            match existing.get_mut().merge_from(record) {
                                Ok(true) => {
                                    self.dirty.insert($dirty(record.id));
                                    stats.merged += 1;
                                }
                                Ok(false) => {
                                    stats.unchanged += 1;
                                }
                                Err(error) => {
                                    stats.rejected += 1;
                                    self.record_issue(
                                        IssueIdentity::Import(
                                            $kind,
                                            record.id,
                                            "immutable-conflict",
                                        ),
                                        format!(
                                            "import rejected {:?} {:#018x}: {}",
                                            $kind, record.id, error
                                        ),
                                    );
                                    continue;
                                }
                            }
                            existing.get().sequence
                        }
                        Entry::Vacant(slot) => {
                            let sequence = record.sequence;
                            slot.insert(record.clone());
                            self.dirty.insert($dirty(record.id));
                            stats.added += 1;
                            sequence
                        }
                    };
                    valid_result_sequence_max = valid_result_sequence_max.max(resulting_sequence);
                }
            };
        }

        merge_namespace!(
            &bundle.discoveries,
            self.discoveries,
            DirtyKey::Discovery,
            RecordKind::Discovery,
            DiscoveryRecord::validate_canonical
        );
        merge_namespace!(
            &bundle.routes,
            self.routes,
            DirtyKey::Route,
            RecordKind::Route,
            RouteRecord::validate_canonical
        );
        merge_namespace!(
            &bundle.preserves,
            self.preserves,
            DirtyKey::Preserve,
            RecordKind::Preserve,
            PreserveRecord::validate_canonical
        );
        if valid_result_sequence_max > self.meta.sequence {
            self.meta.sequence = valid_result_sequence_max;
            self.dirty.insert(DirtyKey::Meta);
        }
        stats
    }

    /// Write dirty records, at most `budget.max_persist_ops` per call, in
    /// deterministic order (records before the meta header). Deferred work is
    /// healthy backpressure; each key crosses the backend's atomic durability
    /// boundary before its dirty entry is retired (ADR 0022).
    #[must_use = "flush failures and remaining backpressure must be handled"]
    pub fn flush(&mut self, budget: &Budget) -> Result<VaultStats, VaultFlushError> {
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
                if let Err(source) = self.store.store(&store_key, &bytes) {
                    stats.dirty = self.dirty.len();
                    let error = self.record_persistence_failure(
                        PersistenceOperation::Store,
                        store_key,
                        source,
                    );
                    return Err(VaultFlushError {
                        progress: stats,
                        error,
                    });
                }
                self.clear_persistence_issue(PersistenceOperation::Store, &store_key);
                stats.flushed += 1;
                stats.bytes += bytes.len();
            }
            self.dirty.remove(&key);
        }
        stats.dirty = self.dirty.len();
        Ok(stats)
    }

    /// Flush everything (tools, shutdown).
    #[must_use = "an explicit drain succeeds only when the vault is clean"]
    pub fn flush_all(&mut self) -> Result<VaultStats, VaultFlushError> {
        let mut total = VaultStats::default();
        loop {
            match self.flush(&Budget::unlimited()) {
                Ok(stats) => {
                    total.flushed += stats.flushed;
                    total.bytes += stats.bytes;
                    total.dirty = stats.dirty;
                    if stats.is_clean() {
                        return Ok(total);
                    }
                }
                Err(error) => {
                    let (progress, error) = error.into_parts();
                    total.flushed += progress.flushed;
                    total.bytes += progress.bytes;
                    total.dirty = progress.dirty;
                    return Err(VaultFlushError {
                        progress: total,
                        error,
                    });
                }
            }
        }
    }
}

/// Restore a session's resident window into a fresh [`RegionMap`]
/// (phase-5-plan.md §12.2). Every region comes back with its bit-exact
/// `current`, stability, and revision as parked authority; follow with settle
/// updates (travel = 0, the session's anchors/bias) so live field admission
/// dirties and re-derives caches, rosters, and organisms deterministically
/// before the journey continues. Restoration is not a fresh load epoch and
/// never resets `current` or revision (ADR 0023).
pub fn apply_session_regions(map: &mut RegionMap, snap: &SessionSnapshot) {
    for region in &snap.regions {
        map.restore_region(region);
    }
}

fn import_reason(error: &RecordCanonicalError) -> &'static str {
    match error {
        RecordCanonicalError::ContentIdMismatch { .. } => "content-id-mismatch",
        RecordCanonicalError::RouteDiscoveryRefs { .. } => "route-discovery-refs",
        RecordCanonicalError::PreserveRegions { .. } => "preserve-regions",
    }
}

fn require_record_key(key: &[u8], prefix: &[u8], id: u64) -> Result<(), String> {
    let expected = match prefix {
        b"disc/" => discovery_key(id),
        b"route/" => route_key(id),
        b"pres/" => preserve_key(id),
        _ => return Ok(()),
    };
    if key == expected {
        Ok(())
    } else {
        Err(format!(
            "storage key {} does not match decoded id {id:#018x}",
            String::from_utf8_lossy(key)
        ))
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
    use std::cell::RefCell;
    use std::collections::{BTreeMap, VecDeque};
    use std::rc::Rc;
    use world_core::{bound_target, domain_mask, AnchorKind, AnchorSource, PossibilityDomain};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum StorageCall {
        Load(Vec<u8>),
        Store(Vec<u8>),
        Remove(Vec<u8>),
        List(Vec<u8>),
    }

    #[derive(Debug)]
    enum ScriptedFailure {
        Store(Vec<u8>),
        Remove { key: Vec<u8>, unlink_first: bool },
    }

    #[derive(Debug, Default)]
    struct ScriptedState {
        entries: BTreeMap<Vec<u8>, Vec<u8>>,
        calls: Vec<StorageCall>,
        failures: VecDeque<ScriptedFailure>,
    }

    /// A deterministic fault backend shared with the test after the vault
    /// takes ownership of its clone.
    #[derive(Debug, Clone, Default)]
    struct ScriptedStorage(Rc<RefCell<ScriptedState>>);

    impl ScriptedStorage {
        fn new() -> Self {
            Self::default()
        }

        fn clear_calls(&self) {
            self.0.borrow_mut().calls.clear();
        }

        fn calls(&self) -> Vec<StorageCall> {
            self.0.borrow().calls.clone()
        }

        fn fail_store(&self, key: Vec<u8>) {
            self.0
                .borrow_mut()
                .failures
                .push_back(ScriptedFailure::Store(key));
        }

        fn fail_remove(&self, key: Vec<u8>, unlink_first: bool) {
            self.0
                .borrow_mut()
                .failures
                .push_back(ScriptedFailure::Remove { key, unlink_first });
        }
    }

    impl Storage for ScriptedStorage {
        fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError> {
            let mut state = self.0.borrow_mut();
            state.calls.push(StorageCall::Load(key.to_vec()));
            state
                .entries
                .get(key)
                .cloned()
                .ok_or(StorageError::NotFound)
        }

        fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
            let mut state = self.0.borrow_mut();
            state.calls.push(StorageCall::Store(key.to_vec()));
            if matches!(state.failures.front(), Some(ScriptedFailure::Store(failed)) if failed == key)
            {
                state.failures.pop_front();
                return Err(StorageError::Backend("scripted store failure".into()));
            }
            state.entries.insert(key.to_vec(), value.to_vec());
            Ok(())
        }

        fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
            let mut state = self.0.borrow_mut();
            state.calls.push(StorageCall::Remove(key.to_vec()));
            if matches!(state.failures.front(), Some(ScriptedFailure::Remove { key: failed, .. }) if failed == key)
            {
                let Some(ScriptedFailure::Remove { unlink_first, .. }) = state.failures.pop_front()
                else {
                    unreachable!()
                };
                if unlink_first {
                    state.entries.remove(key);
                }
                return Err(StorageError::Backend("scripted remove failure".into()));
            }
            state.entries.remove(key);
            Ok(())
        }

        fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError> {
            let mut state = self.0.borrow_mut();
            state.calls.push(StorageCall::List(prefix.to_vec()));
            Ok(state
                .entries
                .range(prefix.to_vec()..)
                .take_while(|(key, _)| key.starts_with(prefix))
                .map(|(key, _)| key.clone())
                .collect())
        }
    }

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
        let disc = vault
            .record_discovery(&sample_anchor(), 0xABCD, "spire".into())
            .unwrap();
        let pres = vault
            .record_preserve(
                vec![(
                    RegionCoord::new(1, 2),
                    PossibilitySignature::of(world_core::PossibilityVector::neutral()),
                )],
                "glade".into(),
            )
            .unwrap();
        vault.mark_seen(RegionCoord::new(1, 2));
        vault.mark_seen(RegionCoord::new(-40, 7));
        let stats = vault.flush_all().unwrap();
        assert!(stats.is_clean());
        assert!(stats.flushed >= 5); // 2 records + 2 seen chunks + meta

        let reopened = Vault::open(vault.store().clone()).unwrap();
        assert_eq!(reopened.issue_count(), 0);
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
        let a = vault
            .record_discovery(&sample_anchor(), 0xABCD, "first".into())
            .unwrap();
        let b = vault
            .record_discovery(&sample_anchor(), 0xABCD, "second".into())
            .unwrap();
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
            vault.record_discovery(&anchor, 0, format!("d{i}")).unwrap();
        }
        let budget = Budget {
            max_persist_ops: 3,
            ..Budget::unlimited()
        };
        let first = vault.flush(&budget).unwrap();
        assert_eq!(first.flushed, 3);
        assert!(first.dirty > 0);
        let mut guard = 0;
        while vault.dirty_records() > 0 {
            vault.flush(&budget).unwrap();
            guard += 1;
            assert!(guard < 100, "flush must drain");
        }
        let reopened = Vault::open(vault.store().clone()).unwrap();
        assert_eq!(reopened.discoveries().len(), 10);
    }

    #[test]
    fn corrupt_records_are_skipped_with_a_report_never_a_panic() {
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        let id = vault
            .record_discovery(&sample_anchor(), 0xABCD, "ok".into())
            .unwrap();
        vault.flush_all().unwrap();
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
        assert_eq!(reopened.issue_count(), 2);
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
        a.record_discovery(&anchor_at(0.0), 1, "shared".into())
            .unwrap();
        a.record_discovery(&anchor_at(100.0), 1, "only-a".into())
            .unwrap();
        let mut b = Vault::open(MemoryStorage::new()).unwrap();
        b.record_discovery(&anchor_at(0.0), 1, "shared-renamed".into())
            .unwrap();
        b.record_discovery(&anchor_at(200.0), 1, "only-b".into())
            .unwrap();
        let mut c = Vault::open(MemoryStorage::new()).unwrap();
        c.record_discovery(&anchor_at(300.0), 1, "only-c".into())
            .unwrap();

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
        a.record_discovery(&anchor_at(0.0), 1, "good".into())
            .unwrap();
        let mut bundle = a.export();
        bundle.discoveries[0].strength_q ^= 1; // immutable field no longer matches id
        let mut b = Vault::open(MemoryStorage::new()).unwrap();
        let stats = b.import(&bundle);
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.added, 0);
        assert!(b.discoveries().is_empty());
        assert_eq!(b.issue_count(), 1);
    }

    #[test]
    fn import_collapses_duplicate_equal_body_records() {
        let mut source = Vault::open(MemoryStorage::new()).unwrap();
        let id = source
            .record_discovery(&anchor_at(0.0), 1, "first".into())
            .unwrap();
        let mut duplicate = source.export().discoveries[0].clone();
        duplicate.sequence += 1;
        duplicate.name = "second".into();
        let bundle = AtlasBundle {
            discoveries: vec![source.export().discoveries[0].clone(), duplicate],
            ..AtlasBundle::default()
        }
        .canonicalized()
        .unwrap();

        let mut target = Vault::open(MemoryStorage::new()).unwrap();
        let stats = target.import(&bundle);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.rejected, 0);
        assert_eq!(target.discoveries()[&id].name, "second");
        let before = target.export();
        let again = target.import(&bundle);
        assert_eq!(again.unchanged, 1);
        assert_eq!(target.export(), before);
    }

    #[test]
    fn open_rejects_noncanonical_preserve_regions() {
        let coord = RegionCoord::new(0, 0);
        let sig = PossibilitySignature::of(world_core::PossibilityVector::neutral());
        let mut record = PreserveRecord {
            id: 0,
            regions: vec![(coord, sig), (coord, sig)],
            sequence: 7,
            name: "legacy-duplicate".into(),
            journal: String::new(),
        };
        record.id = record.content_id();
        let mut store = MemoryStorage::new();
        store
            .store(
                &preserve_key(record.id),
                &encode_record(RecordKind::Preserve, &record),
            )
            .unwrap();

        let vault = Vault::open(store).unwrap();
        assert!(vault.preserves().is_empty());
        assert_eq!(vault.issue_count(), 1);
        assert!(vault
            .issues()
            .next()
            .unwrap()
            .message()
            .contains("appears more than once"));
    }

    #[test]
    fn route_usage_never_double_counts_across_import() {
        let mut a = Vault::open(MemoryStorage::new()).unwrap();
        let node = RouteNode {
            pos_q: (0, 0),
            signature: PossibilitySignature::of(world_core::PossibilityVector::neutral()),
            current_signature: None,
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
            distance_q: 0,
        };
        let id = a.record_route(vec![node], vec![], "trail".into()).unwrap();
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
        vault
            .record_discovery(&sample_anchor(), 0xABCD, "one".into())
            .unwrap();
        // Flush only the record batch's first entries, never re-flushing meta
        // (simulates a crash between record and meta writes): record sequence
        // is 1 but the stored meta still says 0.
        let budget = Budget {
            max_persist_ops: 1,
            ..Budget::unlimited()
        };
        vault.flush(&budget).unwrap(); // writes discovery before meta
        let reopened = Vault::open(vault.store().clone()).unwrap();
        // Healed: the next sequence must exceed the record's.
        let mut reopened = reopened;
        let mut anchor = sample_anchor();
        anchor.world_pos.0 += 1000.0;
        let id = reopened.record_discovery(&anchor, 0, "two".into()).unwrap();
        assert!(reopened.discoveries()[&id].sequence > 1);
    }

    #[test]
    fn flush_failure_keeps_data_before_meta_dirty_and_retry_clears_issue() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let id = vault
            .record_discovery(&sample_anchor(), 7, "faulted".into())
            .unwrap();
        let key = discovery_key(id);
        storage.clear_calls();
        storage.fail_store(key.clone());

        let error = vault.flush_all().unwrap_err();
        assert_eq!(error.progress().flushed, 0);
        assert_eq!(error.progress().dirty, 2);
        assert_eq!(
            error.persistence_error().operation(),
            PersistenceOperation::Store
        );
        assert_eq!(error.persistence_error().key(), key);
        assert_eq!(error.persistence_error().occurrences(), 1);
        assert_eq!(storage.calls(), vec![StorageCall::Store(key.clone())]);
        assert_eq!(vault.issue_count(), 1);

        storage.clear_calls();
        let retry = vault.flush_all().unwrap();
        assert!(retry.is_clean());
        assert_eq!(
            storage.calls(),
            vec![
                StorageCall::Store(key),
                StorageCall::Store(KEY_META.to_vec())
            ]
        );
        assert_eq!(vault.issue_count(), 0, "matching success clears failure");
        let reopened = Vault::open(storage).unwrap();
        assert_eq!(reopened.discoveries().len(), 1);
    }

    #[test]
    fn partial_flush_does_not_rewrite_committed_prefix() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let first = vault
            .record_discovery(&anchor_at(11.0), 1, "one".into())
            .unwrap();
        let second = vault
            .record_discovery(&anchor_at(22.0), 1, "two".into())
            .unwrap();
        let mut keys = [discovery_key(first), discovery_key(second)];
        keys.sort();
        storage.clear_calls();
        storage.fail_store(keys[1].clone());

        let error = vault.flush_all().unwrap_err();
        assert_eq!(error.progress().flushed, 1);
        assert_eq!(error.progress().dirty, 2, "failed record plus meta");
        assert_eq!(
            storage.calls(),
            vec![
                StorageCall::Store(keys[0].clone()),
                StorageCall::Store(keys[1].clone())
            ]
        );

        storage.clear_calls();
        vault.flush_all().unwrap();
        assert_eq!(
            storage.calls(),
            vec![
                StorageCall::Store(keys[1].clone()),
                StorageCall::Store(KEY_META.to_vec())
            ]
        );
    }

    #[test]
    fn metadata_failure_leaves_only_metadata_retryable_and_open_heals() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let id = vault
            .record_discovery(&sample_anchor(), 2, "one".into())
            .unwrap();
        storage.clear_calls();
        storage.fail_store(KEY_META.to_vec());

        let error = vault.flush_all().unwrap_err();
        assert_eq!(error.progress().flushed, 1);
        assert_eq!(error.progress().dirty, 1);
        assert_eq!(error.persistence_error().key(), KEY_META);
        assert_eq!(
            storage.calls(),
            vec![
                StorageCall::Store(discovery_key(id)),
                StorageCall::Store(KEY_META.to_vec())
            ]
        );

        let mut reopened = Vault::open(storage.clone()).unwrap();
        let next = reopened
            .record_discovery(&anchor_at(900.0), 2, "two".into())
            .unwrap();
        assert!(reopened.discoveries()[&next].sequence > 1);

        storage.clear_calls();
        vault.flush_all().unwrap();
        assert_eq!(storage.calls(), vec![StorageCall::Store(KEY_META.to_vec())]);
    }

    #[test]
    fn budget_backpressure_is_success_but_flush_all_failure_is_not() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let id = vault
            .record_discovery(&sample_anchor(), 3, "budget".into())
            .unwrap();
        let zero = Budget {
            max_persist_ops: 0,
            ..Budget::unlimited()
        };
        let stats = vault.flush(&zero).unwrap();
        assert_eq!(stats.flushed, 0);
        assert!(!stats.is_clean());

        let one = Budget {
            max_persist_ops: 1,
            ..Budget::unlimited()
        };
        let stats = vault.flush(&one).unwrap();
        assert_eq!(stats.flushed, 1);
        assert!(!stats.is_clean());
        storage.fail_store(KEY_META.to_vec());
        assert!(vault.flush_all().is_err());
        assert!(storage.0.borrow().entries.contains_key(&discovery_key(id)));
        assert!(!storage.0.borrow().entries.contains_key(KEY_META));
    }

    #[test]
    fn persistence_issues_deduplicate_recover_and_restart() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let id = vault
            .record_discovery(&sample_anchor(), 4, "repeat".into())
            .unwrap();
        let key = discovery_key(id);
        storage.fail_store(key.clone());
        storage.fail_store(key.clone());
        assert_eq!(
            vault
                .flush_all()
                .unwrap_err()
                .persistence_error()
                .occurrences(),
            1
        );
        assert_eq!(
            vault
                .flush_all()
                .unwrap_err()
                .persistence_error()
                .occurrences(),
            2
        );
        assert_eq!(vault.issue_count(), 1);
        assert_eq!(vault.issues().next().unwrap().occurrences(), 2);

        vault.flush_all().unwrap();
        assert_eq!(vault.issue_count(), 0);
        let next = vault
            .record_discovery(&anchor_at(333.0), 4, "again".into())
            .unwrap();
        storage.fail_store(discovery_key(next));
        assert_eq!(
            vault
                .flush_all()
                .unwrap_err()
                .persistence_error()
                .occurrences(),
            1
        );
    }

    #[test]
    fn issue_registry_is_bounded_and_persistence_displaces_newest_nonfatal() {
        let mut source = Vault::open(MemoryStorage::new()).unwrap();
        source
            .record_discovery(&sample_anchor(), 5, "bad".into())
            .unwrap();
        let template = source.export().discoveries.remove(0);
        let mut bundle = AtlasBundle::default();
        for id in 1..=65 {
            let mut record = template.clone();
            record.id = id;
            bundle.discoveries.push(record);
        }
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let stats = vault.import(&bundle);
        assert_eq!(stats.rejected, 65);
        assert_eq!(vault.issue_count(), MAX_VAULT_ISSUES);
        assert_eq!(vault.suppressed_issue_count(), 1);

        vault.import(&AtlasBundle {
            discoveries: vec![bundle.discoveries[0].clone()],
            ..AtlasBundle::default()
        });
        assert_eq!(vault.issue_count(), MAX_VAULT_ISSUES);
        assert_eq!(vault.suppressed_issue_count(), 1);
        assert_eq!(vault.issues().next().unwrap().occurrences(), 2);

        let id = vault
            .record_discovery(&anchor_at(777.0), 5, "persist".into())
            .unwrap();
        storage.fail_store(discovery_key(id));
        let error = vault.flush_all().unwrap_err();
        assert_eq!(error.persistence_error().occurrences(), 1);
        assert_eq!(vault.issue_count(), MAX_VAULT_ISSUES);
        assert_eq!(vault.suppressed_issue_count(), 2);
        assert!(vault.issues().any(|issue| {
            matches!(
                issue.identity,
                IssueIdentity::Persistence(PersistenceOperation::Store, _)
            )
        }));
        vault.flush_all().unwrap();
        assert_eq!(vault.issue_count(), MAX_VAULT_ISSUES - 1);
    }

    #[test]
    fn failed_delete_is_commit_after_remove_and_unlink_error_needs_retry() {
        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        let regions = vec![(
            RegionCoord::new(3, 4),
            PossibilitySignature::of(world_core::PossibilityVector::neutral()),
        )];
        let id = vault
            .record_preserve(regions.clone(), "old".into())
            .unwrap();
        vault.flush_all().unwrap();
        vault.record_preserve(regions, "new".into()).unwrap();
        let dirty_before = vault.dirty_records();
        let key = preserve_key(id);
        storage.fail_remove(key.clone(), false);

        let error = vault.remove_preserve(id).unwrap_err();
        assert_eq!(error.operation(), PersistenceOperation::Remove);
        assert!(vault.preserves().contains_key(&id));
        assert_eq!(vault.dirty_records(), dirty_before);
        assert!(vault
            .export()
            .preserves
            .iter()
            .any(|record| record.id == id));
        assert!(Vault::open(storage.clone())
            .unwrap()
            .preserves()
            .contains_key(&id));

        assert!(vault.remove_preserve(id).unwrap());
        assert!(!vault.preserves().contains_key(&id));
        assert!(!Vault::open(storage.clone())
            .unwrap()
            .preserves()
            .contains_key(&id));
        storage.clear_calls();
        assert!(!vault.remove_preserve(id).unwrap());
        assert!(storage.calls().is_empty());

        let cancel_id = vault
            .record_preserve(
                vec![(
                    RegionCoord::new(8, 9),
                    PossibilitySignature::of(world_core::PossibilityVector::neutral()),
                )],
                "cancel dirty write".into(),
            )
            .unwrap();
        let cancel_key = preserve_key(cancel_id);
        storage.fail_store(cancel_key.clone());
        let store_error = vault.flush_all().unwrap_err();
        assert_eq!(
            store_error.persistence_error().operation(),
            PersistenceOperation::Store
        );
        assert_eq!(store_error.persistence_error().key(), cancel_key);
        assert!(vault.active_persistence_issue().is_some());
        assert!(vault.remove_preserve(cancel_id).unwrap());
        assert!(
            vault.active_persistence_issue().is_none(),
            "durable delete cancels the same-key failed store"
        );

        let node = RouteNode {
            pos_q: (1, 2),
            signature: PossibilitySignature::of(world_core::PossibilityVector::neutral()),
            current_signature: None,
            cost_q: 1,
            stability_q: 2,
            anchor_sig: 3,
            distance_q: 0,
        };
        let canceled_route = vault
            .record_route(vec![node], vec![], "canceled route".into())
            .unwrap();
        let canceled_route_key = route_key(canceled_route);
        storage.fail_store(canceled_route_key);
        assert!(vault.flush_all().is_err());
        assert!(vault.remove_route(canceled_route).unwrap());
        assert!(vault.active_persistence_issue().is_none());

        let route = vault
            .record_route(vec![node], vec![], "route".into())
            .unwrap();
        vault.flush_all().unwrap();
        let route_storage_key = route_key(route);
        storage.fail_remove(route_storage_key.clone(), true);
        assert!(vault.remove_route(route).is_err());
        assert!(
            vault.routes().contains_key(&route),
            "logical record remains"
        );
        assert!(!storage.0.borrow().entries.contains_key(&route_storage_key));
        assert!(vault.remove_route(route).unwrap());
        assert!(!vault.routes().contains_key(&route));
    }

    #[test]
    fn import_advances_sequence_for_added_merged_and_unchanged_valid_results() {
        let mut source = Vault::open(MemoryStorage::new()).unwrap();
        source
            .record_discovery(&sample_anchor(), 6, "remote".into())
            .unwrap();
        let mut bundle = source.export();
        bundle.discoveries[0].sequence = 100;

        let storage = ScriptedStorage::new();
        let mut vault = Vault::open(storage.clone()).unwrap();
        assert_eq!(vault.import(&bundle).added, 1);
        let next = vault
            .record_discovery(&anchor_at(1000.0), 6, "local".into())
            .unwrap();
        assert_eq!(vault.discoveries()[&next].sequence, 101);
        storage.clear_calls();
        vault.flush_all().unwrap();
        let calls = storage.calls();
        assert!(matches!(calls.last(), Some(StorageCall::Store(key)) if key == KEY_META));
        let mut reopened = Vault::open(storage).unwrap();
        let after_reopen = reopened
            .record_discovery(&anchor_at(2000.0), 6, "later".into())
            .unwrap();
        assert!(reopened.discoveries()[&after_reopen].sequence > 101);

        let mut stale = Vault::open(MemoryStorage::new()).unwrap();
        stale.import(&bundle);
        stale.flush_all().unwrap();
        stale.meta.sequence = 0;
        stale.dirty.remove(&DirtyKey::Meta);
        let same = stale.export();
        assert_eq!(stale.import(&same).unchanged, 1);
        assert_eq!(stale.meta.sequence, 100);
        assert!(stale.dirty.contains(&DirtyKey::Meta));

        let mut mixed = bundle.clone();
        let mut rejected = mixed.discoveries[0].clone();
        rejected.strength_q ^= 1;
        rejected.sequence = u64::MAX;
        mixed.discoveries[0].sequence = 50;
        mixed.discoveries.push(rejected);
        let mut target = Vault::open(MemoryStorage::new()).unwrap();
        let stats = target.import(&mixed);
        assert_eq!((stats.added, stats.rejected), (1, 1));
        let local = target
            .record_discovery(&anchor_at(3000.0), 6, "valid max".into())
            .unwrap();
        assert_eq!(target.discoveries()[&local].sequence, 51);

        let mut exhausted = bundle.clone();
        exhausted.discoveries[0].sequence = u64::MAX;
        let mut exhausted_target = Vault::open(MemoryStorage::new()).unwrap();
        let stats = exhausted_target.import(&exhausted);
        assert_eq!((stats.added, stats.rejected), (1, 0));
        let exhausted_export = exhausted_target.export();
        let dirty_before = exhausted_target.dirty_records();
        assert_eq!(
            exhausted_target
                .record_discovery(&anchor_at(4000.0), 6, "exhausted".into())
                .unwrap_err()
                .last_sequence(),
            u64::MAX
        );
        assert_eq!(exhausted_target.export(), exhausted_export);
        assert_eq!(exhausted_target.dirty_records(), dirty_before);
        assert!(exhausted_target
            .record_preserve(Vec::new(), "exhausted preserve".into())
            .is_err());
        assert!(exhausted_target
            .record_route(Vec::new(), Vec::new(), "exhausted route".into())
            .is_err());
        assert!(exhausted_target.preserves().is_empty());
        assert!(exhausted_target.routes().is_empty());
        assert_eq!(exhausted_target.dirty_records(), dirty_before);
        let map = RegionMap::new(crate::stream::StreamConfig::default());
        assert!(exhausted_target
            .snapshot_session(SessionSnapshotInput {
                map: &map,
                player: (0.0, 0.0),
                last_player: (0.0, 0.0),
                bias: &[0.0; POSSIBILITY_DIMS],
                transition_mode: false,
                anchors: &[],
                runtime: session_runtime_record(
                    map.config(),
                    &Budget::unlimited(),
                    None,
                    false,
                    false,
                ),
                recorder: None,
                tracker: world_core::RouteTrackerSnapshot::default(),
            })
            .is_err());
        assert!(exhausted_target.session().is_none());
        let mut max_replica = Vault::open(MemoryStorage::new()).unwrap();
        assert_eq!(max_replica.import(&exhausted_export).added, 1);
        assert_eq!(max_replica.export(), exhausted_export);
        exhausted_target.flush_all().unwrap();
        let mut exhausted_reopened = Vault::open(exhausted_target.store().clone()).unwrap();
        assert_eq!(exhausted_reopened.export(), exhausted_export);
        assert!(exhausted_reopened
            .record_discovery(&anchor_at(4500.0), 6, "still exhausted".into())
            .is_err());

        let mut near_exhausted = bundle;
        near_exhausted.discoveries[0].sequence = u64::MAX - 1;
        let mut near_target = Vault::open(MemoryStorage::new()).unwrap();
        assert_eq!(near_target.import(&near_exhausted).added, 1);
        let last = near_target
            .record_discovery(&anchor_at(5000.0), 6, "last allocatable successor".into())
            .unwrap();
        assert_eq!(near_target.discoveries()[&last].sequence, u64::MAX);
        let final_export = near_target.export();
        let mut final_replica = Vault::open(MemoryStorage::new()).unwrap();
        assert_eq!(final_replica.import(&final_export).added, 2);
        assert_eq!(final_replica.export(), final_export);
        assert!(near_target
            .record_discovery(&anchor_at(6000.0), 6, "one too many".into())
            .is_err());
    }
}
