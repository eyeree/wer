# Phase 1 sign-off — Continuous World Transformation Prototype

Date: 2026-07-10

## The question (implementation-plan.md section 22)

> Can a deterministic region-based world transform continuously through
> possibility space while preserving nearby stability and avoiding visible
> regeneration artifacts?

## The answer

**Yes** — with the caveat that the visual half of the criterion still wants a
human eyeball on it (see *Evidence*, below).

The mechanism that makes it work, validated end to end:

1. **Terrain is possibility-stable** (ADR 0004): topology comes from
   integer-hashed gradients; only the slow Geology/Planetary dimensions touch
   it, through smooth maps, and drift never dirties the terrain layer
   (ADR 0005). Mountains cannot walk.
2. **Realized state is pinned near the player** (`stability = 1` inside the
   near radius) and converges only at distance, on a smoothstep ramp — the
   player never watches the ground rewrite itself.
3. **Regeneration is narrowed and budgeted**: a possibility change recomputes
   only climate + ecology tiles, a bounded number per frame, rippling outward
   over frames instead of hitching.

## Evidence

- **Headless continuity replay** (`wer-replay`, and
  `crates/tools/tests/continuity.rs` in CI): a scripted path with possibility
  nudges and Emphasize/Suppress anchors, asserting per frame that
  (a) no pinned region ever bumps its revision or changes a cached drift
  sample, (b) no cached sample moves more than an epsilon per frame,
  (c) adjacent regions' targets never differ beyond the field gradient bound,
  and (d) two runs produce bit-identical final state. **Passes.**
- **Determinism goldens** (`crates/world-core/tests/determinism.rs`): elevation,
  climate, ecology, field sampling, steering, and projection are pinned at
  `WORLD_ALGORITHM_VERSION = 1`.
- **Native ↔ wasm parity** (`platform-web` tests + CI `wasm32` check): the
  integer identities behind topology (gradient seeds) and the possibility field
  (control-point seeds) are golden-pinned and exported by the wasm shell.
- **Budgets hold** (criterion, dev machine, release profile): elevation sample
  ≈ 116 ns; steady-state `RegionMap::update` over the default ~450-region
  window ≈ 79 µs; worst-case sustained drift ≈ 6 ms with *inline* generation —
  and the interactive app runs generation on the Rayon executor off the main
  thread. Field cache for the default window ≈ 5 MB, bounded by eviction.
- **Interactive check** (`cargo run --release --bin wer`): the app streams,
  steers, and renders with zero changed-while-pinned violations logged. The
  subjective "no visible pop at the horizon" judgment is the remaining human
  step; the changed-while-pinned flash (`X`) and the revision channel (`V`)
  exist precisely to make any failure obvious.

## What Phase 1 deliberately did not build

Hydrology/soil/biome layers as a real dependency graph (Phase 2), organisms
(Phase 3), persistence through `Storage` (Phase 5), Web Workers/browser runtime
(Phases 6–7), production terrain rendering. The `wasm32` build compiles and
agrees on identities; nothing in the design blocks the browser port.

## New ADRs

- [ADR 0004](adr/0004-terrain-noise-and-weak-possibility-coupling.md) — terrain
  noise choice and weak possibility coupling.
- [ADR 0005](adr/0005-drift-dirties-only-possibility-dependent-layers.md) — the
  layer dirtying policy Phase 2's dependency graph will generalize.
