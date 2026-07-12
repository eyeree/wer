# 18. Settled world state is schedule-independent; budgets and tiers scale pacing and capacity, never identity

Date: 2026-07-11

## Status

Accepted

Builds on [ADR 0006](0006-travel-fueled-convergence.md),
[ADR 0008](0008-tiles-are-functions-of-their-dependency-hash.md), and
[ADR 0012](0012-resonance-gates-transition.md).

## Context

Since Phase 1 the codebase has leaned on two informal properties: generation
results integrate order-independently (a threaded executor converges to the
same cache as an inline one), and per-frame budgets change *when* work
happens, not *what* the world is. Phase 6 exploits both aggressively — a
priority-lane executor with cancellation, budget scaling per resource tier,
retarget amortization — so "informal" is no longer good enough: any of these
optimizations could quietly leak scheduling into world content, the classic
determinism failure of an optimization phase (implementation-plan.md §23.5).

There is one genuine subtlety. *While the player travels*, realized
possibility state legitimately depends on pacing: convergence is
travel-fueled (ADR 0006) and resonance-gated (ADR 0012), and resonance reads
the realized near-field organisms — which exist only once their region's
ecology layer has generated. A slower executor realizes organisms a frame or
two later, gates convergence differently for those frames, and leaves
different realized vectors behind; those vectors freeze wherever the ramp
pins them. That is not a bug — it is the designed coupling of transformation
to the journey — but it means "any two schedules produce bit-identical
worlds at every instant" is *not* the invariant, and pretending otherwise
would gate CI on a falsehood.

What *is* invariant, and what everything downstream actually relies on:

1. **Content is a pure function of the dependency key** (ADR 0008): for any
   possibility state a region is in, every tile, roster, and organism
   re-derives bit-identically, on any schedule.
2. **The settled end state of a quiescent script** is schedule-free: a
   script that ends with a neutral run-out (so the terminal window is
   freshly loaded, with no in-transit convergence history) and a stop,
   settled to a fixed point, hashes identically under every schedule.

## Decision

1. **The machine-checked equality.** For a fixed script and configuration
   whose tail is quiescent (neutral pressure run-out, stop, settle to fixed
   point), the settled world state hash (`tools::replay::state_hash`) is
   **bit-identical** across:
   - executor choice and worker count (Inline vs LaneExecutor at any N),
   - budget scale (¼× / 1× / 4× of every counting knob),
   - cancellation on / off,
   - retarget amortization settings,
   - frame-slicing of the script.

   `wer-scale`'s schedule-independence scenarios gate this in CI. A settled
   fixed point means: no stale layers, no in-flight or queued jobs, and a
   state hash stable across consecutive frames (which waits out budget-paced
   loading and realization).

2. **Mid-flight state is comparable only between runs of the same
   schedule.** The continuity replay's two-run bit-identity keeps running on
   identical schedules; under a threaded executor the replay asserts the
   continuity *bounds* (pinned stability, bounded deltas, seams), which must
   hold under any schedule.

3. **Budgets and tiers scale pacing and capacity, never identity.** Budget
   knobs stay count/cost-based — a wall-clock budget would make outputs
   depend on machine speed, which this ADR forbids. Resource tiers select
   radii, budgets, cache ceilings, and realization density; where a tier
   changes world-content knobs (e.g. `organisms_per_cell`), invariance is
   asserted per preset, and the shared/persisted surfaces (record bytes,
   quantized buckets, dependency hashes, steering from records) are asserted
   tier-invariant outright.

4. **Cancellation is advisory, never semantic.** A cancelled job's absence
   must be indistinguishable (except in time and counters) from its result
   arriving and being dropped as superseded. The job-id check at integration
   remains the correctness gate; the token only saves worker time.

Anything that wants to break this — wall-clock-adaptive budgets that alter
outcomes, executors that reorder *integration* rather than execution,
GPU-derived values flowing back into state (ADR 0017) — is out until a
successor ADR.

## Consequences

- Executor, budget, amortization, and tier become freely tunable
  implementation dimensions; CI catches any of them leaking into content.
- The browser runtime (Phase 7) inherits a worker pool contract that is
  already proven schedule-free; frame-rate variability in the browser cannot
  perturb world identity.
- Harness scripts that want cross-schedule hash equality must end quiescent;
  scripts that stop mid-transit measure pacing-dependent (by design) state
  and may only be compared run-to-run on one schedule.
- The `RegionMap::set_cancellation_enabled` A/B hook and the `wer --inline`
  flag exist so the equality is cheap to re-check whenever scheduling code
  changes.
