# Phase 2 — Layered Environmental Generation: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 2 work begins
(it covers the ground of `world-layer-dependency-plan.md` and thin slices of
`hydrology-plan.md`, `terrain-generation-plan.md`, and `ecology-field-plan.md`).
It expands the Phase 2 scope in section 20 into concrete interfaces, data
layouts, algorithms, and milestones, grounded in the validated Phase 1
continuity prototype ([`phase-1-plan.md`](phase-1-plan.md), ADRs 0004–0006).

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it assumes
them and calls out where Phase 2 stresses each.

---

## 1. Goals and non-goals

### 1.1 The question Phase 2 must answer

Phase 1 proved the continuity illusion on a deliberately short, hard-coded
3-layer stack (terrain → climate → ecology) with a single blunt invalidation
rule (`DRIFT_LAYERS`). Phase 2 asks:

> Can world generation scale to a real multi-layer environmental pipeline —
> climate, geology, hydrology, soils, biomes, vegetation — where **changes only
> recompute the layers that actually depend on them**, while generation remains
> stable, reproducible, and inside its temporal budgets? (section 20, Phase 2)

This is the direct attack on the **dependency explosion** risk (section 23.3).
Everything below serves a machine-checkable answer.

### 1.2 Success criterion (from section 20)

- **Precision:** a change to possibility state, or to one layer's algorithm,
  recomputes exactly the layers that declare a dependency on it — nothing more.
  Machine-checked by the invalidation-precision harness (§12.3).
- **Stability:** major topology (elevation, lithology, drainage networks) never
  moves under possibility drift; only expression layers (river width, wetness,
  soils, biome, vegetation) do (section 9).
- **Reproducibility:** the same inputs always produce the same world; the
  Phase 1 continuity replay still passes on the deeper stack; native and wasm
  still agree on every integer identity.
- **Budgets:** the deeper pipeline still respects per-frame budgets — a large
  possibility change ripples through six layers over multiple frames without
  hitching.

### 1.3 Goals

- **A generalized layer dependency graph** replacing the hard-coded Phase 1
  stack: layers declare their input layers and the possibility domains they
  read; dirtiness propagates along declared edges only (section 6.5).
- **Region-layer dependency hashes**: staleness becomes a pure integer
  comparison against a hash of *exactly the inputs a layer consumes* —
  superseding the coarse `(world_version, revision)` provenance of Phase 1.
- **Layer-specific revisioning**: each layer carries its own algorithm
  revision, so changing one layer's math invalidates that layer and its
  dependents, not the world.
- **The Phase 2 layer set** (section 20): climate (upgraded), geology
  expression, hydrology (stable drainage topology + drifting expression),
  soils, biome classification, aggregate vegetation.
- **Temporal generation budgets by cost**, not by count: layers declare
  estimated costs; the frame budget spends cost units (section 6.6).
- **Debug visibility** for all of it: new map channels, per-layer regen
  counters, and an inspector that explains *why* a tile is stale.

### 1.4 Non-goals (explicitly deferred)

- **Organisms, species, food webs, genetics** (Phase 3). Aggregate vegetation
  stays a small set of scalar fields (density, canopy height) — richer than
  Phase 1's single scalar, far short of section 10's full field list.
- **Expanding the possibility vector.** The 8-domain, one-scalar-per-domain
  `PossibilityVector` is unchanged. Phase 2's question is the *graph*, not
  possibility richness; expanding domains into sub-vectors is orthogonal.
- **New anchor kinds or constraint richness** (Phase 4). `steer` /
  `project_plausible` are untouched.
- **Persistence of generated tiles** (Phase 5). The `Storage` trait stays
  unused; dependency hashes are run-local cache keys, not a persistence format.
- **GPU field refinement, SIMD, clipmaps, LOD tiles** (Phase 6). Tiles stay
  single-resolution `FIELD_RES` buffers.
- **Scientific simulation.** No erosion simulation, no hydraulic routing, no
  soil chemistry — ecological *plausibility* over science (section 9).
- **Time-driven world change** (weather, seasons). ADR 0006 stands: change is
  fueled by travel; anything clock-driven must revisit that ADR, not sneak in
  through a climate layer.
- **Adaptive possibility quadtree.** The uniform control-point lattice stays.

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 2 coverage |
|---|---|
| `world-layer-dependency-plan.md` | **Core of Phase 2** — this document largely *is* it. |
| `hydrology-plan.md` | First real slice: stable drainage topology + drifting expression. |
| `terrain-generation-plan.md` | Geology expression (lithology) added beside the Phase 1 heightfield. |
| `ecology-field-plan.md` | Aggregate vegetation upgraded (density + canopy). |
| `job-system-plan.md` | Topological dispatch + cost budgets on the existing `TaskExecutor`. |
| `determinism-and-versioning-plan.md` | Per-layer algorithm revisions; one `WORLD_ALGORITHM_VERSION` bump. |
| `profiling-and-benchmarking-plan.md` | Per-layer counters/benches extend the Phase 1 harness. |

Region streaming, the possibility field, anchors, the renderer's presentation
path, and both platform shells change only where the layer graph forces them to.

---

## 3. Architecture overview

```text
                       possibility current (quantized per domain)
                                        │
      ┌────────────── stable trio ──────┼───────────── expression layers ─────────────┐
      │                                 │                                             │
 L0 Terrain ──────────────┬────────► L3 Climate ────────┬──────────────┐              │
 (elevation)              │         (temperature,       │              │              │
      │                   │          moisture)          │              │              │
      ▼                   │             │               ▼              │              │
 L2 Drainage (macro) ─────┼─────────────┴────► L4 Hydrology expr       │              │
 (flow dirs + accum,      │                    (river width, wetness)  │              │
  one cell per region)    │                             │              │              │
                          │                             ▼              ▼              │
 L1 Geology ──────────────┴──────────────────────► L5 Soils ────► L6 Biome ────► L7 Vegetation
 (lithology, hardness)                             (depth,       (class id)     (density,
                                                    fertility)                   canopy)
```

Three commitments organize everything:

1. **The stable trio (terrain, geology, drainage) is a pure function of
   position + the slow possibility dimensions.** Fast-dimension drift
   (climate, hydrology, ecology steering) can never dirty them, not by a
   hard-coded mask but because their *declared* inputs don't include the fast
   domains. Rivers do not walk; mountains do not move; rock does not change
   under a climate anchor (section 9).
2. **A tile's content is a pure function of its dependency key.** Layers
   generate from the *quantized* possibility inputs and from their input
   layers' tiles, so `dependency hash → tile content` is a function. Staleness
   is `stored_hash != expected_hash` — exact, cheap, and free of both
   over-invalidation (Phase 1's revision coupling) and silent skew (ADR 0005's
   accepted terrain drift-lag, which this supersedes).
3. **Layer ids are assigned in topological order.** Scanning the dirty bitset
   in id order *is* a dependency-order traversal, and the id doubles as the
   dispatch tiebreak — near-field tiles build bottom-up automatically.

---

## 4. The layer graph (world-core)

### 4.1 Layer ids and declarations

`world-core/src/layer.rs` is generalized from three constants to a static
declaration table. Ids are stable integers in topological order (they
participate in `FeatureKey.layer` and dirty bitsets). Phase 2 reassigns ids —
safe exactly once, because nothing is persisted yet and the phase bumps
`WORLD_ALGORITHM_VERSION` anyway (§9.1); after Phase 2 lands, ids are frozen.

```rust
pub const LAYER_TERRAIN: u16 = 0;    // elevation (stable)
pub const LAYER_GEOLOGY: u16 = 1;    // lithology, rock hardness (stable)
pub const LAYER_DRAINAGE: u16 = 2;   // macro flow topology (stable)
pub const LAYER_CLIMATE: u16 = 3;    // temperature, moisture
pub const LAYER_HYDROLOGY: u16 = 4;  // river width, surface wetness
pub const LAYER_SOILS: u16 = 5;      // soil depth, fertility
pub const LAYER_BIOME: u16 = 6;      // classification
pub const LAYER_VEGETATION: u16 = 7; // aggregate density + canopy height
pub const LAYER_COUNT: u16 = 8;

/// Everything the graph needs to know about one layer, declared statically.
#[derive(Debug)]
pub struct LayerDecl {
    pub id: u16,
    /// Input layers (each strictly lower id — acyclicity by construction).
    pub deps: &'static [u16],
    /// Possibility domains this layer reads *directly* (bit = domain.index()).
    pub domains: u8,
    /// Bumped when this layer's algorithm changes without a world-version
    /// bump being warranted; folded into the dependency hash (§9.2).
    pub algorithm_revision: u16,
    /// Relative generation cost in budget units (§8.2).
    pub cost: u32,
}

pub const LAYERS: [LayerDecl; LAYER_COUNT as usize] = [ /* table below */ ];
```

The declaration table (domain abbreviations: **P**lanetary, **C**limate,
**G**eology, **H**ydrology, **E**cology):

| Layer | deps | direct domains | notes |
|---|---|---|---|
| 0 Terrain | — | G, P | Phase 1 heightfield, unchanged math (ADR 0004). |
| 1 Geology | — | G | Integer-hashed lithology; hardness scalar. |
| 2 Drainage | Terrain | — | Macro-level flow topology; slow dims arrive via Terrain. |
| 3 Climate | Terrain | C, H, P | Phase 1 climate, re-expressed on the graph. |
| 4 Hydrology | Terrain, Drainage, Climate | H, P | River width, wetness — the drifting *expression* of stable topology. |
| 5 Soils | Terrain, Geology, Climate, Hydrology | — | All sensitivity inherited through inputs. |
| 6 Biome | Climate, Hydrology, Soils | — | Pure classification of its inputs. |
| 7 Vegetation | Climate, Soils, Biome | E | Ecology domain drives density directly. |

Derived, computed once as `const` or at startup and unit-tested (§12.4):

- `dependents_closure(layer) -> u32` — transitive dependents bitmask, for dirty
  propagation.
- `domain_readers(domain) -> u32` — layers whose *own* declaration reads a
  domain (closure applied afterward).
- `DRIFT_LAYERS` and `ALL_LAYERS` are deleted; ADR 0005's static mask is
  superseded by the declarations (new ADR, §9.4).

### 4.2 Quantized possibility inputs

`world-core/src/possibility.rs` gains explicit quantization:

```rust
/// Quantization steps per possibility dimension. 4096 steps ⇒ a one-bucket
/// change moves any generated sample by well under the continuity replay's
/// per-frame epsilon, and drift smaller than a bucket costs zero regeneration.
pub const POSSIBILITY_QUANT: u16 = 4096;

impl PossibilityVector {
    /// Quantize one domain to an integer bucket (identity-grade input).
    pub fn quantized(&self, domain: PossibilityDomain) -> u16;
    /// The exact f32 a generator must consume for a bucket (dequantization).
    pub fn dequantize(bucket: u16) -> f32;
}
```

Generators consume the **dequantized** values, never the raw `current` floats.
This is what makes commitment 2 in §3 true: tile content is a function of the
integer dependency key. It also rate-limits invalidation for free — convergence
lerps `current` every step, but layers only go stale when a *bucket* flips.

Buckets are run-local cache keys, not cross-platform identities: `current` is
runtime float state, so bucket boundaries may land differently across
platforms for the same script. The cross-platform identity surface remains the
integer seed layer (gradient seeds, control-point seeds, lithology seeds,
drainage routing), exactly as in Phase 1 (§9.3).

### 4.3 Dependency hashes

New module `world-core/src/dephash.rs` — pure integer hashing over the
existing `mix` primitive:

```rust
/// The per-(region, layer) dependency hash: a stable fold of everything the
/// layer's output depends on. Fold order is part of the stable contract.
///
///   basis → WORLD_ALGORITHM_VERSION → layer id → layer algorithm_revision
///         → region (x, y, level) → field resolution
///         → quantized bucket of each directly-read domain (stable domain order)
///         → dep_hash of each input layer's tile (declaration order)
///         → [macro input: the drainage tile's dep_hash, when declared]
pub fn layer_dep_hash(
    region: RegionCoord,
    layer: u16,
    quantized: &[u16],      // buckets for the layer's direct domains
    input_hashes: &[u64],   // dep hashes of input tiles, declaration order
    resolution: u16,
) -> u64;
```

A tile is stale iff its stored hash differs from the freshly computed expected
hash. Because input hashes chain, a change anywhere upstream — a possibility
bucket, an upstream algorithm revision, the world version — changes every
downstream expected hash automatically. There is no second invalidation
mechanism to keep in sync.

`FieldTile` provenance changes accordingly: the `(world_version, revision)`
pair and `is_stale` comparison are replaced by a single `dep_hash: u64` (the
region's `revision` stays on `RegionState` for the pinned-stability contract
and the replay; it just no longer drives staleness).

---

## 5. Public interfaces

### 5.1 `world-core` additions and changes

```text
world-core/src/
    layer.rs      # REWRITTEN: LayerDecl table, closures, id constants (§4.1)
    dephash.rs    # NEW: layer_dep_hash (§4.3)
    possibility.rs# + quantized()/dequantize() (§4.2)
    geology.rs    # NEW: lithology id + rock hardness (§7.2)
    drainage.rs   # NEW: macro flow routing over quantized elevation (§7.3)
    hydrology.rs  # NEW: river width + surface wetness (§7.4)
    soils.rs      # NEW: soil depth + fertility (§7.5)
    biome.rs      # NEW: Biome enum + classification (§7.6)
    vegetation.rs # NEW: density + canopy height (§7.7); replaces ecology.rs
    climate.rs    # unchanged math, inputs re-plumbed through the graph
    terrain.rs    # unchanged math (version constant folds bump, §9.1)
    field.rs      # FieldTile provenance → dep_hash; FieldTile<u8> hashing
    ecology.rs    # DELETED (superseded by vegetation.rs)
```

All new modules are pure and wasm-clean, in the existing
`#[inline] #[must_use] const fn` style where applicable. Signatures are
illustrative:

```rust
// geology.rs
pub struct Geology { pub lithology: u8, pub hardness: f32 }
pub const fn lithology_seed(cell_x: i64, cell_y: i64) -> u64; // integer identity
pub fn geology(world_x: f64, world_y: f64, p_geology: f32) -> Geology;

// drainage.rs — see §7.3 for the algorithm.
pub struct DrainageTile { /* per-region-cell flow dir (u8) + accumulation (u32) */ }
pub fn drainage(macro_coord: RegionCoord, elevation_grid: &QuantizedGrid) -> DrainageTile;

// hydrology.rs
pub struct Hydrology { pub river: f32, pub wetness: f32 }
pub fn hydrology(elevation: f32, drainage_accum: u32, c: &Climate, p: &QuantizedDims) -> Hydrology;

// soils.rs
pub struct Soils { pub depth: f32, pub fertility: f32 }
pub fn soils(elevation: f32, slope: f32, g: &Geology, c: &Climate, h: &Hydrology) -> Soils;

// biome.rs
#[repr(u8)] pub enum Biome { Ocean, River, Wetland, Desert, Grassland,
    Shrubland, TemperateForest, Rainforest, Taiga, Tundra, Bare, Ice }
pub fn classify(elevation: f32, c: &Climate, h: &Hydrology, s: &Soils) -> Biome;

// vegetation.rs
pub struct Vegetation { pub density: f32, pub canopy_height: f32 }
pub fn vegetation(b: Biome, c: &Climate, s: &Soils, p_ecology: f32) -> Vegetation;
```

### 5.2 `world-runtime` changes

```text
world-runtime/src/
    generate.rs  # channels grow; jobs take snapshotted input tiles (§6.2, §8.1)
    stream.rs    # dep-hash staleness; topological multi-pass dispatch (§8.1)
    budget.rs    # cost-weighted regen budget (§8.2)
    macrocache.rs# NEW: drainage tiles keyed by macro RegionCoord (§6.3)
    region.rs    # converge() reports changed buckets instead of ORing a mask
```

Key signature changes:

```rust
// generate.rs — jobs are still pure, but now explicitly carry their inputs.
pub struct LayerInputs {
    pub quantized: Vec<u16>,                       // direct-domain buckets
    pub tiles: Vec<(usize, Arc<FieldTile<f32>>)>,  // input channels, by CHANNEL_*
    pub biome: Option<Arc<FieldTile<u8>>>,         // biome input where declared
    pub drainage: Option<Arc<DrainageTile>>,       // macro input where declared
    pub dep_hash: u64,                             // the tile's provenance-to-be
}
pub fn generate_layer(coord: RegionCoord, layer: u16, inputs: &LayerInputs,
                      resolution: u16) -> GeneratedTile;

// region.rs — drift now reports *what* changed; the stream maps that to layers.
impl RegionState {
    /// Returns the set of domains whose quantized bucket flipped, or None if
    /// nothing moved. Callers translate buckets → dirty layers via the graph.
    pub fn converge(&mut self, rate: f32) -> Option<u8 /* domain bitmask */>;
}
```

`RegionState.dirty_layers` stays a `u32` bitset (8 of 32 bits used — room for
Phase 3). `GenerationStatus`, job ids, supersession, and the results channel
are unchanged.

### 5.3 `renderer`, `platform-native`, `tools`

- **Renderer:** unchanged — it still presents one composed texture.
- **`platform-native` (`viz.rs`, `panel.rs`):** new map channels — geology
  (hardness/lithology tint), river+wetness, soil, and a real biome palette; the
  Phase 1 `Channel::Biome` composite is renamed `Composite`. The info panel
  shows per-layer regen counters and the player cell's biome name.
- **`tools`:** `wer-inspect` grows a `--layers` dump: every layer's value at a
  position plus the full dependency-hash chain and each tile's stale/fresh
  verdict — the "why did this regenerate" debugging story. The continuity
  replay extends to the new channels (§12.2) and a new **invalidation ledger**
  binary drives the precision scenarios (§12.3).
- **`platform-web`:** exports two new parity samples (lithology seed, drainage
  routing sample) mirroring the native goldens (§12.5).

---

## 6. Data layout

### 6.1 Channels

`generate.rs` channel constants grow (illustrative layout):

| Channel | Type | Producer |
|---|---|---|
| `CHANNEL_ELEVATION` | f32 | Terrain |
| `CHANNEL_HARDNESS` | f32 | Geology |
| `CHANNEL_TEMPERATURE`, `CHANNEL_MOISTURE` | f32 | Climate |
| `CHANNEL_RIVER`, `CHANNEL_WETNESS` | f32 | Hydrology |
| `CHANNEL_SOIL_DEPTH`, `CHANNEL_FERTILITY` | f32 | Soils |
| `CHANNEL_VEGETATION`, `CHANNEL_CANOPY` | f32 | Vegetation |
| biome tile (separate field) | u8 | Biome |

Biome ids are small integers, so they get an honest `FieldTile<u8>` (with its
own `content_hash`) rather than being smuggled through f32. Lithology ids stay
inside the geology generator (hardness is the cached expression); if Phase 3
needs the id per cell, it becomes a second u8 tile then.

### 6.2 Shared tiles

`RegionTiles` channels become `Option<Arc<FieldTile<f32>>>` (plus the
`Option<Arc<FieldTile<u8>>>` biome). Rationale: Phase 1 jobs recomputed
elevation per sample to stay independent; with a six-deep graph that redundancy
compounds (soils would recompute terrain, climate, and hydrology per sample)
and drainage cannot be recomputed per-sample at all. Jobs instead receive
cheap `Arc` clones of their input tiles, snapshotted at dispatch. Tiles are
immutable once integrated, so sharing is safe, jobs stay pure, and results
remain order-independent (§10).

Memory at `FIELD_RES = 32`: 10 f32 channels ≈ 40 KB + 1 KB biome per region;
a 1,000-region window ≈ 41 MB — still comfortably inside the section 15 target.
Eviction semantics are unchanged (state + tiles drop together).

### 6.3 The macro drainage cache

Drainage is computed per **macro region** at `MACRO_LEVEL = 4` — a
`RegionCoord` with `level = 4`, covering 16×16 level-0 regions (4096 world
units, one `BASE_WAVELENGTH`), at **one cell per region** plus a 16-region
apron (48×48 cells ≈ 4.5 KB per tile — trivial).

`MacroCache` maps `RegionCoord@level4 → Arc<DrainageTile>` and lives beside
`RegionCache` inside `RegionMap`. A macro tile is resident while any level-0
region under it (or under its apron edge) is resident; eviction sweeps macro
tiles with no remaining dependents. In-flight bookkeeping reuses the existing
`(RegionCoord, layer)` map — macro coords are just `RegionCoord`s at a higher
level, which is exactly what `coord.rs` levels exist for.

---

## 7. Algorithms

### 7.1 Climate (re-plumbed, not redesigned)

Phase 1's `climate()` math survives; the change is plumbing — it now reads
dequantized buckets and the terrain input tile instead of raw `current` and
recomputed elevation. Its declaration (`deps: [Terrain]`, domains C/H/P) makes
its Phase 1 behavior an *instance* of the general graph.

### 7.2 Geology expression

- The world is partitioned into lithology cells on a coarse jittered integer
  lattice (≈ 4–8 regions across). Each cell's lithology id and base hardness
  derive from `lithology_seed` — pure integer hashing under a new fixed basis,
  same discipline as terrain gradients (ADR 0003/0004). Cell edges get a small
  hash-jittered warp so rock boundaries don't read as grid lines.
- Hardness is modulated smoothly by the slow Geology dimension (harder, more
  exposed rock in tectonically active worlds). No fast domain touches it.
- Output: `CHANNEL_HARDNESS` (+ lithology id available to soils via the pure
  function, not a cached channel).

### 7.3 Drainage (stable topology, macro level)

The determinism-critical piece: river networks are *topology* and must be
integer-derived (section 6.2), yet flow is inherently non-local. The design:

1. **Quantized elevation grid.** Sample `elevation()` at each macro cell
   center (one per region) over the 48×48 apron grid, then quantize to integer
   centimeters (`i32`). All routing decisions happen on integers from here on —
   float elevation never decides topology.
2. **Flow direction per cell**: steepest descent among the 8 neighbors on the
   quantized grid, ties broken by integer hash of the cell coordinate (a new
   fixed basis, golden-fixtured). Window-independent: a cell's direction
   depends only on its own 3×3 quantized neighborhood, so adjacent macro tiles
   can never disagree about shared cells.
3. **Flow accumulation**: counting cells draining through each cell, computed
   within the aproned window only. Truncated catchments are a *declared
   plausibility approximation* — long rivers saturate rather than grow without
   bound; width mapping is logarithmic so the truncation reads as "big river"
   rather than a seam. (Hierarchical accumulation at higher macro levels is
   the future refinement; not Phase 2.) A seam assertion in the replay bounds
   the residual width step across macro boundaries (§12.2).
4. Depressions (quantized local minima) become lakes/wetland seeds rather than
   being carved — plausibility over hydraulic correctness.

Drainage's only input is Terrain (whose slow-dim coupling is smooth and
coarse-bucketed), so river *networks* are stable under all fast drift, and
under slow drift they change only when a slow-dim bucket flips — far-field
only, since pinned regions never converge.

### 7.4 Hydrology expression (the drifting half)

Per level-0 sample: bilinearly read the macro accumulation under the sample,
map through `log`-shaped width and proximity falloff into `river ∈ [0,1]`, and
combine climate moisture, drainage proximity, low-slope ponding, and the
Hydrology/Planetary buckets into `wetness ∈ [0,1]`. This is where section 9's
"possibility drift should more commonly modify river width, surface wetness,
marsh extent" lands: the network is pinned by drainage; its *expression*
breathes with possibility.

### 7.5 Soils

`depth = f(slope↓, hardness↓, wetness↑ deposition)`,
`fertility = f(depth, moisture, temperature-window, lithology bias)`. Pure
per-sample arithmetic over four input tiles; no direct domain reads — all its
sensitivity is inherited, which makes it the best test of transitive
invalidation (§12.3).

### 7.6 Biome classification

A Whittaker-style temperature × moisture lookup with priority overrides:
water → `Ocean`; strong river → `River`; wetness + low slope → `Wetland`;
altitude/temperature floors → `Tundra`/`Ice`/`Bare`; shallow soil demotes
forest to shrub/grass. Output is the u8 tile. Biome ids are **derived
presentation** in Phase 2 (thresholds compare f32s, so knife-edge cells may
differ across platforms); if Phase 3 wants identity-grade biomes for species
hashing, classification inputs get quantized first — noted now so it is a
decision, not an accident.

### 7.7 Aggregate vegetation

Phase 1's `vegetation_density` grows into biome-parameterized density plus
canopy height: each biome contributes base density/canopy ranges; density
scales with fertility, moisture and the Ecology bucket; canopy needs soil
depth and shelters below the temperature window (section 8 plausibility rules:
canopy vs soil depth, vegetation vs rainfall — kept as code, mirroring
`project_plausible` at the possibility level).

### 7.8 Dirty propagation

On `converge()` reporting flipped domains `D`:

```text
dirty |= closure( union over d in D of domain_readers(d) )
```

where `closure` ORs in every transitive dependent. On integration of a
regenerated tile, downstream layers' expected hashes change automatically, so
they are picked up as stale on the next dispatch pass without any explicit
marking — `dirty_layers` becomes an optimization hint (skip hash checks for
clean regions), while dep-hash comparison is the ground truth. One bit of
bookkeeping matters: a region whose *upstream macro tile* regenerates must
re-check hydrology; the macro cache tracks its level-0 dependents for exactly
this notification.

---

## 8. Scheduling and budgets

### 8.1 Topological dispatch

`dispatch_regen` becomes a fixed-point loop per frame:

```text
loop (while budget remains and progress was made):
    integrate finished results
    for each resident region (nearest-first), for each layer in id order:
        if tile stale (dep-hash mismatch) AND all input tiles fresh
           AND not in flight:
            snapshot inputs (Arc clones + buckets), dispatch job
```

- With the `InlineExecutor`, one frame settles an entire region bottom-up
  (each pass completes synchronously) — headless tools keep their
  settle-in-one-frame property. With the threaded executor, each pass
  dispatches whatever became ready; a fresh region settles over a handful of
  frames, deepest layers last, which is the correct visual order anyway
  (terrain first, vegetation last).
- Priority: distance band → layer id (topo order) → coord, all deterministic.
  Macro drainage jobs ride the same queue at the priority of their nearest
  dependent region.
- Supersession is unchanged: jobs carry their dep-hash; a result whose hash no
  longer matches the current expected hash is dropped on arrival (this
  replaces the Phase 1 revision check — same shape, sharper key).

### 8.2 Cost-weighted budgets

`Budget.max_regen_layers` (a count) becomes `max_regen_cost` (units). Each
`LayerDecl.cost` is a small constant (terrain/climate cheap; hydrology and
vegetation mid; drainage macro jobs expensive), calibrated by the criterion
benches rather than taste. `max_loads`/`max_converge_regions` are unchanged.
`FrameStats` grows `regenerated_by_layer: [usize; LAYER_COUNT]`,
`macro_jobs`, and `regen_cost_spent` — the raw material for the precision
harness and for §13's dashboards.

---

## 9. Determinism and versioning

### 9.1 One world-version bump

Phase 2 changes generated output for identical inputs (new layers, climate
re-plumbed through quantized inputs), so `WORLD_ALGORITHM_VERSION` bumps
**1 → 2 exactly once**, in the first milestone that alters output (M1). Because
the version folds into every seed (`gradient_seed`, control-point seeds), every
golden fixture re-blesses **deliberately, in that same commit** — the one
sanctioned re-bless of the phase. Subsequent milestones add layers with *new*
fixtures; they must not re-bless existing ones (the AGENTS.md rule stands: a
casual re-bless is a determinism bug).

### 9.2 Layer algorithm revisions

Tuning one layer's constants after M1 bumps that layer's
`algorithm_revision` instead of the world version. The dep-hash chain
invalidates the layer and its dependents; golden fixtures for that layer (and
dependents' fixtures, where values change) update in the same commit. This is
"layer-specific revisioning" from the section 20 scope, and it is what keeps
the six-layer stack tunable without world-wide re-blessing.

### 9.3 The identity ledger

Integer, cross-platform, golden-fixtured and wasm-parity-tested:
terrain gradient seeds, possibility control-point seeds (existing);
lithology seeds, drainage tie-break hashes, quantized-elevation routing
decisions, dep-hash folds (new). Float, per-platform, replay-hash-checked
only: all tile samples, biome threshold outcomes, possibility buckets of
runtime `current` state.

### 9.4 New ADRs

- **ADR 0007 — Declared layer dependencies supersede the static drift mask**
  (supersedes ADR 0005): dirtiness flows only along declared edges from
  quantized-domain changes; the terrain drift-skew accepted by ADR 0005 is
  retired — slow-dim drift now honestly (and cheaply, via coarse buckets)
  regenerates far-field terrain.
- **ADR 0008 — Tiles are functions of their dependency hash**: quantized
  possibility inputs, the dep-hash chain, and the staleness rule.
- **ADR 0009 — Drainage topology from quantized elevation at macro level**:
  integer routing, window-independent directions, truncated-catchment
  approximation.

---

## 10. Threading model

Unchanged in kind from Phase 1 (§9 there): the neutral crates express
parallelism only through `TaskExecutor`; jobs are pure, order-independent, and
safe to supersede. Two Phase 2 refinements:

- Jobs now close over `Arc`ed immutable input tiles instead of recomputing
  inputs. The main thread remains the only cache writer; integration order
  still cannot affect final content because content is a function of the
  dep-hash key (§4.3) — a *stronger* order-independence argument than Phase 1's.
- Dependency ordering is enforced at **dispatch** (only ready layers are
  submitted), not by inter-job synchronization — no job ever waits on another
  job, which keeps the model Web-Worker-compatible (section 19).

Sequencing repeats the Phase 1 de-risking: every milestone lands and passes
the replay under `InlineExecutor` first; the threaded path is re-validated by
the same tests afterward.

---

## 11. Debug visualization and tools

- **Map channels** (`viz.rs`): `Composite` (renamed; now uses real biomes +
  river/wetness darkening), `Elevation`, `Geology`, `Temperature`, `Moisture`,
  `River`, `Wetness`, `Soil`, `Biome` (categorical palette), `Vegetation`,
  `Stability`, `Revision`. Rivers are the new popping-detector-in-chief: a
  drainage discontinuity is instantly visible as a broken river line.
- **Panel**: per-layer regenerated-this-frame counters, macro cache size, and
  the player-cell layer readout (biome name, soil, river).
- **`wer-inspect --layers X Y`**: prints each layer's sampled values, its
  declared inputs, the quantized buckets consumed, the expected vs stored
  dep-hash, and the stale verdict — the tool that makes invalidation *legible*.
- **Invalidation ledger** (`tools`): headless scenario runner that applies a
  scripted change and reports exactly which (region, layer) pairs regenerated,
  asserting the expected set (§12.3).

---

## 12. Testing strategy

### 12.1 Golden determinism fixtures (extend `determinism.rs`)

- Re-blessed at the M1 version bump: existing seeds/elevation/climate/field
  fixtures (one commit, §9.1).
- New known-answer fixtures: `lithology_seed`, `geology()`, drainage flow
  dir + accumulation for a fixed macro tile (small grid printed as the
  fixture), `hydrology()`, `soils()`, `classify()` (including each override
  branch), `vegetation()`, `layer_dep_hash` for a fixed input chain,
  `quantized()`/`dequantize()` round-trips at bucket edges.

### 12.2 Continuity replay (extend, must stay green)

The Phase 1 script and assertions run unchanged over the deeper stack, plus:

- Per-channel epsilons for the new channels; a new elevation epsilon (terrain
  may now regenerate under slow-dim drift in the far field — bounded by one
  quantization bucket's amplitude effect).
- **Stable-trio assertion**: terrain/geology/drainage tiles of *pinned*
  regions never change, ever; and under a fast-dims-only script, they never
  change anywhere.
- **Macro seam assertion**: river width across macro-tile boundaries steps by
  less than the truncation bound (§7.3).
- Two-run state-hash equality now also covers the macro cache and biome tiles.

### 12.3 Invalidation-precision harness (the Phase 2 success criterion)

Scripted scenarios over a settled window, each asserting the exact regen set:

| Change | Expected regeneration |
|---|---|
| Aesthetics/Morphology/Behavior bias | **nothing** |
| Ecology bucket flip | Vegetation only |
| Climate bucket flip | Climate → Hydrology → Soils → Biome → Vegetation; never Terrain/Geology/Drainage |
| Hydrology bucket flip | Climate (reads H) and downstream; stable trio untouched |
| Geology (slow) bucket flip | full pyramid — but only in unpinned regions |
| Soils `algorithm_revision` bump | Soils → Biome → Vegetation only |
| sub-bucket drift (< 1 bucket) | **nothing** |

Plus the budget test: a world-scale change with a small budget must ripple
over many frames with `regen_cost_spent ≤ max_regen_cost` every frame and no
frame regenerating a layer before its inputs.

### 12.4 Unit tests

Graph well-formedness (deps strictly lower id ⇒ acyclic; closures correct;
every channel has exactly one producer); dephash sensitivity (each folded
input changes the hash — mirroring `feature_hash_separates_every_field`);
converge bucket reporting; topological dispatch never submits a layer with a
stale input; macro cache dependent-tracking and eviction; `FieldTile<u8>`
round-trips.

### 12.5 Native ↔ wasm parity

`platform-web` exports `lithology_seed_sample()` and a
`drainage_routing_sample()` (flow direction + accumulation of a fixed cell in
a fixed macro tile — routing is all-integer, so full cross-platform equality
is required, not just seed equality). Pinned to the native goldens in the
existing parity test.

### 12.6 CI

The existing contract, unchanged: fmt, clippy `-D warnings`, native
check+test, wasm32 check of the neutral crates + `platform-web`. New benches
build in CI but are not timing-gated.

---

## 13. Profiling and metrics

- Per-layer generation time and count per frame (extends the Phase 1
  counters); macro job time; dep-hash check time (it runs for every resident
  region-layer every dispatch pass — verify it stays negligible, and add the
  clean-region skip via `dirty_layers` if it doesn't).
- Criterion benches: each new layer generator over one tile; `drainage()` for
  one macro tile; full window settle from cold; the invalidation scenarios of
  §12.3 as throughput benches (cost of a Climate flip over a full window).
  These calibrate `LayerDecl.cost` and the frame budget.
- Cache telemetry grows macro-cache bytes and per-layer tile bytes.

---

## 14. Native and browser constraints

Unchanged obligations, restated where Phase 2 stresses them: all new
generation code is pure and wasm-clean (CI-enforced); `Arc` is fine in neutral
crates (it is alloc, not platform); jobs remain resumable/supersedable with no
job-to-job waits (§10); no filesystem, threads, sockets, or graphics in
neutral crates; drainage's bounded windows keep allocations small and fixed
(section 19's "no large monolithic allocations"). The `platform-web` shell
grows only the two parity exports.

---

## 15. Risks (mapping section 23)

| Risk | Phase 2 manifestation | Mitigation |
|---|---|---|
| 23.3 Dependency explosion | A climate nudge regenerates six layers everywhere | The whole phase: declared edges, quantized buckets (sub-bucket drift is free), dep-hash precision, cost budgets; §12.3 machine-checks it. |
| 23.1 Continuity | Rivers/biome edges pop or tear at macro seams | Window-independent flow dirs; seam assertions; replay stays a gate on every milestone; river channel in the viz. |
| 23.5 Determinism drift | Float creeps into topology via drainage or biome thresholds | Integer-quantized routing (§7.3); biome declared presentation-grade (§7.6); parity samples for all new integer identities. |
| 23.2 Scope | Hydrology/soils becomes a science project | Plausibility-over-science rules (§7); fixed formulas, no simulation loops; truncated catchments accepted and documented. |
| 23.6 Memory growth | Macro cache + 3× channel growth | Dependent-tracked macro eviction; ~41 MB window (§6.2); telemetry with the existing eviction machinery. |
| 23.4 Platform divergence | `Arc`-tile plumbing assumes shared memory semantics | Tiles immutable-after-integration; dispatch-side ordering only; wasm check + parity in CI every milestone. |

The phase-specific risk: **re-blessing discipline** during the M1 version
bump — one commit, reviewed against §9.1, with the replay green before and
after.

---

## 16. Incremental milestones

Each keeps CI green (including wasm32), keeps the continuity replay passing,
and preserves the crate-boundary and determinism invariants.

- **M1 — Graph substrate + version bump.** `LayerDecl` table, closures,
  quantization, `dephash.rs`; the existing three layers re-expressed as graph
  nodes generating from dequantized buckets; dep-hash staleness replaces
  revision staleness; `DRIFT_LAYERS` deleted; `WORLD_ALGORITHM_VERSION = 2`
  with the one sanctioned fixture re-bless; ADRs 0007/0008. *Exit:* Phase 1
  behavior reproduced on the new machinery — replay green, sub-bucket drift
  regenerates nothing.
- **M2 — Topological scheduler + cost budgets.** `Arc` tile inputs, fixed-point
  dispatch, `max_regen_cost`, per-layer `FrameStats`. *Exit:* fresh regions
  settle bottom-up under budget with both executors; scheduler unit tests pass.
- **M3 — Geology expression.** `geology.rs`, hardness channel, viz channel,
  goldens + lithology parity export. *Exit:* stable rock provinces visible;
  fast-dim drift never touches them.
- **M4 — Macro drainage.** `drainage.rs`, `MacroCache`, integer routing,
  dependent-tracked eviction, goldens + routing parity export, ADR 0009.
  *Exit:* deterministic river networks span the window with no seams at macro
  boundaries; networks immobile under all fast drift.
- **M5 — Hydrology expression + soils.** `hydrology.rs`, `soils.rs`, four new
  channels, river/wetness/soil viz. *Exit:* river width and wetness visibly
  breathe with Hydrology steering while the network stands still; soils
  respond only through their inputs.
- **M6 — Biome + vegetation.** `biome.rs` (u8 tiles, palette), `vegetation.rs`
  (density + canopy) replacing `ecology.rs`; composite view upgraded. *Exit:*
  coherent biome maps that shift plausibly under steering; canopy respects the
  soil-depth rule.
- **M7 — Precision harness + sign-off.** Invalidation ledger with the §12.3
  scenario table; `wer-inspect --layers`; benches calibrating layer costs;
  budget-ripple test. *Exit:* every §12.3 scenario asserts its exact regen
  set; budgets hold under a world-scale change.

**Phase 2 is done when** M1–M7 are complete, CI is green (native + wasm32,
goldens, parity, replay, precision harness), and the success criterion holds
with evidence: changes recompute exactly the declared-dependent layers
(machine-checked), the stable trio never moves under drift, and the six-layer
world generates reproducibly inside its budgets — the validated foundation
Phase 3's ecology needs.
