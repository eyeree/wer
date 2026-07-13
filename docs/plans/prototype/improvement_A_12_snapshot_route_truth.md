# Improvement A.12 — Snapshot and route-sample truth

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.12](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [22](../../world-model.md#22-route-recording-and-traversal-depend-on-frame-sampling)
and [29](../../world-model.md#29-session-exactness-has-narrower-preconditions-than-its-headline)

This plan implements roadmap item A.12 in
[`docs/world-model.md`](../../world-model.md): **State and encode the truth of
snapshots and route samples**. The current code overstates two contracts. A
session snapshot restores player state, anchors, and resident `current`, but it
does not encode the runtime configuration, resident `target`, active route
recording, or route-tracker leg state that affect exact continuation. A route
node stores the covering region's aspirational `target` while its cost is
measured from visible current-world resonance, and the recorder emits at most
one node per frame, discarding travel overshoot.

The correction is a persistence and runtime-contract change. It does not change
base world generation, layer dependency hashing, route attraction math, preserve
ownership, executor semantics, renderer output, or the atlas merge law.

Do not modify [`implementation-plan.md`](implementation-plan.md),
`docs/plans/prototype/phase-N-plan.md`, or any `docs/plans/phase-N-plan.md`
file. Those are historical phase records. Update only current-model
documentation in `docs/world-model.md` after implementation, and mark roadmap
A.12 completed there once code, docs, migrations, and tests pass.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. A saved session carries the metadata required to decide whether exact restore
   is being attempted under the same runtime contract: world algorithm version,
   record format version through the envelope, effective streaming config,
   effective frame budget, resource-tier label when known, path-tracking
   toggles, and cache/organism-density knobs that affect realized availability.
2. Every resident region snapshot stores both authoritative `current` and
   authoritative `target` bit-exactly. Restoring a session must not silently
   replace target with current and then claim arbitrary exactness.
3. Active route recording state is part of the session tier. Saving while a
   recorder is active must preserve recorded nodes, attached discoveries,
   accumulated distance remainder, and the previous sample position needed to
   continue interval sampling exactly after load.
4. Route tracker leg state is part of the session tier when path tracking is
   active. Loading a session must not lose already-visited route nodes and then
   make later usage bumps depend on whether the player saved mid-leg.
5. Executor queues, in-flight generation jobs, disposable caches, rosters, and
   realized organism vectors remain outside the persisted session. The
   documentation must say this directly: exactness resumes after the documented
   zero-travel settle under matching metadata, not by replaying worker queues.
6. Route samples distinguish the aspirational possibility target from the
   visible current possibility state. New route records must encode both, or
   encode an explicit legacy/unknown-current state for old records; user-facing
   text must not imply that the stored target is what was visible.
7. Route recording samples every crossed `ROUTE_SAMPLE_SPACING` interval in one
   frame until `MAX_ROUTE_NODES` is reached. It must carry the travel remainder
   instead of resetting it to zero.
8. Interpolated route sample positions use the exact previous and current player
   positions supplied to the recorder, in travel order, with deterministic
   rounding at the persistence boundary.
9. Missing resident authority at an interpolated sample position must have a
   documented policy. Prefer retaining the unsatisfied interval remainder and
   retrying when the region is resident; do not skip an interval and move later
   nodes earlier in the route.
10. Route difficulty becomes distance-weighted for new records, using persisted
    segment/sample distance. Legacy records without distance metadata keep the
    old arithmetic mean fallback.
11. Existing valid v1 stores and atlas bundles remain readable. If the route or
    session schema changes, bump `RECORD_FORMAT_VERSION`, add pure migration for
    v1 bodies, and keep decode tests for old bytes.
12. Shareable route records remain integer/string only. Any new route fields
    must be quantized integer fields or optional integer-tagged legacy fields,
    never raw floats.
13. `WORLD_ALGORITHM_VERSION` remains 2, and no layer `algorithm_revision`
    changes. This item changes persistence truth and sampling behavior, not
    generation algorithms.
14. New v2 route content ids must be deterministic and content-derived. The
    migration policy for v1 route ids must be explicit: either preserve legacy
    ids under a legacy-node fold branch, or re-key migrated records only through
    a tested vault migration that updates storage keys safely.
15. `docs/world-model.md` must be updated after implementation so sections 2.8,
    3.7, 3.21/3.22 as applicable, finding 22, finding 29, and roadmap item
    A.12 describe the resolved behavior and remaining limitations.

## 2. Scope boundaries

### 2.1 In scope

- `world-core/src/record.rs` schema additions for session metadata, region
  target snapshots, route-sample current/target truth, route sample distance,
  and v1 migration if the format bumps.
- `world-runtime/src/vault.rs` session snapshot/restore APIs and compatibility
  checks for the new metadata.
- `world-runtime/src/route.rs` recorder interval sampling, route-recorder
  snapshot/restore state, and route-tracker snapshot/restore state.
- Native shell save/load plumbing for runtime metadata, active recorder state,
  tracker state, path toggles, and load-time compatibility reporting.
- `world-core/src/route.rs::route_difficulty` distance-weighted behavior for
  new records with a legacy fallback.
- Focused unit, runtime, tool, determinism/codec, and harness tests.
- Current-model documentation updates in `docs/world-model.md`.

### 2.2 Explicitly out of scope

- Editing historical phase plans or `implementation-plan.md`.
- Redesigning route traversal to require ordered segment progress and direction.
  That is the functional half of finding 22 already tracked separately in the
  roadmap as "Make routes represent traversal rather than unordered proximity."
  A.12 should persist the current tracker state honestly but does not need to
  replace the current node-set traversal rule.
- Changing route attraction selection, route pull strength, route pull cap,
  anchor canonicalization, or plausibility projection.
- Persisting executor queues, running closures, cache contents, rosters,
  organisms, renderer state, or GPU resources.
- Adding browser storage or asynchronous storage backends.
- Re-blessing generation goldens or changing layer output.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `RegionSnapshotRecord` | Stores `coord`, `current`, `stability`, and `revision`; `RegionMap::restore_region` sets `target = current`. | Store bit-exact `target` and restore it. Any later target refresh must be ordinary live work under matching inputs, not an unlabelled loss of snapshot state. |
| `SessionSnapshot` | Stores player, last player, bias, transition mode, anchors, regions, and sequence only. | Add primitive runtime metadata, active route recorder state, and route tracker state or explicitly fail/narrow exact restore when absent. |
| `Vault::snapshot_session` | Accepts only map/player/bias/anchors and cannot record config or path subsystem state. | Accept a typed runtime/session input struct so call sites cannot forget metadata when new fields are added. |
| Native `save_session`/`load_session` | Saves no tier/config/toggles; load resets recorder/tracker and constructs `RegionMap::new(*self.map.config())`. | Save effective config/budget/toggles/route transient state; load checks metadata against the current run and restores route transient state when compatible. |
| `RouteRecorder::observe` | Adds `travel`, emits at most one node, then resets accumulated distance to zero. | Track previous position and accumulated remainder; emit one node per crossed interval by interpolation until not enough distance remains or node cap is hit. |
| `RouteRecorder::observe` first node | First node drops at the current player position without establishing a previous interpolation baseline. | Store the starting position as the baseline and emit the first node immediately; later intervals interpolate from the last observed position plus carried remainder. |
| Missing region under sample | Current code returns without resetting accumulated distance only before a due node; once a node is due and region exists at final player, overshoot is lost. | Define sample-by-sample admission: if a due sample's covering region is missing, keep the interval due and do not consume later distance. |
| `RouteNode.signature` | Means target, while cost comes from current-world resonance. | Rename/document as target in code/docs and add `current_signature` or an explicit unknown-current legacy state. |
| `route_difficulty` | Arithmetic mean of node costs. | Use persisted distance weights for new records; keep old mean for legacy nodes without weights or zero total distance. |
| Route tracker | Current leg state lives only in memory. | Add deterministic snapshot/restore of the current `visited` map so save/load does not change usage bump outcomes. |

## 4. Persistence schema design

### 4.1 Format version

This item is expected to bump `RECORD_FORMAT_VERSION` from 1 to 2 because it
adds persisted fields to `SessionSnapshot`, `RegionSnapshotRecord`, and
`RouteNode`. Keep `WORLD_ALGORITHM_VERSION` unchanged.

Implement decode as:

```text
if envelope.format_version == 1:
  decode v1 body shape
  migrate to current in-memory shape
elif envelope.format_version == RECORD_FORMAT_VERSION:
  decode current body shape
else if envelope.format_version > RECORD_FORMAT_VERSION:
  reject UnsupportedFormat
```

Keep v1 structs private in `record.rs` or a `record::migration` module. Do not
try to deserialize v1 bytes into the v2 type with defaulted serde fields; that
makes compatibility implicit and fragile.

### 4.2 Region snapshots

Change `RegionSnapshotRecord` to include:

```text
coord
current: [f32; POSSIBILITY_DIMS]
target: [f32; POSSIBILITY_DIMS]
stability: f32
revision: u32
```

V1 migration sets `target = current` and marks the containing session as
`legacy_target_policy = TargetEqualsCurrent` through session metadata. New v2
snapshots set `target` from `RegionState::target.dims`.

`RegionMap::restore_region` should restore both `current` and `target`, leave
the restored authority parked, and still dirty/rederive fields on admission as
today. If the next zero-travel settle recomputes the same target from live
inputs, nothing changes; if inputs differ, the metadata compatibility warning
explains why exactness was not claimed.

### 4.3 Session metadata

Add primitive, platform-neutral snapshot structs in `world-core/src/record.rs`
so `world-core` does not depend on `world-runtime`:

```text
SessionRuntimeRecord {
  stream: StreamConfigRecord,
  budget: BudgetRecord,
  tier: SessionTierRecord,
  path_tracking: bool,
  route_attraction: bool,
}

StreamConfigRecord {
  near_radius: f64,
  far_radius: f64,
  load_radius: f64,
  unload_radius: f64,
  converge_per_unit: f32,
  converge_rate_cap: f32,
  field_resolution: u16,
  max_field_cache_bytes: u64,
  max_macro_cache_bytes: u64,
  max_roster_cache_bytes: u64,
  organisms_per_cell: u16,
}

BudgetRecord {
  max_loads: u64,
  max_converge_regions: u64,
  max_regen_cost: u32,
  max_realize_organisms: u64,
  max_persist_ops: u64,
  max_route_attraction_nodes: u64,
  max_retarget_regions: u64,
}

SessionTierRecord = Unknown | Low | Mid | High
```

Use `u64` rather than `usize` in the record. Runtime conversion can reject
values that do not fit the current platform, which is cleaner than letting
serde encode platform-width assumptions.

Session metadata is not a public atlas contract. It is a run-local exactness
label and compatibility check. On load:

- exact-compatible: current effective config/budget/toggles match the snapshot;
- compatible but not exact: values differ only in fields documented as pacing
  only for the requested operation;
- incompatible: field resolution, organism density, window radii, convergence
  constants, or route toggles differ in a way that changes exact continuation.

The native shell should log the result. Tests should assert the typed result
directly rather than scraping logs.

### 4.4 Route transient session state

Add serializable session-only records:

```text
RouteRecorderSnapshot {
  accumulated: f64,
  last_observed: Option<(f64, f64)>,
  nodes: Vec<RouteNode>,
  discoveries: Vec<u64>,
}

RouteTrackerSnapshot {
  legs: Vec<RouteTrackerLegSnapshot>,
}

RouteTrackerLegSnapshot {
  route_id: u64,
  visited_nodes: Vec<u32>,
}
```

`RouteTrackerSnapshot.legs` must be sorted by `route_id`, and each
`visited_nodes` list must be sorted unique. The runtime should expose
`RouteRecorder::snapshot/from_snapshot` and `RouteTracker::snapshot/from_snapshot`
instead of making their maps public.

If no recorder is active, store `None`. If path tracking is disabled, store an
empty tracker state and restore an empty tracker. V1 sessions migrate with
`recorder = None` and empty tracker, and metadata should label that they cannot
prove exact continuation across an active recording or mid-leg traversal.

## 5. Route record design

### 5.1 Node truth fields

Replace or extend the current route node shape so new nodes carry:

```text
pos_q: (i64, i64)
target_signature: PossibilitySignature
current_signature: Option<PossibilitySignature>
cost_q: u8
stability_q: u8
anchor_sig: u64
distance_q: u32
```

`target_signature` is the covering region's steered target. `current_signature`
is the covering region's visible realized current at sample time. `None` is
reserved for v1 legacy nodes whose visible-current truth was not encoded.
`distance_q` is the rounded world-distance represented by this node since the
previous node; the first node uses zero. New recorder output should always set
`current_signature = Some(...)` and a nonzero `distance_q` for non-initial
interval nodes.

If the implementation keeps the public field name `signature` for compatibility,
document it as a deprecated alias for `target_signature` in comments and
current-model docs. Do not let user-facing text say "signature" without saying
whether it is target or current.

### 5.2 Content ids and legacy ids

Prefer preserving v1 route ids for migrated legacy records with an explicit
legacy-node fold branch:

```text
if every node has current_signature == None and distance_q == 0:
  fold exactly the v1 fields in the v1 order
else:
  fold a v2 route-node tag, pos, target seed, current-present/current seed,
  cost, stability, anchor signature, and distance
```

This lets old stores open without re-keying `route/<id>` files, while new route
records receive content ids that honestly cover the new truth fields. Add tests
that a v1 golden route still decodes with the old id and that a v2 route changes
id when only `current_signature` or `distance_q` changes.

If preserving ids proves too awkward, implement a vault key migration that
decodes `route/<old-id>`, writes `route/<new-id>`, removes the old key only
after the new durable write succeeds, and reports progress/failure through the
existing dirty-key machinery. This is higher risk and should be avoided unless
necessary.

### 5.3 Difficulty

Change `route_difficulty` to:

```text
if any node has distance_q > 0:
  sum(cost_q / 255 * distance_q) / sum(distance_q)
else:
  arithmetic mean, matching v1 behavior
```

The first node's zero distance naturally contributes no cost. This makes
difficulty a property of represented travel distance, not frame count.

## 6. Recorder interval sampling

Refactor `RouteRecorder` around a previous-position baseline:

```text
observe(map, player, travel, effective_anchors, resonance_strength):
  if first observation:
    emit initial node at player if resident
    last_observed = player
    return

  segment_start = last_observed
  segment_end = player
  segment_len = hypot(delta)
  available = accumulated + segment_len
  while available >= ROUTE_SAMPLE_SPACING and nodes.len < MAX_ROUTE_NODES:
    distance_from_segment_start = ROUTE_SAMPLE_SPACING - accumulated
    t = distance_from_segment_start / segment_len
    sample = lerp(segment_start, segment_end, t)
    if sample region missing:
      keep accumulated and last_observed unchanged enough to retry this due
      interval on the next observe
      return
    emit node at sample with distance_q = round(ROUTE_SAMPLE_SPACING)
    segment_start = sample
    segment_len = remaining distance to segment_end
    accumulated = 0
    available = segment_len
  accumulated += remaining segment_len
  last_observed = player
```

Handle zero-length frames explicitly: after the first node, zero travel should
not produce additional nodes. Clamp tiny floating error so a sample exactly at
the end of the segment is consumed once, not repeated.

All nodes emitted during one `observe` call may use the frame's supplied
`effective_anchors` and `resonance_strength`; document this as frame-level cost
sampling. Per-interpolated-position resonance would require recomputing the
resonance graph for positions the map did not update, and is out of scope.

## 7. Runtime and native API changes

Introduce a typed session input in `world-runtime/src/vault.rs`:

```text
SessionSnapshotInput<'a> {
  map: &'a RegionMap,
  player: (f64, f64),
  last_player: (f64, f64),
  bias: &'a [f32; POSSIBILITY_DIMS],
  transition_mode: bool,
  anchors: &'a [Anchor],
  runtime: SessionRuntimeRecord,
  recorder: Option<RouteRecorderSnapshot>,
  tracker: RouteTrackerSnapshot,
}
```

Then replace the many-argument `snapshot_session` call with a single input
parameter. This avoids adding more positional booleans and arrays to an already
wide API.

Add runtime conversion helpers:

- `StreamConfigRecord::from_runtime(&StreamConfig)` and
  `try_to_runtime() -> Result<StreamConfig, SessionConfigError>`;
- `BudgetRecord::from_runtime(&Budget)` and
  `try_to_runtime() -> Result<Budget, SessionConfigError>`;
- `SessionRuntimeRecord::compare_to_runtime(...) -> SessionCompatibility`.

Native `World::save_session` should build `SessionRuntimeRecord` from
`self.map.config()`, `self.budget`, selected tier, `path_tracking`, and
`route_attraction`; snapshot `self.recorder`; and snapshot `self.tracker`.

Native `World::load_session` should:

1. clone the snapshot from the vault;
2. compare snapshot runtime metadata to the current run;
3. log exact/compatible/incompatible status;
4. create the map from the snapshot's stream config when exact restore is
   allowed, or from current config only with an explicit non-exact warning;
5. apply session regions;
6. restore player, last player, bias, transition mode, anchors;
7. restore path toggles, recorder, and tracker state when compatible;
8. apply preserves and run the existing zero-travel settle path.

## 8. Documentation updates after implementation

Update `docs/world-model.md` only after the code and tests land.

Required edits:

- Section 2.8: describe session metadata, region targets, active recorder and
  tracker state, and the exactness compatibility check. Keep the statement that
  executor queues/caches/organisms are not persisted.
- Section 3.7: define route nodes as target plus optional/current visible
  signature, distance-weighted cost, and interval-complete sampling. Say that
  frame-level resonance is shared by all nodes emitted from one frame.
- Section 3.22 or the vault section: update record kinds, namespace behavior,
  and format v2 migration notes.
- Finding 22: mark the recording/sample-truth parts resolved by Improvement
  A.12, and leave ordered traversal semantics as the separate route traversal
  roadmap item if not implemented here.
- Finding 29: mark the session-precondition truth resolved, listing exactly
  what is encoded and what remains outside the contract.
- Roadmap item A.12: mark completed with a link to this plan and a concise
  summary of the implementation.

Do not update `docs/plans/prototype/implementation-plan.md` or any phase plan.

## 9. Test plan

Add focused tests before broad harness runs.

### 9.1 Unit tests

- `world-runtime/src/route.rs`: one large movement crossing three sample
  intervals emits three nodes and retains the correct remainder.
- `world-runtime/src/route.rs`: overshoot is carried across frames rather than
  discarded.
- `world-runtime/src/route.rs`: missing authority at a due interpolated sample
  does not skip that interval.
- `world-runtime/src/route.rs`: recorder snapshot/restore continues with the
  same next node as an uninterrupted recorder.
- `world-runtime/src/route.rs`: tracker snapshot/restore preserves current leg
  visited-node state and later usage bump outcome.
- `world-core/src/route.rs`: distance-weighted difficulty differs from the old
  mean when long and short segments have different costs, and legacy nodes use
  the old mean fallback.
- `world-core/src/record.rs`: v2 route content id changes when
  `current_signature` or `distance_q` changes.

### 9.2 Codec and migration tests

- Keep v1 encoded discovery/route/session fixture bytes and prove they decode
  through the migration path.
- Add v2 record-wire golden coverage for at least one route with
  `current_signature = Some` and nonzero `distance_q`.
- Add a v2 session snapshot round trip that preserves runtime metadata, target
  vectors, recorder state, and tracker state.
- Assert `peek_envelope` reports v2 for newly encoded records and still rejects
  future formats.

### 9.3 Runtime/tool tests

- Extend `crates/tools/tests/route.rs` so the real streaming recorder emits all
  intervals for a movement larger than `ROUTE_SAMPLE_SPACING`, persists current
  and target signatures, and reopens with identical route bytes.
- Extend `crates/tools/tests/persistence.rs` so save/load/settle also compares
  restored region targets and metadata compatibility.
- Add a native-shell unit test around save/load if practical: active recording
  and tracker state survive a session save/load boundary.
- Extend `wer-vault` (`crates/tools/src/vault.rs`) with a scenario that saves
  during active recording and mid-route leg, reloads, finishes the route/leg,
  and matches an uninterrupted run's resulting route nodes and usage bump.
- Extend `wer-scale` only if route/session truth differs across tiers after the
  schema change. The existing tier route-byte probe should remain exact for
  canonical slot-0 inputs.

### 9.4 Required final commands

Run the CI-equivalent commands from `AGENTS.md`:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
```

Also run the harnesses most directly tied to this work:

```sh
cargo run --bin wer-vault
cargo run --bin wer-scale
```

If a schema change updates deterministic record-byte goldens, the final review
must list exactly which record-format fixtures changed and confirm no generator
or layer-output golden was re-blessed.

## 10. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Format bump accidentally breaks old vaults. | Keep explicit v1 body structs and decode/migration tests for route and session records before changing write paths. |
| Migrated v1 route ids no longer match storage keys. | Prefer a legacy fold branch that preserves ids for nodes without current/distance truth; test old id stability. |
| Session metadata grows into platform-specific types. | Store primitive records in `world-core`; convert to/from `StreamConfig` and `Budget` in `world-runtime`. |
| Exactness is overstated again. | Return/log a typed `SessionCompatibility` result and document executor queues, caches, rosters, organisms, and mismatched configs as outside exact restore. |
| Multi-interval sampling changes route bytes unexpectedly across tiers. | Use only canonical slot-0 frame stats and quantized interpolated positions; add tier route-byte regression coverage. |
| Interpolated missing-region policy causes recorder stalls. | Retain the due interval and expose debug/test assertions; the recorder should resume once streaming catches up or stop at node cap. |
| Distance-weighted difficulty changes legacy displays. | Branch on presence of distance metadata; v1 routes keep the old arithmetic mean. |

## 11. Implementation order

1. Add v1 migration structs/tests around current route and session records so
   compatibility is protected before editing schemas.
2. Add v2 record structs/fields, bump `RECORD_FORMAT_VERSION`, update encode
   goldens, and implement content-id/difficulty rules.
3. Add runtime conversion records for `StreamConfig`, `Budget`, and session
   compatibility checks.
4. Refactor `Vault::snapshot_session` to take a typed input and write region
   targets plus metadata.
5. Restore region targets in `RegionMap::restore_region` and update
   save/load/settle tests.
6. Refactor `RouteRecorder` for previous-position interval sampling and add
   recorder snapshot/restore.
7. Add `RouteTracker` snapshot/restore without changing traversal semantics.
8. Wire native save/load through metadata, recorder, tracker, and compatibility
   reporting.
9. Extend `wer-vault`, route tests, persistence tests, and determinism/codec
   tests.
10. Update `docs/world-model.md`, marking A.12 completed and leaving the
    separate ordered-route traversal item open unless it was implemented under a
    separate, explicit scope change.
11. Run the final command set and audit version/golden diffs.
