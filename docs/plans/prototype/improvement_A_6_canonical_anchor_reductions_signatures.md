# Improvement A.6 — Canonical Anchor Reductions and Signatures

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.6](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [1](../../world-model.md#1-resolved-anchor-combination-was-not-bitwise-order-independent)
and [21](../../world-model.md#21-resolved-anchor-set-signatures-did-not-describe-actual-steering-sets)

This plan implements the sixth item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). The current steering equations are
symmetric over real numbers, but their `f32` additions and multiplications run
in caller slice order. The current runtime invalidation signature also runs in
slice order, while the route-node signature tries to recover commutativity with
XOR over an incomplete, quantized per-anchor hash. Those three definitions can
therefore disagree about whether two anchor slices represent the same active
steering multiset.

The correction defines one bitwise total key over every field that can affect
steering, projects every live anchor to that key, and sorts the complete
multiset before any floating-point reduction or hash fold. Duplicate anchors
remain duplicate influences. The signature starts with cardinality and folds
every canonical occurrence in order, so duplicates cannot cancel. The runtime
retarget fingerprint consumes that same canonical signature, and native route
recording passes the effective player-plus-route anchor slice that actually
steered the just-completed map update.

This is a post-prototype corrective plan. Do not modify
[`implementation-plan.md`](implementation-plan.md), any
`docs/plans/prototype/phase-N-plan.md`, or the historical performance ledger.
Roadmap A.6 and findings 1/21 stay open until the core implementation, focused
and adversarial regressions, native effective-anchor recording gate, ADR,
current documentation, and complete native/wasm CI matrix all pass in the A.6
worktree.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. `steer(base, anchors, at)` is bitwise invariant under every permutation of
   the same anchor multiset. Equality means all eight output dimensions have
   equal `f32::to_bits()` values, not merely an epsilon comparison or equal
   possibility buckets.
2. Canonicalization is defined by a complete lexicographic key made only from
   integer IEEE encodings and enum/mask tags. It never uses float
   `partial_cmp`, locale/platform ordering, allocation addresses, source
   metadata, or caller insertion order.
3. The key contains, in one documented order, both `world_pos` `f64` bit
   patterns, the domain mask, an explicit polarity tag, the `strength` `f32`
   bits, the `falloff_radius` `f64` bits, and the `f32` bits of each masked
   target dimension in possibility-domain order.
4. Unmasked target values are normalized out of the semantic key because
   `steer` never reads them. `AnchorSource` is excluded because it is
   discovery/legibility metadata and never affects steering. Changing either
   alone must change neither steering nor its anchor signature.
5. Raw unsigned bit-pattern ordering is the contract. It need not match
   numerical float ordering; it only needs to be total and portable. It also
   provides a deterministic order for signed zero, infinities, and NaN payloads
   if malformed/session-local data reaches the API, without adding a new
   validation or normalization policy in this item.
6. Every supplied anchor occurrence remains in the canonical multiset. Do not
   deduplicate equal keys: two identical anchors contribute twice to the
   weighted mean denominator and saturating product, so their signature must
   also contain two occurrences. Zero influence at one evaluation position is
   a per-position reduction result, not permission to erase the anchor from the
   global multiset.
7. Sorting happens once per `steer` call, before the per-domain loops. All
   domains iterate the same canonical reference order. Do not independently
   sort each domain, and do not mutate or reorder the caller's slice.
8. Within each domain, preserve the existing formulas and operation order for
   one canonical slice: compute Emphasize numerator/denominator/keep product,
   compute Suppress reflected numerator/denominator/keep product, apply the
   Emphasize blend to the base first, then apply the Suppress blend to that
   intermediate value.
9. Suppress therefore has explicit final-blend priority when both polarities
   affect a domain. This item does not introduce a simultaneous solver. The
   decision and current-model equations must say so directly, and a focused
   test must distinguish this result from Emphasize-final or simultaneous
   alternatives.
10. `anchor_set_signature` becomes a canonical ordered fold, not XOR. It folds
    a domain-separating basis, the exact multiset count, and every field of
    every canonical steering key in the exact key order. Repeated equal keys
    are folded repeatedly and therefore retain exact multiplicity.
11. The signature and `steer` obtain their canonical references/keys from one
    shared implementation in `world-core`. Parallel hand-written field lists
    are forbidden: adding a future steering field must have one obvious place
    that changes both reduction order and signature coverage.
12. The runtime target-refresh signature retains raw bias bits in fixed
    possibility-domain order and incorporates the canonical anchor signature.
    Reordering an unchanged multiset must stay on the amortized refresh path;
    changing cardinality or any steering-relevant field must force the normal
    full-authority refresh.
13. A route node's `anchor_sig` describes the same effective anchor multiset
    used by the map update that produced its target and resonance cost. When
    route attraction is enabled, this includes the selected route-derived
    anchors as well as the player's explicit anchors. When it is disabled, the
    effective multiset is just the explicit anchors.
14. Route-derived anchors remain ordinary anchors after deterministic
    nearest-node selection. No special signature representation is added for
    route id, node index, or usage; the signature records the actual derived
    steering fields produced for that frame.
15. `RouteRecorder::observe` continues to accept an anchor slice, but its API
    documentation and parameter name identify it as `effective_anchors` and
    require callers to pass the exact slice used for the immediately preceding
    map update. It does not independently rediscover routes or own vault state.
16. The `RouteNode` schema and `RouteRecord::content_id` field order do not
    change. Existing persisted routes remain readable and keep their stored
    ids; newly recorded routes may intentionally receive a corrected
    `anchor_sig` and therefore a different content id from the faulty build.
17. Canonical sorting changes no permanent base-world identity, generator
    equation, layer dependency hash, feature id, or record encoding. Keep
    `WORLD_ALGORITHM_VERSION == 2`, every layer `algorithm_revision == 0`, and
    `RECORD_FORMAT_VERSION == 1`.
18. The pure key, sort, reduction, and signature code stays in `world-core`,
    uses no platform APIs, and compiles for `wasm32-unknown-unknown`. For the
    same live IEEE inputs, native and wasm must compute the same canonical
    signature and steered vector under the existing steering parity contract.
19. Add a dedicated fixed canonical-anchor parity probe rather than weakening
    an existing one. Existing record-codec bytes and permanent identity
    goldens must not be re-blessed. If canonical reduction intentionally moves
    a steering-only float golden, update only that named steering fixture with
    an explanation and the new ADR.
20. Current documentation, verification descriptions, the new ADR, and this
    plan must agree on multiset rather than set semantics, complete field
    coverage, duplicate retention, final Suppress priority, and effective route
    anchors before A.6 is marked completed.

## 2. Scope boundaries

### 2.1 In scope

- A shared semantic key for canonical anchor ordering and hashing.
- Bitwise permutation-invariant `steer` reductions.
- Explicit Emphasize-then-Suppress blend priority.
- Cardinality- and multiplicity-preserving anchor signatures.
- Runtime retarget invalidation based on the canonical signature.
- Native route recording of the complete effective anchor slice, including
  selected route-derived anchors.
- Adversarial pure, runtime, route, harness, golden, and parity regressions.
- A new accepted ADR, ADR index entry, and completion/current-model edits in
  `world-model.md`.

### 2.2 Explicitly out of scope

- Changing the Emphasize/Suppress equations to a simultaneous solve. The fixed
  polarity order is made explicit and retained.
- Changing how Suppress anchors are scored by resonance compatibility
  (A.7/finding 17).
- Capping combined route attraction or changing per-route pull
  (A.7/finding 7).
- Changing route-node sampling, overshoot interpolation, traversal detection,
  or the difference between recorded target and realized state (findings 22
  and 23).
- Deduplicating anchors. Duplicate occurrences are semantically meaningful and
  remain so in both reduction and signature.
- Quantizing live steering before reduction or changing the persistence
  boundary. Canonicalization uses exact live bits; shareable discovery/route
  fields retain ADR 0013's existing quantization.
- Adding source metadata to steering. `AnchorSource` remains non-semantic for
  steering and signatures.
- Executing wasm parity exports in CI; that broader infrastructure gap belongs
  to finding 9/A.8. This item adds and compiles a wasm export and pins its
  native twin like the existing parity surfaces.
- Changing `RouteNode`, `RouteRecord`, the postcard envelope, content-id fold
  order, CRDT merge laws, or migrating existing vault data.
- Editing accepted ADRs in place, historical prototype/phase plans, or the
  committed performance baseline.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `world-core::anchor::steer` | Accumulates `f32` numerators, denominators, and products in caller slice order for every domain. | Build one canonical reference order from the complete steering key and use it for every reduction. |
| Anchor unit test | Tries three benign orderings whose values happen to produce equal outputs. | Exercise adversarial magnitudes and many/all permutations with exact bit equality. |
| `world-runtime::stream::steering_signature` | Folds every anchor field in slice order, including all target dims, so reorder-only edits force full retarget. | Fold bias plus the shared canonical anchor signature; ignore unmasked target/source changes and preserve multiplicity. |
| `world-core::route::anchor_set_signature` | Quantizes some fields, omits radius, includes all target buckets, and XORs per-anchor hashes. | Fold count and every exact canonical steering key occurrence in sorted order. |
| Duplicate anchors | Two equal per-anchor hashes XOR to zero even though steering becomes stronger. | Retain both sorted occurrences and fold both; distinguish empty, singleton, pair, and larger multiplicities. |
| Native route recording | Updates the map with explicit + route-derived anchors, but records only `self.anchors`. | Keep the effective vector through recording and pass the same slice to `RouteRecorder::observe`. |
| Route-recorder contract | Calls its parameter `anchors` and says “active set,” leaving effective versus explicit ambiguous. | Name and document the exact immediately-preceding-update contract. |
| Parity surface | Steering output is probed, but the anchor multiset signature has no dedicated fixed parity sample. | Add a canonical-signature sample built from portable fixed inputs and expose/pin it beside existing probes. |
| ADR 0011/current model | Claims symmetric order independence but does not specify numerical canonicalization; fixed polarity priority is only implicit in equations/code. | Add a refining ADR and state the bitwise reduction order and final Suppress priority explicitly. |

## 4. Required design

### 4.1 One semantic anchor key

Define an internal value type in `world-core/src/anchor.rs`, named
approximately `AnchorSteeringKey`, with derived `Copy`, `Eq`, `Ord`,
`PartialEq`, and `PartialOrd`. It contains only integers:

```text
world_x_bits: u64
world_y_bits: u64
mask: u8
kind_tag: u8              # Emphasize = 0, Suppress = 1
strength_bits: u32
falloff_radius_bits: u64
masked_target_bits: [u32; POSSIBILITY_DIMS]
```

Build `masked_target_bits` in domain-index order. For a set mask bit, store
`anchor.target.dims[i].to_bits()`; for an unset bit, store one fixed zero
sentinel. The mask itself disambiguates an actual masked `+0.0` target from an
unmasked slot. Do not quantize, clamp, round, or numerically sort any field.

The displayed struct field order above is the lexicographic sort order and the
hash fold order. It deliberately follows the live fields that can affect
`Anchor::influence` and the per-domain steering reduction. `AnchorSource` and
unmasked target bits are omitted because the algorithm never reads them.

Raw-bit ordering is chosen over `f32::total_cmp`/`f64::total_cmp` so the
portable contract is visibly an integer lexicographic fold. Numerical ordering
has no gameplay meaning here: sorting exists solely to make the non-associative
operation sequence canonical.

### 4.2 Canonical multiset view

Add one private helper, approximately:

```text
canonical_anchors(&[Anchor]) -> Vec<(AnchorSteeringKey, &Anchor)>
```

It projects every occurrence, sorts by key, and retains all equal-key entries.
It must not call `dedup`, collect into a map/set, or use `AnchorSource` as a
tiebreak. Equal keys are steering-equivalent: if their source or unmasked
target data differs, arbitrary ordering among them cannot change reduction
bits because every value read by steering is equal. Retaining every tuple
preserves multiplicity.

Both `steer` and `anchor_set_signature` call this helper. If profiling later
shows allocation material, optimization may replace the temporary vector only
if it preserves this exact ordering and tests; A.6 should prefer one obvious,
auditable definition over parallel clever implementations. The default player
slice plus at most 32 route candidates keeps the temporary bounded in normal
runtime use.

### 4.3 Canonical floating-point reduction and polarity

Refactor `steer` to canonicalize once before iterating dimensions. Preserve the
existing per-domain loop and arithmetic expression order exactly:

```text
for key/anchor in canonical order:
    if domain is unmasked: continue
    w = anchor.influence(at)
    if w <= 0: continue
    accumulate into Emphasize or Suppress numerator/denominator/keep

value = base
if Emphasize denominator > 0:
    value += (emphasize_mean - value) * (1 - emphasize_keep)
if Suppress denominator > 0:
    value += (suppress_reflected_mean - value) * (1 - suppress_keep)
clamp value
```

Suppress reflection remains relative to the original unsteered `base_i`, as it
is today. Only its final blend starts from the Emphasize result. This is a
fixed polarity precedence, not symmetry between polarities. Anchor order is
canonical *within and across* both groups; polarity order remains semantic.

Add a small exact regression with one Emphasize and one Suppress influence
whose Emphasize-first/Suppress-final result differs clearly from the reverse
order. Assert the documented formula's result bits. This prevents a future
cleanup from silently moving the two `if` blocks or claiming a simultaneous
solve without a successor ADR.

### 4.4 Canonical, multiplicity-preserving signature

Move the implementation of `anchor_set_signature` from `route.rs` alongside
the key/helper in `anchor.rs`; retain the existing public re-export/name so
callers do not churn. Remove the XOR/splitmix per-anchor construction.

Use a domain-separated fixed basis and this fold shape:

```text
h = ANCHOR_SET_BASIS
h = mix(h, canonical.len())
for key in canonical order:
    h = mix(h, key.world_x_bits)
    h = mix(h, key.world_y_bits)
    h = mix(h, key.mask)
    h = mix(h, key.kind_tag)
    h = mix(h, key.strength_bits)
    h = mix(h, key.falloff_radius_bits)
    for target_bits in key.masked_target_bits:
        h = mix(h, target_bits)
```

The count is folded even for the empty multiset, so the empty signature is a
domain-separated nonzero hash rather than the old XOR identity `0`. Every
duplicate runs the full field fold again. The signature is a compact
fingerprint and can still collide like any `u64` hash; “exact multiplicity”
means it encodes every occurrence instead of algebraically cancelling equal
ones, not that it becomes a collision-free serialization.

Do not fold source, quantized target seeds, rounded positions, or rounded
falloff. Those describe record identity/presentation metadata, not the exact
live steering computation. Record-reconstructed anchors still produce portable
bits because ADR 0013's integer dequantization is portable.

### 4.5 Runtime target-refresh fingerprint

Replace the independent anchor field loop in
`world-runtime/src/stream.rs::steering_signature` with:

```text
runtime basis
-> each bias f32 bit pattern in domain order
-> canonical anchor_set_signature(anchors)
```

Keep the runtime-specific basis so bias+anchors cannot collide trivially with
the route-node anchor-only namespace. The anchor count and complete fields are
inside the canonical sub-fingerprint; do not maintain a second copy of its
field order in runtime.

Strengthen the amortized-retarget regression. After the first full refresh,
submit the same adversarial multiset in a different permutation under
`max_retarget_regions = 1`; require `retarget_deferred == active_regions - 1`
and unchanged target bits. Then add an exact duplicate or change falloff by one
bit and require `retarget_deferred == 0` for the full refresh. Also show that
changing only source or an unmasked target dimension does not force a refresh.

### 4.6 Effective anchors in route recording

Keep the `effective` vector in `platform-native::World::update` alive through
the recorder call:

```text
effective = explicit anchors
if tracking + attraction enabled:
    append attraction_anchors(...)
stats = map.update(..., &effective, ...)
recorder.observe(..., &effective, stats.resonance_strength)
```

This makes the three recorded summaries coherent:

```text
effective anchor multiset
    -> map target
    -> canonical resonance / transition cost
    -> RouteNode.anchor_sig
```

The recorder remains independent of the vault and route-selection policy. Its
argument becomes `effective_anchors`, and docs state that a caller which uses a
different slice for update and observe is violating the API contract.

Add a native regression that installs at least one explicit anchor and one
nearby route record, enables path tracking and route attraction, starts a
recorder, runs an update, and closes the recording. Reconstruct the expected
derived vector using the same public deterministic selector, append it to the
explicit vector, and require the node signature to equal
`anchor_set_signature(expected_effective)`. Also require inequality from the
explicit-only signature and equality when the effective vector is permuted.

Do not sign the route records, route ids, or candidate nodes that lost the
nearest/cap selection. Only derived anchors actually appended to the update are
active steering inputs.

### 4.7 Native/wasm parity and golden boundaries

Add `canonical_anchor_signature_sample()` (name may be shortened consistently)
to `platform-web`. Construct a fixed multiset from integer/record-derived
values, include both polarities, at least one exact duplicate, different masks,
and deliberately non-neutral unmasked target values. Hash or directly return
the canonical `anchor_set_signature`, expose the same function through
`wasm_bindgen`, and pin the native value in `parity_samples_match_goldens`.

The sample should also call the canonical path with a reversed/permuted slice
in its native unit test and assert the same result before comparing the fixed
constant. The wasm check compiles the identical pure code. Do not claim this
executes wasm in CI; `world-model.md` must retain that limitation until A.8.

Add a named `anchor_set_signature_golden` in
`world-core/tests/determinism.rs` using the same or another compact fixed
multiset. This is a new contract fixture, not permission to change existing
base-generation, record-codec, content-id, or wire-byte goldens.

The existing `steer_sample`, `shared_steer_sample`, and steering-only
`steering_and_projection_golden` may remain numerically unchanged if their
within-polarity inputs are already in canonical order or contain only one
anchor per group. If one moves because the old caller order differed, inspect
the exact arithmetic change and update only that steering presentation fixture,
with an ADR 0025 comment. Any change to origin/terrain/geology/drainage/genome/
food-web identities, record bytes, record ids, or generator goldens is a bug in
this implementation and must not be re-blessed.

### 4.8 Persistence and compatibility

`RouteNode::anchor_sig` remains a `u64` in the same serialized field position,
and `RouteRecord::content_id` continues folding that value in the same order.
No reader recomputes old anchor signatures, so old records retain their ids and
merge behavior. New recordings use the corrected fingerprint and may be
content-distinct from a recording made by the old implementation; that is an
intentional semantic correction, not a schema migration.

`RECORD_FORMAT_VERSION` remains 1. Do not rewrite vault records, migrate
bundles, change codec fixtures, or reinterpret a stored old signature as if it
had the new field coverage. It is an opaque historical summary of the
recording build's active-anchor logic.

## 5. Verification matrix

### 5.1 Pure key and reduction tests

Extend `world-core/src/anchor.rs` tests with an adversarial fixture of at least
six anchors containing:

- both Emphasize and Suppress polarities;
- overlapping and disjoint masks;
- repeated exact keys;
- close and widely different `f32` strengths/targets chosen so naive order
  changes one or more accumulator ULPs;
- different positive/negative positions and radii; and
- different source/unmasked target metadata on steering-equivalent keys.

Generate all permutations for six anchors (720 cases), or a comparably strong
deterministic exhaustive/adversarial set if fixture size changes. For multiple
evaluation positions—including center, partial falloff, and one position where
some anchors contribute zero—assert exact output bits against the first
canonical result. Do not use an epsilon.

Add focused assertions that:

1. forward, reverse, and every generated permutation produce identical bits;
2. duplicate occurrences strengthen a non-saturated influence and are not
   silently collapsed;
3. only changing source leaves output/signature equal;
4. only changing an unmasked target leaves output/signature equal;
5. changing each masked target, mask, kind, position coordinate, strength, or
   radius changes the canonical signature;
6. empty, singleton, duplicate pair, and duplicate triple signatures are all
   distinct for the fixture;
7. signed-zero/NaN payload keys can be sorted without panic (no assertion that
   malformed NaN steering is meaningful); and
8. the fixed polarity-order example matches Emphasize-first,
   Suppress-final bits and not the reverse result.

### 5.2 Route-signature tests

Replace the current weak `world-core/src/route.rs` signature test with coverage
for the shared implementation:

- many permutations have one signature;
- falloff radius is field-sensitive;
- masked target bits are field-sensitive while unmasked ones are not;
- two identical anchors do not cancel to empty or singleton;
- source is ignored; and
- exact live bits, rather than record quantization buckets/rounded positions,
  distinguish otherwise nearby anchors when they can alter steering.

Keep route attraction selection/order/corridor tests unchanged. They verify how
derived anchors are selected; the new native integration regression verifies
that selected results reach the recorder signature.

### 5.3 Runtime invalidation tests

Extend the existing `retarget_amortizes_and_refreshes_on_steering_change`
integration test or add a neighboring focused test:

1. settle under a multi-anchor slice and a one-region retarget budget;
2. reorder only and prove the update remains amortized with exact unchanged
   targets;
3. vary source/unmasked target only and prove it remains amortized;
4. add an identical duplicate and prove the whole authority refreshes;
5. vary falloff or a masked target by one raw bit and prove the whole authority
   refreshes; and
6. on the following unchanged frame, prove round-robin amortization resumes.

The test must inspect `retarget_deferred`, not infer behavior only from final
settled targets, because unnecessary full-window work is part of finding 1.

### 5.4 Native effective-route test

In the native crate's existing unit-test support:

1. use memory storage/vault helpers to install a route with a node inside the
   attraction corridor;
2. give the world an explicit player anchor;
3. enable path tracking, attraction, and route recording;
4. run the real `World::update` so target, resonance cost, and recorder observe
   one shared frame;
5. finish the recorder and inspect the actual `RouteNode`;
6. independently build the selected derived anchors with
   `attraction_anchors` and assert `anchor_sig` equals the canonical signature
   of explicit + derived anchors; and
7. assert it differs from explicit-only and remains equal under a permutation
   of the expected effective vector.

This regression must not hard-code private route ordering or fabricate a node
without going through `World::update`; it exists specifically to catch the old
caller mismatch.

### 5.5 Harness and parity gates

Strengthen `wer-anchor`'s combination scenario or add a named canonical-
multiset scenario. It should report exact bitwise agreement over adversarial
permutations, duplicate sensitivity, and the explicit polarity result. Pure
unit tests carry exhaustive detail; the harness provides a visible phase sign-
off failure if the contract regresses.

Add and pin the platform-web canonical anchor signature probe. Retain all
existing parity constants unless canonical steering itself intentionally moves
a steering-only fixture. The neutral/web wasm compile proves the key/sort/hash
code has no native dependency.

### 5.6 Regression expectations

- `wer-anchor` passes with a new canonical multiset report.
- `wer-vault` remains green; route schema, import/merge behavior, and old
  records need no migration.
- `wer-scale` remains green across executor, budget, cancellation,
  amortization, and tier schedules.
- Existing route integration tests still show bounded, soft attraction.
- Existing continuity and persistence replays remain self-equal.
- Only newly computed route anchor summaries/content ids change where the old
  signature omitted or cancelled real effective influences.
- No base-world generator golden, record byte fixture, content-id fixture,
  version constant, or layer declaration changes.

## 6. Exact file set

| File | Planned change |
|---|---|
| `crates/world-core/src/anchor.rs` | Define the complete integer steering key and shared canonical multiset helper; canonicalize once in `steer`; implement the canonical count/field signature; document final Suppress priority; add exhaustive adversarial, multiplicity, field-sensitivity, and polarity tests. |
| `crates/world-core/src/route.rs` | Remove the obsolete XOR/quantized signature implementation and its basis/imports; retain route math and update signature-focused tests to the shared canonical API. |
| `crates/world-core/src/lib.rs` | Re-export `anchor_set_signature` from its new owning module while preserving the public name; adjust route re-exports only mechanically. |
| `crates/world-core/tests/determinism.rs` | Add a new canonical anchor-signature golden and, only if demonstrated necessary, update a steering-only presentation fixture with ADR rationale; do not alter generator, record-id, or wire-byte goldens. |
| `crates/world-runtime/src/stream.rs` | Replace the slice-order anchor fold with bias plus the shared canonical signature; add private/focused signature tests if useful. |
| `crates/world-runtime/src/route.rs` | Rename/document the recorder argument as the exact effective anchors used for the preceding map update; continue writing the shared canonical signature. |
| `crates/world-runtime/tests/streaming.rs` | Extend retarget amortization tests for reorder equivalence, irrelevant metadata, duplicate/field changes, exact targets, and full-refresh/deferred counters. |
| `crates/platform-native/src/main.rs` | Pass the still-live effective player-plus-derived vector to the recorder and add a real update/route-attraction recording regression. |
| `crates/platform-web/src/lib.rs` | Add/export/pin a canonical anchor-signature parity sample with duplicate and permutation coverage. |
| `crates/tools/src/anchor.rs` | Strengthen the anchor sign-off harness with bitwise adversarial permutation, multiplicity, and fixed-polarity checks. |
| `docs/adr/0025-anchor-steering-uses-canonical-multisets.md` | Record the total key, duplicate-retaining canonical reduction, complete ordered signature, Suppress-final priority, route-effective input, portability, and compatibility decisions. Renumber if `main` gains the next ADR before rebase. |
| `docs/adr/README.md` | Index the new accepted ADR; do not edit the text of ADR 0011/0013/0015 in place. |
| `docs/world-model.md` | Update steering, target refresh, route summaries, determinism/verification text; resolve findings 1/21 and mark roadmap A.6 completed after all gates pass. |
| this plan | Change status to `Completed` only after implementation, documentation, and every validation gate pass. |

If compilation reveals a direct import of `anchor_set_signature` from the old
private module path, include only the mechanical caller migration and record it
in this table before commit. Do not expand into unrelated anchor, route,
resonance, or persistence cleanup.

## 7. ADR and versioning decision

A new ADR is required. ADR 0011 establishes order-independent anchor
composition in principle but describes mathematically symmetric reductions
without specifying the numerical order needed for bitwise equality. ADR 0015
adds route-derived anchors but does not say whether a recorded route summary
includes them. A.6 defines both durable contracts and fixes the polarity
precedence, so code comments alone are insufficient.

Create ADR 0025, currently the next free number, titled approximately “Anchor
steering uses canonical multisets.” It should:

- refine ADR 0011's order-independent intent with the exact raw-bit key and
  canonical multiset reduction;
- state that multiplicity is semantic and equal anchors never deduplicate;
- specify Emphasize reduction/blend first and Suppress reduction/blend last,
  with reflection based on the unsteered base;
- require anchor signatures to fold cardinality and the identical key
  occurrences in canonical order;
- define source and unmasked target values as non-steering metadata;
- require runtime invalidation and route recording to reuse the core signature;
- require route summaries to cover the effective explicit-plus-derived slice
  used for that frame under ADR 0015;
- retain ADR 0013's quantized persistence boundary and explain why exact live
  bit hashing is still portable for identical/reconstructed inputs;
- state that existing stored route signatures are opaque and not migrated;
  and
- name the wasm parity sample and adversarial permutation tests as enforcement.

This ADR refines and makes ADR 0011 numerically true; it does not reverse the
decision that anchors are order-independent, so ADR 0011 can remain Accepted
in the index. Do not edit accepted ADR text in place. If maintainers prefer to
label the numerical portion superseded, do so only in the new ADR/index, never
by rewriting ADR 0011's historical body.

No version bump or record migration is warranted:

- `WORLD_ALGORITHM_VERSION` identifies permanent generated base-world output;
  canonical steering is transient possibility targeting and corrects ADR
  0011's existing contract;
- layer algorithms/dependency hashes do not change, so every
  `algorithm_revision` remains 0;
- the route record schema and codec are byte-for-byte unchanged, so
  `RECORD_FORMAT_VERSION` remains 1;
- old records retain stored `anchor_sig` and content id without recomputation;
  and
- a corrected new route id is expected when the live steering summary changes,
  just as any changed RouteNode value already changes route content identity.

The new signature/parity fixed values are additive fixtures. Existing
steering-only fixed values may change only if canonical arithmetic demonstrably
changes their old noncanonical evaluation order. Existing generator identity,
record content-id, and record wire-byte fixtures must remain unchanged.

## 8. Implementation sequence

1. Add the integer semantic key and canonical multiset helper in `anchor.rs`,
   with field-normalization/key-order unit tests before changing consumers.
2. Route `steer` through the canonical references once per call, retain the
   existing formulas and final Suppress blend, and add exhaustive adversarial
   permutation plus polarity tests.
3. Move/rewrite `anchor_set_signature` around the same helper, then add count,
   duplicate, irrelevant-field, every-relevant-field, and golden tests.
4. Replace runtime's independent anchor fold with bias plus the canonical
   signature; strengthen the retarget-amortization integration regression.
5. Clarify `RouteRecorder::observe` and change native `World::update` to pass
   the effective slice; add the real explicit-plus-derived recording test.
6. Add the platform-web parity export/native golden and strengthen
   `wer-anchor` with a visible canonical multiset scenario.
7. Add ADR 0025/index and update current `world-model.md`; mark the roadmap,
   findings, and this plan completed only after every implementation/test gate
   is green.
8. Run focused tests, phase harnesses, and the complete CI-equivalent
   native/wasm validation matrix.

## 9. Validation commands

Run from `/tmp/wer-improvement-a6` with the pinned toolchain (source Cargo's
environment first if necessary):

```sh
cargo test -p world-core anchor
cargo test -p world-core route
cargo test -p world-core --test determinism
cargo test -p world-runtime --test streaming retarget
cargo test -p world-runtime route
cargo test -p platform-native
cargo test -p platform-web
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale -- --quick
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTFLAGS="-D warnings" cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
git diff --check
```

If rustc overflows its native stack while compiling the graphics dependency
tree, rerun the affected full command with the repository's established local
workaround `RUST_MIN_STACK=16777216`; do not weaken warning denial.

Also verify the immutable/history/version boundaries:

```sh
git diff -- docs/plans/prototype/implementation-plan.md 'docs/plans/prototype/phase-*-plan.md'
git diff -- docs/perf-baseline.md
git diff -- crates/world-core/src/layer.rs crates/world-core/src/record.rs
rg -n 'WORLD_ALGORITHM_VERSION: u32 = 2|RECORD_FORMAT_VERSION: u16 = 1|algorithm_revision: 0' crates/world-core/src
git diff -- crates/world-core/tests/determinism.rs crates/platform-web/src/lib.rs
```

The historical-plan, performance-ledger, layer-declaration, and record-schema
diffs must show no change, and the `rg` values must remain pinned. Review the
last diff manually: it may contain only the additive canonical-signature
goldens and an explicitly justified steering-only value if canonical operation
order required it. It must not alter base identity, record id, record wire
bytes, or existing unrelated parity constants.

## 10. Documentation completion edits

After all implementation and tests pass:

- Change roadmap A.6 to **Completed**, link this plan, and summarize the raw-
  bit total key, canonical duplicate-retaining reduction, shared signature,
  explicit Suppress-final priority, and effective route recording.
- Rename finding 1 as resolved. Retain the original failure description for
  auditability and append the concrete canonical-key/reduction solution plus
  exhaustive exact-bit permutation and retarget-amortization tests.
- Rename finding 21 as resolved. Retain the omission/XOR/explicit-only failure
  history and append count/full-field folding, duplicate retention, irrelevant-
  field exclusion, and the native explicit-plus-route-derived recording gate.
- Update Section 2.4 to state that all anchor occurrences are sorted by the
  documented steering key before reduction, and that Suppress deliberately
  applies after Emphasize rather than using a simultaneous solve.
- Update Section 3.3 to distinguish steering-semantic fields from
  `AnchorSource` metadata and unmasked target storage.
- Update Section 3.5/3.25/3.26 target-refresh text so reorder-only slices reuse
  amortization while count or relevant-field changes force a full refresh.
- Update Section 3.7 so route-node `anchor_sig` is a canonical multiset
  signature of the exact effective anchors, including route-derived anchors
  selected for the just-completed frame.
- Update Sections 2.9 and 3.28 with the new canonical-signature golden/parity
  probe and adversarial exact-bit/harness/native integration tests, while
  retaining the caveat that CI compiles but does not execute wasm exports.
- Change this plan's status from `Planned` to `Completed` last.

Do not overclaim collision-free identity or general cross-platform equality of
live float captures/resonance. The achieved contract is: for identical anchor
IEEE fields, bias, base, and evaluation point, every slice permutation executes
one canonical operation sequence and yields one exact result; the compact
signature fingerprints the same canonical multiset with every occurrence.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Key omits a field later read by steering | Define key beside `Anchor`/`steer`, make signature consume it, test every current relevant field independently, and require future fields to update one helper. |
| Signature includes metadata steering ignores | Normalize unmasked target slots and omit `AnchorSource`; test metadata-only changes leave both output and signature equal. |
| Equal anchors are deduplicated accidentally | Keep a vector, never a set/map; fold count and every occurrence; assert singleton/pair/triple differences and stronger duplicate influence. |
| Sort uses numerical/partial float comparison | Store only `to_bits()` integers and derive `Ord`; include signed-zero/NaN-payload no-panic key tests. |
| Canonicalization is repeated eight times | Sort once per `steer` call and share references across domain loops. |
| Refactor silently changes polarity math | Preserve expression order and add a fixture that distinguishes Suppress-final from reverse/simultaneous results. |
| Runtime hash drifts from core signature | Delete its anchor-field loop; fold the public canonical signature after bias bits. |
| Recorder still omits derived routes | Pass the exact live `effective` vector and test through real native `World::update`. |
| Old route ids are invalidated | Never recompute stored summaries/ids; schema and reader stay unchanged. Only new corrected recordings differ. |
| Raw-bit signature conflicts with persistence portability | Hash reconstructed live bits; ADR 0013 makes those bits portable for record-derived anchors. Keep live-capture portability claims scoped. |
| Broad fixture re-bless hides generation drift | Permit only additive signature/steering fixtures and inspect determinism/parity diffs explicitly; versions and record bytes stay fixed. |
| Allocation becomes visible in profiling | Anchor counts are bounded/small; land the auditable shared definition first, then optimize only under equivalent exact-bit tests. |

Rollback is one commit: revert the A.6 commit. There is no schema migration,
vault rewrite, record re-encoding, base-world version change, or irreversible
external state.

## 12. One-commit worktree, merge, and push workflow

Implement only on `codex/improvement-a6-anchor-canonicalization` in
`/tmp/wer-improvement-a6`. The planning agent does not commit. The fresh
execution agent implements and validates the entire plan, stages only the
documented file set, and creates exactly one commit, for example:

```text
Canonicalize anchor steering and signatures
```

Before merge, check the latest local `main`. If it advanced, rebase the single
A.6 commit onto it, resolve without dropping either side, and rerun every
focused and full gate on the rebased tree. If the next ADR number was consumed,
rename ADR 0025 and its index/citations during the rebase without editing an
accepted record in place.

Fast-forward merge from the primary worktree with `git merge --ff-only`,
confirm the forbidden historical plans, performance ledger, record fixtures,
generator goldens, and version constants remain untouched, then push `main` to
`origin`. Do not begin A.7 until that push succeeds.

## 13. Definition of done

A.6 is done only when one complete raw-bit key orders every anchor occurrence;
`steer` is bitwise equal across adversarial permutations; Emphasize-first/
Suppress-final priority is explicit and tested; signatures fold cardinality
and every relevant field with duplicate multiplicity; runtime reorders remain
amortized while semantic edits refresh fully; native route nodes sign the exact
explicit-plus-derived anchors that steered their frame; parity/golden/harness
gates cover the contract; ADR 0025 and current documentation land; every
focused, harness, native, and wasm gate passes; exactly one commit is fast-
forwarded to `main`; and that commit is pushed successfully to `origin`.
