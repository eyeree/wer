# Improvement A.4 — Authoritative Regional History Independent of Field Memory

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.4](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [4](../../world-model.md#4-resolved-amortized-retargeting-can-violate-the-geometric-near-field-pin) and [5](../../world-model.md#5-resolved-field-capacity-eviction-removes-authoritative-history-not-just-cache)

This plan implements the fourth item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). It separates a region's small,
authoritative transformation history from its disposable generated field
tiles, and it removes geometric stability from the amortized target-refresh
budget. A field-cache ceiling may change which derived tiles are resident and
when pure generation work runs; it must not delete, omit, or pause the
authoritative coordinates that would exist in the same streaming window with a
roomy cache.

The work is complete only when the state-lifecycle refactor, focused recovery
and trajectory tests, scale-harness gates, ADR, current documentation, and all
native/wasm CI checks land together. The roadmap item and both findings remain
open until those gates pass.

This is a post-prototype corrective plan. Do not modify
[`implementation-plan.md`](implementation-plan.md), any
`docs/plans/prototype/phase-N-plan.md`, or the historical performance ledger.

---

## 1. Required outcome and invariants

The implementation must satisfy every invariant below.

1. `RegionMap::regions` remains the ordered authoritative set for the current
   streaming window. It contains `RegionState { current, target, stability,
   revision, ... }` whether or not that coordinate currently owns field tiles.
2. `Budget::max_loads`, geometry, and load/unload hysteresis determine which
   authoritative coordinates are created and removed. Field capacity is not an
   input to that decision. Given the same player script and load budget, tight
   and roomy field caches create the same coordinates in the same order.
3. A capacity victim keeps its `RegionState`. Capacity parking removes only
   disposable field state: regional tiles, region-signature bookkeeping,
   near-field organisms and their keys, and obsolete per-region generation
   work.
4. Authoritative state remains bounded. Crossing `unload_radius` still removes
   the complete `RegionState` and its derived state. Capacity-parked entries do
   not survive the ordinary radius sweep, so this change does not create an
   unbounded history database.
5. Use the existing `GenerationStatus::Unloaded` state to mean that authority
   exists but no field working set is admitted. `Generating` and `Ready` are
   field-active states. Do not add a second coordinate map or cached authority
   copy that can disagree with `regions`.
6. Every authoritative region, including `Unloaded` capacity-parked regions,
   participates in target refresh, stability refresh, and convergence in the
   same deterministic coordinate/distance order as a field-active counterpart.
   Only derived-cache readers and generation dispatch filter on field
   residency.
7. Geometric stability is refreshed for every authoritative region on every
   update before resonance and convergence. A region that crosses into
   `near_radius` is pinned in that same frame even if its target refresh is
   deferred by `max_retarget_regions`.
8. `max_retarget_regions` budgets only steered-target calculation. Its
   `retarget_deferred` telemetry counts deferred targets, never deferred
   geometric stability.
9. A steering-input signature change still refreshes every authoritative
   target immediately. With unchanged steering, the target-only pass remains
   deterministic round-robin over the complete authoritative coordinate set,
   parked entries included.
10. Before a parked region becomes field-active, recompute its target from the
    current field, anchors, and bias (or set `target = current` for a preserve)
    and refresh its geometric stability. Steering changes that occurred while
    it was parked therefore cannot publish fields against a stale target.
11. Reactivation marks every layer dirty and sets the status to `Generating`.
    Dependency hashes rederive fields from the retained current possibility
    state; activation never resets `current`, `revision`, or an unpreserved
    region to a fresh `current = target` epoch.
12. Near and contributor-covered regions remain field-capacity exemptions.
    A parked resident that becomes near or gains a preserve contributor is
    admitted on the next update even if the disposable field target is full.
13. Preserve winner changes apply to parked authority exactly as they apply to
    field-active authority: a material snap advances revision once, bucket
    changes dirty the declared closure, same-bucket normalization remains a
    revision/organism event only, and final-owner removal keeps the ADR 0020
    no-snap release. Parking or reactivation adds no synthetic revision.
14. Parking retires the current per-region in-flight entries. With cancellation
    enabled their tokens are flipped; with cancellation disabled a late result
    runs but fails the existing dispatch-identity gate, is reclaimed, and may
    not recreate tiles or change the parked status.
15. Macro and roster working-set protection follows field-active dependents,
    not every parked authority entry. Expected dependency keys remain
    computable from parked authoritative state, but parked coordinates do not
    pin derived macro/roster allocations they cannot consume.
16. `RegionMap::get`, `len`, `iter_active`, and `FrameStats::active_regions`
    continue to expose/count all authoritative residents, including parked
    ones. Their documentation must make this explicit. Cache iteration remains
    the derived-residency traversal.
17. Session snapshots include every authoritative resident, parked or active.
    Restore inserts those records initially parked, then reconstructs field
    admission under the live configuration. Restored current/stability/revision
    bits are not replaced by capacity admission.
18. The session schema continues to omit `target`: anchors, bias, and the field
    reconstruct it, and activation now recomputes it before dispatch. The
    broader snapshot-precondition limitation remains finding 29; A.4 must not
    change record bytes or falsely claim arbitrary mid-frame target persistence.
19. A dedicated ordered regional-history hash includes coordinate, `current`,
    `target`, stability, and revision for every authoritative resident. It is
    independent of field residency and is used for tight-versus-roomy
    trajectory comparisons. The full replay hash incorporates this authority
    fold before hashing derived caches.
20. No generation formula, dependency-hash fold, stable feature identity,
    record schema, or record encoding changes. `WORLD_ALGORITHM_VERSION`
    remains 2, all declared `algorithm_revision` values remain 0,
    `RECORD_FORMAT_VERSION` is unchanged, and no golden fixture is re-blessed.

## 2. Scope boundaries

### 2.1 In scope

- Authoritative versus field-active lifecycle inside `RegionMap`.
- Independent authoritative loading and deterministic field admission.
- Capacity parking, radius deletion, cancellation, late-result rejection,
  buffer reclamation, and dependent-cache sweeping.
- All-frame stability refresh and target-only amortization.
- Parked preserve mutation, activation, session snapshot/restore, and hashing.
- Focused runtime regressions and a `wer-scale` tight-versus-roomy authority
  gate.
- ADR 0023, ADR index, and present-tense/roadmap/finding updates in
  `world-model.md`.

### 2.2 Explicitly out of scope

- Tier-invariant resonance, capture selection, route cost, or extra organism
  slots (A.5/finding 6). The paired trajectory test uses an inline executor,
  unlimited generation, and equal near-field prerequisites to isolate field
  capacity from ADR 0018's already-documented schedule-sensitive resonance.
- Anchor canonicalization, route influence, boundary generation, organism
  identity redesign, or persistence hardening from later roadmap items.
- Retaining unpreserved authority beyond `unload_radius`, durable regional
  history, seasons, succession, or evolutionary state.
- Converting logical payload ceilings into allocator/process-memory limits
  (finding 32).
- Adding `target` or status to the persisted session record (finding 29).
- Editing accepted ADRs in place, historical prototype plans, performance
  baselines, algorithm/record versions, or goldens.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `enforce_capacity` | Calls `drop_region`, deleting authority with tiles. | Park derived fields and retain `RegionState`. |
| `load` | Capacity can skip creation of a non-near region. | Create authority under `max_loads` regardless of field capacity; admit fields separately. |
| `drop_region` | Conflates radius deletion and cache eviction. | Split `park_region_fields` from full radius removal. |
| `retarget` | Amortizes stability and target together. | Refresh all stability each frame; budget targets only. |
| `dispatch_regen` | Treats every region with dirty bits as dispatchable. | Skip `Unloaded` parked regions without clearing their authority. |
| reactivation | A removed state reloads with `current = target`. | Retain current/revision and rebuild all tiles from that state. |
| dependent sweeps | “Resident” means any entry in `regions`. | Derived macro/roster protection uses field-active dependents only. |
| session restore | Inserts restored regions directly as generating. | Restore authority parked; reconstruct admission deterministically. |
| replay/scale | Full hashes and settling assume every authority has fields. | Hash authority separately; treat intentionally parked state as quiescent. |

## 4. Required design

### 4.1 One authority map, explicit field residency

Keep `BTreeMap<RegionCoord, RegionState>` as the sole authority. Define a
central private predicate equivalent to:

```text
field_active(region) := region.status != GenerationStatus::Unloaded
```

`Unloaded` does not mean the coordinate is absent; `regions.get(coord) == None`
is absence. Update the enum and iterator comments accordingly. A parked region
may carry dirty hints, but those hints do not make it dispatchable. Helpers that
mark dirty or refresh status must preserve `Unloaded`; only the field-admission
path may move it to `Generating`.

Add one shared exact full-field payload calculation:

```text
resolution² × (CHANNEL_COUNT × sizeof(f32) + sizeof(u8) + sizeof(u16))
```

Use it in runtime admission and scale gates instead of the duplicated `55`
literal. This is still a logical payload estimate, not a heap cap.

### 4.2 Authoritative loading, admission, and radius removal

Refactor the load pass into two ordered operations:

1. Enumerate coordinates missing from `regions` inside `load_radius`, sort by
   nonnegative distance bits then coordinate, and insert at most
   `max_loads`. Initialize preserved authority from the effective signature;
   initialize ordinary authority with the existing fresh-region
   `current = target_for(...)` rule. Insert each as `Unloaded`. Do not inspect
   cache bytes here. `loaded` and `deferred_loads` describe only this
   authoritative creation budget.
2. Admit eligible parked fields. Candidates are parked coordinates inside
   `load_radius`, plus any parked contributor-covered resident. Sort nearest
   first with the same total tie-break. Near/preserved candidates are exempt;
   ordinary candidates require one full-payload reservation below the
   disposable target. Admission recomputes target/stability, sets every layer
   dirty, and changes status to `Generating`; the generation-cost budget then
   controls actual work.

Existing field-active entries in the load/unload hysteresis band remain active
unless capacity parks them. Field admission must reserve the full eventual
payload of every `Generating` or `Ready` disposable coordinate, not just bytes
already integrated, so partially generated regions cannot overbook the target.
The target applies to disposable reservations; near/preserved payload is an
explicit floor above it, matching the existing scale bound of target plus near
exemption.

The radius sweep remains first and authoritative. For a coordinate beyond
`unload_radius`, call the derived parking helper and then remove its
`RegionState`. Preserve contributors remain sparse and survive as ADR 0020
already requires. This is the only ordinary path by which unpreserved regional
history is forgotten.

### 4.3 Capacity parking and late work

Capacity enforcement considers field-active, non-near, non-preserved regions
and parks farthest first with the current distance-bit/coordinate order until
the full-payload reservation target is met. `evicted_for_capacity` counts field
parking events, not deletion of authoritative residents.

`park_region_fields(coord)` must perform one centralized teardown:

1. remove `RegionTiles` and return uniquely owned buffers to `TilePool`;
2. remove `region_signatures` for roster sweeping;
3. retire/recycle organisms and their realization key;
4. retire each level-0 in-flight entry, flipping its token only when
   cancellation is enabled;
5. set the region status to `Unloaded` without changing current, target,
   stability, or revision; and
6. leave regeneration hints in a state that activation can replace with
   `all_layers_mask()`.

After all victims are parked, sweep macro jobs/tiles against field-active
dependents. A shared macro job remains if another active covered region needs
it; otherwise it is retired normally. Result integration already requires a
live in-flight entry and current key. Preserve that gate: a late result from a
parked coordinate is counted/reclaimed as dropped and cannot create a cache
entry or change status.

### 4.4 Stability and target data flow

Keep the update ordering but split the retarget pass internally:

```text
integrate -> radius/capacity park -> create/admit ->
refresh all stability -> refresh budgeted targets -> resonance -> converge ->
dispatch field-active work -> integrate -> realize
```

The all-region stability loop is coordinate ordered. Contributor-covered
regions get stability one and `target = current`; every other region gets
`stability_for(config, center_distance)`. It runs even with
`max_retarget_regions == 0` and before convergence.

The target loop retains `steer_signature` and `retarget_cursor`. A signature
change refreshes all authoritative targets and clears deferral. Otherwise it
processes at most `max_retarget_regions` coordinates round-robin over the
complete `regions` key set. Rename internal helpers/comments as needed so
“retarget” telemetry unambiguously means target calculation. Player movement
alone no longer leaves stale stability.

Convergence continues to iterate all authoritative regions, including parked
ones, farthest first with the existing coordinate tie-break and
`max_converge_regions`. Bucket flips may accumulate dirty hints on a parked
state but must not wake dispatch. This preserves the same authoritative
candidate set and evolution order across field ceilings.

### 4.5 Reactivation, preserves, and sessions

Activation is not a load epoch. For an ordinary parked state, recompute target
from current field/anchors/bias and refresh stability, but retain current and
revision. For a preserve, retain its snapped current, force target equal to
current and stability one. Then dirty all layers and materialize through the
normal dependency-key scheduler.

`apply_effective_preserve_signature` continues to mutate a parked
`RegionState`: exact current changes bump revision once, quantized flips mark
the declared closure, and material changes retire organisms. It must not move
`Unloaded` to `Generating` directly because the mutation API lacks the field
and steering inputs needed for safe admission. The next update sees the
contributor exemption and activates correctly. Removing the final contributor
keeps the no-snap parked state.

`Vault::snapshot_session` already walks `iter_active`; keep that traversal and
clarify that it includes parked authority. Change `restore_region` to install
current/stability/revision as `Unloaded`, with no fields or in-flight work.
Admission recomputes target before dispatch and the normal radius sweep prunes
snapshot entries no longer valid for the restored player/config. Do not change
the session codec.

### 4.6 Iterator, hash, and quiescence audit

Audit every `iter_active`, `regions.keys`, and status consumer:

- authoritative consumers — stability, target, convergence, preserve
  reconciliation, snapshotting, continuity state, `get`/`len`, and
  `active_regions` — include parked entries;
- derived consumers — dispatch/readiness, realization, cache inspection,
  region-signature protection, and dependent-cache sweeping — require field
  activity or iterate the cache directly;
- near gameplay reads remain safe because near states are admitted as an
  exemption before dispatch/realization; and
- generic settle helpers treat `Unloaded` as intentionally quiescent while
  still rejecting `Generating`, in-flight, or queued work.

Add `regional_history_hash(&RegionMap)` in `tools::replay`, folding ordered
coordinates and the bit patterns of current, target, stability, and revision.
Re-export it beside `state_hash`. Make `state_hash` start from/include that
fold, then continue hashing derived caches, rosters, and organisms. Do not fold
`GenerationStatus` into the authority hash: field admission is deliberately
derived and may differ across ceilings.

## 5. Verification matrix

### 5.1 Focused runtime tests

Add tests in `crates/world-runtime/tests/streaming.rs` and the private recovery
module in `stream.rs` covering:

1. **Load independence:** with identical `max_loads`, zero field capacity and
   roomy capacity create the same ordered coordinates/current/target even
   though only the roomy map owns far tiles. `deferred_loads` reflects budget,
   not capacity.
2. **History retention:** force a material convergence/revision, park its
   tiles under pressure while it stays inside `unload_radius`, and assert
   current, target, stability, and revision survive byte-for-byte.
3. **Radius bound:** move the same parked coordinate beyond `unload_radius` and
   assert the authority entry is then removed.
4. **Reactivation:** revisit/approach the parked coordinate; with zero travel,
   assert activation does not reset current or revision, recomputes the current
   steered target, dirties/rebuilds all layers, and reproduces roomy-cache tile
   hashes.
5. **Geometric pin:** configure `max_retarget_regions = 1`, move a previously
   far coordinate into `near_radius` without changing steering, and assert all
   resident stability values match geometry that frame; the crossing region's
   current/revision do not change despite positive travel.
6. **Target telemetry:** unchanged steering still processes only one target
   and reports `active_regions - 1` deferred, while stability for all regions
   is current. A steering change still reports no deferred targets.
7. **Parked preserve:** apply lower/higher winners and final deletion to a
   parked state; assert ADR 0020 winner/revision/same-bucket/no-snap semantics,
   then activate and verify no extra revision and correct fields.
8. **Late result:** queue region work, disable cancellation, park it, run the
   old closure, integrate, and prove the result is dropped/reclaimed without
   resurrecting tiles. Repeat/assert the cancellation-enabled token counter.
9. **Session:** snapshot a map containing a parked revised coordinate, restore
   it, assert authority bits survive and it begins parked, then admit/settle
   without a fresh-state reset.

### 5.2 Harness tests

Extend the `wer-scale` memory scenario rather than creating a parallel binary:

- run a tight and roomy map through the same deterministic inline/unlimited
  trajectory with equal near-field prerequisites;
- compare `regional_history_hash` after every frame and report the first
  divergence;
- require at least one capacity parking event and at least one coordinate whose
  authority exists while its tiles do not;
- retain the existing field plateau, pool bound, and return-trip content-hash
  gates; and
- use the shared payload-byte helper for ceiling and near-exemption math.

The existing `quick_harness_passes` test makes the new gate part of
`cargo test --workspace`. Keep ADR 0018's broader allowance for executor-paced
mid-flight resonance visible; this regression proves that capacity does not
directly remove or stop regional authority.

### 5.3 Regression expectations

- Existing preserve overlap/session tests stay green.
- Dependency recovery and stale-result matrices stay green.
- Continuity replay reports zero pinned violations with amortized targets.
- Schedule-independence settled hashes remain equal across their comparison
  matrix even though the numeric hash changes when target joins the fold.
- No determinism or record golden changes appear in the diff.

## 6. Exact file set

| File | Planned change |
|---|---|
| `crates/world-runtime/src/region.rs` | Document `Unloaded` as retained authority without admitted fields. |
| `crates/world-runtime/src/generate.rs` | Correct the cache/authority docs and add the shared full-region payload helper. |
| `crates/world-runtime/src/macrocache.rs` | Clarify that derived macro residency follows field-active dependents, not parked authority. |
| `crates/world-runtime/src/rostercache.rs` | Clarify that roster eviction follows field-active signature dependents. |
| `crates/world-runtime/src/stream.rs` | Split radius deletion from capacity parking; independent state creation/admission; all-state evolution; field-active dispatch/sweeps; activation, session restore, cancellation, and private regressions. |
| `crates/world-runtime/src/budget.rs` | Define `max_retarget_regions` as target-only. |
| `crates/world-runtime/src/tier.rs` | Update tier comments from retarget/stability amortization to target amortization. |
| `crates/world-runtime/src/lib.rs` | Re-export the payload helper if tools consume it. |
| `crates/world-runtime/src/vault.rs` | Correct session-restore documentation now that restored authority begins parked. |
| `crates/world-runtime/tests/streaming.rs` | Add load/history/reactivation/geometric-pin and telemetry integration tests. |
| `crates/tools/src/replay.rs` | Add the regional-history hash, include targets in authority hashing, and make quiescence parking-aware. |
| `crates/tools/src/lib.rs` | Re-export `regional_history_hash`. |
| `crates/tools/src/scale.rs` | Add tight-versus-roomy trajectory gates and parking-aware settle/payload accounting. |
| `docs/adr/0023-field-cache-pressure-parks-derived-state.md` | Record the authority/field-residency and all-frame stability decision. |
| `docs/adr/README.md` | Index ADR 0023 (renumber only if main gains the number before rebase). |
| `docs/world-model.md` | Update present-tense model, resolve findings 4/5, and mark A.4 completed. |
| this plan | Change status to `Completed` only after implementation and all gates pass. |

Do not add other files merely for convenience. If implementation reveals an
unavoidable caller not listed here, document why in this plan before including
it, and keep historical plan files untouched.

## 7. ADR and versioning decision

A new ADR is required because this change defines which state is authoritative,
what a cache ceiling is allowed to discard, how preserves interact with parked
state, and which half of retargeting may be amortized. These are durable
architecture constraints, not a local bug fix.

Create ADR 0023, currently the next free number, titled approximately “Field
cache pressure parks derived state; regional history follows streaming
geometry.” It builds on ADRs 0006, 0008, 0018, 0019, and 0020. It refines ADR
0019's use of “resident” for derived working sets: authoritative residents may
be parked, while only field-active dependents protect generated macro/roster
inputs. Do not edit accepted ADRs in place.

No version bump is warranted. The corrected script trajectory changes only in
the previously broken capacity/amortization cases; generation remains the same
pure function for every dependency key. Verify explicitly that version
constants, declaration revisions, and golden fixture files have no diff.

## 8. Implementation sequence

1. Add the payload helper and clarify `GenerationStatus`/iterator semantics.
2. Introduce centralized field-active, park, full-radius-drop, and activation
   helpers; migrate capacity and load paths.
3. Gate dispatch/status repair and dependent sweeps on field activity; verify
   cancellation and late integration behavior.
4. Split all-frame stability from budgeted targets; keep convergence over the
   full authority set and update telemetry/docs.
5. Make preserve and session restore paths parking-safe; audit every
   authoritative/derived traversal.
6. Add focused tests, then regional-history hashing and scale gates.
7. Add ADR 0023/index and update `world-model.md`; only now mark the plan and
   roadmap/finding statuses completed.
8. Run focused tests followed by the complete CI-equivalent matrix.

## 9. Validation commands

Run from the A.4 worktree with the pinned toolchain (source Cargo's environment
first if necessary):

```sh
cargo test -p world-runtime --test streaming
cargo test -p world-runtime
cargo test -p tools scale::tests::quick_harness_passes
cargo run --release --bin wer-scale -- --quick
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTFLAGS="-D warnings" cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
git diff --check
```

Also verify:

```sh
git diff -- docs/plans/prototype/implementation-plan.md 'docs/plans/prototype/phase-*-plan.md'
git diff -- crates/world-core/tests/determinism.rs
git diff -G 'WORLD_ALGORITHM_VERSION|RECORD_FORMAT_VERSION|algorithm_revision'
```

All three commands must show no unintended version/historical/golden changes.

## 10. Documentation completion edits

After every test passes:

- Change roadmap A.4 to **Completed**, link this plan, and summarize retained
  authority, capacity parking, all-frame stability, activation, and gates.
- Rename findings 4 and 5 as resolved, add status links, retain their original
  failure descriptions for auditability, and append concrete resolution/test
  paragraphs.
- Update sections 2.1, 2.5, 3.5, 3.25, and 3.26 so “eviction forgets history”
  means only crossing `unload_radius`; field capacity parks derived memory;
  state creation and trajectory are capacity-independent inside the window;
  and only target calculation is amortized.
- Update the intentional-scope bullet to say unpreserved regions forget
  history after radius unload, not field-capacity parking.
- Update verification prose for the tight-versus-roomy authority gate without
  claiming A.5 tier invariance or resolving finding 33's other harness gaps.
- Change this plan's status from `Planned` to `Completed` last.

## 11. Risks and rollback

| Risk | Mitigation |
|---|---|
| Parked states accidentally stop evolving | Paired per-frame authority hashes and iterator audit. |
| Parked state grows without bound | Full radius removal regression at `unload_radius`. |
| Partial generation exceeds target | Reserve full eventual payload for every field-active disposable region. |
| Parked dirty bits wake work | Dispatch/status helpers explicitly preserve `Unloaded`. |
| Stale target publishes after reactivation | Unconditional target recomputation at activation. |
| Preserve snap loses/bump history twice | Parked winner/same-bucket/final-delete tests. |
| Late job recreates parked cache | Cancellation-off queued-result integration regression. |
| Macro/roster memory pinned by cold authority | Field-active dependent-set sweep and focused cache assertions. |
| Settle loops never finish under a ceiling | Treat intentional `Unloaded` as quiescent, never `Generating`. |
| O(window) stability cost regresses performance | Stability is the cheap geometry-only loop; target math remains amortized and pass timing remains visible. |

Rollback is one commit: revert the A.4 commit. There is no data migration,
fixture re-bless, record-format transition, or irreversible external state.

## 12. One-commit worktree, merge, and push workflow

Perform implementation only on `codex/improvement-a4-authoritative-history` in
`/tmp/wer-improvement-a4`. The planner does not commit. The fresh execution
agent implements and validates the complete plan, then stages only the file set
above and creates exactly one commit, for example:

```text
Keep regional history independent of field cache pressure
```

Before merging, fetch/check the latest local `main`. If it advanced, rebase the
single A.4 commit onto it, resolve without losing either side, and rerun all
focused and full gates on the rebased tree. Fast-forward merge from the primary
worktree with `git merge --ff-only`, confirm the forbidden historical files and
goldens remain untouched, then push `main` to `origin`. Do not begin A.5 until
that push succeeds.

## 13. Definition of done

A.4 is done only when all invariants and tests above hold; capacity pressure
parks fields while retaining bounded, continuously evolving authority;
geometric near pinning is current every frame; parked preserves, sessions, and
late work are correct; ADR 0023 and current documentation are landed; all CI
and wasm gates pass; exactly one commit is fast-forwarded to `main`; and that
commit is pushed successfully to `origin`.
