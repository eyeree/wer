# Phase 6 performance baseline

The committed ledger of "profiling justifies" (phase-6-plan.md §3, §12):
the M1 baseline every later milestone is measured against, plus a delta
entry per milestone. Regenerate the raw scenario table with
`cargo run --release --bin wer-scale -- --report`; kernel and update
numbers come from `cargo bench -p world-core` / `-p world-runtime`.

Wall-clock here is **telemetry, never a CI gate** (§12.6): CI gates counts,
bytes, hashes, and invariants; these numbers are compared locally, on the
reference machine below, against `wer-scale --report`.

## Reference machine

| | |
|---|---|
| CPU | 12th Gen Intel Core i9-12900K (24 logical CPUs), WSL2 |
| Toolchain | rustc 1.97.0 (stable, pinned by `rust-toolchain.toml`) |
| Profile | `--release` (thin LTO, 1 codegen unit); criterion via `cargo bench` |
| World config | `StreamConfig::default()` unless stated; harness scenarios at `field_resolution = 8` |

## M1 — baseline (measure only; no optimizations)

### Per-cell generation kernels (criterion, `world-core/benches/generation.rs`)

| kernel | time |
|---|---|
| `elevation` (5-octave fBm) | 121 ns |
| `geology` | 35.4 ns |
| `climate` | 1.16 ns |
| `hydrology` | 6.69 ns |
| `soils` | 2.92 ns |
| `classify` (biome) | 0.82 ns |
| `vegetation` | 2.53 ns |
| `possibility_field_sample` | 54.7 ns |
| `drainage_macro_tile` (whole 48² macro job) | 587 µs |

### Row kernels (the SIMD denominators, `row32/*` — M4 adds the `simd` twins)

| row (32 cells) | scalar |
|---|---|
| `row32/elevation` | 3.92 µs |
| `row32/climate` | 11.1 ns |
| `row32/vegetation` | 35.7 ns |

### Ecology kernels (`world-core/benches/ecology.rs`; `population_sample` is the M4 hoist target)

| kernel | time |
|---|---|
| `genome_from_seed` | 2.51 ns |
| `species_roster` | 71.7 ns |
| `food_web` | 179 ns |
| `population_sample` (per cell, pre-hoist) | 171 ns |

### `RegionMap::update` (criterion, `world-runtime/benches/update.rs`)

| scenario | time |
|---|---|
| `region_map_update_steady` | 335 µs |
| `region_map_update_drifting` | 1.18 ms |
| `region_map_settle_cold` (full window, unbudgeted) | 164 ms |
| `climate_flip_ripple` (world-scale flip, settle) | 2.77 s |

### `wer-scale --report` (per-pass means, default sizing, InlineExecutor + QueueExecutor pump)

| pass | long-haul avg ms | teleport-storm avg ms |
|---|---|---|
| integrate | 0.026 | 0.022 |
| evict | 0.035 | 0.005 |
| load | 0.030 | 0.025 |
| retarget | 0.056 | 0.044 |
| converge | 0.017 | 0.000 |
| dispatch | 0.820 | 0.371 |
| realize | 0.009 | 0.016 |
| flush | 0.000 | 0.000 |

Counters (deterministic; the gates ride these): long-haul peak 519 resident
regions, peak cache 1.8 MB (res 8), backlog drains in 56 frames after
pressure stops; teleport storm settles worst-case in 71 frames at the
default budget.

`dispatch` dominating the update passes (0.4–0.8 ms mean, spent almost
entirely inside generation kernels routed through it) and `retarget` being
the largest non-dispatch pass are the M3/M4 targets in miniature.

### Executor go/no-go evidence (§6.2)

Measured on the deterministic `QueueExecutor` (FIFO, bounded throughput per
frame — counts, not wall-clock):

- **Wasted (superseded) work, drift storm:** at 64 jobs/frame throughput,
  **30,870 of 78,350 executed jobs (39.4%)** produced results that were
  dropped as superseded during the long-haul bias storms. Cancellation
  (checking a token at dequeue) reclaims most of that worker time.
- **Priority inversion, cold settle:** throughput-dependent. At 64
  jobs/frame the queue clears each frame and Critical submissions see ~0
  lower-priority jobs ahead; at a constrained 8 jobs/frame (a Low-tier
  proxy), each Critical submission queued behind **~309** lower-priority
  jobs (peak queue depth 829). Lanes matter exactly when workers lag the
  dispatcher — the tiers where budget stability is hardest.

**Verdict: go.** Both §6.2 numbers are material in the scenarios the success
criterion names; M2 builds the LaneExecutor with cancellation.

## Milestone deltas

Recorded as each milestone lands; "no delta ⇒ revert" (§3).

| milestone | headline delta |
|---|---|
| M2 — LaneExecutor + cancellation | Long-haul drift storm: wasted kernel runs **39.4% → 0.0%** (35,985 of 78,386 dispatched jobs cancelled before running; 0 superseded results arrive). Backlog drain after pressure stops: 56 → 33 frames. Teleport-storm impatient-jump phase cancels 2,644 doomed jobs. Priority inversion eliminated by lanes (Critical drains first; the FIFO measurement was ~309 jobs ahead at constrained throughput). ADR 0018 equality gates green: settled hash `0xe8d0d5c815594e01` identical across Inline / Lane(2) / Lane(8) / budget ¼× / budget 4× / cancellation-off. `rayon` removed from the workspace. |
| M3 — pooling, ceilings, retarget amortization | Counting-allocator drift test (§4.2's own exit metric): the pool serves ≥ 98% of tile-buffer demand in steady drift (81 misses vs 4,499 tiles regenerated over 120 frames at res 16; ~37 KB/frame of tile allocation eliminated, remaining ~61 KB/frame is per-frame scratch). Wall-clock `region_map_update_*` statistically flat on the reference machine (allocator was not the bottleneck natively; the win is allocator pressure, which the wasm/browser target and bigger tiers inherit). Capacity ceilings land with deterministic farthest-first eviction: memory-ceiling scenario shows 222 capacity evictions with the near-window probe bit-identical on return (ADR 0008 recompute-exactness) and cache plateau ≤ ceiling + exempt near window. Retarget amortization (16/frame) joins the ADR 0018 equality gates — settled hash still `0xe8d0d5c815594e01`, unchanged through M3. |
| M4 — SIMD kernels + L8 hoist | Per-kernel: `row32/elevation` 3.95 µs → 1.52 µs (**2.6×**, gradient memoization + f32x4 transcription); `population` per cell 170 ns → 1.13 ns (**150×**, the §6.3 hoist into the roster entry); `drainage_macro_tile` 587 µs → 419 µs (−29%, fBm row fill). `climate`/`vegetation` row transcriptions measure ~1× (already 1–2.5 ns/cell scalar) — kept for the uniform, differential-pinned row interface, per ADR 0016's honesty note. Headline: `region_map_settle_cold` 164 ms → **95 ms (−42%)**, `climate_flip_ripple` 2.77 s → **1.60 s (−42%)**, drifting update −22%. All 5 differential suites bit-equal; `LayerDecl.cost` recalibrated (drainage 10 → 17, hydrology/vegetation 2 → 1; unit ≈ 25 µs/tile). Settled harness hash still `0xe8d0d5c815594e01` — zero output bits changed; zero fixtures re-blessed. The integer drainage *flow pass* was left scalar: post-fBm it is a minor share of the 419 µs macro job (the sort+accumulate half is sequential anyway) — deferred with reasons, per the measure-then-move rule. |
| M5 — GPU compose + refinement | Live shell A/B on the reference machine (llvmpipe, default window, idle at origin, refinement on): CPU path compose **21.4 ms/frame** (42 fps, full ~4 MB image re-upload every frame); GPU path compose **2.7 ms/frame** (153 fps) — an **8× compose reduction** — with steady-state upload **10 KB/frame** (the once-a-second panel refresh; atlas delta uploads are zero at rest, overlay uploads only on change). Refinement octaves (3, continuing the terrain spectrum at λ = 128/64/32) toggle live with `.`; `,` is the CPU/GPU parity A/B; `WER_CPU_MAP=1` starts in CPU mode. Headless `--screenshot` remains the untouched CPU composer. Atlas bookkeeping is pure-tested (delta uploads stop when nothing changes; slots recycle); both WGSL shaders naga-validate in CI; ADR 0017 (no readback API exists — enforced by the renderer's type surface). |
| M6 — tiers + density scale-up | Full `wer-scale` sign-off green at every gate (33 gates across 9 scenario families). Tier identity: the settled center region's 9 layer dependency hashes and its quantized possibility signature are bit-identical across Low/Mid/High. Density: `organisms_per_cell = 4` scales population ×4.12 (linear within tolerance), density-1 identities survive verbatim as slot 0 (231/231), all ids unique. Per-tier stability at each preset: backpressure bounded, backlog drains (Low 57 / Mid 57 / High 42 frames), realize cap respected, cache plateaus under ceiling + near exemption. High-tier continuity replay and the density-4 gates pass; settled harness hash `0xe8d0d5c815594e01` unchanged since M2 — six milestones of optimization, zero output bits moved. |

## Final tier presets (M6)

The §7.4 table as landed (`world-runtime/src/tier.rs`); Low is exactly the
Phase 5 defaults. Detection: ≤ 4 cores or a cpu-class adapter → Low; ≥ 8
cores and a discrete adapter → High; else Mid; `WER_TIER` overrides.

| Knob | Low | Mid | High |
|---|---|---|---|
| `load/unload_radius` (regions) | 12 / 14 | 14 / 16 | 17 / 19 |
| `far_radius` | 9 | 11 | 13 |
| `max_regen_cost` | 96 | 192 | 384 |
| `max_realize_organisms` | 400 | 800 | 1,600 |
| `organisms_per_cell` | 1 | 2 | 4 |
| `max_resonance_nodes` | 64 | 96 | 128 |
| `max_retarget_regions` | every frame | 160 | 240 |
| field cache ceiling | 48 MB | 96 MB | 160 MB |
| GPU refinement | off | on | on |

**§1.5 honesty accounting.** The High targets landed as planned. The headroom
purchasing them: cancellation reclaims the ~39% of drift-storm kernel runs
that were previously wasted (M2); the lane executor spreads generation over
`cores − 1` workers with Critical-first draining (the Phase 5 shim ignored
priority); the kernels themselves are 1.7× faster end-to-end (M4 settle-cold
−42%); and presentation left the CPU entirely (M5 compose 21.4 → 2.7 ms).
Together that exceeds the 4× regen-throughput bar the High budget
(`max_regen_cost` 96 → 384) spends. Wall-clock remains locally verified via
`wer-scale --report`; CI gates only the counts, bytes, and hashes above.

## 3D-1 — POV terrain (3d-phase-1-plan.md §9)

Measured on the reference machine (WSL2/llvmpipe, X11, default window,
`--release`, radius 3 = 49 chunks). Presentation-only work: no generated
output changed, zero fixtures re-blessed.

| metric | value |
|---|---|
| mesh time, worker-side | 26.8 ms for 49 chunks ≈ **0.55 ms/chunk** (4,489 `elevation_row` samples + color packing per chunk) |
| chunk vertex data | 4,485 verts × 28 B = **123 KB/chunk** (~6 MB at radius 3, pooled) |
| shared index buffer | 27,648 × u32 = 108 KB (skirt quads double-sided), built once for every chunk ever drawn |
| cold entry | 49 chunks meshed, integrated at the 4-uploads/frame cap (~13 frames to full ring); ~56 KB/frame mean upload during the first second |
| steady state | **0 remeshes, 0 uploads, 0 buffer allocations** (dep-hash + terrain-bucket keying; pool warm) |
| llvmpipe frame rate, POV radius 3 | ~160 fps (present 5.2 ms, update 0.6 ms) |
| llvmpipe frame rate, 2D GPU-map baseline | ~155 fps (compose 3.5 ms + present 2.0 ms) — POV is not the bottleneck |
| `wer --inline` A/B | identical lifecycle (49 meshed, 0 remeshed at rest); meshing runs synchronously inside the frame, pacing differs only (ADR 0018 posture) |

The 2D map path is pixel-identical to pre-3D-1: a `--screenshot` byte-diff
against the previous commit shows **0 differing map pixels** (the info panel
legitimately gains the `mesh` pass row).

Palette parity, measured with the ADR 0021 capture (`--pov-script`): a flat
unfogged ground pixel in POV reads (65, 112, 102) where the 2D Composite map
shows (67, 111, 100) at the same world position — the lighting constants
hold the 2D palette's value range within ±2/255. Region-border cracks were
found with the same harness (skirts were one-sided under back-face culling,
and the possibility-field gradient steps borders by far more than the
planned 4-unit drop) and fixed: double-sided skirt quads, 128-unit drop;
a top-down radius-8 sweep now shows zero gap pixels.

3D-2 (walk mode) adds one O(1) `ground_height` query per frame — a hash
lookup, four `f32` reads, and a handful of multiplies — with no measurable
frame-time change (verified on the llvmpipe reference environment).

## 3D-3 — water (3d-phase-3-plan.md §8)

Measured on the reference machine (WSL2/llvmpipe, `--release`), same-machine
A/B against `main` at the time of the change. Presentation-only: no
generated output changed, zero fixtures re-blessed, no new lifecycle events
(keying, scheduling, and eviction untouched — steady state remains 0
remeshes / 0 uploads / 0 allocations).

| metric | value |
|---|---|
| mesh time, worker-side (radius-1 ring over the (3050, 100) river basin) | 2.62 ms/chunk before, 2.66 ms/chunk after — the light-byte packing and overlay selection are noise against the shadow/AO march |
| river-overlay indices, same river-basin ring (9 chunks, 6 with overlay) | 69 531 indices ≈ **271 KB** at `RIVER_OVERLAY_MIN = 0.12`; riverless regions carry zero |
| threshold calibration | the plan's 0.08 selected 48% of the ring's core triangles, of which the 0.08–0.12 band renders at alpha ≤ 0.04 (invisible fill); 0.12 selects 31% — the wide remainder is the honest field (hydrology paints broad 0.2–0.5 river swaths that the 2D map colors blue too) |
| sea plane | 4 vertices, no vertex buffer, one blended pass; capture-path snapshots over an ocean vantage at 960×600 render without measurable wall-time regression vs. terrain-only |
| new GPU state | 2 pipelines + one vec4 in the frame uniform; overlay index buffers are exact-size and unpooled (replaced wholesale on remesh, dropped on evict) |

Verified with the ADR 0021 capture harness: waterline sits exactly where the
sediment ramp meets the beach; walk mode stands on the sea floor at
`z < 0` with the surface visible from below; a `RIVER_LIFT` exaggeration A/B
confirmed the overlay pipeline draws through the terrain vertex buffers.

## 3D-4 — organisms and directional shadows (3d-phase-4-plan.md §9)

Implementation evidence in this checkout is recorded separately from the
phase's performance sign-off. The available WSL2 environment is suitable for
pipeline creation, capture smoke tests, and invariant checks, but it is not the
required native-Windows hardware-GPU reference. No Windows frame-time or GPU
pass number is inferred from llvmpipe, and the unresolved cells below are
deliberately marked **pending** rather than populated with estimates.

| implemented quantity | factual value / evidence |
|---|---|
| CPU direct-shadow bake | The per-chunk horizon march and its scratch buffers are removed. `PovVertex.light[0]` is now neutral `255`; the existing coarse terrain AO remains in `light[1]`. The last pre-removal local number is the 3D-3 radius-1 river-basin result above (**2.66 ms/chunk**, where the shadow/AO march dominated); a same-machine post-removal ms/chunk measurement is pending. |
| shadow target | Low: **1024² `Depth32Float` ≈ 4 MiB**. Mid/High: **2048² ≈ 16 MiB**. It is independent of `WER_POV_SCALE`; `B` off omits the shadow draw and neutralizes both directional visibility and terrain AO. |
| shadow-pass submissions | Resident terrain core indices only (skirts excluded), followed by one instanced box batch and one instanced sphere batch. River overlays and sea do not cast. The pass is rebuilt every enabled frame; there is no unmeasured cache claim. |
| primitive cost | Box: **24 vertices, 12 triangles**. Two-subdivision icosphere: **162 vertices, 320 triangles**. Each visible organism is submitted once in the color pass and once in the enabled shadow pass. |
| packed instance traffic | **64 B/live instance** on an exact replacement. Buffers grow independently to powers of two and never shrink. A retained frame writes **0 B**; an explicit empty replacement clears both live counts without a write. Actual Low/Mid/High populations and high-water capacities remain scene measurements, not a 1,600-instance assumption. |
| CPU assembly lifecycle | One exact scan of `RegionMap::organisms()`, rendered-lattice height/AO queries, stable `(id, slot)` ordering, and collision-free vector comparison. Camera look and sub-cull-boundary translation retain the lists; realization, cull membership, expression, ground height, or AO changes replace them. Device-free lifecycle tests cover the zero-steady-upload contract; wall-clock scan time remains pending on the Windows reference. |
| optional activity bob | **Not built.** The static path was selected for this phase; every packed bob amplitude/phase is zero. A measured native-Windows animation go/no-go was unavailable, and static organisms satisfy the phase contract. |
| captures | Live POV, F12, and `--pov-script` use the same replacement upload and stabilized light fit. Script capture first runs the existing eight-update settle, then continues polling authoritative realization up to a bound of 128 zero-travel updates total; both capture paths freeze display time at zero. |
| local functional smoke (not a performance reference) | WSL2 llvmpipe (Mesa 25.2.8, Vulkan), dev profile, `WER_POV_RADIUS=1`, 64×64 capture at the origin: **9 chunks; 9,829 published; 1,616 drawn (769 box / 847 sphere); 324 waiting for ground; 7,889 distance-culled; 33 settle updates**. Packed instances were **101 KiB live / 128 KiB capacity / 101 KiB first replacement**. Repeating the command produced a byte-identical PPM. This proves pipeline creation/submission, frozen-time reproducibility, and telemetry on the available adapter; it supplies no native-Windows timing. |

### Native-Windows measurement gate — pending

The following matrix is required before 3D-4 performance sign-off. It was not
available in this implementation environment, so the quality/performance gate
remains open; in particular, 2048² for Mid/High is still an initial setting,
not a measured final choice.

| required measurement | result |
|---|---|
| CPU / GPU / adapter / driver / Windows version / Rust commit | **pending — native-Windows reference unavailable** |
| release configuration | `WER_WINDOW` fixed, `WER_POV_RADIUS=3`, `WER_POV_SCALE=1`, `WER_PRESENT_MODE=immediate`; execution **pending** |
| old/new cold-ring mesh total and ms/chunk | old local context recorded above; same-machine native-Windows A/B **pending** |
| terrain-only `B` off/on warmed median and p95 | **pending** |
| organism-dense High-tier `B` off/on warmed median and p95 | **pending** |
| world-update CPU and organism-scan CPU | **pending** |
| Low/Mid/High published, box, sphere, drawn, waiting counts | **pending** |
| Low/Mid/High live/capacity instance bytes and first replacement bytes | **pending** |
| steady-state replacement traffic | Exact lifecycle contract/test: **0 B/frame**; native-Windows telemetry confirmation **pending** |

The measurement owner should record the same fixed poses with `B` off/on in
one executable and compare 1024² with 2048². Per the implementation plan, a
2048² tier setting that adds more than 15% warmed p95 without a visible
contact/terrain-shadow improvement should move to 1024²; organism count must
not be reduced to make the result pass.

## Improvement A.8 — fixed routing and halo Terrain

Measured on the same local execution machine with the release Criterion
profiles. The pre-A.8 drainage measurement was taken from `main` immediately
before the worktree measurement; no full-Terrain job benchmark existed before
A.8, so the two new Terrain cases establish its ongoing baseline.

| benchmark | pre-A.8 | post-A.8 |
|---|---:|---:|
| one fixed routing elevation | n/a (float presentation path) | **249 ns** |
| `drainage_macro_tile` | **421 µs** | **764 µs** |
| Terrain 32², uniform P/G halo, Elevation + Slope | no dedicated case | **78.8 µs** |
| Terrain 32², adversarial varying halo, Elevation + Slope | no dedicated case | **78.4 µs** |

The macro tradeoff is deliberate: topology no longer uses the float/SIMD fBm
row and is now fixed Q30/i128 identity math. At the existing approximate
25-µs scheduling unit, conservative ceiling division recalibrates Terrain
`cost` 2 → **4** and Drainage `cost` 17 → **31**. A complete 32² field payload
is now **60,416 bytes** (14 `f32` channels plus biome `u8` and dominant `u16`);
the three rolling Terrain ghost rows are bounded job scratch and are not
misreported as cache payload. Finite temporal-budget scaling now floors
`max_regen_cost` at the largest declared atomic cost (31), preserving liveness
for the ADR 0018 quarter-budget schedule.

## Native/web alignment — Map, POV, and Split

Measured for the native/web alignment milestone on the WSL2 reference machine
above with the release profile, Mesa llvmpipe, the origin pose, default POV
radius 3, and POV render scale 1. Map uses the tier's complete streamed window;
POV and Split use a fixed 1024×768 offscreen target. The times below are one
cold process's total wall time, including world settle, meshing, renderer
startup, capture, and file output; peak RSS is `/usr/bin/time` telemetry. They
are characterization numbers, not frame-time claims or CI thresholds.

Representative commands:

```sh
WER_TIER=low target/release/wer --screenshot /tmp/low-map.ppm composite 0 0 1
WER_TIER=low target/release/wer --pov-script \
  "size:1024x768; pos:0,0; snap:/tmp/low-pov.ppm"
WER_TIER=low target/release/wer --pov-script \
  "size:1024x768; pos:0,0; split:/tmp/low-split.ppm"
```

The same commands were repeated with `WER_TIER=mid` and `high`.

| tier | Map CPU capture (output, elapsed, peak RSS) | POV capture (elapsed, peak RSS) | Split capture (elapsed, peak RSS) |
|---|---|---|---|
| Low | 1640×800, **0.18 s**, 44,216 KiB | **0.83 s**, 294,668 KiB | **0.85 s**, 301,156 KiB |
| Mid | 1768×928, **0.25 s**, 57,668 KiB | **0.94 s**, 323,652 KiB | **0.94 s**, 324,160 KiB |
| High | 1960×1120, **0.37 s**, 81,876 KiB | **1.19 s**, 354,388 KiB | **1.19 s**, 366,248 KiB |

POV and Split report the same resident presentation set because they are two
layouts over one traveler and post-update world state:

| tier | drawn / published organisms | box / sphere | live / capacity instance bytes | first-capture terrain uploads |
|---|---:|---:|---:|---:|
| Low | 9,593 / 9,829 | 5,667 / 3,926 | 599.6 / 768.0 KiB | 49 |
| Mid | 19,203 / 19,689 | 11,392 / 7,811 | 1,200.2 / 1,536.0 KiB | 49 |
| High | 38,360 / 39,327 | 22,598 / 15,762 | 2,397.5 / 3,072.0 KiB | 49 |

Two same-target `split:` instructions were then run in one process at every
tier. Each second capture was byte-identical to the first and reported **0
terrain uploads** and **0.0 KiB organism replacement**, versus 49 uploads and
599.6 / 1,200.2 / 2,397.5 KiB on the first capture. This is direct evidence
that file-bound Split capture retains the POV resources and does not turn ADR
0021 into a live readback path.

Steady presentation work is gated structurally, not by machine-dependent timing:

- shared atlas tests require zero dependency-keyed region uploads on an
  unchanged sync and stable Map backing storage;
- shared panel-cache and web DOM tests require an unchanged semantic key to
  produce no document rebuild and no DOM field mutation; and
- shared POV hover tests require an unchanged screen ray and geometry key to
  reuse the cached hit without another geometry query.

These are zero-work/reuse assertions, not a claim that a general-purpose
allocator recorded zero process allocations. The browser
`web-signoff --assert-layout` run passed all six viewport/DPR cases plus real
click, primary-drag, key, DOM-wheel, generated-help, panel-cadence, Split-focus,
one-update, and loss-fallback assertions. `web-signoff --profile-alignment` is
the repeatable local Low/Mid/High × Map/POV/Split collection command; its
wall-clock fields are informational and intentionally have no thresholds.
Ordinary headless Chrome has no renderer surface, so a settled Split GPU delta
measurement is explicitly not applicable there rather than faked. That
environment-specific measurement belongs to a hardware/CDP run; shared atlas
tests and the native repeated Split captures above remain the deterministic
zero-delta evidence.
