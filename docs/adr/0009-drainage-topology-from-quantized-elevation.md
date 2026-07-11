# 9. Drainage topology from quantized elevation at macro level

Date: 2026-07-11

## Status

Accepted

## Context

River networks are *topology* — permanent structure the player navigates by —
yet flow is inherently non-local: where a river runs depends on elevations far
upstream. Phase 2 needs river networks that (a) are exactly reproducible
across native and wasm, (b) never move under possibility drift of any speed,
(c) never tear at generation-window seams, and (d) fit the per-frame budget.
Section 6.2 of the implementation plan requires topology to be integer-derived;
section 9 requires that drift express through river *width and wetness*, never
through the network.

## Decision

Drainage is computed per **macro region** (level 4: 16×16 level-0 regions) at
one cell per region, over a 16-region apron (a 48×48 grid):

1. **Integer routing.** Elevation is sampled at each cell's region center and
   quantized to integer centimeters; every routing decision happens on those
   integers. Float elevation never decides topology.
2. **No runtime possibility input.** The elevation each cell routes over is
   sampled at the *quantized anchor-free possibility-field base* of that
   cell's region — a pure function of coordinates and the world algorithm
   version. A macro tile is therefore a permanent, window-independent value:
   rivers cannot walk under fast **or slow** drift, and a macro tile spanning
   the pinned zone can never rewrite the ground under the player. The
   realized-terrain-vs-routing skew in strongly steered worlds is a declared
   plausibility approximation; possibility expresses through the hydrology
   layer (width, wetness) instead. The declared Terrain edge is honored by
   folding the terrain algorithm revision into the drainage dependency hash,
   so terrain algorithm changes still invalidate every network.
3. **Window-independent flow directions.** A cell's direction is steepest
   integer descent (distance-weighted ×10 cardinal / ×7 diagonal) over its own
   3×3 quantized neighborhood, ties broken by an integer hash of the cell
   coordinate under a fixed basis. Adjacent macro tiles can never disagree
   about a shared cell.
4. **Truncated catchments.** Accumulation counts only cells inside the aproned
   window. Long rivers saturate rather than grow without bound; hydrology's
   logarithmic width mapping makes the truncation read as "big river" rather
   than a seam, and the continuity replay bounds the residual width step
   across macro boundaries. Hierarchical accumulation at higher macro levels
   is the future refinement, not Phase 2.
5. **Depressions become lakes.** Quantized local minima keep `FLOW_NONE` and
   act as lake/wetland seeds rather than being carved through — plausibility
   over hydraulic correctness.

## Consequences

- Flow directions and accumulations are full cross-platform identities: the
  wasm parity test pins a routing sample (direction + accumulation), not just
  a seed.
- The §12.3 "slow bucket flip → full pyramid" scenario deliberately excludes
  drainage: a Geology flip regenerates terrain, geology, and every expression
  layer, while the river network stands still. The invalidation ledger
  encodes this exclusion.
- Macro tiles are cached per macro coordinate with dependent-tracked eviction:
  resident while any level-0 region under them is resident (~4.5 KB each).
- Rivers at one-cell-per-region resolution are coarse ribbons; finer channel
  geometry is a later phase's refinement layered *under* this stable topology.
