# 11. Anchors capture trait targets and combine order-independently

Date: 2026-07-11

## Status

Accepted

## Context

Phase 1 shipped a deliberately blunt anchor sketch (`anchor.rs`): two kinds —
`Emphasize` and `Suppress` — that pull the masked possibility dimensions toward
the fixed bounds `1`/`0`, a per-domain mask, radial falloff, and a two-rule
`project_plausible`. Its stated purpose was to prove *the seam between steering
and constraints exists*, not to model steering. Anchors were disconnected from
anything the player discovers, and `steer` combined them by a **sequential
contraction** whose order sensitivity was documented as "mild but deterministic"
— a latent determinism trap the moment anchors are ever reordered (persistence
load order, shared anchors, Phase 5).

Phase 4 makes steering the game (phase-4-plan.md): the player captures the
traits of the living things and places they choose to remember, and emphasizes,
suppresses, or combines those captures to steer the world's evolution. This
forces three decisions about what an anchor *is* and how anchors combine.

## Decision

1. **An anchor carries a captured `target` and a `source`.** `Anchor` grows from
   `{kind, mask, strength, falloff}` into the full section 8 shape: a
   `PossibilityVector target` the masked dimensions are pulled toward
   (`Emphasize`) or pushed away from (`Suppress`, the anti-anchor), plus an
   `AnchorSource` recording what the anchor was captured from (an organism with
   its `Species::id`, a landform, a river, an atmosphere, or `Manual` for a
   debug placement). The Phase 1 `Emphasize`/`Suppress`-toward-a-bound behaviour
   is the special case `target = 1.0` across the mask, so the Phase 1 debug keys
   keep working by constructing `Anchor { target: bound, source: Manual, .. }`.

2. **A captured target is a nudge, not a snapshot.** `capture_target(baseline,
   deviation, mask, gain)` sets each masked domain to `clamp(baseline + gain ·
   deviation)` — the habitat's own possibility signature pushed a bounded step
   toward what makes the discovery distinctive (its deviation from the habitat
   baseline). The anchor targets neither the raw discovery nor the fixed bound,
   but *the world that would make this discovery typical*. This is what makes
   steering feel like carrying a memory forward rather than copy-pasting a place.

3. **`steer` is rewritten order-independent.** For each masked domain, emphasize
   anchors contribute an influence-weighted pull toward their combined target
   and suppress anchors a weighted push away from theirs (a reflection about the
   base); the base is blended toward each combined desired value by a
   *saturating* weight `1 − ∏(1 − wₐ)`. Both the weighted means and the
   saturating products are symmetric functions of the anchor *set*, so the result
   is a pure function of the set and the position — not of slice order. This
   retires the Phase 1 order caveat and is asserted directly by a unit test and a
   golden encoding the same output for two orderings.

4. **Captured targets are presentation-grade.** `capture_target` and
   `organism_trait_deviation` read `f32` tiles and organism expression, so which
   *world* a capture yields is per-run, per-platform — the ADR 0010 lineage. The
   portable surface Phase 4 adds is the pure `steer`/`project_plausible`/
   `capture_target` *math*, float-deterministic and identical on native and wasm
   for the *same inputs* (the `steer_sample` parity export). Live `capture_at`
   and resonance are deliberately not parity exports.

## Consequences

- **No world-version bump.** Steering moves a region's runtime `target`/`current`
  possibility vector — presentation state that has never been a golden-fixtured
  world identity. `steer`/`project_plausible` change behaviourally and get *new*
  Phase 4 fixtures; no Phase 2/3 identity fixture re-blesses, and
  `WORLD_ALGORITHM_VERSION` stays at 2 (§9.1).
- The two-run continuity replay's state-hash equality is now robust to any future
  anchor reordering (persistence load order, shared anchors), because
  combination no longer depends on order.
- The in-fiction trait categories (coloration, morphology, behavior, …) map onto
  the eight possibility domains via `TraitCategory`, several collapsing onto one
  scalar in Phase 4 — a recorded limitation (§1.4) that resolves without touching
  the anchor algebra when the possibility vector grows (the mask simply widens).
- Cross-platform anchor/trait identity for the community atlas is a Phase 5
  problem, solved then with the same move ADR 0010 names — quantize the
  classification inputs into portable bands before capture.
- This ADR governs later steering decisions; revisit it with a superseding ADR
  rather than silently reintroducing order-dependence or promoting live capture
  to a cross-platform guarantee.
