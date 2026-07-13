# Improvement A.13 — Verification surface expansion

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.13](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [33](../../world-model.md#33-some-advertised-verification-is-absent-or-narrower-than-stated)

This plan implements roadmap item A.13 in
[`docs/world-model.md`](../../world-model.md): **Expand the verification surface
alongside these fixes**. This is the cross-cutting exit criterion for the A.1
through A.12 correctness work. It should not change world generation,
persistence semantics, route math, resource-tier behavior, or WebAssembly
exports except where a test-only probe must be added to expose already-existing
pure behavior.

The outcome is stronger machine coverage over claims that are currently absent
or narrower than stated: frame slicing, simultaneous cache ceilings, cross-tier
persistence, full settled-state equality, ordinary border behavior, multi-node
route softness, all-biome SIMD coverage, and executed wasm parity.

**Implementation result:** A.13 landed as verification-only work. It did not
bump `WORLD_ALGORITHM_VERSION`, layer `algorithm_revision` values, or record
format versions. The implementation strengthened `tools::replay::state_hash`
for settled organisms, added `wer-scale` scenarios for frame slicing,
all-cache pressure, and cross-tier persistence, added a multi-node route
softness test, fixed all-biome SIMD differential coverage, and documented that
the existing wasm parity suite is executed in Node. The ordinary
divergent-history border coverage was already present in `world-runtime` and
is now recorded here as part of the closed verification surface.

Do not modify [`implementation-plan.md`](implementation-plan.md),
`docs/plans/prototype/phase-N-plan.md`, or any `docs/plans/phase-N-plan.md`
file. Those are historical phase records. During implementation, update only
current-model documentation in `docs/world-model.md` and mark roadmap A.13
completed there once code, docs, and tests pass.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. `wer-scale` includes an alternate frame-slicing scenario. The same scripted
   journey must settle to an identical authoritative/derived state when run as
   normal frames, coalesced large-travel frames, and split small-travel frames.
2. `wer-scale` includes a simultaneous all-cache-ceiling scenario that pressures
   field, macro, roster, pool, and organism/realization working sets in one
   run. It must prove that ceilings may evict or rebuild disposable caches, but
   not change settled output.
3. Cross-tier persistence is covered. A save made under one resource tier must
   load under another tier with explicit compatibility reporting and produce
   the documented settled result: low-tier canonical identities survive, higher
   density adds only additive presentation identities, and incompatible exact
   continuation is not silently claimed.
4. The settled-state equality hash used by harnesses covers every component the
   docs claim it covers, or the docs are narrowed. At minimum, add coverage for
   organism world position, expression/presentation fields that are part of the
   settled near-field surface, and any restored override/session authority that
   affects a settled fixed point. Keep executor queues and in-flight closures
   outside "settled" equality, but assert they are empty before hashing.
5. Ordinary divergent-history borders are covered outside preserve/radius-drop
   special cases. A normal streaming update must exercise a border where
   neighboring regions have different realized P/G history and prove Terrain,
   Slope, Hydrology, Soils, and Biome stay continuous under the A.8 halo rule.
6. Multi-node route attraction softness is covered in the runtime/tool harness,
   not only with a singleton or dense co-located cap probe. A recorded route
   with several distinct nodes must pull softly near multiple segments, remain
   corridor-bounded, stay below the global route pull cap, and never replace
   the target outright.
7. SIMD differential tests exercise every biome id, including Ice (`id == 11`).
   The vegetation row generator must not accidentally use `next_below(11)` or
   any other range that excludes one legal biome.
8. Wasm parity continues to execute in Node, not merely compile. The parity
   suite should include any new portable test probe introduced for this item.
9. No generated-output goldens are re-blessed unless a separate intentional
   algorithm change explicitly requires it. A.13 is verification-only:
   `WORLD_ALGORITHM_VERSION` and layer `algorithm_revision` values should not
   change.
10. New tests must be deterministic, platform-neutral where they touch
    `world-core` or `world-runtime`, and compatible with the workspace's
    `RUSTFLAGS=-D warnings` CI policy.
11. `docs/world-model.md` must be updated after implementation so finding 33
    accurately describes what was added, any remaining limitations are explicit,
    and roadmap item A.13 is marked completed.

## 2. Scope boundaries

### 2.1 In scope

- New `wer-scale` scenarios and gates for frame slicing, all-cache pressure,
  cross-tier persistence, and full settled-state equality.
- Focused `world-runtime` tests for ordinary divergent-history borders if the
  existing private test hooks are better suited than `wer-scale`.
- Route softness/corridor/cap tests in `crates/tools/tests/route.rs` or the
  vault harness, using the real `RouteRecorder`, `Vault`, and
  `attraction_anchors` where practical.
- SIMD differential coverage adjustment in
  `crates/world-core/tests/simd_differential.rs`.
- Wasm parity test additions in `crates/platform-web/tests/wasm_parity.rs` and
  test-only/public parity exports in `crates/platform-web/src/lib.rs` only if
  needed.
- Current-model documentation updates in `docs/world-model.md`.

### 2.2 Explicitly out of scope

- Editing historical phase plans or `implementation-plan.md`.
- Changing base generation algorithms, layer dependency folds, route attraction
  formulas, persistence schemas, record ids, atlas merge laws, executor
  scheduling semantics, or resource-tier presets except for test-only setup.
- Adding browser storage, Web Workers, networked sharing, or a new renderer
  backend.
- Persisting executor queues or in-flight closures. The stronger hash should
  assert settled emptiness rather than make live work serializable.
- Treating cache telemetry as an honest process-memory cap. Finding 32 remains
  a separate performance/memory-accounting item.

## 3. Current verification map

| Area | Current coverage | A.13 addition |
|---|---|---|
| Schedule independence | `wer-scale::schedule_independence` compares inline, lane worker counts, budget scale, cancellation, and amortized retarget after settle. | Add alternate frame slicing for the same script: normal, split, and coalesced travel all settle to the same hash. |
| Memory ceilings | `wer-scale::memory_ceiling` pressures field cache and pool; runtime tests separately pressure field, macro, and roster recovery. | Add one simultaneous all-cache scenario with tight field/macro/roster ceilings and realization pressure, then compare against roomy replay. |
| Tier identity | `wer-scale::tier_identity` checks generated surfaces, canonical organisms, capture/resonance, and encoded route records across Low/Mid/High. | Add save/load across tiers and assert exactness is claimed only for compatible metadata while settled canonical identities remain invariant. |
| Settled hash | `tools::replay::state_hash` includes regional authority, field/macro/roster caches, and organism id/species/slot. | Extend or rename it so equality covers the claimed settled surface, including organism position and expression fields, and asserts no queued/in-flight work. |
| Border continuity | A.8 added focused divergent-history border tests for special cases and wasm parity for topology probes. | Add an ordinary live-streaming divergent-history border case through Terrain, Slope, Hydrology, Soils, and Biome. |
| Route softness | Vault and route tests cover singleton usage, dense cap, corridor bound, and one recorded route path. | Add explicit multi-node, multi-segment softness gates near distinct route nodes/segments, including target-not-replaced assertions at several probes. |
| SIMD parity | Row kernels are bit-equal to scalar twins, but vegetation randomized biome ids currently exclude Ice if using `next_below(11)`. | Cover all `BIOME_COUNT` ids deterministically and keep randomized coverage around them. |
| Wasm parity | `wasm-pack test --node crates/platform-web` executes public parity probes in Node. | Keep it as a required gate and add any missing portable probe needed by A.13. |

## 4. Implementation plan

### 4.1 Strengthen settled-state hashing

1. Audit `tools::replay::state_hash` against the current docs and finding 33.
   It already folds regional authority, field cache content, macro cache
   content, roster entries, and organism id/species/slot.
2. Add the missing stable settled organism surface fields: trophic role, cell,
   world position bits, jitter if represented separately, and expression/body
   presentation fields that are part of `Organism`.
3. Include authoritative organism identity keys if they are public enough to
   access without breaking module boundaries; otherwise add a small
   platform-neutral diagnostic iterator on `RegionMap`.
4. Do not fold executor queues directly. Instead add a helper such as
   `assert_settled_for_hash(map, executor, player)` or keep the existing
   `settled` gate and ensure all callers hash only after `queue_len == 0`,
   `jobs_in_flight == 0`, and authoritative realization is complete.
5. Rename comments if necessary so "full settled state" means the settled
   authoritative/derived runtime state, not live executor internals or GPU
   presentation.

Tests:

- Update `crates/tools/tests/persistence.rs`, vault harness expectations, and
  `wer-scale` scenarios that compare state hashes.
- Add a focused regression that mutates/captures two maps differing only in an
  organism position/expression field and proves the hash changes, if such a
  mutation can be done without exposing test-only internals.

### 4.2 Add frame-slicing schedule scenario

Add a `frame_slicing` scenario to `crates/tools/src/scale.rs` and include it in
`run_scale_harness`.

Recommended shape:

1. Define a deterministic list of logical travel segments using the existing
   storm script and neutral run-out.
2. Run three equivalent journeys:
   - baseline: current one-update-per-frame script;
   - split: each logical segment divided into two to four smaller updates whose
     travel sums to the same value;
   - coalesced: pairs of adjacent logical segments merged into one update with
     summed travel and final position.
3. Stop and settle all maps with the same `settle_fixed_point` helper.
4. Gate on equal strengthened `state_hash` values and nonempty settled windows.
5. Report the number of logical segments, physical updates, and final hash.

Keep the scenario deterministic and modest in `ScaleConfig::quick()` so
`cargo test -p tools` remains practical.

### 4.3 Add simultaneous all-cache pressure scenario

Add an `all_cache_ceiling` `wer-scale` scenario rather than expanding the
existing field-only `memory_ceiling` until it becomes unreadable.

Recommended shape:

1. Use a small resolution and window with nontrivial macro coverage and several
   habitat signatures.
2. Create a tight config with:
   - `max_field_cache_bytes` below the full field window but above the near
     indispensable floor;
   - `max_macro_cache_bytes` low enough to force orphan/least-needed macro
     eviction during travel;
   - `max_roster_cache_bytes` low enough to force roster eviction down to the
     resident protected set;
   - `organisms_per_cell` high enough to exercise realization/pool churn.
3. Run a paired roomy config with all cache ceilings effectively unlimited.
4. Drive both through the same route: settle, travel far enough to evict, return
   to the probe window, and settle again.
5. Gate that:
   - each cache family observed pressure (`evicted_for_capacity`,
     macro-cache bytes below roomy or macro evictions if exposed, roster
     eviction/build counts, pool bytes bounded);
   - tight and roomy strengthened state hashes match after final settle;
   - near-window content probes match before and after return;
   - all layer diagnostics are current in the final probe window.

If macro or roster eviction counts are not exposed in `FrameStats`, either add
minimal deterministic telemetry or use observable byte/build deltas. Do not add
allocator-observed process memory gates here; that belongs to finding 32.

### 4.4 Add cross-tier persistence scenario

Add a `cross_tier_persistence` scenario to `wer-scale`, or extend
`tier_identity` only if the result stays readable.

Recommended shape:

1. For each pair of source and destination tiers, settle a source map with a
   vault, active route/capture content, and a known canonical organism set.
2. Save a session snapshot through the existing vault APIs.
3. Load under the destination tier/config.
4. Assert the compatibility result matches the A.12 truth:
   - same metadata can claim exact continuation;
   - changed tier/config must report non-exact or compatibility mismatch rather
     than silently claiming exactness.
5. After the documented zero-travel settle, compare canonical tier-invariant
   surfaces and slot-0 organism identities to the source where appropriate.
6. For higher-density destinations, assert additional organisms are additive
   and slot 0 remains identical. For lower-density destinations, assert extra
   presentation slots disappear without changing canonical identities.

This scenario should reuse `MemoryStorage` and should not alter
`RECORD_FORMAT_VERSION`.

### 4.5 Add ordinary divergent-history border coverage

The existing runtime private tests around A.8 already exercise radius-drop,
preserve, queued stale halo, and border key correction paths. Add a new ordinary
case that does not rely on preserve ownership or manual radius drops.

Recommended shape in `world-runtime` tests:

1. Create two adjacent resident regions with different realized P/G history
   through normal movement/steering and convergence, not by directly editing
   cache internals unless no public path can make the fixture stable.
2. Ensure both sides are field-active and current.
3. Inspect the border row/column for:
   - Terrain elevation continuity;
   - Slope channel continuity/ghost derivation;
   - Hydrology river/wetness boundedness across the border;
   - Soils depth/fertility boundedness across the border;
   - Biome classification staying derived from the same border inputs.
4. Compare forward and reverse approach orders to catch history-order leaks.

Use the existing A.8 tolerances and helper style in `world-runtime/src/stream.rs`
tests where possible. If the ordinary public setup is too slow for a unit test,
put the heavier version in `wer-scale` and keep a smaller targeted unit probe.

### 4.6 Expand multi-node route verification

Add route tests that use several distinct route nodes and probe multiple
positions.

Recommended checks:

1. Record a route through `RouteRecorder` over the real `RegionMap`, then
   persist and reload through `Vault`.
2. Bump usage below saturation and compare attraction at:
   - a node near the beginning;
   - a midpoint segment;
   - a node near the end;
   - a point just outside the corridor;
   - a far-off point.
3. At in-corridor probes, assert:
   - `attraction_anchors` is nonempty;
   - `anchor_influence_profile` never exceeds `ROUTE_PULL_CAP`;
   - steered target differs from plain target but every domain movement remains
     below a strict softness threshold and below replacement with the recorded
     route signature;
   - selected anchors are ordered deterministically for the same route set.
4. At outside probes, assert no anchors and identical target bits.
5. Keep the existing dense co-located cap test; this new test is for ordinary
   multi-node behavior.

### 4.7 Cover every biome in SIMD differential tests

In `crates/world-core/tests/simd_differential.rs`:

1. Replace any `rng.next_below(11)` biome generation with a `BIOME_COUNT`-based
   range.
2. Add a deterministic prefix or separate test that enumerates all biome ids at
   least once, including Ice (`11`), before filling the rest of the row with
   randomized legal ids.
3. Keep bit-equality assertions between `vegetation_row` and
   `vegetation_row_scalar`.

This is a test-only correction and must not change row-kernel code unless the
new coverage exposes a real bug.

### 4.8 Preserve executed wasm parity

The current CI command, `wasm-pack test --node crates/platform-web`, executes
`crates/platform-web/tests/wasm_parity.rs` in Node. A.13 should keep that as the
definition of wasm parity.

Implementation work:

1. If no new portable parity probe is required, update docs to say A.8/A.13
   execute the existing parity suite in Node.
2. If the strengthened settled hash or route multi-node check needs a portable
   pure sample, add a small deterministic export in `platform-web/src/lib.rs`
   and a matching native golden in `world-core` or `tools` tests.
3. Do not expose live `RegionMap` or storage through wasm just for this item.

## 5. Documentation updates

After implementation, update `docs/world-model.md` only.

Required edits:

1. Mark roadmap item A.13 as completed and link to this plan.
2. Rewrite finding 33 from "Status: Open" to a completed/resolved status.
3. List the new verification surface concretely: frame slicing, all-cache
   pressure, cross-tier persistence, strengthened settled hash, ordinary border
   coverage, multi-node route softness, all-biome SIMD, and executed wasm
   parity.
4. Preserve honest limitations:
   - executor queues and in-flight closures are not persisted or hashed as
     state; settled equality asserts they are empty;
   - byte ceilings remain logical cache budgets, not full heap ceilings
     (finding 32);
   - route traversal ordering, if still not implemented, remains separate from
     multi-node attraction softness.
5. Do not update `implementation-plan.md` or historical phase plan files.

## 6. Suggested implementation order

1. Strengthen `state_hash` and update existing hash-based tests.
2. Fix all-biome SIMD coverage; this is isolated and should pass quickly.
3. Add route multi-node softness tests.
4. Add ordinary divergent-history border coverage.
5. Add `wer-scale::frame_slicing`.
6. Add `wer-scale::all_cache_ceiling`.
7. Add `wer-scale::cross_tier_persistence`.
8. Update wasm parity only if a new pure portable sample is needed.
9. Update `docs/world-model.md` and mark A.13 completed.
10. Run the full CI-equivalent verification list.

## 7. Required verification commands

Implementation-time focused verification run:

- `cargo test -p world-core --test simd_differential vegetation_row_is_bit_identical` — passed.
- `cargo test -p tools --test route multi_node_route_attracts_softly_near_distinct_segments` — passed.
- `cargo test -p tools scale::tests::scaled_budget_always_admits_the_largest_atomic_layer` — passed.
- `cargo test -p tools scale::tests::quick_harness_passes` — passed.

The remaining full CI-equivalent commands below are still the expected release
gate before merging broadly; A.13 did not require wasm export changes, so the
existing Node wasm parity suite remains the parity surface.

Run the normal CI-equivalent gates before marking A.13 complete:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
```

Also run the sign-off harnesses directly, including the full scale harness:

```sh
cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale
```

Use `cargo run --release --bin wer-scale -- --report` only if the
implementation changes reported baseline tables; A.13 should not require
perf-baseline re-blessing unless scenario reporting is intentionally expanded.

## 8. Risks and mitigations

1. **Harness runtime growth.** New `wer-scale` scenarios can make CI too slow.
   Keep `ScaleConfig::quick()` small for unit tests and reserve the full run for
   the release harness command.
2. **Hash scope creep.** Folding live executor or GPU state would make equality
   schedule- or platform-dependent. Hash settled runtime state only and assert
   queues/in-flight work are empty.
3. **Overclaiming memory ceilings.** All-cache pressure proves deterministic
   cache behavior under logical byte budgets. It must not claim full process
   memory accounting.
4. **Tier exactness ambiguity.** Loading a snapshot under a different tier must
   not silently claim exact continuation. Tests should assert the compatibility
   result, not just the final settled hash.
5. **Ordinary border fixture fragility.** If a fully public movement fixture is
   too hard to stabilize, use existing internal test helpers but keep the setup
   focused on ordinary streaming history rather than preserve/radius-drop
   special cases.
6. **Wasm surface bloat.** Add wasm exports only for pure portable probes with
   clear native goldens. Do not expose runtime/session machinery to wasm for
   this verification item.
