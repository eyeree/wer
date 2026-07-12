# 26. Route attraction is globally bounded and resonance scores final desire

Date: 2026-07-12

## Status

Accepted

Refines [ADR 0012](0012-resonance-gates-transition.md) and
[ADR 0015](0015-routes-attract-as-derived-anchors.md), and builds on
[ADR 0025](0025-anchor-steering-uses-canonical-multisets.md).

## Context

ADR 0015 capped each route-derived anchor at 0.35. Steering combines repeated
anchors with `1 - product(1 - w)`, however, so many individually weak,
overlapping route nodes could approach total influence one. Applying the cap
per route would leave the same stacking failure between route records, while
measuring only at the player would miss stronger overlap at a region center.

Resonance had a second semantic mismatch. Its compatibility term compared the
realized vector with each anchor's literal target. That direction is sensible
for Emphasize, but a Suppress anchor asks the world to move away from its
literal target. Mixed polarities, player bias, and plausibility projection make
any independent reconstruction inside resonance still less authoritative than
the final target already computed by the retarget pass.

## Decision

1. **One route budget covers the selected group.** Candidate discovery,
   squared-distance ordering, route-id/node-index tiebreaks, and `max_nodes`
   truncation remain unchanged. Only after selection, all selected occurrences
   across every route share one `ROUTE_PULL_CAP == 0.35` peak budget. Discarded
   candidates consume none of it. Explicit player/discovery anchors compose
   outside this route-only budget.

2. **Usage first assigns raw relative strengths.** Each selected candidate
   receives the existing monotone curve
   `0.35 * (0.35 + 0.65 * usage / (usage + 4))`. A singleton therefore retains
   its former strength bits. Dense groups may plateau at the aggregate cap;
   common scaling preserves their relative usage weights up to `f32` rounding.

3. **The cap uses steering's exact canonical product.** Peak pull in each
   route-masked domain is `1 - product(1 - clamp(strength, 0, 1))`, folding
   occurrences in ADR 0025 raw-bit order. The selected group's peak is the
   maximum over route-attraction domains. Duplicates remain occurrences.

4. **Normalization is deterministic and safe.** If the raw peak is already
   safe, anchors are returned bit-for-bit unchanged and in nearest-first order.
   Otherwise one common scale is found by exactly 32 `f32` bisection iterations
   over `[0, 1]`. Each trial evaluates the exact canonical product; the greatest
   known safe trial vector is retained. No logarithm, exponential, platform
   libm inversion, data-dependent termination, sequential budget allocation,
   or per-route cap is permitted.

5. **The peak bound holds everywhere.** Spatial falloff multiplies each peak
   strength by a factor in `[0, 1]`. Consequently its canonical product cannot
   exceed the peak product, so the complete selected route channel is bounded
   at every region center, node center, player position, and corridor point.
   Routes remain fast-domain-only Emphasize anchors; stable topology is still
   excluded.

6. **One canonical relevance profile is shared.** `world-core` exposes a
   per-domain profile that folds every reaching anchor occurrence, of either
   polarity, as `1 - product(1 - influence)`, using ADR 0025 order. The profile
   says only which domains are actively addressed. It does not choose a desired
   direction, reflect Suppress targets, or duplicate projection rules.

7. **Compatibility scores final desire.** For the covering region, resonance
   compares authoritative `current` with the already-refreshed authoritative
   final `target`. It weights absolute differences by the canonical relevance
   profile evaluated at that region's center, folds the eight domains in fixed
   order, and returns `clamp(1 - weighted_mean_difference, 0, 1)`. That target
   already includes field plus bias, normalized effective anchors,
   Emphasize-first/Suppress-final composition, and plausibility projection.

8. **Freshness is an API precondition.** `RegionMap::update` retargets before
   computing resonance and passes the same effective anchor slice to both.
   Direct `resonance_at` callers must likewise pass the multiset that produced
   resident targets. Target weights use the region center, never a player-point
   weight paired with a center-defined target.

9. **Neutral cases are explicit.** Missing covering authority, no active
   center influence, and an effective preserve return compatibility `1.0`.
   Preserves reject steering and self-target by policy. An ordinary near pinned
   region is not neutral merely because its stability is one: disagreement with
   an active final target remains meaningful.

10. **Persistence and portability boundaries remain.** Existing route records
    are not migrated or recomputed; following them now derives globally bounded
    anchors. New recordings may truthfully change target, cost, anchor
    signature, content id, and bytes. Quantized route normalization has an
    additive native/wasm parity fixture. Live resonance still reads
    presentation-grade state and is not promoted to a portable identity.

## Alternatives considered

- **Keep a per-node or per-route cap:** rejected because overlapping nodes or
  records still stack beyond the advertised softness.
- **Measure overlap only at the player:** rejected because authoritative
  targets and their falloff weights are evaluated at region centers.
- **Use a closed-form product inversion:** rejected because transcendental
  implementations introduce platform variation and do not test actual `f32`
  arithmetic.
- **Clip or allocate budget nearest-first:** rejected because selection order
  would become semantic and usage proportions would be distorted.
- **Collapse route nodes into one anchor:** rejected because it discards their
  spatial supports and recorded target proportions.
- **Reflect Suppress again inside resonance:** rejected because it can diverge
  from steering priority, bias, and projection. The resident final target is
  the authoritative desired state.
- **Treat all pinned regions like preserves:** rejected because ordinary near
  authority can have an active target it has not yet converged toward.

## Consequences

- Dense overlapping routes bend the target through one bounded channel instead
  of nearly replacing it. A singleton preserves the existing usage curve, and
  explicit anchors retain their independent strength.
- Suppress-compatible reality increases compatibility; mixed polarities score
  the actual Suppress-final projected target.
- Route normalization allocates and sorts small bounded vectors and performs 32
  fixed trials only when the selected raw group exceeds the cap.
- New route observations can have corrected content while old records remain
  valid immutable history under the unchanged schema and codec.
- `WORLD_ALGORITHM_VERSION` remains 2, every layer `algorithm_revision`
  remains 0, and `RECORD_FORMAT_VERSION` remains 1. Existing generator,
  steering, content-id, and record-wire goldens do not change; the route parity
  fixture is additive.
- CI still only compiles wasm parity exports; executable wasm parity remains a
  separate improvement.
