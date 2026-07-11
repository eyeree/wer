# 5. Possibility drift dirties only possibility-dependent layers

Date: 2026-07-10

## Status

Accepted

## Context

The Phase 0 bootstrap conservatively set `dirty_layers = u32::MAX` whenever a
region's realized possibility state changed, meaning any drift would regenerate
every layer — the "dependency explosion" risk of plan section 23.3, and a direct
cause of visible chunk replacement (a terrain tile regenerating under the
player's eyes reads as a pop). Phase 1 has a fixed 3-layer stack — terrain,
climate, ecology — and terrain is possibility-stable by construction (ADR 0004).

## Decision

`RegionState::converge` narrows dirtying to the drift mask
(`world_core::layer::DRIFT_LAYERS` = climate | ecology). The terrain layer is
never dirtied by possibility drift; terrain tiles are generated once per region
residence and are treated as never-stale within a run (phase-1-plan.md
section 8). Fresh regions start with all layers dirty (`ALL_LAYERS`), which is
what triggers their initial generation.

Layer membership in the drift mask is a static property of the Phase 1 stack.
Phase 2's generalized layer dependency graph (plan section 6.5) supersedes this
constant with per-layer declared dependencies; until then, a new layer added to
the stack must be classified into the drift mask explicitly.

## Consequences

- A possibility nudge recomputes only the two cheap layers per affected region;
  budgeted regeneration keeps frames flat (measured: ~6 ms worst-case inline
  for a default window, off the main thread via the Rayon executor in the app).
- Terrain can never pop from drift, machine-checked by the continuity replay
  and the `drift_dirties_climate_and_ecology_but_never_terrain` unit test.
- Cached terrain intentionally does *not* reflect Geology/Planetary drift while
  a region stays resident; the small realized-vs-cached elevation skew is
  invisible in practice and vanishes on reload. If later phases want terrain to
  respond to slow drift, that becomes an explicit, versioned layer-graph edge.
