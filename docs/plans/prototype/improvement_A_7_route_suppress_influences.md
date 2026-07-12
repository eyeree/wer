# Improvement A.7 — Route and Suppress Influence Semantics

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.7](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [7](../../world-model.md#7-resolved-route-attraction-is-globally-capped-after-candidate-selection)
and [17](../../world-model.md#17-resolved-suppress-compatibility-scores-the-final-desired-state)

This plan implements the seventh item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). Two influence calculations currently
describe a different model from the one the game and its accepted decisions
claim.

First, `route_pull(usage)` caps each selected route node independently, but
`steer` combines those nodes with `1 - product(1 - w)`. Thirty-two individually
weak, overlapping nodes can therefore have almost unit total influence. The
correction keeps the existing route candidate selection and usage curve, then
normalizes the complete selected route group against one worst-case aggregate
ceiling before those anchors join the player's explicit anchors. The ceiling is
global across all selected routes and nodes, not per route, per node, or only at
the player's current position.

Second, resonance compatibility compares the local realized vector with every
anchor's literal target. That is directionally correct for Emphasize but
backward for Suppress. The correction scores the covering region's
authoritative `current` vector against the authoritative, final projected
`target` that the immediately preceding retarget pass produced from the same
effective anchor multiset. Per-domain weights come from the same A.6 canonical
anchor order, so mixed polarities, Suppress-final priority, duplicates, route
normalization, and plausibility projection are represented once rather than
being approximately reconstructed inside resonance.

This is a post-prototype corrective plan. Do not modify
[`implementation-plan.md`](implementation-plan.md), any
`docs/plans/prototype/phase-N-plan.md`, or the historical performance ledger.
Roadmap A.7 and findings 7/17 stay open until the core implementation, focused
regressions, sign-off harnesses, ADR, current documentation, and complete
native/wasm CI matrix all pass in the A.7 worktree.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. Route candidate discovery remains corridor-bounded and deterministic:
   squared distance, route content id, and node index select the nearest
   `max_nodes` candidates exactly as they do now.
2. Route normalization runs only after all routes have contributed candidates
   and the deterministic selection has been truncated. Discarded candidates
   consume no pull budget.
3. Every selected candidate first receives its raw peak strength from the
   existing monotone `route_pull(route.usage)` curve. The curve remains nonzero
   for usage zero and saturates with usage.
4. `ROUTE_PULL_CAP` becomes the ceiling for the *combined selected route
   channel*, not merely a promise about one node. The name and value may remain
   for API compatibility, but its documentation and tests must state aggregate
   semantics.
5. The aggregate is global across all selected route records and nodes. Do not
   grant each route its own cap; two overlapping routes must not stack to twice
   the advertised route ceiling.
6. Define the selected group's worst-case peak pull in a possibility domain as
   the same saturating weight used by steering,
   `1 - product(1 - clamp(strength, 0, 1))`, with occurrences folded in A.6's
   canonical raw-bit anchor order. The group peak is the maximum of that value
   over `ROUTE_ATTRACTION_MASK` domains.
7. Normalized peak pull must be `<= ROUTE_PULL_CAP` under the actual `f32`
   calculation, not only under a real-number derivation or epsilon-sized test.
8. Because `Anchor::influence(at)` is each normalized peak strength multiplied
   by a falloff factor in `[0, 1]`, the selected route group's combined pull is
   then bounded at *every* evaluation position. A cap measured only at the
   player is insufficient because a region center can be closer to several
   selected nodes.
9. If the raw group is already under the cap, return every raw strength
   bit-for-bit unchanged. One isolated route node therefore preserves the
   existing `route_pull(usage)` behavior and fixed samples below saturation.
10. If the raw group exceeds the cap, multiply every selected raw strength by
    one common nonnegative scale. This retains route usage weighting and target
    proportions except for unavoidable `f32` rounding; do not clip nearest
    nodes first or give iteration order semantic priority.
11. Find the scale with a fixed-count `f32` bisection over `[0, 1]`, testing the
    exact canonical aggregate after each trial and retaining the greatest known
    safe lower bound. Do not use `ln`, `exp`, `powf`, platform libm, or a
    data-dependent convergence loop.
12. The normalization helper may internally canonicalize trial anchors, but
    `attraction_anchors` must continue returning anchors in deterministic
    nearest-candidate order. Common scaling does not change selection or output
    ordering.
13. Explicit player/discovery anchors are not charged against the route budget.
    Normalize only the derived route slice before it is appended to the
    explicit slice; ordinary anchor composition and Suppress-final priority
    remain governed by ADR 0025.
14. Route anchors remain fast-domain-only Emphasize anchors with the existing
    corridor radius and target reconstruction. Stable topology domains remain
    unmasked and untouched.
15. Increasing usage remains monotone until the group reaches its aggregate
    ceiling and nondecreasing thereafter. Saturated overlapping routes may
    redistribute the fixed route budget or plateau; they may never exceed it.
16. Resonance compatibility no longer compares `current` with literal
    per-anchor targets. It compares the covering region's authoritative
    `current` vector with that region's authoritative final `target` vector.
17. The final target is the stored result of
    `project_plausible(steer(field + bias, effective_anchors, region_center))`
    from the runtime retarget pass. This automatically includes mixed
    Emphasize/Suppress combination, Suppress-final priority, route-derived
    anchors, bias, and plausibility constraints.
18. Compatibility uses the covering region center as its anchor-influence
    evaluation point, because that is the point at which the region-level
    target is defined. It must not combine a player-position weight with a
    center-evaluated target.
19. Build one per-domain active-influence profile from the effective anchor
    multiset. For each domain, combine all reaching occurrences, across both
    polarities, with `1 - product(1 - w)` in the A.6 canonical anchor order.
20. Fold the weighted absolute differences between `current` and final target
    in fixed possibility-domain order, divide by the fixed-order weight sum,
    and return `clamp(1 - mean_difference, 0, 1)`.
21. The active profile is a relevance weight, not a second desired-state
    solver. It must never re-reflect Suppress targets, average literal targets,
    or reproduce projection rules. The authoritative target is the only
    desired vector scored.
22. A missing covering-region authority, no anchor influence at the region
    center, or an effective preserve yields neutral compatibility `1.0`.
    Preserves deliberately reject steering and self-target, so resonance must
    not invent a suppressed/emphasized desire inside one.
23. `RegionMap::update` keeps the required ordering: retarget with the effective
    anchors, publish canonical organisms, compute resonance from the same
    effective anchors and refreshed targets, then converge.
24. Public/direct callers of `resonance_at` must pass the effective anchor
    multiset that produced the resident authoritative targets. Update tests and
    harness call sites that currently substitute a new slice only for the
    resonance read, and document this precondition.
25. The influence-profile helper and steering obtain canonical references from
    the same `world-core::anchor` implementation established by A.6. Do not add
    a caller-order accumulation in `world-runtime`.
26. Permuting an unchanged effective anchor multiset must produce bit-identical
    route-normalized strengths, region targets, compatibility, resonance
    strength, and route-node cost bands.
27. Duplicate route nodes remain selected occurrences when capacity permits,
    but the total route channel stays capped. Duplicate explicit anchors retain
    normal ADR 0025 multiset semantics outside the route budget.
28. Route selection and normalization remain pure and platform-neutral. The
    compatibility read remains pure runtime state inspection; neither neutral
    crate gains filesystem, thread, socket, renderer, or platform dependencies.
29. Existing stored `RouteRecord`s remain readable without migration. The new
    normalization affects how every old or new route attracts when followed;
    it does not reinterpret stored bytes.
30. Newly recorded route nodes may intentionally receive changed `target`,
    `anchor_sig`, `cost_q`, content id, and encoded bytes because route
    attraction and Suppress compatibility now satisfy their advertised
    semantics. Existing stored records are never recomputed in place.
31. No permanent base-world generator, layer dependency hash, feature identity,
    record schema, or codec layout changes. Keep `WORLD_ALGORITHM_VERSION == 2`,
    every layer `algorithm_revision == 0`, and `RECORD_FORMAT_VERSION == 1`.
32. Existing generator and record-wire goldens are not re-blessed. A new
    route-normalization parity/golden fixture is additive; only an explicitly
    named route-steering presentation fixture may receive its first value.
33. Current documentation, the new ADR, tests, harness output, and this plan
    must agree on one global route ceiling and authoritative-final-target
    compatibility before A.7 is marked completed.

## 2. Scope boundaries

### 2.1 In scope

- Aggregate worst-case normalization of all selected route-derived anchors.
- A deterministic, non-transcendental normalization algorithm.
- Shared canonical per-domain anchor influence weights.
- Resonance compatibility against the covering region's authoritative final
  target.
- Exact permutation, dense-overlap, mixed-polarity, preserve, and route-record
  regressions.
- Route-attraction native/wasm compile parity with an additive fixed sample.
- A new accepted ADR, its index entry, and completion/current-model edits in
  `world-model.md`.

### 2.2 Explicitly out of scope

- Replacing derived route anchors with a separate field, one collapsed anchor,
  or a second steering algebra.
- Changing route candidate spatial indexing, the default candidate count,
  corridor radius, fast-domain mask, route graph, or traversal detection.
- Changing route node sampling/interpolation or whether nodes store target
  rather than visible current state (findings 22 and 23).
- Making the route cap per route, per route segment, per target cluster, or
  player-position-only.
- Changing explicit player-anchor strength or capping the complete
  explicit-plus-route anchor set.
- Replacing the Emphasize-first/Suppress-final steering equations established
  by ADR 0025.
- Adding an independent Suppress reflection to resonance; the authoritative
  final target already owns that semantic.
- Changing the resonance density, diversity, distance, occlusion, node
  selection, fixed 64-node ceiling, or multiplicative travel gate.
- Correcting transition-mode direction (A.12/finding 8), ecological richness,
  or capture locality.
- Claiming live resonance is cross-platform portable. It still reads
  presentation-grade float-derived world state; only route attraction built
  from quantized records gets a new parity probe.
- Executing wasm exports in a wasm engine in CI; A.8/finding 9 owns that gap.
- Changing `RouteNode`, `RouteRecord`, `SessionSnapshot`, atlas envelopes,
  content-id fold order, merge laws, or codec migrations.
- Editing accepted ADRs in place, historical prototype/phase plans, the
  performance baseline, or unrelated roadmap findings.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `world-core::route::route_pull` documentation | Describes `ROUTE_PULL_CAP` as one route anchor's ceiling. | Distinguish raw per-candidate usage pull from the complete selected route channel's aggregate ceiling. |
| `world-core::route::attraction_anchors` | Selects up to `max_nodes`, then returns every node at full raw pull. | Normalize the selected group after truncation against one worst-case canonical aggregate cap. |
| Route soft-attraction tests | Check each anchor's strength and a loose output movement bound. | Construct maximally overlapping multi-route nodes and prove actual combined pull never exceeds the advertised cap. |
| `world-runtime::stream::anchor_compatibility` | Accumulates weighted distance from `current` to every literal anchor target. | Score `current` against the authoritative projected target, weighted by canonical active domains. |
| Suppress anchors | A world near the trait being suppressed receives high compatibility. | A world near the actual Suppress-final desired state receives high compatibility. |
| Mixed anchor compatibility | Reimplements a per-anchor approximation that omits final polarity priority and projection. | Read the already-combined authoritative target once. |
| Compatibility reduction order | Iterates the caller slice and accumulates `f32` weights/differences in that order. | Canonicalize anchor influence products in core and fold only eight domains in fixed order in runtime. |
| Direct resonance tests/harness | Some call `resonance_at` with anchors that did not produce resident targets. | Update the map/retarget with the same effective slice before reading resonance and document the API contract. |
| ADR 0015 | Calls per-anchor strength soft without constraining aggregate stacking. | Refine softness as a global selected-route-channel invariant. |
| ADR 0012 | Defines the resonance gate but not polarity-correct compatibility. | Refine compatibility to score the actual authoritative desired state. |

## 4. Required design

### 4.1 Canonical influence profiles in `world-core`

Extend `world-core/src/anchor.rs` around A.6's private
`canonical_anchors` helper. Add a pure helper named approximately:

```text
anchor_influence_profile(anchors, at) -> [f32; POSSIBILITY_DIMS]
```

It canonicalizes the complete anchor multiset once. For every possibility
domain in fixed index order, it starts `keep = 1.0`, walks canonical
occurrences, and for every anchor whose mask includes the domain multiplies
`keep *= 1.0 - anchor.influence(at)`. The result is
`clamp(1.0 - keep, 0.0, 1.0)`.

Both polarities contribute to this profile. It answers only “how strongly is
this domain actively addressed here?” and deliberately does not choose a
direction or target. Duplicate occurrences remain multiplicative. Source and
unmasked target metadata remain absent through the A.6 canonical projection.

Expose this helper from `world-core` for `world-runtime`; keep its name focused
on anchor influence rather than resonance. Add exact tests showing:

- forward, reverse, and adversarial permutations have identical profile bits;
- disjoint masks affect only their selected domains;
- duplicate occurrences strengthen a non-saturated profile;
- a zero-radius, out-of-range, zero-strength, or zero-mask anchor contributes
  zero; and
- both Emphasize and Suppress contribute equal relevance for equal geometry and
  strength, even though `steer` gives them different desired directions.

For route peak normalization, add a crate-private sibling that evaluates peak
strength rather than spatial falloff, approximately:

```text
anchor_peak_profile(anchors) -> [f32; POSSIBILITY_DIMS]
```

It uses the same canonical occurrence order and saturating product, replacing
`anchor.influence(at)` with `anchor.strength.clamp(0.0, 1.0)`. Keeping both
profiles next to `steer` prevents route code from reproducing the canonical
ordering or product arithmetic. It need not become public outside
`world-core`.

### 4.2 Global route peak and normalization

Keep the current candidate tuple and its total sort:

```text
(distance_squared_bits, route_id, node_index, node, usage)
```

Collect candidates across every supplied route, sort, and truncate to
`max_nodes` before building raw anchors. Preserve the returned nearest-first
order.

For the resulting raw selected anchors `A`, define:

```text
peak(A) = max(anchor_peak_profile(A)[domain]
              for domain in ROUTE_ATTRACTION_MASK)
```

All current route anchors share the same mask, so the values are equal, but the
max-over-mask definition remains correct if a later accepted decision narrows a
derived route anchor's mask.

If `peak(A) <= ROUTE_PULL_CAP`, return `A` unchanged. Otherwise retain a copy of
the raw strengths and find one scale:

```text
safe = 0.0_f32
unsafe = 1.0_f32
repeat exactly 32 times:
    mid = safe + (unsafe - safe) * 0.5
    trial[i].strength = raw[i] * mid
    if peak(trial) <= ROUTE_PULL_CAP:
        safe = mid
    else:
        unsafe = mid
final[i].strength = raw[i] * safe
```

Thirty-two is an explicit contract-sized fixed bound, not a convergence test.
If `mid` rounds to an endpoint, the remaining iterations are harmless and
still deterministic. Re-evaluate `peak(final)` with the exact helper in a
debug assertion and in tests. If implementation details show that multiplying
the raw strengths after the last trial can differ from the tested trial, keep
the safe trial vector itself rather than reconstructing it.

The fixed lower bound guarantees the actual computed peak is never above the
cap. The one common scale preserves the raw usage ratios; a per-anchor clamp or
sequential remaining-budget allocation is forbidden. The algorithm uses only
fixed-order comparisons and IEEE `f32` add/subtract/multiply plus the existing
integer canonical sort. It must not use logarithms or exponentials to invert
the product.

The worst-case proof is part of the API documentation:

```text
0 <= influence_i(at) <= normalized_peak_strength_i

therefore

1 - product(1 - influence_i(at))
    <= 1 - product(1 - normalized_peak_strength_i)
    <= ROUTE_PULL_CAP
```

Tests must exercise the implementation's exact arithmetic at co-located node
centers as the attainable worst case, as well as many off-center points. Do not
settle for this proof in prose without an end-to-end `steer` assertion.

### 4.3 Route usage and composition semantics

`route_pull(usage)` remains the raw candidate curve:

```text
raw(u) = ROUTE_PULL_CAP * (0.35 + 0.65 * u / (u + 4))
```

Update its docs to say `raw(u)` is a relative contribution before group
normalization. For a singleton it is the final peak strength. With overlapping
selected nodes, the group may hit the cap even at usage zero; further usage can
change relative shares or plateau but cannot increase the aggregate beyond the
ceiling.

The following separations are contractual:

- selection/truncation decides which candidates exist;
- raw usage pull decides their relative strength;
- group normalization enforces route softness;
- A.6 canonical steering combines normalized route anchors with explicit
  anchors; and
- projection plus resonance/travel gating remains unchanged.

Do not include explicit anchors while computing the route scale. The product
contract is “routes as a group cannot force,” not “all player steering is
limited to 0.35.”

### 4.4 Authoritative final-target compatibility

Replace `RegionMap::anchor_compatibility`'s per-anchor literal-target loop. At
the resonance player position:

1. derive the covering `RegionCoord`;
2. if no authoritative region exists, return `1.0`;
3. if the coordinate has an effective preserve contribution, return `1.0`;
4. derive the region center, the same point used by `target_for`;
5. call `anchor_influence_profile(effective_anchors, center)`;
6. in possibility-domain index order, accumulate
   `weight * abs(region.current[domain] - region.target[domain])` and the weight;
7. if the weight sum is zero, return `1.0`; otherwise return
   `(1.0 - diff_sum / weight_sum).clamp(0.0, 1.0)`.

Equivalently, for center-evaluated canonical domain weights `q_d`,
authoritative current `c`, and authoritative final target `t`,

```text
compatibility = 1 - (sum_d q_d * abs(c_d - t_d)) / sum_d q_d
```

with the zero-denominator cases handled neutrally above. Both sums execute in
possibility-domain index order.

This definition intentionally reads the stored `region.target` rather than
calling `steer` again. The target already includes:

- the real unsteered field sample and player bias;
- the exact selected, normalized route anchors;
- A.6 canonical multiset arithmetic;
- Emphasize-first/Suppress-final priority; and
- `project_plausible`'s fixed constraint order.

Recomputing a “desired Suppress target” inside resonance would risk using the
wrong base, omitting bias/projection, or diverging from target refresh. The
target is authoritative; the profile only says which domains make its
difference relevant to active steering.

An ordinary stable near region is not treated like a preserve. Its `current`
may be pinned while its target differs, and that difference should reduce
compatibility because the local reality is not yet the desired steered one.
Only a preserve self-targets by explicit policy and therefore returns neutral.

### 4.5 Evaluation point, freshness, and public API contract

Steering is region-level and evaluates at region centers. Compatibility must
therefore use center-evaluated influence weights with the center-evaluated
authoritative target even though resonance nodes and distance are gathered
around the player point.

`RegionMap::update` already calls `retarget` before `resonance_at`. A steering
signature change refreshes all authorities; an unchanged signature leaves the
same target semantic while ordinary round-robin work proceeds. Preserve this
order and add a comment at the call site naming the coherence invariant.

Document `resonance_at(player, effective_anchors)` as requiring the same
effective anchor multiset used to produce current authoritative targets. This
is already true in production `update`; it is not true in a few tests and the
anchor harness that install a new slice only for the read. Change those call
sites to update/retarget with the slice first. Do not silently calculate weights
from an unrelated slice.

If a future public API needs arbitrary hypothetical resonance, it must accept
the field/bias and calculate a hypothetical final target explicitly. A.7 does
not add that second API.

### 4.6 Exact order and mixed-polarity behavior

The per-domain influence product uses A.6 canonical anchor references. The
runtime then folds exactly eight domain weights in fixed index order. No
anchor-slice loop remains in runtime compatibility, so a caller permutation
cannot change `f32` addition order.

For mixed polarity, compatibility is neither an average of Emphasize and
Suppress literal targets nor an average of independent reflected targets. It is
agreement with the one final target after Suppress has blended last. A test
must use values where Emphasize-first/Suppress-final differs from the reverse
or a simultaneous solve, and must prove compatibility follows the stored final
target.

For a Suppress-only case, build a real target whose final suppressed desire is
far from the literal captured target. With `current` at the final desired value,
compatibility must be high; with `current` at the literal suppressed value
while the authoritative target remains the final desired value, compatibility
must be lower. This directly fails the old formula.

### 4.7 Recording, persistence, and replay consequences

Native `World::update` already uses one effective explicit-plus-derived slice
for target refresh, resonance, and `RouteRecorder::observe` after A.6. Keep that
coherence. With A.7:

```text
selected route records
    -> raw derived anchors
    -> global normalized route group
    -> explicit + normalized effective multiset
    -> authoritative final targets
    -> final-target compatibility and resonance cost
    -> RouteNode { signature, cost_q, anchor_sig }
```

No record field changes. Existing records keep their stored target, cost,
signature, id, and encoded body. Following an existing record uses the new
bounded interpretation because attraction is derived at runtime.

A new recording can differ from an old-build recording of the same physical
journey: normalized route anchors can change target and `anchor_sig`, while
polarity-correct compatibility can change `cost_q`. Those are truthful content
changes and naturally produce a different `RouteRecord::content_id`; do not
rewrite old records or add a migration.

The save/load/settle invariant remains self-consistency under one build and one
record set. Session snapshots do not persist derived route anchors or resonance
graphs. After load, the same records deterministically reconstruct the same
normalized effective slice and final-target compatibility.

### 4.8 Native/wasm parity and golden boundaries

Add a fixed `route_attraction_sample()` to `platform-web`. Construct at least
two quantized `RouteRecord`s with multiple co-located/overlapping nodes,
different usage counts, and more raw aggregate pull than the cap. Select a
strict subset using a fixed `max_nodes`, call `attraction_anchors`, and fold:

- selected anchor count;
- returned anchor strength bits in returned selection order; and
- the `f32` bits of a fixed `steer`/`project_plausible` result.

Expose the sample through `wasm_bindgen`, pin it in the native platform-web
parity test, and compile the identical code for wasm. Also add an additive
route-aggregate fixture in `world-core/tests/determinism.rs`, or share the exact
sample construction if that can be done without adding test-only product API.
The fixture is a route-steering presentation/interop contract, not a permanent
base-world identity.

Do not export live `anchor_compatibility` or resonance: it reads authoritative
float-derived runtime/ecology state and remains presentation-grade under ADRs
0010/0012/0024. Exact permutation tests are required on native runtime state,
but this item does not broaden cross-platform resonance claims.

Existing origin, terrain, possibility-field, geology, drainage, genome,
food-web, steering, canonical-anchor-signature, record-codec, and shared-anchor
goldens must remain unchanged. A diff in any of those values is an unintended
scope expansion and must not be re-blessed.

## 5. Verification matrix

### 5.1 Pure route normalization tests

Extend `world-core/src/route.rs` tests with these cases:

1. An empty candidate set and `max_nodes == 0` return empty without division or
   bisection.
2. One in-range node returns exactly `route_pull(usage)` bits and remains below
   the cap.
3. A sparse multi-node group whose raw canonical peak is below the cap remains
   bit-for-bit unchanged.
4. Thirty-two co-located, same-target, maximum-usage nodes have raw peak near
   one but normalized peak `<= ROUTE_PULL_CAP` exactly.
5. Split those nodes across at least two route records and prove the same one
   global cap; a per-route normalization would fail this test.
6. Evaluate the returned anchors at every node center, the player, intermediate
   corridor points, and the corridor rim; combined route pull never exceeds the
   worst-case cap and is zero outside support.
7. Reverse/permuted route iterators produce the same selected anchors and
   strength bits because route id/node selection and canonical normalization
   are deterministic.
8. A dense selection's raw strengths retain their usage ordering after common
   scaling; no candidate is privileged by nearest-first output order.
9. Increasing usage on a singleton increases pull; increasing usage in a
   saturated group never decreases the group's aggregate pull below its prior
   value or exceeds the cap. Individual share changes are checked separately
   from aggregate saturation.
10. The returned anchors retain `ROUTE_ATTRACTION_MASK`, Emphasize polarity,
    node positions/targets, corridor radius, and metadata; only strength may be
    normalized.
11. Stable topology domains remain exactly unchanged under a maximally dense
    normalized group.
12. Fixed bisection returns the identical vector on repeated runs, terminates
    after its fixed count, and its retained final trial is demonstrably safe.

For end-to-end weight assertions, use a base of zero and co-located route
targets of one in one fast domain so the `steer` output in that domain directly
equals the saturating group pull. Do not infer aggregate softness only from the
largest individual `Anchor::strength`.

### 5.2 Core canonical influence-profile tests

In `world-core/src/anchor.rs`, add a multi-anchor fixture with both polarities,
overlapping masks, duplicate occurrences, unequal falloff, and inert metadata.
Assert exact profile bits over all fixture permutations or a complete
deterministic permutation set. Verify zero influence cases and exact equality
between equal-geometry Emphasize/Suppress relevance.

Keep the existing 720-permutation steering suite green. The new helper must not
change `steer`; it factors the same canonical order for another consumer.

### 5.3 Runtime Suppress and final-target tests

Add focused tests in `world-runtime/src/stream.rs`'s private test module or
`crates/world-runtime/tests/streaming.rs` where public setup is sufficient:

1. Settle/update a map with canonical organisms and an Emphasize anchor using
   the same slice for retarget and resonance; a current vector near final target
   scores above one far from final target.
2. Use a Suppress anchor whose literal target and computed final target are
   clearly separated. After placing `current` at the final target, require high
   compatibility; compare with a current at the literal suppressed target and
   require lower compatibility.
3. Use overlapping Emphasize and Suppress anchors where final Suppress priority
   is distinguishable. Require compatibility to equal the stored final target
   definition, not either literal-target average.
4. Enumerate permutations of a multi-anchor effective multiset and require
   exact `to_bits()` equality for the influence profile, target dimensions,
   `anchor_compatibility`, and `resonance.strength`.
5. Include duplicate anchors and prove multiplicity changes the profile/target
   coherently while permutation does not.
6. An anchor that reaches the player but not the covering region center is
   inactive for compatibility, demonstrating the specified evaluation point.
7. No anchors, zero mask, zero/out-of-range influence, and missing covering
   authority return neutral compatibility `1.0`.
8. An effective preserve returns neutral compatibility and remains pinned with
   `target == current` even under a strong Suppress anchor.
9. An ordinary near-stable region with `target != current` is not mistaken for
   a preserve; its active-domain difference affects compatibility.
10. The full `RegionMap::update` path computes the same compatibility from the
    effective slice after retarget and feeds the expected bounded resonance
    strength into convergence.

Tests may use a small private pure function for the fixed-domain weighted
difference, but they must include at least one real update/retarget integration
case. A fabricated `target`-only unit test is not sufficient.

### 5.4 Route recording and tier invariance

Extend the native A.6 effective-route recording regression or add a neighboring
test with enough overlapping nodes to trigger normalization and at least one
Suppress explicit anchor. Run the real `World::update` and assert:

- the map, resonance read, and recorder receive one normalized effective slice;
- `RouteNode::anchor_sig` equals the canonical signature of explicit plus
  normalized derived anchors, not raw derived strengths;
- recorded `cost_q` is derived from final-target compatibility through the
  canonical `FrameStats::resonance_strength`;
- permuting input routes/explicit anchors does not change target bits,
  compatibility bits, `cost_q`, anchor signature, route content id, or encoded
  route bytes; and
- the route group alone never exceeds its cap despite the uncapped explicit
  anchor remaining semantically independent.

Keep A.5's Low/Mid/High actual route-record byte gate green. Strength
normalization and final-target compatibility must be resource-tier invariant
once authoritative prerequisites match.

### 5.5 Harness gates

Strengthen `wer-vault`'s route scenario. Its report must include a dense
multi-route or duplicate-node worst-case and fail if total selected route pull
exceeds the cap. Keep corridor exclusion, stable-domain exclusion, singleton
usage monotonicity, and traversal debounce checks.

Strengthen `wer-anchor`'s resonance scenario with a polarity-correct Suppress
case. The map must be retargeted with the same anchor slice before reading
resonance. Report that compatibility with the actual final suppressed target
is greater than compatibility with the literal suppressed trait. Retain the
existing stationary/barren, density, Emphasize compatibility, and canonical
multiset reports.

`wer-scale` must remain green across executor, budget, cancellation,
amortization, and Low/Mid/High tiers. If its route probe summary changes because
the corrected semantics generate different truthful bytes, update only
non-golden explanatory output; equality across schedules/tiers remains exact.

### 5.6 Regression expectations

- Dense route fields bend rather than nearly replace a target.
- One isolated route retains its former raw usage curve.
- Multiple routes share one bounded channel.
- Player anchors remain capable of stronger intentional steering.
- Suppress-compatible local reality increases rather than decreases resonance.
- Mixed polarity reads the actual Suppress-final projected target.
- Empty/barren resonance remains zero because density still dominates, even
  though neutral compatibility is one.
- Stationary convergence remains exactly zero under ADR 0006/0012.
- Preserves remain self-targeted and unaffected by anchors/routes.
- Canonical permutation and tier invariance remain exact.
- Existing route records decode and merge without migration.
- Existing generator identities, record codec bytes, version constants, and
  layer declarations do not change.

## 6. Exact file set

| File | Planned change |
|---|---|
| `crates/world-core/src/anchor.rs` | Add canonical spatial and peak per-domain influence profiles around the A.6 helper; add polarity, mask, duplicate, zero-influence, and exact permutation tests. |
| `crates/world-core/src/route.rs` | Re-document raw route pull versus aggregate cap; normalize all selected route anchors with fixed safe bisection after truncation; add singleton, dense multi-route worst-case, order, usage, corridor, and stable-domain tests. |
| `crates/world-core/src/lib.rs` | Re-export the public spatial influence-profile helper for neutral runtime use without changing existing API names. |
| `crates/world-core/tests/determinism.rs` | Add an additive fixed route-attraction normalization fixture; do not change existing generator, steering, canonical-signature, content-id, or wire fixtures. |
| `crates/world-runtime/src/stream.rs` | Replace literal-target compatibility with canonical active-domain weighting against covering-region authoritative final target; specify center evaluation, preserve/missing/no-active semantics, same-effective-slice contract, and update-order coherence; add private focused tests where state control is required. |
| `crates/world-runtime/tests/streaming.rs` | Add public/full-update integration coverage for mixed polarity, exact permutation, ordinary pinned versus preserved behavior, and resonance/convergence coherence if it is clearer outside the private module. |
| `crates/platform-native/src/main.rs` | Extend the real effective-route recording test for normalized derived strengths, Suppress compatibility, route signature/cost/id/byte permutation invariance; production flow should need at most comments because A.6 already keeps one effective vector alive. |
| `crates/platform-web/src/lib.rs` | Add/export/pin a quantized multi-route aggregate-normalization parity sample, compiled for wasm and golden-tested natively. |
| `crates/tools/src/anchor.rs` | Retarget with the same slice before compatibility reads; add a visible final-target Suppress compatibility harness case. |
| `crates/tools/src/vault.rs` | Strengthen the route sign-off scenario with a global dense-overlap aggregate-cap assertion and preserve singleton monotonicity. |
| `crates/tools/tests/route.rs` | Tighten end-to-end persisted-route softness to assert the real aggregate ceiling rather than a loose per-domain movement bound. |
| `crates/tools/src/scale.rs` | Only if needed, expose/assert exact cross-tier normalized-route compatibility fields in the existing A.5 probe; do not weaken existing byte equality. |
| `docs/adr/0026-route-attraction-is-globally-bounded.md` | Record the global route-channel cap, fixed normalization, authoritative-final-target compatibility, evaluation/freshness semantics, determinism, and persistence consequences. Renumber if main gains the next ADR before implementation. |
| `docs/adr/README.md` | Index the new accepted ADR without rewriting prior records. |
| `docs/world-model.md` | Update anchor/resonance/route equations and verification text; resolve findings 7/17 and mark roadmap A.7 completed only after every gate passes. |
| this plan | Change status to `Completed` only after implementation, documentation, and all validation gates pass. |

If compilation reveals additional direct `resonance_at` callers that pass a
hypothetical/mismatched anchor slice, include only the mechanical update needed
to satisfy the documented same-effective-slice contract and add the path to
this table before commit. Do not expand into unrelated resonance or route
cleanup.

## 7. ADR and versioning decision

A new ADR is required. ADR 0015 correctly chooses derived anchors and a
saturating usage curve, but its per-anchor cap does not establish the promised
softness after anchor composition. ADR 0012 correctly makes resonance multiply
travel, but does not define polarity-aware compatibility. ADR 0025 now supplies
the canonical multiset and explicit Suppress-final target needed to fix both
without inventing new algebra.

Create ADR 0026, currently the next free number, titled approximately “Route
attraction is globally bounded and resonance scores final desire.” It should:

- refine ADR 0015 by defining `ROUTE_PULL_CAP` over the complete selected
  route-derived group across every route/node;
- retain deterministic nearest-first candidate selection and the raw monotone
  usage curve;
- require normalization after selection with one common scale and a fixed safe
  bisection over the exact canonical saturating product;
- state the worst-case/global proof from peak strengths to every spatial
  evaluation point;
- prohibit platform-sensitive transcendental inversion and per-route caps;
- retain derived fast-domain Emphasize anchors and ordinary composition with
  uncapped explicit anchors;
- refine ADR 0012's compatibility term as agreement between authoritative
  current and authoritative final projected target over canonically weighted
  active domains;
- specify covering-region center evaluation and neutral missing/no-active/
  preserve semantics;
- require the same effective slice to produce target, resonance, and route-node
  summaries;
- retain ADR 0025's canonical multiset, duplicate, and Suppress-final rules;
- explain that live resonance remains presentation-grade while quantized route
  normalization is an additive native/wasm parity surface; and
- state persistence/version consequences for old records and newly truthful
  route ids.

This ADR refines rather than reverses ADRs 0012, 0015, and 0025. Keep their
index status Accepted and do not edit their historical text. If concurrent work
claims 0026, use the next free number consistently.

No version bump or record migration is warranted:

- `WORLD_ALGORITHM_VERSION` identifies permanent generated base-world output;
  A.7 corrects transient player/record-derived steering and the presentation-
  grade resonance gate without changing the base field or any generator;
- no layer algorithm or dependency-hash definition changes, so every
  `algorithm_revision` remains zero;
- `RouteNode` and `RouteRecord` fields, field order, serde/postcard encoding,
  and content-id fold order remain unchanged, so `RECORD_FORMAT_VERSION`
  remains one;
- existing records are immutable historical observations and are not
  recomputed; and
- newly recorded targets, costs, signatures, ids, and bytes may differ because
  their content truthfully differs under the corrected runtime semantics.

The new route-attraction fixed sample is additive. Existing base-world,
steering, canonical-signature, content-id, and record-byte fixtures must not be
changed. If any existing fixture moves, stop and diagnose scope rather than
casually re-blessing it.

## 8. Implementation sequence

1. Add canonical spatial/peak influence profiles in `anchor.rs` and exact
   permutation/mask/polarity tests without changing `steer`.
2. Refactor route docs to distinguish raw usage pull from aggregate ceiling;
   normalize the selected derived group with fixed safe bisection and add pure
   dense-overlap/global-route tests.
3. Replace runtime literal-target compatibility with the authoritative
   current/final-target weighted definition; add center, missing, no-active,
   preserve, ordinary pinned, Suppress, mixed-polarity, and permutation tests.
4. Repair direct resonance test/harness callers so the same effective slice
   retargets and is then scored; keep production update ordering explicit.
5. Strengthen real native recording and route integration tests for normalized
   signatures, polarity-correct cost, ids/bytes, and tier/permutation equality.
6. Add the fixed platform-web route-attraction parity export/native golden and
   the additive core determinism fixture.
7. Strengthen `wer-anchor` and `wer-vault` visible sign-off scenarios.
8. Add ADR 0026/index and update current `world-model.md`; leave roadmap/finding
   status open while any test or documentation gate is outstanding.
9. Run focused tests, all harnesses, complete native CI commands, and wasm
   checks. Inspect version/golden/historical-plan diffs explicitly.
10. Only after all gates pass, mark this plan `Completed`, mark roadmap A.7
    completed with a link/summary, label findings 7 and 17 resolved with the
    implementation evidence, and rerun documentation/diff checks.

## 9. Validation commands

Run from the A.7 worktree. If `cargo` is absent, first run
`source "$HOME/.cargo/env"`.

### 9.1 Focused implementation checks

```sh
cargo test -p world-core anchor
cargo test -p world-core route
cargo test -p world-core --test determinism
cargo test -p world-runtime anchor_compatibility
cargo test -p world-runtime suppress
cargo test -p world-runtime --test streaming
cargo test -p platform-native route_recording
cargo test -p platform-web
cargo test -p tools --test route
```

Test-name filters may be adjusted to the final names, but every new pure,
runtime, native, web, and route-integration regression must run explicitly at
least once before the full workspace gates.

### 9.2 Sign-off harnesses

```sh
cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale
```

`wer-anchor` must visibly report polarity-correct final-target compatibility.
`wer-vault` must visibly report the global dense-route cap. `wer-scale` must
retain exact Low/Mid/High route bytes and all schedule/capacity gates.

### 9.3 Full native CI-equivalent gates

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo test --workspace
```

### 9.4 Browser portability

```sh
RUSTFLAGS="-D warnings" cargo check \
  -p world-core -p world-runtime -p platform-web \
  --target wasm32-unknown-unknown
```

This compiles but does not execute the parity export in wasm. Preserve that
limitation in `world-model.md`; A.8 owns executable wasm probes.

### 9.5 Scope, version, and diff audit

```sh
git diff --check
git status --short
git diff --name-only
git diff -- \
  docs/plans/prototype/implementation-plan.md \
  docs/plans/prototype/phase-1-plan.md \
  docs/plans/prototype/phase-2-plan.md \
  docs/plans/prototype/phase-3-plan.md \
  docs/plans/prototype/phase-4-plan.md \
  docs/plans/prototype/phase-5-plan.md \
  docs/plans/prototype/phase-6-plan.md \
  docs/plans/prototype/3d-phase-1-plan.md \
  docs/perf-baseline.md
git diff -G 'WORLD_ALGORITHM_VERSION|RECORD_FORMAT_VERSION|algorithm_revision'
git diff -- crates/world-core/tests/determinism.rs crates/platform-web/src/lib.rs
```

The historical-plan diff must be empty. Version declarations must be unchanged.
The determinism/web diff may contain only the additive route-attraction sample;
all pre-existing constants and record/generator fixtures remain untouched.

## 10. Documentation completion edits

Only after implementation and every validation command pass:

1. Change this plan's status from `Planned` to `Completed` and update its two
   “Findings addressed” fragment links to the final `resolved-...` headings.
2. Change roadmap A.7 in `docs/world-model.md` to “Completed,” link this plan,
   cite findings 7/17, and summarize the global selected-route cap plus
   authoritative-final-target compatibility and their exact tests.
3. Rename finding 7 to “Resolved: route attraction is globally capped after
   candidate selection,” add a status/link block, retain the original failure
   analysis, and describe common-scale fixed bisection, worst-case coverage,
   multi-route tests, and parity sample.
4. Rename finding 17 to “Resolved: Suppress compatibility scores the final
   desired state,” add a status/link block, retain the original failure
   analysis, and describe final target, center-evaluated canonical domain
   weights, preserve/no-active semantics, and exact permutation tests.
5. Update Section 2.4's anchor equations only as needed to cross-reference the
   canonical influence profile; do not change A.6's steering equations.
6. Update Section 2.5/3.6 so compatibility is agreement between authoritative
   current and final projected target over active domains, not resemblance to
   every literal anchor target.
7. Update Section 3.7 so the route equation distinguishes raw candidate pull
   from one globally normalized selected-route peak capped at 0.35 across all
   nodes/routes and all evaluation positions.
8. Update Section 3.28's verification list with dense multi-route cap,
   Suppress-final compatibility, exact anchor permutation, native recording,
   tier equality, and additive route parity coverage.
9. Add ADR 0026 to `docs/adr/README.md` with Accepted status and keep prior ADR
   history immutable.
10. Preserve the explicit caveats that live resonance is presentation-grade,
    parity exports are not executed in wasm CI, routes still store aspirational
    target rather than visible current, and route scans/traversal remain open
    findings.

Documentation must not say a route can never dominate merely because each node
is weak. It must name the combined bound and where normalization occurs.
Likewise, it must not say compatibility reflects literal anchor targets; it
reflects the actual authoritative target the runtime will converge toward.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Cap is enforced only at the player while a region center sees stronger overlap | Normalize canonical peak strengths independent of position; prove falloff can only reduce them and test co-located worst cases. |
| Each route receives its own 0.35 budget | Collect/select across all routes first and run one normalization; split the worst-case fixture across multiple route ids. |
| Closed-form inversion varies across native/wasm libm | Use fixed-count `f32` bisection and the exact canonical product; prohibit transcendental math. |
| Bisection reconstruction rounds above the safe trial | Retain the exact safe trial vector or recheck final peak; exact `<=` tests, not epsilon. |
| Normalization changes candidate order or favors nearest nodes | Apply one common scale after selection and retain original output order; test relative raw usage ordering. |
| Dense-route saturation makes a strict usage test fail | Require strict monotonicity for unsaturated/singleton cases and nondecreasing bounded aggregate behavior after saturation. |
| Runtime rebuilds desired Suppress math differently from steering | Read the authoritative final target; use core only for domain relevance weights. |
| Compatibility uses player weights with a center target | Specify and test region-center evaluation for both target and weights. |
| A caller passes anchors unrelated to stored targets | Document same-effective-slice precondition, fix all repository call sites, and use real retarget/update integration tests. |
| New profile accumulation regresses A.6 order independence | Reuse `canonical_anchors`; assert exact profile/compatibility/resonance bits over permutations. |
| Preserves appear maximally compatible for the wrong reason | Make preserve neutrality explicit: no active steering desire is defined inside an override. |
| Ordinary near stability is accidentally treated as preserve | Check effective preserve ownership, not `stability == 1`; test pinned ordinary target differences. |
| Corrected costs/targets change new route ids and are mistaken for schema breakage | Document content-level change, keep field layout/fold order/version fixed, and preserve old-record decode tests. |
| A route presentation correction is used to re-bless generator goldens | Add only named route fixtures; audit pre-existing constants, versions, layer revisions, and record bytes. |
| Live resonance is overclaimed as portable | Add parity only for quantized route attraction and retain presentation-grade resonance caveat. |

## 12. One-commit worktree, merge, and push workflow

This roadmap item is developed only in the dedicated A.7 worktree and branch.
Do not begin A.8 until A.7 is committed, fast-forwarded into `main`, and pushed.

1. Confirm the primary checkout is clean/synchronized and this worktree is on
   `codex/improvement-a7-route-suppress-semantics`.
2. Keep plan and implementation changes together in this worktree. Preserve any
   unrelated user changes; do not clean/reset them destructively.
3. After every gate in Section 9 passes and completion docs are updated, inspect
   the complete diff and stage only the A.7 file set.
4. Verify the staged diff has no historical-plan changes, no unrelated files,
   no version changes, and no modified existing golden constants.
5. Create exactly one A.7 commit, for example:

   ```sh
   git add <reviewed A.7 files>
   git diff --cached --check
   git diff --cached --stat
   git commit -m "Bound route pull and fix suppress compatibility"
   ```

6. If review finds an issue before merge, amend/rebuild this one commit rather
   than adding fixup commits.
7. Return to the primary checkout, ensure `main` has not diverged, and merge
   with `git merge --ff-only codex/improvement-a7-route-suppress-semantics`.
   If main advanced, rebase safely in the item worktree, rerun affected/full
   gates, and still produce one final commit.
8. Push `main` to `origin`, verify local `main` and `origin/main` name the same
   commit, and only then remove the A.7 worktree/branch or start A.8.

Do not commit from the primary checkout, merge multiple item commits, squash
unreviewed unrelated work, force-push, or continue to the next roadmap item
before the push succeeds.

## 13. Definition of done

A.7 is complete only when all of these are true:

- all selected route nodes across all selected routes share one aggregate
  worst-case `ROUTE_PULL_CAP` budget after deterministic truncation;
- an exact co-located multi-route worst case proves combined steering pull is
  never above the cap at any evaluation point;
- route normalization uses one common scale, fixed safe bisection, A.6
  canonical products, and no transcendental math;
- singleton/raw usage, corridor, fast-domain, deterministic selection, and
  explicit-anchor composition semantics remain intact;
- resonance compatibility scores authoritative current against the actual
  authoritative final projected target with canonical center-evaluated active
  domain weights;
- Suppress-only and mixed-polarity tests fail the old literal-target formula
  and pass exact permutation checks;
- missing/no-active/preserve cases are neutral while ordinary pinned regions
  remain meaningful;
- real route recording signs normalized effective anchors and records the
  polarity-correct canonical resonance cost;
- tier, schedule, save/load, persistence, continuity, ledger, anchor, vault,
  scale, native CI, and wasm compile gates pass;
- ADR 0026 and its index entry land without rewriting accepted ADR history;
- `WORLD_ALGORITHM_VERSION`, all layer revisions, `RECORD_FORMAT_VERSION`,
  existing record bytes, and existing generator/steering goldens remain
  unchanged;
- `world-model.md` marks roadmap A.7 and findings 7/17 resolved with accurate
  equations, evidence, and remaining caveats;
- this plan says `Completed` only after those conditions hold; and
- the complete A.7 change is one reviewed commit, fast-forwarded to `main` and
  pushed to `origin` before work on A.8 begins.
