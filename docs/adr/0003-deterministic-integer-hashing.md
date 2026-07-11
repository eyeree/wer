# 3. Deterministic integer hashing for identities

Date: 2026-07-10

## Status

Accepted

## Context

The world is effectively infinite and cannot be stored; it must be reconstructed
deterministically from stable inputs (plan sections 6.2, 18, 23.5). The same
inputs must produce the same permanent identities on native x86_64, native ARM,
and `wasm32`, and across CPU and (eventually) GPU evaluators. Sequential random
streams and unstable floating-point results cannot back permanent identities.

## Decision

Permanent identities are derived by integer hashing over an explicit, ordered set
of stable inputs — world algorithm version, region coordinate, generator layer,
feature index, possibility-state revision — using a portable `splitmix64`-based
mix (`world_core::hash`). Region coordinates are integers quantized from
continuous positions; feature indices are integers.

- Floating point is permitted only for approximate simulation and presentation,
  never for permanent identity.
- The field fold order is part of the stable contract. Any change to the hashing,
  the fold order, or a generation algorithm requires bumping
  `WORLD_ALGORITHM_VERSION` and updating the golden fixtures in
  `crates/world-core/tests/determinism.rs` in the same commit.
- A portable PRNG (`Rng`) may be seeded *from* a stable hash for approximate
  sampling; its float outputs are not sources of identity.

## Consequences

- Worlds are reproducible and portable; persistence stores only sparse deviations.
- Golden determinism tests guard against silent drift and double as the
  native/wasm cross-check.
- Contributors must consciously version any change that affects generated output,
  which is a deliberate friction.
