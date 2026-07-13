# Graphics library and shader candidates

Date: 2026-07-12

This is a research note for future graphics work in the Infinite World
Exploration Game. The current renderer is intentionally small: `crates/renderer`
owns `wgpu`/WGSL presentation, native shell code packages world data for upload,
and ADR 0017 forbids GPU readback into authoritative state. Any graphics
dependency must preserve that boundary: presentation code may become richer, but
world identity, generation, steering, persistence, and sharing cannot depend on
GPU-computed values.

## Recommendation

Do not adopt a full Rust game engine as the renderer. Keep the existing
`renderer` crate on direct `wgpu`, then selectively borrow:

1. **Bevy as a reference implementation and source of shader patterns**, not as
   a runtime dependency. Its renderer, PBR, glTF, animation, mesh, texture,
   batching, culling, fog, SSAO, and WGSL modules are the most complete Rust
   `wgpu` example set, but importing Bevy would bring an ECS, asset system,
   scheduler, and render graph that overlap the existing architecture.
2. **`gltf` for imported mesh/material/skin/animation asset interchange** when
   external assets enter the project. It is format-focused and does not force a
   renderer architecture.
3. **A small mesh/texture support stack** around `meshopt`, `image`, `ktx2`,
   and possibly `basis-universal`, added only when the corresponding asset path
   exists.
4. **Shader tooling before shader abstraction**: keep authored shaders as WGSL,
   validate with `naga` as CI already does, and consider `naga_oil` or WESL only
   once duplicated shader code becomes a maintenance problem.

For near-term procedural terrain, water, and organism rendering, direct `wgpu`
plus project-owned WGSL is still the right shape. The useful external material is
mostly shader code, format readers, layout helpers, and examples of how to
organize larger pipelines.

## Fit criteria

- Must run through `wgpu` on native and browser/WebGPU targets.
- Must not add graphics, filesystem, threads, sockets, or platform APIs to
  `world-core` or `world-runtime`.
- Must tolerate derived-presentation-only GPU work. GPU output must not be read
  back into hashes, persistence, gameplay, steering, or generation.
- Should support or inform these future needs:
  - procedural terrain mesh generation and LOD,
  - texture loading/generation/compression,
  - glTF-style meshes, materials, skins, morph targets, and animation clips,
  - animation evaluation/execution for many organisms,
  - reusable WGSL for lighting, fog, water, skinning, instancing, post-processing,
    and debugging.

## Candidates

| Candidate | What it offers | Fit for this project | Risk / caveat |
| --- | --- | --- | --- |
| `wgpu` examples | Official Rust examples for buffers, textures, compute, HDR, instancing, and platform behavior. | Best baseline for idiomatic API usage and version migrations. | Examples are building blocks, not a renderer or asset pipeline. |
| Bevy renderer / PBR / animation / glTF | Large production Rust renderer built on `wgpu`; has meshes, textures, cameras, lights, shadows, glTF loading, custom shaders, render graph, skeletal animation, blending, morph targets, and substantial WGSL. | Best reference source. Mine shader structure, bind-group layout ideas, PBR/fog/water/skinning approaches, and asset pipeline lessons. | Poor dependency fit as an engine: ECS, scheduler, asset server, scene model, and render graph would compete with the existing architecture. Be careful with license headers if copying code. |
| `rend3` | Standalone `wgpu` 3D renderer with glTF and animation-related crates in its repository. | Historically close to “renderer library, not whole engine”; useful for design study. | Archived/read-only as of 2025 and marked maintenance mode. Do not build future architecture on it. |
| `gltf` crate | glTF 2.0 loader with modules for accessors, meshes, materials, textures, skins, and animations; can import from slices for non-filesystem asset sources. | Strong fit for future external asset interchange while keeping renderer ownership local. | It parses data; it does not execute animation, upload GPU buffers, or choose material shaders for us. |
| `meshopt` crate | Rust bindings for meshoptimizer-style remapping, vertex-cache optimization, overdraw optimization, simplification, stripification, clustering, packing, and encoding. | Good for imported/generated mesh post-processing and future LOD/meshlet experiments. | Native build dependency surface; evaluate wasm story before browser use. For deterministic world identities, use only on presentation meshes or versioned imported assets. |
| `image` crate | Safe Rust image encoding/decoding and image buffers for common formats. | Good default for debug assets, screenshots, generated texture baking, and uncompressed runtime texture input. | Broad default features can pull format dependencies; add with explicit feature choices. |
| `ktx2` crate | Parser/validator for KTX2 texture containers and mip levels. | Good candidate for GPU-ready texture assets, especially once browser delivery matters. | Parser only; transcode/compression decisions still need separate tooling/runtime support. |
| `basis-universal` crate | Bindings for Basis Universal texture encoding/transcoding into GPU-friendly compressed formats. | Useful if runtime texture transcoding becomes important across desktop/browser GPUs. | C++/FFI dependency and older crate surface; prefer offline asset tooling unless runtime transcoding is required. |
| `encase` | Compile-time checked WGSL host-shareable buffer layout and uniform/storage buffer writers. | Good fit once uniforms/storage structs grow beyond small hand-audited POD layouts. | Adds a layout abstraction; current simple `bytemuck` use is fine until layouts become complex. |
| `bytemuck` | Plain-data casting for GPU uploads; already used here. | Keep using for simple vertex/uniform structs with explicit `repr(C)` and no padding surprises. | Does not understand WGSL alignment rules by itself. For complex nested uniforms, use `encase` or exhaustive tests. |
| `naga_oil` | WGSL composition/import tooling from the Bevy ecosystem. | Good candidate if shared WGSL modules become needed before WESL stabilizes in this codebase. | Ties shader authoring to a preprocessor-like tool; defer until duplication justifies it. |
| WESL | Community “enhanced WGSL” language/tooling that compiles to WGSL, with build-time workflows. | Worth watching for modular shader libraries and reusable includes. | Adds a language layer above WGSL. Use only if the shader corpus becomes large enough to require modules. |
| WebGPU Fundamentals | Large tutorial/reference with WGSL examples for cameras, lighting, textures, skyboxes, post-processing, compute, and debugging. | Excellent shader and API reference because concepts map directly to `wgpu`. | JavaScript examples need translation to Rust/wgpu binding patterns. |
| WebGPU Samples | Official-style sample repository with meshes, shaders, and WebGPU demos. | Useful source of portable WGSL techniques, especially for browser parity. | TypeScript-first; examples are not Rust integration code. |
| Learn Wgpu | Rust/wgpu tutorial covering surfaces, pipelines, buffers, textures, cameras, depth, model loading, lights, normal mapping, HDR, compute, and memory layout. | Good onboarding and sanity-check reference for project-owned renderer code. | Tutorial code is educational and may lag latest project architecture choices. |

## Candidates to avoid as primary dependencies

- **Full engines as runtime dependencies** (`bevy`, Fyrox-style engines, etc.).
  They solve more than rendering and would force architecture decisions around
  ECS, asset lifetimes, scene ownership, scheduling, input, and serialization.
  The project already has strong crate boundaries and deterministic world
  lifecycle rules.
- **Archived or maintenance-mode renderers** as foundations. They can still
  teach useful patterns, but should not become dependencies for a long-lived
  browser-targeted renderer.
- **Non-WGSL-first shader stacks** unless there is a specific asset requirement.
  `wgpu` can accept GLSL/SPIR-V with features, but WebGPU itself is WGSL-only;
  keeping authored shaders in WGSL keeps the browser path direct.

## Shader sources to mine first

1. **Current project shaders**: keep expanding `renderer/shaders/*.wgsl` as the
   authoritative style. These already validate in CI and follow ADR 0017.
2. **Bevy WGSL**: inspect PBR, fog, shadows, clustered lighting, skinning,
   morph targets, prepass, meshlet, SSAO, atmosphere, and tonemapping modules.
   Treat them as implementation references and port the smallest needed pieces.
3. **`wgpu` examples**: use for API transitions, HDR/surface handling, texture
   upload details, compute, and validation-compatible patterns.
4. **WebGPU Fundamentals and WebGPU Samples**: use for clean, portable WGSL
   examples of water-ish effects, skyboxes, environment maps, post-processing,
   LUTs, compute patterns, and debugging.
5. **Learn Wgpu**: use for Rust-side wiring examples when adding standard
   renderer features such as depth, texture bind groups, cameras, lights, and
   model loading.

## Mesh generation and imported geometry

Near-term terrain and organism geometry should stay project-owned:

- Terrain meshes are deterministic CPU sampling of existing world surfaces,
  uploaded as derived presentation.
- Organisms can start as instanced procedural primitives driven by stable
  organism IDs and expressed traits. Slot 0 identity constraints remain in
  world/runtime; extra presentation detail remains renderer-side.
- Generated mesh buffers should be versioned as presentation cache entries, not
  as authoritative world data.

For imported or more complex assets:

- Use `gltf` as the interchange reader for meshes, skins, morph targets,
  materials, and animation clips.
- Convert glTF primitives into project-owned vertex/index buffers and material
  descriptors at the platform or asset-loading boundary.
- Use `meshopt` after generation/import for vertex/index optimization and future
  LOD/simplification, but do not let meshopt output become world identity.
- Keep any filesystem-backed asset loader outside neutral crates. For browser
  Phase 7, prefer byte-slice import paths so assets can come from fetch, embed,
  or a browser storage backend later.

## Texture generation and loading

The likely progression:

1. Continue procedural colors and CPU-generated small textures where possible.
2. Add `image` for ordinary PNG/JPEG/WebP-style source assets and generated
   texture baking.
3. Add KTX2 as the delivery container for mipmapped GPU textures when the asset
   pipeline exists.
4. Evaluate Basis Universal for offline compression first. Runtime transcoding is
   only worth its FFI cost if the project needs one compressed texture payload
   that adapts to heterogeneous GPU formats at load time.

For world-derived textures, keep the same rule as geometry: CPU/world state is
authoritative; GPU texture generation is visual-only and not read back.

## Animation generation and execution

There are two different problems:

- **Imported animation**: glTF channels, skins, joints, inverse bind matrices,
  and morph targets. `gltf` can read these. The project still needs its own
  evaluator that samples clips, blends channels, computes joint matrices, and
  uploads pose palettes.
- **Procedural organism animation**: likely better generated from species traits,
  body form, activity, aggression, speed, and terrain/water context than from
  asset clips. This should start as deterministic CPU-side presentation state
  keyed by stable organism identity plus time/session state, then uploaded for
  instanced rendering.

Execution options:

- Start with CPU animation evaluation and GPU vertex skinning. This is simple,
  browser-compatible, and keeps GPU output presentation-only.
- Use texture-buffer or storage-buffer pose palettes for many organisms once
  counts rise. Bind layout should be designed with WebGPU limits in mind.
- Avoid GPU-computed animation state feeding gameplay. If GPU animation becomes
  necessary for scale, it remains a visual approximation under ADR 0017.

Bevy is the best reference for skeletal animation, blending, morph targets, and
glTF integration. It is still more useful as a pattern library than as a direct
engine dependency.

## Practical next steps

1. Add a `docs/plans/prototype/graphics-pipeline-plan.md` only when we are ready
   to implement the next rendering phase; keep this file as the candidate list.
2. Before adding dependencies, prototype terrain/water/organism rendering with:
   - direct `wgpu`,
   - WGSL under `crates/renderer/shaders`,
   - `bytemuck` for simple buffers,
   - current `naga` shader validation in CI.
3. When shader duplication appears, evaluate `naga_oil` first because it is
   already used by Bevy and targets WGSL composition directly; evaluate WESL if
   the team wants a more language-level module system.
4. When external assets appear, add `gltf` behind a platform/asset boundary and
   convert into project-owned upload structs.
5. When textured materials become important, add `image` with explicit features,
   then evaluate KTX2/Basis as an asset-pipeline decision.

## Sources checked

- `wgpu` docs: <https://docs.rs/wgpu/latest/wgpu/>
- `wgpu` examples: <https://github.com/gfx-rs/wgpu/tree/trunk/examples>
- Bevy website: <https://bevy.org/>
- Bevy renderer/PBR/glTF source trees:
  <https://github.com/bevyengine/bevy/tree/main/crates/bevy_render>,
  <https://github.com/bevyengine/bevy/tree/main/crates/bevy_pbr>,
  <https://github.com/bevyengine/bevy/tree/main/crates/bevy_gltf>
- `rend3`: <https://github.com/BVE-Reborn/rend3>
- `gltf`: <https://docs.rs/gltf/latest/gltf/>
- `meshopt`: <https://docs.rs/meshopt/latest/meshopt/>
- `image`: <https://docs.rs/image/latest/image/>
- `ktx2`: <https://docs.rs/ktx2/latest/ktx2/>
- `basis-universal`: <https://docs.rs/basis-universal/latest/basis_universal/>
- `bytemuck`: <https://docs.rs/bytemuck/latest/bytemuck/>
- `encase`: <https://docs.rs/encase/latest/encase/>
- `naga_oil`: <https://docs.rs/naga_oil/latest/naga_oil/>
- WESL: <https://docs.rs/wesl/latest/wesl/>
- WebGPU Fundamentals: <https://webgpufundamentals.org/>
- WebGPU Samples: <https://github.com/webgpu/webgpu-samples>
- Learn Wgpu: <https://sotrh.github.io/learn-wgpu/>
