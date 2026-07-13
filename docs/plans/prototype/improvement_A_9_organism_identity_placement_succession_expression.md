# Improvement A.9 — Organism identity, placement, succession, and expression

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.9](../../world-model.md#prioritized-improvement-roadmap)

**Finding addressed:** [12](../../world-model.md#12-appearance-only-changes-can-re-roll-identity-and-placement)

This plan implements roadmap item A.9 in
[`docs/world-model.md`](../../world-model.md): **Separate stable organism
identity, placement, succession, and expression**. The current runtime treats
one Ecology-layer provenance hash and one region revision as the gate for the
whole transient organism vector. That is too coarse: Morphology, Behavior, and
Aesthetics are expression inputs, but changing only those domains can retire
the vector and re-roll presence, species, and jittered placement. Same-bucket
normalization can also rebuild organisms from new raw expression floats while
the cached L8 key is unchanged.

The correction is a runtime/keying change, not a world-generator re-blessing.
Aggregate Ecology, stable entity identity, placement, explicit succession, and
expression receive distinct inputs and distinct tests. Do not modify
[`docs/plans/prototype/implementation-plan.md`](implementation-plan.md), any
`docs/plans/prototype/phase-N-plan.md`, any `docs/plans/phase-N-plan.md`, or
historical phase plans. Those files are historical records.

**Completed outcome:** L8 aggregate Ecology now declares Ecology as its only
direct possibility-domain input. `world-runtime` uses typed organism identity,
expression, and presentation keys; expression is quantized to M/B/A bucket
centers, and identity/placement exclude M/B/A and `RegionState::revision`.
Focused realization and runtime tests cover expression-only refresh,
same-bucket snaps, fail-closed roster recovery, resource-tier slot stability,
and near exit/re-entry consistency. `WORLD_ALGORITHM_VERSION` remains 2 and no
layer `algorithm_revision` was bumped.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. Morphology, Behavior, and Aesthetics changes update expressed genome values
   only. They must not change a realized organism's presence, id, species,
   trophic role, density slot, local cell, or jittered position.
2. Aggregate Ecology provenance remains separate from organism identity. A
   change that only refreshes expression must not be described or implemented
   as a new L8 aggregate output.
3. Stable entity identity and placement are keyed from exact integer or
   quantized content: world version, region coordinate, L8 layer id, density
   slot/cell feature index, field resolution, a stable habitat/roster key, and
   an explicit succession epoch. They must not use raw M/B/A floats or the
   general `RegionState::revision`.
4. The Phase 5 canonical slot contract remains intact. Slot 0 is still the
   sole gameplay/capture/resonance sample, and higher slots remain additive
   presentation instances.
5. Resource tiers remain identity-additive. Increasing `organisms_per_cell`
   keeps every slot-0 organism bit-identical and only adds higher-slot
   presentation instances.
6. True re-identification is explicit. When a material change should create a
   new organism generation, it advances a content-derived succession epoch that
   is independent of expression-only changes.
7. Same-bucket snaps and sub-bucket M/B/A drift cannot create two different
   organism identities depending on whether a region stayed near or was
   dropped and re-entered.
8. Expression reads one stable representation of M/B/A. Use bucket centers
   unless the expression key deliberately carries exact raw inputs. Do not mix
   a bucket-keyed cache with raw-float expression reads.
9. Roster completeness remains fail-closed. A missing roster for a current
   habitat key retires or blocks realization before capture and resonance can
   read a partial vector.
10. Region parking, near-window exit, resource-tier expansion, and allocation
    pooling remain transient runtime behavior only. They must not define or
    mutate stable identity.
11. No filesystem, threading, renderer, or platform API dependencies are added
    to `world-core` or `world-runtime`.
12. `WORLD_ALGORITHM_VERSION` remains 2. Do not bump a layer
    `algorithm_revision` unless the final implementation deliberately changes
    generated tile contents. The intended A.9 change should avoid tile-output
    changes.
13. Existing persistence codecs and record formats remain unchanged. The vault
    still persists deviations and snapshots, not generated organism vectors.
14. Native and wasm parity surfaces continue to compile and run; add an
    executed wasm parity sample only if a new public deterministic helper is
    exposed from `platform-web`.
15. `docs/world-model.md` must be updated after implementation so section 3.19,
    section 3.20, finding 12, and roadmap A.9 describe the new contracts, and
    roadmap item A.9 is marked completed.

## 2. Current behavior to replace

### 2.1 Aggregate L8 key doubles as expression key

`world-core/src/layer.rs` declares L8 with domains `E | M | B | A`. The
comments state that M/B/A are folded into the Ecology dependency hash only so
near-field expression is rebuilt. `world-runtime/src/generate.rs` repeats that
model in the L8 generation branch, but the aggregate fields themselves only use
Ecology, vegetation, climate, soils, biome, rosters, and the food web. M/B/A do
not change herbivore pressure, predator pressure, diversity, or dominant index.

That means the current dependency graph regenerates or reprovenances aggregate
L8 for an expression-only concern.

### 2.2 Realization uses region revision as identity epoch

`world-runtime/src/realize.rs::realize_region_into` currently computes each
organism id with:

```text
feature_hash(FeatureKey {
  world_version,
  region,
  layer: LAYER_ECOLOGY,
  feature_index: cell + slot * resolution^2,
  possibility_revision: region.revision,
})
```

The same RNG stream then gates presence, samples species, expresses the genome,
and draws placement jitter. Any change to `region.revision` can therefore
change every identity-grade decision. `RegionMap::realize_slots` passes raw
current M/B/A floats into `GenomeBias`, so expression can differ even when the
L8 bucket key is unchanged.

### 2.3 Runtime currency maps are too coarse

`RegionMap` tracks:

- `authoritative_organism_keys: BTreeMap<RegionCoord, u64>`
- `presentation_organism_keys: BTreeMap<RegionCoord, (u64, u16)>`

Both are keyed by the current fresh L8 hash, with slot count added for visual
completion. `retire_invalid_realizations` retires the whole vector when that
L8 hash changes. This conflates aggregate availability, entity identity, and
expression freshness.

### 2.4 Existing tests encode part of the bug

The ecology harness already checks that calling `realize_region` with the same
revision and different M/B/A bias keeps identity/species while changing
expression. That is useful but insufficient because it bypasses the runtime
keys that currently retire and rebuild vectors.

At least one focused test in `world-runtime/src/stream.rs`,
`same_bucket_snap_bumps_revision_and_rebuilds_only_organisms`, currently
expects same-bucket normalization to rebuild organism ids from a new revision.
A.9 must replace that expectation with the new contract: same-bucket
normalization may refresh expression, but stable identity and placement remain
unchanged unless the explicit succession epoch changes.

## 3. Data model and key split

### 3.1 Introduce explicit realization keys

Add small `Debug + Clone + Copy + PartialEq + Eq + PartialOrd + Ord` key types
in `world-runtime/src/realize.rs` or a focused neighboring module. Keep them
platform-neutral and integer-only.

Suggested shapes:

```rust
pub struct OrganismIdentityKey {
    pub ecology_hash: u64,
    pub habitat_hash: u64,
    pub succession_epoch: u64,
    pub resolution: u16,
}

pub struct OrganismExpressionKey {
    pub morphology_bucket: u16,
    pub behavior_bucket: u16,
    pub aesthetics_bucket: u16,
}

pub struct OrganismPresentationKey {
    pub identity: OrganismIdentityKey,
    pub expression: OrganismExpressionKey,
    pub slots: u16,
}
```

The exact names can change during implementation, but the split must remain
visible in types and comments. Do not store a naked `u64` where readers cannot
tell whether it represents aggregate provenance, entity identity, succession,
or expression.

### 3.2 Define the stable habitat hash

The identity key needs a content hash that changes when species/placement
should legitimately change, and stays stable when only expression changes. Fold
the following in a documented fixed order:

1. `WORLD_ALGORITHM_VERSION`;
2. `RegionCoord`;
3. `LAYER_ECOLOGY`;
4. `field_resolution`;
5. all non-expression possibility buckets that decide habitat/roster and
   density, at minimum Climate, Soils-derived upstream buckets as represented
   by current tile hashes, Biome, Vegetation, and Ecology;
6. the region's sorted habitat-signature set or the relevant L8/vegetation
   content hashes; and
7. the explicit succession epoch.

Prefer reusing existing deterministic tile `content_hash` and
`layer_hash` values where they already represent exact generated content.
Avoid ad hoc float reads. If a hash includes a set, encode cardinality and
sorted entries so duplicates or omissions cannot collapse silently.

### 3.3 Add an explicit succession epoch

Do not use `RegionState::revision` as the organism identity epoch. Add a
separate runtime field, for example:

```rust
organism_succession_epochs: BTreeMap<RegionCoord, u64>
```

or store the epoch on `RegionState` if the field is semantically part of
authoritative region state. The epoch is content-derived or content-transition
derived, not a frame counter:

- initialize to zero for ordinary generated regions;
- change when the stable habitat identity key changes in a way that should
  intentionally re-identify organisms;
- do not change for M/B/A-only bucket flips, same-bucket snaps, owner-only
  preserve churn, visual slot expansion, near exit/re-entry, or cache parking;
- make preserve successor changes deterministic by deriving the next epoch
  from the old stable key and new stable key, or from the new stable key alone
  if a pure content epoch is sufficient.

The implementation can choose the minimal viable epoch policy, but tests must
show which transitions are identity-preserving and which are succession events.

### 3.4 Quantize expression inputs

Build `GenomeBias` from `OrganismExpressionKey`, not directly from raw
`region.current` floats, unless the expression key stores exact raw bit
patterns. The recommended route is bucket centers:

```text
Morphology bucket -> bucket center -> GenomeBias.morphology
Behavior bucket   -> bucket center -> GenomeBias.behavior
Aesthetics bucket -> bucket center -> GenomeBias.aesthetics
```

This resolves the documented cache mismatch: staying in the near window and
leaving/re-entering must produce the same expression for the same expression
key.

## 4. Realization algorithm changes

### 4.1 Split RNG streams by semantic decision

`realize_region_into` should stop consuming one RNG stream for presence,
species, expression, and placement. Derive separate seeds from the stable
entity id, with fixed labels or feature-index offsets:

- presence seed;
- species seed;
- placement seed; and
- optional expression variation seed, if expression later needs per-organism
  variation beyond `GenomeBias`.

Expression-only changes must not consume or perturb the identity/placement
streams. A convenient implementation is:

```text
entity_id = feature_hash(stable identity FeatureKey)
presence_rng = Rng::new(mix(entity_id, PRESENCE_LABEL))
species_rng  = Rng::new(mix(entity_id, SPECIES_LABEL))
place_rng    = Rng::new(mix(entity_id, PLACEMENT_LABEL))
```

Use an existing stable hash/mix helper from `world-core`; do not introduce
platform-dependent hashing.

### 4.2 Preserve slot feature-index semantics

Keep the existing feature index formula:

```text
feature_index = cell_index + slot * resolution^2
```

Slot 0 must retain its canonical role. If the identity key now includes
`resolution`, document why a resolution change is a different realization
surface. If the identity key excludes `resolution`, prove through tests that
two resolutions cannot accidentally collide on feature indices and placement.

### 4.3 Keep expression a pure post-selection transform

After presence and species are decided from the stable entity key, express the
selected species genome from `OrganismExpressionKey` and clamp size to the food
web's max body size. This expression step may change `Organism::expressed`
only. It must not alter `Organism::id`, `species`, `trophic`, `slot`, `cell`,
or `world_pos`.

### 4.4 Consider compatibility wrappers

Keep the public `realize_region` helper if tests and tools use it, but route
it through the new key-building API with neutral/default succession and an
explicit expression key. Existing callers should not be forced to invent
runtime internals, but new runtime call sites should use the typed key API.

## 5. Runtime integration

### 5.1 Replace organism currency maps

Replace or augment the current currency maps with separate state:

- authoritative identity completion: `RegionCoord -> OrganismIdentityKey`;
- authoritative expression completion: `RegionCoord -> OrganismExpressionKey`;
- presentation completion: `RegionCoord -> OrganismPresentationKey`.

An empty barren vector still counts as completed when all three relevant keys
match.

### 5.2 Compute keys in one place

Add helpers on `RegionMap` such as:

```rust
fn organism_identity_key(&self, coord: RegionCoord) -> Option<OrganismIdentityKey>;
fn organism_expression_key(&self, coord: RegionCoord) -> Option<OrganismExpressionKey>;
fn organism_presentation_key(&self, coord: RegionCoord, slots: u16)
    -> Option<OrganismPresentationKey>;
```

These helpers should enforce all preconditions currently spread through
`fresh_ecology_hash`, `realization_rosters_complete`, and raw bias creation.
They should return `None` for dirty/pending/missing aggregate inputs, missing
rosters, or unavailable tile content.

### 5.3 Retire only for the reason that changed

Revise `retire_invalid_realizations`:

- near exit still drops the transient vector and all completion keys;
- missing stale aggregate provenance or missing rosters still fail closed;
- stable identity-key changes retire and rebuild the vector;
- slot-count changes rebuild the vector for presentation expansion;
- expression-key changes may recompute and publish the vector with the same
  stable identity and placement, or update expression in place if implemented
  carefully;
- M/B/A-only changes must not clear authority before capture/resonance in a
  way that creates a frame with no canonical organism solely because expression
  changed.

If expression is recomputed by rebuilding the vector, tests must assert that
stable fields compare equal before and after the rebuild.

### 5.4 Update preserve and revision paths

`apply_effective_preserve_signature` currently retires organisms for any
material exact-vector change. Change this to consult the new key split:

- same-bucket snap: no identity retirement; refresh expression if M/B/A bucket
  or exact expression key changes;
- M/B/A-only bucket flip: no identity retirement; expression refresh only;
- stable habitat bucket flip: mark dependent generated layers dirty and retire
  identity only after the new stable key is available, or fail closed until it
  is available;
- owner-only preserve changes remain inert;
- final preserve deletion remains no-snap and does not change organisms.

Keep in-flight cancellation tied to declared generated-layer dirtiness. Do not
cancel generation just because an expression key changed.

### 5.5 Decide whether to remove M/B/A from L8 declared domains

The cleaner end state is for `LAYER_ECOLOGY` declared domains to drop `M | B |
A`, because aggregate L8 fields do not read them. However, this changes
dependency hashes and invalidation behavior, and may require updating focused
tests that inspect layer diagnostics.

Implementation should make an explicit decision:

- **Preferred:** remove M/B/A from `LayerDecl { id: LAYER_ECOLOGY }.domains`,
  update comments in `layer.rs` and `generate.rs`, and keep
  `algorithm_revision` unchanged because aggregate tile output is unchanged.
  Add tests proving M/B/A changes do not dirty or recompute aggregate L8.
- **Fallback:** keep the declared domains temporarily but ensure organism
  identity ignores the L8 hash component affected only by M/B/A. This is less
  clear and must be documented as temporary debt in `world-model.md`.

Do not silently keep the old declaration and claim the aggregate/identity split
is complete.

## 6. Documentation updates required during implementation

Only this plan file is created by the planning task. The implementation task
must also update current documentation.

### 6.1 Update `docs/world-model.md`

Required edits:

1. In section 3.19, replace the statement that M/B/A are folded into the L8
   dependency hash to force organism expression rebuilds. Document the new
   aggregate Ecology key and expression key.
2. In section 3.20, replace the single RNG stream and `region.revision`
   identity description with the new identity, placement, succession, and
   expression key model.
3. In finding 12, mark it resolved and summarize the final implementation.
4. In the prioritized roadmap, change item A.9 to `Completed`, link to this
   plan, and describe the landed behavior.
5. Check nearby text that mentions "old-revision organisms", "organism epoch",
   or "same-bucket snap" and update it so it no longer contradicts A.9.

### 6.2 ADR requirement

If the implementation removes M/B/A from the L8 declared domains or defines a
new persistent/architectural succession contract, add a new ADR and index
entry under `docs/adr/`. If the change is entirely a runtime key split under
existing ADRs, an ADR is optional, but `world-model.md` must still be precise.

## 7. Test plan

### 7.1 Focused runtime tests

Add or update tests in `world-runtime/src/stream.rs`:

1. M/B/A-only bucket flips keep canonical organism id, species, trophic role,
   slot, cell, and world position stable while changing at least one expressed
   field.
2. Same-bucket normalization no longer re-rolls organism ids. Replace
   `same_bucket_snap_bumps_revision_and_rebuilds_only_organisms` with an
   assertion that stable organism fields survive and expression is rebuilt only
   when the expression key changes.
3. Stable habitat changes, such as Climate/Ecology/Vegetation-affecting bucket
   changes, intentionally advance the succession key or otherwise re-identify
   organisms according to the chosen epoch policy.
4. Near exit and re-entry with unchanged keys reproduces the same vector,
   including expression, not merely the same ids.
5. Missing roster behavior still fails closed and does not publish stale or
   partial organisms under a current key.
6. Resource-tier expansion still preserves slot-0 organisms exactly.

### 7.2 Realization unit tests

Add tests in `world-runtime/src/realize.rs`:

1. Different expression keys over the same identity key leave all stable fields
   unchanged and modify expected expression channels.
2. Different succession epochs can re-roll identity and placement.
3. The presence/species/placement RNG streams are independent of expression
   key changes.
4. Bucket-center expression is stable across raw sub-bucket drift.

### 7.3 Harness updates

Update `crates/tools/src/ecology.rs` so the expression response scenario tests
the runtime path, not only direct calls to `realize_region` with a fixed
revision. Keep the direct pure helper test if it remains useful, but ensure the
machine check would fail if `RegionMap` still retires and re-identifies
organisms on M/B/A-only changes.

Run and, if necessary, update:

- `cargo test -p world-runtime`;
- `cargo run --bin wer-ledger`;
- `cargo run --bin wer-anchor`;
- `cargo run --bin wer-vault`;
- `cargo run --release --bin wer-scale -- --report`;
- `cargo test --workspace`;
- `cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown`;
- `wasm-pack test --node crates/platform-web`;
- `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`;
- `cargo fmt --all -- --check`.

Use the CI-equivalent set before marking the roadmap item completed.

## 8. Implementation sequence

1. Add typed realization key structs and pure helpers for expression-key
   quantization, stable identity-key construction, and optional succession
   epoch mixing.
2. Refactor `realize_region_into` so stable identity, presence, species, and
   placement use identity/succession inputs, while expression uses only the
   expression key.
3. Update `RegionMap` completion keys and key-building helpers. Keep old map
   names only if they continue to reflect their exact semantics.
4. Revise retirement, authoritative publication, and visual expansion so
   expression refreshes do not create identity gaps and visual expansion keeps
   slot 0 identical.
5. Update preserve/revision paths and the same-bucket snap behavior.
6. Decide and implement the L8 declared-domain cleanup. Update comments in
   `world-core/src/layer.rs` and `world-runtime/src/generate.rs`.
7. Add focused realization/runtime tests, then update ecology, vault, anchor,
   and scale harness expectations as needed.
8. Update `docs/world-model.md` and any required ADR after tests define the
   final semantics.
9. Run the full CI-equivalent command set and only then mark A.9 completed in
   the roadmap.

## 9. Risks and review checks

The main risk is preserving the old behavior under renamed keys. Review should
look for any path where `region.revision`, raw M/B/A floats, or the full L8
hash can still influence presence, species choice, or placement.

The second risk is a one-frame authority gap. Expression-only changes should
not clear canonical slot-0 organisms before capture and resonance unless the
replacement is published atomically in the same update pass.

The third risk is accidental tile-output churn. Removing M/B/A from the L8
declared domains changes dependency hashes, but aggregate tile values should
not change. Do not re-bless determinism fixtures unless a deliberately changed
surface is named and justified.

Finally, keep the documentation synchronized. Finding 12 and roadmap A.9 must
not be marked completed until `world-model.md`, tests, and the implemented key
split all state the same model.
