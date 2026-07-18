# World Loom Stage 0B pre-visualization implementation plan

## Status, scope, and stop line

This plan implements the machine-checkable portion of the Stage 0B kill gate in
[`new-world-model-option-4.md`](new-world-model-option-4.md). It extends the
ratified Stage 0A kernel without changing the existing prototype, its world
algorithm version, layer revisions, viewer, renderer, or persistence formats.

The stopping point is the first claim that needs a Visualization: judging that
two Egress modes look distinct, proving that presentation never blanks or
reloads, and running the preregistered human playtest. Before that boundary the
experiment can and should establish canonical planetary topology, realization,
interaction, travel, transition, and host-facing data contracts. Those outputs
make the later visual test precise instead of asking a renderer prototype to
invent Model semantics.

This slice does **not** claim that Stage 0B has passed. It produces a
`ReadyForVisualization` result only when all pre-visualization gates pass.
Option 4 may proceed to a visualization spike at that point; it may not proceed
to Stage 1 until the visual continuity and playtest gates also pass.

## Deliverables

1. `loom-world`, a platform-neutral Stage 0B experiment containing:
   - the closed 20-face spherical topology and canonical face adjacency;
   - one deterministic material field, habitat measure, and organism trait per
     face, derived from a Stage 0A State Packet and integer addresses;
   - typed Accentuate, Repress, and Hold interactions lowered to one canonical
     normalized intent;
   - a single authoritative traveler advanced by canonical path segments;
   - physical-distance-to-Egress credit conversion and selected-mode commit;
   - a bounded Transition Plan with correspondence, birth/death, changed-face,
     and continuation metadata;
   - model-neutral Map and local-tangent POV DTOs, with no renderer types or
     platform APIs.
2. A `wer-loom-0b` sign-off harness with a frozen ordinary and adversarial
   corpus, native latency ledger, determinism/schedule checks, and explicit
   readiness verdict.
3. Native and actual Node-wasm parity fixtures for topology, realization,
   interaction, traveler update, transition-plan bytes, and presentation DTOs.
4. A documented test evaluation that separates pre-visualization readiness
   from the still-unmeasured visual and human gates.

## Frozen experiment contract

### Closed planet and addresses

- The planet has exactly 20 triangular faces with the standard combinatorial
  icosahedron adjacency table. Face ids are `0..20`; every face has three
  sorted, distinct neighbors; adjacency is symmetric; Euler counts are
  `V=12`, `E=30`, `F=20`.
- Stage 0B has no refinement. `FaceId` is the only canonical spatial address.
  A position is `(face, barycentric Q16 u, barycentric Q16 v)`, with
  `u + v <= 2^16`. Altitude is signed centimetres.
- A `TravelerPathSegment` contains canonical start/end positions and an integer
  surface distance in millimetres. It is rejected unless endpoints are on the
  same face or adjacent faces, and zero distance requires identical endpoints.
  No float or host clock enters state or identity.

### Tiny realization

- Realization is a pure function of `(state root, face id)` and uses only
  domain-separated integer hashing and packet masses.
- Material is one scalar Q24 sample, habitat is a two-atom measure whose total
  is exactly Q24 one, and organism trait is one Q24 scalar. All outputs are
  canonical integers.
- The 20-face snapshot is computed in face-id order and commits to its canonical
  bytes. Query order, cache state, and task completion order are not inputs.
- The experiment deliberately makes the Stage 0A full and tempered route
  signatures produce materially different canonical field deltas. Whether
  those differences are *visibly* distinct remains a visualization question.

### Interaction semantics

- `Accentuate(subject, weight)` adds positive intent at the selected typed
  atom; `Repress` adds the corresponding negative intent; `Hold` adds an
  opposing, higher-weight term for the source value.
- Subjects are limited to Material, Habitat, and OrganismTrait. Habitat and
  OrganismTrait lower to the Stage 0A trait law; Material lowers to its
  conserved law. Stable term ids are derived from interaction content, not
  insertion order.
- Canonical normalization supplies order independence and conflicting-id
  rejection. The probe remains fail-closed with `Unresolved`.

### Travel gating and the one-update rule

- One millimetre of accepted physical travel grants one micro-length unit of
  Egress credit. Credit uses checked addition and is retained until the selected
  Stage 0A segment can be committed atomically.
- `LoomHost::update` accepts exactly one `TravelerPathSegment`, validates and
  applies it once, adds its credit once, and advances the selected Egress mode
  at most once. Map and POV DTOs are then built from the same post-update
  traveler and state root.
- Presentation DTO construction is read-only. It cannot advance travel, probe
  Egress, commit a state, or change credit. This is the Stage 0B form of ADR
  0028's one traveler/one world update rule and is identical on native/wasm.

### Transition Plan

- A plan contains source/destination roots, selected mode id, total and applied
  Egress length, all 20 face correspondences, bounded changed-face records, and
  explicit birth/death events for threshold crossings.
- Face correspondence is identity in this unrefined planet. Each record carries
  before/after material, habitat, and trait values; unchanged records are
  omitted. Ordering is by face id and event kind.
- The cap is 20 correspondence records, 20 changed faces, and 40 events. Any
  violation returns `Unresolved`; truncation is never called complete. Canonical
  bytes are stable and suitable for a later visualization replay test.

### Host-facing DTO boundary

- `MapSnapshot` supplies the state root, traveler face/barycentrics, and the 20
  canonical face samples in topology order.
- `PovSnapshot` supplies the same state/traveler identity plus an integer local
  tangent frame descriptor and the current face plus its three neighbors.
- DTOs contain no GPU handles, colors, meshes, DOM/winit values, files, clocks,
  or mutable Model handles. A future separate Loom host or generalized viewer
  consumes them; Stage 0B does not pre-decide renderer integration.

## Work sequence

### A. Workspace and boundaries

- Add `loom-world` as a workspace member depending only on `loom-core` and
  `loom-transport`.
- Add it to `tools` and wasm parity dependencies.
- Keep it platform neutral: no filesystem, sockets, threads, random devices,
  clocks, graphics, `viewer-host`, or platform crates.

### B. Topology and realization

- Implement checked address types and the frozen adjacency table.
- Machine-check closure, symmetry, degree, edge count, and address bounds.
- Implement domain-separated fixed integer sampling from State roots.
- Build canonical face samples and snapshot bytes; test query-order and native/
  wasm parity.

### C. Interaction and Egress session

- Implement typed interaction lowering with content-derived stable ids.
- Wrap Stage 0A probing/selection without weakening `Complete`/`Unresolved`.
- Implement a session holding one packet, selected mode, traveler, and unused
  credit. Check overflow and reject discontinuous path segments.
- Test Accentuate/Repress direction, Hold resistance, insertion-order
  invariance, two distinct endpoint roots, zero-travel behavior, equal
  cumulative-distance behavior, and at-most-one commit per update.

### D. Transition and presentation contracts

- Derive the bounded Transition Plan before committing its endpoint.
- Canonically encode plan records and verify roots/mode/length against the
  selected certified mode.
- Produce Map and POV snapshots only after the update. Assert their traveler
  position and state root agree for every frame.
- Add structural tests that transition data always covers all face
  correspondences and never represents a committed transition as an empty
  replacement. Actual no-blank presentation remains deferred.

### E. Harness and parity

- Freeze at least 128 ordinary interactions spanning subjects, all three
  influences, face addresses, both route strengths, no-op travel, partial
  credit, exact commit, and surplus credit.
- Gate ordinary complete rate at 99%, while reporting adversarial resolution
  separately.
- Run 10,000 deterministic permutations of interaction order, face query order,
  simulated probe schedule/cancellation, and travel segmentation. Compare
  packet roots, selected mode ids, transition bytes, final credit, and Map/POV
  identities.
- Gate native interaction/probe plus 20-face realization below 8 ms per frozen
  ordinary case. Gate the corresponding warmed Node-wasm fixture below 20 ms.
  Timing stays outside canonical results.

### F. Verification and decision

Run:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p viewer-host -p platform-web \
  -p loom-core -p loom-transport -p loom-world --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
cargo run --release --bin wer-loom
cargo run --release --bin wer-loom-0b
```

Interpretation is intentionally asymmetric:

- any correctness, parity, 99% completion, boundedness, or latency failure is a
  **no-go** for visualization and requires simplifying Stage 0B or Option 4;
- all pre-visualization gates passing is a **go only for the visualization and
  playtest spike**;
- Option 4 is a **no-go for Stage 1** until two modes are visibly distinct, no
  transition blanks/reloads, and the preregistered 80%/majority playtest gate
  passes.

## Completion checklist

- [x] Closed 20-face topology and checked canonical addresses.
- [x] Deterministic material, habitat, and organism-trait realization.
- [x] Canonical Accentuate/Repress/Hold lowering.
- [x] Physical travel credit and exactly one authoritative update.
- [x] Two distinct certified Egress endpoints.
- [x] Complete bounded Transition Plan or explicit `Unresolved`.
- [x] Model-neutral Map/local-tangent POV DTOs agree post-update.
- [x] Native/wasm frozen bytes and 10,000 schedule/permutation cases agree.
- [x] At least 99% ordinary completion within native and wasm targets.
- [ ] Visualization and preregistered playtest gates remain explicitly open.

## Execution result and recommendation

Executed on 2026-07-18. The release `wer-loom-0b` ledger reported 128/128
ordinary requests complete, 10,000/10,000 randomized interaction-order,
schedule/cancellation, realization-address, and sampled travel-segmentation
cases equal, 6/6 adversarial checks passing, and a worst native ordinary case of
475.838 microseconds against the 8 millisecond gate. The frozen combined parity
digest is `bb9e7b7cf5edef53b8032ada4f55dcc937b2ef0d5e632f2cf8bb24dfddb99553`.
The Node wasm parity suite passed the same digest and its warmed 20 millisecond
average-time assertion.

The complete repository CI surface also passed: formatting, warnings-as-errors
Clippy, workspace check/test, the required wasm32 check, and the Node wasm
suite. Re-running `wer-loom` left the Stage 0A evidence green at 128/128
ordinary, 8/8 adversarial, and 10,000 randomized cases.

**Decision:** proceed with Option 4 only to the Stage 0B visualization and
preregistered playtest spike. The evidence is a go for that next experiment: no
pre-visualization correctness, portability, resolution-rate, boundedness, or
latency gate failed. It is not evidence to proceed to Stage 1. The two endpoint
roots and their face fields are canonically distinct, but only Visualization
can establish that the distinction is legible, that correspondence avoids a
perceived blank/reload, and that 80% of participants identify the requested
direction while a majority find the consequences non-obvious but coherent.
