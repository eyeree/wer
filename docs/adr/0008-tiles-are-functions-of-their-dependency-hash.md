# 8. Tiles are functions of their dependency hash

Date: 2026-07-11

## Status

Accepted

## Context

Phase 1 tiles carried `(world_version, revision)` provenance: staleness meant
"the region's realized state moved at all", which over-invalidates (a
sub-epsilon drift regenerated every drift layer) and under-describes (a tile
could not tell *which* of its inputs changed). With a six-deep layer graph,
staleness must be exact per `(region, layer)` or the dependency explosion
returns through the back door.

## Decision

1. **Quantized possibility inputs.** Each domain scalar quantizes into one of
   `POSSIBILITY_QUANT = 4096` buckets; generators consume the *dequantized
   bucket center*, never the raw runtime float. Drift smaller than a bucket
   therefore costs zero regeneration, and a tile's content is a pure function
   of integer inputs.
2. **The dependency hash.** Every generated tile records
   `layer_dep_hash(region, layer, algorithm_revision, buckets, input_hashes,
   resolution)` — a `mix`-fold (fixed order, part of the stable contract) of
   the world algorithm version, the layer id and its algorithm revision, the
   region coordinate, the field resolution, the quantized buckets of the
   layer's declared domains, and the dependency hashes of its declared input
   tiles, in declaration order.
3. **Staleness is one integer comparison.** A tile is stale iff its stored
   hash differs from the freshly computed expected hash. Because input hashes
   chain, a change anywhere upstream — a bucket flip, an upstream algorithm
   revision, the world version — changes every downstream expected hash
   automatically. There is no second invalidation mechanism to keep in sync;
   the scheduler's dirty bitset is an exact optimization *hint* over this
   ground truth, never a substitute for it.
4. **Per-layer algorithm revisions.** Tuning one layer's constants bumps that
   layer's `algorithm_revision` (invalidating it and its dependents) instead
   of the world version, which keeps the stack tunable without world-wide
   re-blessing (phase-2-plan.md §9.2).

Dependency hashes are **run-local cache keys, not identities and not a
persistence format**: buckets come from runtime float state, whose boundaries
may land differently across platforms. The cross-platform identity surface
remains the integer seed layer (ADR 0003): gradient seeds, control-point
seeds, lithology seeds, drainage routing.

## Consequences

- Region revisions still exist (pinned-stability contract, replay) but no
  longer drive staleness.
- Supersession sharpens: a result whose layer was re-dirtied while the job was
  in flight is dropped on arrival, because its dependency key is no longer the
  expected one — the same shape as Phase 1's revision check, with an exact key.
- Content being a function of the dependency key gives a *stronger*
  order-independence argument for threaded execution than Phase 1 had:
  whichever job lands last for a key, its content is identical.
- The convergence lerp updates `current` every step, but layers only go stale
  when a bucket flips — quantization is what rate-limits invalidation.
