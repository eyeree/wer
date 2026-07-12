# Improvement A.2 — Deterministic Preserve Ownership and Revision Semantics

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.2](../../world-model.md#prioritized-improvement-roadmap)

**Finding addressed:** [26](../../world-model.md#26-resolved-overlapping-preserves-lacked-ownership-and-conflict-semantics)

This plan implements the second item in the prioritized improvement roadmap in
[`world-model.md`](../../world-model.md). It replaces the runtime's single,
last-writer-wins preserve override with a contributor set keyed by the
preserve's content id, selects an effective owner independently of application
order, restores the next owner when a record is deleted, and makes every
resident realized-state replacement participate in the region revision and
near-field organism identity contracts.

The work is complete only when the implementation, focused overlap and
revision regressions, architecture record, full native/wasm validation, and
the final `world-model.md` edits land together. The roadmap item and finding
must remain open until every gate has passed.

This is a post-prototype corrective plan. It must not modify
[`implementation-plan.md`](implementation-plan.md) or any
`docs/plans/prototype/phase-N-plan.md` file; those files are the immutable
record of the original prototype plan.

---

## 1. Outcome and completion criteria

After this work, a region may be covered by any number of preserve records,
but its effective persisted state and owner are pure functions of that set,
not of startup, import, merge, or UI event order.
Canonical synchronization batches also have order-independent resident
revision/derived results. Separate UI calls remain separate material events,
so their revision and organism epochs intentionally retain sequential history.

All of the following must hold:

1. `RegionMap` retains every `(preserve content id, signature)` contributor for
   each covered coordinate, including contributors that do not currently win.
2. The effective owner is the contributor with the numerically lowest `u64`
   content id. This total order is stable, portable, already present in every
   `PreserveRecord`, and does not depend on mutable name, sequence, or import
   order.
3. Startup/session-load synchronization and any bulk resynchronization collect
   one atomic batch, install its complete contributor set, and reconcile each
   touched resident once. Vault preserves traverse in ascending content-id
   order and each record's canonical coordinate order for auditability, but
   reversing distinct records within the batch produces the same winner,
   revision, tiles, and organisms. Repeated same-id coordinates still follow
   canonical record order pending finding 25's duplicate policy.
4. Adding or deleting a non-winning contributor changes only contributor
   bookkeeping. It does not replace resident state, bump revision, dirty
   layers, cancel work, or rebuild organisms.
5. Adding a lower-id contributor, deleting the winning contributor, or
   replacing a winning contributor recomputes the winner immediately. If a
   successor exists, resident `current` and `target` snap to that winner's
   quantized bucket centers and stability remains fully pinned.
6. Removing the final contributor preserves the existing no-snap release
   contract: resident `current`, revision, tiles, and organisms are unchanged;
   ordinary retargeting and travel-fueled convergence resume on subsequent
   updates. Evicted regions simply lose the override and next load normally.
7. Whenever applying a newly effective signature changes a resident region's
   realized `current` vector, even if only by normalizing an already-matching
   bucket to its canonical center, increment `RegionState::revision` exactly
   once with wrapping arithmetic before any organism realization can publish.
8. Dirty generation only for quantized-domain bucket changes, using the ADR
   0007 declared-domain closure. A same-bucket center normalization therefore
   bumps revision and invalidates organism identity but leaves tile dependency
   hashes, tile dirtiness, and in-flight tile work unchanged.
9. A material realized-state replacement retires/recycles the resident's old
   near-field organism vector and realization key. Organisms are rebuilt only
   after the effective winner's L8 inputs are fresh, using the new region
   revision; stale feature ids from the previous epoch may not remain current.
10. If effective owner changes but its signature equals the prior winner's
    signature, only the reported owner changes. Since realized state is
    bit-identical, revision, tiles, jobs, and organisms remain unchanged.
11. Preserve ownership survives ordinary radius eviction and field-capacity
    eviction. A later reload uses the effective winner; removing a contributor
    while the coordinate is evicted still selects the correct next winner for
    that reload.
12. Native `P`-key deletion resolves the runtime's effective owner at the
    player's coordinate and removes that exact vault record. It never deletes
    an arbitrary covering record selected by iteration order, and it removes
    only that record's contributor entries from all of its regions.
13. Existing local preserve creation, startup, session load, atlas import, and
    eviction/reload behavior use the contributor-aware API. There is one
    shared application/removal path rather than parallel ownership logic in
    the shell.
14. No generator formula, dependency-hash fold, permanent feature hash,
    persistent record encoding, or CRDT merge law changes.
    `WORLD_ALGORITHM_VERSION` remains 2, every declared
    `algorithm_revision` remains 0, `RECORD_FORMAT_VERSION` remains unchanged,
    and no golden fixture is re-blessed.
15. Native CI-equivalent checks and the neutral/web `wasm32` check pass, ADR
    0020 records the ownership/revision decision, and `world-model.md` marks
    A.2 and finding 26 completed/resolved without claiming that duplicate
    canonicalization (finding 25) or persistence error handling (A.3/finding
    28) is complete.

## 2. Scope boundaries

### 2.1 In scope

- Contributor-aware preserve state in `RegionMap`.
- A deterministic lowest-content-id conflict rule and canonical bulk traversal.
- Effective-owner/signature queries for runtime and native-shell consumers.
- Winner recomputation after contributor add/remove, resident application, and
  no-snap release when the last contributor disappears.
- Exact revision, dirty-layer, cancellation, tile, roster, and organism
  behavior for effective-winner changes and same-bucket normalization.
- Startup, session restore, atlas import/resynchronization, local creation,
  deletion, and eviction/reload call sites.
- Focused runtime and preserve-harness regressions for ordering, ownership,
  deletion, revision, tiles, and organisms.
- ADR 0020, the ADR index entry, and current-state/roadmap/finding updates in
  `world-model.md`.

### 2.2 Explicitly out of scope

- Deduplicating preserve region lists or atlas record-id lists, rejecting
  repeated coordinates, or changing preserve content-id construction. Those
  are finding 25 and must not be silently bundled into A.2.
- Tombstones, distributed deletion, collision-resistant record ids, sequence
  handling, flush/delete error propagation, or storage durability. Those are
  separate persistence findings, especially A.3/finding 28.
- Delaying imported preserves until offscreen or adding a visual transition.
  This plan keeps the existing explicit snap-to-persisted-buckets behavior and
  makes its revision/re-realization effects correct and documented.
- Retaining ordinary unpreserved regional history across field-capacity
  eviction (A.4), changing eviction victim order, or redefining memory caps.
- Changing steering, convergence, resonance, generation math, layer
  declarations, resource tiers, record bytes, or atlas merge laws.
- Editing any accepted ADR in place, any historical prototype/phase plan, any
  golden fixture, or any algorithm/record version.

## 3. Current failure map

| Path | Current behavior | Contract failure | Required correction |
|---|---|---|---|
| `RegionMap::overrides` | Stores one `PossibilitySignature` per coordinate. | Loses the identity and state of every overwritten contributor. | Store a per-coordinate ordered map from preserve id to signature. |
| `RegionMap::set_override` | Plain insertion makes the last call win. | Startup/import/application order changes authoritative state. | Insert contributor, then derive the lowest-id winner. |
| `RegionMap::clear_override` | Removes the coordinate's only override. | Deleting any overlapping preserve can unpin a region still covered by another. | Remove only `(coord, preserve_id)` and recompute the successor. |
| resident override application | Snaps `current`/`target`, dirties bucket readers, but never bumps `revision`. | Organism identity may reuse a prior possibility-revision epoch for a different realized state. | Compare old and snapped `current`; bump once on any actual vector change. |
| same-bucket snap | Changes sub-bucket `current` to bucket centers but reports no flipped buckets. | Tiles correctly remain fresh, but revision and organism identity incorrectly remain old. | Separate material state change from bucket change; bump/re-realize without tile dirtying. |
| `organism_keys` | Reuse is keyed only by L8 dependency hash. | A revision-only change can reuse old organism ids even though realization hashes the revision. | Retire the old vector/key on preserve-driven revision change (and test the unchanged-L8 case). |
| native `apply_preserves` | Flattens signatures and discards record ids. | Runtime cannot represent ownership or recover overlap. | Pass each record id with its canonical regions. |
| native `toggle_preserve` | Deletes the first vault record that covers the player, then clears every listed coordinate. | Iteration happens to be ordered today but is not the declared runtime winner; clearing also erases other contributors. | Query the map's effective owner, remove that record, then remove only its contributions. |
| eviction/load | Reload consults a single signature. | The wrong last-applied value survives, and deletion while evicted cannot reveal a hidden contributor. | Consult the contributor set's effective winner on every load/pin/eviction exemption check. |

Useful existing contracts remain unchanged:

- `PreserveRecord::id` is an immutable integer content id and vault maps are
  `BTreeMap`s, so a portable total order already exists.
- signatures dequantize to canonical bucket centers at the persistence
  boundary (ADR 0013).
- `domain_dirty_mask` is the authoritative translation from flipped
  possibility buckets to the declared dependency closure (ADR 0007).
- dependency hashes, not `RegionState::revision`, govern tile staleness (ADR
  0008); revision remains part of near-field organism identity.
- the main thread owns `RegionMap`, cache mutation, cancellation, and organism
  vectors, so winner changes can be applied atomically without worker-side
  shared state.

## 4. Required design

### 4.1 Contributor model and conflict rule

Replace the single-value override map with a contributor index equivalent to:

```rust
BTreeMap<RegionCoord, BTreeMap<u64, PossibilitySignature>>
```

The inner map contains one entry per preserve content id. Its first entry is
the effective owner/signature. Do not maintain a second cached winner that can
drift from the contributor set; use `first_key_value` (or one centralized
helper) as the source of truth. Empty inner maps must be removed.

The public runtime seam should carry ownership explicitly. Exact names may
follow local style, but the API must provide these operations:

```text
apply preserve contribution(id, coord, signature)
remove preserve contribution(id, coord)
effective preserve(coord) -> optional (id, signature)
is overridden(coord) -> bool
```

A bulk application helper accepts owned `(id, coord, signature)` entries. It
must install the entire batch before reconciling each touched resident exactly
once, and must not make `world-runtime` depend on a platform crate or storage.
Startup, session restore, import synchronization, and local record creation use
this seam. Applying the same
`(id, coord, signature)` twice is idempotent. A same-id replacement is handled
as a contributor update and reconciled through the same old-winner/new-winner
path; valid vault records should never present inconsistent content under an
equal content id.

Lowest id wins even when a later-applied record has a different signature.
This rule is deliberately independent of mutable sequence/name metadata and
does not change vault CRDT merge behavior. Startup/session restore walks
`Vault::preserves()` in ascending id order and uses each record's already
canonical region order. Import must re-run the same synchronization path (or
apply newly accepted records through it) so imported lower ids take effect
immediately.

### 4.2 Centralized winner reconciliation

For a single contributor event, or once per touched coordinate in an atomic
batch, capture the old effective `(id, signature)`, mutate the contributor
state, compute the new effective value, and reconcile exactly once:

| Old winner | New winner | Resident action |
|---|---|---|
| none | `(id, sig)` | Pin; apply `sig` through the material-change rules below. |
| `(a, x)` | `(a, x)` | None; contributor-only mutation. |
| `(a, x)` | `(b, x)` | Update reported owner only; realized state is unchanged. |
| `(a, x)` | `(b, y)` | Stay pinned; apply `y` once through the material-change rules. |
| `(a, x)` | none | Release without snapping; leave current/revision/derived state intact. |

For an evicted coordinate, contributor mutations only update the index. On
load, initialize `current` and `target` from the then-effective signature and
set stability to one. Initial materialization of a newly loaded
`RegionState` is not a mutation of an already-realized region and therefore
does not add a synthetic revision bump; session restore retains its persisted
revision, after which applying a different effective winner follows the normal
resident comparison and bump rule.

Removing a non-winner or adding a higher-id non-winner must be observably inert
outside the contributor/effective-owner query. Removing the last winner keeps
the old current vector until normal retarget/converge acts; it must not restore
an earlier field value or calculate an immediate target inside the mutation
method.

### 4.3 Resident revision and dirty semantics

When a new effective signature must be applied to a resident region:

1. Snapshot the old `current` and its quantized domain buckets.
2. Dequantize the new signature once to canonical centers.
3. Assign the snapped vector to `current` and `target`, and set stability to
   one.
4. Define a **material realized-state change** as `old_current != snapped`
   using the vector's existing exact `f32` equality. If material, bump
   `revision` once with `wrapping_add(1)`.
5. Independently calculate the domain-bit mask whose quantized buckets differ.
   If nonzero, OR `domain_dirty_mask(flipped)` into `dirty_layers`, move a
   ready region back to generating, and cancel superseded in-flight jobs in
   that closure through the existing cancellation path.
6. If material, retire/recycle the region's old organism vector and remove its
   realization key before the next realization pass. Do this even when the
   flipped-domain mask is zero.

The separation in steps 4 and 5 is essential. A local preserve commonly
captures a signature from a `current` vector that lies inside a bucket rather
than exactly at its center. Applying it may keep every tile dependency key
unchanged while changing `current`; that is a revision event and an organism
identity event, but not a tile-generation event. Conversely, changing only
owner id while the signature and realized vector remain equal is not a
revision event.

The organism vector must not expose old-revision feature ids while the new
winner's dirty L8 chain settles. Recycle/clear it immediately on material
replacement; the existing realization preflight then rebuilds only from a
fresh Ecology tile and complete roster set. Add a focused assertion that the
new organisms use the incremented `possibility_revision`, including the
same-bucket case where the L8 dependency hash is unchanged.

### 4.4 Native lifecycle integration

Refactor the native shell around record-level helpers so every path preserves
the content id:

- **Startup:** after vault open and after session restoration, traverse all
  records in ascending id and apply `(id, coord, signature)` contributions.
- **Atlas import:** after a successful merge, synchronize all vault preserves
  through the idempotent canonical helper (or apply the exact added records)
  before the next world update. Re-applying existing entries must be harmless.
- **Local creation:** call `Vault::record_preserve`, retain its returned id,
  then apply each returned record contribution using that id. If an existing
  identical record is returned, runtime application remains idempotent.
- **Deletion:** ask `RegionMap::effective_preserve(player_region)` for the id,
  obtain that exact record from the vault, remove it from the vault, then
  remove `(id, coord)` for every coordinate in the record. Regions with a
  successor immediately adopt it; only regions with no successor release.
- **Eviction/reload:** every `contains`, exemption, retarget, converge, and load
  check uses contributor presence/effective signature rather than a separate
  single override.

Do not make decorations define ownership: map decor may continue to show the
union of all covered coordinates. Inspector output that reports one covering
preserve should use the same lowest-id rule where it claims an effective
owner, or label a simple covering record as non-authoritative.

The A.3 persistence plan will make backend delete failures transactional. For
this item, preserve the existing `Vault::remove_preserve` interface and do not
claim durable deletion on failure; keep runtime/vault mutation ordering
consistent with current behavior and explicitly leave finding 28 open.

## 5. Affected APIs and files

| File | Planned change |
|---|---|
| `crates/world-runtime/src/stream.rs` | Replace `overrides` with per-region contributor maps; add single and atomic-batch mutation/effective-owner helpers; centralize once-per-coordinate winner reconciliation; update load, retarget, converge, capacity exemption, and eviction paths; bump resident revision and retire organisms on material snaps; add focused unit tests. |
| `crates/platform-native/src/main.rs` | Preserve ids during startup/session synchronization and local creation; delete the runtime's effective owner; remove only that record's contributions; route import/resync through canonical application. |
| `crates/tools/tests/preserve.rs` | Update APIs and add end-to-end overlap, deletion, eviction/reload, tile, revision, and organism regressions. |
| `crates/tools/src/vault.rs` | Update the sign-off scenario to contributor-aware APIs and extend it to report deterministic overlap/recovery failures. |
| `crates/tools/src/atlas.rs` and inspector call sites, if needed | Align any singular “covering preserve” claim with lowest-id effective ownership without changing bundle validation or record bytes. |
| `docs/adr/0020-preserve-overlaps-use-lowest-content-id.md` | Record contributor retention, lowest-id ownership, no-snap final release, and revision/derived-state consequences. |
| `docs/adr/README.md` | Add accepted ADR 0020 to the index. |
| `docs/world-model.md` | Update current preserve/runtime descriptions, testing summary, A.2 status, and finding 26 resolution text. |

Do not touch `world-core/src/record.rs` or determinism fixtures unless a compile
only API import adjustment is unavoidable; no persistent structure or content
id changes are part of this plan.

## 6. Ordered implementation milestones

### Milestone 1 — Pin the decision and runtime contract

1. Add ADR 0020 using the repository's Nygard template and mark it Accepted.
2. State why lowest content id is selected: it is immutable, portable,
   independent of mutable metadata, total over valid records, and naturally
   supported by ordered maps.
3. Record alternatives rejected: last-applied wins, highest sequence wins,
   lexical name order, blending signatures, and rejecting all overlaps.
4. Record exact resident reconciliation, no-snap last removal, same-signature
   owner changes, same-bucket normalization, and organism invalidation.
5. Add the ADR index row without editing ADRs 0013/0014 or other accepted
   history.

### Milestone 2 — Introduce contributor-aware runtime state

1. Change the override field and comments to the nested ordered contributor
   model.
2. Add one source-of-truth effective-winner helper and public owner/signature
   query; retain `is_overridden` as a contributor-presence convenience if it
   remains useful.
3. Replace `set_override`/`clear_override` with owner-carrying add/remove
   operations and update all call sites so compilation prevents accidental
   ownerless mutation.
4. Make repeated contribution application idempotent and remove empty inner
   maps after deletion.
5. Add unit tests that apply two conflicting preserves in both orders and
   assert the same effective id/signature before involving generation.

### Milestone 3 — Reconcile resident state, revision, and derived work

1. Implement the old-winner/new-winner transition table once, outside native
   code.
2. On a material resident snap, bump revision once before organism rebuilding;
   calculate bucket flips separately and dirty only declared readers.
3. Cancel only in-flight layers in the resulting dirty closure. A
   same-bucket normalization must not cancel tile work.
4. Remove/recycle stale organisms and the realization key on every material
   snap, including unchanged-L8 cases.
5. Keep last-contributor removal no-snap and leave revision/tiles/organisms
   untouched until normal convergence changes current.
6. Add white-box tests for revision count, dirty mask, status, cancellation or
   unchanged job identity as observable, and organism re-realization.

### Milestone 4 — Convert every lifecycle path

1. Make load/retarget/converge and cache-capacity logic consult contributors
   and the effective signature.
2. Update native startup and session load to canonical record-id traversal
   through one atomic application batch.
3. Update local creation to apply the returned content id.
4. Change `P`-key deletion to query and remove the effective owner, then remove
   only its per-coordinate contributions.
5. Connect atlas import/resynchronization to the same idempotent helper.
6. Update tools/harness call sites and audit the repository with
   `rg 'set_override|clear_override|overrides'` so no ownerless path remains.

### Milestone 5 — Add overlap and lifecycle regressions

Cover at least these cases:

1. two conflicting signatures supplied low/high and high/low in one resident
   synchronization batch reconcile once to the same lowest-id owner, realized
   signature, revision, dependency hashes, settled tiles, and organisms;
2. deleting the winning preserve selects the remaining contributor and bumps
   revision exactly once when realized current changes;
3. deleting a non-winner changes no revision, dirty bit, tile hash, organism
   vector/key, or effective state;
4. deleting the final contributor causes no immediate snap/revision/tile or
   organism change and later convergence remains bounded;
5. an owner-id change with identical signatures updates owner only;
6. a same-bucket capture normalizes to the center, increments revision,
   retains tile hashes/dep hashes, and rebuilds organisms with new feature ids;
7. a bucket-changing foreign winner dirties exactly the declared reader
   closure, rejects/cancels stale work as existing contracts require, and
   eventually realizes the new winner's tiles and organisms;
8. overlap survives travel, steering, ordinary eviction, field-capacity
   eviction, reload, and deletion while the region is evicted;
9. resident startup/session/import batches in forward and reversed record order
   reconcile once and converge to identical ownership, revision, tiles, and
   organisms; repeated synchronization and local creation remain idempotent;
10. native toggle deletes the effective owner and reveals the next owner rather
    than unpinning the overlap.
11. a restored session resident reconciles an overlapping batch once, and a
    bucket-changing winner cancels exactly the declared in-flight closure.
12. inspector selection uses the last repeated coordinate signature within the
    lowest-id winning record, matching runtime batch insertion without defining
    finding 25's eventual duplicate policy.

Prefer small `RegionMap` unit tests for transition bookkeeping and the existing
`tools/tests/preserve.rs`/`wer-vault` paths for cross-component proof. Keep
tests deterministic and inspect exact ids, revisions, signatures, dep hashes,
tile hashes, and organism ids instead of relying only on screenshots/log text.

### Milestone 6 — Update the living model and close the item

Only after code and validation pass:

1. In the current-state preserve description around §3.21/§3.25, replace the
   single override description with contributor sets, lowest-id effective
   ownership, winner recomputation, no-snap final release, and eviction/reload
   behavior.
2. In the runtime/coordinator description, state that a material resident
   winner snap increments the region revision; bucket flips alone govern tile
   dirtiness, while any material snap retires old-revision organisms.
3. In the test inventory, mention deterministic overlap order, winner/nonwinner
   deletion, revision-only normalization, and tile/organism recovery coverage.
4. Change roadmap A.2 to
   `**Completed: Give preserves deterministic ownership and revision semantics**`,
   link this plan, and summarize the landed lowest-id/revision tests.
5. Change finding 26's heading to
   `#### 26. Resolved: Overlapping preserves lacked ownership and conflict semantics`
   and append a concise resolution naming contributor tracking, lowest-id
   ownership, successor recomputation, material revision bumps, same-bucket
   behavior, and focused regressions.
6. Preserve the original problem text for auditability where practical and do
   not mark findings 25, 27, 28, A.3, or A.4 complete.
7. Change this plan's status from Planned to Completed in the same final patch.

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| A hidden ownerless call site recreates last-writer behavior. | Remove or signature-break old APIs; finish with repository-wide `rg` audit. |
| Lowest id is applied only in native code and runtime state drifts. | Make the nested `BTreeMap` and centralized runtime helper authoritative; native ordering is auditability only. |
| Revision bumps for mere contributor churn. | Compare effective signature/realized vector; non-winner and same-signature owner changes have explicit no-op tests. |
| Same-bucket normalization needlessly regenerates tiles. | Separate exact vector change from quantized bucket flips and assert unchanged dep/tile hashes and in-flight work. |
| Same-bucket normalization leaves old organism ids. | Retire vector/key on every material snap and assert feature ids carry the new revision. |
| Winner change presents old organisms while L8 settles. | Clear/recycle organisms immediately, then rely on fresh-L8 and roster-completeness gates before rebuilding. |
| Removing the winner accidentally releases overlapping coordinates. | Remove by `(id, coord)`, recompute the first remaining entry, and test mixed overlap sets across all record regions. |
| Eviction logic exempts or reloads from stale cached ownership. | Derive both checks from contributor presence/effective winner; test deletion while evicted. |
| Import synchronization duplicates or reorders material epochs. | Install the complete atomic batch before one reconciliation per touched coordinate; test forward/reverse resident batches and repeated synchronization. Separate UI calls deliberately remain separate history. |
| Work expands into record canonicalization or storage failure handling. | Keep record bytes/APIs stable, call out findings 25/28 in scope exclusions, and reject golden/version changes. |
| Extra contributor maps increase sparse-state memory. | Preserves are already sparse durable deviations; document one ordered entry per covering record/region and defer unrelated compression. |

## 8. Validation matrix

### 8.1 Focused checks during development

```sh
cargo test -p world-runtime preserve
cargo test -p tools --test preserve
cargo run --bin wer-vault
```

Use the actual test names/filters introduced by the implementation if `preserve`
does not select all new unit cases. Exercise the native startup/create/delete
flow manually or through existing shell-level helpers where practical, but do
not make a windowed smoke test the only ownership proof.

### 8.2 Required pre-completion gates

Run from the repository root with the pinned stable toolchain:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets
RUSTFLAGS="-D warnings" cargo test --workspace
RUSTFLAGS="-D warnings" cargo check \
  -p world-core -p world-runtime -p platform-web \
  --target wasm32-unknown-unknown
```

Also run the preserve/vault sign-off path above after the full suite. If the
repository's CI workflow differs when implementation begins, mirror the
checked-in `.github/workflows/ci.yml` jobs as the final authority.

### 8.3 Invariant audit

Before completion, confirm explicitly:

- `git diff -- crates/world-core/tests` contains no golden re-bless;
- `WORLD_ALGORITHM_VERSION == 2`;
- all layer declaration `algorithm_revision` values remain `0`;
- `RECORD_FORMAT_VERSION` and encoded preserve fixtures are unchanged;
- neutral crates still compile for `wasm32-unknown-unknown` and gain no
  filesystem, thread, socket, renderer, or native-platform dependency;
- historical implementation and `phase-N-plan.md` files are untouched; and
- `world-model.md` closes only A.2/finding 26 and describes behavior proven by
  the new tests.

## 9. Definition of done

Improvement A.2 is done only when all of the following are true:

- every preserve contributor is retained per coordinate and lowest content id
  is the runtime's documented effective owner;
- record order within one add/import/startup/session synchronization batch
  cannot change winner, current signature, revision, settled tile hashes, or
  organism result; separate application calls retain their material history;
- winner deletion reveals the next contributor; non-winner deletion is inert;
  final deletion releases without a snap;
- resident material snaps bump revision exactly once, same-bucket center
  normalization does not dirty tiles, and both paths prevent reuse of
  old-revision organisms;
- startup, import, local creation/deletion, session load, ordinary/capacity
  eviction, and reload all use the contributor-aware path;
- focused runtime, integration, and `wer-vault` regressions pass;
- ADR 0020 and its index entry are accepted and consistent with ADRs 0007,
  0008, 0013, 0014, and 0019;
- full native and wasm CI-equivalent gates pass with warnings denied;
- algorithm, layer, record versions and all deterministic goldens remain
  unchanged;
- `world-model.md` current-state prose is accurate, roadmap A.2 and finding 26
  are explicitly completed/resolved, and unrelated findings remain open;
- this plan is marked Completed; and
- the complete item lands as one reviewed commit, with no edits to historical
  prototype/phase plans.
