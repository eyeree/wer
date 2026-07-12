# 20. Preserve overlaps use the lowest content id and material snaps advance revision

Date: 2026-07-11

## Status

Accepted

Builds on [ADR 0007](0007-declared-layer-dependencies.md),
[ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md),
[ADR 0013](0013-shareable-records-quantized-at-persistence-boundary.md), and
[ADR 0014](0014-vault-stores-deviations-with-crdt-merge.md).

## Context

A preserve is a sparse durable claim that one or more regions should realize
recorded quantized possibility signatures. Atlas merge permits independent
preserves to overlap, but the runtime previously stored only one signature per
coordinate. The last record applied silently replaced any earlier one, and
deleting either record removed the whole override. Startup, import, or UI event
order could therefore select different landscapes and deleting one record could
unpin a coordinate that another record still covered.

Applying a preserve to a resident also snaps its live possibility vector to
bucket centers. That snap could materially change the vector without advancing
`RegionState::revision`. Tile staleness correctly follows quantized dependency
hashes, but near-field organism identities include the revision. In particular,
normalizing a value within its existing bucket changes realized state without
changing any tile key, so an old-revision organism vector could survive a new
realized epoch.

Preserve content ids are immutable `u64` values derived from their quantized
content. They already have a portable total order and vault records already live
in ordered maps. Names, journals, sequence numbers, and application order are
mutable or local and cannot define shared ownership.

## Decision

1. **Retain every contributor.** `RegionMap` stores an ordered map from region
   coordinate to an ordered map of preserve content id to signature. The nested
   map is the source of truth and survives ordinary and capacity eviction.

2. **The numerically lowest content id wins.** The first contributor is the
   effective owner and signature. Adding or removing a non-winner changes only
   contributor bookkeeping. Startup, session restore, and resynchronization
   collect a complete batch, install all contributions, and then reconcile each
   touched resident coordinate once. Records still traverse by ascending id and
   coordinates canonically for auditability, but reversing distinct records
   inside that batch cannot create intermediate revision or organism epochs.

3. **Winner changes reconcile centrally.** Adding a lower-id contributor or
   removing the winner applies the new effective signature immediately to a
   resident. Removing the last contributor releases the region without a snap:
   its current vector, revision, tiles, jobs, and organisms remain untouched
   until ordinary retargeting and travel-fueled convergence resume. If only the
   owner id changes and both signatures are equal, realized state is unchanged.

4. **Material vector changes and bucket changes are separate events.** A newly
   effective signature sets resident `current` and `target` to its canonical
   bucket centers and stability to one. If the exact old and new vectors differ,
   revision advances once with wrapping arithmetic. Only domains whose
   quantized buckets differ dirty their ADR 0007 declared-reader closure and
   retire matching in-flight tile work. Thus same-bucket normalization advances
   revision without changing tile dependency hashes or cancelling tile work.

5. **A material snap retires near-field organisms immediately.** The old vector
   and its realization key are removed and recycled before a later realization
   pass. Rebuilding waits for fresh Ecology inputs and uses the incremented
   region revision, including when the L8 dependency hash itself did not change.

6. **Deletion resolves runtime ownership.** Native deletion asks
   `RegionMap` for the effective owner at the player's coordinate, removes that
   exact vault record, and removes only that record's contributions across its
   regions. Any retained successor becomes effective.

Separate application calls are separate material events. Their final effective
owner and signature still follow the contributor set, but revision and organism
epochs legitimately record each sequential winner change. Arbitrary UI event
history is therefore not collapsed into one order-independent event.

This decision does not change preserve record construction, content-id folds,
CRDT merge laws, record encoding, generation formulas, or algorithm revisions.

## Alternatives considered

- **Last applied wins:** rejected because startup and import order become
  authoritative state.
- **Highest sequence wins:** rejected because sequence is store-local mutable
  metadata and is excluded from immutable identity.
- **Lexicographically smallest name wins:** rejected because names are mutable
  presentation fields.
- **Blend overlapping signatures:** rejected because it invents state not
  recorded by either preserve and would require a new persistence and
  generation contract.
- **Reject every overlap:** rejected because independently authored atlas
  records may legitimately cover the same coordinate; deterministic retention
  composes with the existing union-by-id merge.

## Consequences

- Preserve ownership is always a function of the contributor set. Canonical
  startup/session/import batches also produce identical resident revision,
  tiles, and organisms when their distinct records are traversed in reverse.
- Sequential UI events retain their material history; only their final owner
  and signature, not their revision count, are order-independent.
- Deleting a winner reveals the next contributor; deleting a non-winner is
  inert, and deleting the final contributor keeps the established no-snap
  release behavior.
- A same-bucket snap can change organism identities and expressed traits while
  leaving every generated tile and in-flight tile job untouched.
- Sparse runtime memory grows by one ordered entry for each covering
  preserve/region pair, including non-winning contributors.
- Lowest-id ownership is deterministic, not a claim that 64-bit content ids are
  collision-proof. Wider authenticated identity and duplicate-coordinate
  canonicalization remain separate work.
- Durable delete failure handling remains separate; this decision preserves the
  current vault removal interface.
