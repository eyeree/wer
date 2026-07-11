# Phase 3 — Procedural Genetics and Ecology: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 3 work begins
(it covers the ground of `procedural-genetics-plan.md`, `ecology-field-plan.md`,
and the near-field slice of `entity-realization-plan.md`). It expands the
Phase 3 scope in section 20 into concrete interfaces, data layouts, algorithms,
and milestones, grounded in the landed Phase 2 dependency graph
([`phase-2-plan.md`](phase-2-plan.md), ADRs 0007–0009).

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it assumes
them and calls out where Phase 3 stresses each.

---

## 1. Goals and non-goals

### 1.1 The question Phase 3 must answer

Phase 2 proved that a six-layer environmental pipeline can be invalidated with
surgical precision: a change recomputes exactly the layers that declare a
dependency on it, the stable trio never moves under drift, and the world
generates reproducibly inside its budgets. But the top of that pipeline is
still two scalar fields (vegetation density, canopy height). The Ecology,
Morphology, Behavior, and Aesthetics possibility domains reach **nothing** —
`layer::tests::domain_dirty_masks_match_the_declared_graph` asserts it. Phase 3
asks:

> Can the world grow organism-level richness — procedural genomes, species
> archetypes, food webs, near-field organisms — that is **diverse but
> internally coherent**, and that **responds meaningfully** to possibility
> changes, without simulating every organism and without breaking determinism,
> continuity, or the invalidation precision Phase 2 won? (section 20, Phase 3)

This is the direct attack on the **scope risk** (section 23.2): ecology is where
a custom engine most easily becomes a science project. Everything below serves a
machine-checkable answer built on aggregate fields first, entities second
(section 10).

### 1.2 Success criterion (from section 20)

- **Diversity:** distinct environments carry distinct species rosters and food
  webs; a settled window contains many species, and neighbouring habitats
  differ. Machine-checked by the diversity half of the ecology harness (§12.3).
- **Coherence:** every generated ecosystem obeys the ecological plausibility
  rules (herbivore biomass bounded by primary productivity, predator biomass
  bounded by herbivore biomass, body size bounded by productivity, no orphan
  trophic tiers). Machine-checked as invariants over the whole window (§12.3).
- **Response:** steering an Ecology / Morphology / Behavior / Aesthetics anchor
  measurably shifts the ecosystem (rosters, pressures, colours) in the far
  field, while a Geology or (unrelated) Climate anchor provably does not touch
  organisms — the Phase 2 precision property, extended to the new layers.
- **Continuity & determinism preserved:** the Phase 2 continuity replay still
  passes; near-field organisms do not pop or contradict the aggregate fields
  they are sampled from; every organism and species identity is reproducible.

### 1.3 Goals

- **Procedural genomes** (section 11): a stable, integer-seeded genome with
  three independent sub-genomes — appearance, behavior, ecological niche — whose
  expressed traits bias under the Morphology / Behavior / Aesthetics domains.
- **Species archetypes**: a deterministic *roster* of species for a habitat,
  each a genome plus a niche assignment, memoized by an environmental signature
  so identical habitats across the world share species (data-oriented, bounded).
- **Food-web graphs**: trophic tiers and predator–prey edges over a roster,
  projected through rule-based plausibility constraints (section 8) — the first
  real consumer of the "iterative relaxation, not ML" stance.
- **Aggregate population fields** as a new graph layer (section 10): herbivore
  pressure, predator pressure, species diversity, dominant species — cached,
  dependency-hashed, budgeted tiles that finally wire the E/M/B/A domains into
  the declared graph.
- **Near-field organism realization**: individual organism instances near the
  player, sampled from the aggregate fields so counts and coverage preserve the
  aggregate (section 10), each with a stable [`FeatureKey`]-derived identity —
  the first real use of the identity machinery built in Phase 0.
- **Lifecycle and succession replacement** via the two cheapest section 11
  strategies (distance-based regeneration, offscreen replacement): as a distant
  region converges to a new aggregate, re-realization reflects the new state.
- **Debug visibility**: species/ecology map channels, a per-cell species and
  food-web readout, and a coherence/diversity harness that is the phase's
  machine-checkable sign-off.

### 1.4 Non-goals (explicitly deferred)

- **Cross-platform species identity.** Like biome ids (phase-2-plan.md §7.6),
  Phase 3 species identities are **presentation-grade**: deterministic and
  reproducible per run and per platform, but derived from `f32` environmental
  tiles, so knife-edge habitats may roster differently across platforms. The
  cross-platform habitat *class* the community atlas needs is a Phase 5 concern
  (ADR 0010); the portable surface Phase 3 does guarantee is `genome(seed)`
  itself (§9.3).
- **Persistence of species, discoveries, or named organisms** (Phase 5). The
  `Storage` trait stays unused; rosters and organism instances are run-local,
  reconstructed deterministically.
- **Anchors that capture organism traits** (Phase 4). Phase 3 wires the existing
  E/M/B/A domains into generation; `steer` / `project_plausible` and the anchor
  kinds are untouched. Trait-capture anchors and the resonance graph
  (Overview, "Player Avatar") are Phase 4.
- **Full local simulation**: no per-organism behavior loops, no movement, no
  hunger/reproduction ticks. Organisms are *realized* (placed, expressed,
  identified), not *simulated*. Section 11: only nearby interactive organisms
  ever need full entity state, and even that is Phase 4+.
- **Continuous genome morphing.** Succession is distance-based regeneration and
  offscreen replacement (section 11), not per-frame trait interpolation of
  visible organisms — that is a Phase 6 fidelity concern.
- **Rendering real organism geometry.** The renderer still presents one composed
  debug texture; near-field entities surface as debug markers and aggregate
  tints, not meshes (section 10's near-field geometry is a later renderer phase).
- **GPU ecology distribution, SIMD, LOD organism tiers** (Phase 6).
- **Expanding the possibility vector.** The 8-domain, one-scalar-per-domain
  `PossibilityVector` is unchanged; Phase 3 finally *reads* its last four
  domains, it does not grow them.

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 3 coverage |
|---|---|
| `procedural-genetics-plan.md` | **Core of Phase 3** — genome, species, food web (§4, §7). |
| `ecology-field-plan.md` | Aggregate population fields as graph layer L8 (§6, §7.5). |
| `entity-realization-plan.md` | First slice: near-field organism realization from aggregate fields (§8.3). |
| `world-layer-dependency-plan.md` | Appends L8 to the frozen graph; no id reassignment (§4.1). |
| `determinism-and-versioning-plan.md` | New identity surfaces; **no** world-version bump (§9.1). |
| `job-system-plan.md` | One new cached layer + one un-cached realization pass on the existing scheduler (§8). |

The environmental layers (terrain … vegetation), the possibility field, the
streaming window, anchors, and both platform shells change only where the new
layer and near-field realization force them to.

---

## 3. Architecture overview

Phase 3 adds two tiers on top of the Phase 2 environmental stack, and they map
exactly onto section 10's "aggregate fields before entities":

```text
  Phase 2 environmental stack (unchanged)
  ────────────────────────────────────────────────────────────────
   … Climate ─┬─ Hydrology ─┬─ Soils ─┬─ Biome ─┬─ Vegetation (L7)
              │             │         │         │        │
              └─────────────┴─────────┴─────────┴────────┤
                                                         ▼
  Tier A — aggregate ecology, IN the cached dependency graph
  ────────────────────────────────────────────────────────────────
   L8 Ecology   reads E, M, B, A domains directly
     ├─ per cell: HabitatSignature ── memoized ──▶ SpeciesRoster + FoodWeb
     └─ produces aggregate tiles: herbivore, predator, diversity,
        dominant-species (u16)                         (RosterCache, §6.3)
                                                         │
  Tier B — near-field realization, OUTSIDE the graph, transient
  ────────────────────────────────────────────────────────────────
   Realization (near window only): sample L8 + Vegetation aggregates,
     instantiate individual organisms preserving aggregate counts,
     each with a FeatureKey-derived stable identity           (§8.3)
```

Four commitments organize everything, each a continuation of a Phase 2
commitment:

1. **Aggregate before individual.** All of ecology that participates in caching,
   invalidation, and budgets is expressed as *aggregate fields* in the layer
   graph (Tier A). Individual organisms (Tier B) are a pure, un-cached function
   of those aggregates plus an integer identity — never a source of cached
   state. This is section 10 made literal, and it keeps the dependency-explosion
   win of Phase 2: a possibility nudge dirties an aggregate tile, not a
   population of entities.

2. **Identity is integer; expression is float.** A species genome and an
   organism instance are identified by integer hashing ([`FeatureKey`],
   [`species_seed`]); their expressed traits are portable `f32` derived from
   that seed via [`Rng`]. This is the Phase 2 identity ledger (§9.3) extended:
   `genome(seed)` is cross-platform golden-fixtured; the *derivation of the seed
   from a habitat* is presentation-grade, exactly as biome classification is
   (ADR 0010).

3. **Rosters are functions of a habitat signature, memoized.** Distinct habitats
   are far fewer than regions, so the roster and food web for a signature are
   computed once and shared through a `RosterCache` keyed by signature — the
   Tier-A analogue of the macro drainage cache (§6.3). A cell resolves its
   species through `(signature, slot)`, so the cached aggregate tiles stay
   compact.

4. **The graph absorbs L8 with zero id churn.** `dirty_layers` is a `u32` with
   8 of 32 bits used — Phase 2 left the room deliberately (`region.rs`). L8 is
   *appended* (id 8, still topological), existing ids are frozen, and because no
   existing generator's output changes, **no golden fixture re-blesses and
   `WORLD_ALGORITHM_VERSION` does not bump** (§9.1).

---

## 4. Procedural genetics (world-core)

### 4.1 The habitat signature (identity-grade-enough environment key)

New module `world-core/src/habitat.rs`. A `HabitatSignature` is the compact
environmental class a roster hashes from:

```rust
/// The environmental class a species roster is a function of. Presentation-
/// grade in Phase 3: derived from f32 environment tiles, so knife-edge cells
/// may differ across platforms — the same residual biome classification already
/// has (phase-2-plan.md §7.6, ADR 0010). Coarse on purpose: nearby cells share
/// a signature so rosters are shared and the RosterCache stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HabitatSignature {
    pub biome: u8,           // Biome::id()
    pub temperature_band: u8, // quantized climate temperature
    pub moisture_band: u8,    // quantized climate moisture
    pub fertility_band: u8,   // quantized soil fertility
}

impl HabitatSignature {
    /// Classify a cell's environment into a signature (coarse quantization).
    pub fn of(biome: Biome, c: &Climate, s: &Soils) -> Self;
    /// The 64-bit seed a roster for this habitat derives from (portable given a
    /// signature — the signature itself is presentation-grade).
    pub fn seed(&self) -> u64;
}
```

The coarse banding (a handful of bands per axis) is what makes rosters *shared*
and the cache *bounded*: an entire biome at similar temperature draws from one
roster, so the world reads as ecologically zoned rather than per-cell noise.

### 4.2 Genomes

New module `world-core/src/genome.rs`. Three independent sub-genomes
(section 11), each a packed struct of quantized integer traits derived from a
species seed, with expressed `f32` biased by possibility domains at read time:

```rust
/// A stable procedural genome: three independent domains (section 11). The raw
/// trait words are integer and portable; expression into f32 happens on read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Genome {
    pub appearance: AppearanceGenes, // colour, luminance, size class, form
    pub behavior: BehaviorGenes,     // activity, aggression, sociality
    pub niche: NicheGenes,           // trophic tier, diet breadth, tolerances
}

impl Genome {
    /// Derive a genome purely from a species seed (cross-platform, §9.3).
    pub fn from_seed(seed: u64) -> Self;
}

/// Possibility bias applied when expressing appearance (Aesthetics domain),
/// behavior (Behavior domain), and morphology (Morphology domain). These are
/// the dequantized buckets L8 reads; a neutral vector reproduces the unbiased
/// genome, so an organism far from any anchor expresses its base genes.
pub struct GenomeBias { pub morphology: f32, pub behavior: f32, pub aesthetics: f32 }
```

Expression is where "responds meaningfully" becomes concrete: colour shifts with
Aesthetics, body-plan tendency with Morphology, activity/aggression with
Behavior. Bias is a bounded modulation of base genes, never a re-identification
— the genome id is fixed; only its expressed `f32` moves (mirroring how Phase 2
possibility drift changes tile *content* but never feature identity).

### 4.3 Species archetypes and rosters

New module `world-core/src/species.rs`:

```rust
/// A species: a stable identity plus its genome and assigned niche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Species {
    pub id: u64,          // species_seed(signature, index): the stable identity
    pub genome: Genome,
    pub trophic: Trophic, // Producer, Herbivore, Carnivore, Omnivore, Decomposer
}

/// The deterministic species roster for a habitat: a small, ordered set.
#[derive(Debug, Clone)]
pub struct SpeciesRoster {
    pub signature: HabitatSignature,
    pub species: Vec<Species>, // bounded (ROSTER_MAX), trophic-sorted
}

/// The stable per-species identity for the `index`th species of a habitat.
pub fn species_seed(signature: HabitatSignature, index: u32) -> u64;

/// Derive the full roster for a habitat (pure; memoized by the runtime, §6.3).
pub fn species_roster(signature: HabitatSignature) -> SpeciesRoster;
```

Roster size and trophic composition follow from the habitat: barren biomes
(Ocean surface, Ice, Bare) yield tiny producer-only rosters; rainforest yields
the largest and most trophically complete. The count is derived, integer, and
capped at `ROSTER_MAX` so the aggregate tiles' dominant-species index fits a
small type and the cache stays bounded.

### 4.4 Food webs

New module `world-core/src/foodweb.rs`:

```rust
/// A trophic graph over a roster: edges point predator → prey, with the
/// plausibility constraints of section 8 already enforced.
#[derive(Debug, Clone)]
pub struct FoodWeb {
    pub edges: Vec<(u32, u32)>, // (predator index, prey index) within the roster
    /// Sustainable biomass share per trophic tier, summing to ~1 — the aggregate
    /// L8 samples to fill its pressure channels.
    pub tier_biomass: [f32; TROPHIC_TIERS],
}

/// Build (and constrain) the food web for a roster and its primary
/// productivity (aggregate vegetation density is the producer base).
pub fn food_web(roster: &SpeciesRoster, primary_productivity: f32) -> FoodWeb;
```

`food_web` is the first embodiment of section 8's "rule-based constraints and
iterative relaxation, not ML": herbivore biomass is capped at a fraction of
primary productivity, carnivore biomass at a fraction of herbivore biomass,
maximum body size at a function of productivity; species that cannot be
sustained are pruned to `Decomposer` or dropped, and edges are only drawn where
predator size/diet admits the prey. The output is *coherent by construction*,
which is what §12.3's invariants assert.

---

## 5. Public interfaces

### 5.1 `world-core` additions

```text
world-core/src/
    habitat.rs   # NEW: HabitatSignature + seed (§4.1, ADR 0010)
    genome.rs    # NEW: Genome, sub-genomes, GenomeBias, expression (§4.2)
    species.rs   # NEW: Species, SpeciesRoster, species_seed, species_roster (§4.3)
    foodweb.rs   # NEW: FoodWeb, food_web (+ plausibility constraints) (§4.4)
    population.rs# NEW: aggregate population sampling from roster+web (§7.5)
    layer.rs     # + LAYER_ECOLOGY = 8; LAYER_COUNT 8 → 9; one table row (§4.1)
    hash.rs      # unchanged; FeatureKey/Rng finally consumed by realization
```

All new modules are pure and wasm-clean, in the existing
`#[inline] #[must_use] const fn` style where applicable. The layer table gains
one row:

| Layer | deps | direct domains | notes |
|---|---|---|---|
| 8 Ecology | Climate, Soils, Biome, Vegetation | E, M, B, A | Aggregate populations; finally reads the last four domains. |

`domain_dirty_mask`, `dependents_closure`, and `domain_readers` need no code
change — they iterate `LAYERS`, so appending the row wires the new domains
automatically. The Phase 2 test that asserted Morphology/Behavior/Aesthetics
reach nothing is *updated* (not deleted) to assert they now reach exactly `{L8}`,
and that Ecology now reaches `{Vegetation, L8}` (it already drove Vegetation's
density; L8 is the new reader) — §12.4.

### 5.2 `world-runtime` changes

```text
world-runtime/src/
    generate.rs   # + LAYER_ECOLOGY arm; population channels + dominant-species u16 tile
    rostercache.rs# NEW: RosterCache — memoized (roster, food web) by signature (§6.3)
    realize.rs    # NEW: near-field organism realization from aggregates (§8.3)
    stream.rs     # RosterCache alongside the caches; near-window realize pass; L8 dispatch
    region.rs     # unchanged (dirty_layers already u32; LAYER_COUNT widening only adds a bit)
    budget.rs     # + max_realize_organisms (near-field realization cap, §8.4)
```

Key signature additions:

```rust
// generate.rs — L8 consumes its input tiles plus the memoized roster/web for
// each distinct signature it encounters (resolved by the scheduler at dispatch,
// passed in like the drainage macro input).
pub struct LayerInputs {
    // … existing fields …
    /// Rosters for the signatures this tile will encounter, where L8 is
    /// declared. Keyed by signature; the job looks each cell's signature up.
    pub rosters: Option<Arc<RosterSnapshot>>,
}

// realize.rs — transient, not cached.
pub struct Organism {
    pub id: u64,              // feature_hash(FeatureKey{ layer: LAYER_ECOLOGY, … })
    pub species: u64,         // Species::id
    pub local_pos: LocalPos,  // where in the region
    pub expressed: Expressed, // colour/size/etc. after GenomeBias
}
pub fn realize_region(
    coord: RegionCoord, tiles: &RegionTiles, rosters: &RosterCache,
    budget_remaining: usize,
) -> Vec<Organism>;
```

`RegionState`, `GenerationStatus`, job ids, supersession, and the results
channel are unchanged. Realization is driven off the *near* window each frame
and never enters the results channel — it is a pure read of settled caches.

### 5.3 `renderer`, `platform-native`, `tools`, `platform-web`

- **Renderer:** unchanged — still one composed texture.
- **`platform-native` (`viz.rs`, `panel.rs`):** new map channels — `Herbivore`,
  `Predator`, `Diversity`, and `DominantSpecies` (a categorical palette keyed by
  species id hash); the composite view tints by dominant-species colour so
  ecosystem zonation is visible at a glance. Near-field organisms draw as debug
  point markers coloured by expressed appearance. The info panel shows the
  player cell's habitat signature, roster size, dominant species, and trophic
  breakdown.
- **`tools`:** `wer-inspect` grows `--species X Y` (dump the cell's signature,
  full roster with genomes, and food-web edges) and `--ecology X Y` (the L8
  aggregate values plus the dependency-hash chain, extending `--layers`). A new
  **ecology harness** binary drives the coherence/diversity scenarios (§12.3),
  the Phase 3 analogue of the invalidation ledger.
- **`platform-web`:** exports two parity samples — `genome_sample()` (the genome
  of a fixed species seed) and `food_web_sample()` (tier biomass for a fixed
  roster) — mirroring the native goldens. These are the *portable* surface;
  signature derivation is deliberately not a parity export (§9.3).

---

## 6. Data layout

### 6.1 Channels

`generate.rs` channel constants grow by three `f32` channels; the dominant
species is a `u16` tile (species can exceed 255 across a window), beside the
biome `u8` tile:

| Channel | Type | Producer |
|---|---|---|
| *(CHANNEL_ELEVATION … CHANNEL_CANOPY — unchanged)* | f32 | Terrain … Vegetation |
| `CHANNEL_HERBIVORE` | f32 | Ecology |
| `CHANNEL_PREDATOR` | f32 | Ecology |
| `CHANNEL_DIVERSITY` | f32 | Ecology |
| dominant-species tile (separate field) | u16 | Ecology |

`CHANNEL_COUNT` goes 10 → 13. `RegionTiles` gains
`dominant: Option<Arc<FieldTile<u16>>>` beside `biome`, and `FieldTile<u16>`
gets the same `content_hash`/`dep_hash` treatment the `u8` biome tile already
has (§12.4). The dominant-species value is an *index into the cell's roster*
(resolved via the signature), not a global id — keeping the tile compact while
`--species` reconstructs the full identity on demand.

### 6.2 Memory

At `FIELD_RES = 32`: three new f32 channels ≈ 12 KB + a `u16` tile ≈ 2 KB per
region, ~14 KB on top of Phase 2's ~41 KB — a 1,000-region window grows from
~41 MB to ~55 MB, still comfortably inside the section 15 low-hundreds-of-MB
target. Near-field organisms exist only for the pinned near window (a few dozen
regions) and are transient; at a capped few-hundred organisms per near region
they are a bounded, evictable few MB, not a growing store (§8.4). Eviction
semantics are unchanged (state + tiles drop together; rosters evict by §6.3).

### 6.3 The roster cache

`RosterCache` maps `HabitatSignature → Arc<(SpeciesRoster, FoodWeb)>` and lives
beside `RegionCache` / `MacroCache` in `RegionMap`. It is the Tier-A analogue of
the macro drainage cache: computed once per distinct signature, shared across
every cell and region that resolves to that signature. Because coarse banding
makes signatures repeat heavily, the cache is small and naturally bounded
(≤ `Biome × band³` entries). Eviction sweeps signatures no resident region's
tiles reference any more, reusing the macro cache's dependent-sweep shape.

The scheduler resolves, at L8 dispatch, the set of signatures a region's cells
will produce (from the settled biome/climate/soil tiles), ensures each is in the
cache, and snapshots them into `LayerInputs.rosters` — exactly as it snapshots
the drainage macro tile. Roster computation itself can run as its own budgeted
job when a signature is missing (§8.2).

---

## 7. Algorithms

### 7.1 Signature classification (§4.1)

Per cell: read the settled biome id and the climate/soil tiles, quantize
temperature/moisture/fertility to coarse bands, and pack into a
`HabitatSignature`. Coarse enough that a contiguous habitat yields one signature
over many cells (shared rosters, visible zonation); fine enough that a
temperature or moisture gradient crosses into a new roster where the biome map
already shows a transition.

### 7.2 Genome derivation and expression (§4.2)

`Genome::from_seed` folds the species seed through `mix` into three independent
trait words (one per sub-genome), then unpacks quantized traits — all integer,
all portable, golden-fixtured. Expression multiplies base traits by a bounded
function of the relevant dequantized domain bucket: appearance colour/luminance
by Aesthetics, form/size by Morphology, activity/aggression by Behavior. A
neutral possibility vector reproduces the base genome exactly (the unbiased
identity), so expression is a modulation, never a re-identification.

### 7.3 Roster construction (§4.3)

`species_roster(signature)` derives a count from the habitat (barren → few,
rainforest → many, capped at `ROSTER_MAX`), then for each index computes
`species_seed(signature, index)`, a genome, and a trophic assignment biased by
the habitat (wet/warm/fertile habitats admit more consumer tiers). The roster is
trophic-sorted so the dominant-species index and the food web have a stable
order. Pure and memoized (§6.3).

### 7.4 Food-web construction and plausibility (§4.4)

`food_web` starts from primary productivity (aggregate vegetation density),
allocates a sustainable biomass budget down the trophic tiers by fixed fractions
(herbivore ≤ α·producer, carnivore ≤ β·herbivore), draws predator→prey edges
only where genome size/diet admits the prey, and prunes or demotes species that
end with no sustainable biomass. This is a single deterministic relaxation pass
(no iteration to convergence needed at Phase 3 fidelity), and its post-conditions
are precisely §12.3's coherence invariants — so the harness checks the algorithm
it, not a re-derivation.

### 7.5 Aggregate population fields (L8, §6.1)

Per cell: classify the signature, resolve `(roster, web)` from the snapshot,
read the dequantized E/M/B/A buckets, and emit:

- `dominant` = index of the highest-biomass species for the cell (u16 tile),
- `herbivore` / `predator` = the web's herbivore / carnivore tier biomass scaled
  by local primary productivity and the Ecology bucket,
- `diversity` = species entropy of the roster weighted by tier biomass.

Pure per-cell arithmetic over four input tiles plus the memoized roster — the
Ecology/Morphology/Behavior/Aesthetics buckets are its only direct domain reads.
Morphology, Behavior, and Aesthetics reach *only* L8 (they invalidate nothing
upstream); Ecology also drives Vegetation's density as it did in Phase 2, so an
Ecology flip cascades Vegetation → L8 (the §12.3 response property). This is section 10's aggregate ecology, and section 9 steps 7–8
(food-web structure, species distributions) collapsed into cached fields.

### 7.6 Near-field realization (§8.3)

For each pinned near region, for each cell in the near window: read the
aggregate density/dominant/pressure values and instantiate a count of organisms
that *preserves the aggregate* (section 10: 70% canopy → ~70% near-field
coverage). Placement and per-instance jitter are seeded from
`feature_hash(FeatureKey{ region, layer: LAYER_ECOLOGY, feature_index: slot,
possibility_revision })` — the identity machine built in Phase 0, first used
here. Each organism resolves its species from the cell's `(signature, dominant
or sampled slot)`, expresses its genome under the region's possibility bias, and
carries a stable id. Distance-based regeneration and offscreen replacement
(section 11) fall out for free: organisms are recomputed when the region enters
the near window or its source tiles change, and are simply dropped when it
leaves — no morphing, no stored entity state.

---

## 8. Scheduling and budgets

### 8.1 L8 rides the existing topological dispatch

L8 is an ordinary graph layer: `dispatch_regen`'s fixed-point loop already
submits any stale layer whose inputs are fresh in id order, so appending id 8
means it is dispatched after Vegetation with no scheduler change. Its dep-hash
folds the E/M/B/A buckets and its four input tiles' hashes exactly as every
Phase 2 layer does; supersession, in-flight bookkeeping, and integration are
unchanged.

### 8.2 Roster jobs

A missing `(roster, food web)` for a signature is produced by a small budgeted
job (cheap — pure computation over a bounded roster), dispatched when L8 finds a
cell whose signature is absent from the cache, riding the same queue at the
priority of the requesting region. L8 for that region defers (its inputs
aren't fresh) until the roster lands — identical to the macro-drainage-then-
hydrology ordering. Rosters are shared, so a signature is computed once and
serves the whole window.

### 8.3 Near-field realization pass

After dispatch settles, `RegionMap::update` runs a **realization pass** over the
pinned near window only: for each near region whose L8 and vegetation tiles are
fresh, (re)build its organism list. This is a pure read of settled caches — it
never mutates tiles, never enters the results channel, and is safe to run on the
main thread (it touches only the near window, a few dozen regions). It is
recomputed when a near region's source tiles change (distance-based
regeneration) and discarded on eviction.

### 8.4 Budgets

`Budget` gains `max_realize_organisms` (a per-frame cap on organisms
instantiated, so entering a dense biome amortizes realization over a few frames
rather than hitching). L8 generation is budgeted by its declared `LayerDecl.cost`
like every other layer (calibrated by the §13 benches — roster-backed L8 is
mid-cost, comparable to hydrology). `FrameStats` grows
`organisms_realized`, `rosters_built`, and `roster_cache_bytes` — the raw
material for the harness and dashboards. `regenerated_by_layer` widens to
`LAYER_COUNT` (9) automatically.

---

## 9. Determinism and versioning

### 9.1 No world-version bump

Phase 3 **appends** a layer and adds new generators; it changes **no existing
layer's output for identical inputs**. Terrain … Vegetation tiles are
bit-identical to Phase 2, so every existing golden fixture stays blessed and
`WORLD_ALGORITHM_VERSION` **stays at 2**. This is the sanctioned path
phase-2-plan.md §9.1 named: "subsequent milestones add layers with *new*
fixtures; they must not re-bless existing ones." The only mechanical widenings —
`LAYER_COUNT 8 → 9`, `CHANNEL_COUNT 10 → 13`, `all_layers_mask()` gaining a bit
— add *new* work and *new* fixtures without altering old identities. (A casual
re-bless of a Phase 2 fixture during Phase 3 is a determinism bug, per AGENTS.md.)

### 9.2 Layer algorithm revisions

Tuning L8's constants, or the genome/roster/food-web math after it lands, bumps
`LAYER_ECOLOGY`'s `algorithm_revision` (or the run-local
`RegionMap::bump_layer_revision` in tests), invalidating L8 and re-blessing L8's
fixtures only. Genome/species/food-web are pure helpers L8 consumes, so a change
to them is an L8 algorithm change — same revision discipline, and the dep-hash
chain propagates it to near-field realization automatically (realization reads
the L8 tile).

### 9.3 The identity ledger (extended)

- **Cross-platform, golden-fixtured and wasm-parity-tested:** `genome(seed)` for
  fixed seeds, `species_seed(signature, index)`, `food_web` tier biomass for a
  fixed roster, the L8 dep-hash fold. These are pure integer→integer or
  integer→portable-`f32` functions.
- **Presentation-grade, per-platform, replay-hash-checked only:** the
  `HabitatSignature` a cell derives (it reads `f32` biome/climate/soil tiles, so
  it inherits biome's knife-edge residual), and therefore which roster a cell
  gets and which organisms realize. Deterministic and reproducible within a run
  and platform; not asserted cross-platform. The community atlas's
  cross-platform species identity is a Phase 5 problem, solved then by quantizing
  the classification inputs into a portable habitat class (ADR 0010 records the
  decision and the upgrade path).

### 9.4 New ADR

- **ADR 0010 — Species identity is presentation-grade until the atlas needs
  otherwise.** Records that Phase 3 rosters derive from `f32` environment tiles
  (like biome, ADR/§7.6 of Phase 2), that `genome(seed)` is the portable
  surface, and that cross-platform habitat classification is deferred to Phase 5
  with a named upgrade path (quantize climate/soil into portable bands before
  hashing the signature).

---

## 10. Threading model

Unchanged in kind from Phase 2 (§10 there): neutral crates express parallelism
only through `TaskExecutor`; L8 and roster jobs are pure, order-independent, and
safe to supersede, closing over `Arc`ed immutable input tiles and roster
snapshots. Two Phase 3 refinements:

- **Roster jobs** are pure functions of a signature; two regions requesting the
  same signature may both dispatch one before either lands — the integrator
  keeps the first and drops the duplicate (same content, so harmless), or the
  scheduler dedups by an in-flight signature set. Either way content is a
  function of the signature key, so order cannot affect the result.
- **Near-field realization** runs on the main thread as a pure read of settled
  caches (§8.3). It is not a `TaskExecutor` job in Phase 3 (the near window is
  small and realization is cheap); if profiling later demands it, it parallelizes
  trivially because each region's organisms are an independent pure function of
  that region's tiles — no cross-region state. Kept Web-Worker-compatible by
  construction (section 19).

Sequencing repeats the Phase 1/2 de-risking: every milestone lands and passes
the replay under `InlineExecutor` first; the threaded path is re-validated by
the same tests afterward.

---

## 11. Debug visualization and tools

- **Map channels** (`viz.rs`): add `Herbivore`, `Predator`, `Diversity`,
  `DominantSpecies` (categorical palette by species-id hash); the `Composite`
  view tints by dominant-species colour so ecosystem zones read at a glance.
  Near-field organisms overlay as markers coloured by expressed appearance —
  the popping/coherence detector for Tier B (a marker that contradicts its
  cell's aggregate is instantly visible).
- **Panel**: player-cell habitat signature, roster size, dominant species name/id,
  trophic breakdown, and per-layer regen counters (now including L8) plus
  organisms-realized and roster-cache size.
- **`wer-inspect --species X Y`**: the cell's signature, full roster (each
  species' id, genome, trophic role), and food-web edges — makes ecology
  *legible*, the Tier-A analogue of `--layers`.
- **`wer-inspect --ecology X Y`**: L8 aggregate values plus the dependency-hash
  chain and stale/fresh verdict (extends `--layers` to id 8).
- **Ecology harness** (`tools`): headless runner for the §12.3 coherence and
  response scenarios — the Phase 3 sign-off tool, alongside the Phase 2
  invalidation ledger (which still runs and still passes).

---

## 12. Testing strategy

### 12.1 Golden determinism fixtures (extend `determinism.rs`)

New known-answer fixtures (no existing fixture re-blesses, §9.1):
`species_seed` for fixed signatures, `Genome::from_seed` trait words for fixed
seeds, `species_roster` composition for a fixed signature (roster size + each
species' trophic role), `food_web` edges + tier biomass for a fixed roster,
population aggregates for a fixed L8 input chain, and `layer_dep_hash` for the
L8 chain.

### 12.2 Continuity replay (extend, must stay green)

The Phase 2 script and assertions run unchanged over the deeper stack, plus:

- Per-channel epsilons for `herbivore`/`predator`/`diversity`.
- **Aggregate-preservation assertion**: near-field organism count/coverage in a
  cell is within tolerance of that cell's aggregate density/pressure (section 10).
- **Organism-identity stability**: a pinned near region realizes bit-identical
  organism ids across frames and across a two-run replay; organisms do not
  flicker in/out while the region stays pinned.
- Two-run state-hash equality now also covers the L8 tiles and the roster cache.

### 12.3 Ecology harness (the Phase 3 success criterion)

Two scenario families over a settled window, each machine-checked:

**Coherence invariants (hold for every cell/region):**

| Invariant | Assertion |
|---|---|
| Productivity bound | herbivore biomass ≤ α · primary productivity |
| Trophic bound | predator biomass ≤ β · herbivore biomass |
| Body-size bound | max realized body size ≤ f(primary productivity) |
| No orphan tiers | no predator with zero admissible prey survives in the web |
| Aggregate ↔ entity | realized organism trophic mix matches the cell's aggregate |

**Response / diversity (scripted possibility changes):**

| Change | Expected effect |
|---|---|
| Aesthetics bucket flip | organism colour expression shifts; rosters/webs/biomass **unchanged**; only L8 regenerates |
| Morphology/Behavior flip | body-plan/activity expression shifts; L8 only |
| Ecology bucket flip | Vegetation density + L8 pressures/diversity shift (Ecology drove Vegetation since Phase 2); nothing else |
| Climate/Soil (upstream) flip | biome→…→L8 cascade; roster changes where the signature crosses a band |
| Geology (slow) flip | **no organism change** in unpinned regions beyond what the terrain/soil cascade already implies; stable trio untouched |
| Settled window | species count ≥ diversity floor; neighbouring distinct biomes carry distinct rosters |

Plus a budget test: entering a dense biome realizes organisms over several
frames with `organisms_realized ≤ max_realize_organisms` each frame and no
frame realizing from a stale L8 tile.

### 12.4 Unit tests

Graph well-formedness re-checked with L8 (deps strictly lower id; L8's closure;
every channel exactly one producer, now 13); the Phase 2
`domain_dirty_masks_match_the_declared_graph` test **updated** so M/B/A/E map to
exactly `{L8}` (was `{}` / `{Vegetation}`); dep-hash sensitivity for the L8 fold;
`HabitatSignature::of` banding boundaries; `species_roster` determinism and
`ROSTER_MAX` cap; `food_web` post-conditions equal the §12.3 invariants;
`FieldTile<u16>` round-trips; roster-cache dependent-tracking and eviction;
realization preserves aggregate within tolerance; realization is a pure function
of `(tiles, rosters)`.

### 12.5 Native ↔ wasm parity

`platform-web` exports `genome_sample()` (genome of a fixed seed) and
`food_web_sample()` (tier biomass for a fixed roster), pinned to the native
goldens in the existing parity test. Signature derivation is **not** exported —
it is presentation-grade by decision (§9.3, ADR 0010), and asserting it
cross-platform would bake in a guarantee Phase 3 explicitly does not make.

### 12.6 CI

The existing contract, unchanged: fmt, clippy `-D warnings`, native
check+test, wasm32 check of the neutral crates + `platform-web`. New benches
build in CI but are not timing-gated.

---

## 13. Profiling and metrics

- Per-layer generation time/count now includes L8; roster-build time and cache
  hit rate; realization time and organism count for the near window.
- Criterion benches: `Genome::from_seed`, `species_roster`, `food_web` over a
  representative roster, L8 generation over one tile (cold roster vs warm cache),
  `realize_region` for a dense near region, and a full window settle from cold
  including ecology. These calibrate `LAYER_ECOLOGY.cost`,
  `max_realize_organisms`, and the roster-cache size.
- Cache telemetry grows roster-cache bytes and the L8 tile bytes (§6.2).

---

## 14. Native and browser constraints

Unchanged obligations, restated where Phase 3 stresses them: all new generation
and genetics code is pure and wasm-clean (CI-enforced); `Arc` and `Vec` are fine
in neutral crates (alloc, not platform); the roster cache and organism lists are
bounded and evictable (section 19's "no large monolithic allocations"); L8 and
roster jobs remain resumable/supersedable with no job-to-job waits (§10);
near-field realization is a pure read that parallelizes if ever needed. The
`platform-web` shell grows only the two parity exports.

---

## 15. Risks (mapping section 23)

| Risk | Phase 3 manifestation | Mitigation |
|---|---|---|
| 23.2 Scope | Ecology/food-web becomes a simulation science project | Aggregate-before-entity (§3); fixed rule-based constraints in one relaxation pass, no simulation loops (§7.4); organisms *realized*, not simulated (§1.4); §12.3 checks the rules, not a re-derivation. |
| 23.5 Determinism drift | Species identity diverges native vs wasm | `genome(seed)` is the only cross-platform claim and is parity-tested; signature/roster declared presentation-grade with a named Phase 5 upgrade (ADR 0010, §9.3). |
| 23.3 Dependency explosion | A colour anchor regenerates ecosystems everywhere | L8's declared E/M/B/A domains + the Phase 2 dep-hash/budget machinery, unchanged; §12.3 machine-checks that Aesthetics touches only L8 expression. |
| 23.1 Continuity | Near-field organisms pop or contradict the aggregate | Aggregate-preservation + identity-stability replay assertions (§12.2); organism markers in viz; near regions pinned so rosters hold still while visible (§7.6). |
| 23.6 Memory growth | Roster cache + organism lists grow unbounded | Signature-keyed shared rosters with dependent eviction (§6.3); `ROSTER_MAX`; transient, capped near-window organisms (§6.2, §8.4). |
| 23.4 Platform divergence | Realization assumes threads / shared memory | Pure per-region function, main-thread in Phase 3, Web-Worker-compatible by construction (§10); wasm check + parity every milestone. |

The phase-specific risk: **coherence-vs-diversity tension** — constraints tight
enough to guarantee plausible webs can flatten diversity into sameness. Mitigation:
the harness asserts *both* a diversity floor and the coherence invariants (§12.3),
so neither can be won by sacrificing the other.

---

## 16. Incremental milestones

Each keeps CI green (including wasm32), keeps the continuity replay and the
Phase 2 invalidation ledger passing, and preserves the crate-boundary and
determinism invariants. No milestone re-blesses a Phase 2 fixture (§9.1).

- **M1 — Genome + species substrate.** `habitat.rs`, `genome.rs`, `species.rs`;
  `HabitatSignature`, `Genome::from_seed` + expression, `species_seed`,
  `species_roster`; ADR 0010; genome/roster goldens + `genome_sample` parity
  export. Pure world-core, no graph change. *Exit:* deterministic genomes and
  rosters; genome parity native == wasm; `ROSTER_MAX` and banding unit-tested.
- **M2 — Food webs + plausibility.** `foodweb.rs`, `population.rs`;
  `food_web` with the section 8 constraints; coherence invariants as unit tests;
  food-web goldens + `food_web_sample` parity. *Exit:* every constructed web
  satisfies the §12.3 coherence invariants; parity holds.
- **M3 — Aggregate ecology layer L8.** Append `LAYER_ECOLOGY`; `RosterCache`;
  population channels + dominant-species `u16` tile; `generate.rs` L8 arm; wire
  E/M/B/A; update the domain-dirty-mask test; roster jobs on the scheduler; viz
  channels. *Exit:* L8 settles under budget with both executors; steering
  Morphology/Behavior/Aesthetics regenerates exactly L8, and Ecology regenerates
  only Vegetation + L8; a Geology/Climate-only anchor provably never regenerates
  ecology beyond the environmental cascade.
- **M4 — Near-field realization.** `realize.rs`, the near-window realization
  pass, `FeatureKey`-derived organism identities, `max_realize_organisms`,
  organism debug markers. *Exit:* near-field organism counts preserve the
  aggregate within tolerance; organism ids are stable across frames and a
  two-run replay.
- **M5 — Succession + response.** Distance-based regeneration and offscreen
  replacement wired (re-realize when source tiles change; discard on eviction);
  distant-ecology-shifts-under-steering demonstrated. *Exit:* a converging
  far region visibly shifts its dominant species/colour as its L8 regenerates,
  and re-realizes coherently as the player approaches — no pop, no stored state.
- **M6 — Ecology harness + sign-off.** The coherence + response/diversity
  scenario harness (§12.3); `wer-inspect --species` / `--ecology`; benches
  calibrating `LAYER_ECOLOGY.cost` and realization budgets; the aggregate-
  preservation and identity-stability replay assertions. *Exit:* every §12.3
  invariant and response assertion holds over a settled window; the diversity
  floor and coherence bounds hold simultaneously.

**Phase 3 is done when** M1–M6 are complete, CI is green (native + wasm32,
goldens, parity, continuity replay, Phase 2 ledger, ecology harness), and the
success criterion holds with evidence: the world produces diverse, internally
coherent ecosystems (machine-checked invariants + diversity floor) that respond
meaningfully to Ecology/Morphology/Behavior/Aesthetics steering while leaving
the environmental stack and the stable trio exactly as precise as Phase 2 left
them — the organism-rich foundation Phase 4's anchors will let the player steer.
