# 4. Terrain noise: hashed-gradient fBm with weak possibility coupling

Date: 2026-07-10

## Status

Accepted

## Context

Phase 1 needs an infinite, deterministic heightfield whose major topology stays
stable while possibility state drifts (plan section 9; phase-1-plan.md section
6.1). Landmark contradiction — mountains that move when the player steers
climate — is the primary continuity failure mode (section 23.1). Terrain must
also reproduce exactly across native and wasm, but per ADR 0003 only *integer*
identities are required to be bit-identical; `f32` presentation values are not.

## Decision

Elevation is multi-octave gradient (Perlin-style) noise, fBm-summed
(`world_core::terrain`):

- **Every lattice-corner gradient is selected by integer hashing** (a
  `splitmix64` mix over world version, octave, and corner coordinate). The
  gradients — the thing that decides *where* mountains are — are integer
  identities; interpolation and octave summation are `f32` presentation math.
- **Each octave's lattice is offset by a hash-derived constant**, so octave
  zeros never align (unoffset gradient noise is exactly zero at every shared
  lattice corner, which would stamp a periodic grid of forced sea-level points
  across the world).
- **Possibility coupling is deliberately weak and slow.** Elevation reads only
  the Geology dimension (relief amplitude, ±50%) and the Planetary dimension
  (sea-level shift, ±60 units) through smooth linear maps. Fast dimensions
  (climate, hydrology, ecology) never touch terrain, and possibility drift
  never dirties the terrain layer (ADR 0005). Steering deforms terrain gently;
  it never rearranges it.
- Sea level and shaping are fixed functions of the noise, not of fast
  possibility dimensions.

## Consequences

- Major topology is reproducible for a given `WORLD_ALGORITHM_VERSION` and is
  effectively stable under possibility drift — the property the Phase 1
  continuity result depends on.
- The gradient seed function is part of the versioned determinism contract:
  golden-fixtured in `world-core` and parity-exported by `platform-web`.
- Richer terrain (ridged transforms, erosion, hydrology carving) must be added
  as *deterministic functions of the same integer-seeded lattice* — or arrive
  with a version bump — rather than by re-rolling the noise.
