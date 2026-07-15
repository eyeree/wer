# AGENTS.md

Operating guide for AI coding agents working in this repository. Humans should
read [`README.md`](README.md) first; this file is the machine-facing companion
that assumes you will edit code.

## What this project is

The **Infinite World Exploration Game** — a native and browser
Rust/WebAssembly/WebGPU engine for an exploration game built around
*continuous travel through possibility space*. The design vision is in
[`docs/Infinite_World_Exploration_Project_Overview.md`](./docs/Infinite_World_Exploration_Project_Overview.md);
the phased technical plan is in
[`implementation-plan.md`](docs/plans/prototype/implementation-plan.md).

The repository has landed the Phase 7 static browser viewer/runtime slice on top of
**Phase 6** (performance and scale, see
[`phase-6-plan.md`](docs/plans/prototype/phase-6-plan.md)), built on the landed
Phase 2–5 stacks.
Phase 2 is a nine-layer declared dependency graph — terrain, geology,
macro drainage, climate, hydrology, soils, biome, vegetation, and **ecology
(L8)** — with dependency-hash staleness (ADR 0008), topological cost-budgeted
dispatch, stable integer river topology (ADR 0009), the continuity replay, and
the invalidation-precision harness (`wer-ledger`). Phase 3 adds procedural
genomes, species rosters, food webs, the aggregate-ecology layer L8 (the first
reader of the Morphology/Behavior/Aesthetics domains), a signature-keyed roster
cache, and near-field organism realization — species identity is
presentation-grade until the atlas needs otherwise (ADR 0010), machine-checked by
the ecology harness. Phase 4 turns the possibility machinery into the game: anchors
capture the *traits* of discoveries into a possibility `target` and combine
**order-independently** (ADR 0011), `project_plausible` grows into the full
section-8 rule set as an idempotent relaxation, and a transient **resonance**
graph gates convergence (ADR 0012), machine-checked by the anchor harness
(`wer-anchor`). Phase 5 makes exploration durable and shareable: the **vault**
(the first and only user of the `Storage` trait) persists *deviations only* —
named discoveries, expedition routes, preserves, the discovered-region set, and a
bit-exact run-local session snapshot — through a versioned record codec
(`RECORD_FORMAT_VERSION`, serde + postcard, golden byte fixtures). Shareable
records are **quantized at the persistence boundary** (ADR 0013: integers only,
so a shared anchor steers bit-identically everywhere), keyed by content-derived
ids with CRDT merge laws (ADR 0014: union-by-id, commutative/associative/
idempotent — the whole "server-compatible" claim, exercised by `wer-atlas`
bundles, no server anywhere). Preserves pin regions to their quantized buckets
(a few dozen bytes reproduce the whole landscape via ADR 0008); routes attract
as derived weak anchors on the fast domains only (ADR 0015: soft, saturating in
usage, never steering the stable topology). Save→load→settle is state-hash
exact, machine-checked by the vault harness (`wer-vault`). Phase 6 is the
optimization phase, and it changes **no generated output for any input**:
`WORLD_ALGORITHM_VERSION` stays at 2, every `algorithm_revision` stays 0, and
zero golden fixtures were re-blessed. It adds the measurement layer (per-pass
timings behind the `pass-timing` feature, the committed
[`perf-baseline.md`](docs/plans/prototype/perf-baseline.md) ledger), the **LaneExecutor**
(three priority lanes + cancellation tokens, hosted in `tools::executor` so
the harnesses drive the production scheduler; `rayon` is gone; `wer --inline`
is the A/B), the **tile pool** and byte-capacity **cache ceilings** with
deterministic farthest-first eviction, amortized retarget, **SIMD row
kernels** in `world-core/src/simd.rs` that are *bit-identical* to their
scalar twins (ADR 0016, differential-tested in CI; `wide`, scalar on wasm
without `simd128`), the same-math L8 hoist, the **GPU-composed debug map**
(region-tile atlas, dep-hash-keyed delta uploads, WGSL refinement octaves —
derived presentation only, no readback API exists, ADR 0017; the CPU
composer remains the headless/screenshot/test path, `,`/`.` toggle
compose/refinement, `WER_CPU_MAP=1` starts CPU), and **resource tiers**
(`world-runtime/src/tier.rs`: Low = the Phase 5 defaults, Mid, High with
`organisms_per_cell` up to 4 — additive identities, slot 0 keeps Phase 5
ids; `WER_TIER`/`WER_CACHE_MB` override). Settled world state is proven
schedule-independent across executor, worker count, budget scale,
cancellation, amortization, and tier (ADR 0018), machine-checked by the
scale harness (`wer-scale`). The possibility vector is still one scalar per
domain, and there is no networking or account/server subsystem.
The browser shell now builds to `target/web-dist` with viewer/docs/help routes,
wasm facade snapshots, real CPU map composition from the streamed `RegionMap`
(shared per-cell colors in `world_runtime::mapcolor`), map movement/zoom/panel
parity with the native shell, and a `web-signoff` harness. POV mode hosts the
shared 3D renderer (`crates/pov-host` + `crates/renderer`) on a WebGPU canvas,
with device-loss/unsupported fallbacks returning to map mode. Browser
world jobs still use `InlineExecutor`; the Worker is a capability/ping probe,
IndexedDB is opened only as a capability probe, and vault/session effects are
reported unavailable. Browser networking and accounts still do not exist.

Native and browser viewer behavior is aligned by `crates/viewer-host` (ADR
0028): normalized input and one binding/action registry feed one traveler and
one `RegionMap` update, while Map, POV, and fixed-ratio Split are presentation
panes of that same post-update state. `viewer-host` also owns Map composition
and atlas preparation, layout/focus, CPU-authoritative Map/POV inspection, and
the semantic information-panel document. The shells retain only environment
adapters, services, surface/panel rendering, and lifecycle/recovery. The
renderer records every visible pane in one surface acquire/submit/present and
still exposes no live readback.

Post-Phase-6 Improvement A.8 deliberately changes Terrain and Drainage output
under their layer-local version boundary: `WORLD_ALGORITHM_VERSION` remains 2,
Terrain and Drainage `algorithm_revision` are 1, and all other revisions remain
0. Macro routing elevation is integer-only Q30/i128 from field seed to
centimeters. Terrain samples a 3×3 realized-current/fallback P/G halo and emits
Elevation plus a centered ghost-derived Slope channel atomically; Hydrology and
Soils consume Slope. CI runs every parity probe as actual wasm in Node with
pinned `wasm-bindgen-test` 0.3.76 / `wasm-pack` 0.13.1 (ADR 0027).

## Toolchain

- Rust **stable**, pinned in [`rust-toolchain.toml`](rust-toolchain.toml)
  (edition 2021, `rust-version = 1.85`). Components: `rustfmt`, `clippy`. Target
  `wasm32-unknown-unknown` is installed by the toolchain file.
- `cargo` may not be on a non-interactive shell's PATH; run
  `source "$HOME/.cargo/env"` first if you get `command not found: cargo`.

## Commands

```sh
# Build & run the native app shell. In the app, F12 writes a debug dump —
# screenshot.ppm plus state.txt for Map, POV, or Split (mode/focus/panes,
# traveler/camera/hover, steering, telemetry, dep hashes, vault counters) —
# to ./dump/<UTC datetime>/, for diagnosing problems after the fact.
# The default UI is the wry overlay dock: the same DOM toolbar + information
# panel the browser shell renders (docs/wry-overlay/implementation-plan.md;
# Linux needs libwebkit2gtk-4.1-dev — see README prerequisites). WER_OVERLAY=0
# keeps the legacy bitmap panel + winit-only input (the benchmark-clean path);
# headless captures never create webviews.
cargo run --bin wer

# Deterministic inspector: world position -> region + origin feature hash.
# Add --layers / --species / --ecology / --steer for the dependency-hash chain,
# the roster + food web, the L8 aggregates, or the capture->steer->project
# chain; --vault / --routes read the record store at $WER_VAULT_DIR (default
# ./wer-vault) and report the records/route-graph around the position.
cargo run --bin wer-inspect -- 300 -10 --steer

# Atlas bundles: export/import/validate/list record stores and bundle files
# (the file-based proof of the sharing model; no server).
cargo run --bin wer-atlas -- list wer-vault

# Phase sign-off harnesses (headless, CI gates): invalidation precision,
# Phase 4 steering, Phase 5 persistence/sharing, and the Phase 6 scale gates
# (schedule independence, per-tier stability, memory ceilings, density).
cargo run --bin wer-ledger
cargo run --bin wer-anchor
cargo run --bin wer-vault
cargo run --release --bin wer-scale          # add --report for the baseline table

# Run everything, including the determinism golden fixtures.
cargo test --workspace

# Keep the shared runtime/viewer + web shell compiling for the browser.
cargo check -p world-core -p world-runtime -p viewer-host -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web

# Format & lint exactly as CI does.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

## Debugging the browser build (agent-browser)

Build and serve the static artifact, then drive it with the `agent-browser`
CLI:

```sh
cargo run --bin web-build     # rebuild target/web-dist (wasm + assets)
cargo run --bin web-serve     # serve at http://localhost:8080/
agent-browser open http://localhost:8080/

# Automated functional and local tier/mode diagnostics (starts its own server).
cargo run --bin web-signoff -- --assert-layout
cargo run --bin web-signoff -- --profile-alignment
```

For anything **WebGL/WebGPU** (the map atlas, POV terrain), know the
environment's traps before trusting a screenshot:

- **agent-browser's own headless Chrome has no WebGPU adapter** — POV init
  fails there by design (that exercises the fallback-to-map path, which is
  itself worth testing). And in headless Chrome, WebGPU canvas content does
  **not composite into screenshots**: a white/black canvas in a capture is
  an artifact, not a rendering bug. Verify headless runs functionally
  instead. `window.__viewerCharacterization()` reports layout, focus, tier,
  backing sizes, panel cadence, and frame/tick counters;
  `window.__mapStatus`, `window.__povStatus`, and
  `window.__rendererFrameStatus` expose the latest Map, POV, and one-surface
  frame diagnostics. Check those values, the information panel, and the
  diagnostics log rather than GPU pixels.
- **For real, visible GPU output, drive the Windows Chrome over CDP** (WSL2
  localhost forwarding reaches it in both directions):

  ```sh
  pwsh.exe -NoProfile -Command '& "$env:ProgramFiles\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222 --user-data-dir="$env:LOCALAPPDATA\agent-browser\chrome-cdp-profile" --no-first-run'
  agent-browser --session win --cdp 9222 open http://localhost:8080/
  ```

  Use a **named `--session`**: with a plain `--cdp` flag an existing local
  session silently wins and you end up evaluating in the wrong browser —
  check `navigator.userAgent` says `Windows NT` before trusting results.
  Windows Chrome composites WebGPU canvases into screenshots correctly and
  has a hardware adapter, so `agent-browser --session win --cdp 9222
  screenshot out.png` shows the real render.
- **Compare against native ground truth** at the same pose with the
  headless capture harness (ADR 0021), e.g.
  `cargo run --release --bin wer -- --pov-script "pos:0,0; mouse:-60,100;
  snap:/tmp/native-pov.ppm"` — pixel-level tone differences between that
  and the browser screenshot localize the bug (a uniformly darker browser
  render was the missing-sRGB-view symptom: WebGPU canvases refuse sRGB
  *swapchain* formats, so the renderer routes through an sRGB **view**
  format — see `render_format` in `crates/renderer/src/lib.rs`).
  Use `split:/tmp/native-split.ppm` in the same script to capture the shared
  fixed-ratio Map + POV + panel/focus layout; `WER_TIER=low|mid|high` applies
  to `--screenshot`, `snap:`, and `split:` (default Low).
- A SwiftShader WebGPU adapter is available in WSL headless Chrome via
  `--enable-unsafe-webgpu --enable-unsafe-swiftshader
  --use-webgpu-adapter=swiftshader` (launch your own Chrome with
  `--remote-debugging-port` and connect with `--cdp`) — good for exercising
  the code path, subject to the screenshot caveat above.

**Before you consider a change done, it must pass what CI runs** (see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml)): `fmt --check`, `clippy`,
`check`, and `test` on the whole workspace natively, plus a `wasm32` `cargo
check` of `world-core`, `world-runtime`, `viewer-host`, and `platform-web`,
followed by the Node wasm parity suite. **CI sets
`RUSTFLAGS: -D warnings`, so any warning fails the build** — treat clippy
warnings and unused-code warnings as errors. Run clippy locally the same way if
in doubt: `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`.

## Crate architecture and the boundary rule

The authoritative world crates are **platform-neutral**. Cross-platform viewer
and rendering crates compile for native and `wasm32`; winit, DOM, storage, and
executor APIs stay at platform boundaries. `renderer` has the narrow
wasm-specific `HtmlCanvasElement` surface adapter needed by `wgpu` (see
[ADR 0002](docs/adr/0002-workspace-crate-boundaries.md) and
[ADR 0028](docs/adr/0028-shared-viewer-host-and-one-world-multi-view.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `crates/world-core` | neutral lib | Deterministic hashing, coordinates, possibility space. Pure computation. |
| `crates/world-runtime` | neutral lib | Region lifecycle + abstract `Storage` / `TaskExecutor` traits. |
| `crates/renderer` | cross-platform GPU lib | wgpu/WGSL device and surfaces; Map atlas and POV passes recorded as one Map/POV/Split frame. No live readback. |
| `crates/pov-host` | cross-platform presentation lib | Fly/walk camera, pure terrain/organism presentation geometry, chunk scheduling, and CPU-side POV picking. |
| `crates/viewer-host` | environment-neutral viewer lib | Normalized input/actions, one-traveler controller, layout/focus, Map composer/atlas, inspection, and semantic panel model. No environment APIs. |
| `crates/platform-native` | bin `wer` | Thin winit adapter plus native storage/executor, bitmap panel, debug capture, and lifecycle. |
| `crates/platform-web` | cdylib+rlib + static shell | Thin wasm/DOM adapter, streamed viewer runtime, capability probes, DOM panel, canvas lifecycle, and recovery; execution remains inline and vault effects unavailable. |
| `crates/tools` | command-line bins | Inspectors, validators, replay/sign-off harnesses, web build/serve tooling, lane executor, and native storage. |

**The rule that must not be broken:** the neutral crates (`world-core`,
`world-runtime`) may **not** touch the filesystem, spawn threads, open sockets,
or call platform graphics/browser APIs, and must **never** depend on a platform
crate. Anything platform-specific they need is expressed as a trait here
(`Storage` in `world-runtime/src/storage.rs`, `TaskExecutor` in
`world-runtime/src/task.rs`) and implemented in a platform crate. Dependency
direction flows one way: platform crates depend on neutral crates, never the
reverse. This is enforced by review and by the wasm CI job — if you add a native
dependency to a neutral crate, the `wasm32` check will break.
`viewer-host` has the parallel ADR 0028 rule: it may depend on the world,
renderer, and POV crates, but must not touch winit, DOM/browser APIs,
filesystems, sockets, or platform thread creation. `renderer` must never depend
back on `viewer-host`.

## Determinism — the core invariant

Permanent world identities are derived by **integer hashing over stable inputs**
(world algorithm version, region coordinate, generator layer, feature index,
possibility-state revision) via a portable `splitmix64`-based mix in
`world-core/src/hash.rs`. See [ADR 0003](docs/adr/0003-deterministic-integer-hashing.md).

Non-negotiable rules when touching generation code:

- **Floating point is for approximate simulation and presentation only — never
  for a permanent identity.** Region coordinates are integers quantized from
  continuous positions; feature indices are integers.
- **The field fold order in `feature_hash` (and in `layer_dep_hash`) is part
  of the stable contract.** Changing the hashing, a fold order, or *any*
  generation algorithm that alters output for the same inputs requires
  **bumping `WORLD_ALGORITHM_VERSION`** (`world-core/src/lib.rs`) — or, when
  the change is confined to one layer's math, **bumping that layer's
  `algorithm_revision`** in the `world-core/src/layer.rs` declaration table
  (phase-2-plan.md §9.2) — **and updating the golden fixtures in
  `crates/world-core/tests/determinism.rs` in the same commit**. The golden
  determinism tests exist to catch accidental drift; a casual "just re-bless the
  test" is a determinism bug unless you meant to change the algorithm.
- **Native and wasm must agree.** `platform_web::origin_feature_hash()` must
  return the identical value as native `cargo run --bin wer-inspect -- 0 0`
  (currently `0x4c6ca5de38f90b17` at algorithm version 2). The same applies to
  every parity export (terrain gradient seed, control-point seed, lithology
  seed, the drainage routing sample — routing is all-integer topology, so
  full direction+accumulation equality is required, ADR 0009 — the Phase 3
  `genome_sample` and `food_web_sample`, the portable genetics surface: a
  genome and food-web tier biomass are pure functions of an integer seed, so
  they are cross-platform, but the *habitat signature a cell derives* is
  presentation-grade and deliberately not a parity export, ADR 0010 — and the
  Phase 4 `steer_sample`, the portable steering-math surface: `steer` and
  `project_plausible` are float-deterministic functions of the anchor set, so a
  fixed scripted steer is cross-platform, but a *live capture* reads `f32`
  organisms/tiles and is presentation-grade, deliberately not exported, ADR
  0011). This equality is the determinism guarantee the browser port depends on.
- A portable PRNG (`Rng`) may be seeded *from* a stable hash for approximate
  sampling; its float outputs are not sources of identity.

## Conventions

- **Dependencies are centralized.** Add a version to `[workspace.dependencies]`
  in the root `Cargo.toml`, then reference it from a crate with
  `thecrate.workspace = true`. Do not pin versions inside individual crate
  manifests. Browser-only deps in `platform-web` are gated under
  `[target.'cfg(target_arch = "wasm32")'.dependencies]` so a native workspace
  check stays fast and browser-free.
- **Lints are workspace-wide.** Every crate opts in with `[lints] workspace =
  true`. The workspace warns on `unsafe_op_in_unsafe_fn` and
  `missing_debug_implementations` — derive `Debug` on public types and keep
  `unsafe` blocks explicit.
- **Doc comments carry the "why" and cite the plan.** Existing modules reference
  the relevant `implementation-plan.md` section (e.g. "section 6.2") and the ADRs.
  Match that style: explain intent and portability/determinism implications, not
  just mechanics. Prefer `#[inline]`, `#[must_use]`, and `const fn` where the
  existing code does (hashing/coordinate primitives are `const`).
- **Shaders are WGSL** (`renderer/shaders/`) for WebGPU portability; the renderer
  targets `wgpu` only, never a native-specific graphics API.

## Architecture Decision Records

Significant architectural decisions live in [`docs/adr/`](docs/adr/), one
Markdown file per decision, numbered sequentially, using Nygard's template.
Records are **immutable once accepted** — to change course, add a new ADR that
supersedes the old one rather than editing history, and update the index in
[`docs/adr/README.md`](docs/adr/README.md). Before making a decision that
constrains later subsystems (determinism, crate boundaries, portability,
persistence formats), check whether an ADR already governs it.

## Where to look

- Vision / gameplay: `docs/Infinite_World_Exploration_Project_Overview.md`
- Technical plan and phases: `docs/plans`
- Why the structure is the way it is: `docs/adr/`
- Human-facing build/run/browser-smoke instructions: `README.md`
