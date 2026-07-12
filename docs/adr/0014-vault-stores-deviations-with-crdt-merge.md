# 14. The vault stores deviations, keyed by content-derived ids, with CRDT merge laws

Date: 2026-07-11

## Status

Accepted

## Context

The world is infinite and must not be fully stored (implementation-plan.md
section 18): generated base world data is reconstructed deterministically, and
persistence holds only sparse deviations (section 12.4). Phase 5 also has to
make records **shareable** — the Overview's social model (shared anchors,
published expeditions, the community atlas) needs two stores to combine without
a coordinator, and "server-compatible persistence model" (section 20, Phase 5)
needs a precise meaning that does not involve building a server.

## Decision

1. **The vault stores deviations only — never generated output.** The record
   store (`world-runtime/src/vault.rs`, the first and only user of the
   `Storage` trait) holds quantized intents and identities: discoveries,
   routes, preserves (coords + possibility buckets), the discovered-region
   bitmap, and the run-local session snapshot. No tiles, organisms, meshes, or
   any other derivable data ever enter it; everything downstream re-derives via
   ADR 0008 (tiles are functions of their dependency hash). Store size is
   `O(player actions)`, machine-checked by the vault harness. Anything later
   wanting to persist generated output (e.g. baked meshes) is a *cache*, not a
   record, and lives outside the vault.

2. **Record ids are content-derived.** A shareable record's id is a `mix`-fold
   of its immutable integer fields in a fixed, golden-fixtured order. The same
   discovery therefore yields the same id in every store on every platform,
   and two records with the same id have equal immutable fields *by
   construction*. A record whose stored id mismatches its recomputed fold is
   corrupt or tampered and is rejected (skip-and-report), never repaired or
   partially applied. Mutable presentation fields (name, journal) and the
   store sequence are excluded from the id — renaming never changes identity.

3. **Merge is a state CRDT.** Stores and bundles merge by union-by-id;
   immutable fields are conflict-free by (2); mutable fields resolve by a
   deterministic `(sequence, content-hash)` max; route `usage` merges by `max`
   (re-importing a bundle never double-counts); seen bitmaps merge by union.
   Merge is **commutative, associative, and idempotent** — asserted directly
   by the harness. This is the entire "server-compatible" claim: a future
   server is a dumb id-keyed record store, and bundle exchange needs no
   coordination.

4. **Persistence is budgeted and crash-consistent.** Mutations mark records
   dirty in O(1); `flush` writes at most `Budget::max_persist_ops` records per
   frame, each key atomically (the `Storage` contract; natively
   write-temp-then-rename). A crash loses at most un-flushed dirtiness. The
   store header's sequence counter may lag a crash; it heals on open by taking
   the max over loaded records.

## Consequences

- Durability is *exact*: a session saved mid-run and reloaded settles to the
  same two-run state hash as the uninterrupted run (the session tier is
  bit-exact, ADR 0013; everything else re-derives deterministically).
- Load order is irrelevant: records are independent keys, and steering was made
  order-independent in ADR 0011 precisely so persisted/shared anchors could
  arrive in any order.
- Deleting is a real operation (keys are removed eagerly), but identity is
  forever: re-creating the same discovery reproduces the same id, which is what
  makes deletion + re-import converge rather than fork.
- The key namespace (`meta/`, `session/`, `disc/`, `route/`, `pres/`, `seen/`)
  is part of the store layout contract; new record kinds get new prefixes and
  new `RecordKind`s (additive, no format break).
- One-way doors: records must stay self-contained (no cross-record pointers
  that break partial loading), and nothing in the shareable tier may depend on
  wall-clock time (sequences give order; clocks do not exist in the neutral
  crates).
