# Infinite World Exploration Game

A native Rust engine (with a planned browser/WebAssembly/WebGPU target) for an
exploration game built around **continuous travel through possibility space** —
one seamless journey across an infinite landscape of possible worlds, steered by
*anchors* the player collects.

See the design and architecture documents:

- [`Infinite_World_Exploration_Project_Overview.md`](Infinite_World_Exploration_Project_Overview.md) — the game vision.
- [`implementation-plan.md`](implementation-plan.md) — the high-level technical plan.
- [`docs/adr/`](docs/adr/) — architecture decision records.

This repository is currently at **Phase 2** (layered environmental generation,
see [`phase-2-plan.md`](phase-2-plan.md), building on the Phase 1 continuity
prototype of [`phase-1-plan.md`](phase-1-plan.md)): an eight-layer declared
dependency graph — terrain, geology, macro drainage, climate, hydrology,
soils, biomes, vegetation — where staleness is a dependency-hash comparison,
changes recompute exactly the layers that declare a dependency on them
(machine-checked by `wer-ledger`), river networks are stable integer topology,
and an interactive false-color debug map makes continuity — or its failure —
visible.

## Workspace layout

Platform-neutral crates compile for both native and `wasm32`; platform crates
hold everything OS/browser-specific (see
[ADR 0002](docs/adr/0002-workspace-crate-boundaries.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| [`world-core`](crates/world-core) | neutral lib | Deterministic hashing, coordinates, possibility space, the layer graph and every environmental generator. |
| [`world-runtime`](crates/world-runtime) | neutral lib | Region streaming, convergence, dep-hash staleness, topological cost-budgeted dispatch; abstract storage & task interfaces. |
| [`renderer`](crates/renderer) | native/gpu lib | wgpu/WGSL renderer (debug-map presentation). |
| [`platform-native`](crates/platform-native) | bin `wer` | winit window + event loop, input, Rayon executor. |
| [`platform-web`](crates/platform-web) | cdylib | wasm-bindgen smoke shell (grows into the browser runtime). |
| [`tools`](crates/tools) | bins `wer-inspect`, `wer-replay`, `wer-ledger` | Inspectors, the continuity replay, the invalidation-precision harness. |

## Prerequisites

- Rust (stable) via [rustup](https://rustup.rs). The pinned toolchain and the
  `wasm32-unknown-unknown` target are declared in [`rust-toolchain.toml`](rust-toolchain.toml).
- Native GPU drivers with a Vulkan/Metal/DX12 backend (for running `wer`).

## Common commands

```sh
# Build & run the interactive continuity prototype (opens the debug map).
cargo run --release --bin wer

# Deterministic inspector: world position -> region, hashes, every layer's
# samples; --layers adds the dependency-hash chain and stale/fresh verdicts.
cargo run --bin wer-inspect -- 300 -10 --layers

# Headless continuity replay: scripted path + anchors, machine-checked.
cargo run --bin wer-replay

# Invalidation-precision harness: asserts each scripted change regenerates
# exactly the declared-dependent layers (phase-2-plan.md §12.3).
cargo run --bin wer-ledger

# Headless map screenshot (no window/GPU): settle the world and dump a PPM.
cargo run --release --bin wer -- --screenshot map.ppm composite 0 0

# Test everything, including determinism goldens and the continuity replay.
cargo test --workspace

# Generation & streaming benchmarks (sizes the per-frame budgets).
cargo bench -p world-core --bench generation
cargo bench -p world-runtime --bench update

# Continuously verify the core still compiles for the browser target.
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown

# Lints & formatting (as run in CI).
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

## Driving the prototype

`cargo run --release --bin wer` opens a top-down false-color map of the
streaming window centered on the player, with an information panel on the
right: frame and streaming telemetry (fps, update time, active regions, cache
size, generation-job queue), the selected channel, current possibility nudges,
active anchors, the key bindings, and — when the mouse is over the map — the
world/region coordinates, streaming state, field samples, and biome of the
cell under the cursor. Watch the distance transform while the ground near you
stays put.

Transformation is fueled by travel
([ADR 0006](docs/adr/0006-travel-fueled-convergence.md)): stand still and the
world holds steady — bias nudges and anchors set the *destination*, and the
world drifts toward it only as you move (sprinting drifts it faster).

| Input | Effect |
|-------|--------|
| `WASD` / arrows (+`Shift`) | Move (sprint) |
| `1`–`8` (+`Shift`) | Nudge a possibility dimension up (down): Planetary, Climate, Geology, Hydrology, Ecology, Morphology, Behavior, Aesthetics |
| `Z` | Reset all nudges |
| `E` / `Q` | Drop an Emphasize / Suppress anchor at the player |
| `C` | Clear anchors |
| `V` | Cycle channel: composite → elevation → geology → temperature → moisture → river → wetness → soil → biome → vegetation → stability → revision |
| `G` / `N` / `X` | Toggle region grid / stability rings / changed-while-pinned flash |
| `Esc` | Quit |

The white and orange rings are the near (pinned) and far (free) stability
radii. Any region that flashes red changed while pinned — that is a continuity
bug by definition; the same condition fails `wer-replay` and CI.

### WSL2 / WSLg note

Under WSL the app automatically uses the X11 backend: WSLg's Wayland compositor
drops the connection a few seconds after a Vulkan swapchain starts presenting
(`ERROR_SURFACE_LOST_KHR`, then `Connection reset by peer`). Set
`WER_FORCE_WAYLAND=1` to opt back into Wayland, and `WGPU_BACKEND=vulkan|gl|...`
to override the graphics backend (the renderer honors the standard wgpu
environment variables). Rendering may run on Mesa's `llvmpipe` software
rasterizer in WSL — the debug map is a single texture blit, so it stays fast.

## Browser smoke test

`platform-web` compiles the deterministic core to `wasm32` and logs the origin
feature hash. To build and serve it locally:

```sh
cargo install wasm-bindgen-cli   # once
cargo build -p platform-web --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/platform_web.wasm \
  --out-dir crates/platform-web/web/generated --target web
# then serve crates/platform-web/web over http (WebGPU needs a secure context)
python3 -m http.server --directory crates/platform-web/web 8080
```

Open <http://localhost:8080> and check the console for
`[wer] wasm smoke ok — origin feature hash: …`. That value **must** match the
native `wer-inspect 0 0` output — the determinism guarantee the browser port
depends on.

## Determinism

Permanent world identities are derived from integer hashing over stable inputs
(world version, region coordinate, layer, feature index, possibility revision);
drainage routing is all-integer topology, parity-tested against wasm. Any
change that alters generated output must bump `WORLD_ALGORITHM_VERSION` — or,
for a single layer's tuning, that layer's `algorithm_revision` in the
declaration table — and update the golden fixtures in
[`crates/world-core/tests/determinism.rs`](crates/world-core/tests/determinism.rs)
in the same commit. See [ADR 0003](docs/adr/0003-deterministic-integer-hashing.md)
and [ADR 0008](docs/adr/0008-tiles-are-functions-of-their-dependency-hash.md).

## License

Dual-licensed under MIT or Apache-2.0.
