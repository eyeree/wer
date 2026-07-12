# 24. Gameplay uses one canonical organism slot; resource-tier density is visual

Date: 2026-07-12

## Status

Accepted

Builds on [ADR 0006](0006-travel-fueled-convergence.md),
[ADR 0010](0010-species-identity-presentation-grade-until-atlas.md),
[ADR 0011](0011-anchors-capture-trait-targets-combine-order-independently.md),
[ADR 0012](0012-resonance-gates-transition.md),
[ADR 0013](0013-shareable-records-quantized-at-persistence-boundary.md),
[ADR 0015](0015-routes-attract-as-derived-anchors.md),
[ADR 0018](0018-settled-state-is-schedule-independent.md),
[ADR 0019](0019-dependency-hashes-gate-integration.md), and
[ADR 0023](0023-field-cache-pressure-parks-derived-state.md).

It supersedes ADR 0012 decisions 2 and 4 only where they let the complete
tier-scaled realized population and a budget-selected node cap define
resonance. It tightens ADR 0018 decision 3: organism density may change
presentation population, never gameplay sampling or shared route bytes. The
travel/resonance multiplication rule and the settled-readiness caveat remain.

## Context

Phase 6 resource tiers realize one, two, or four organisms per cell. Slot 0
keeps the Phase 5 feature identity and higher slots add new identities. The
runtime nevertheless fed every displayed organism to nearest-organism capture
and resonance, and tiers selected resonance ceilings of 64, 96, and 128. A
hardware preset could therefore change convergence, captured traits, route
transition cost, route content id, and encoded shared bytes.

Organism realization was also one post-resonance pass under the tier-scaled
`max_realize_organisms` budget. A larger tier could publish the gameplay sample
earlier merely by admitting a full-density vector sooner. One L8 key could not
distinguish a completed canonical sample from a vector still awaiting visual
expansion, including the valid case where realization was empty.

Resource capacity is allowed to alter pacing and displayed density. It is not
allowed to select semantic inputs once the same prerequisite L8 and roster
state is ready.

## Decision

1. **Every organism carries its density slot.** `Organism::slot` records the
   realization loop slot directly. Feature indexing remains
   `cell + slot * resolution²`; slot is not folded a second time into identity.
   Slot 0 retains every Phase 5 id and RNG draw. Higher slots retain their
   additive Phase 6 ids.

2. **Slot 0 is the authoritative gameplay sample.** Capture searches only the
   nearest slot-0 organism. Resonance collects only slot-0 organisms. Public
   presentation iteration, rendering, diagnostics, and population counts keep
   every realized slot. Future gameplay consumers must choose an explicit
   canonical population or biomass model; they may not silently consume visual
   density.

3. **Canonical publication has one fixed scheduler.** Before resonance, the
   runtime visits fresh near regions in the existing nearest-first total order
   and publishes one whole slot-0 region per frame. This admission is not a
   `Budget` or `ResourceTier` field. It requires a recursively current L8 key
   and the complete resident roster set.

4. **Visual expansion is separate optional work.** After dispatch and the
   second integration, a budgeted pass may atomically recompute the canonical
   vector at the configured one/two/four-slot density. It may expand only a
   region whose canonical key is current. Recomputed slot 0 is bit-identical;
   higher slots are presentation additions. `max_realize_organisms` budgets
   this visual expansion only.

5. **Authority and presentation have separate currency.** One map stores the
   organism vectors, while separate keys record `(current L8)` canonical
   availability and `(current L8, displayed slot count)` visual completion.
   Empty vectors carry both keys. Missing or changed L8 provenance, missing
   signature/roster inputs, near exit, parking, preserve revision changes, and
   session replacement retire the vector and both keys before gameplay reads.

6. **Resonance has a fixed semantic ceiling.** The nearest/species/position
   total sort is unchanged and always truncates to
   `MAX_RESONANCE_NODES = 64`. `Budget::max_resonance_nodes` is removed.
   Density, entropy, distance, compatibility, occlusion, and the final equation
   are unchanged.

7. **Routes consume canonical frame resonance.** Callers pass the
   `FrameStats::resonance_strength` returned by the immediately preceding map
   update to `RouteRecorder`. Transition cost remains
   `floor(255 * (1 - strength))`; record fields, codec order, sampling spacing,
   sequence behavior, and content-id folds do not change.

8. **The guarantee begins at equal ready inputs.** The fixed publication pass
   removes resource-density and resonance-cap coupling. It cannot publish
   before L8 and its rosters are fresh. ADR 0018 still permits different
   executor/generation schedules to reach that prerequisite on different
   frames. Live capture and resonance remain presentation-grade across
   native/wasm under ADRs 0010 and 0011 because they read float habitat and
   expression. Given equal ready same-platform inputs, Low, Mid, and High must
   produce exact canonical capture/resonance and encoded route bytes.

## Alternatives considered

- **Use aggregate Ecology fields for gameplay:** viable for a future explicit
  biomass model, but it would change capture/resonance equations beyond this
  correction.
- **Use all slots but normalize their weights:** rejected because nearest
  capture and finite node selection would still depend on which presentation
  samples exist.
- **Give every tier the same displayed density:** rejected because additive
  visual population is a useful, measured quality setting and is safe once it
  is downstream of gameplay.
- **Keep the node cap in `Budget` but give tiers equal values:** rejected
  because a public temporal-work knob could still change semantic content.
- **Duplicate slot 0 in a second authoritative allocation:** rejected because
  separate vectors can drift; one vector plus two currency keys states the
  lifecycle without duplicate organisms.
- **Publish every ready canonical region in one frame:** rejected because a
  large near-window entry could hitch. One nearest whole region preserves the
  previous Low-tier pacing shape with a tier-independent bound.

## Consequences

- Low, Mid, and High retain approximately one/two/four times the displayed
  population and all existing additive ids, while canonical organism
  projections, capture, fixed-cap resonance, convergence input, and actual
  encoded route records agree once prerequisites are equally ready.
- A newly integrated L8 becomes gameplay-eligible on the next update. Visual
  budget pressure can delay extra slots but cannot hide or accelerate slot 0.
- Frame telemetry separates fixed authoritative publication from budgeted
  visual expansion. Budget assertions apply only to the latter.
- Same-platform replay hashing now includes the explicit slot label so broken
  labeling is observable; no literal diagnostic-hash golden is changed.
- This does not establish native/wasm equality for live float capture, nor
  frame-identical readiness across executors.
- No generation equation, dependency-hash fold, feature-id fold, layer
  revision, world version, record format, schema, codec order, or golden
  fixture changes.
