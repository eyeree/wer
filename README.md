# Infinite World Exploration Game

A native and browser Rust/WebAssembly/WebGPU engine for an exploration game
built around **continuous travel through possibility space** — one seamless
journey across an infinite landscape of possible worlds, steered by *anchors*
the player collects.

See the design and architecture documents:

- [`docs/Infinite_World_Exploration_Project_Overview.md`](docs/Infinite_World_Exploration_Project_Overview.md) — the game vision.
- [`docs/plans/prototype/implementation-plan.md`](docs/plans/prototype/implementation-plan.md) — the high-level technical plan.
- [`docs/adr/`](docs/adr/) — architecture decision records.

The repository has landed Phases 1–6 plus the static Phase 7 browser viewer:
deterministic layered generation,
procedural ecosystems, resonance-gated steering, sparse persistence and atlas
sharing, performance/resource tiers, and a real streamed browser Map/POV/Split
runtime. Native and browser now use the same `viewer-host` behavior for actions,
one-traveler world updates, Map composition, inspection, panel data, and layout
([ADR 0028](docs/adr/0028-shared-viewer-host-and-one-world-multi-view.md)). The
measured performance ledger lives in
[`docs/plans/prototype/perf-baseline.md`](docs/plans/prototype/perf-baseline.md).
Post-Phase-6 Improvement A.8 makes macro routing elevation entirely fixed-point
and gives ordinary Terrain a realized-current 3×3 P/G halo plus a centered
Slope output (ADR 0027; Terrain and Drainage layer revisions are 1, world
version remains 2).

## Workspace layout

The authoritative world and viewer behavior is environment-neutral.
Cross-platform viewer/rendering crates compile for native and `wasm32`; winit,
DOM, storage, and executor APIs stay at platform boundaries, with only a narrow
wasm canvas-surface adapter in `renderer` (see
[ADR 0002](docs/adr/0002-workspace-crate-boundaries.md) and
[ADR 0028](docs/adr/0028-shared-viewer-host-and-one-world-multi-view.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| [`world-core`](crates/world-core) | neutral lib | Deterministic hashing, coordinates, possibility space, the layer graph and every environmental generator. |
| [`world-runtime`](crates/world-runtime) | neutral lib | Region streaming, convergence, dep-hash staleness, topological cost-budgeted dispatch; abstract storage & task interfaces. |
| [`renderer`](crates/renderer) | cross-platform GPU lib | wgpu/WGSL device, surfaces, Map atlas and POV rendering; records Map, POV, or Split as one multi-view frame. |
| [`pov-host`](crates/pov-host) | cross-platform presentation lib | Fly/walk camera, terrain and organism presentation geometry, chunk scheduling, and CPU-side POV picking. |
| [`viewer-host`](crates/viewer-host) | environment-neutral viewer lib | Normalized input, typed actions, the one-traveler controller, layout/focus, Map composition/atlas preparation, inspection, and semantic panel documents. |
| [`platform-native`](crates/platform-native) | bin `wer` | Thin winit adapter plus native executor, storage, bitmap panel, capture, and lifecycle services. |
| [`platform-web`](crates/platform-web) | cdylib+rlib and static shell | Thin wasm/DOM adapter, streamed viewer runtime, capability probes, DOM panel rendering, canvas lifecycle, and recovery; world jobs remain inline and vault effects remain unavailable. |
| [`tools`](crates/tools) | command-line bins | Inspectors, replay/sign-off harnesses, web build/serve/sign-off, lane executor, atlas tooling, and native file-tree storage. |

## Prerequisites

- Rust (stable) via [rustup](https://rustup.rs). The pinned toolchain and the
  `wasm32-unknown-unknown` target are declared in [`rust-toolchain.toml`](rust-toolchain.toml).
- Native GPU drivers with a Vulkan/Metal/DX12 backend (for running `wer`).
- The native viewer's default UI is the shared DOM overlay dock (the same
  toolbar and information panel the browser shell renders — see
  [`docs/wry-overlay/implementation-plan.md`](docs/wry-overlay/implementation-plan.md)).
  On Linux this needs the WebKitGTK/GTK development packages at build time:

  ```sh
  sudo apt install -y pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev
  ```

  On Windows the overlay uses the preinstalled WebView2 runtime; macOS
  needs nothing extra. `WER_OVERLAY=0` runs with the legacy bitmap panel
  and winit-only input instead (the benchmark-clean/recovery mode, and the
  automatic fallback when no X11/WebKitGTK is available); building with
  `--no-default-features` produces that minimal shell without the wry
  dependency at all.

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

# Headless POV/Split screenshots (offscreen GPU, ADR 0021): drive the fly/walk
# camera through a scripted sequence — pos:x,y[,z] | mouse:dx,dy |
# move:f[,r[,u]] | walk | fly | settle[:n] | size:WxH | snap:file.ppm |
# split:file.ppm. `split:` captures Map + POV + the shared panel/focus border.
cargo run --release --bin wer -- --pov-script \
  "pos:300,-10; walk; move:200; snap:walk-a.ppm; mouse:400,0; move:200; snap:walk-b.ppm"

# A fixed-size Split capture at an explicit resource tier.
WER_TIER=high cargo run --release --bin wer -- --pov-script \
  "size:1024x768; pos:0,0; split:aligned-split.ppm"

# Test everything, including determinism goldens and the continuity replay.
cargo test --workspace

# Generation & streaming benchmarks (sizes the per-frame budgets).
cargo bench -p world-core --bench generation
cargo bench -p world-runtime --bench update

# Continuously verify the shared runtime/viewer and web shell for wasm.
cargo check -p world-core -p world-runtime -p viewer-host -p platform-web --target wasm32-unknown-unknown

# Execute every deterministic parity probe as real wasm in Node.
wasm-pack test --node crates/platform-web

# Lints & formatting (as run in CI).
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

## Native Windows executable

CI builds the Windows desktop app from Linux as a native MSVC executable, not
with the MinGW/GNU target. The build uses
[`cargo-xwin`](https://github.com/rust-cross/cargo-xwin), which drives
`clang-cl`/`lld-link` against the Microsoft CRT and Windows SDK import libraries
that `xwin` downloads. This matches Rust's first-class Windows MSVC target and
is the preferred path for the wgpu/winit desktop app.

Local Linux build:

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --version 0.23.0 --locked
cargo xwin build --release --bin wer --target x86_64-pc-windows-msvc
```

The executable is written to
`target/x86_64-pc-windows-msvc/release/wer.exe`. CI uploads the same file as the
`wer-windows-x86_64-msvc` artifact.

## Driving the prototype

`cargo run --release --bin wer` opens the shared viewer in Map mode. Map, POV,
and side-by-side Split all follow one traveler and one post-update world state.
The information panel remains visible in every mode and reports frame/streaming
telemetry, presentation state, steering, persistence, warnings, and the Map or
POV feature under the pointer. In Split, the highlighted pane owns
view-scoped input; click a pane or press `Tab` to change focus.

Transformation is fueled by travel
([ADR 0006](docs/adr/0006-travel-fueled-convergence.md)): stand still and the
world holds steady — bias nudges and anchors set the *destination*, and the
world drifts toward it only as you move (sprinting drifts it faster).

| Input | Effect |
|-------|--------|
| `WASD` / arrows | Move in the focused view; Map translates the traveler, POV moves along the camera basis |
| `Shift` | Map: sprint. POV fly mode: descend (`Space` ascends) |
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
| Mouse wheel | Map: zoom x1–x16. POV: adjust movement speed. Split routes the wheel to the focused pane |
| Primary click | Focus that pane in Split; hover updates Map or POV inspection |
| Primary drag | Look in POV only while the primary button is held |
| `Tab` | In a single view, toggle Map/POV; in Split, focus the other pane |
| `F12` | Write `screenshot.ppm` and `state.txt` under `./dump/<UTC datetime>/`; Map, POV, and Split use the live shared layout and record mode, focus, panes, traveler/camera pose, hover, steering, telemetry, dependency hashes, and vault counters |
| `Esc` | Quit |

The possibility, anchor, route, load, channel, and overlay bindings in the
table apply when Map is focused; Save (`O`) remains global. When POV is focused,
`F` toggles walk/fly,
`B` toggles directional shadows and terrain AO, `N` toggles detail normals,
and `V` toggles water. Walk holds the camera at eye height over the rendered
terrain; fly also enables `Space`/left `Shift` vertical motion. Map organism
inspection becomes available at zoom x4 and above. POV inspection raycasts the
resident CPU terrain lattice and renderer-ready organism primitives—never GPU
readback.

Set `WER_VIEW=map|pov|split` to choose the native startup mode
(`WER_POV=1` remains a legacy POV alias). `WER_POV_RADIUS` (1–8, default 3)
sets the chunk radius, and `WER_POV_SCALE` (0.25–1.0) sets the internal POV
raster scale. For reproducible presentation measurements, also pin
`WER_WINDOW=WxH` and optionally set `WER_PRESENT_MODE=immediate`. `WER_START=x,y`
chooses the initial world position.

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
cargo run --bin web-signoff -- --assert-layout      # functional viewport/input/Split matrix
cargo run --bin web-signoff -- --profile-alignment  # local Low/Mid/High × Map/POV/Split diagnostics
cargo run --bin web-serve            # optional: [port] [dir], default 8080 target/web-dist
```

`web-serve` exists because browsers refuse ES modules and wasm from `file://`
URLs (CORS) — the viewer needs an HTTP origin. It serves loopback-only with
correct MIME types (including `application/wasm`) and the
cross-origin-isolation headers a future shared-memory worker backend requires,
which a generic `python3 -m http.server` does not send.

Open <http://localhost:8080> and check the console for
`[wer] wasm smoke ok — origin feature hash: …`. The viewer status bar also
prints the same hash. That value **must** match the native `wer-inspect 0 0`
output — the determinism guarantee the browser port depends on.

The toolbar selects Map, POV, or Split. Split focus is visible and routes
view-scoped keys/wheel to the clicked pane; `Tab` swaps focus while a view
surface owns keyboard focus. The Help page builds its control table at runtime
from the shared Rust action/binding descriptors. Use `?tier=low`, `?tier=mid`,
or `?tier=high` to pin the startup resource tier for diagnostics (the shipped
default is Low). The browser information dock keeps Inspection and Ecology in
their own scrollable panels; Inspection shows two label/value pairs per row and
keeps the last valid Map/POV sample while the pointer moves into the dock. Drag
the vertical dividers to resize adjacent panels, or the horizontal divider to
trade space between the dock and the Map/POV views; focus a divider and use its
arrow keys for the keyboard equivalent. The selected relative sizes survive
viewport resizing.

For headless functional debugging, inspect
`window.__viewerCharacterization()`, `window.__mapStatus`,
`window.__povStatus`, and `window.__rendererFrameStatus`. Headless Chrome can
verify DPR/layout, input, world-update counts, panel cadence, hover caching,
and fallback status, but it cannot validate visible WebGPU pixels; use the
Windows CDP path documented in [AGENTS.md](AGENTS.md) for GPU screenshots.

CI also pins `wasm-pack` 0.13.1 and executes the complete parity suite in Node,
including signed fixed routing elevations and three complete macro tiles.

### Static deployment

Publish `target/web-dist` as-is. The artifact is subpath-safe: pages link to
`./assets`, `./generated`, `./docs/world-model.html`, and `./help/` with
relative URLs, so GitHub Pages can serve it from a project path. There is no
server, socket, filesystem, or generated-world cache in the deployment. The
shell probes IndexedDB, but browser vault/session effects are still reported as
unavailable rather than claiming durable persistence.

Low/Mid/High are resource presets, independent of browser capabilities:

| Tier | Runtime budget and presentation density |
|------|-------------------------------------------|
| Low | Phase 5/default radii, one organism slot per cell, 48 MiB field-cache ceiling |
| Mid | Wider streaming window, two presentation slots per cell, 96 MiB field-cache ceiling |
| High | Widest streaming window, four presentation slots per cell, 160 MiB field-cache ceiling |

WebGPU availability independently selects GPU Map/POV support; without it the
viewer uses CPU Map and reduces POV/Split to Map with a warning. World jobs
currently remain on `InlineExecutor`: the browser Worker and IndexedDB code are
capability probes, not completed task or vault backends. These limitations are
visible in the status/info panel and do not change settled hashes.

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
