# 13. Shareable records are quantized at the persistence boundary

Date: 2026-07-11

## Status

Accepted

## Context

Phase 5 is the first time any world state outlives a process: named
discoveries, preserves, expedition routes, shared anchors, and the community
atlas schema (phase-5-plan.md). The engine's runtime state is `f32`/`f64`
presentation math, and ADR 0010/0011 deliberately declared the live derivations
that read it (habitat-signature classification, capture, resonance)
**presentation-grade** — deterministic within a run and platform, not asserted
across platforms. Both ADRs named the same upgrade path for the day sharing
would need portability: *quantize the classification inputs into portable
integer bands before hashing*.

Persistence forces the question in two directions at once. A **shared** record
must mean the same world on every platform, which floats cannot promise. A
**saved session** must restore *exactly* — the save→load→settle state hash must
equal the uninterrupted run's — which quantization would break.

## Decision

Persistence has **two tiers**, enforced by the record types in
`world-core/src/record.rs`:

1. **Shareable records carry only integers and strings.** `DiscoveryRecord`,
   `RouteRecord`/`RouteNode`, `PreserveRecord`, `SeenRecord`, and `AtlasBundle`
   quantize every float at write time: possibility vectors onto the existing
   `POSSIBILITY_QUANT` bucket grid (`PossibilitySignature`), strengths onto the
   same unit grid, positions and falloffs to integer world units, transition
   costs and stabilities to byte bands. Reading dequantizes to bucket centers —
   identical on every platform — so everything downstream of a record (`steer`,
   `project_plausible`, dependency hashes, tile regeneration from preserved
   buckets) is cross-platform *by construction*. Portability is won at the
   record boundary; the live derivations stay presentation-grade exactly as
   ADR 0010/0011 left them. The atlas never re-derives a classification on the
   receiving platform — it stores the result.

2. **The session tier is run-local and bit-exact.** `SessionSnapshot` (player,
   bias, live anchors, the resident window's `current` vectors, stabilities,
   revisions) carries raw IEEE bit patterns so that save→load is *state-hash
   exact* on the platform that wrote it. It is never shared, never merged, and
   excluded from bundles. A live anchor and its discovery record coexist: the
   session keeps the run's own floats exact while the record is the quantized
   shareable shadow.

The wire format is `postcard` under a versioned `Envelope`
(`RECORD_FORMAT_VERSION`, independent of `WORLD_ALGORITHM_VERSION`); the exact
bytes, serde field/variant orders, and content-id fold orders are golden-
fixtured, and readers refuse newer formats and migrate older ones forward.

## Consequences

- **No world-version bump.** Records change what outlives a frame, never any
  generated output for an input; `WORLD_ALGORITHM_VERSION` stays at 2.
- A shared anchor steers **bit-identically everywhere**: quantized integers in,
  and `steer`/`project_plausible` were already float-deterministic parity
  surfaces. This is machine-checked end-to-end by the `record_codec_sample` and
  `shared_steer_sample` parity exports.
- An anchor reconstructed from its own record steers within quantization
  epsilon (≤ half a bucket per domain) of the live original — asserted by unit
  test, so quantization loss stays a non-event.
- A preserve persists ~a few dozen bytes per region (coord + buckets) and
  deterministic generation reproduces the landscape (ADR 0008): identical
  possibility state, dependency hashes, and integer-topology surfaces on every
  platform; `f32` tile values remain per-platform float-deterministic as they
  have been since Phase 2.
- The knife-edge classification residual ADR 0010 documented is *routed
  around*, not removed: records store derived identities (signature seeds,
  species ids), so no shared meaning ever depends on re-running a
  presentation-grade classification.
- One-way door: a future record kind that stores raw floats in the shareable
  tier, or shares the session tier, must supersede this ADR.
