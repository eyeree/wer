# Phase 1 — Continuous World Transformation Prototype: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 1 work begins. It
expands the Phase 1 scope in section 20 into concrete interfaces, data layouts,
algorithms, and milestones, grounded in the primitives that already exist after
Phase 0 (`world-core`, `world-runtime`, `renderer`, and the platform shells).

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the crate-boundary
rule, the determinism invariant, or the CI contract — it assumes them and calls
out where Phase 1 stresses each.

---

## 1. Goals and non-goals

### 1.1 The one question Phase 1 must answer

> Can a deterministic region-based world transform continuously through
> possibility space while preserving nearby stability and avoiding visible
> regeneration artifacts? (section 22)

Everything below is in service of a *yes/no* answer with evidence. If a feature
does not help demonstrate continuity, it is out of scope.

### 1.2 Success criterion (from section 20)

The player can move through the world and change possibility state, and:

- Nearby terrain and ecology **do not pop, snap, or contradict themselves.**
- Distant terrain visibly **transforms** as possibility state drifts or the
  player steers with an anchor.
- The same inputs always produce the same world (determinism holds, native and
  wasm agree on identities).

### 1.3 Goals

- Infinite, deterministic **heightfield** with stable major topology.
- **Small possibility vector** (reuse the existing 8-domain `PossibilityVector`;
  Phase 1 actively drives only a handful of dimensions).
- **Sparse possibility field** that varies smoothly across the infinite world.
- **Stable near radius** (pinned) and **transforming distant radius**
  (converging), with a smooth stability ramp between them.
- **Basic climate and ecology fields** derived from possibility + terrain.
- **One or two anchor types** (emphasize / suppress) steering the target state.
- **Region streaming**: a moving window of active regions around the player.
- **Incremental regeneration**: only the affected layers recompute when
  possibility drifts.
- **Debug visualization** good enough to *see* continuity (or its failure).

### 1.4 Non-goals (explicitly deferred)

- Individual organisms, procedural genetics, food webs (Phase 3).
- The full layered dependency graph — hydrology, soils, geology expression,
  biome classification as separate cached layers (Phase 2). Phase 1 uses a
  deliberately short 3-layer stack.
- Persistence of overrides, routes, preserves, named discoveries (Phase 5). The
  `Storage` trait exists but Phase 1 stores nothing through it; the world is
  fully reconstructed from the seed each run.
- Web Workers, browser storage, GPU compute, suspend/resume (Phases 6–7). The
  `wasm32` build must keep **compiling** and agree on identities, but there is no
  browser runtime yet.
- A production terrain renderer (clipmaps, LOD meshing, atlasing). Phase 1 ships
  the smallest visualization that reveals popping.
- Custom schedulers/allocators (section 23.2 — delay until profiling justifies).

---

## 2. Where this sits in the subsystem-plan map

Section 21 lists the full set of subsystem plans. Phase 1 draws a thin slice
through several of them; the rest stay unwritten until their phase:

| Section 21 plan | Phase 1 coverage |
|---|---|
| `region-streaming-plan.md` | **Core of Phase 1** — window, stability, eviction. |
| `possibility-space-plan.md` | Sparse lattice + interpolation (minimal). |
| `terrain-generation-plan.md` | Deterministic fBm heightfield (minimal). |
| `ecology-field-plan.md` | Single aggregate vegetation scalar (minimal). |
| `anchor-system-plan.md` | 1–2 anchor types, trivial plausibility projection. |
| `world-layer-dependency-plan.md` | 3 hard-coded layers + dirty bitset (Phase 2 generalizes). |
| `job-system-plan.md` | Use the existing `TaskExecutor` trait; native impl. |
| `renderer-plan.md` | One debug pipeline; not the real render graph. |
| `determinism-and-versioning-plan.md` | Extend golden fixtures + wasm parity. |
| `profiling-and-benchmarking-plan.md` | Frame/gen budget counters + a bench. |

Hydrology, soils, genetics, entities, persistence, routes, GPU compute, and the
browser runtime are **not** touched.

---

## 3. Architecture overview

```text
                 player position + input
                          │
         ┌────────────────┼─────────────────────────┐
         │                │                          │
   possibility field   anchors                  streaming window
   (sparse lattice)    (emphasize/suppress)     (active RegionCoord set)
         └──────┬─────────┘                          │
                ▼                                     ▼
        target PossibilityVector  ──►  RegionState { current, target, stability }
                                              │  converge() within budget
                                              ▼
                                   dirty_layers bitset
                                              │
                        ┌─────────────────────┼──────────────────────┐
                        ▼                      ▼                      ▼
                 L0 Terrain (stable)    L1 Climate (drifts)   L2 Ecology (drifts)
                        └──────────── RegionCache (field tiles) ──────┘
                                              │
                                              ▼
                                   debug visualization (renderer)
```

Two crucial design commitments make continuity work:

1. **Terrain (major topology) is nearly possibility-independent.** The stable
   heightfield is a function of world position + a couple of slow possibility
   dimensions only. Possibility *drift* moves climate and ecology, not the
   mountains and valleys — matching section 9 ("major topology should be highly
   stable; possibility drift should more commonly modify river width, surface
   wetness, vegetation, …"). This is what prevents landmark contradiction.
2. **Realized state is pinned near the player and only converges at distance.**
   `RegionState::converge` already scales movement by `(1 - stability)`. Phase 1
   drives `stability` from distance so the player never watches the ground they
   stand on rewrite itself.

---

## 4. Public interfaces

All new pure computation lands in `world-core` (compiles for wasm); all
orchestration lands in `world-runtime`; all platform wiring in `platform-native`
(+ `renderer`). Signatures below are illustrative and should match the existing
`#[inline] #[must_use] const fn` style where applicable.

### 4.1 `world-core` additions

New modules, each pure and wasm-clean:

```text
world-core/src/
    terrain.rs           # deterministic infinite heightfield
    climate.rs           # temperature + moisture from possibility + elevation
    ecology.rs           # aggregate vegetation/canopy scalar
    possibility_field.rs # sparse lattice → interpolated target vector
    anchor.rs            # Anchor, steering combination, plausibility projection
    field.rs             # FieldTile<T>: a region-sized sample buffer + metadata
    layer.rs             # LayerId constants + dirty-bit helpers
```

Illustrative interfaces:

```rust
// layer.rs — the Phase 1 layer stack (a fixed subset of section 6.5).
pub const LAYER_TERRAIN: u16 = 0; // stable topology; rarely dirtied
pub const LAYER_CLIMATE: u16 = 1; // temperature + moisture
pub const LAYER_ECOLOGY: u16 = 2; // aggregate vegetation
pub const LAYER_COUNT: u16 = 3;

#[inline] #[must_use] pub const fn layer_bit(layer: u16) -> u32 { 1 << layer }

// terrain.rs — elevation is presentation state (f32), but every lattice
// gradient is chosen by INTEGER hashing so topology is reproducible.
// Depends only on world position + WORLD_ALGORITHM_VERSION + the slow
// Geology/Planetary possibility dims, so it is essentially possibility-stable.
#[must_use]
pub fn elevation(world_x: f64, world_y: f64, p: &PossibilityVector) -> f32;

// climate.rs — cheap, drifts with possibility. Temperature falls with
// elevation (lapse rate); moisture rises with the Hydrology dim.
pub struct Climate { pub temperature: f32, pub moisture: f32 }
#[must_use]
pub fn climate(elevation: f32, p: &PossibilityVector) -> Climate;

// ecology.rs — a single aggregate vegetation density in [0,1] for Phase 1.
#[must_use]
pub fn vegetation_density(elevation: f32, c: &Climate, p: &PossibilityVector) -> f32;

// possibility_field.rs — sparse control lattice, bilinearly interpolated.
// Control points are seeded deterministically from their integer coordinate.
pub struct PossibilityField { pub cell_regions: u32 /* lattice spacing */ }
impl PossibilityField {
    #[must_use] pub fn sample(&self, region: RegionCoord) -> PossibilityVector;
}

// anchor.rs — Phase 1 supports Emphasize and Suppress (section 8).
pub enum AnchorKind { Emphasize, Suppress }
pub struct Anchor {
    pub world_pos: (f64, f64),
    pub mask: u8,              // which PossibilityDomains this anchor touches
    pub kind: AnchorKind,
    pub strength: f32,         // 0..1
    pub falloff_radius: f64,   // world units
}
// Combine field sample + all nearby anchors into a steering result, then
// project through plausibility constraints (section 8) before it becomes target.
#[must_use]
pub fn steer(base: PossibilityVector, anchors: &[Anchor], at: (f64, f64))
    -> PossibilityVector;
#[must_use]
pub fn project_plausible(v: PossibilityVector) -> PossibilityVector;
```

`FieldTile<T>` holds a region's sampled buffer plus the `(revision, world_version)`
it was generated from, so staleness is a pure comparison (see §8).

### 4.2 `world-runtime` additions

```text
world-runtime/src/
    stream.rs    # RegionMap: the active window, load/evict, stability ramp
    generate.rs  # layer regeneration + RegionCache
    budget.rs    # per-frame temporal budgets (section 6.6)
```

Illustrative:

```rust
// stream.rs
pub struct StreamConfig {
    pub near_radius: f64,   // pinned (stability = 1)
    pub far_radius: f64,    // free (stability = 0); ramp in between
    pub load_radius: f64,   // regions kept resident
    pub unload_radius: f64, // eviction threshold (> load_radius = hysteresis)
}

pub struct RegionMap { /* HashMap<RegionCoord, RegionState> + cache */ }
impl RegionMap {
    pub fn new(cfg: StreamConfig) -> Self;
    /// Called once per frame. Loads/evicts around `player`, recomputes each
    /// region's stability + target, converges distant regions within budget,
    /// and returns which region-layers went dirty this tick.
    pub fn update(
        &mut self,
        player: (f64, f64),
        field: &PossibilityField,
        anchors: &[Anchor],
        budget: &mut Budget,
    );
    pub fn iter_active(&self) -> impl Iterator<Item = &RegionState>;
    pub fn cache(&self) -> &RegionCache;
}

// budget.rs — mirrors the section 6.6 examples for the Phase 1 subset.
pub struct Budget {
    pub max_converge_regions: usize,
    pub max_regen_layers: usize,
    pub max_loads: usize,
}
impl Budget { pub fn per_frame(target_ms: f32) -> Self; }
```

`RegionState` (already in `region.rs`) is reused as-is; §7 refines how
`stability`, `dirty_layers`, and `converge()` are driven. The current
`converge()` sets `dirty_layers = u32::MAX` on any change — Phase 1 narrows this
to only the possibility-dependent layers (climate, ecology), leaving terrain
undirtied (see §6.1 and §8).

### 4.3 `renderer` additions

One debug pipeline (WGSL, `renderer/shaders/debug_map.wgsl`) that draws the
active world as false color. See §10. The clear-only `render_clear` path stays;
the new path is additive.

### 4.4 `platform-native` and `tools`

- `platform-native`: input handling (move player, nudge possibility dimensions,
  drop/clear anchors, toggle debug overlays), the frame loop calling
  `RegionMap::update` then the renderer, and a concrete `TaskExecutor` (§9). The
  `demo_world_tick` scaffold is deleted.
- `tools`: extend `wer-inspect` to dump a region's terrain/climate/ecology
  samples and its target/current vectors; add a headless **continuity replay**
  (§11.3) that runs a scripted camera path and asserts near-field stability.

---

## 5. Data layout

Following section 15 (compact, data-oriented, quantized, handles over pointers)
but **not** prematurely optimizing (section 23.2). Phase 1 targets correctness
and legibility first; layout tightening is a Phase 6 concern.

- **Active region set**: `HashMap<RegionCoord, RegionState>`. `RegionCoord` is
  16 bytes and `Hash`; the window is a few hundred to low-thousands of regions,
  so a hash map is fine for Phase 1. (A sparse grid / quadtree per section 12.1
  is a later optimization.)
- **Field cache**: `RegionCache` keyed by `RegionCoord`, holding one
  `FieldTile<f32>` per possibility-dependent layer at a modest per-region
  resolution (e.g. `FIELD_RES = 32` → 32×32 samples). Terrain elevation is
  sampled on demand from the pure `elevation()` function and cached the same way.
  Each tile stores the `revision`/`world_version` it was built from.
- **Quantization**: field samples may be stored as `f32` for Phase 1 clarity;
  note in code where packing to `u8`/`u16` (section 15) is a later win. Identity
  inputs (region coords, feature/layer indices) remain integers — never store a
  float as an identity.
- **Memory budget**: with `FIELD_RES = 32`, 3 layers × 1024 samples × 4 bytes ≈
  12 KB/region; a 1,000-region window ≈ 12 MB of field cache — comfortably inside
  the "low hundreds of MB" target (section 15). Eviction (§7.4) bounds it.

---

## 6. Algorithms

### 6.1 Deterministic heightfield (`terrain.rs`)

- Multi-octave gradient (Perlin/simplex-style) noise, **fBm**. For each octave,
  the lattice-corner gradient is selected by hashing the integer corner
  coordinate through `feature_hash`/`splitmix64` and mapping the result to a unit
  direction. Interpolation and octave summation are `f32` (presentation), but the
  gradients — the thing that defines *where* mountains are — come from integer
  hashing, so topology is exactly reproducible for a given
  `WORLD_ALGORITHM_VERSION`.
- **Possibility coupling is deliberately weak.** Elevation reads only the slow
  `Geology`/`Planetary` dimensions (e.g. tectonic activity scales amplitude,
  ocean fraction shifts sea level) and does so through a smooth function. This is
  what keeps "major topology highly stable" (section 9) and is the single most
  important choice for avoiding landmark contradiction: as the player steers
  climate/ecology, the mountains do not walk around.
- Sea level, ridge/valley shaping (e.g. billow/ridged transforms) are fixed
  functions of the noise, not of fast possibility dims.

### 6.2 Climate and ecology (`climate.rs`, `ecology.rs`)

- **Climate**: `temperature = base(p.Climate) − lapse_rate * elevation_above_sea`;
  `moisture = f(p.Hydrology, p.Planetary_ocean, distance-to-low-elevation)`. Pure,
  cheap, no lattice needed beyond the region's own possibility + elevation.
- **Ecology**: `vegetation_density = plausible(p.Ecology, moisture, temperature,
  elevation)` — e.g. a product of climate suitability and the `Ecology`
  possibility dim, clamped by a rainfall/vegetation plausibility rule (section 8:
  "vegetation density versus rainfall"). One aggregate scalar in `[0,1]` for
  Phase 1; canopy/biomass/species split arrives in Phase 3.

These are the two layers that **drift**. They are cheap enough to recompute for a
whole region tile every time its realized possibility state changes, which is why
Phase 1 can afford naïve incremental regeneration.

### 6.3 Possibility field + steering (`possibility_field.rs`, `anchor.rs`)

- **Sparse lattice** (section 7): control points on a coarse grid, one every
  `cell_regions` regions. Each control point's base vector is derived
  deterministically from its integer coordinate (`Rng::from_key` seeded on the
  control-point coord). A region's base target = **bilinear interpolation** of the
  four surrounding control points → a smoothly varying field across the infinite
  world. (Adaptive quadtree is a later refinement; a uniform lattice is enough to
  prove continuity.)
- **Anchors** (section 8): for each active region, gather anchors within falloff,
  compute per-anchor influence `strength * falloff(distance)`, and combine into a
  steering delta over the masked dimensions. `Emphasize` pushes toward high,
  `Suppress` toward low. Sum onto the base target.
- **Plausibility projection** (`project_plausible`): a couple of rule-based clamps
  and one or two relaxation passes (section 8, "rule-based constraints and
  iterative relaxation rather than machine learning"), e.g. vegetation capped by
  moisture. Phase 1 keeps this tiny; the point is to prove the *seam* between
  steering and constraints exists, not to model an ecosystem.

The result of field-sample → steer → project is the region's **target**
`PossibilityVector`. `current` converges toward it per §7.

### 6.4 Convergence and continuity

`RegionState::converge(rate)` already lerps `current → target` scaled by
`(1 - stability)`. Phase 1:

- Drives `stability` from distance (§7.2), so near regions have `stability ≈ 1`
  (no movement, `converge` returns `false`) and far regions have `stability ≈ 0`
  (converge quickly).
- Narrows the dirtying: after a `current` change, dirty **only**
  `LAYER_CLIMATE | LAYER_ECOLOGY`, never `LAYER_TERRAIN` (patch `converge` or move
  the dirty policy into `stream.rs`). This is the incremental-regeneration win and
  the fix for the current bootstrap's `dirty_layers = u32::MAX`.

---

## 7. Streaming, stability, memory ownership

### 7.1 The window

Each frame, from the player world position:

1. Compute the player's `RegionCoord` (`RegionCoord::from_world`).
2. **Load**: for every region within `load_radius` not already resident, insert a
   fresh `RegionState` (status `Unloaded`), up to `budget.max_loads` per frame.
3. **Evict**: drop regions beyond `unload_radius` (hysteresis: `unload_radius >
   load_radius` prevents thrashing at the boundary), freeing their cache tiles.

### 7.2 Stability ramp

```text
d = distance(region_center, player)
stability = 1.0                       if d <= near_radius
stability = 0.0                       if d >= far_radius
stability = smoothstep(far, near, d)  otherwise  (1 at near edge → 0 at far edge)
```

`near_radius` should comfortably exceed the visible near field so nothing the
player can clearly see is mid-transformation. `far_radius` sits at or beyond the
horizon so transformation happens where it reads as "the distance changing," not
as a pop.

### 7.3 Target, converge, regenerate (per frame, budgeted)

For resident regions, in priority order (nearest-first for loads/regen,
farthest-first for convergence is fine since near is pinned):

1. Recompute `target = project(steer(field.sample(region), anchors, center))`.
2. `converge(rate)`; if it changed, dirty climate+ecology layers.
3. Regenerate up to `budget.max_regen_layers` stale region-layers (§8) into the
   cache, marking `status = Ready`.

Convergence and regeneration are **budgeted** (section 6.6) so a big possibility
change ripples outward over several frames instead of hitching. Jobs are safe to
supersede: if a region's `target` changes again before its layers regenerate, the
next tick simply recomputes from the newer `current`.

### 7.4 Ownership

`RegionMap` owns the `RegionState` map and the `RegionCache`. Field tiles are
owned `Vec<f32>` inside the cache, handed to the renderer by reference each frame
(no duplication). Eviction drops both state and tiles together. No region holds a
pointer into another region; cross-region reads (e.g. field interpolation) go
through the pure `PossibilityField`, not through neighbor state.

---

## 8. Determinism, layers, and cache invalidation

- **Layer stack**: `LAYER_TERRAIN`(0, stable), `LAYER_CLIMATE`(1),
  `LAYER_ECOLOGY`(2). `dirty_layers` in `RegionState` is the bitset; §6.4 narrows
  what gets set.
- **Staleness is a pure comparison.** Each `FieldTile` records the
  `(world_version, revision)` it was generated from. A tile is stale when the
  region's current `(WORLD_ALGORITHM_VERSION, revision)` differs. Terrain tiles
  carry `revision = 0`-equivalent semantics (they depend on position + version
  only, so they are effectively never stale within a run) — which is exactly why
  terrain does not regenerate as possibility drifts.
- **Determinism invariant (unchanged from AGENTS.md).** Any change to
  `terrain`/`climate`/`ecology`/`possibility_field`/`anchor` math that alters
  output for the same inputs requires bumping `WORLD_ALGORITHM_VERSION` **and**
  updating the golden fixtures in the same commit. Phase 1 adds new golden
  samples (§11.1). Do not casually re-bless.
- **Identity vs presentation.** Elevation and field values are `f32`
  presentation state and are **not** required to be bit-identical across native
  and wasm. What must be identical is every **integer** identity
  (`feature_hash`, gradient selection, control-point seeds). Keep the identity
  layer integer so cross-platform agreement is structural, not luck. The existing
  native↔wasm `origin_feature_hash` equality test is extended to cover the new
  integer-seeded pieces (§11.2).

---

## 9. Threading model

- The core (`world-core`, `world-runtime`) stays platform-neutral and expresses
  parallel work through the existing `TaskExecutor` trait (section 16); it never
  spawns a thread itself.
- **Sequence the work in two steps to de-risk determinism:**
  1. **First, single-threaded.** Implement `RegionMap::update` and regeneration
     synchronously. Prove continuity and determinism before adding concurrency.
  2. **Then, offload layer regeneration** through a native `TaskExecutor`
     (Rayon or a small `std::thread` pool in `platform-native`). Field
     generation is pure and per-region, so results must be **independent of
     completion order**; the main thread only integrates finished tiles into the
     cache. This ordering independence is the property a later Web Worker
     executor will also need (section 19).
- No shared mutable region state across jobs; a job takes immutable inputs
  (coord, target vector, config) and returns an owned tile.

---

## 10. Debug visualization (the thing that proves it)

The success criterion is visual, so the visualization is a first-class Phase 1
deliverable, not an afterthought. Build the cheapest thing that makes popping
*obvious*:

- **Primary: top-down false-color map.** A single WGSL pipeline draws the active
  world from above, coloring each cell by a selectable channel: elevation,
  temperature, moisture, vegetation, `stability`, or `revision`. A continuous
  field renders as smooth gradients; a chunk-replacement bug renders as a visible
  seam or a flickering tile — which is precisely what we need to catch. This is
  achievable on top of the current clear-only `Renderer` with one instanced-quad
  or fullscreen-sample pipeline and minimal new GPU surface area.
- **Overlays** (toggle keys): the near/far radius rings, region grid lines, and a
  "changed-while-pinned" highlight that flashes any region whose `revision`
  advanced while `stability == 1.0` (that is a continuity bug by definition — see
  §11.3).
- **Optional secondary: 2.5-D heightfield.** A simple vertex-colored terrain mesh
  for the near window, colored by ecology. Nice for demos but not required to
  answer the question; gate it behind a flag so it never blocks the milestone.

Controls (`platform-native`): WASD/arrows move the player; keys nudge individual
possibility dimensions up/down; a key drops an `Emphasize`/`Suppress` anchor at
the player; a key clears anchors; keys cycle the visualized channel and toggle
overlays.

---

## 11. Testing strategy

### 11.1 Golden determinism fixtures (extend `crates/world-core/tests/determinism.rs`)

- `elevation()` at a few fixed world positions and possibility vectors.
- `climate()` / `vegetation_density()` at fixed inputs.
- `PossibilityField::sample()` at a control point and at an interpolated point.
- `steer()` + `project_plausible()` for one Emphasize and one Suppress anchor.

Each is a known-answer test; changing the math means a deliberate
`WORLD_ALGORITHM_VERSION` bump + fixture update in the same commit.

### 11.2 Native ↔ wasm parity

Extend the existing parity guarantee: the wasm smoke target exposes the new
**integer-seeded** identities (gradient selection sample, control-point seed) and
a test asserts they equal the native values, the same way
`platform_web::origin_feature_hash()` already mirrors native. Float field values
are explicitly *not* asserted bit-equal across platforms (§8).

### 11.3 Continuity regression (headless, in `tools`)

A scripted-camera **continuity replay**: advance a deterministic path over N
frames while nudging possibility and dropping anchors, driving `RegionMap` with a
no-op `TaskExecutor`. Assertions:

- **Pinned stability**: no region with `stability == 1.0` ever bumps its
  `revision` (the core "no near-field pop" guarantee, machine-checked).
- **Bounded per-frame delta**: the max change in any near-field field sample per
  frame is ≤ a small epsilon (no snapping).
- **Determinism**: two runs of the same script produce identical region
  revisions and identical cached-tile hashes.
- **No orphan seams**: adjacent resident regions never differ in target by more
  than the field's per-region gradient bound (interpolation is continuous).

This replay is the automated proxy for the visual success criterion and guards
against regressions once the prototype "looks right."

### 11.4 Unit tests

Streaming window load/evict with hysteresis; stability ramp endpoints and
monotonicity; budget enforcement (never exceed `max_*` per frame); dirty-layer
narrowing (a `current` change dirties climate+ecology but not terrain);
staleness comparison.

### 11.5 CI

Everything runs under the existing CI contract (`fmt --check`, `clippy` with
`-D warnings`, native `check`+`test`, and the `wasm32` check of `world-core`,
`world-runtime`, `platform-web`). New pure crates/modules must stay in the wasm
check set. Add the benchmark (§12) as a `cargo bench`/criterion target that is
built but not gated on timing in CI.

---

## 12. Profiling and metrics (section 6.6, 15)

Instrument from the start; a continuity prototype that hitches fails the
experience test even if it is technically seamless.

- **Per-frame counters** (logged / on-screen): regions loaded, evicted,
  converged, layers regenerated, active region count, field-cache bytes.
- **Timings**: `RegionMap::update` split into stream / target / converge /
  regen; frame time; generation time per layer.
- **Budgets**: assert the per-frame caps hold; log when work is deferred to a
  later frame (backpressure is expected and healthy, not an error).
- **Benchmark harness**: a criterion bench for `elevation()`, `climate()`,
  `vegetation_density()`, and a full `RegionMap::update` tick over a fixed window,
  to catch generation-cost regressions and to size budgets. This seeds the future
  `profiling-and-benchmarking-plan.md`.

Wire lightweight profiling scopes (Puffin/Tracy per section 4.1) behind a feature
flag so they never enter the wasm build.

---

## 13. Native and browser constraints

- `world-core` and `world-runtime` additions **must keep compiling for wasm**
  (CI enforces). No filesystem, threads, sockets, or platform graphics in the
  neutral crates — parallelism only via `TaskExecutor`, persistence only via
  `Storage` (unused in Phase 1).
- Generation jobs are pure, per-region, resumable, and supersede cleanly
  (section 19) — the same properties a Web Worker executor will require later.
- No browser runtime is built in Phase 1; the obligation is only that nothing in
  the design *blocks* it. The `platform-web` smoke target grows just enough to
  cover the new parity test (§11.2).
- Shaders are WGSL only (`debug_map.wgsl`), keeping the renderer WebGPU-portable.

---

## 14. Risks specific to Phase 1 (mapping section 23)

| Risk (section 23) | Phase 1 manifestation | Mitigation in this plan |
|---|---|---|
| 23.1 Continuity | Ground pops / landmarks contradict | Possibility-stable terrain (§6.1); pinned near radius (§7.2); revisioned realized state; the changed-while-pinned detector (§10, §11.3). |
| 23.2 Scope | Building engine instead of validating the idea | 3-layer stack; no organisms/persistence/GPU; cheapest viz; no custom scheduler. |
| 23.3 Dependency explosion | A possibility nudge regenerates everything | Narrowed dirtying (§6.4); terrain never dirtied by drift; temporal budgets (§7.3). |
| 23.4 Platform divergence | Native-only assumptions creep in | Neutral crates stay wasm-clean (CI); `TaskExecutor`/`Storage` traits; WGSL. |
| 23.5 Determinism drift | Native/wasm disagree; accidental algo change | Integer-seeded identities (§8); golden fixtures + wasm parity (§11.1–2); version-bump discipline. |
| 23.6 Memory growth | Field cache grows unbounded | Eviction with hysteresis (§7.1); bounded window; budgeted loads; per-frame cache-byte telemetry (§12). |

---

## 15. Incremental milestones

Each milestone is independently reviewable, keeps CI green, and preserves the
crate-boundary and determinism invariants.

- **M1 — Deterministic terrain (world-core).** `terrain.rs` fBm heightfield +
  golden fixtures + a `wer-inspect` elevation dump. *Exit:* elevation is
  reproducible and version-guarded; wasm parity for gradient seeding.
- **M2 — Possibility field + steering (world-core).** `possibility_field.rs`,
  `anchor.rs` (Emphasize/Suppress), `project_plausible`; golden fixtures. *Exit:*
  a region's target vector is a pure, smooth, deterministic function of position +
  anchors.
- **M3 — Climate + ecology layers (world-core).** `climate.rs`, `ecology.rs`,
  `field.rs`, `layer.rs`; golden fixtures. *Exit:* the 3-layer stack computes
  cheaply and deterministically per region.
- **M4 — Streaming + convergence, single-threaded (world-runtime).** `stream.rs`,
  `generate.rs`, `budget.rs`; narrowed dirtying; unit tests + the headless
  continuity replay (§11.3). *Exit:* the continuity replay passes with no
  changed-while-pinned events — continuity proven *without* graphics.
- **M5 — Debug visualization (renderer + platform-native).** Top-down false-color
  map, overlays, and input; delete `demo_world_tick`. *Exit:* a human can move,
  steer possibility, drop anchors, and **see** smooth transformation with no
  visible chunk replacement.
- **M6 — Parallel regeneration + profiling.** Native `TaskExecutor`
  implementation; order-independent generation; per-frame counters; criterion
  bench. *Exit:* budgets hold, no hitches on large possibility changes,
  generation offloaded off the main thread.
- **M7 — Sign-off.** Run the visual and headless success checks together; record
  the Phase 1 answer (and any new ADRs, e.g. terrain noise choice or the layer
  dirtying policy) so Phase 2 can build the full dependency graph on a validated
  continuity model.

**Phase 1 is done when** M1–M7 are complete, CI is green (including the `wasm32`
check and the new determinism/parity/continuity tests), and both the visual and
headless success criteria hold: the player moves and steers possibility state
with no visible chunk replacement or landmark contradiction, and distant regions
demonstrably transform.
```
