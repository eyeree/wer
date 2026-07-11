# 2. Workspace and crate boundaries

Date: 2026-07-10

## Status

Accepted

## Context

The project targets native desktop first but treats a browser/WebAssembly/WebGPU
build as a planned platform, not a late-stage port (plan sections 3 and 19). If
platform assumptions (filesystem access, native threads, sockets, native-only
GPU features) leak into core generation code, the wasm target rots and browser
support becomes a rewrite.

## Decision

Split the workspace into **platform-neutral** and **platform-specific** crates
from the start, matching the repository structure in plan section 5:

- Neutral: `world-core` (pure deterministic computation) and `world-runtime`
  (orchestration; expresses platform needs as abstract `Storage` / `TaskExecutor`
  traits). These must compile for `wasm32-unknown-unknown` continuously and may
  not touch the filesystem, spawn threads, open sockets, or call platform
  graphics APIs.
- Platform: `renderer` (wgpu/WGSL), `platform-native` (winit + native services),
  `platform-web` (wasm-bindgen + browser services), and `tools`.

CI checks the neutral crates for `wasm32` on every change.

## Consequences

- Browser divergence is caught immediately instead of at porting time.
- Some functionality must be threaded through trait objects rather than called
  directly, adding a small indirection cost that we accept.
- The dependency direction is enforced by review: neutral crates never depend on
  platform crates.
