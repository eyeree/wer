# Improvement A.3 — Explicit Persistence Failures and Power-Loss Durability

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.3](../../world-model.md#prioritized-improvement-roadmap)

**Finding addressed:** [28](../../world-model.md#28-resolved-persistence-failures-and-sequence-handling-are-explicit-and-durable)

This plan implements the third item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). It makes every persistence mutation
report failure, prevents callers from treating a partially drained dirty queue
as a completed save, makes imported sequence values advance the local
monotonic counter immediately, bounds repeated failure diagnostics, and makes
the native temp-file protocol durable across process and power loss by
synchronizing both file content and the containing directory.

The work is complete only when the API changes, ordering invariants, native
durability implementation, deterministic failure-injection regressions,
current documentation, and full native/wasm validation land together. The
roadmap item and finding remain open until every gate has passed.

This is a post-prototype corrective plan. It must not modify
[`implementation-plan.md`](implementation-plan.md) or any
`docs/plans/prototype/phase-N-plan.md` file; those files remain the historical
record of the original prototype plan.

---

## 1. Outcome and completion criteria

After this work, an explicit save/drain succeeds only when every queued
mutation has reached the storage backend and no dirty record remains; an
individual store/delete succeeds only after that key crosses the backend's
durability boundary. A successful budgeted flush may still report ordinary
backpressure explicitly. Backend failures are structured values available to
every caller, remain retryable, and do not silently discard in-memory data or
repeat an unbounded issue string each frame. Native writes and deletes use a
documented durability boundary; neutral and wasm-facing crates retain a
synchronous abstract `Storage` contract without importing filesystem APIs.

Completion requires all of the following:

1. `Vault` delete and flush operations return structured results which expose
   backend failures without losing the affected in-memory record or dirty key.
2. A flush reports completion only when the dirty queue is empty. Exhausting
   `Budget::max_persist_ops` is ordinary backpressure, distinct from failure;
   a user-requested save drains in bounded calls until clean or stops on the
   first error.
3. Dirty data is written before sequence metadata, and metadata is not marked
   clean until its write succeeds. A failed record write remains dirty; a
   failed metadata write leaves only metadata dirty and is retryable.
4. Import preserves the existing per-record partial-acceptance contract. For
   every valid added, merged, or unchanged record, it includes the resulting
   local record's sequence in an import maximum, raises the local counter to
   that maximum, and dirties metadata in the same operation. Rejected records
   do not contribute. Every valid sequence through `u64::MAX` remains accepted
   and shareable. The next local edit is strictly newer when representable; at
   `u64::MAX`, every sequence-allocating mutation returns a typed exhaustion
   error without changing records, session state, metadata, or dirtiness.
5. Repeated retries of the same pending failure occupy bounded diagnostic
   state and do not grow the issue list. Recovery clears or supersedes the
   active issue deterministically.
6. `FileStorage::store` uses same-directory temp creation, writes
   complete bytes, synchronizes the temp file, atomically renames it, and
   synchronizes the parent directory before returning success. Failed temp
   operations are reported and cleanup is best-effort without masking the
   primary error.
7. `FileStorage::remove` removes an existing file and synchronizes its parent
   directory before returning success. A retry that finds the file absent
   still synchronizes the parent before success, closing the unlink-succeeded /
   directory-sync-failed window. Missing ancestors synchronize the nearest
   existing ancestor. A failed remove or sync is reported and logical deletion
   remains retryable.
8. Failure injection covers each storage operation and each dirty/metadata
   ordering boundary, including delete resurrection prevention, partial
   flushes, metadata retry, import sequence advancement, bounded issue state,
   and native file/parent-directory synchronization order.
9. All native UI, session-save, atlas import, inspector/harness, and test call
   sites explicitly handle the new results; none logs or prints a successful
   save/import/delete while relevant dirty state or an error remains.
10. No record bytes, merge law, content identity, generation formula, or
    permanent world identity changes. `RECORD_FORMAT_VERSION`,
    `WORLD_ALGORITHM_VERSION`, and every layer `algorithm_revision` remain
    unchanged; no golden fixture is re-blessed.
11. The native CI-equivalent suite and neutral/web wasm check pass, and
    `world-model.md` marks A.3 completed and finding 28 resolved without
    claiming A.10 findings 24/25 (content equality, canonical sets, and CRDT
    deletion/counters) or B.2 async/lazy browser storage is complete.

## 2. Scope boundaries

### 2.1 In scope

- Structured `Vault` mutation/flush outcomes and retry-preserving dirty-state
  sequencing.
- Delete error propagation and native preserve-removal integration.
- Immediate local sequence advancement plus metadata dirtiness on import.
- Bounded, deduplicated persistence issue reporting.
- Native `FileStorage` file and directory synchronization for `store`,
  `remove`, and first-use directory creation.
- Failure-injection storage doubles and focused unit/integration/harness tests.
- Current-state, validation, roadmap, and finding-28 edits in
  `world-model.md`, plus new ADR 0022 superseding the persistence-success part
  of ADR 0014.

### 2.2 Explicitly out of scope

- Collision-resistant content ids, full-record equality on equal ids,
  tombstones, or cross-store deletion convergence. Those belong to A.10 and
  findings 24 and 25 (especially finding 24's deletion caveat); local
  failed-delete retry must not invent a CRDT deletion protocol.
- Async `Storage`, IndexedDB, paged namespace scans, lazy vault indexes, or
  browser persistence. Those belong to B.2/finding 27; this work keeps the
  neutral synchronous trait and only makes its current result contract honest.
- Changing atlas merge semantics, mutable-field ranking, sequence meaning,
  record encoding, bundle encoding, or content-derived ids.
- Broad transactionality across multiple keys. Flush remains a deterministic,
  budgeted sequence of per-key durable operations with retryable dirty state.
- Editing accepted ADRs in place, any original prototype/phase plan, record
  goldens, or generation goldens.

## 3. Current failure map and caller inventory

| Path | Current failure | Required correction |
|---|---|---|
| `Vault::remove_preserve` | Removes memory first and ignores backend removal errors. | Do not finalize logical removal until durable removal succeeds; return an error and preserve retryable state. |
| `Vault::remove_route` | Mirrors the same ignored eager-remove pattern. | Use the same fallible commit-after-remove contract; partial native route clearing must be reported honestly. |
| `Vault::flush` | Converts failures to repeated issue strings and returns stats, allowing dirty records to be mistaken for success. | Return a structured outcome/error, retain failed dirty keys, and bound issue reporting. |
| `Vault::flush_all` | Stops on zero progress and returns ordinary stats even when a storage failure left dirtiness. | Return `Err` on the first failure and reserve `Ok` for a fully clean vault. |
| native save/reporting | Can announce a save after one budgeted flush even when dirtiness remains. | Use an explicit drain-until-clean save helper and only emit success for a clean vault. |
| native periodic flush | Discards the reason for failure after copying counters. | Preserve progress telemetry, surface the first active error, and let later frames retry without repeated log spam. |
| native preserve/route delete | Mutates `RegionMap`/route tracking and prints success despite ignored backend errors. | Change runtime state only after durable vault deletion; stop and report partial route clearing on failure. |
| `Vault::import` | Merges high-sequence records without advancing/dirtying `meta/store`. | Raise sequence and enqueue metadata atomically with accepted merge state. |
| `wer-atlas import` | Prints import success after `flush_all` even if dirty writes failed. | Propagate flush failure to a nonzero exit and print success only after clean completion. |
| `FileStorage::store` | Temp-write plus rename lacks file and directory synchronization; first namespace creation is not durably linked from the root. | Durably create missing ancestors, sync the completed temp file before rename, then sync the containing directory. |
| `FileStorage::remove` | Deletion is not followed by directory synchronization; a retry sees `NotFound` and could falsely commit after a failed sync. | Sync the directory after both successful unlink and not-found retry, and propagate failure. |
| open/import/flush issue log | An unrestricted `Vec<String>` grows for repeated failures and untrusted rejected records. | Deduplicate under stable keys, retain at most 64 entries, count suppressed reports, and clear recovered active persistence failures. |

The complete call-site migration is: `world-runtime/src/vault.rs` and its unit
tests; the re-exports in `world-runtime/src/lib.rs`; `tools/src/filestore.rs`;
`tools/src/bin/atlas.rs`; issue-copying in `tools/src/atlas.rs`; the `wer-vault`
harness in `tools/src/vault.rs`; `tools/tests/{atlas,persistence,preserve,route}.rs`;
and native periodic flush, `O` save, `P` preserve removal, `Delete` route
removal, HUD telemetry, and their focused tests in
`platform-native/src/{main,panel}.rs`. There is no native live atlas-import
path outside `wer-atlas`. `wer-atlas export` writes a standalone exchange file,
not a vault key and makes no save/durability claim; its raw output protocol is
not broadened in A.3.

## 4. Required design

### 4.1 Result and error APIs

Add and re-export these public, `Debug` types from `world-runtime` (field
visibility may use read-only accessors, but their information is mandatory):

```rust
pub enum PersistenceOperation { Store, Remove }

pub struct VaultPersistenceError {
    operation: PersistenceOperation,
    key: Vec<u8>,
    source: StorageError,
    occurrences: u64,
}

pub struct VaultFlushError {
    progress: VaultStats,
    error: VaultPersistenceError,
}

pub struct VaultSequenceError {
    last_sequence: u64,
}
```

`VaultPersistenceError` implements `Display` and `Error`, exposing the
operation, byte key (lossily rendered only for display), underlying
`StorageError`, and the number of consecutive reports for that active
operation/key. `VaultFlushError` implements `Display` and `Error` and exposes
both progress made before failure and the persistence error. Callers match the
operation/key through typed accessors, never by parsing strings.

Keep the existing telemetry fields on `VaultStats` and add
`VaultStats::is_clean() -> bool` (`dirty == 0`). Use these exact signatures:

```rust
pub fn flush(&mut self, budget: &Budget)
    -> Result<VaultStats, VaultFlushError>;
pub fn flush_all(&mut self)
    -> Result<VaultStats, VaultFlushError>;
pub fn remove_preserve(&mut self, id: u64)
    -> Result<bool, VaultPersistenceError>;
pub fn remove_route(&mut self, id: u64)
    -> Result<bool, VaultPersistenceError>;
pub fn record_discovery(...)
    -> Result<u64, VaultSequenceError>;
pub fn record_preserve(...)
    -> Result<u64, VaultSequenceError>;
pub fn record_route(...)
    -> Result<u64, VaultSequenceError>;
pub fn snapshot_session(...)
    -> Result<(), VaultSequenceError>;
```

`flush` returns `Ok(stats)` after zero through `max_persist_ops` successful
writes, even when budget exhaustion leaves `stats.is_clean() == false`;
backpressure is not failure. It returns `Err` immediately on a backend error,
with progress and the current dirty count. `flush_all` aggregates progress and
returns `Ok` only when clean; it returns the first error with aggregate
progress. Mark these results/types `#[must_use]` where Rust permits so a caller
cannot silently discard a failed save/delete.

`VaultError` remains the open-time error (`Storage` or corrupt/future meta).
Do not fold runtime write/delete failures into it: their operation/key/progress
context is distinct. `Storage` keeps the method names `load`, `store`,
`remove`, and `keys_with_prefix`; this work strengthens success semantics but
does not rename or async-ify the trait.

`VaultSequenceError` implements `Display` and `Error` and reports the exhausted
counter. Sequence allocation uses checked addition before any mutation. Thus a
store at `u64::MAX` accepts, exports, and merges its valid records unchanged,
while discovery, preserve, route, and session authoring all fail explicitly
without partial in-memory state or integer wrap.

### 4.2 Dirty data and metadata sequencing

Retain deterministic dirty-key order with data namespaces before
`meta/store`. Remove a dirty key only after its `store` has returned
success. Stop a flush at the first backend failure so later metadata cannot
overtake failed data. If data succeeds and metadata fails, only metadata
remains dirty. Retry resumes from the first pending key.

Deletion uses **commit after durable remove**, not a pending tombstone:

1. If the id is absent from the in-memory namespace, return `Ok(false)` and do
   not touch storage.
2. If present, call `Storage::remove` while the record and dirty key remain
   intact.
3. On `Err`, record/update the active persistence issue and return it. The
   record remains visible to reads/exports and its runtime preserve
   contributions or route tracking remain active; any prior dirty write also
   remains retryable.
4. On `Ok`, clear the matching active issue, then remove the record and its
   `DirtyKey`, and return `Ok(true)`. Native callers mutate `RegionMap` or route
   tracking only after this result.

This policy makes failure a rejected user action rather than a half-applied
logical delete. It needs no in-memory delete queue and adds no persisted or
distributed tombstone. If unlink succeeds but directory synchronization fails,
`FileStorage::remove` returns `Err`; the next call sees `NotFound` but still
synchronizes the parent before returning `Ok`, at which point the vault may
commit the logical removal.

### 4.3 Import sequence advancement

Do not make import bundle-transactional. Preserve its current namespace order
and per-record behavior: reject an invalid record and continue accepting valid
siblings. During each valid entry operation, after `merge_from` or insertion,
observe the **resulting local record's** sequence. This applies to `added`,
`merged`, and `unchanged` records: an unchanged valid record may reveal that
the counter lags a sequence already represented locally. Invalid/id-mismatched
records do not contribute.

After all three shareable namespaces, set
`meta.sequence = max(meta.sequence, valid_result_sequence_max)`. If it
increases, insert `DirtyKey::Meta` in the same `import` call. Existing record
dirty keys still sort before metadata. `MergeStats` meanings and counts remain
unchanged, and a repeat import with an already-current counter does not create
new dirtiness. The next `next_sequence` call therefore returns a value strictly
greater than every valid resulting imported record when one is representable.
Every valid sequence through `u64::MAX` remains in the merge domain. At
exhaustion, checked allocation returns `VaultSequenceError` before changing
metadata or any authored/session state. Do not scan or trust rejected records,
change mutable ranking, or include bundle order in the result.

### 4.4 Bounded issue reporting

Replace `Vec<String>` with retained `VaultIssue` entries keyed internally by
stable issue identity, not full rendered text. Expose `VaultIssue` (message and
saturating `occurrences`), `issues()` as an iterator, `issue_count()`, and
`suppressed_issue_count()`. Use `MAX_VAULT_ISSUES = 64` as a hard retained-entry
cap with these deterministic rules:

- open/decode problems key by store/session/record key; import rejection keys
  by record kind plus content id; a repeated identity increments its
  occurrence count and updates its message in place;
- a new non-persistence identity appends while below 64; at the cap it does
  not allocate another entry and increments one saturating suppressed counter;
- at most one persistence issue is active. A repeat of the same
  `(PersistenceOperation, key)` increments it. A different persistence failure
  supersedes the old active entry. If all 64 slots are occupied by nonfatal
  entries, deterministically replace the newest retained nonfatal entry with
  the active persistence issue and count that displaced report as suppressed;
- a successful `store`/`remove` clears the active issue only when its operation
  and key match. A later failure after recovery starts again at occurrence one.

The structured error is returned regardless of whether a diagnostic entry was
retained, so the cap never hides failure from the initiating caller. Native
periodic flush logs only occurrence one of an active error, relies on HUD issue
telemetry for repeats, and logs recovery once when the matching retry succeeds.
CLI save/import/delete paths print the returned error and exit or abort that
action. Tools render `VaultIssue` with ` (repeated N times)` when `N > 1` and
report the suppressed count once.

### 4.5 Native durability protocol

For `FileStorage::store(key, value)`:

1. Resolve the validated key. Durably create every missing directory from the
   nearest existing ancestor through the store root/namespace in top-down
   order: create one directory, then synchronize its parent before creating
   the next. `FileStorage::open` uses the same helper for a new store root.
2. Create a dot-prefixed, same-directory temp sibling with `create_new`, using
   process id plus a process-local monotonic suffix and retrying collisions.
   Dot-prefix filtering keeps abandoned temps out of namespace scans.
3. Write the complete value with `write_all`, call `File::sync_all` on the temp
   file, and close it.
4. Atomically rename the temp sibling over the destination. Never emulate
   replacement by deleting the destination first; on a platform where atomic
   replacement is unavailable, return a contextual backend error.
5. Open and synchronize the destination's containing directory before
   returning `Ok(())`. This commits the rename/directory entry. A failure at
   or after rename returns `Err`, so the dirty key is retried even if the new
   whole value is already visible.

On any pre-rename error, best-effort remove the known temp path without masking
the primary error. A leftover dot temp remains invisible and harmless. Each
backend error identifies the failed stage and path.

For `remove(key)`, treat not-found as success. After removing an existing file,
open and synchronize the containing directory before returning success. If
`remove_file` returns `NotFound`, synchronize that same parent anyway; if the
parent does not exist, synchronize the nearest existing ancestor down to the
store root which proves the absent namespace. This is required for a retry
after an unlink that succeeded before directory sync failed. Return `Ok` only
after that barrier.

Factor the protocol over a private native file-operations seam so tests can
log and fail `create_dir`, parent sync, temp create, write, file sync, rename,
remove, and post-operation directory sync without relying on real disk faults.
The production implementation uses `sync_all`: ordinary directory `File`
handles on Unix, a directory handle opened with backup-semantics flags on
Windows, and an explicit `StorageError::Backend` on any other native target
where a durable directory barrier is unsupported. It must never silently
downgrade durability. The existing real-filesystem contract tests remain.

These are per-key durability guarantees, not a multi-key transaction: a crash
may expose any successfully committed data prefix, metadata never precedes
that prefix, and open-time max-sequence healing remains defense in depth.

### 4.6 Platform and wasm boundary

All result/error/issue types and the ordering logic stay in `world-runtime` and
use only portable Rust data. `MemoryStorage` remains an immediate, infallible
reference implementation. Filesystem operations, temp naming, directory
handles, and platform `cfg`s remain exclusively in native `tools::FileStorage`,
preserving ADR 0002 and the wasm check.

This work deliberately leaves `Storage` synchronous. It neither supplies a
`platform-web` backend nor claims that a directory-sync concept maps to
IndexedDB; a future B.2/finding-27 async trait must preserve the new meaning
that successful completion reached that backend's durability boundary. No
`platform-web` source change is part of this item.

## 5. Implementation milestones

Execute in this order so each intermediate state is testable and callers never
temporarily acquire false-success behavior:

1. In `world-runtime/src/vault.rs`, add the scripted `Storage` double and
   failing tests for record/meta ordering, aggregate progress, deduplicated
   issues, commit-after-remove, and import sequence advancement.
2. Add `PersistenceOperation`, `VaultPersistenceError`, `VaultFlushError`,
   `VaultIssue`, the 64-entry registry, `VaultStats::is_clean`, and the exact
   fallible signatures. Re-export public types from `world-runtime/src/lib.rs`
   and strengthen `Storage` documentation in `storage.rs` so `Ok` means the
   implementation's durability boundary has completed.
3. Implement record-before-meta flush/error sequencing and commit-after-remove
   for both preserves and routes. Make all existing `MemoryStorage` tests and
   neutral harness callers unwrap/inspect results explicitly.
4. Extend per-record import to observe resulting valid sequences and dirty
   metadata on advancement. Keep its partial rejection and `MergeStats` laws,
   then add mixed-valid/invalid and unchanged-valid regressions.
5. Refactor `tools/src/filestore.rs` behind its private `FileOps` seam;
   implement durable ancestor creation, collision-safe temp store, file sync,
   atomic rename, directory sync, not-found delete barrier, and staged fault
   tests. Keep the real filesystem smoke tests for overwrite/list/remove.
6. Migrate `wer-atlas import`, vault inspection issue rendering, `wer-vault`,
   and all `tools` integration tests. `wer-atlas import` exits nonzero on flush
   failure and prints its success line only for `flush_all` `Ok(clean)`.
7. Migrate native frame flushing, `O` save, `P` delete, `Delete` route clear,
   HUD issue/suppression telemetry, and the focused effective-preserve test.
   Preserve deletion changes `RegionMap` only after `Ok(true)`. Route clearing
   processes ascending ids, stops at the first error, retains failed/unvisited
   routes in both the vault and tracker, and reports `removed/total` rather
   than claiming a full clear.
8. Add accepted ADR 0022 and its index row. Run focused tests and the full
   validation matrix. Only after all pass, change this plan's status to
   `Completed` and make every `world-model.md` completion edit in section 8.

## 6. Required tests

### 6.1 Neutral vault fault matrix

Add a deterministic `ScriptedStorage` in `vault.rs` tests that records
`Load/Store/Remove/List` operations, can fail before an operation, and can
simulate remove having unlinked before returning an error. Assert:

1. Failure on the first discovery `store` returns `VaultFlushError` with zero
   progress, leaves discovery plus meta dirty, does not attempt meta, and keeps
   the backing store unchanged. A successful retry writes discovery before
   meta, clears the active issue, and reaches clean.
2. With two dirty records, failure on the second reports one flushed record,
   retains only the failed record plus meta, and retry does not rewrite the
   already-clean first key.
3. Failure on `meta/store` after all data succeeds leaves only meta dirty;
   retry writes only meta. Reopen between data and meta heals the counter and
   sees only whole records.
4. A zero/small budget returns `Ok` with `is_clean() == false`; repeated
   budgeted calls make deterministic progress. `flush_all` returns `Err` on
   storage failure and `Ok` only at dirty zero.
5. Repeating one `(Store, key)` failure retains one issue and saturating-counts
   occurrences. Recovery removes it and the next failure starts at one. More
   than 64 distinct corrupt/import issues retain exactly 64 entries and a
   deterministic suppressed count; repeating an existing identity consumes no
   slot. A new active persistence failure displaces the specified newest
   nonfatal entry rather than exceeding the cap.
6. Failed preserve and route removal returns `Err`, keeps the record and its
   preexisting dirty state visible/exportable, and reopening the pre-failure
   store still sees it. Successful retry returns `Ok(true)`, clears the dirty
   key, and reopening sees no record. `Ok(false)` performs no backend call.
   The unlink-succeeded/error simulation still requires a second remove call
   before the vault drops its in-memory record.
7. Importing a valid added or merged high-sequence record makes the next local
   record's sequence strictly higher in the same process, dirties meta after
   data, and preserves the counter after flush/reopen. A valid unchanged record
   also heals deliberately stale local metadata. A mixed bundle accepts valid
   siblings, rejects invalid ones, and only valid resulting records contribute
   to the maximum; a rejected tampered record with `u64::MAX` sequence cannot
   move it. An otherwise content-valid record at `u64::MAX` is accepted,
   exportable, and reimportable with the merge laws intact; subsequent
   discovery, preserve, route, and snapshot attempts return typed exhaustion
   without partial mutation or wrap.
8. Existing merge commutativity/associativity/idempotence, route-usage max, and
   stale-meta-on-open tests retain their prior results and bytes.

### 6.2 Native filesystem protocol matrix

The private fake `FileOps` must assert exact production order:

- first store into a new `disc/` namespace: create namespace, sync root, create
  temp, write all, sync temp file, rename, sync `disc/`;
- store into an existing namespace skips creation but still performs temp file
  sync, rename, and directory sync;
- each injected failure stops before later steps, returns the original staged
  error, performs only best-effort temp cleanup, and never reports success;
- rename-success/directory-sync-failure returns `Err`; retry performs the full
  store protocol and succeeds only after directory sync;
- remove success is `remove_file -> sync parent`; unlink-success/sync-failure
  returns `Err`, and its `NotFound` retry still syncs the same parent before
  `Ok`; a never-created namespace syncs the nearest existing ancestor;
- nested new store roots/directories sync each newly created entry's parent in
  top-down order; and
- unsupported-directory-sync platform branches return an explicit backend
  error rather than an optimistic `Ok`.

Real-filesystem tests continue covering load/store overwrite, sorted prefix
listing, idempotent missing remove, hostile-key rejection, abandoned-dot-temp
invisibility, and reopen after successful durable store/delete. Tests may
prove call order through the fake seam; they must not pretend to simulate an
actual power cut.

### 6.3 Caller and sign-off regressions

- Extend `wer-vault` with a persistence-failure/sequence report that fails the
  harness if a dirty failure is reported as clean, metadata overtakes data, a
  delete disappears after failure, issue state grows, or a post-import edit is
  not newer.
- Update `tools/tests/persistence.rs` crash cuts and all other flush callers to
  inspect/unwrap results. Keep save→load→settle exactness and atlas/preserve/
  route integration behavior unchanged.
- Extend the native effective-preserve unit test with a failing remove: the
  vault record and `RegionMap` winner/successor remain unchanged on `Err`, then
  both change together on successful retry. Extract route deletion into a
  small testable helper and test that partial failure retains the failed and
  unvisited ids in both the vault and tracker.
- Factor `wer-atlas import` persistence/reporting into a result-returning
  helper and test that a failed flush returns an error before the caller can
  emit the success summary; keep `main` as the thin print/exit-code layer.

## 7. Validation

Run from the repository root, with the pinned stable toolchain:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTFLAGS="-D warnings" cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
cargo run --bin wer-vault
```

Focused tests may be run during development, but do not replace the full
matrix. No GUI launch is required: the extracted native helpers provide the
save/delete result-path coverage without opening a window.

## 8. Required `world-model.md` completion edits

Only after implementation and every validation gate pass:

1. In section 3.22, replace the paragraph beginning `Mutations update...` with
   the implemented contract: budgeted `flush` returns structured progress or
   an operation/key error; `flush_all` succeeds only when clean; dirty data is
   retired only after successful per-key commit and precedes meta; preserve and
   route deletion commits in memory only after durable backend remove; valid
   import results raise the local sequence and dirty meta immediately; retained issue
   telemetry is deduplicated/capped at 64 plus a suppressed counter. Replace
   `temporary sibling file followed by rename` with the precise durable native
   protocol: durable ancestor creation, temp `sync_all`, atomic rename, and
   containing-directory `sync_all`, with remove/not-found using the same
   directory barrier. State clearly that this remains synchronous and the web
   backend is future work.
2. In section 3.28, add focused neutral failure-injection coverage for
   data/meta order, progress/error outcomes, retry-safe delete, issue bounds,
   and immediate import sequence advancement; native protocol-order/failure
   tests for file/directory synchronization; and the expanded `wer-vault`
   scenario.
3. Rewrite roadmap A.3 exactly in the established completed-item style:
   `**Completed: Make persistence failures explicit and durable**`, link
   `[Improvement A.3](plans/prototype/improvement_A_3_persistence_failures_durability.md)`,
   cite finding 28, and summarize structured flush/delete failure, clean-only
   saves, import sequence advancement, bounded diagnostics, and durable
   file/directory barriers.
4. Rename finding 28 to `#### 28. Resolved: Persistence failures and sequence
   handling are explicit and durable`, add `**Status:** Resolved by
   [Improvement A.3](plans/prototype/improvement_A_3_persistence_failures_durability.md).`
   before the preserved original problem statement, then append a
   `**Resolution (Improvement A.3):**` paragraph naming commit-after-remove,
   structured clean-only flush, valid-result import sequence healing, capped
   diagnostics, file-plus-directory barriers, and the focused tests.
5. In finding 26's A.2 resolution, replace the stale sentence saying durable
   delete failure handling remains open with wording that only duplicate-
   coordinate canonicalization (finding 25) remains open and durable local
   delete failure handling is resolved by A.3/finding 28.
6. Leave A.10/findings 24 and 25 and B.2/finding 27 explicitly open. Preserve
   finding 24's warning that local deletion is not a CRDT operation without
   tombstones; do not imply that local durable deletion is distributed
   convergence or that the synchronous trait is browser-shaped.
7. Change this plan's `**Status:** Planned` to `**Status:** Completed` in the
   implementation commit, after the checks pass.

## 9. Architecture record, affected files, and versioning

### 9.1 ADR 0022 is required

Add
`docs/adr/0022-persistence-success-requires-durable-backend-commit.md` using
the exact title **"Persistence success requires durable backend commit"**, use
the Nygard template, and index it as Accepted in `docs/adr/README.md`. It must
state that it **supersedes decision 4 of ADR 0014 only**; do not edit accepted
ADR 0014. Record these decisions:

1. a vault persistence operation succeeds only after the backend returns from
   its durability boundary, and explicit drain succeeds only when clean;
2. data dirty keys precede meta and remain dirty on error;
3. local deletion is commit-after-durable-remove, with absence retries still
   requiring a directory barrier, but is not a CRDT tombstone;
4. valid import results advance and dirty store-local sequence metadata immediately while
   per-record partial acceptance and merge laws remain unchanged;
5. failure diagnostics are returned structurally and retained in bounded,
   deduplicated telemetry; and
6. native `FileStorage` durability is sync-file, atomic rename/unlink, and
   sync-directory, including durable directory creation. Other backends must
   define their own equivalent success boundary.

The alternatives section must reject ignored errors, remove-memory-first,
pending local tombstones (unnecessary for retry and confused with A.10), meta-
first flush, rename-without-sync, and silently claiming success on platforms
without a directory barrier. Consequences must acknowledge per-key rather than
multi-key atomicity, retryable visible records after failed delete, possible
whole new value visibility after a failed post-rename sync, and the unchanged
synchronous/browser limitation.

### 9.2 Exact affected file set

Implementation and tests are expected in:

- `crates/world-runtime/src/storage.rs`
- `crates/world-runtime/src/vault.rs`
- `crates/world-runtime/src/lib.rs`
- `crates/tools/src/filestore.rs`
- `crates/tools/src/vault.rs`
- `crates/tools/src/atlas.rs`
- `crates/tools/src/bin/atlas.rs`
- `crates/tools/src/bin/inspect.rs`
- `crates/tools/tests/atlas.rs`
- `crates/tools/tests/persistence.rs`
- `crates/tools/tests/preserve.rs`
- `crates/tools/tests/route.rs`
- `crates/platform-native/src/main.rs`
- `crates/platform-native/src/panel.rs`
- `docs/adr/0022-persistence-success-requires-durable-backend-commit.md`
- `docs/adr/README.md`
- `docs/world-model.md`
- this plan

Do not touch `.github/workflows/ci.yml`: existing CI already runs the native
workspace and neutral/web wasm checks needed here. Implement directory handles
and temp naming with `std` platform extensions; do not add dependencies or edit
`Cargo.toml`/`Cargo.lock`.

### 9.3 Versioning decision

This is contract hardening, not a serialized-data or world-generation change.
The in-memory result types, dirty ordering, filesystem barriers, diagnostic
registry, and imported meta update do not alter any encoded record body or
content-id fold. Therefore keep `RECORD_FORMAT_VERSION` unchanged, keep
`WORLD_ALGORITHM_VERSION` at 2, keep every layer `algorithm_revision` at 0,
and do not re-bless record or determinism fixtures. If implementation appears
to require any byte or identity change, stop and revise the plan rather than
silently expanding A.3.

## 10. Final invariant audit and definition of done

Before marking the plan/roadmap complete, review the final diff against this
checklist in addition to running section 7:

1. Every `flush`, `flush_all`, `remove_preserve`, and `remove_route` call site
   consumes the `Result`; no `let _ =`, ignored result, stale bool-only helper,
   or unconditional success log remains.
2. `flush_all -> Ok` implies `VaultStats::is_clean()` and
   `Vault::dirty_records() == 0`. Budgeted `flush -> Ok` with dirtiness is
   labeled backpressure, while every backend failure returns its typed
   operation/key and progress.
3. No dirty data key is retired before backend success, no meta write can
   overtake a failed data key, and import can allocate no local sequence at or
   below a valid resulting record's sequence.
4. A failed local delete leaves the record, dirty state, preserve contributor,
   and route tracking logically intact. A successful return has crossed the
   backend remove/absence durability barrier. No tombstone or claim of
   replica-wide deletion convergence was introduced.
5. `FileStorage::store` success has crossed temp-file `sync_all`, atomic
   rename, and containing-directory `sync_all`; durable creation covers every
   newly introduced ancestor. `remove` success has crossed a directory barrier
   even on a not-found retry. Unsupported barriers return errors.
6. Retained issues never exceed `MAX_VAULT_ISSUES`; repeated active failure
   changes only a count, all suppressed reporting is numeric/bounded, and a
   successful matching retry clears the active issue.
7. Neutral crates contain no filesystem, thread, socket, graphics, Windows, or
   Unix APIs; the three neutral/web crates still compile for wasm. The
   synchronous/lazy-storage limitation remains explicitly open.
8. `RECORD_FORMAT_VERSION`, `WORLD_ALGORITHM_VERSION`, layer revisions,
   record/generation golden bytes, merge laws, and content ids are unchanged.
9. ADR 0022 and its index row are present; accepted ADRs and all historical
   implementation/phase plans are untouched. `world-model.md` section 3.22,
   verification list, roadmap A.3, finding 26 cross-reference, and finding 28
   all describe the implemented behavior, and this plan says `Completed`.
10. `git diff --check` is clean and the A.3 worktree contains only the intended
    plan, code, tests, ADR/index, and `world-model.md` changes. Finish as one
    reviewed commit for this roadmap item; only then merge that commit to
    `main` and push it before starting A.4.
