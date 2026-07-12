# Improvement A.5 — Resource-Tier-Invariant Gameplay Sampling

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.5](../../world-model.md#prioritized-improvement-roadmap)

**Finding addressed:** [6](../../world-model.md#6-resource-tiers-feed-gameplay-and-persistent-identity)

This plan implements the fifth item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). Resource tiers may increase visual
organism density and the pace/capacity of derived work, but they must not
change which organism drives a capture, the resonance scalar that gates
convergence, or the transition cost and content identity of a recorded route.

The correction gives every realized organism an explicit density-slot label,
defines slot 0 as the one authoritative gameplay sample, schedules that sample
independently of the tier's presentation-realization budget, and gives the
authoritative resonance graph one fixed 64-node ceiling. Slots above zero
remain additive presentation instances: they continue to render, appear in
ecology inspection, scale population approximately linearly, and retain their
existing feature ids, but no gameplay or persistence path may read them.

This is a post-prototype corrective plan. Do not modify
[`implementation-plan.md`](implementation-plan.md), any
`docs/plans/prototype/phase-N-plan.md`, or the historical performance ledger.
The roadmap item and finding stay open until the code, focused regressions,
encoded-record gate, ADR, current documentation, and complete native/wasm CI
matrix pass in the A.5 worktree.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. Each `Organism` records the realization density slot that produced it.
   Slot membership is data, not inferred from vector position, organism id,
   population count, or a tier-dependent index. `slot == 0` is the canonical
   gameplay sample; `slot > 0` is presentation-only.
2. Existing identities do not move. For resolution `n`, cell index `c`, and
   slot `s`, the feature index remains `c + s*n^2`. Every slot-0 organism keeps
   its Phase 5 id, every higher-slot organism keeps its Phase 6 additive id,
   and the RNG draw order within each organism is unchanged.
3. `StreamConfig::organisms_per_cell` continues to select the displayed
   density (Low 1, Mid 2, High 4). The public presentation iterators and
   renderer continue to expose all realized slots. A density-4 map must still
   contain the density-1 ids and approximately four times the population.
4. Gameplay reads a filtered slot-0 view. Organism trait capture finds the
   nearest slot-0 organism only. Resonance collects slot-0 organisms only.
   Aggregate/environment capture stays unchanged.
5. Slot-0 availability is not governed by
   `Budget::max_realize_organisms`. Split near-field realization into a fixed
   authoritative pass and a budgeted visual-expansion pass. The authoritative
   pass runs before resonance, visits fresh eligible near regions in the
   existing nearest-first total order, and admits a fixed number of whole
   regions per frame independent of `ResourceTier` and `Budget`.
6. Use one whole authoritative region per frame as the fixed admission limit.
   Whole-region publication remains atomic and bounded; the covering/nearest
   eligible region wins first. This preserves the old Low-tier pacing shape
   without letting Mid/High's 800/1,600-organism presentation budgets publish
   gameplay samples earlier merely because the hardware tier is larger.
7. The authoritative pass publishes exactly `organisms_per_cell = 1` from the
   current fresh L8 key and complete roster set. A changed or missing L8 key
   retires the stale vector/key before gameplay reads it. A missing roster
   defers publication without exposing a partial or old authoritative sample.
8. The visual pass may replace that slot-0 vector with a full 1/2/4-slot vector
   only after the authoritative key is current. It remains bounded by
   `max_realize_organisms`, and its higher density must not create or accelerate
   authoritative availability.
9. Track authoritative-key currency separately from visual-density currency.
   An L8 key alone is insufficient to say whether the requested visual slot
   count was expanded. Empty realizations also need explicit keys, so absence
   of organisms cannot be mistaken for unfinished work.
10. Parking, radius unload, preserve-driven revision changes, session restore,
    and roster-repair failure retire both authoritative and visual realization
    bookkeeping through one helper. No stale slot-0 sample may survive a
    changed possibility revision or dependency key.
11. The resonance graph always truncates to one semantic constant,
    `MAX_RESONANCE_NODES = 64`, after the existing distance/species/position
    total sort. Remove `max_resonance_nodes` from `Budget`; Low/Mid/High and
    `Budget::unlimited` must not be able to change resonance content.
12. Density, entropy, distance, anchor compatibility, occlusion, and the final
    resonance equation remain mathematically unchanged. Given equal settled
    authoritative inputs, Low/Mid/High return bit-identical resonance strength,
    node order, and node fields.
13. The route recorder continues to quantize the frame's authoritative
    resonance as `floor(255 * (1-strength))`. Its record schema, sampling
    spacing, target/stability/anchor fields, and vault sequence semantics do not
    change.
14. A same-path, same-anchor, settled cross-tier expedition must produce
    exactly equal `RouteNode`s, equal `RouteRecord::id`, and byte-for-byte equal
    `encode_record(RecordKind::Route, record)` output—not merely equal mean
    difficulty.
15. Live capture and resonance remain presentation-grade across native/wasm
    because they still read float-derived habitat and expression (ADRs 0010 and
    0011). This change adds same-platform resource-tier invariance; it does not
    silently promote those live derivations to portable parity exports.
16. Generation/executor readiness remains ADR 0018's explicit mid-journey
    caveat. The fixed authoritative pass removes tier-dependent *realization*
    admission, but it cannot sample a region before L8 and its rosters are
    fresh. Different generation schedules may therefore still make a sample
    available on different frames. Cross-tier equality is asserted once the
    same authoritative inputs are ready, and the encoded-route gate settles
    each sampled waypoint before observing it.
17. No generator equation, layer declaration, dependency-hash fold, feature-id
    fold, record field, or codec order changes. `WORLD_ALGORITHM_VERSION`
    remains 2, every layer `algorithm_revision` remains 0,
    `RECORD_FORMAT_VERSION` remains 1, and no golden fixture is re-blessed.

## 2. Scope boundaries

### 2.1 In scope

- Explicit organism slot membership and canonical-slot iterators.
- A fixed, tier-independent slot-0 realization pass before resonance.
- Budgeted expansion of the same presentation vector to the tier's full
  density after canonical publication.
- Slot-0-only capture and resonance.
- A fixed semantic resonance ceiling instead of a resource budget knob.
- Cross-density/runtime tests and Low/Mid/High capture, resonance, and encoded
  route-record gates.
- ADR 0024, its index entry, and completion/current-model edits in
  `world-model.md`.

### 2.2 Explicitly out of scope

- Making L8 generation or executor completion frame-identical across resource
  tiers. ADR 0018 deliberately permits schedule-paced mid-flight differences.
- Cross-platform live-capture or live-resonance parity; float habitat
  classification remains presentation-grade under ADRs 0010/0011.
- Changing organism identity/placement epochs or separating M/B/A expression
  from succession (A.9/finding 12).
- Reconciling aggregate pressure with realized consumer biomass (later ecology
  work/finding 11).
- Adding a capture radius, changing Planetary capture, or changing fallback
  behavior (finding 18).
- Canonicalizing anchors/signatures, suppress compatibility, route-attraction
  strength, frame-spaced route interpolation, or route traversal semantics
  (later roadmap items/findings 1, 7, 17, 21, and 22).
- Changing route schema, record format, route content-id fold order, vault
  merge laws, or existing codec goldens.
- Editing accepted ADRs in place, historical prototype plans, phase plans, or
  performance-baseline ledgers.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `realize_region_into` | Generates slots in a stable order but `Organism` does not retain `slot`. | Store `slot: u16` on every instance without changing its feature index or RNG stream. |
| `organism_keys` | One L8 key describes both gameplay availability and full visual density. | Track current canonical L8 key and completed `(L8 key, slot count)` separately. |
| `realize_near_window` | Whole 1/2/4-slot vectors are admitted under the tier-scaled realization budget after resonance. | Canonicalize one slot on a fixed pre-resonance schedule; expand visual slots separately. |
| `nearest_organism` | Searches every realized slot. | Search the canonical slot-0 iterator only. |
| `resonance_at` | Scans every realized slot and truncates at `budget.max_resonance_nodes`. | Scan slot 0 only and truncate at fixed 64. |
| `ResourceTier::budget` | Selects resonance caps 64/96/128. | Delete the semantic knob; tiers may scale work, not resonance content. |
| `RouteRecorder` caller | Receives a `FrameStats` strength that currently depends on density and cap. | Preserve the API/data flow after making `FrameStats::resonance_strength` canonical. |
| `wer-scale::tier_identity` | Compares layer hashes and possibility buckets only. | Add exact canonical resonance/capture and encoded route-byte equality. |

## 4. Required design

### 4.1 Explicit slot identity

Extend the transient `Organism` type with:

```text
slot: u16
```

Set it directly from the existing realization loop. Do not derive it later
from vector ordering: density gating means slots may be absent, and cell-major
ordering does not provide a reliable inverse. Do not add `slot` to any stable
hash; the existing `feature_index = cell + slot*n^2` already supplies the
identity input and must stay byte-for-byte unchanged.

Keep the current full presentation APIs:

```text
organisms() -> all displayed slots
organisms_in(coord) -> all displayed slots in that region
organism_count() -> all displayed slots
```

Add canonical read helpers whose implementation filters `slot == 0`:

```text
authoritative_organisms() -> ordered slot-0 iterator
authoritative_organisms_in(coord) -> ordered slot-0 iterator for one region
```

The renderer, visual hit testing, ecology/entity diagnostics, and density gates
continue to use the full presentation APIs. `capture_at` and `resonance_at`
must use only the authoritative helpers. Naming these helpers
"authoritative" is intentional: a future gameplay consumer should encounter a
clear API boundary rather than accidentally reusing the visual iterator.

Include `slot` in the same-platform replay hash after id/species so incorrect
slot labeling is observable. This changes only the numeric diagnostic hash,
not feature identity or persisted data; comparison laws, not a literal hash
golden, are the contract.

### 4.2 Two-stage realization with one stored presentation vector

Retain one `BTreeMap<RegionCoord, Vec<Organism>>`; do not duplicate every
slot-0 organism in a second allocation. Split the key bookkeeping:

```text
authoritative_organism_keys: coord -> current L8 dependency hash
presentation_organism_keys: coord -> (current L8 dependency hash, realized slot count)
```

An empty vector with both keys is a valid, completed barren realization.

Refactor `realize_near_window` into two internal operations sharing the same
freshness and roster-completeness preflight:

1. `realize_authoritative_near_window` runs before resonance. It removes
   offscreen/stale vectors and keys, sorts eligible current-L8 regions nearest
   first using distance bits then coordinate, and publishes at most one whole
   region per frame using `realize_region_into(..., organisms_per_cell = 1)`.
   On publication it records the authoritative key and presentation key
   `(l8_hash, 1)`.
2. `expand_visual_near_window` runs after dispatch and the second integration.
   It considers only regions whose authoritative key already equals their
   current L8 key. Under `Budget::max_realize_organisms`, it recomputes the
   whole vector with `max(config.organisms_per_cell, 1)` and atomically replaces
   the slot-0 vector, recording `(l8_hash, target_slots)`. The recomputed slot 0
   is bit-identical; higher slots are additive presentation.

The one-region authoritative cap is a semantic fixed scheduler, not a public
resource-tier knob. If a future measurement justifies changing it, change it
as one global constant with cross-tier tests; never put it back into `Budget`
or `ResourceTier`.

Run the authoritative stage after integrate/load/retarget and before
`resonance_at`. This removes the old dependency on the prior frame's tier-
budgeted full-density realization. A newly synchronous-dispatched L8 result
integrated later in the frame becomes eligible on the next update; capture and
route recording therefore observe the same frame sample that gated convergence,
not a post-gate sample that appeared only after dispatch.

`FrameStats::organisms_realized` continues to describe the budgeted visual
expansion work. Add a separate
`authoritative_organisms_realized` counter for fixed slot-0 publication so
telemetry and budget assertions do not conflate semantic work with optional
density work. Panel/harness totals that mean “work realized this frame” should
sum or display both explicitly; budget gates apply only to the visual counter.

### 4.3 Invalidation and lifecycle rules

Centralize realization retirement. The helper must clear/recycle the vector
and remove both key entries. Use it for:

- leaving the near radius;
- capacity parking or geometric unload;
- material preserve-winner snaps/revision changes;
- session replacement/restore paths that discard derived instances; and
- a current L8 key mismatch before the authoritative stage.

When roster preflight fails for a changed key, fail closed: no old slot-0
sample participates in resonance or capture, neither key advances, and the
normal ADR 0019 roster repair path retries. When only visual expansion is
deferred, retain the current slot-0 vector and authoritative key so gameplay
does not disappear under presentation backpressure.

### 4.4 Fixed authoritative resonance

Define and re-export:

```text
MAX_RESONANCE_NODES: usize = 64
```

Change the public runtime API from:

```text
resonance_at(player, anchors, budget)
```

to:

```text
resonance_at(player, anchors)
```

Collect only `authoritative_organisms()`, keep the existing radius test and
total sort, and truncate to `MAX_RESONANCE_NODES`. The formula and
`ResonanceNode` layout stay unchanged. `FrameStats::resonance_nodes` and the
debug graph now report only canonical nodes.

Delete `Budget::max_resonance_nodes` from its type, constructors, scaling,
literal callers, and tier presets. The cap changes semantic input selection and
therefore is not a temporal-work budget. Resource-tier documentation should
show a fixed 64-node contract rather than 64/96/128. `Budget::unlimited` must
also retain the 64-node semantic ceiling.

Update the anchor harness: it may no longer manufacture a “sparse ecology” by
lowering a semantic cap. Keep the pure density monotonicity unit test, assert
the live graph is bounded by the fixed constant, and preserve stationary and
anchor-compatibility gates.

### 4.5 Capture and route data flow

Change `nearest_organism` to iterate slot 0 only. Keep its distance comparison
and deterministic vector order. Environment-channel capture and Ecology's
vegetation fallback do not change. A query at an extra-slot organism's exact
position must still select the same nearest canonical organism as Low tier,
rather than the visually closer extra.

Keep `RouteRecorder::observe`'s `resonance_strength` argument and all record
fields unchanged. The native pipeline already passes
`FrameStats::resonance_strength` from the map update that immediately preceded
recording; once that field comes only from canonical slot 0 and fixed 64, route
cost is authoritative. Document that callers must pass that frame value, not a
separately constructed visual-density statistic.

This plan deliberately does not recompute resonance inside `RouteRecorder`:
the update used the effective anchor set (including enabled route-derived
anchors), while the record's `anchor_sig` intentionally receives the player's
explicit anchor slice. Conflating those two inputs would partially and
incorrectly solve finding 21 while changing route semantics beyond A.5.

### 4.6 Availability and the ADR 0018 boundary

The two-stage pass closes the resource-density leak:

```text
fresh L8 + complete rosters
        -> fixed nearest-first slot-0 publication (one region/frame)
        -> slot-0 capture + fixed-cap resonance
        -> convergence + route cost

same slot-0 publication
        -> tier-budgeted expansion to 1/2/4 displayed slots
        -> renderer / inspector only
```

Whole-region *presentation* realization budget no longer changes slot-0
availability. Generation readiness can still differ: a threaded or smaller
generation budget may make the prerequisite L8 key fresh later. That is the
existing ADR 0018 allowance for mid-flight schedule coupling, not permission
for a tier-density/cap knob to alter results given the same ready inputs. Tests
must make this precondition explicit by settling the compared waypoint before
capture/resonance/record observation.

## 5. Verification matrix

### 5.1 Pure realization tests

Extend `world-runtime/src/realize.rs` tests to prove:

1. every returned organism carries the loop slot that generated it;
2. density 1 contains only slot 0;
3. density 4 retains the exact density-1 structs/ids as its slot-0 subset;
4. every higher-slot id remains unique and unchanged by the new field; and
5. filtering density 4 to slot 0 equals density 1 byte-for-byte at the struct
   field level.

### 5.2 Runtime lifecycle and cross-density tests

Add focused tests in the private `stream.rs` tests and/or
`world-runtime/tests/streaming.rs`:

1. **Fixed admission:** with equal fresh inputs and radically different
   `max_realize_organisms`, the same nearest canonical region publishes each
   frame and authoritative keys/vectors match. A large visual budget may add
   higher slots only after that publication.
2. **No stale authority:** change an L8/revision input; before roster/L8 repair,
   neither capture nor resonance reads the old slot-0 vector. After repair the
   canonical vector and keys publish atomically.
3. **Visual backpressure:** force visual expansion to defer and prove slot-0
   capture/resonance remains available and unchanged while extra slots arrive
   later.
4. **Cross-density resonance:** settle identical maps at 1 and 4 slots; require
   exact equality of `Resonance::strength.to_bits()`, ordered node fields, and
   node count, while full presentation counts differ materially.
5. **Cross-density capture:** choose a density-4 `slot > 0` position where the
   old nearest-any-slot algorithm would select the extra. The exact resulting
   `Anchor` (target bits, source species, mask/kind/position/strength/falloff)
   must equal density 1's capture.
6. **Fixed cap:** make more than 64 canonical nodes eligible and assert exactly
   the first 64 under the documented total order are used independent of
   `Budget::default`, `Budget::unlimited`, and resource preset.
7. **Lifecycle retirement:** parking, near exit, and preserve revision
   invalidation remove both key kinds and never leave a slot-0 organism visible
   to gameplay.

### 5.3 Cross-tier sign-off gate

Extend `tools::scale::tier_identity` beyond its current layer-hash and bucket
checks. Build and settle real Low, Mid, and High presets at the harness
resolution, then compare:

- the complete ordered slot-0 organism projections;
- resonance strength bits and ordered node data at the same player/anchor
  input;
- a capture performed at a High-only extra-slot position (exact `Anchor`
  equality across all three tiers); and
- one multi-node expedition recorded at the same integer waypoints.

For the route gate, settle every waypoint under its tier before the recorder
observes it, feed the map update's canonical frame strength, and use identical
travel increments, explicit anchors, discoveries, sequence, name, and journal.
Close the route through the real `RouteRecorder`/`Vault` path, retrieve the
actual `RouteRecord`, and compare:

```text
record.nodes
record.id
encode_record(RecordKind::Route, &record)
```

The encoded `Vec<u8>` equality is the decisive gate. Comparing only
`route_difficulty`, `cost_q`, ids, or decoded fields is insufficient because
the requirement is byte-identical shared records.

Retain the density-realization scenario's approximate 4x population, slot-0 id
subset, and uniqueness checks so the correction cannot “pass” by disabling
higher-slot presentation.

### 5.4 Regression expectations

- `wer-anchor` retains stationary, compatibility, and bounded-resonance gates.
- `wer-vault` and the existing route integration test remain green without a
  record migration or golden update.
- Continuity and schedule-independence comparisons remain equal under their
  documented schedules.
- High-tier density still satisfies ecology coherence and full presentation
  counts.
- Native organism rendering/hit testing continues to include higher slots.
- No diff appears in determinism goldens, record fixtures, version constants,
  layer declarations, or historical plan files.

## 6. Exact file set

| File | Planned change |
|---|---|
| `crates/world-runtime/src/realize.rs` | Add explicit `Organism::slot`, populate it without changing identity/RNG math, and add slot-subset tests. |
| `crates/world-runtime/src/stream.rs` | Split fixed canonical realization from budgeted visual expansion; maintain two key states over one vector; add authoritative iterators; filter capture/resonance; update lifecycle retirement, update order, stats, and focused tests. |
| `crates/world-runtime/src/resonance.rs` | Define/document fixed `MAX_RESONANCE_NODES = 64` and update graph docs/tests. |
| `crates/world-runtime/src/budget.rs` | Remove `max_resonance_nodes`; define `max_realize_organisms` as visual-expansion work only. |
| `crates/world-runtime/src/tier.rs` | Remove 96/128 resonance presets and state that tier density is presentation-only. |
| `crates/world-runtime/src/lib.rs` | Re-export the fixed resonance constant and any authoritative iterator/stat API needed by harnesses. |
| `crates/world-runtime/tests/streaming.rs` | Add cross-budget/cross-density authoritative availability, capture, resonance, fixed-cap, and lifecycle regressions. |
| `crates/tools/src/anchor.rs` | Replace variable-cap harness assumptions with the fixed canonical cap. |
| `crates/tools/src/ecology.rs` | Distinguish fixed canonical realization telemetry from budgeted visual expansion while retaining density/coherence gates. |
| `crates/tools/src/ledger.rs` | Remove the deleted budget field from explicit unlimited literals. |
| `crates/tools/src/replay.rs` | Remove the deleted budget field, fold `Organism::slot` into the same-platform state hash, and require canonical completion in fixed-point settling. |
| `crates/tools/src/scale.rs` | Remove cap scaling, require canonical completion in fixed-point/queue settle predicates, extend tier identity with exact canonical capture/resonance and actual encoded route bytes, and retain visual-density gates. |
| `crates/tools/src/vault.rs` | Gate the scripted save point on complete canonical state and drain fixed canonical publication at zero travel after restore with explicit diagnostics. |
| `crates/tools/tests/route.rs` | Add/strengthen an end-to-end Low/Mid/High route-byte regression if the scale gate's fixture is cleaner to share here. |
| `crates/tools/tests/persistence.rs` | Let save→load settling drain the fixed one-region canonical publication schedule at zero travel before continuing the exact replay. |
| `crates/platform-native/src/panel.rs` | Report fixed canonical versus budgeted visual realization work without making Low tier appear idle. |
| `crates/platform-native/src/main.rs` | Clarify that session loading guarantees the first zero-travel update while exact canonical availability requires holding still until the fixed publication drain completes. |
| `docs/adr/0024-gameplay-uses-one-canonical-organism-slot.md` | Record canonical slot-0 gameplay, fixed admission/cap, visual-only extra slots, and the schedule/cross-platform boundaries. |
| `docs/adr/README.md` | Index ADR 0024 (renumber if main gains the number before rebase). |
| `docs/world-model.md` | Update the abstract/data flow, capture, resonance, routes, realization, budget/tier, verification text; resolve finding 6 and mark A.5 completed. |
| this plan | Change status to `Completed` only after implementation and every gate pass. |

`platform-native/src/main.rs` and `world-runtime/src/route.rs` should need only
comment changes, if any: the route recorder signature intentionally remains
stable. If compilation reveals another `Budget` struct literal containing the
deleted field, include that mechanical caller migration and record it in this
table before commit. Do not expand into unrelated cleanup.

## 7. ADR and versioning decision

A new ADR is required. This correction changes which transient organisms are
authoritative gameplay input, removes a purported budget knob because it is
semantic, and tightens the resource-tier identity contract. Those are durable
architecture decisions, not implementation details.

Create ADR 0024, currently the next free number, titled approximately
“Gameplay uses one canonical organism slot; resource-tier density is visual.”
It should:

- build on ADRs 0006, 0010, 0011, 0012, 0013, 0015, 0018, 0019, and 0023;
- supersede ADR 0012's decision-2/decision-4 assumption that the entire
  tier-scaled realized set and budget-selected cap feed resonance, while
  preserving multiplicative travel gating and presentation-grade
  cross-platform status;
- tighten ADR 0018 decision 3 so `organisms_per_cell` is allowed to change
  displayed density but never capture, resonance, convergence, or shared route
  bytes;
- retain ADR 0018's prerequisite-readiness/mid-flight schedule caveat; and
- state that future gameplay entity systems must choose an explicit canonical
  population/biomass model rather than silently consuming visual density.

Do not edit the accepted ADRs in place. Add the successor/refinement only.

No version bump or migration is warranted:

- organism feature ids and generator output for every dependency key are
  unchanged;
- slot labeling and gameplay filtering affect transient selection, not world
  generation identity;
- route *values* may correct previously tier-dependent `cost_q` and content ids,
  but the record schema and fold algorithm are unchanged; and
- hand-authored codec/determinism fixtures remain valid and must not be
  re-blessed.

## 8. Implementation sequence

1. Add `Organism::slot`, its realization tests, and authoritative iterator
   helpers while preserving all full-presentation callers.
2. Split realization keys/lifecycle and implement the fixed one-region slot-0
   stage before resonance plus budgeted visual expansion after integration.
3. Filter capture and resonance to slot 0; introduce fixed 64 and remove the
   `Budget`/tier cap field from every caller.
4. Update telemetry, panel, anchor/ecology/replay tools, and state hashing.
5. Add focused runtime cross-budget/cross-density/cap/lifecycle tests.
6. Extend `wer-scale` tier identity and route integration with exact capture,
   resonance, route record, and encoded-byte equality; retain visual density
   gates.
7. Add ADR 0024/index and update current `world-model.md`; mark the roadmap,
   finding, and this plan completed only after all gates are green.
8. Run focused tests, harnesses, and the complete CI-equivalent native/wasm
   validation matrix.

## 9. Validation commands

Run from `/tmp/wer-improvement-a5` with the pinned toolchain (source Cargo's
environment first if necessary):

```sh
cargo test -p world-runtime realize
cargo test -p world-runtime --test streaming
cargo test -p world-runtime
cargo test -p tools --test route
cargo test -p tools scale::tests::quick_harness_passes
cargo run --bin wer-anchor
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

Also verify the immutable/history boundaries:

```sh
git diff -- docs/plans/prototype/implementation-plan.md 'docs/plans/prototype/phase-*-plan.md'
git diff -- docs/plans/prototype/perf-baseline.md crates/world-core/tests/determinism.rs
git diff -G 'WORLD_ALGORITHM_VERSION|RECORD_FORMAT_VERSION|algorithm_revision'
```

All commands must show no unintended historical, golden, or version change.

## 10. Documentation completion edits

After all implementation and tests pass:

- Change roadmap A.5 to **Completed**, link this plan, and summarize explicit
  slot labels, fixed canonical publication/cap, slot-0-only gameplay, and the
  encoded route-byte gate.
- Rename finding 6 as resolved. Retain the original failure description for
  auditability and append the concrete resolution and tests.
- Update Sections 2.1/2.7/2.9 and 3.3/3.5/3.6/3.7/3.20/3.24/3.25/3.28 so the
  model distinguishes canonical gameplay organisms from higher-slot visual
  population, explains the fixed one-region publication schedule and 64-node
  cap, and states the remaining L8-readiness/cross-platform caveats.
- Change the resource-tier table's resonance row from 64/96/128 to a fixed 64
  semantic ceiling, and define `max_realize_organisms` as visual expansion.
- State that route transition cost and therefore route content bytes use the
  canonical frame resonance.
- Update the verification surface to name exact cross-tier capture, resonance,
  and encoded route-record gates while retaining density scaling.
- Change this plan's status from `Planned` to `Completed` last.

Do not claim that all mid-frame schedules or native/wasm live float captures are
identical. The achieved contract is: equal ready authoritative world inputs
produce equal gameplay samples and shared route bytes regardless of resource
tier; optional higher density never participates.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Slot membership inferred incorrectly after density gating | Store `slot` directly on each organism and test density-4's filtered structs against density 1. |
| High tier publishes gameplay samples earlier through its larger visual budget | Separate canonical key/stage with one fixed whole-region admission and require canonical currency before expansion. |
| Old L8/revision sample participates while repair is pending | Retire vector and both keys on mismatch before resonance; fail closed on incomplete rosters. |
| Fixed slot publication causes an unbounded hitch | Publish one nearest whole region per frame; retain pass timing and explicit telemetry. |
| Visual expansion replaces slot 0 with different bits | Reuse identical realization function/RNG path and assert exact filtered equality. |
| Removing `max_resonance_nodes` breaks tests/tools silently | Compile-migrate every struct literal and replace artificial-cap tests with fixed-cap assertions. |
| Route test compares a derived summary instead of shared bytes | Encode the actual retrieved `RouteRecord` and compare the full `Vec<u8>`. |
| “Tier invariant” overclaims executor readiness | Settle every comparison point and document ADR 0018's generation-readiness caveat. |
| Correction accidentally removes higher-slot visuals | Retain renderer/full iterators and the 4x population, additive-id, uniqueness gates. |
| Record or generator identity is casually re-blessed | Explicit no-diff checks for goldens, versions, layer declarations, and historical plans. |

Rollback is one commit: revert the A.5 commit. There is no persistence
migration, fixture re-bless, remote data rewrite, or irreversible state.

## 12. One-commit worktree, merge, and push workflow

Implement only on `codex/improvement-a5-tier-invariance` in
`/tmp/wer-improvement-a5`. The planning agent does not commit. The fresh
execution agent implements and validates the entire plan, stages only the
documented file set, and creates exactly one commit, for example:

```text
Make gameplay sampling invariant across resource tiers
```

Before merge, check the latest local `main`. If it advanced, rebase the single
A.5 commit onto it, resolve without dropping either side, and rerun every
focused and full gate on the rebased tree. Fast-forward merge from the primary
worktree with `git merge --ff-only`, confirm the forbidden historical plans,
performance ledger, goldens, and version constants remain untouched, then push
`main` to `origin`. Do not begin A.6 until that push succeeds.

## 13. Definition of done

A.5 is done only when explicit slot membership is correct; fixed slot-0
publication is independent of tier presentation budgets; capture and fixed-cap
resonance consume only slot 0; Low/Mid/High retain full 1/2/4-slot visuals but
produce exact canonical capture/resonance and actual encoded route bytes;
ADR 0024 and current documentation land; every focused, harness, native, and
wasm gate passes; exactly one commit is fast-forwarded to `main`; and that
commit is pushed successfully to `origin`.
