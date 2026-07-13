# Improvement A.10 — Content equality and canonical set encoding

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.10](../../world-model.md#prioritized-improvement-roadmap)

**Findings addressed:** [24](../../world-model.md#24-a-64-bit-content-fold-is-not-proof-of-immutable-equality)
and [25](../../world-model.md#25-canonical-sets-preserve-duplicate-multiplicity)

This plan implements roadmap item A.10 in
[`docs/world-model.md`](../../world-model.md): **Harden content equality and
canonical set encoding**. The current vault and atlas code validates that a
record's stored `id` equals its recomputed `content_id`, but merge then assumes
same id implies same immutable body. Bundle canonicalization sorts record ids
without rejecting duplicate ids, `PreserveRecord::new` sorts coordinates without
deduplicating them, and route discovery references retain duplicate ids despite
being described as a set. These gaps are harmless under trusted local use but
are not strong enough for public or adversarial atlas exchange.

The correction is a persistence-boundary hardening change. It introduces
explicit immutable-body equality checks for same-id records, validates or
normalizes canonical sets at every constructor/import/check boundary, and adds a
wider cryptographic digest for public/untrusted bundle identity. It does not
change base-world generation, layer output, route attraction, preserve ownership
rules, or deletion semantics.

Do not modify [`implementation-plan.md`](implementation-plan.md),
`docs/plans/prototype/phase-N-plan.md`, or any `docs/plans/phase-N-plan.md`
files. Those are historical phase records. Update only current-model
documentation in `docs/world-model.md` after implementation, and mark roadmap
A.10 completed there once the code, docs, and tests pass.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. Same-id records are merged only when their immutable bodies are equal under a
   typed equality predicate. Do not rely on a `debug_assert_eq!(id)` or the
   64-bit `content_id` alone.
2. On an id match with unequal immutable content, import/open/check rejects the
   incoming record as a collision or tamper case and reports a structured issue.
   It must not merge mutable fields, overwrite local content, or partially apply
   the record.
3. Mutable fields remain outside content identity: discovery, route, and
   preserve `name`, `journal`, `sequence`, and route `usage` continue to merge
   by the existing deterministic rules after immutable equality has passed.
4. The existing 64-bit `content_id` remains the local storage key for v1 records
   unless the implementation deliberately bumps `RECORD_FORMAT_VERSION` with a
   migration. The intended A.10 path should avoid a format bump by adding
   validation and public digest metadata outside existing record bodies where
   possible.
5. Public/untrusted atlas exchange exposes or validates a cryptographic digest
   that covers canonical shareable bundle content. The digest is for
   adversarial collision resistance and bundle identity; it does not replace the
   existing local key namespace in this item.
6. Canonical bundle encoding is a function of mathematical sets, not caller
   vector multiplicity. Duplicate records by id are either rejected or
   deterministically collapsed only when their immutable bodies are equal and
   mutable fields are merged by the normal record merge law.
7. Preserve region membership is a true set of coordinates. A preserve cannot
   contain the same `RegionCoord` more than once with different signatures, and
   its identity cannot depend on repeated equal coordinates.
8. Route discovery references have an explicit duplicate policy. Prefer
   deduplicating them as an ordered set of discovery ids unless duplicate
   references are proved semantically meaningful; whichever policy is chosen
   must be documented and tested.
9. Empty route paths and empty preserve region sets remain invalid for public
   bundles. Keep or strengthen the existing `wer-atlas check` findings.
10. Canonicalization is deterministic and platform-neutral: sort by integer keys,
    fold explicit counts, and avoid filesystem, thread, renderer, or platform
    APIs in `world-core` and `world-runtime`.
11. Existing valid v1 records continue to decode. If legacy records with
    duplicate preserve coordinates or duplicate bundle ids are encountered,
    readers must use a documented compatibility path: normalize when
    unambiguous, otherwise skip/reject and report.
12. `WORLD_ALGORITHM_VERSION` remains 2, and no layer `algorithm_revision`
    changes. This item changes record validation and exchange semantics, not
    generated world output.
13. `RECORD_FORMAT_VERSION` remains 1 unless the final design stores new fields
    inside persisted record bodies. If it changes, add a migration and update
    byte goldens in the same commit with an explicit format-change note.
14. Deletion/tombstone and per-replica route usage counters are documented as
    out of scope for this item unless they are fully designed and implemented.
    Do not claim deletion or usage are fully CRDT-compatible after A.10 merely
    because content equality is hardened.
15. `docs/world-model.md` must be updated after implementation so findings 24
    and 25 describe the resolved content-equality and canonical-set behavior,
    any remaining tombstone/counter limitations are still called out, and
    roadmap item A.10 is marked completed.

## 2. Scope boundaries

### 2.1 In scope

- Typed immutable-body equality predicates for `DiscoveryRecord`,
  `RouteRecord`, and `PreserveRecord`.
- Import/open/check rejection for same-id unequal immutable bodies.
- Bundle canonicalization that sorts and handles duplicate ids explicitly.
- Preserve-region coordinate uniqueness and route discovery-reference
  duplicate policy.
- A cryptographic digest for canonical public atlas content, with CLI/check
  reporting and tests.
- Focused unit, runtime, atlas, file-backend, and determinism/codec regression
  tests.
- Current-model documentation updates in `docs/world-model.md`.

### 2.2 Explicitly out of scope

- Changing base-world generation, layer dependency hashes, terrain/drainage
  output, organisms, steering, route attraction, or preserve conflict
  ownership.
- Editing historical phase plans or `implementation-plan.md`.
- Retrofitting deletion tombstones, garbage collection, authenticated user
  signatures, or per-replica grow-only route usage counters unless the scope is
  deliberately expanded with a separate design.
- Replacing every local storage key with a cryptographic digest. The local v1
  keyspace can stay `disc/<id>`, `route/<id>`, and `pres/<id>`.
- Trusting a digest alone. The typed content-id and immutable-body validators
  still run after decode.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `world-core/src/record.rs::merge_from` | `debug_assert_eq!(id)` then merges mutable fields. Release builds do not check immutable equality. | Add typed immutable equality and make merge return a collision/error result or require callers to check before mutation. |
| `world-runtime/src/vault.rs::import` | Rejects `id != content_id`, then calls `merge_from` for same ids. | Reject same-id unequal immutable bodies, record a nonfatal issue, and leave existing records untouched. |
| `world-runtime/src/vault.rs::open` | Loads one key per id and inserts after `id == content_id`. | Validate canonical inner sets and immutable body shape before insertion; report and skip legacy ambiguous duplicates. |
| `world-core/src/record.rs::AtlasBundle::canonicalize` | Sorts `discoveries`, `routes`, and `preserves` by id but keeps duplicate ids. | Sort and either deduplicate identical records by merge law or return a duplicate/collision report. |
| `crates/tools/src/atlas.rs::check_bundle` | Flags id mismatches, empty routes, empty preserves, and unsorted canonical form. | Also flag duplicate ids, same-id unequal bodies, duplicate preserve coordinates, non-canonical route discovery refs, and digest mismatches. |
| `PreserveRecord::new` | Sorts region entries only. Duplicate coordinates remain and the last duplicate can win in inspection/runtime application. | Canonicalize coordinates as a set and reject conflicting duplicate signatures. |
| `RouteRecord::new` | Stores discovery refs in caller order with multiplicity. | Define and enforce a stable ordered unique list unless multiplicity is intentionally retained. |
| `tools::effective_covering_preserve` | Mirrors legacy last-duplicate behavior. | Remove the legacy behavior after preserve records cannot contain duplicate coordinates; keep only compatibility tests if readers still normalize old records. |
| Public atlas identity | Uses small `u64` ids intended for deterministic local content addressing, not adversarial authentication. | Add a wider digest over canonical shareable bundle bytes/content for public exchange and CLI validation. |

## 4. Required design

### 4.1 Typed immutable equality

Add explicit predicates in `world-core/src/record.rs`, for example:

```text
DiscoveryRecord::immutable_eq(&self, other: &Self) -> bool
RouteRecord::immutable_eq(&self, other: &Self) -> bool
PreserveRecord::immutable_eq(&self, other: &Self) -> bool
```

The predicates compare exactly the fields folded by `content_id` and exclude
only mutable fields:

- discovery: `source`, `signature_seed`, `target`, `mask`, `kind`,
  `strength_q`, `falloff_q`, and `pos_q`;
- route: canonical `nodes` and canonical discovery refs;
- preserve: canonical coordinate/signature entries.

Then change merge plumbing so accidental caller misuse is hard. Two acceptable
shapes:

1. make `merge_from` return `Result<bool, RecordMergeError>` and check
   `immutable_eq` internally before applying mutable fields; or
2. keep `merge_from` private/internal and expose a checked wrapper used by
   `Vault::record_*`, `Vault::import`, and any tests.

Prefer the `Result` API because it moves the safety invariant into the type
surface. `RecordMergeError` should identify at least `IdMismatch` and
`ImmutableConflict`, derive `Debug + Clone + PartialEq + Eq`, and implement
`Display`.

### 4.2 Canonical duplicate policy

Define one policy per vector field and encode it in constructors, validators,
and docs.

1. Bundle record vectors are keyed sets by `id`. Duplicates with equal immutable
   bodies are collapsed by the same mutable merge law used during import.
   Duplicates with unequal immutable bodies are rejected as collisions.
2. Preserve regions are keyed sets by `RegionCoord`. Exact duplicate
   coordinate/signature pairs are collapsed. Duplicate coordinates with
   different signatures are invalid because they would make ownership and
   identity depend on multiplicity/order.
3. Route discovery refs should be sorted and deduplicated by id before
   `RouteRecord::content_id` unless route-journal multiplicity is declared
   semantic. This plan recommends sorted unique refs because route identity
   should name the set of discoveries made along the path, while node order
   already captures traversal.

Make the canonicalizers return a status rather than silently hiding malformed
input. Suggested names:

```text
PreserveRecord::try_new(...)
RouteRecord::new(...)  // internally canonicalizes discovery refs
AtlasBundle::canonicalize_checked() -> Result<(), BundleCanonicalError>
AtlasBundle::canonicalized(self) -> Result<Self, BundleCanonicalError>
```

Keep ergonomic constructors for trusted runtime recording, but ensure they
cannot produce non-canonical records. For tests that need malformed legacy or
tampered bodies, construct values manually and recompute ids deliberately.

### 4.3 Compatibility for existing v1 records

A.10 should avoid making old readable records fail unless they are ambiguous.
Use these compatibility rules:

- exact duplicate preserve entries with the same signature can be normalized to
  one entry, then marked dirty on next local write/import if the vault owns the
  record;
- duplicate preserve coordinates with different signatures are skipped/rejected
  with an issue because no set value is implied;
- duplicate route discovery ids normalize to one id if the chosen route policy
  is set semantics;
- duplicate bundle record ids normalize only if immutable bodies match and
  mutable fields converge by merge law.

If normalizing an opened local record changes its body, decide explicitly
whether to queue the normalized record dirty. Prefer queuing dirty for local
stores so subsequent exports are canonical, but ensure an import/check path can
validate without mutating.

### 4.4 Public cryptographic digest

Add a public digest over canonical shareable atlas content. The exact location
depends on whether the team wants a v1 body-format change.

Preferred no-format-bump path:

- add a dependency such as `sha2` in root `[workspace.dependencies]` and use it
  from `world-core` or `tools` with `workspace = true`;
- define `AtlasDigest([u8; 32])` in `world-core/src/record.rs` or a small
  tools-facing module;
- compute it from the already canonical `encode_record(RecordKind::Bundle,
  &canonical_bundle)` bytes, or from a documented kind-tagged field fold that
  is independent of envelope version if the digest should survive envelope
  migration;
- expose it through `tools::check_bundle`, `wer-atlas list/check`, and tests.

If the digest must be embedded inside exported files, bump
`RECORD_FORMAT_VERSION`, introduce a v2 bundle wrapper, and add a v1 migration.
Do this only if there is a concrete consumer requirement, because it expands
the blast radius and byte golden updates.

The digest is not a signature. It detects accidental/adversarial collision or
tamper when the digest was obtained over a trusted channel, but it does not
authenticate the author. Keep optional authored signatures documented as later
work.

### 4.5 Vault import/open behavior

Update `world-runtime/src/vault.rs` so all incoming shareable records pass the
same validation pipeline:

1. decode and kind-check through the existing record codec;
2. validate canonical inner sets;
3. validate `id == content_id`;
4. if an existing record with the id exists, require immutable equality before
   mutable merge;
5. only after all checks pass, mutate indexes and dirty sets.

`MergeStats.rejected` can continue to count all rejected records. The issue
identity should distinguish content-id mismatch from immutable conflict so
repeated bad imports deduplicate cleanly in the bounded issue registry.

For local duplicate keys during `Vault::open`, the file namespace already has
one key per id. The important open-time checks are malformed inner sets and
future migrated records whose stored key id disagrees with the decoded record
id. If feasible, validate the key suffix matches the decoded id and report a
wrong-key issue; this is adjacent hardening and cheap while touching the loader.

## 5. Implementation sequence

1. Add immutable equality predicates and merge error types in
   `world-core/src/record.rs`.
2. Add canonical validation helpers for preserve regions, route discovery refs,
   and atlas bundles. Keep constructors canonical for trusted runtime records.
3. Update `Vault::record_discovery`, `record_route`, and `record_preserve` to
   use the checked merge API for re-recording local duplicates.
4. Update `Vault::import` to reject same-id immutable conflicts without
   mutating existing state, and to count/report duplicate/collision records
   deterministically.
5. Update `Vault::open` namespace loaders to validate canonical inner sets and,
   if implemented, storage-key/id agreement.
6. Update `tools::encode_bundle`, `decode_bundle`, and `check_bundle` to use
   checked canonicalization and report duplicate-id/duplicate-coordinate
   findings.
7. Add the cryptographic digest helper and surface it in `BundleCheck` plus the
   `wer-atlas` CLI output/check path.
8. Remove or replace the legacy
   `effective_covering_preserve` last-duplicate behavior once duplicate
   preserve coordinates are invalid. Keep a compatibility test only if old
   exact duplicates are normalized.
9. Update comments and module docs in `record.rs`, `vault.rs`, and `atlas.rs`
   so they no longer claim same id proves immutable equality or that sorted
   vectors alone are canonical sets.
10. Update `docs/world-model.md` current-model text, findings 24 and 25, and
    roadmap A.10. Mark A.10 completed only after tests and docs are in sync.

## 6. Verification plan

Add or update the following tests.

### 6.1 `world-core/src/record.rs` unit tests

- `merge_rejects_same_id_immutable_conflict`: manually clone a record, keep its
  id, mutate an immutable field, and assert checked merge returns
  `ImmutableConflict` with no mutation.
- `merge_allows_same_immutable_body_mutable_update`: same id/body but higher
  sequence or route usage still merges as before.
- `preserve_constructor_deduplicates_exact_regions`: repeated identical
  coordinate/signature entries produce the same id/body as one entry.
- `preserve_constructor_rejects_conflicting_duplicate_region`: duplicate
  coordinate with different signatures cannot be built through the checked API.
- `route_discovery_refs_are_canonical`: repeated/out-of-order refs produce a
  sorted unique list and a stable id.
- `bundle_canonicalize_collapses_equal_duplicate_ids`: duplicate equal-body
  records converge by mutable merge law.
- `bundle_canonicalize_rejects_same_id_unequal_body`: same id and unequal
  immutable fields is an error.

### 6.2 `world-runtime/src/vault.rs` tests

- Importing a bundle with a same-id immutable conflict against an existing
  record increments `rejected`, records one issue, does not dirty the existing
  key, and leaves the exported vault unchanged.
- Re-importing a bundle with duplicate equal-body records is idempotent and
  converges to the same export regardless of duplicate order.
- Opening a store containing an invalid preserve duplicate reports an issue and
  skips or normalizes according to the compatibility policy.
- Sequence healing still uses only accepted records; rejected collision records
  with high sequence values must not exhaust or advance the local counter.

### 6.3 `tools` and file-backed atlas tests

- `check_bundle` flags duplicate ids, same-id immutable conflicts, duplicate
  preserve coordinates, non-canonical route discovery refs, empty routes, and
  empty preserves.
- `encode_bundle` emits canonical bytes independent of input record order and
  harmless duplicate order.
- The digest of a canonical bundle is stable across encode/decode and changes
  when any immutable or mutable shareable field changes.
- A file-backed export/import with duplicate equal-body records converges to
  the same vault as importing the already canonical bundle.

### 6.4 Golden and CI checks

- Keep `WORLD_ALGORITHM_VERSION == 2`.
- Keep layer `algorithm_revision` values unchanged.
- Keep `RECORD_FORMAT_VERSION == 1` and existing `record_wire_bytes_golden`
  unchanged if the digest is not embedded in record bodies.
- If constructor canonicalization intentionally changes `RouteRecord` or
  `PreserveRecord` content ids for duplicate-containing malformed inputs, add
  focused tests rather than re-blessing normal goldens.
- Run:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
```

## 7. Documentation updates

After implementation, update `docs/world-model.md` only. Do not edit historical
phase plans.

Required edits:

1. In the persistence/atlas model text, replace any wording that says same-id
   records are equal "by construction" with the new rule: ids select candidate
   records, then immutable bodies are compared before mutable merge.
2. In finding 24, mark the content-equality and public-digest portions
   resolved, but leave tombstones and per-replica usage counters described as
   remaining limits unless separately implemented.
3. In finding 25, mark duplicate canonical-set encoding resolved and describe
   the exact policies for bundle record ids, preserve coordinates, and route
   discovery refs.
4. In the prioritized roadmap, change A.10 to **Completed**, link to this plan,
   and summarize the implemented behavior and any explicitly deferred
   tombstone/counter work.

## 8. Risks and review focus

- A silent normalization path can hide malicious input. Normalize only when the
  mathematical set value is unambiguous; otherwise reject and report.
- Changing route discovery-ref ordering can change content ids for records that
  were previously created with out-of-order refs. Check whether runtime
  `RouteRecorder` already emits stable first-seen order before deciding whether
  to canonicalize existing valid routes.
- Adding a digest dependency to `world-core` affects wasm builds. Keep the
  dependency no-std/wasm-compatible or compute the digest in `tools` if the core
  crate should remain dependency-light.
- Bumping `RECORD_FORMAT_VERSION` would require migration and golden updates.
  Avoid it unless embedded digest bytes are a hard requirement.
- Do not use `HashMap` iteration order or serde output of non-canonical values
  as digest input. Digest only canonicalized content.
