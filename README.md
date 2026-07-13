# Infinite World Exploration Game

A native Rust engine (with a planned browser/WebAssembly/WebGPU target) for an
exploration game built around **continuous travel through possibility space** —
one seamless journey across an infinite landscape of possible worlds, steered by
*anchors* the player collects.

See the design and architecture documents:

- [`docs/Infinite_World_Exploration_Project_Overview.md`](docs/Infinite_World_Exploration_Project_Overview.md) — the game vision.
- [`docs/plans/prototype/implementation-plan.md`](docs/plans/prototype/implementation-plan.md) — the high-level technical plan.
- [`docs/adr/`](docs/adr/) — architecture decision records.

This repository is currently at **Phase 6** (performance and scale, see
[`phase-6-plan.md`](phase-6-plan.md), building on the Phase 1–5 stacks): the
layered generation and invalidation precision of Phase 2 (`wer-ledger`), the
procedural ecosystems of Phase 3, the resonance-gated steering of Phase 4
(`wer-anchor`), and the durable, shareable exploration of Phase 5
(`wer-vault`, `wer-atlas`) — now running on the optimization-era substrate:
a priority-lane executor with cancellation, a tile pool and byte-ceilinged
caches, SIMD kernels bit-identical to their scalar twins, a GPU-composed
debug map with refinement octaves (derived presentation only), and Low/Mid/
High resource tiers that scale world density without changing a single
generated output (`wer-scale` machine-checks all of it; the measured ledger
lives in [`docs/perf-baseline.md`](docs/perf-baseline.md)). The post-Phase-6
A.8 correction makes macro routing elevation entirely fixed-point and gives
ordinary Terrain a realized-current 3×3 P/G halo plus a centered Slope output
(ADR 0027; Terrain and Drainage layer revisions are 1, world version remains 2).

## Workspace layout

Platform-neutral crates compile for both native and `wasm32`; platform crates
hold everything OS/browser-specific (see
[ADR 0002](docs/adr/0002-workspace-crate-boundaries.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| [`world-core`](crates/world-core) | neutral lib | Deterministic hashing, coordinates, possibility space, the layer graph and every environmental generator. |
| [`world-runtime`](crates/world-runtime) | neutral lib | Region streaming, convergence, dep-hash staleness, topological cost-budgeted dispatch; abstract storage & task interfaces. |
| [`renderer`](crates/renderer) | native/gpu lib | wgpu/WGSL renderer (debug-map presentation). |
| [`platform-native`](crates/platform-native) | bin `wer` | winit window + event loop, input, tier detection, the GPU-map shell. |
| [`platform-web`](crates/platform-web) | cdylib | wasm-bindgen smoke shell (grows into the browser runtime). |
| [`tools`](crates/tools) | bins `wer-inspect`, `wer-replay`, `wer-ledger`, `wer-anchor`, `wer-atlas`, `wer-vault`, `wer-scale` | Inspectors, the continuity replay, the phase sign-off harnesses, the lane executor, atlas bundle tooling, and the shared native file-tree storage backend. |

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

# Steering harness: intentional/selective/coherent/resonance-gated
# (phase-4-plan.md §12.3).
cargo run --bin wer-anchor

# Persistence/sharing harness: durable, sparse, shareable, preserves, routes,
# precision-preserved (phase-5-plan.md §12.3).
cargo run --bin wer-vault

# Scale harness: schedule independence (ADR 0018), per-tier stability,
# memory ceilings, density gates; --report prints the perf-baseline table.
cargo run --release --bin wer-scale -- --report

# Atlas bundles: share discoveries/routes/preserves between record stores.
cargo run --bin wer-atlas -- export wer-vault my.bundle
cargo run --bin wer-atlas -- check my.bundle

# Headless map screenshot (no window/GPU): settle the world and dump a PPM.
cargo run --release --bin wer -- --screenshot map.ppm composite 0 0

# Headless POV screenshots (offscreen GPU, ADR 0021): drive the fly/walk
# camera through a scripted sequence — pos:x,y[,z] | mouse:dx,dy |
# move:f[,r[,u]] | walk | fly | settle[:n] | size:WxH | snap:file.ppm —
# the POV debugging harness.
cargo run --release --bin wer -- --pov-script \
  "pos:300,-10; walk; move:200; snap:walk-a.ppm; mouse:400,0; move:200; snap:walk-b.ppm"

# Test everything, including determinism goldens and the continuity replay.
cargo test --workspace

# Generation & streaming benchmarks (sizes the per-frame budgets).
cargo bench -p world-core --bench generation
cargo bench -p world-runtime --bench update

# Continuously verify the core still compiles for the browser target.
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown

# Execute every deterministic parity probe as real wasm in Node.
wasm-pack test --node crates/platform-web

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
| `T` / `Y` / `K` | Cycle capture trait category / toggle polarity / capture the feature under the player |
| `R` | Toggle transition movement mode (slow, resonance-gated steering) |
| `C` | Clear anchors |
| `O` / `L` | Save / load the session (store at `WER_VAULT_DIR`, default `./wer-vault`) |
| `B` / `I` | Record the latest anchor as a named discovery / summon vault discoveries as anchors |
| `P` | Preserve the pinned near window (or delete the preserve you stand in) |
| `H` | Toggle persistent path tracking (off by default; gates route recording, traversal detection, attraction, and map polylines) |
| `J` / `U` | Start/finish recording an expedition route / toggle route attraction (effective while path tracking is on) |
| `Delete` | Clear all recorded routes from the vault |
| `V` | Cycle the visualized channel (composite, layers, ecology, influence, stability, …) |
| `G` / `N` / `X` / `M` / `F` | Toggle region grid / stability rings / changed-while-pinned flash / organism markers / discovered-region dimming |
| Mouse wheel | Zoom the map view in/out (x1–x16); zoomed in past x4, hovering an organism marker shows that organism's details in the panel instead of the region info |
| `Tab` | Toggle the 3D POV mode: a fly camera over lit, fogged terrain meshed from the same authoritative fields the map shows. The ocean is a translucent animated surface at sea level over a sediment sea floor (coastlines correct at every possibility state by construction); rivers and wetlands glint on the ground, with strong channels carrying a conformal water overlay. In POV: **hold the left mouse button and drag to look**, `WASD` along view/strafe, `Space`/`LShift` up/down, wheel adjusts the active mode's speed, `F` toggles walk ↔ fly (walk holds the eye 1.7 units above the rendered terrain, cliffs climb as fast ramps, the sea floor is walkable). An FPS counter sits in the top-right corner, and three diagnostic toggles isolate render cost on software rasterizers: `B` baked lighting (shadows/AO), `N` per-fragment detail normals, `V` the water passes. All map bindings above are map-mode-only. `WER_POV=1` starts in POV; `WER_POV_RADIUS` (1–8, default 3) sets the chunk draw radius |
| `F12` | Write a debug dump to `./dump/<UTC datetime>/`: a screenshot of the active view (map or POV) plus `state.txt` with the player/camera state (position, forward vector), steering, telemetry, the region's dependency-hash chain, and vault counters. Works in both modes |
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

## Browser static app

`platform-web` builds as a static browser artifact under `target/web-dist`.
The artifact contains ordinary relative links (`index.html`, `assets/`,
`generated/`) so it can run from a repository subpath on a static host such as
GitHub Pages. To build and serve it locally:

```sh
cargo run --bin web-build
node crates/platform-web/web/smoke.mjs target/web-dist
cargo run --bin web-signoff
python3 -m http.server --directory target/web-dist 8080
```

Open <http://localhost:8080> and check the console for
`[wer] wasm smoke ok — origin feature hash: …`. The viewer status bar also
prints the same hash. That value **must** match the native `wer-inspect 0 0`
output — the determinism guarantee the browser port depends on.

CI also pins `wasm-pack` 0.13.1 and executes the complete parity suite in Node,
including signed fixed routing elevations and three complete macro tiles.

### Static deployment

Publish `target/web-dist` as-is. The artifact is subpath-safe: pages link to
`./assets`, `./generated`, `./docs/world-model.html`, and `./help/` with
relative URLs, so GitHub Pages can serve it from a project path. Runtime storage
uses browser APIs only; there is no server, socket, filesystem, or generated
world cache in the deployment.

Browser compatibility tiers:

| Tier | Expected support | Behavior |
|------|------------------|----------|
| WebLow | no WebGPU or weak worker/storage support | CPU map, inline execution, small cache ceiling |
| WebMid | ordinary desktop browser | WebGPU atlas mode when available, worker mode selectable |
| WebHigh | strong desktop with stable WebGPU | larger cache ceiling and refinement default |

Unsupported WebGPU, worker, storage, and POV paths fall back visibly in the
HTML status/info panel and do not change settled hashes.

## Determinism

Permanent world identities are derived from integer hashing over stable inputs
(world version, region coordinate, layer, feature index, possibility revision);
drainage routing elevation and topology are fixed-point/integer from seed to
centimeters and execute under wasm in Node. Any
change that alters generated output must bump `WORLD_ALGORITHM_VERSION` — or,
for a single layer's tuning, that layer's `algorithm_revision` in the
declaration table — and update the golden fixtures in
[`crates/world-core/tests/determinism.rs`](crates/world-core/tests/determinism.rs)
in the same commit. See [ADR 0003](docs/adr/0003-deterministic-integer-hashing.md)
and [ADR 0008](docs/adr/0008-tiles-are-functions-of-their-dependency-hash.md).

## License

Dual-licensed under MIT or Apache-2.0.
