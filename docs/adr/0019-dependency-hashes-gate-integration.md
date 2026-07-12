# 19. Dependency hashes gate integration; resident inputs outrank cache targets

Date: 2026-07-11

## Status

Accepted

Builds on [ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md)
and refines the integration gate described by
[ADR 0018](0018-settled-state-is-schedule-independent.md).

## Context

ADR 0008 makes a tile's dependency hash the compact provenance for all inputs
that can affect its content. The runtime used that hash to decide whether a
stored tile was fresh, but completed work was accepted using only its current
job id and dirty-bit bookkeeping. A missed invalidation could therefore publish
content whose dependency hash no longer described current authoritative state.
Cancellation could make that race less likely, but it could not prove content
provenance.

Capacity eviction exposed two related liveness failures. A dirty consumer
deferred when an ordinary or macro dependency was absent, but a clean producer
hint did not necessarily request the missing input again. The roster cache
could also evict a habitat signature still referenced by resident Ecology
state, causing inspection to fail and near-field realization to publish an
incomplete result.

The cache ceilings are logical targets, not hard process-memory limits. A
resident world's indispensable inputs must therefore take precedence over a
target that is too small to hold its current working set.

## Decision

1. **Dependency hashes are the integration-time provenance authority.** Every
   completed macro or region-layer result passes three checks before any output
   channel is replaced: its job id matches the current in-flight entry, its
   dependency hash matches the key captured at dispatch, and that hash matches
   the dependency key recursively expected from current authoritative state.
   Job id protects dispatch identity. Dirty bits are scheduling hints and
   cancellation is a work-saving optimization; neither is a provenance gate.

2. **Expected keys do not depend on cache residency.** A region layer's current
   expected key is derived recursively from its authoritative quantized
   domains, effective revisions, field resolution, and the expected keys of
   declared inputs in declaration order. Drainage uses its independently
   computable macro key. Materialized input presence and equality remain
   separate dispatch-readiness requirements.

3. **Rejected results return to ordinary scheduling.** An unavailable or
   unequal current key deterministically rejects the result before cache
   mutation. The runtime retires the matching in-flight work, reclaims owned
   buffers, marks the affected layer and dependent closure dirty, cancels
   obsolete dependent work, and lets the cost-budgeted topological dispatcher
   retry it. Macro rejection applies the Drainage closure to covered resident
   regions.

4. **Missing generated inputs heal through the declared DAG.** Before testing
   consumer readiness, dispatch restores work for any missing or stale ordinary
   input or Drainage macro. A producer job already carrying the current key is
   allowed to finish; obsolete work is cancelled and replaced. Regeneration is
   demand-driven, so evicting a macro does not immediately rebuild it when no
   dirty consumer needs it. A demanded macro that has just integrated remains
   protected through the pre-dispatch capacity pass until dirty Hydrology can
   take its immutable snapshot; the cache target may be exceeded transiently.

5. **Resident roster signatures are indispensable.** The union of habitat
   signatures tracked for all resident regions is the roster working set.
   Maintenance ensures every required entry exists, then capacity eviction
   considers only disposable entries in deterministic order. Near-field
   realization verifies that the whole region set is present before replacing
   organisms or advancing its Ecology key.

6. **A roster ceiling yields to its required floor.** When indispensable
   entries alone exceed the configured logical byte target, the cache retains
   them and reports the actual overage. This does not redefine logical payload
   accounting as a hard heap or process-memory cap.

These policies do not change generation formulas, dependency-hash folds,
stable identities, or persistence bytes.

## Consequences

- A stale result cannot publish merely because dirty bookkeeping missed an
  invalidation; current dependency provenance is checked on the main thread.
- Cache pressure can delay or repeat pure work, but a resident declared
  dependency chain can repair itself and settle through the normal scheduler.
- Inspection and realization retain complete roster inputs for resident
  Ecology state, while signatures not referenced by residents remain
  deterministically evictable.
- Actual roster bytes can exceed the selected target. Hard memory enforcement
  requires broader accounting and a policy that does not discard indispensable
  state.
- Job-id checks and cancellation remain useful, but ADR 0018's description of
  job id as the integration correctness gate is narrowed: job id establishes
  dispatch identity, while the dependency hash establishes content provenance.
