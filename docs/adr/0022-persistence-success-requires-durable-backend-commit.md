# 22. Persistence success requires durable backend commit

Date: 2026-07-12

## Status

Accepted

Supersedes decision 4 of
[ADR 0014](0014-vault-stores-deviations-with-crdt-merge.md) only. The record
identity, sparse-deviation, and CRDT merge decisions in ADR 0014 remain in
force.

## Context

The vault previously converted failed writes into unbounded diagnostic strings
and returned ordinary flush statistics. `flush_all` could therefore stop with
dirty data while a native caller announced a successful save. Preserve and
route deletion removed the in-memory record before attempting backend removal,
and ignored a failure, allowing a supposedly deleted record to reappear after
reopen. Import accepted records carrying high store-local sequence values but
did not advance the live store counter until a later reopen.

The native file backend wrote a same-directory temporary file and renamed it.
That prevented an ordinary torn value, but rename alone does not establish a
power-loss durability boundary: completed bytes must reach the file, and a new,
replaced, or removed directory entry must reach the containing directory. New
namespace ancestors have the same requirement. The neutral `Storage` trait
must express the outcome without importing filesystem concepts into
`world-runtime` or pretending that a future browser backend uses directory
barriers.

## Decision

1. **Success crosses the backend durability boundary.** A vault store or
   removal succeeds only after `Storage` returns from the durability boundary
   defined by that backend. A budgeted flush returns structured progress or an
   operation/key error; an explicit drain succeeds only when no dirty key
   remains.

2. **Data precedes metadata and remains retryable.** Dirty data keys sort before
   `meta/store`. A key is retired only after its backend write succeeds, and a
   flush stops at the first error. A failed data write therefore prevents
   metadata from overtaking it; a failed metadata write leaves only metadata
   dirty.

3. **Local deletion commits after durable removal.** A preserve or route stays
   visible in memory, exports, and runtime contributions until its backend
   removal succeeds. A retry that finds the key absent must still cross the
   backend absence barrier before the vault commits logical deletion. This is a
   local retry policy, not a CRDT tombstone or a claim that deletion converges
   across replicas.

4. **Valid import results advance local sequence metadata immediately.** Every
   valid added, merged, or unchanged record contributes its resulting local
   sequence to the import maximum. An increase updates and dirties store-local
   metadata in the same import call. Every valid value through `u64::MAX`
   remains accepted and shareable. When no strictly newer `u64` is
   representable, every local sequence-allocating mutation returns a typed
   exhaustion error before changing records, session state, metadata, or dirty
   keys. Rejected records do not contribute, valid siblings remain partially
   accepted, and encoded bytes, merge laws, and mutable ranking are unchanged.

5. **Failures are structured and telemetry is bounded.** Callers receive the
   operation, byte key, backend error, progress, and consecutive occurrence
   count as typed values. Retained diagnostics deduplicate stable identities,
   retain no more than 64 entries plus a saturating suppressed counter, and
   keep at most one active persistence issue. A matching successful retry
   clears that active issue.

6. **Native file success includes file and directory synchronization.** A
   store durably creates missing ancestors, creates a collision-safe hidden
   temp sibling, writes all bytes, synchronizes the temp file, atomically
   renames it, and synchronizes the containing directory. Removal synchronizes
   the containing or nearest existing directory after both unlink and a
   not-found retry. Native platforms without a supported directory barrier
   return an explicit backend error. Other `Storage` implementations define an
   equivalent success boundary for their medium.

## Alternatives considered

- **Ignore backend errors or report them only as strings:** rejected because a
  caller cannot distinguish clean completion from retryable dirty state.
- **Remove memory first and repair on failure:** rejected because reads,
  exports, preserve ownership, and route tracking would observe a deletion
  which had not committed.
- **Persist pending local tombstones:** rejected because commit-after-remove is
  sufficient for local retry and a local marker would be easily confused with
  the distributed tombstone protocol still required for CRDT deletion.
- **Write metadata before data:** rejected because a crash could publish a
  sequence advancement for records which never committed.
- **Treat rename or unlink without synchronization as durable:** rejected
  because whole visible bytes and durable directory entries are different
  guarantees.
- **Silently claim success where directory synchronization is unsupported:**
  rejected because it makes the same `Ok` result mean materially different
  things on different native platforms.

## Consequences

- Persistence remains atomic per key, not transactional across a flush. A
  crash may expose any successfully committed data prefix, but metadata cannot
  precede that prefix.
- A failed deletion leaves the record and its runtime effect visible and
  retryable. If unlink or rename completed before a later directory-sync
  failure, the whole absence or new value may already be visible even though
  the vault correctly returns an error and retries the barrier/protocol.
- Explicit saves and atlas imports can fail and leave honest progress
  telemetry. Periodic retries do not grow diagnostics without bound.
- Import may dirty only metadata even when every valid record is unchanged,
  ensuring that the next local edit is newer without requiring reopen.
- File and directory synchronization adds system calls to native persistence;
  authored records are sparse and frame flushing remains budgeted.
- `Storage` is still synchronous, vault opening is still eager, and no browser
  storage backend is supplied. A future asynchronous/lazy browser design must
  preserve this success meaning using its own transaction durability boundary.
