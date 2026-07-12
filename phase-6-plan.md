# Phase 6 — Performance and Scale: Implementation Plan

This is the lower-level plan required by section 21 of
[`implementation-plan.md`](implementation-plan.md) before Phase 6 work begins
(it covers the ground of `job-system-plan.md` and
`profiling-and-benchmarking-plan.md`, the first slices of `gpu-compute-plan.md`
and `renderer-plan.md`, the memory-strategy commitments of section 15, and the
concurrency model of section 16). It expands the Phase 6 scope in section 20 —
data-layout optimization, SIMD kernels, improved region arenas, custom
scheduling if justified, GPU field refinement, resource-tier detection, cache
tuning, large-world stress tests, deterministic replay tests — into concrete
interfaces, algorithms, and milestones, grounded in the landed Phase 2–5 stacks
and the deliberately pre-cut performance seams: the executor-agnostic
`TaskExecutor` trait ("will grow dependency tracking, cancellation handles …
as the job-system plan is written", `task.rs`), the priority parameter the
`RayonExecutor` currently ignores ("priority lanes arrive with the job-system
plan", `platform-native/src/executor.rs`), the bench-calibrated `LayerDecl`
costs ("sized by the criterion benches … rather than taste", `layer.rs`), and
every earlier phase's explicit deferral — "GPU field refinement, SIMD,
clipmaps, LOD tiles (Phase 6)" (phase-2-plan §"non-goals"), "layout tightening
is a Phase 6 concern" (phase-1-plan), "parallelizes … if profiling later
demands it" (phase-3/4-plans).

Read [`AGENTS.md`](AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it assumes
them and calls out where Phase 6 stresses each. One sentence of orientation up
front, because it governs everything below: **Phase 6 changes no generated
output for any input.** Every optimization must be output-identical; the
phase's determinism obligation is to *prove* that, not to re-bless around it.

---

## 1. Goals and non-goals

### 1.1 The question Phase 6 must answer

Phases 1–5 built the complete world model: layered deterministic generation,
procedural ecology, anchor steering, and durable sharing. They also
deliberately deferred every optimization that profiling had not yet justified.
The result is an engine that is *correct everywhere and calibrated nowhere*:
generation runs through a fire-and-forget Rayon shim that ignores priority,
tiles are per-channel heap `Vec`s allocated fresh on every regeneration, the
hot kernels are scalar, the map is composed pixel-by-pixel on the CPU and
re-uploaded whole every frame, `retarget` is an unbudgeted O(resident) pass,
caches evict by distance only with no byte ceiling, the only timing
instrumentation is a whole-`update()` stopwatch in the native shell, and every
budget constant is nominal. Phase 6 asks:

> Can the engine hold **stable frame and generation budgets across target
> native hardware tiers** — and convert the headroom won into **higher world
> density and active simulation scale** — without changing a single generated
> output, re-blessing a single golden fixture, or loosening the continuity,
> invalidation-precision, or persistence guarantees the earlier phases won?
> (section 20, Phase 6)

This is the phase where "delay custom schedulers and allocators until profiling
justifies them" (sections 16, 23.2) comes due: first the profiling, then —
only where the numbers demand it — the machinery. It is also the last
native-only phase, so it is where the browser-portability risks of the
performance work itself (SIMD isolation, GPU-derived-only discipline,
scheduler abstraction) must be locked down before Phase 7 inherits them.

### 1.2 Success criterion (from section 20)

> The engine maintains stable frame and generation budgets across target
> native hardware tiers.

Decomposed into machine-checkable properties (asserted by the scale harness,
§12.4):

- **Stable:** under scripted steady-state travel, drift storms, and cold
  teleports, every per-frame pass stays within its budget, the `deferred_*`
  backpressure counters stay bounded and *drain* when pressure stops (a backlog
  that only grows is a failed budget, not backpressure), and memory telemetry
  plateaus below the configured ceiling — asserted on counts and bytes, which
  are deterministic, with wall-clock reported but not CI-gated (§12.6).
- **Across tiers:** three named resource tiers (Low / Mid / High, §7.4) each
  sustain their configured world density under the same scripts — the tier
  changes *when and how much* work happens per frame, never *what* the world
  is. Same script + settle ⇒ same state hash on every tier, at every
  parallelism, under every budget scale (**schedule independence**, ADR 0018,
  §9.3) — the Phase 6 extension of the deterministic replay.
- **Scaled:** the High tier runs a materially denser world than the Phase 5
  defaults — the concrete targets in §1.5, sized in regions, organisms, and
  resolution rather than milliseconds — while meeting the same stability
  properties. Density increase is *purchased with measured headroom*, not with
  looser budgets.
- **Output-identical:** `WORLD_ALGORITHM_VERSION` stays at 2, every layer's
  `algorithm_revision` stays at 0, `RECORD_FORMAT_VERSION` stays at 1, and
  **zero golden fixtures are re-blessed** (§9.1). SIMD kernels are bit-identical
  to their scalar forms on the same platform (ADR 0016, §9.2), memoizations are
  same-math caches, and GPU refinement is derived presentation that no
  authoritative or persisted value ever reads (ADR 0017, §9.4). The continuity
  replay, ledger, ecology, anchor, and vault harnesses all still pass,
  unmodified.

### 1.3 Goals

- **A profiling foundation before any optimization** (sections 2, 21):
  per-pass timings inside `RegionMap::update` (feature-gated so the neutral
  crates stay wasm-clean, §5.2), pass/pool/executor telemetry in `FrameStats`,
  a panel timing breakdown, frame pacing fixed in the shell (vsync-managed
  present mode instead of the current `ControlFlow::Poll` busy-loop), and a
  **committed baseline document** (`docs/perf-baseline.md`) recording the
  criterion and harness numbers every later milestone is measured against.
  Nothing in M2–M6 lands without a baseline delta.
- **The job system, minimally** (section 16; `job-system-plan.md` ground): a
  native `LaneExecutor` that honors the `TaskPriority` the trait has declared
  since Phase 0 — three FIFO lanes, workers draining Critical > Normal >
  Background — plus **cancellation of superseded jobs** so an evicted or
  re-dirtied region stops costing worker time, not just integration time. The
  `TaskExecutor` trait itself does not change (§5.3); cancellation rides a
  token captured in the job closure. Justified by the M1 measurements
  (priority inversion during cold settle, wasted superseded jobs during drift);
  if M1 shows neither, Rayon stays and the executor work is shelved — the
  "custom scheduling **if justified**" clause, taken literally.
- **Data-layout and allocation work where it measurably pays** (section 15):
  a main-thread **tile pool** recycling sample buffers through the
  dispatch→generate→integrate→evict cycle (steady-state generation stops
  touching the global allocator), organism-vec recycling in the realizer, and
  **byte-capacity cache ceilings** with distance-priority eviction on top of
  the existing radius eviction (cache tuning; the section 23.6 mitigation
  "explicit cache budgets" made real). What we deliberately do *not* do —
  repack `f32` channels into quantized integer tiles, merge per-channel tiles
  into per-region slabs — is listed with reasons in §4.4.
- **SIMD kernels behind portable interfaces** (section 19): vectorize the
  measured hot kernels — terrain fBm (5 octaves × 4 hashed gradients per
  cell), climate, vegetation, the soils/hydrology arithmetic, the integer
  drainage flow pass — across cells (the data is already
  structure-of-arrays), under the **bit-identity rule** (ADR 0016): the scalar
  per-cell operation sequence is unchanged, lanes never interact, no FMA
  contraction, no reassociation, differential-tested against the scalar path
  on every platform. Plus the one *algorithmic* hot-spot fix profiling already
  found: hoisting the cell-invariant roster scans out of the L8 per-cell loop
  (§6.3) — same math, same results, O(cells·roster²) → O(cells).
- **GPU field refinement and GPU map composition** (sections 6.1, 17; the
  first render-graph node): the debug map moves from CPU pixel composition +
  full-texture re-upload to **GPU composition from a region-tile atlas** with
  delta uploads (only changed tiles), and gains optional **refinement
  octaves** — WGSL continuing the terrain/detail spectrum above `FIELD_RES`
  using the same integer-hash gradient scheme, per-pixel, presentation-only.
  The CPU composer is retained as the headless/screenshot/test path and the
  correctness reference. This executes section 17's dual-resolution model (CPU
  authoritative low-res, GPU derived high-res preserving CPU averages) at
  debug-map scale, and establishes the derived-only discipline (no readback,
  ADR 0017) before the real renderer is built on it.
- **Resource-tier detection and scaling** (section 20): detect cores
  (`available_parallelism`), adapter class (wgpu `DeviceType`/limits), and
  optional overrides (`WER_TIER`, `WER_CACHE_MB`) into a `ResourceTier`;
  tiers select `StreamConfig` radii, `Budget` scale, cache ceilings, realize
  and resonance caps, and refinement on/off. Tiers scale pacing and capacity,
  never identity (§9.3).
- **Scale-up: spend the headroom** (the phase's *goal*, not just its
  mechanism): raise the High-tier defaults — §1.5's targets — and make the
  larger world pass the same stability properties.
- **Large-world stress tests and extended deterministic replay** (section 20):
  a **scale harness** (`wer-scale`) — long-haul journeys, teleport storms,
  drift storms, memory-ceiling pressure, schedule-independence equality —
  as the phase's machine-checkable sign-off, alongside the still-passing
  ledger, ecology, anchor, and vault harnesses.

### 1.4 Non-goals (explicitly deferred)

- **The browser runtime** (Phase 7): no Web Workers, no wasm executor, no
  browser storage, no WebGPU-in-browser bring-up. Phase 6's obligation to
  Phase 7 is negative space: SIMD isolated behind portable interfaces with
  scalar fallbacks (wasm builds compile and pass without `simd128`), the
  executor stays behind `TaskExecutor`, GPU work stays derived-only, and the
  wasm CI check keeps passing throughout.
- **GPU-authoritative anything.** No GPU compute writes back into world state,
  identity, persistence, or anything a harness hashes (ADR 0017). GPU ecology
  distribution, erosion, distance fields (section 17's larger candidates) wait
  for the renderer plan; Phase 6 proves the discipline on the map.
- **A real 3D renderer.** Terrain meshes, vegetation instancing, clipmaps, the
  render graph proper (`renderer-plan.md`) are not Phase 6. The renderer
  deliverable is scoped to the debug map: atlas, GPU composition, refinement —
  the *first node* of the eventual graph and the proof of its rules.
- **Custom global allocators, `no_std`, arena-everything.** The tile pool and
  capacity caps are the section 15 work profiling justifies now. Swapping the
  global allocator, region-local bump arenas for transient state, or `no_std`
  world-core are shelved until a future profile demands them.
- **Changing the world model to make it faster.** No layer reordering, no
  resolution change to the authoritative tiles (FIELD_RES stays 32; visual
  density above that is GPU-derived), no quantizing `f32` channels, no
  reduced-precision math, no "approximately equal" anywhere. If an
  optimization cannot be made output-identical, it does not land in Phase 6.
- **Timing-gated CI.** Wall-clock assertions on shared CI runners are flaky by
  construction. CI gates counts, bytes, hashes, and invariants; wall-clock is
  measured locally by `wer-scale --report` against the committed baseline
  (§12.6).
- **The lower-level plans Phase 6 does not consume.** `renderer-plan.md` and
  the full `gpu-compute-plan.md` still need writing before their phases; this
  plan only stakes out the constraints they inherit (ADR 0017).

### 1.5 Concrete scale targets (High tier vs. Phase 5 defaults)

Sized in world quantities, not milliseconds; each is a config default the
scale harness runs at High tier, chosen to stay inside section 15's
"low hundreds of megabytes" authoritative working set:

| Quantity | Phase 5 default | High tier target | Cost driver |
|---|---|---|---|
| Resident window (`load_radius`) | 12 regions (~625 resident) | 17 regions (~1,225 resident) | field cache ≈ 55 KB/region ⇒ ~67 MB; regen throughput |
| `far_radius` (transforming band) | 9 regions | 13 regions | converge + retarget volume |
| `max_realize_organisms` / frame | 400 | 1,600 | realize kernel |
| Realized organism density | ≤ 1 per cell | ≤ 4 per cell (sub-cell slots, §6.6) | realize + resonance + viz |
| `max_resonance_nodes` | 64 | 128 | resonance scan |
| `max_regen_cost` / frame | 96 | 384 | generation kernels (SIMD pays here) |
| Map presentation | 32²/region CPU compose | per-pixel GPU refine | GPU (derived) |

Low tier is the Phase 5 defaults (they are the proven configuration); Mid is
the geometric midpoint. Targets are commitments to *attempt* with measured
headroom and to gate in the harness at whatever the milestones actually won —
if SIMD + pooling + the executor deliver less than 4× regen throughput on the
reference machine, the High-tier numbers shrink to fit and the delta is
recorded in the baseline doc. Budgets never loosen to fake the target.

---

## 2. Where this sits in the subsystem-plan map

| Section 21 plan | Phase 6 coverage |
|---|---|
| `profiling-and-benchmarking-plan.md` | **Core of Phase 6** — pass timings, telemetry, baseline discipline, bench suite growth (§5.2, §12, §13). |
| `job-system-plan.md` | **Core of Phase 6** — priority lanes, cancellation, supersession; the trait's declared contract finally honored (§6.2, §7.2). |
| `gpu-compute-plan.md` | First slice — the dual-resolution model and derived-only discipline, proven on map refinement (§6.5, ADR 0017). |
| `renderer-plan.md` | First slice — tile atlas, delta uploads, GPU composition: the first render-graph node (§6.5). |
| `region-streaming-plan.md` | Cache capacity ceilings, retarget amortization, tier-scaled radii (§6.4, §7.3). |
| Section 15 (memory strategy) | Tile/organism pooling, byte telemetry → byte budgets (§4). |
| `determinism-and-versioning-plan.md` | The optimization-era determinism obligations: bit-identical SIMD, schedule independence, derived-only GPU (§9, ADRs 0016–0018). |

Generation math is **untouched**: no layer changes, no new layer, no hashing
change, no fold-order change, no record change. Phase 6 changes how fast, in
what order, with what memory, and at what visual fidelity the *same* world is
produced — and proves "same" at every step.

---

## 3. Architecture overview

Phase 6 adds no world-model concept. It adds a **measurement layer** (pass
timings, telemetry, baselines), swaps the **execution substrate** under the
existing `TaskExecutor` seam, threads a **buffer pool** through the existing
dispatch→integrate cycle, adds **capacity ceilings** to the existing caches,
vectorizes the existing kernels **without changing their outputs**, and moves
presentation composition onto the GPU **without letting anything flow back**:

```text
            unchanged world model (Phases 1–5)
  ┌────────────────────────────────────────────────────────┐
  │ possibility field → layers L0..L8 → tiles → organisms  │
  │ anchors/steer/project · routes · preserves · vault     │
  └────────────────────────────────────────────────────────┘
        │ same outputs, bit-for-bit (§9.1)
        ▼
  Phase 6 substrate work
  ──────────────────────────────────────────────────────────
   measure   FrameStats + pass timings + baseline doc (M1)
   schedule  LaneExecutor: 3 priority lanes, cancel tokens (M2)
   allocate  TilePool / organism recycling; byte ceilings  (M3)
   vectorize SIMD kernels ≡ scalar (differential-tested)   (M4)
   present   tile atlas → GPU compose → refinement octaves (M5)
                 │  derived only — nothing flows back (ADR 0017)
   scale     ResourceTier → config presets → density up    (M6)
        │
        ▼
  Phase 6 proof obligations
  ──────────────────────────────────────────────────────────
   zero re-blessed fixtures · SIMD ≡ scalar (ADR 0016)
   settled state hash invariant across executor/budget/tier
   (ADR 0018) · all Phase 2–5 harnesses green, unmodified
```

Four commitments organize everything:

1. **Measure, then move.** Every optimization milestone starts from an M1
   baseline number and ends with a measured delta in `docs/perf-baseline.md`.
   An optimization without a delta is reverted — the codebase does not
   accumulate speculative cleverness (sections 16, 23.2 said "until profiling
   justifies"; the ledger of justification is a committed artifact).

2. **Output identity is the contract; schedules are free.** The world is a
   pure function of its inputs; *when* work happens (parallelism, priority,
   budget, tier, frame pacing) is an implementation detail that may change
   freely — and therefore must be *proven* not to leak into outputs. Phase 6
   turns the informal "integration is order-independent" property into a
   machine-checked equality (ADR 0018) and then exploits it aggressively.

3. **Fast paths are twins, not forks.** Every SIMD kernel keeps its scalar
   twin (the wasm/fallback path *and* the differential oracle); the GPU
   composer keeps the CPU composer (the headless/test path *and* the visual
   reference); the LaneExecutor keeps `InlineExecutor` (the harness
   substrate). The slow path is never deleted, because it is the spec.

4. **Presentation may exceed the authoritative resolution; it may never feed
   back.** GPU refinement reads tiles and writes pixels. No readback, no
   hashing of GPU output, no gameplay or persistence consumer (ADR 0017) —
   the section 6.1 rule ("authoritative state must not depend on … synchronous
   GPU readback") upgraded from "not exclusively" to "not at all" for
   everything Phase 6 builds.

---

## 4. Data layout

### 4.1 What exists (and stays)

Tiles are `FieldTile<T> { resolution, dep_hash, samples: Vec<T> }` — flat
row-major, one buffer per channel per region, 13 `f32` channels + biome `u8` +
dominant `u16` at `FIELD_RES = 32` (≈ 55 KB/region), shared as
`Arc<FieldTile<T>>`, immutable once integrated. This is already
structure-of-arrays, already SIMD-shaped, already cache-resident per region
(each channel is 4 KB). The `BTreeMap` region/cache indexes stay: deterministic
iteration order is a correctness convention (§9), and ~600–1,300 resident
regions is far below the scale where the index matters. **Phase 6 does not
change any persisted or hashed byte.**

### 4.2 Tile pool (kill steady-state allocation churn)

Every regenerated layer currently allocates fresh `Vec`s on a worker thread
and drops the superseded tile on the main thread — at drift-storm rates,
thousands of 4 KB allocations per second through the global allocator. Phase 6
adds a main-thread pool, threaded through the existing single-writer cycle:

```rust
// world-runtime/src/pool.rs — NEW. Allocation-only, wasm-clean, main-thread.
/// Recycles tile sample buffers through dispatch→generate→integrate→evict.
/// Buffers are handed *into* job closures at dispatch (the main thread is the
/// only pool toucher; workers just fill what they were given) and reclaimed
/// when a superseded/evicted tile's Arc refcount proves sole ownership.
#[derive(Debug, Default)]
pub struct TilePool {
    f32_bufs: Vec<Vec<f32>>,   // capacity == resolution²; cleared, not zeroed
    u8_bufs: Vec<Vec<u8>>,
    u16_bufs: Vec<Vec<u16>>,
    // stats: hits, misses, reclaimed, resident_bytes (→ FrameStats)
}
```

- **Dispatch:** `dispatch_region` pops a buffer per output channel and moves
  it into the job closure; the job fills it (same fill code — `FieldTile::new`
  grows a provided buffer instead of allocating when one is supplied).
- **Reclaim:** on eviction and on superseded-tile replacement, `Arc::try_unwrap`
  recovers the `Vec` when the map holds the last reference (in-flight readers
  — viz sampling, realize snapshots — just delay reclaim by a frame; the pool
  falls back to allocation, never blocks).
- **Bound:** the pool itself is capacity-capped (it exists to serve churn, not
  to hoard); its bytes report into `FrameStats`.

Same story, smaller, for the realizer's per-region `Vec<Organism>` (recycled
through the existing rebuild-on-L8-change path) and the composer's scratch
rows. **Exit measurement:** steady-state drift frames perform ~zero global
allocations in the update path (measured with a counting allocator in the
bench harness, not shipped).

### 4.3 Cache ceilings (cache tuning)

Eviction today is purely distance-based (`unload_radius`); nothing caps total
bytes if the window is configured large. Phase 6 adds explicit ceilings —
section 23.6's "explicit cache budgets", sized per tier:

```rust
// stream.rs — StreamConfig additions
pub max_field_cache_bytes: usize,   // RegionCache ceiling (tiles)
pub max_macro_cache_bytes: usize,   // DrainageTile ceiling
pub max_roster_cache_bytes: usize,  // roster/web snapshot ceiling
```

When a ceiling is exceeded after the radius sweep, the evictor removes
farthest-first (deterministic: distance, then coord order) until under
ceiling; pinned/preserved regions and anything inside `near_radius` are
exempt (correctness: eviction is always safe — every tile re-derives from its
dep hash, ADR 0008 — so a ceiling can only cost recompute, never correctness;
the harness asserts a return trip under a tight ceiling reproduces identical
content hashes, §12.4). `FrameStats` gains `evicted_for_capacity`.

### 4.4 Deliberately rejected layout changes

Recorded so the next reader doesn't re-litigate them:

- **Quantized integer field tiles** (section 15 "packed integer fields"):
  storing `u16` samples would change consumer-visible values ⇒ an algorithm
  change ⇒ fixture re-blesses. Memory pressure is handled by ceilings +
  recompute instead. Revisit only alongside a deliberate, versioned algorithm
  revision.
- **Per-region channel slabs** (one allocation holding all 13 channels):
  conflicts with per-layer regeneration granularity — replacing one layer's
  channels would force copying the others or aliasing games. Per-channel
  `Arc` tiles keep regen granularity and job-result transfer simple; locality
  is already adequate (whole region ≈ 55 KB).
- **Replacing `BTreeMap` with hashed/slotted indexes:** iteration-order
  determinism is load-bearing across the codebase and the maps are small;
  no measured win justifies the risk.

---

## 5. Public interfaces

### 5.1 `world-core` additions

```text
world-core/src/
    simd.rs      # NEW: portable SIMD kernels + scalar twins (§6.1):
                 #   fbm_row, climate_row, vegetation_row, soils_row,
                 #   hydrology_row, flow_direction_row (integer)
                 #   — each `fn xxx_row(inputs…, out: &mut [T])`, dispatching
                 #   wide-vector or scalar; signatures take rows, not cells
    terrain.rs   # elevation loops route through simd::fbm_row (output-identical)
    …            # other kernel files gain row entry points; per-cell fns remain
```

`wide` joins `[workspace.dependencies]` (stable-Rust portable SIMD: SSE/AVX,
NEON, wasm `simd128` when enabled, scalar otherwise — no nightly `std::simd`,
per the pinned toolchain). All `simd.rs` code is `#[must_use]`, wasm-clean,
and carries the ADR 0016 contract in its module docs. No public world-core API
changes otherwise; `population.rs`/`foodweb.rs` hoist cell-invariant tables
into `RosterSnapshot` (§6.3) without signature changes visible outside the
crate.

### 5.2 `world-runtime` changes

```text
world-runtime/src/
    pool.rs      # NEW: TilePool (§4.2)
    timing.rs    # NEW: PassTimings + Pass enum; feature "pass-timing"
                 #   (std::time::Instant), enabled by platform-native only —
                 #   wasm builds compile without it, fields report zeros
    stream.rs    # per-pass timing hooks; capacity evictor; retarget
                 #   amortization (max_retarget_regions, §7.3); cancel tokens
                 #   in in_flight; pool threading through dispatch/integrate
    budget.rs    # + max_retarget_regions; Budget::for_tier(ResourceTier);
                 #   per_frame() unchanged (Low tier == today's defaults)
    tier.rs      # NEW: ResourceTier { Low, Mid, High } + TierInputs
                 #   (cores, adapter class, overrides) → detect() is pure;
                 #   the *inputs* are gathered by platform crates (§5.3)
    task.rs      # trait UNCHANGED; docs updated: priority is now honored
                 #   by the native executor; cancellation rides job closures
```

`FrameStats` grows: `pass_ms: [f32; PASS_COUNT]` (integrate, evict, load,
retarget, converge, dispatch, realize, flush — zeros without the feature),
`jobs_cancelled`, `pool_hits`, `pool_misses`, `pool_bytes`,
`evicted_for_capacity`, `retarget_deferred`. `RegionMap::update`'s signature is
**unchanged** — tiering happens in the config/budget the caller already
passes; the map never learns what a tier is.

### 5.3 `platform-native`, `renderer`, `tools`, `platform-web`

- **`platform-native`:** `LaneExecutor` (§6.2) replacing `RayonExecutor`
  behind the same trait (`rayon` leaves the workspace if M2's justification
  holds; the bin keeps a `--inline` flag for A/B). Tier detection inputs
  (`available_parallelism`, wgpu adapter type/limits, `WER_TIER` /
  `WER_CACHE_MB` overrides) feeding `ResourceTier::detect`. Present-mode
  management (FIFO/vsync default, `WER_PRESENT_MODE` override) replacing the
  busy-loop as the frame pacer. Panel: per-pass timing block, executor line
  (lanes queued / in flight / cancelled), pool line, tier line. Keys: toggle
  CPU/GPU compose; toggle refinement.
- **`renderer`:** the tile atlas + GPU map composition + refinement pipeline
  (§6.5): `FieldAtlas` (array texture, one layer set per resident region,
  delta uploads keyed by tile dep-hash change), `compose_map.wgsl`
  (false-color + channel select + refinement octaves + overlay blend),
  `render_map` gains the GPU path while `render_map_cpu` (the current path)
  remains for headless/screenshot/tests.
- **`tools`:** the **scale harness** (`wer-scale`: lib module + thin bin, the
  established pattern) — §12.4's scenario families; `--report` prints the
  timing/counter table that `docs/perf-baseline.md` snapshots.
  `wer-inspect` unchanged.
- **`platform-web`:** **no new exports.** Phase 6 adds no new portable
  vocabulary — the parity surface is already exactly the integer/steering/
  codec set Phases 2–5 pinned, and none of it changes. The wasm CI check now
  additionally proves the SIMD module's scalar fallback and the pool compile
  wasm-clean.

---

## 6. Algorithms

### 6.1 SIMD kernels (bit-identical vectorization)

The M1 kernel benches rank the hot loops; the vectorization order follows the
ranking, expected (from the Phase 2–5 bench structure) to be:

1. **Terrain fBm** — 5 octaves × 4 corner gradients per cell, each gradient 3
   integer `mix` folds; the top kernel and also drainage's elevation-fill cost
   (2,500 fBm samples per macro tile). Vectorize across cells in a row:
   `u64` lanes for `splitmix64`/`mix` (wrapping mul/xor/shift vectorize
   exactly), `f32` lanes for fade/lerp/accumulate. Per-lane operation sequence
   identical to scalar; octave loop stays an ordered scalar-order accumulation
   per lane (no cross-lane reduction exists in the kernel — cells are
   independent).
2. **Climate / vegetation / soils arithmetic** — pure per-cell arithmetic,
   trivial row vectorization.
3. **Hydrology** — slope taps and bilinear accum vectorize; `ln`/`sqrt` in
   `river_intensity` stay *scalar per lane* (extract, call the same `f32::ln`,
   reinsert) — slower than a vector-math approximation, but bit-identical,
   which is the rule; the surrounding arithmetic still pays.
4. **Drainage flow pass** — 8-neighbor integer steepest-descent over 48²;
   integer SIMD compare/select. The sort+accumulate pass stays scalar
   (sequential dependency).
5. **Biome classifier** — branchy threshold cascade; convert to branchless
   integer selects only if the differential test proves bit-identity for every
   input in a saturation sweep; otherwise leave scalar (it is cost 1).

The rules (ADR 0016, enforced by review + tests):

- A SIMD kernel is a **lane-parallel transcription** of the scalar kernel:
  same operations, same order, per cell. No FMA (`wide` doesn't contract; we
  also never call `mul_add`), no reassociation, no fast-math, no cross-lane
  ops in `f32` paths.
- Every kernel keeps its scalar twin, used for: wasm-without-simd128 builds,
  the tail cells of a row, and the **differential test** — randomized inputs
  (seeded), full-range sweeps, `assert_eq!` on output *bit patterns*, run in
  native CI (§12.2).
- Golden fixtures are the second oracle: `elevation_golden`,
  `climate_golden`, `drainage_routing_golden` et al. pass unchanged, by
  definition of the rule.

Dispatch is compile-time-portable: `simd.rs` picks the widest `wide` type the
target offers; there is no runtime CPU dispatch in Phase 6 (the pinned
baseline x86-64 feature set + NEON + simd128 cover the targets; runtime
dispatch is complexity deferred until a measured need).

### 6.2 LaneExecutor: priority lanes and cancellation (the job system slice)

**Measured justification first (M1):** two numbers from the stress scripts —
(a) *priority inversion*: during a cold teleport settle, the latency between a
Critical (near-window) job's submission and start while Background jobs
occupy workers; (b) *wasted work*: jobs whose results are dropped as
superseded (`in_flight` mismatch) or that belong to evicted regions, as a
fraction of jobs run during a drift storm. If (a) is negligible and (b) is
small, Rayon stays. Expectation from the architecture (FIFO global queue, no
cancellation): both are material during exactly the scenarios the success
criterion names.

**The executor** (`platform-native/src/executor.rs`, ~150 lines, std only):

- N = `available_parallelism() - 1` workers (main thread keeps a core), three
  `Mutex<VecDeque<Job>>` lanes + condvar. Workers drain Critical, then
  Normal, then Background. `parallelism()` reports N.
- **Cancellation without a trait change:** `RegionMap` already keys
  `in_flight` by `(coord, layer) → job_id`. It now also holds
  `Arc<AtomicBool>` per in-flight job; the token is captured by the job
  closure, which checks it *once, on dequeue* — a superseded or evicted job
  becomes a no-op before doing kernel work. Supersession/eviction paths flip
  the token (they already update `in_flight`). Late results remain handled by
  the existing job-id drop — the token is an optimization, the id check stays
  the correctness gate (belt stays on when the suspenders are new).
- Shutdown: drop = poison + join (bounded, jobs are short).

Determinism: none required of the executor beyond what Phase 0 established —
jobs are pure, results integrate keyed by dep hash, order never matters. That
claim graduates from convention to machine-checked property in §9.3.

### 6.3 The L8 hoist (the one algorithmic fix)

`generate_layer`'s ecology arm currently calls `population` per cell, which
re-derives `species_biomass` per roster member (itself re-scanning the roster:
O(roster²)) and the diversity entropy — per cell, though both are functions of
(roster, web) only, not of the cell. Phase 6 hoists them into the
signature-keyed `RosterSnapshot` built once per (signature, revision):
a precomputed biomass table and diversity scalar; the per-cell loop drops to
table lookups + the genuinely per-cell dominant/pressure math. **Same
arithmetic, same order, same `f32` results** — a memoization, not an
approximation; `food_web_golden`, the ecology harness, and L8 content hashes
are the proof. This is the pattern for any future "algorithmic" optimization
in Phase 6: only same-math caching qualifies.

### 6.4 Retarget amortization

`retarget` recomputes stability + steered target for **every** resident region
every frame — unbudgeted (a noted seam), and 2× the volume at High tier. Phase
6 rounds it robin: `max_retarget_regions` per frame (default sized so the full
window refreshes in ≤ 4 frames; Low tier default keeps today's
every-frame behavior), always including regions whose covering control points
or anchor set changed this frame (dirty-first, then round-robin by coord
order — deterministic). Convergence pacing shifts by at most the refresh
period; settled fixed points are unchanged (the harness asserts settled-hash
equality across amortization settings, §9.3 — this is exactly the kind of
freedom ADR 0018 licenses).

### 6.5 GPU map composition and field refinement

Today: `MapComposer` paints side² pixels on the CPU (iterating every resident
region, every frame), `Hud` blits, and the whole RGBA image re-uploads via
`write_texture` every frame. Phase 6:

- **`FieldAtlas`** (renderer): array textures holding per-region channel
  tiles (13×`r32float` planes packed as 4×`rgba32float` layers + 1
  `rg16uint` for biome/dominant), one layer-slot per resident region, slots
  assigned by the shell, **uploaded only when a tile's dep hash changes**
  (the shell already sees revision bumps; steady-state upload traffic → ~zero).
- **`compose_map.wgsl`**: fullscreen pass mapping screen → world → region →
  atlas slot; false-color per channel (transcribing `viz.rs`'s palettes);
  bilinear or nearest per channel to match the CPU look; **refinement
  octaves** (toggleable): above `FIELD_RES`, continue the gradient-noise
  spectrum per pixel — same integer-hash gradient construction transcribed to
  WGSL (u32-pair arithmetic for the 64-bit mix), amplitude-matched to the
  authoritative octaves, modulating elevation/vegetation display only. The
  refined pixel is *derived presentation*: it is never read back, hashed,
  sampled for gameplay, or persisted (ADR 0017) — and the aggregate constraint
  of section 17 holds by construction (refinement adds zero-mean detail around
  the authoritative sample).
- **Overlays** (routes, preserves, rings, organisms, grid, player) stay
  CPU-drawn — they are sparse vector-ish content — into a small transparent
  RGBA overlay texture, blended in the same pass. The HUD panel stays the
  CPU strip it is.
- **CPU path retained**: `--screenshot`, headless tests, and the `M`-key A/B
  toggle keep the CPU composer authoritative-for-tests (CI has no GPU; the
  GPU path is exercised locally and by the visual A/B, §12.5).

This removes the largest single CPU consumer outside `update()` (composition
scales with window pixels × resident regions) and establishes atlas + delta
upload + derived-refinement — the exact substrate the future terrain renderer
node needs.

### 6.6 Sub-cell organism realization (density lever)

`realize_region` currently instantiates ≤ 1 organism per cell (probability =
vegetation density; identity = `feature_hash` over the cell's feature index).
The High-tier density target adds sub-cell slots: `organisms_per_cell ∈
{1..4}` (config), slot `s` using feature index `cell*4 + s` — new *additive*
identities from the same identity scheme (feature indices are just integers;
no existing identity changes), each slot independently density-gated so
expected population scales linearly and the aggregate↔entity consistency the
ecology harness asserts is preserved (its assertions are ratios, not counts;
it runs unchanged at `organisms_per_cell = 1`, and a new scenario runs it at
4). Realized organisms remain Tier-B presentation state (ADR 0010), so this
changes no persisted or shared byte — but it does change the *default realized
world* at High tier, so: default stays 1 in `StreamConfig::default()` (all
existing harnesses/replays run unmodified), and tiers opt in explicitly.

### 6.7 Resource-tier detection

```rust
// world-runtime/src/tier.rs — pure decision from platform-gathered inputs
pub enum ResourceTier { Low, Mid, High }
pub struct TierInputs { pub cores: usize, pub adapter: AdapterClass,
                        pub override_tier: Option<ResourceTier> }
impl ResourceTier {
    #[must_use] pub fn detect(i: &TierInputs) -> Self;  // documented table
    #[must_use] pub fn stream_config(self) -> StreamConfig;
    #[must_use] pub fn budget(self) -> Budget;
}
```

Detection is a small documented table (≤ 4 cores or cpu-type adapter → Low;
≥ 8 cores and discrete adapter → High; else Mid), overridable (`WER_TIER`),
logged at startup, shown in the panel. The tier premise the harness enforces:
**tiers select pacing and capacity presets; identity is tier-invariant**
(§9.3). Native-shell only for now; Phase 7 will feed browser inputs (worker
count, memory hints) into the same pure decision.

---

## 7. Scheduling and budgets

### 7.1 The frame, unchanged in shape

The update pipeline keeps its exact step order (integrate, evict, load,
retarget, converge, dispatch, integrate, realize, flush) — Phase 6 changes the
cost of steps, not their order or semantics. Pass timings wrap each step;
the pool hands buffers to dispatch and reclaims in evict/integrate; the
capacity evictor runs inside evict; amortized retarget replaces full retarget.
Everything stays main-thread single-writer except the kernels the executor
already ran on workers.

### 7.2 Budget model

Budgets stay count/cost-based (deterministic, schedule-independent — a ms
budget would make *outputs* depend on machine speed, which ADR 0018 forbids).
Phase 6: recalibrates `LayerDecl.cost` against the post-SIMD benches (declared
costs are relative weights; drainage's 10 is re-measured — costs may change
because they are scheduling metadata, not identity inputs — they fold into no
hash); adds `max_retarget_regions`; sizes `Budget::for_tier` presets from the
measured per-unit costs so that a full budget spends ≈ the frame slice the
tier targets (the ms→units conversion lives in the baseline doc, re-derivable
by `wer-scale --report`).

### 7.3 Backpressure discipline

The `deferred_*` counters are the stability signal: the scale harness asserts
that under sustained pressure they oscillate bounded (budget saturation is
healthy) and that when pressure stops they drain to zero within a stated
frame count (backlog clears). A backlog that grows monotonically under a
tier's own preset scenario is a sign-off failure — the tier's density target
shrinks until it doesn't (§1.5's honesty clause).

### 7.4 Tier presets

| Knob | Low (= Phase 5 defaults) | Mid | High |
|---|---|---|---|
| `load/unload_radius` | 12 / 14 | 14 / 16 | 17 / 19 |
| `far_radius` | 9 | 11 | 13 |
| `max_regen_cost` | 96 | 192 | 384 |
| `max_realize_organisms` | 400 | 800 | 1,600 |
| `organisms_per_cell` | 1 | 2 | 4 |
| `max_resonance_nodes` | 64 | 96 | 128 |
| field cache ceiling | 48 MB | 96 MB | 160 MB |
| GPU refinement | off | on | on |

(Values are the plan's starting points; M6 fixes them from measurement and
records the final table in the baseline doc and AGENTS.md.)

---

## 8. Threading model

Unchanged in principle, upgraded in substance: the main thread remains the
only writer of all world state; workers run pure kernels and post owned
results; the channel + job-id integration is untouched. New: the LaneExecutor
owns its workers (std threads, platform-native only — the neutral crates
still spawn nothing); cancellation tokens are `Arc<AtomicBool>` (the only
cross-thread mutation, and it is advisory); the pool is main-thread-only by
construction (buffers move into closures, they don't share). Everything still
lands and passes under `InlineExecutor` first — now not merely by convention
but as one leg of the schedule-independence equality (§9.3).

---

## 9. Determinism and versioning

### 9.1 No version bumps, no re-blesses — the phase invariant

Phase 6 changes **no generated output for any input**: no layer math, no
hashing, no fold order, no record bytes, no steering math. Therefore
`WORLD_ALGORITHM_VERSION` stays **2**, every `algorithm_revision` stays 0,
`RECORD_FORMAT_VERSION` stays **1**, and every golden fixture in
`determinism.rs`, every parity export golden, and every harness expectation
passes **unmodified**. A re-bless appearing in a Phase 6 diff is a determinism
bug by definition (AGENTS.md), full stop. This is the sharpest tool the phase
has: the entire Phase 2–5 fixture corpus becomes the regression net for the
optimization work.

### 9.2 SIMD bit-identity (ADR 0016)

Same-platform, same-input, SIMD-vs-scalar outputs are **bit-equal** — not
epsilon-close. This is achievable because the hot kernels are per-cell
independent maps (no cross-cell reductions), `wide` performs exact IEEE
lane ops, and the rules in §6.1 ban the three ways vectorization changes
results (contraction, reassociation, approximation). Cross-*platform* `f32`
remains per-platform presentation exactly as Phases 2–5 left it — SIMD
changes nothing about that boundary, and the integer parity surfaces
(hashes, drainage topology, genome/food-web fingerprints, codec bytes,
steering samples) remain the cross-platform contract, untouched.

### 9.3 Schedule independence (ADR 0018)

The new machine-checked property: for a fixed script and config, the
**settled** world state hash (`replay.rs`'s `state_hash`) is invariant
across — executor choice (Inline vs LaneExecutor at any worker count), budget
scale (¼× / 1× / 4×), retarget amortization setting, frame-slicing of the
script, and cancellation on/off. Mid-flight hashes may differ (different work
completed); *settled* hashes may not. Tier presets that change world content
knobs (`organisms_per_cell`, radii) are compared like-for-like: identity
invariance is asserted per preset, and separately the *shared/persisted*
surfaces (record bytes, steering from records, quantized buckets, dep hashes)
are asserted tier-invariant outright. This turns "integration is
order-independent" and "budgets change pacing, not outcomes" — assumptions the
codebase has leaned on since Phase 1 — into gates.

### 9.4 GPU output is derived-only (ADR 0017)

No value computed on the GPU is read back into authoritative state, hashed,
persisted, or consumed by gameplay/steering/persistence code — enforced
structurally (the renderer exposes no readback API to the shell; world-core
and world-runtime have no GPU types) and by review. Refinement must preserve
the authoritative sample as its mean (zero-mean detail), so CPU and GPU
presentations agree at tile resolution — checked visually via the A/B toggle,
not by a hash (it is presentation).

### 9.5 New ADRs

- **ADR 0016 — SIMD kernels are lane-wise bit-identical to their scalar
  twins.** Vectorization is transcription: same per-cell operation sequence,
  no contraction/reassociation/approximation, scalar twin retained as spec,
  fallback, and differential oracle; differential bit-equality tests are CI
  gates. An optimization that cannot meet bit-identity is an algorithm change
  and must go through `algorithm_revision` + fixtures — in a later phase, not
  this one.
- **ADR 0017 — GPU compute is derived presentation; authoritative state never
  reads it back.** The dual-resolution model (CPU authoritative low-res, GPU
  derived high-res preserving CPU-level means) with the feedback edge cut
  entirely: no readback, no hashing, no persistence, no gameplay consumer.
  One-way door for every future GPU workload until a successor ADR carves a
  proven-portable exception.
- **ADR 0018 — Settled world state is schedule-independent, and budgets/tiers
  scale pacing and capacity, never identity.** Executor parallelism, priority
  order, cancellation, budget scale, amortization, and resource tier are free
  implementation dimensions; equality of settled state hashes across them is
  a harness gate. Anything that wants to break this (e.g. wall-clock-adaptive
  budgets that alter outcomes) is out until a successor ADR.

---

## 10. Debug visualization and tools

- **Panel:** a TIMINGS block (per-pass ms from `pass_ms`, compose/upload ms
  from the shell, frame ms); an EXEC block (workers, queued per lane, in
  flight, cancelled); a POOL line (hits/misses/bytes); a TIER line (detected
  tier, active preset, cache ceiling headroom); `evicted_for_capacity` joins
  the cache line.
- **Overlays:** unchanged set, plus refinement toggle and CPU/GPU compose A/B
  key (the parity eyeball); the pinned-violation detector keeps running under
  the GPU path (it reads world state, not pixels — unaffected by ADR 0017).
- **`wer-scale`** (bin + lib, the harness pattern): runs the §12.4 scenario
  families headless; `--report` prints the baseline table (per-pass ms,
  bench-derived unit costs, counters) for `docs/perf-baseline.md`;
  `--strict` additionally gates local wall-clock against the committed
  baseline ±tolerance (developer tool, not CI).
- **`docs/perf-baseline.md`** (new, committed): machine-labeled baseline and
  per-milestone deltas — the "profiling justifies" ledger.

---

## 11. Testing strategy

### 11.1 Existing fixtures and harnesses: the regression net

`cargo test --workspace` already runs the determinism goldens, parity
goldens, continuity replay, ledger, ecology, anchor, and vault harnesses.
Phase 6's first testing commitment is that **all of them pass unmodified at
every milestone** — they are the output-identity oracle. The replay
additionally runs under the LaneExecutor in the native test lane (it was
Inline-only by circumstance, not by design).

### 11.2 New unit and differential tests

- **SIMD differential** (per kernel): seeded randomized inputs + edge sweeps
  (denormals, exact bucket boundaries, tie cases in integer kernels) ⇒
  bit-equality of SIMD and scalar outputs; runs native CI on x86-64, and on
  any other native target that builds (NEON via local/ARM CI if added).
- **Pool:** reclaim-on-sole-ownership, fallback-on-shared, capacity bound,
  buffer reuse produces identical tiles (trivially — same fill code — but the
  test pins the plumbing).
- **Capacity evictor:** deterministic farthest-first order; pinned/near
  exemption; return-trip content-hash equality under a tight ceiling.
- **LaneExecutor:** priority draining order, cancellation no-ops, shutdown
  joins; a stress test hammering submit/cancel from the map's real call
  pattern.
- **Retarget amortization:** dirty-first inclusion; full-window refresh bound;
  settled-hash equality vs. unamortized (also covered by §11.4).
- **L8 hoist:** L8 tile content hashes equal pre/post hoist on a scripted
  window (a one-commit assertion that can then be deleted — the goldens carry
  it after that).
- **Tier detection:** the decision table, overrides, logging.

### 11.3 Continuity replay (extended, must stay green)

Unchanged script and assertions, run in three additional configurations:
LaneExecutor (max parallelism), quartered budgets, and High-tier stream
config with `organisms_per_cell = 1` (radii/budget scaling must not perturb
continuity bounds). The two-run bit-identical `state_hash` assertion now also
runs Inline-vs-LaneExecutor (§9.3's strongest form: not just two runs of the
same schedule, but two schedules).

### 11.4 Scale harness (`wer-scale`) — the Phase 6 success criterion

Scenario families (headless, deterministic scripts; wall-clock reported, never
CI-gated; counts/bytes/hashes gated):

**Schedule independence (ADR 0018):**

| Scenario | Gate |
|---|---|
| Same script: Inline vs Lane(2) vs Lane(8) | settled state hashes equal |
| Budget ¼× / 1× / 4×; amortization on/off; cancellation on/off | settled state hashes equal |
| Per tier preset: two runs, both executors | settled state hashes equal; shared/persisted surfaces identical across tiers |

**Stability (per tier preset):**

| Scenario | Gate |
|---|---|
| Long-haul: 50k+ units travel with bias storms, anchors, routes, saves | `deferred_*` bounded and oscillating; caches plateau ≤ ceiling; zero pinned violations; vault flush within `max_persist_ops` |
| Teleport storm: repeated far teleports | window settles within a stated frame bound at the tier's budget; cancellation measurably reduces jobs-run vs. cancellation-off (gate on the counter, not time) |
| Pressure release | after each storm, backlog drains to zero within a stated frame count |

**Memory (section 23.6):**

| Scenario | Gate |
|---|---|
| Tight ceiling round-trip | capacity eviction fires; revisited regions reproduce identical content hashes; `pool_bytes` bounded |
| Steady-state allocation | counting-allocator bench shows ~zero update-path allocations during steady drift (bench-side, not shipped code) |

**Density (the scale-up proof):**

| Scenario | Gate |
|---|---|
| High-tier long-haul | all stability gates at High preset — the success criterion sentence, executed |
| Ecology at `organisms_per_cell = 4` | ecology-harness coherence scenarios re-run at density 4 (ratios hold; realize cap respected) |

### 11.5 GPU path

CI has no GPU; the GPU composer is exercised by local runs, the A/B toggle,
and the screenshot path staying CPU (so image-based checks remain
deterministic). Structural tests that do run in CI: atlas slot assignment /
delta-upload bookkeeping (pure), WGSL passes `naga` validation at build
(wgpu's pipeline creation in a headless adapter-less unit test where
possible), and the ADR 0017 structural rule (no renderer readback API —
enforced by the crate's public surface, which review guards).

### 11.6 CI

The existing contract, unchanged in shape: fmt, clippy `-D warnings`, native
check+test (now including SIMD differentials, executor/pool/evictor units,
scale-harness invariance + bounds scenarios sized to CI), wasm32 check of the
neutral crates + `platform-web` (now proving `wide`-scalar-fallback and pool
code wasm-clean). Benches still build under clippy `--all-targets` and remain
timing-ungated. No new jobs.

---

## 12. Profiling and metrics

- **In-runtime:** `pass_ms` per update step (feature `pass-timing`);
  pool, executor, capacity-eviction, retarget-deferral counters in
  `FrameStats` — always present, cheap, deterministic.
- **Shell:** compose ms, atlas-upload bytes/frame, present-wait vs. work time
  (separating vsync idle from real cost — impossible under today's busy-loop,
  which M1 fixes), 1-second rollups in the panel as today.
- **Benches (criterion, extended):** per-kernel scalar vs. SIMD pairs in
  `world-core/benches/generation.rs` (the ratio is the SIMD ledger entry);
  `population_sample` pre/post hoist; pool-cycle micro-bench;
  `region_map_update_*` re-run per milestone (the headline numbers);
  High-tier `window_settle` joins `world-runtime/benches/update.rs`.
- **Baseline discipline:** `docs/perf-baseline.md` records machine spec,
  toolchain, and the numbers at M1, then a delta table per milestone —
  the artifact that makes "profiling justifies" auditable. `wer-scale
  --report` regenerates the raw table so the doc is cheap to keep honest.

---

## 13. Native and browser constraints

Restating where Phase 6 stresses standing obligations: the neutral crates
still spawn no threads (LaneExecutor is platform-native; the pool is passive
allocation reuse), still touch no filesystem or GPU, and still compile
wasm-clean — `wide` falls back to scalar without `simd128`, `pass-timing`
stays off for wasm builds, and `tier.rs` is a pure decision over
platform-gathered inputs. SIMD is isolated behind portable row interfaces
with the scalar twin as the permanent fallback (section 19's "SIMD-specialized
code must be isolated behind portable interfaces … platform-specific
acceleration must have portable fallbacks"). GPU work is WGSL-only,
WebGPU-shaped, and derived-only, so the browser renderer inherits it
unchanged. Generation jobs remain small, pure, and cancellable — exactly the
shape Web Workers need — and the LaneExecutor's contract (priority honored,
fire-and-forget, owned results) is deliberately the contract a Phase 7 worker
pool will implement behind the same trait. No large monolithic allocations
appear (the atlas is the largest new allocation and is a renderer-side,
tier-sized texture; the pool caps itself). Budgets remain count/cost-based
and deterministic, so browser frame variability cannot leak into outcomes.

---

## 14. Risks (mapping section 23)

| Risk | Phase 6 manifestation | Mitigation |
|---|---|---|
| 23.5 Determinism drift | The classic optimization failure: SIMD/reordering/caching quietly changes outputs; parallel timing leaks into state | Bit-identity rule + differential tests (ADR 0016); same-math-only caching (§6.3); schedule-independence gates (ADR 0018); the whole Phase 2–5 fixture corpus as regression net; zero-re-bless invariant (§9.1). |
| 23.2 Scope risk | Engine-polishing forever; speculative machinery without payoff | Measure-then-move: baseline doc, per-milestone deltas, revert-without-delta rule (§3); the executor's explicit "if justified" gate (§6.2); rejected-layout list (§4.4) prevents re-litigating. |
| 23.6 Memory growth | Bigger tiers + pool + atlas grow the working set past section 15's target | Byte ceilings with deterministic eviction (§4.3); pool capacity cap; tier-sized presets; plateau gates in the harness (§11.4); telemetry lines in the panel. |
| 23.4 Platform divergence | SIMD intrinsics, native threads, or GPU shortcuts that wasm can't follow | `wide` portable lanes + scalar twins; executor behind the unchanged trait; GPU derived-only (ADR 0017); wasm CI check throughout; no new parity surface to maintain (§5.3). |
| 23.1 Continuity | Higher density/radii or amortized retarget produce visible pops at scale | Continuity replay re-run under High tier, parallel executor, quartered budgets (§11.3); pinned-violation detector stays on under the GPU path; density defaults opt-in per tier, Low tier stays the proven config. |
| 23.3 Dependency explosion | Bigger windows amplify ripple costs | Unchanged invalidation machinery; ledger re-run is already in `cargo test`; budget saturation + drain gates make ripple cost visible and bounded (§7.3). |

The phase-specific risk: **optimization entropy** — a hundred small "obviously
fine" changes whose composition is not fine. Mitigation is structural: every
change lands under the zero-re-bless invariant with the differential and
equality gates in CI, milestones are small and independently reverted, and
the twins rule (§3.3) means every fast path has a slow path that still states
the intended semantics.

---

## 15. Incremental milestones

Each keeps CI green (native + wasm32), keeps every Phase 2–5 fixture and
harness passing unmodified, and adds its delta to `docs/perf-baseline.md`.

- **M1 — Measure.** `pass-timing` feature + `timing.rs`; FrameStats telemetry
  fields; panel TIMINGS/POOL/EXEC placeholders; present-mode management
  (vsync) replacing the busy-loop; scalar-vs-SIMD bench *skeletons*; the
  priority-inversion and wasted-work measurements (§6.2); `wer-scale`
  skeleton with the long-haul and teleport scripts running and reporting;
  `docs/perf-baseline.md` committed with the full baseline. *Exit:* every
  pass visible in the panel and the report; baseline numbers committed; the
  executor go/no-go evidence is in the doc.
- **M2 — Schedule.** LaneExecutor + cancellation tokens (if M1 justified;
  else this milestone shrinks to the replay-under-parallelism work);
  `rayon` removed if superseded; ADR 0018; replay and scale-harness
  schedule-independence gates (Inline vs Lane, budget scales, two-schedule
  state-hash equality). *Exit:* invariance gates green in CI; teleport-storm
  jobs-cancelled counter shows supersession working; measured settle-latency
  delta recorded.
- **M3 — Allocate.** `pool.rs` threaded through dispatch/integrate/evict;
  organism-vec recycling; capacity ceilings + deterministic capacity evictor;
  `max_retarget_regions` amortization; counting-allocator bench. *Exit:*
  ~zero steady-state update-path allocations; ceiling round-trip hash
  equality; settled-hash equality across amortization settings; deltas
  recorded.
- **M4 — Vectorize.** `simd.rs` + `wide`; fBm, climate, vegetation, soils,
  hydrology, drainage-flow rows per the bench ranking; the L8 hoist; ADR
  0016; differential tests in CI; `LayerDecl.cost` recalibrated from the new
  benches. *Exit:* all differentials bit-equal; all goldens untouched;
  per-kernel speedup table in the baseline doc; `region_map_update_*`
  improvement recorded.
- **M5 — Present.** `FieldAtlas`, delta uploads, `compose_map.wgsl`
  (false-color + overlays + refinement octaves), CPU/GPU A/B toggle, CPU path
  retained for headless/screenshot; ADR 0017. *Exit:* steady-state compose
  CPU cost and upload bytes/frame drop to the recorded targets; refinement
  toggles cleanly; screenshot/headless output byte-identical to Phase 5's.
- **M6 — Scale + sign-off.** `tier.rs` + detection inputs + presets (§7.4
  finalized from measurement); sub-cell realization (`organisms_per_cell`);
  full `wer-scale` scenario families incl. High-tier density gates and the
  ecology-at-density-4 scenario; panel TIER line; AGENTS.md / README command
  and architecture updates; final baseline table. *Exit:* every §11.4 gate
  green at every tier; §1.5 targets met or honestly revised with recorded
  reasons.

**Phase 6 is done when** M1–M6 are complete, CI is green (native + wasm32,
every Phase 2–5 golden and harness unmodified, the new differential and
invariance gates passing), and the success criterion holds with evidence: the
engine holds stable frame and generation budgets across the Low/Mid/High
tiers (bounded, draining backpressure; memory plateaus under ceilings) while
running a measurably denser world at the top tier — with settled world state
proven independent of executor, budget, amortization, and tier (ADR 0018),
every SIMD kernel proven bit-identical to its scalar twin (ADR 0016), GPU
output proven unable to reach authoritative state (ADR 0017), and not one
golden fixture re-blessed — the performance foundation, and the discipline,
that Phase 7's browser runtime will inherit through the same traits, the same
scalar fallbacks, and the same schedule-free world model.
