# 7. Declared layer dependencies supersede the static drift mask

Date: 2026-07-11

## Status

Accepted

Supersedes [ADR 0005](0005-drift-dirties-only-possibility-dependent-layers.md).

## Context

Phase 1 classified layers into a hard-coded drift mask (`DRIFT_LAYERS` =
climate | ecology): any realized-state change dirtied exactly those layers.
That worked for a fixed 3-layer stack but cannot scale to the Phase 2
environmental pipeline — climate, geology, hydrology, soils, biomes,
vegetation — where each layer is sensitive to *different* possibility domains
and to specific upstream layers. Keeping a hand-maintained mask in sync with
what generators actually read is exactly the "dependency explosion" failure
mode of plan section 23.3. ADR 0005 also accepted a deliberate skew: cached
terrain did not reflect slow-dimension (Geology/Planetary) drift while a
region stayed resident.

## Decision

Every layer declares, in a static table (`world_core::layer::LAYERS`), its
input layers and the possibility domains it reads *directly*. Dirtiness
propagates only along declared edges:

- When a region's realized state crosses a quantized-bucket boundary in a set
  of domains, the layers to re-check are the declared readers of those
  domains, closed over transitive dependents (`domain_dirty_mask`).
- Layer ids are assigned in topological order (deps have strictly lower ids),
  so acyclicity holds by construction and an id-order scan is a
  dependency-order traversal.
- `DRIFT_LAYERS` and the Phase 1 layer ids are gone; generators consume only
  their declared domains (undeclared domains read as a neutral constant), so
  an undeclared dependency cannot leak into generated content.

The stable trio — terrain, geology, drainage — declares no fast domain, which
makes section 9's stability commitment (rivers do not walk, mountains do not
move, rock does not change under a climate anchor) a checkable property of the
declaration table rather than a convention.

ADR 0005's accepted terrain drift-skew is retired: slow-dimension drift now
honestly (and cheaply, because slow dims are smooth and coarse-bucketed)
regenerates far-field terrain through the same declared mechanism. Pinned
regions never converge, so the near field still never rewrites itself.

## Consequences

- Invalidation precision is machine-checked: the `wer-ledger` harness asserts
  that every scenario of phase-2-plan.md §12.3 regenerates exactly the
  declared-dependent set, per region.
- Adding a layer means adding one declaration; the graph closures, dirty
  propagation, topological dispatch, and dependency hashing (ADR 0008) follow
  from it without touching the scheduler.
- The declaration must match what the generator actually consumes. This is
  enforced structurally on the possibility side (undeclared domains read
  neutral) and by review on the input-tile side (the biome layer declares
  Terrain because `classify` reads elevation — a deliberate addition to the
  planning table).
