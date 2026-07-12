# 25. Anchor steering uses canonical multisets

Date: 2026-07-12

## Status

Accepted

Refines [ADR 0011](0011-anchors-capture-trait-targets-combine-order-independently.md),
and builds on
[ADR 0013](0013-shareable-records-quantized-at-persistence-boundary.md) and
[ADR 0015](0015-routes-attract-as-derived-anchors.md).

## Context

ADR 0011 made anchor composition mathematically symmetric within each
polarity, but the implementation accumulated `f32` values in caller slice
order. Floating-point addition and multiplication are not associative, so two
permutations of the same anchors could differ by an ULP and even cross a
possibility bucket boundary. The runtime invalidation fingerprint repeated a
separate, slice-ordered field list.

Route-node summaries used a different approximation: rounded and quantized
fields were XORed. Falloff radius was absent, unmasked target storage was
present, duplicate anchors cancelled, and cardinality was absent. Native route
recording also omitted route-derived anchors even when those anchors had just
changed the recorded target and resonance cost.

Order independence therefore needs a numerical operation order, and every
consumer needs one definition of the steering-semantic multiset.

## Decision

1. **One raw-bit key defines steering equivalence and order.** Each anchor is
   projected to the lexicographic tuple `(world x f64 bits, world y f64 bits,
   mask, polarity tag, strength f32 bits, falloff-radius f64 bits, masked
   target f32 bits in domain order)`. Unmasked target slots use a zero sentinel.
   Unsigned integer ordering is deliberate; it is total and portable and does
   not claim numerical float ordering.

2. **Source and unmasked targets are metadata.** `AnchorSource` and target
   values outside the mask do not affect steering, canonical order, or the
   anchor signature. Signed zero, infinities, and NaN payloads still have a
   deterministic key if malformed session-local values reach the API; this ADR
   adds no new validation policy.

3. **The input is a multiset.** Every occurrence is projected and sorted. Equal
   keys are never deduplicated: repeated anchors contribute repeatedly to the
   weighted denominator and saturating product. Steering sorts once per call
   and every possibility domain iterates the same canonical references.

4. **Polarity priority is explicit.** Existing equations and arithmetic order
   remain. Emphasize reductions are blended into the unsteered base first.
   Suppress targets are reflected about that original base, but their combined
   result is blended last into the Emphasize result. Suppress therefore has
   final-blend priority; this is not a simultaneous solve.

5. **The canonical signature consumes the same projection.** A domain-separated
   basis folds cardinality and then every field of every sorted key occurrence
   in the order above. It is a compact `u64` fingerprint and is not claimed to
   be collision-free. Runtime retarget invalidation folds raw bias bits and this
   signature rather than maintaining another anchor-field list.

6. **A route signs its effective frame inputs.** `RouteRecorder::observe`
   receives the exact effective slice used for the immediately preceding map
   update. With route attraction enabled, this is the explicit player anchors
   plus the deterministically selected route-derived anchors from ADR 0015.
   Candidate routes or nodes not selected for that frame are not signed.

7. **Persistence remains quantized at its boundary.** The live signature hashes
   exact IEEE fields. Record-derived anchors are still portable because ADR
   0013 reconstructs those fields from integers. Existing stored `anchor_sig`
   values remain opaque historical summaries and are never recomputed or
   migrated. The `RouteNode` schema and content-id fold order do not change;
   newly recorded routes can receive corrected signatures and ids.

8. **Verification is exact and portable in scope.** Core tests exhaust all 720
   permutations of an adversarial six-anchor fixture at multiple positions and
   compare output bits. Tests cover multiplicity, every semantic field,
   metadata exclusion, unusual float encodings, and final Suppress priority.
   Runtime tests distinguish amortized reorder/metadata edits from full
   semantic refreshes. The native shell tests explicit-plus-derived recording,
   `wer-anchor` reports the multiset contract, and a fixed canonical-signature
   probe is golden-tested natively and compiled for wasm.

## Alternatives considered

- **Use the caller's order and tolerate approximate equality:** rejected
  because possibility buckets, dependency hashes, and target-refresh behavior
  can amplify an ULP difference.
- **Sort numerically with partial float comparison:** rejected because NaNs are
  unordered and numerical order has no gameplay meaning here.
- **Deduplicate equal anchors:** rejected because it would change the existing
  saturating influence model and would require persistence/UI identity rules.
- **Quantize before steering or signing:** rejected because it would erase live
  distinctions that affect the float calculation and move the persistence
  boundary established by ADR 0013.
- **Solve both polarities simultaneously:** deferred as a different steering
  model. This correction makes the existing Suppress-final behavior explicit.
- **Let the recorder rediscover routes:** rejected because selection belongs to
  the caller; the recorder must summarize the inputs that actually produced
  the frame.

## Consequences

- A permutation of identical anchor IEEE fields executes exactly one operation
  sequence and yields identical steering bits and signature.
- Reorder-only, source-only, and unmasked-target-only edits no longer trigger a
  full target refresh. Multiplicity or any steering-field edit does.
- Route summaries now agree with target and resonance inputs, including active
  route attraction.
- Canonicalization allocates and sorts a small temporary vector per `steer`
  call. Normal explicit anchors plus the bounded route candidates keep it
  small; an equivalent optimization may replace it only under the same tests.
- `WORLD_ALGORITHM_VERSION` remains 2, layer revisions remain 0,
  `RECORD_FORMAT_VERSION` remains 1, and existing generator/record-byte goldens
  do not change. The new signature fixtures are additive.
- This does not make live float capture or resonance generally portable across
  platforms, and CI still compiles rather than executes the wasm probes.
