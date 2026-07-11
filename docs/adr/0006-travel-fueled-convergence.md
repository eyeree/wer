# 6. Convergence is fueled by player travel, not wall-clock time

Date: 2026-07-11

## Status

Accepted

## Context

Phase 1 originally converged unpinned regions toward their targets every
frame. That let transformation proceed while the player stood still, which
produced a bad failure mode: with a standing bias, the entire far field would
quietly finish converging while the pinned near disc held the old state,
banking up a sharp old/new discontinuity exactly at the pinned boundary. A
player who stood still for a while and then moved a little would walk straight
into a world-sized change — the opposite of the continuity the prototype
exists to demonstrate. It also fought the game's core fantasy: the vision is
*continuous travel through possibility space*, where the journey itself is
what carries you into different worlds.

## Decision

Convergence is fueled by travel. `RegionMap::update` takes the distance the
player moved since the previous update; the per-update convergence rate is
`converge_per_unit × travel`, clamped to `converge_rate_cap`
(`StreamConfig`). Consequences of the formula:

- A stationary player's world is perfectly still: zero travel, zero
  convergence, no realized state moves anywhere, so change can never
  accumulate out of sight.
- Transformation unfolds in proportion to distance traveled and is frame-rate
  independent to first order; moving faster transforms the world faster, up
  to the cap (which keeps even a sprinting or scripted camera's per-step
  change smooth).
- Only convergence is gated. Streaming (load/evict), stability, retargeting,
  and regeneration of already-dirty layers still run every update — a fresh
  window settles, and pending regen completes, without movement.

Travel is an explicit input rather than being derived internally from
successive positions: callers own the definition of "journey" (the app feeds
real displacement; tests and the replay script it; a future mechanic could
feed synthetic travel, e.g. meditation or vehicles).

## Consequences

- The stand-still-then-step cliff is gone by construction, and bias/anchor
  edits made while stationary take effect only as the player moves — steering
  chooses where the journey goes, it does not teleport the world.
- The continuity replay and unit tests pin the new contract (no travel ⇒ no
  convergence; travel ⇒ bounded, budgeted convergence).
- Anything that later wants time-driven world change (seasons, weather)
  must be modeled outside realized-state convergence or revisit this ADR.
