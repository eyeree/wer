# 12. Resonance gates transition; it multiplies the travel-fueled rate

Date: 2026-07-11

## Status

Accepted

Extends [ADR 0006](0006-travel-fueled-convergence.md).

## Context

ADR 0006 made world convergence fueled by player *travel*, not wall-clock time,
closing a stand-still cliff: a stationary player's world is perfectly still, so
change can never silently bank up into an old/new discontinuity at the pinned
boundary. Phase 4 adds the game's steering front-end and, with it, the Overview's
"transition controls": the player's avatar resonates with nearby reality, and
"sparse environments make transition difficult or impossible" while dense, varied
surroundings let the player steer strongly (section 14, Overview "Player Avatar").

This introduces a **resonance** signal — a scalar transition capability computed
from the near-window features (Phase 3's realized organisms and aggregate
fields). The question is how resonance couples to convergence. If resonance could
*drive* change (add to the rate), a player standing in a rich biome would see the
world transform while stationary — reopening exactly the cliff ADR 0006 closed,
because change would accumulate out of the travel budget's control.

## Decision

1. **Resonance multiplies; it never adds.** The convergence rate is

   ```text
   converge_rate = converge_per_unit · travel · resonance · transition_scale
   ```

   clamped to `converge_rate_cap`. Because resonance and travel both enter as
   factors, the rate is zero when *either* is zero:
   - zero travel ⇒ still world regardless of resonance (ADR 0006 preserved);
   - zero resonance ⇒ a barren neighbourhood holds the world still no matter how
     far the player travels — change resumes only when they reach richer ground,
     and because travel gates too, nothing banked up while they crossed the
     barren stretch.

   Resonance can only *slow or enable* transformation, never manufacture it.

2. **The resonance graph is transient and locally built.** It is rebuilt each
   frame from the near-window organisms and aggregate tiles (a pure read of the
   settled caches, order-independent, capped at `max_resonance_nodes`) and
   dropped at end of frame — never a global stored structure (section 14). Only
   the scalar strength is folded into convergence; the node list survives one
   frame for the viz.

3. **Transition mode scales the deliberate-steering rate distinctly.** A
   `transition_mode` flag threaded into `RegionMap::update` scales
   `converge_per_unit` down for slow, precise reality-transition travel, versus
   fast free-exploration travel that surveys the world — free movement surveys,
   transition movement steers (Overview, Movement).

4. **Resonance strength is presentation-grade.** It reads the presentation-grade
   realized organism set (ADR 0010/0011), so it is deterministic and reproducible
   within a run and platform, but not asserted cross-platform.

## Consequences

- The continuity replay's stand-still and no-boundary-discontinuity assertions
  hold through the gate: a stationary player still produces zero convergence, and
  a resonance-forced-low run produces no convergence, banking nothing at the
  pinned boundary.
- Two-run state-hash determinism holds: resonance is a deterministic,
  order-independent read of settled caches.
- **The one-way door.** Anything later wanting resonance to *drive* change (rather
  than gate it) — e.g. ambient transformation in a rich biome — must revisit this
  ADR with a superseding record, because that reopens the ADR 0006 cliff and
  changes the continuity guarantee the replay depends on.
