# 10. Species identity is presentation-grade until the atlas needs otherwise

Date: 2026-07-11

## Status

Accepted

## Context

Phase 3 grows organism-level richness — procedural genomes, species archetypes,
food webs, near-field organisms (phase-3-plan.md). A species roster is a
function of a *habitat*: distinct habitats carry distinct rosters, and identical
habitats across the world share species so the roster cache stays bounded and
the world reads as ecologically zoned (§4.1, §6.3).

That forces a choice about how a cell's habitat is classified. The natural
inputs are the settled environment tiles — biome id, climate temperature and
moisture, soil fertility — but those are `f32` presentation state. Biome ids
already carry this property: the Whittaker classifier compares floats, so a cell
on a knife-edge threshold may classify differently across native and wasm
(phase-2-plan.md §7.6). Hashing a habitat signature from those tiles inherits
the same residual. Meanwhile the Phase 2 determinism invariant (section 23.5)
demands that *some* well-defined surface be cross-platform identical, and the
future community atlas (Overview) will need a cross-platform species identity so
two players in "the same" habitat discover the same species.

## Decision

Phase 3 species identity is split into a portable core and a presentation-grade
derivation, exactly mirroring how Phase 2 treats biome classification:

1. **Portable, golden-fixtured, wasm-parity-tested.** `HabitatSignature::seed`,
   `genome(seed)`, `species_seed(signature, index)`, and `food_web` tier
   biomass for a fixed roster are pure integer→integer (or
   integer→portable-`f32`) functions. Given a signature, everything downstream
   is bit-identical on every platform. `genome(seed)` is the parity export the
   browser port pins (§12.5).
2. **Presentation-grade, per-platform, replay-hash-checked only.** The
   `HabitatSignature` a cell *derives* (`HabitatSignature::of`) reads `f32`
   climate/soil tiles, so a knife-edge cell may band differently across
   platforms — and therefore which roster it gets and which organisms realize.
   This is deterministic and reproducible within a run and platform; it is
   **not** asserted cross-platform, and signature derivation is deliberately not
   a wasm parity export (asserting it would bake in a guarantee Phase 3 does not
   make).
3. **No world-version bump.** Phase 3 appends layer L8 and new generators but
   changes no existing layer's output for identical inputs, so
   `WORLD_ALGORITHM_VERSION` stays at 2 and no Phase 2 golden re-blesses
   (§9.1).

## Consequences

- The coherence and diversity harness (§12.3) checks *properties* (roster size,
  trophic bounds, response to steering) that hold per-platform, not
  cross-platform habitat equality.
- Species discoveries, named organisms, and cross-platform species identity are
  deferred to Phase 5 together with persistence: the `Storage` trait stays
  unused and rosters are reconstructed deterministically per run.
- **The named upgrade path:** when the atlas needs cross-platform species
  identity, quantize the classification inputs (temperature, moisture,
  fertility, and the biome-boundary decision) into portable integer bands
  *before* hashing the signature — the same move that would make biome ids
  portable. The banding in `HabitatSignature` is already integer; only its
  `f32`-reading `of` constructor is the residual, and it is isolated so the
  upgrade touches one function.
- This ADR governs any later decision that would constrain species identity
  portability; revisit it (with a superseding ADR) rather than silently
  promoting signature derivation to a cross-platform guarantee.
