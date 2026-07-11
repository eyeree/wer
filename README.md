# Infinite World Exploration Game

A native Rust engine (with a planned browser/WebAssembly/WebGPU target) for an
exploration game built around **continuous travel through possibility space** —
one seamless journey across an infinite landscape of possible worlds, steered by
*anchors* the player collects.

See the design and architecture documents:

- [`Infinite_World_Exploration_Project_Overview.md`](Infinite_World_Exploration_Project_Overview.md) — the game vision.
- [`implementation-plan.md`](implementation-plan.md) — the high-level technical plan.
- [`docs/adr/`](docs/adr/) — architecture decision records.

This repository is currently at **Phase 0** (architecture and technical spikes):
a compiling workspace with deterministic core primitives, a native window shell,
and a wasm smoke target.

## Workspace layout

Platform-neutral crates compile for both native and `wasm32`; platform crates
hold everything OS/browser-specific (see
[ADR 0002](docs/adr/0002-workspace-crate-boundaries.md)).

| Crate | Kind | Responsibility |
|-------|------|----------------|
| [`world-core`](crates/world-core) | neutral lib | Deterministic hashing, coordinates, possibility space. |
| [`world-runtime`](crates/world-runtime) | neutral lib | Region lifecycle, abstract storage & task interfaces. |
| [`renderer`](crates/renderer) | native/gpu lib | wgpu/WGSL renderer (clear-screen for now). |
| [`platform-native`](crates/platform-native) | bin `wer` | winit window + event loop; native services. |
| [`platform-web`](crates/platform-web) | cdylib | wasm-bindgen smoke shell (grows into the browser runtime). |
| [`tools`](crates/tools) | bin `wer-inspect` | Inspectors, validators, replay tools. |

## Prerequisites

- Rust (stable) via [rustup](https://rustup.rs). The pinned toolchain and the
  `wasm32-unknown-unknown` target are declared in [`rust-toolchain.toml`](rust-toolchain.toml).
- Native GPU drivers with a Vulkan/Metal/DX12 backend (for running `wer`).

## Common commands

```sh
# Build & run the native application shell (opens a window, clears the frame).
cargo run --bin wer

# Deterministic inspector: world position -> region + feature hash.
cargo run --bin wer-inspect -- 300 -10

# Test everything, including the determinism golden fixtures.
cargo test --workspace

# Continuously verify the core still compiles for the browser target.
cargo check -p world-core -p platform-web --target wasm32-unknown-unknown

# Lints & formatting (as run in CI).
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
```

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
(world version, region coordinate, layer, feature index, possibility revision).
Any change to a generation algorithm must bump `WORLD_ALGORITHM_VERSION` and
update the golden fixtures in
[`crates/world-core/tests/determinism.rs`](crates/world-core/tests/determinism.rs).
See [ADR 0003](docs/adr/0003-deterministic-integer-hashing.md).

## License

Dual-licensed under MIT or Apache-2.0.
