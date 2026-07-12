# Improvement A.1 — Self-Healing Dependency Integration and Cache Eviction

**Status:** Planned

**Roadmap item:** [Correctness and contract integrity A.1](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [2](../../world-model.md#2-macro-cache-capacity-eviction-can-strand-hydrology-forever), [3](../../world-model.md#3-roster-cache-capacity-eviction-can-make-life-permanently-disappear), and [10](../../world-model.md#10-integration-does-not-revalidate-the-dependency-hash)

This plan implements the first item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md): make missing generated dependencies
recover on demand, keep the roster inputs of resident Ecology tiles available,
and make the current dependency hash—not dirty-bit bookkeeping—the final
provenance check for every completed generation job.

The work is complete only when the implementation, focused recovery tests,
full regression suite, architecture record, and the final `world-model.md`
edits have landed together. The roadmap item must remain open while any of
those gates is incomplete.

This is a post-prototype corrective plan. It must not modify
[`implementation-plan.md`](implementation-plan.md) or any
`docs/plans/prototype/phase-N-plan.md` file; those files remain the historical
record of the original prototype plan.

---

## 1. Outcome and completion criteria

After this work, cache pressure may delay or repeat pure generation work, but
it may not permanently block a resident dependency chain or change the content
of its settled result.

All of the following must hold:

1. If a dirty resident layer needs a missing ordinary tile or a missing/stale
   macro Drainage tile, dependency resolution restores the appropriate dirty
   hint and requests that dependency without waiting for an unrelated
   possibility change, revision bump, eviction, or player movement.
2. Macro-capacity eviction may leave already-fresh Hydrology tiles alone, but
   the next Hydrology invalidation must re-request Drainage and settle the full
   `Hydrology -> Soils -> Biome -> Vegetation -> Ecology` chain.
3. Every habitat signature referenced by a resident region's current or
   in-flight Ecology input set remains available in `RosterCache`. Disposable
   roster entries may be evicted; required entries may not.
4. A roster ceiling smaller than the required resident working set is treated
   as a cache target, not permission to violate correctness. The required set
   is allowed to exceed that target, and the overage is documented and tested.
5. Near-field realization never publishes a partial/empty organism vector as
   current merely because a required roster entry is absent. It defers without
   advancing the region's organism key, and the roster-maintenance pass repairs
   the missing input for a later retry.
6. A completed macro or region-layer job is integrated only when its job id is
   current **and** its dependency hash exactly equals both the dispatch key and
   the hash recursively expected from current authoritative state at
   integration time.
7. If the current expected hash is unavailable or differs, the result is
   dropped, its buffers are reclaimed, its in-flight entry is retired, and the
   affected layer/dependent closure is left in deterministic retryable state.
8. A tight-cache run and a roomy-cache reference settle to identical tile
   content and dependency hashes for the same state. Recovery remains correct
   with cancellation enabled or disabled and under a finite generation budget.
9. No generation formula, dependency-hash fold, record format, stable identity,
   or persistent byte changes. `WORLD_ALGORITHM_VERSION` remains 2, every
   `algorithm_revision` remains 0, `RECORD_FORMAT_VERSION` remains unchanged,
   and no golden fixture is re-blessed.
10. The native CI-equivalent checks and the neutral/web `wasm32` checks pass,
    and `world-model.md` marks roadmap item A.1 and findings 2, 3, and 10 as
    completed/resolved without claiming that the separate field-history or
    hard-memory-budget roadmap items are complete.

## 2. Scope boundaries

### 2.1 In scope

- Demand repair of missing declared generation inputs in `RegionMap`.
- On-demand recovery of capacity-evicted macro Drainage tiles.
- A protected resident working set for `RosterCache`.
- A fail-closed realization precondition for roster completeness.
- Integration-time dependency-hash validation for macro and region-layer
  results.
- Deterministic rejection, buffer reclamation, dirtying, cancellation, and
  retry behavior after validation fails.
- Focused unit/integration tests for every layer result shape and the full
  downstream recovery chain.
- An ADR that records the acceptance and indispensable-working-set policies.
- Current-state documentation and roadmap status updates in `world-model.md`.

### 2.2 Explicitly out of scope

- Separating `RegionState` from field-tile eviction or retaining
  transformation history across field-capacity eviction. That is roadmap A.4
  (findings 4 and 5).
- Turning logical payload estimates into hard process-memory caps. That is
  roadmap B.3 (finding 32). This plan deliberately permits a protected roster
  working set to exceed its nominal cache target.
- Changing cache victim ordering, resource-tier values, field resolution, or
  generation math except where needed to exclude protected roster entries.
- Executor panic recovery, queue fairness, or shutdown semantics.
- Ecology-model, organism-identity, transition-mode, persistence, route, or
  renderer changes.
- Editing an accepted ADR in place, editing an original prototype/phase plan,
  changing goldens, or bumping an algorithm/record version.

## 3. Current failure map

| Finding | Existing path | Failure |
|---|---|---|
| 2: macro eviction | `RegionMap::enforce_capacity` removes a `MacroCache` tile; `dispatch_region` calls `check_macro` only for a set Drainage bit; `inputs_fresh` merely returns false for dirty Hydrology | Hydrology can wait forever after its macro input was removed while the Drainage hint stayed clean. |
| 3: roster eviction | `build_ecology_rosters` records `region_signatures`, but `RosterCache::evict_to_bytes` ignores them; `cell_ecology` and `realize_region_into` use lookup-only `get` calls | A fresh L8 tile can lose a required roster. Realization can then skip affected cells and still record the unchanged L8 hash, preventing retry. |
| 10: stale integration | `integrate_finished` checks current job id and, for region tiles, the dirty bit; `GeneratedTile::dep_hash` is not compared with `expected_layer_hash` | A bookkeeping omission can publish a result generated from inputs that are no longer current, contradicting ADR 0008's ground-truth contract. |

The useful existing seams should be retained:

- `layer_decl`, topological layer ids, `layer_bit`, and
  `dependents_closure` already describe repair and retry ordering.
- `expected_macro_hash` and `expected_layer_hash` are the starting points for
  the authoritative keys the integrator needs.
- `region_signatures` already identifies the required roster working set.
- job ids still reject results from superseded dispatches or evicted/reloaded
  coordinates.
- cancellation tokens still save worker time, while the main-thread
  integrator remains the only cache writer.
- `TilePool` already has the reclamation path for rejected region results.

## 4. Required design

### 4.1 Separate dispatch identity from content provenance

Use three related gates, in this order:

1. **Dispatch identity:** the result's job id must match the current
   `InFlightJob` entry. This rejects late work from a superseded dispatch or an
   evicted/reloaded coordinate.
2. **Dispatch provenance:** the result's dependency hash must equal the
   expected key stored in that `InFlightJob` at submission. This verifies that
   the result describes the snapshot the scheduler actually dispatched.
3. **Current provenance:** the result's dependency hash must equal the
   currently expected hash recomputed on the main thread. This rejects work
   whose dispatch was once legitimate but whose authoritative inputs are no
   longer current.

The dirty bit remains a conservative scheduling hint. A clear bit is not
evidence that a result is current, and a set false-positive does not make a
result stale when both provenance keys still match. Cancellation remains an
optimization only. Neither mechanism replaces dependency-hash comparison.

Refactor expected-key calculation so cache absence does not make authoritative
provenance unknowable. For a resident region, derive each ordinary layer's
expected key recursively from:

- current quantized region domains;
- the layer and effective algorithm revisions;
- field resolution; and
- the **expected** keys of declared inputs in declaration order.

For the Drainage input use `expected_macro_hash`, not a resident macro tile.
For an ordinary input recurse through the lower-id DAG, not through the cached
tile's stored key. Memoize the nine results in a small per-region array during
a scan if profiling shows repeated recursion matters; do not introduce a
persistent second provenance store.

This calculation is distinct from readiness. Dispatch still requires the
actual input tile to be present and to carry the recursively expected key (or
the matching expected-key job to land first). Integration can validate a job
whose immutable `Arc` snapshot is still valid even if capacity eviction has
since removed a cache entry. Root-layer keys derive directly from current
region buckets, revision, and resolution.

For a macro result, compare `DrainageTile::dep_hash` directly with
`expected_macro_hash(coord)`. The expected macro key is independent of live
region tiles and remains computable from the macro coordinate and effective
algorithm revisions.

Do not overwrite any cached channel until all acceptance checks for the whole
result have passed. All channels of one layer remain an atomic integration
unit. Extend `InFlightJob` with its expected dependency key so these checks and
the repair pass can distinguish current work from obsolete work without
guessing from a dirty bit.

### 4.2 Make rejection retryable

Centralize stale-result rejection so all paths perform the same cleanup:

- remove the matching in-flight entry because that dispatch has completed;
- increment `FrameStats::results_dropped`;
- reclaim every owned tile buffer for a rejected `GeneratedTile`;
- leave the previous cached layer untouched;
- mark the rejected layer and its transitive dependents dirty;
- set affected resident regions to `GenerationStatus::Generating`;
- cancel/remove any now-doomed dependent jobs through the existing
  cancellation path; and
- let the normal cost-budgeted topological dispatcher retry the work.

For a rejected macro result, apply the Drainage dependent closure to every
resident level-0 region covered by that macro coordinate. If no covered region
is resident, no retry is needed. For a rejected region result, apply the
closure only to that result's region.

Marking the closure, rather than only the rejected layer, makes recovery robust
when the very bookkeeping omission being defended against also failed to dirty
an already-dispatched dependent. False-positive dirty hints are harmless: the
existing stored-versus-expected comparison clears them without regeneration.

### 4.3 Repair missing dependencies before testing readiness

Add a `RegionMap` helper that closes a dirty region's work set over missing
declared inputs before the ascending layer scan. Conceptually:

```text
repeat until no bits/jobs change:
    for each dirty consumer layer:
        for each actual cached input:
            derive the input's authoritative expected key
            if the cached key matches, the input is ready
            else if an in-flight job carries that expected key, wait for it
            else cancel obsolete input work and add the input's dirty bit
        apply the same rule to a missing/stale Drainage macro
```

Important details:

- Run this repair before `dispatch_region` scans ids `0..LAYER_COUNT`, so any
  newly restored lower-id hint can be submitted in the same fixed-point pass.
- Treat a missing or unequal cached dependency key as repairable, not just a
  missing allocation. This also heals a stale clean input left by a bookkeeping
  defect.
- Do **not** re-dirty a dependency when its `InFlightJob` already carries the
  current authoritative key. Dispatch clears dirty bits, so failing to make
  this distinction would repeatedly reject correct asynchronous work.
- If an in-flight dependency carries an obsolete key, cancel/remove it through
  the existing path and dirty it for immediate redispatch.
- Do not require a Terrain tile to run macro Drainage. Drainage's declared
  Terrain edge carries the Terrain **algorithm revision**; the macro generator
  does not consume a resident Terrain tile.
- Add only dirty/status bookkeeping in this helper. Actual generation still
  goes through `check_macro`/`submit_layer`, so priority, costs, budgets,
  cancellation, immutable snapshots, and deterministic ordering remain
  unchanged.
- Strengthen the final dispatch-readiness predicate to require each cached
  input's stored key to equal its recursively expected key, in addition to
  presence and no pending replacement. If a budget prevents a repaired
  dependency from being submitted this frame, its bit remains set and the next
  update retries it normally.

This demand-driven repair is preferable to eagerly regenerating every macro at
eviction time: a fresh Hydrology tile does not need its macro snapshot merely
to remain readable. Regeneration occurs only when a dirty consumer actually
needs the missing input.

### 4.4 Protect and repair the resident roster working set

Define the indispensable roster set as the union of `region_signatures` for
all currently resident regions. Continue recording a region's complete set at
Ecology dispatch, before the worker receives its immutable `RosterSnapshot`.

Refactor the roster-capacity path as follows:

1. Build that required set once in deterministic signature order.
2. Call `RosterCache::ensure` for every required signature. This repairs any
   missing entry left by old behavior or an interrupted lifecycle without
   forcing an L8 invalidation.
3. Evict only entries not in the required set, in the existing deterministic
   reverse-signature order, until the byte target is met or no disposable
   entry remains.
4. If required entries alone exceed the target, retain all of them and report
   their actual bytes through the existing `roster_cache_bytes` telemetry.

Change `RosterCache::evict_to_bytes` to accept the protected set and return
useful eviction information (at minimum the count or bytes removed) for unit
tests and diagnostics. Its documentation must call the limit a target with a
required-working-set floor. Use one `RegionMap` helper to derive the union for
both orphan sweeping and capacity enforcement so the two paths cannot acquire
different definitions of "needed."

The immutable `Arc<RosterEntry>` values already captured by an in-flight L8
job remain safe. Protecting the corresponding signature in the map also makes
the entry available to later synchronous readers after integration.

### 4.5 Fail closed before near-field realization

Before clearing/reusing an organism vector, verify that every signature tracked
for that region is present in `RosterCache`.

- If the set is complete, realization proceeds unchanged.
- If any signature is absent, do not call `realize_region_into`, do not replace
  the old organism vector, and do not write `organism_keys[coord]`.
- The next update's roster-working-set maintenance rebuilds the missing pure
  entry, after which the normal realization pass retries.

Keep `cell_ecology(&self, ...)` lookup-only. Under the maintained invariant it
continues to succeed for a settled resident L8 cell; if the invariant is ever
broken transiently, returning `None` is safer than hiding mutation in a read
API. Tests must cover both inspection and realization, because finding 3 names
both consumers.

### 4.6 Determinism and versioning

This is scheduler/cache correctness work. It changes which stale result is
allowed to publish and whether a pure dependency is retained/recomputed; it
does not change the value generated for a valid dependency key.

Therefore:

- do not change `layer_dep_hash`, `drainage_dep_hash`, hash fold order, layer
  declarations, generator arithmetic, or stable ids;
- do not bump `WORLD_ALGORITHM_VERSION`, a layer `algorithm_revision`, or
  `RECORD_FORMAT_VERSION`;
- do not edit or re-bless determinism or record golden fixtures; and
- compare pressure-run **content hashes as well as dependency hashes**, so a
  test cannot pass merely because both runs arrived at the same key metadata.

## 5. Architecture record

Add the next available ADR (expected to be
`docs/adr/0019-dependency-hashes-gate-integration.md`) and add it to
[`docs/adr/README.md`](../../adr/README.md). Do not edit accepted ADRs 0008 or
0018 in place.

The ADR should record:

- ADR 0008's dependency key is the integration-time provenance authority;
- job id protects dispatch identity, while dirty bits and cancellation are
  hints/optimizations;
- unavailable or unequal current keys cause deterministic rejection and
  requeue;
- missing dependencies are repaired on demand through the declared DAG;
- resident roster signatures form an indispensable working set protected from
  capacity eviction; and
- a logical cache target yields to that working-set floor.

This ADR builds on
[`0008-tiles-are-functions-of-their-dependency-hash.md`](../../adr/0008-tiles-are-functions-of-their-dependency-hash.md)
and narrows the statement in
[`0018-settled-state-is-schedule-independent.md`](../../adr/0018-settled-state-is-schedule-independent.md)
that calls the job-id check the correctness gate: job id remains necessary for
dispatch identity, but the dependency key becomes the content-provenance gate.

## 6. File-by-file implementation plan

### 6.1 `crates/world-runtime/src/stream.rs`

- Refactor expected-layer-key calculation to fold recursively expected input
  keys rather than requiring cached input tiles. Keep a separate readiness
  check for the actual cache/snapshot prerequisites of dispatch.
- Extend `InFlightJob` with the dependency key captured at dispatch and update
  both macro and ordinary submission sites to populate it.
- Add a helper that derives the union of resident `region_signatures`.
- Add roster-working-set maintenance and invoke it from capacity enforcement
  before near-field realization can read the cache.
- Pass the protected set into roster capacity eviction and preserve required
  entries even when their bytes exceed the target.
- Add the missing/stale-dependency closure pass before each region's
  topological dispatch scan. Recognize a matching-key in-flight job as pending
  valid input and cancel/redirty only an obsolete-key job.
- Keep `check_macro` as the only macro submission path, but make it reachable
  from a dirty consumer whose Drainage hint was initially clean.
- Add small helpers for marking a layer/dependent closure dirty and for marking
  all level-0 dependents of one macro coordinate. Reuse them from rejected
  integration paths to keep status/cancellation handling consistent.
- Rework `integrate_finished` so neither a macro tile nor any channel of a
  region layer is inserted before the job id, dispatch key, result key, and
  recursively current expected key agree.
- On rejection, retire/reclaim/redirty exactly as specified in section 4.2.
- Add the roster-completeness preflight in `realize_near_window` and advance an
  organism key only after a complete realization.
- Update comments on `JobResult`, `InFlightJob`, dispatch, integration,
  eviction, and realization so none calls job id or dirty state the sole
  correctness gate.

### 6.2 `crates/world-runtime/src/rostercache.rs`

- Change capacity eviction to accept a protected signature set.
- Select only unprotected victims while preserving deterministic
  reverse-signature order; do not stop merely because the greatest signature
  happens to be protected.
- Return an eviction count/byte total suitable for assertions.
- Document the soft-target/required-floor behavior.
- Add unit tests for mixed protected/disposable entries, a zero-byte target,
  all-protected overage, deterministic victim order, and rebuilding a removed
  required entry with identical content.

### 6.3 `crates/world-runtime/src/generate.rs`

- Update `GeneratedTile::dep_hash` documentation: the field is validated
  against current expected provenance at integration, not trusted through
  dirty bookkeeping.
- Keep generation behavior and output structs otherwise unchanged.

### 6.4 `crates/world-runtime/src/realize.rs`

- Update function/module documentation to state that the runtime verifies a
  complete resident roster set before publishing a realization.
- Do not change sampling, random-stream consumption, identities, density, or
  expression math.
- Retain lookup-only behavior inside the pure realization function; the
  coordinator owns completeness and retry policy.

### 6.5 Tests

Use focused tests in
[`crates/world-runtime/tests/streaming.rs`](../../../crates/world-runtime/tests/streaming.rs)
for public behavior and private unit tests beside `RegionMap` where an
intentional bookkeeping fault must be injected. Avoid adding a production
cache-corruption API solely for testing.

The exact cases are defined in section 7.

### 6.6 Documentation

- Add the ADR and index entry described in section 5.
- Update [`docs/world-model.md`](../../world-model.md) only after the code and
  verification gates pass, following section 9.
- Set this plan's status to **Completed** in the same final documentation
  change.
- Do not edit `docs/plans/prototype/implementation-plan.md`, any
  `docs/plans/prototype/phase-N-plan.md`, or the Phase 6 performance baseline;
  this work neither rewrites the historical plan nor changes benchmark
  baselines.

## 7. Verification plan

### 7.1 Characterization tests first

Add failing regressions before changing production behavior:

1. **Stranded macro reproduction.** Settle a small map, force the covering
   macro tile out through a sub-tile/zero macro ceiling while leaving resident
   regions and fresh Hydrology in place, then invalidate Hydrology without
   dirtying Drainage. The pre-fix map must fail to settle.
2. **Roster-loss reproduction.** Settle L8 and its organisms, pressure the
   roster cache below the required set, and show that a required signature can
   disappear while `region_signatures` and the L8 key remain unchanged.
3. **Stale-result reproduction.** Use a test-only manual executor and private
   test access to queue a legitimate job, change its effective expected key
   without setting the corresponding dirty hint (simulating the bookkeeping
   omission finding 10 is about), execute it, and show that the pre-fix
   integrator accepts it.
4. **Missing-edge reproduction.** Remove one cached producer while leaving its
   dirty bit clean, dirty only a direct consumer, and show that the pre-fix
   scheduler defers the consumer without restoring the producer's work.

Keep fault injection inside `#[cfg(test)]` code. Normal public mutation hooks
such as `bump_layer_revision` correctly dirty/cancel work and therefore do not
exercise the missing-defense case by themselves.

### 7.2 Integration-hash matrix

Parameterize the stale-result test across every result shape:

| Result | Required assertion |
|---|---|
| Macro Drainage (L2) | Old macro key is rejected; all covered resident Drainage closures become retryable; no stale macro replaces the cache. |
| Terrain (L0) | Root key is recomputed from current buckets/revision; stale `f32` output is rejected. |
| Geology (L1) | Same root-layer behavior for the second independent result. |
| Climate (L3) | A stale ordinary dependent result is rejected before its two channels integrate. |
| Hydrology (L4) | Missing/stale macro or changed input key rejects both river and wetness atomically. |
| Soils (L5) | Multi-input dependent rejection leaves depth/fertility untouched and retries its closure. |
| Biome (L6) | The `u8` tile follows the same provenance gate. |
| Vegetation (L7) | Both vegetation channels are atomic under rejection/retry. |
| Ecology (L8) | Three `f32` channels plus the `u16` dominant tile are atomic; its roster snapshot does not weaken the key check. |

For each case assert:

- `results_dropped` increments;
- the previous cached hash/content remains unchanged before retry;
- no partial channel replacement occurred;
- the matching in-flight entry is gone;
- the proper dependent closure is dirty/generating;
- a later normal dispatch settles; and
- every settled diagnostic has `stored == expected`.

Run representative cases with cancellation both enabled and disabled. The
disabled case proves correctness does not depend on a token preventing the
stale closure from running.

### 7.3 Declared-edge repair matrix

Add a table-driven private runtime test that iterates every actual cached edge
in `world_core::layer::LAYERS`:

- settle a small region;
- remove only the selected producer's output while keeping its dirty bit clear;
- dirty the direct consumer only;
- leave a current producer job in flight in one variant, and an obsolete-key
  producer job in flight in another;
- run normal budgeted dispatch to quiescence; and
- assert the producer is requested exactly when needed, the correct in-flight
  job is not self-superseded, the obsolete job is rejected/replaced, and final
  content/keys match the pre-removal oracle.

Cover Terrain inputs to Climate, Hydrology, Soils, and Biome; Geology to Soils;
Climate to Hydrology, Soils, Biome, Vegetation, and Ecology; Hydrology to Soils
and Biome; Soils to Biome, Vegetation, and Ecology; Biome to Vegetation and
Ecology; and Vegetation to Ecology. Handle Drainage-to-Hydrology through the
macro-ceiling case below. Do not treat Drainage's Terrain-revision edge as a
resident Terrain-tile input.

This matrix proves the roadmap's “re-request any missing dependency” language,
rather than only special-casing the reported macro failure.

### 7.4 Tight macro-ceiling recovery

Create a public streaming regression with a ceiling smaller than one macro
tile:

1. Settle a small fixed window and capture a roomy-cache reference snapshot of
   per-layer dependency hashes and content hashes.
2. Run an idle capacity pass on the pressured map and assert that a macro input
   is absent while its resident Hydrology tile remains fresh.
3. Bump only the Hydrology effective revision (or use another precise
   Hydrology-only invalidation), leaving the Drainage hint initially clean.
4. Update under a finite cost budget until quiescent, accumulating
   `regenerated_by_layer`.
5. Assert that Drainage was requested, Hydrology and every downstream layer
   L5–L8 made progress, no region remains permanently `Generating`, and all
   stored keys equal their expected keys.
6. Compare final content and keys with a roomy-cache map subjected to the same
   revision change.

Stop the test when the recovered fixed point is observed rather than running an
extra idle frame that is allowed to evict the now-unneeded macro again. The
contract is demand recovery, not permanent retention of every macro tile.

### 7.5 Tight roster-ceiling and realization recovery

Use a zero/sub-entry roster target so the required working set necessarily
exceeds it:

- settle Ecology for multiple resident signatures;
- assert that every signature obtained from every settled resident cell is
  still present in `RosterCache` after repeated capacity passes;
- assert actual roster bytes may exceed the configured target and stabilize at
  the required-set floor;
- assert `cell_ecology` succeeds for settled cells;
- move a previously far-but-resident region into the near window and assert its
  organisms are realized from species in the correct roster;
- compare roster content, L8 content, organism ids, and organism expressions
  with a roomy-cache reference; and
- in a private unit test, clear the roster cache while retaining
  `region_signatures`, force a realization retry, and prove maintenance rebuilds
  the exact entries before `organism_keys` advances.

Also test that obsolete, unprotected signatures remain evictable; otherwise
the correctness fix would silently disable the roster capacity policy.

### 7.6 Existing regressions that must remain unchanged

- dependency invalidation precision (`wer-ledger`);
- continuity and two-run determinism;
- ecology coherence and organism stability;
- anchor/resonance behavior;
- vault save/load exactness;
- scale/schedule/cancellation/tier gates;
- world-core determinism and record byte goldens; and
- native/wasm parity compilation.

Do not weaken an existing assertion to accommodate the fix. No golden output
should need an update.

## 8. Implementation sequence

### Milestone 1 — Contract and failing regressions

- Add the ADR in Proposed form and its index entry.
- Add the macro-stranding, declared-edge, protected-roster, and stale-result
  characterization tests.
- Confirm the tests fail for the documented reasons, not because of timing or
  an impossible ceiling invariant.

**Exit:** each of findings 2, 3, and 10 has a direct reproducible test.

### Milestone 2 — Dependency repair and provenance-gated integration

- Add missing-input closure repair before topological scanning.
- Refactor recursively authoritative expected keys and store the dispatch key
  in each in-flight entry.
- Add main-thread dispatch/current-key validation for macro and region results.
- Add shared rejection/redirty/cancel/reclaim helpers.
- Run the every-layer result matrix with cancellation on and off.

**Exit:** no stale result shape can integrate, and every rejected result can
settle through the ordinary dispatcher.

### Milestone 3 — Resident roster working set

- Refactor protected roster eviction and resident-set derivation.
- Ensure missing required entries before evicting disposable entries.
- Add realization completeness preflight and retry semantics.
- Run the zero-ceiling inspection/realization tests.

**Exit:** capacity pressure cannot remove resident ecology inputs or publish a
partial realization, while unused signatures still evict deterministically.

### Milestone 4 — End-to-end recovery and regression gates

- Run the tight macro-ceiling/full-dependent-chain comparison against the
  roomy reference under a finite budget.
- Assert `stored == expected` for every settled layer in recovery scenarios.
- Run all native sign-off harnesses and CI-equivalent checks.
- Run the `wasm32` checks for neutral crates and `platform-web`.
- Confirm versions, revisions, and golden fixtures are untouched.

**Exit:** all criteria in section 1 except final documentation are satisfied.

### Milestone 5 — Current-state documentation and completion

- Promote the ADR to Accepted if implementation matches it.
- Update `world-model.md` exactly as described in section 9.
- Mark this plan and roadmap item A.1 completed only now.
- Review the final diff to confirm no original implementation/phase plan was
  changed.

**Exit:** code, tests, ADR, current model description, and roadmap status agree.

## 9. Required `world-model.md` update

Once Milestone 4 passes, update the following current-state sections. Preserve
the detailed findings as review history, but clearly distinguish resolved
problems from open ones.

### 9.1 Model and runtime description

- **Section 2.6, generated dependency graph:** state that completed results, as
  well as stored tiles, are checked against the currently expected dependency
  key; job id is the dispatch-identity gate and dirty bits are hints. Mention
  that expected keys derive recursively from authoritative state even when a
  cache entry is absent, while dispatch separately demand-repairs missing or
  stale materialized inputs.
- **Section 3.17, habitat signatures and roster caching:** describe the union
  of resident `region_signatures` as protected, deterministic eviction of only
  disposable entries, repair through `ensure`, and allowed overage when the
  required set exceeds its target.
- **Section 3.20, near-field realization:** state the complete-roster preflight
  and that an incomplete input defers without advancing the L8 organism key.
- **Section 3.23, incremental generation and job integration:** update the
  dispatch list to request missing dependencies before deferral. Replace the
  current acceptance sentence with the two-gate job-id/current-dependency-hash
  rule and describe rejection/requeue behavior.
- **Section 3.25, resource tiers/caches/pools:** remove the statement that macro
  and roster capacity paths still have correctness gaps. Describe macros as
  demand-rebuilt and roster targets as having a protected resident floor.
- **Section 3.28, verification surfaces:** name the focused macro-ceiling,
  roster-ceiling, every-layer stale-result, cell-inspection, and realization
  recovery tests. Attribute them to the runtime test suite unless a harness is
  actually extended.

### 9.2 Roadmap and findings

- Add a short convention to Section 4 saying resolved findings remain in the
  document for auditability and are explicitly labeled.
- Mark roadmap A.1 unambiguously as **Completed**, retain its references to
  findings 2, 3, and 10, and link to this plan. Rewrite the summary in past
  tense to name the landed repair and tests. Do not renumber later items.
- Mark detailed findings 2, 3, and 10 **Resolved**. Retain a concise statement
  of the former failure, then record the selected implementation and the
  regression that closes it.
- In finding 33, remove macro/roster cache-recovery and independent settled-key
  assertions from the list of missing checks only to the extent the new tests
  actually cover them. Keep broader all-cache-ceiling, frame-slicing, and full
  state-hash gaps open for roadmap A.13.

### 9.3 Claims that must remain open

Do not use this completion edit to claim any of the following:

- field-capacity eviction preserves `RegionState` history;
- cache byte targets are hard heap/process limits;
- every cache is pressured simultaneously by `wer-scale` unless such a
  scenario was actually added;
- worker panic/loss recovery is solved; or
- all of finding 33 is resolved.

## 10. Validation commands

Run from the repository root (source `$HOME/.cargo/env` first if needed):

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown

cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale
```

Record `git status --short` before implementation, then finish with a targeted
diff/version audit:

```sh
git diff --check
git diff -- crates/world-core/src/lib.rs crates/world-core/src/layer.rs crates/world-core/tests/determinism.rs
git diff -- docs/plans/prototype/implementation-plan.md 'docs/plans/prototype/phase-*-plan.md'
```

The task must add no changes to the files in the last two diffs. Preserve and
exclude any pre-existing user changes rather than reverting them. The
task-owned implementation diff should be limited to runtime cache/scheduling
code, focused tests, the new ADR/index, this improvement plan's status, and
`docs/world-model.md`.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Cache absence makes a valid current key unknowable or causes an async eviction/retry loop. | Derive expected keys recursively from authoritative state; use cache presence/equality only for dispatch readiness, and record each in-flight job's key. |
| Rejected work becomes permanently clean after its in-flight entry is removed. | Centralize rejection and always dirty the layer's full dependent closure before returning. |
| A stale upstream result races with dependent jobs when cancellation is off. | Dependency-key validation is independent of cancellation; rejection dirties/cancels the closure, and cancellation-off tests execute the stale jobs deliberately. |
| Missing-input repair self-supersedes a correct job or bypasses budgets. | Treat a matching-key in-flight job as valid pending work, cancel only obsolete-key jobs, add bits in a finite DAG, and submit through existing budgeted paths. |
| Macro eviction causes eager regeneration thrash. | Repair only when a dirty consumer demands Drainage; a fresh Hydrology tile may coexist with an absent macro. |
| Protecting roster entries makes the nominal ceiling unattainable. | Define and document the required-set floor; evict all disposable entries first and expose actual bytes. Hard heap caps remain finding 32. |
| Realization still records a partial vector. | Preflight the whole tracked signature set and update `organism_keys` only after complete realization. |
| A test proves metadata equality but not world equality. | Compare content hashes, roster contents, and organisms against a roomy-cache reference in addition to dependency hashes. |
| The documentation overstates completion. | Update only the listed claims, label the three findings resolved, and leave A.4, A.13, B.3, and the remaining finding 33 gaps open. |
| Correctness changes accidentally alter stable output/versioning. | Keep generators/hashes untouched, run existing goldens unmodified, and audit version/layer/golden diffs before completion. |

## 12. Definition of done

The improvement is done when:

- all ten criteria in section 1 hold;
- every test in section 7 exists and passes;
- all commands in section 10 pass with warnings denied;
- the accepted ADR and index describe the landed behavior;
- `world-model.md` describes the new behavior, labels findings 2/3/10
  resolved, and marks roadmap A.1 completed with a link to this plan;
- this plan's status is Completed; and
- the historical implementation and phase plans, algorithms, versions,
  revisions, and golden fixtures remain untouched.
