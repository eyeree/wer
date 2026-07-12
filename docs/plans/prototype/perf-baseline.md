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
| shared index buffer | 26,112 × u32 = 102 KB, built once for every chunk ever drawn |
| cold entry | 49 chunks meshed, integrated at the 4-uploads/frame cap (~13 frames to full ring); ~56 KB/frame mean upload during the first second |
| steady state | **0 remeshes, 0 uploads, 0 buffer allocations** (dep-hash + terrain-bucket keying; pool warm) |
| llvmpipe frame rate, POV radius 3 | ~160 fps (present 5.2 ms, update 0.6 ms) |
| llvmpipe frame rate, 2D GPU-map baseline | ~155 fps (compose 3.5 ms + present 2.0 ms) — POV is not the bottleneck |
| `wer --inline` A/B | identical lifecycle (49 meshed, 0 remeshed at rest); meshing runs synchronously inside the frame, pacing differs only (ADR 0018 posture) |

The 2D map path is pixel-identical to pre-3D-1: a `--screenshot` byte-diff
against the previous commit shows **0 differing map pixels** (the info panel
legitimately gains the `mesh` pass row).
