# AGENTS.md

Operating guide for AI coding agents working in this repository. Humans should
read [`README.md`](README.md) first; this file is the machine-facing companion
that assumes you will edit code.

## What this project is

The **Infinite World Exploration Game** — a native Rust engine (with a planned
browser/WebAssembly/WebGPU target) for an exploration game built around
*continuous travel through possibility space*. The design vision is in
[`Infinite_World_Exploration_Project_Overview.md`](Infinite_World_Exploration_Project_Overview.md);
the phased technical plan is in [`implementation-plan.md`](implementation-plan.md).

The repository is at **Phase 3** (procedural genetics and ecology, see
[`phase-3-plan.md`](phase-3-plan.md)), built on the landed Phase 2 stack. Phase 2
is a nine-layer declared dependency graph — terrain, geology, macro drainage,
climate, hydrology, soils, biome, vegetation, and now **ecology (L8)** — with
dependency-hash staleness (ADR 0008), topological cost-budgeted dispatch, stable
integer river topology (ADR 0009), the continuity replay, and the
invalidation-precision harness (`wer-ledger`). Phase 3 adds procedural genomes,
species rosters, food webs, the aggregate-ecology layer L8 (the first reader of
the Morphology/Behavior/Aesthetics domains), a signature-keyed roster cache, and
near-field organism realization from the aggregate fields — species identity is
presentation-grade until the atlas needs otherwise (ADR 0010), and its coherence
and diversity are machine-checked by the ecology harness. The renderer still only
presents one CPU-composed debug texture (near-field organisms surface as debug
markers, not meshes), the possibility vector is still one scalar per domain
(Phase 3 *reads* its last four domains, it does not grow them), and the `Storage`
trait is still unused — those grow in later phases; do not mistake them for
finished subsystems.

## Toolchain

- Rust **stable**, pinned in [`rust-toolchain.toml`](rust-toolchain.toml)
  (edition 2021, `rust-version = 1.85`). Components: `rustfmt`, `clippy`. Target
  `wasm32-unknown-unknown` is installed by the toolchain file.
- `cargo` may not be on a non-interactive shell's PATH; run
  `source "$HOME/.cargo/env"` first if you get `command not found: cargo`.

## Commands

```sh
# Build & run the native app shell (opens a window, clears to dusk blue).
cargo run --bin wer

# Deterministic inspector: world position -> region + origin feature hash.
cargo run --bin wer-inspect -- 300 -10

# Run everything, including the determinism golden fixtures.
cargo test --workspace

# Keep the platform-neutral crates + web shell compiling for the browser.
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown

# Format & lint exactly as CI does.
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

**Before you consider a change done, it must pass what CI runs** (see
[`.github/workflows/ci.yml`](.github/workflows/ci.yml)): `fmt --check`, `clippy`,
`check`, and `test` on the whole workspace natively, plus a `wasm32` `cargo
check` of `world-core`, `world-runtime`, and `platform-web`. **CI sets
`RUSTFLAGS: -D warnings`, so any warning fails the build** — treat clippy
warnings and unused-code warnings as errors. Run clippy locally the same way if
in doubt: `RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets`.

## Crate architecture and the boundary rule

The workspace is split into **platform-neutral** and **platform-specific** crates
so core simulation/generation compiles for native *and* `wasm32` from the start
(see [ADR 0002](docs/adr/0002-workspace-crate-boundaries.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `crates/world-core` | neutral lib | Deterministic hashing, coordinates, possibility space. Pure computation. |
| `crates/world-runtime` | neutral lib | Region lifecycle + abstract `Storage` / `TaskExecutor` traits. |
| `crates/renderer` | platform lib | wgpu/WGSL renderer (clear-screen only for now). |
| `crates/platform-native` | bin `wer` | winit window + event loop; native services. |
| `crates/platform-web` | cdylib+rlib | wasm-bindgen smoke shell; grows into the browser runtime. |
| `crates/tools` | bin `wer-inspect` | Inspectors, validators, replay tools. |

**The rule that must not be broken:** the neutral crates (`world-core`,
`world-runtime`) may **not** touch the filesystem, spawn threads, open sockets,
or call platform graphics/browser APIs, and must **never** depend on a platform
crate. Anything platform-specific they need is expressed as a trait here
(`Storage` in `world-runtime/src/storage.rs`, `TaskExecutor` in
`world-runtime/src/task.rs`) and implemented in a platform crate. Dependency
direction flows one way: platform crates depend on neutral crates, never the
reverse. This is enforced by review and by the wasm CI job — if you add a native
dependency to a neutral crate, the `wasm32` check will break.

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
  full direction+accumulation equality is required, ADR 0009 — and the Phase 3
  `genome_sample` and `food_web_sample`, the portable genetics surface: a
  genome and food-web tier biomass are pure functions of an integer seed, so
  they are cross-platform, but the *habitat signature a cell derives* is
  presentation-grade and deliberately not a parity export, ADR 0010). This
  equality is the determinism guarantee the browser port depends on.
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

- Vision / gameplay: `Infinite_World_Exploration_Project_Overview.md`
- Technical plan and phases: `implementation-plan.md` (Phase 0 is current; the
  central question the prototype must answer is section 22)
- Why the structure is the way it is: `docs/adr/`
- Human-facing build/run/browser-smoke instructions: `README.md`
