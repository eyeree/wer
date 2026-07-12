# 15. Routes attract as derived anchors; attraction is soft and saturating

Date: 2026-07-11

## Status

Accepted

## Context

Section 13 of the implementation plan asks for routes through possibility
space whose frequent use "creates a soft attraction field rather than forcing
exact replay". Phase 5 records expeditions as quantized `RouteRecord`s
(ADR 0013/0014) and must decide *how* a recorded route influences the world.
A bespoke route-steering system would be a second algebra to keep coherent
with anchors, projection, travel-fueled convergence (ADR 0006), and resonance
gating (ADR 0012) — four hard-won invariants.

## Decision

1. **Route attraction is derived anchors, on the fast domains only.** Each
   frame, the recorded nodes within the corridor radius of the player become
   weak `Emphasize` anchors toward the node's recorded possibility state
   (`world-core/src/route.rs::attraction_anchors`), masked to
   `ROUTE_ATTRACTION_MASK` — every domain *except* Geology and Planetary — so
   a followed route recreates the remembered corridor's living character
   (climate, water expression, ecology, morphology, behavior, aesthetics)
   without ever steering the stable topology (section 9's rule; the vault
   harness machine-checks that persisted influence never regenerates the
   stable trio). Anchors are capped nearest-first at
   `Budget::max_route_attraction_nodes` and appended to the player's own
   anchors before the unchanged order-independent `steer` →
   `project_plausible` path. There is no second steering system: route
   influence automatically composes with player anchors (ADR 0011), obeys
   plausibility projection, and remains travel-fueled and resonance-gated.

2. **Soft and saturating.** Anchor strength is `route_pull(usage)` — monotone
   in the traversal count ("frequently used routes become easier to follow"),
   nonzero at usage 0 (a freshly shared route is followable), and capped well
   below 1 (`ROUTE_PULL_CAP`), so a route biases the target toward its
   recorded signature but can never force it, and a conflicting player anchor
   can always win. Beyond `ROUTE_CORRIDOR_RADIUS` a route has no influence.

3. **Difficulty falls out of the world model.** A node's transition cost is
   banded from `1 − resonance` at record time; `route_difficulty` is the mean
   node cost. Barren, low-resonance ground records hard routes; dense living
   ground records easy ones — no designer knob.

4. **The route graph is a view.** The possibility-space index
   (`RouteGraph`, nodes keyed by quantized signature) is rebuilt from records
   and never persisted (section 13: not stored as a global graph). Records
   are the truth; usage merges by `max` (ADR 0014), so shared traversal
   counts never double-count.

## Consequences

- Route steering is portable end-to-end: records carry quantized integers,
  and attraction anchors + `steer` are float-deterministic, so a shared route
  attracts identically on every platform.
- The stand-still and barren-ground guarantees survive unchanged: a stationary
  player on a route sees a still world (zero travel), and a route through
  barren ground cannot force change (resonance still gates convergence).
- Traversal detection (enough of a corridor walked in one leg) bumps usage
  once per leg, debounced — camping on a route does not inflate it.
- One-way door: any future mechanic wanting routes to *replay* recorded
  worlds exactly (rather than bias toward them), or to steer through a
  channel other than the anchor algebra, must supersede this ADR.
