# Infinite World Exploration Game — High-Level Implementation Plan

## 1. Purpose

This document defines the high-level implementation approach for the Infinite World Exploration Game.

The project began as a **native Rust application** with a custom engine
architecture designed for **browser/WebAssembly/WebGPU** portability. Both
shells now run the same world model and shared viewer contracts; native and web
remain separate environment adapters.

This plan is intentionally architectural and phased. Lower-level implementation plans will be created before work begins on each subsystem.

---

## 2. Core Technical Goals

The implementation must support:

- An effectively infinite, deterministic world.
- Continuous travel through both physical space and possibility space.
- Seamless transformation of distant regions while nearby regions remain stable.
- Procedural generation driven by hierarchical environmental systems.
- Anchor-driven steering of future world generation.
- Dense ecosystems represented at multiple levels of abstraction.
- Sparse persistence for discoveries, routes, preserves, and player-caused changes.
- Scalable execution across native desktop and browser environments.
- A CPU-authoritative world model with optional GPU acceleration for dense parallel workloads.

The system must prioritize:

- Determinism.
- Data-oriented design.
- Incremental recomputation.
- Clear cache and dependency boundaries.
- Temporal budgeting.
- Portability.
- Profilability.
- Graceful degradation across hardware tiers.

---

## 3. Platform Strategy

### 3.1 Native Foundation

The native desktop implementation remains the primary profiling and deep-debug
environment.

Native development favors:

- Fast iteration.
- Strong profiling and debugging.
- Full filesystem access.
- Native threading.
- Easier experimentation with low-level engine architecture.
- A controlled performance environment.

### 3.2 Browser Runtime

The browser viewer/runtime slice is delivered as a static artifact after
validation of the native foundation. Streamed world updates, Map/POV/Split,
input, panel, and recovery are live; the serializable worker executor and
durable browser vault backend remain Phase 7 work.

The browser target will use:

- WebAssembly for core simulation and generation.
- WebGPU for rendering and selected compute workloads.
- Web Workers for parallel execution.
- Shared memory where available.
- Browser-compatible persistence such as IndexedDB or OPFS.
- A TypeScript or minimal JavaScript shell for browser integration.

Browser support is a first-class platform, not a divergent port. Core world,
viewer, and renderer contracts compile for both targets; environment services
stay at platform boundaries. `renderer` contains the narrow wasm canvas-surface
adapter required to hand an `HtmlCanvasElement` to `wgpu`.

---

## 4. Proposed Technology Stack

### 4.1 Native Runtime

- Rust.
- `wgpu` for rendering and compute abstraction.
- WGSL shaders.
- `winit` or equivalent for window and event management.
- A Rust ECS or custom data-oriented entity store.
- The priority-lane `TaskExecutor` implementation shared with sign-off tools.
- Per-pass timing and committed performance ledgers.
- `serde` with versioned formats for serialization.
- A native bitmap information-panel renderer over shared semantic panel data.

### 4.2 Browser Runtime

- `wasm32-unknown-unknown`.
- `wasm-bindgen`.
- WebGPU through `wgpu`.
- Web Worker capability probing; a serializable worker executor remains open.
- `SharedArrayBuffer` where cross-origin isolation is available.
- IndexedDB capability probing; durable vault/session effects remain open.
- Minimal JavaScript for DOM/canvas integration and the application shell.

---

## 5. High-Level Repository Structure

The project should be divided into platform-neutral and platform-specific crates from the beginning.

```text
world-core/
    deterministic math
    coordinate hashing
    possibility-space model
    procedural generation
    environmental layers
    procedural genetics
    ecology
    spatial structures
    route representation
    persistence schemas

world-runtime/
    region lifecycle
    streaming policy
    dependency management
    generation scheduling
    simulation coordination
    resource budgeting
    abstract storage interface
    abstract task execution interface

renderer/
    cross-platform wgpu device and surface renderer
    WGSL shaders
    GPU map atlas and refinement
    POV terrain, organisms, water, and shadows
    one-surface Map / POV / Split frame recording

pov-host/
    fly and walk camera
    pure terrain and organism presentation geometry
    chunk and organism scheduling
    CPU-side POV picking

viewer-host/
    normalized input and typed semantic actions
    one-traveler exploration/view controller
    Map / POV / Split layout and focus
    CPU map composition and GPU-atlas preparation
    CPU-authoritative inspection
    semantic information-panel documents

platform-native/
    winit input/window adapter
    native file storage and lane executor
    bitmap panel and file-bound debug capture
    surface and application lifecycle

platform-web/
    wasm-bindgen and DOM input adapter
    canvas/DPR integration
    Worker and IndexedDB capability probes
    unavailable worker/vault effect reporting
    DOM panel renderer
    surface and lifecycle recovery

tools/
    world inspectors
    procedural visualizers
    profiling tools
    atlas tools
    deterministic replay tools
    validation tools
    web build / serve / sign-off tools
```

The authoritative platform-neutral crates must avoid direct dependencies on
native filesystem APIs, native thread creation, sockets, platform-specific
graphics backends, or browser APIs. `viewer-host` is environment-neutral under
ADR 0028: it may depend on the cross-platform world, POV, and renderer crates,
but never winit, DOM/browser APIs, filesystems, sockets, or platform thread
creation. `renderer` does not depend back on `viewer-host`.

---

## 6. Core Architectural Principles

## 6.1 CPU-Authoritative World Model

The CPU will define the authoritative state of the world.

The CPU owns:

- Possibility-space state.
- Anchor evaluation.
- Region state.
- Procedural dependency resolution.
- Stable feature identities.
- Ecological graphs.
- Local simulation state.
- Persistence.
- Route graphs.
- Multiplayer-relevant state.
- Cache invalidation.

The GPU will be used as a high-throughput evaluator for derived dense data where useful.

The GPU may perform:

- Dense field refinement.
- Noise synthesis.
- Diffusion and relaxation.
- Distance-field generation.
- Candidate scoring.
- High-resolution ecology distribution.
- Procedural geometry generation.
- Rendering.

Authoritative world state must not depend exclusively on native-only GPU features or synchronous GPU readback.

Presentation behavior is shared without becoming world authority. Normalized
input and typed actions enter one `viewer-host` controller tick, which computes
travel and updates `RegionMap` once. Map, POV, and Split are derived panes of
that same post-update state. The renderer records every visible pane in one
surface acquire/submit/present and exposes no live readback; CPU Map data and
resident POV presentation geometry supply picking and information panels.

---

## 6.2 Deterministic Generation

World generation must be based on stable inputs rather than sequential random streams.

A generated feature should be reproducible from values such as:

```text
world version
region coordinate
generator layer
feature index
possibility state revision
```

Persistent identities should use:

- Integer hashing.
- Stable coordinate systems.
- Versioned procedural algorithms.
- Explicit quantization.
- Stable iteration order.
- Portable pseudorandom number generators.

Floating-point values may be used for approximate simulation and presentation, but permanent feature identities must not depend on unstable floating-point behavior.

---

## 6.3 Hierarchical World Representation

The world will be represented at several spatial and conceptual levels.

```text
Macro regions
    geology
    large-scale climate
    drainage topology
    major landmarks

World regions
    local climate
    hydrology expression
    soils
    biome
    aggregate ecology
    possibility state

Local cells
    feature candidates
    vegetation clusters
    organism populations
    interactive entities
```

Each layer has its own:

- Resolution.
- Update frequency.
- Persistence requirements.
- Cache lifetime.
- Dependency set.
- Transformation rules.

---

## 6.4 Realized State and Target State

Regions must distinguish between:

- Their currently realized possibility state.
- Their target possibility state.

Nearby regions remain stable while more distant regions gradually converge toward the target state.

Each region should track:

- Region identity.
- Current state.
- Target state.
- Stability.
- Revision.
- Dirty procedural layers.
- Generation status.
- Cached aggregate fields.
- Persistent overrides.

This enables continuous transformation without globally regenerating the world.

---

## 6.5 Incremental Dependency Graph

Procedural generation will be implemented as explicit dependent layers.

```text
Possibility state
    ↓
Climate
    ↓
Geology expression
    ↓
Hydrology
    ↓
Soils
    ↓
Biome
    ↓
Aggregate ecology
    ↓
Species distribution
    ↓
Feature candidates
    ↓
Interactive entities
```

Each layer should produce:

- Output data.
- A dependency hash.
- A revision.
- A dirty scope.
- One or more level-of-detail representations.

Changes to high-level aesthetic or organism traits should not invalidate terrain, hydrology, or other unrelated layers.

---

## 6.6 Temporal Budgeting

The engine must not regenerate every affected region immediately.

All major subsystems should operate within explicit per-frame or per-second budgets.

Examples:

```text
possibility updates
climate generation
ecology generation
feature realization
persistence
route updates
local simulation
```

Work should be prioritized using factors such as:

- Distance from the player.
- Visibility.
- Screen-space importance.
- Direction of travel.
- Likelihood of entering the region.
- Magnitude of possibility change.
- Player interaction history.
- Route or preserve importance.
- Estimated generation cost.

Jobs may span multiple frames and must be safe to cancel or supersede.

---

## 7. Possibility-Space Model

The world will not be represented as a set of discrete world seeds.

Instead, each realized location is generated from a continuous, hierarchical possibility state.

Possible dimensions include:

- Temperature.
- Precipitation.
- Seasonality.
- Atmospheric density.
- Ocean fraction.
- Tectonic activity.
- Erosion strength.
- Soil fertility.
- Vegetation density.
- Morphological tendencies.
- Color tendencies.
- Ecological aggression.
- Bioluminescence.
- Behavioral traits.

Possibility dimensions should be grouped by domain:

```text
planetary
climate
geology
hydrology
ecology
morphology
behavior
aesthetics
```

The possibility field should vary spatially. A sparse lattice, adaptive quadtree, or other bounded-query structure should be used to interpolate local possibility state.

---

## 8. Anchors

Anchors are procedural constraints and biases rather than copied assets.

Each anchor should capture:

- A trait target.
- A trait mask.
- Strength.
- Polarity.
- Scope.
- Falloff.
- Source metadata.

Anchors may:

- Emphasize traits.
- Leave traits neutral.
- Suppress traits.

Multiple anchors combine into a steering vector that modifies the target possibility state.

The result must be projected through plausibility constraints so that invalid combinations do not directly create incoherent worlds.

Examples of constraints include:

- Vegetation density versus rainfall.
- Animal scale versus primary productivity.
- Canopy height versus soil depth and wind exposure.
- Ice versus temperature.
- Wetland formation versus hydrology.

The first implementation should use rule-based constraints and iterative relaxation rather than machine learning.

---

## 9. Environmental and Ecological Layers

Environmental generation should follow a hierarchical sequence:

1. Climate.
2. Geology.
3. Hydrology.
4. Soils.
5. Biome classification.
6. Aggregate vegetation.
7. Food-web structure.
8. Species distributions.
9. Local variation.
10. Interactive organisms.

The implementation should favor ecological plausibility over scientific simulation.

Major topology such as mountain ranges and river networks should be highly stable.

Possibility drift should more commonly modify:

- River width.
- Surface wetness.
- Vegetation.
- Marsh extent.
- Weather.
- Species expression.
- Canopy density.
- Color.
- Atmospheric conditions.

This reduces expensive global recomputation.

---

## 10. Aggregate Fields and Entity Realization

The engine should represent ecology as aggregate fields before representing it as individual entities.

Example fields:

- Biomass.
- Moisture.
- Canopy height.
- Canopy coverage.
- Dominant species.
- Species entropy.
- Herbivore pressure.
- Predator pressure.
- Fungal activity.
- Bioluminescence.

At different scales:

```text
Far distance
    aggregate terrain and ecology fields

Mid distance
    vegetation clusters
    approximate organism groups

Near distance
    stable feature identities
    interactive entities
    local simulation
```

Refinement should preserve aggregate quantities.

For example, a region with 70% canopy coverage at low resolution should resolve into near-field vegetation with approximately the same coverage.

---

## 11. Procedural Genetics

Organisms will be generated from stable procedural identities plus environmental and possibility-space inputs.

A conceptual organism expression is:

```text
stable genome
+ local ecology
+ possibility bias
+ lifecycle state
= realized organism
```

Separate genome domains may include:

- Appearance.
- Behavior.
- Ecological niche.

World drift should not require morphing every organism.

Supported transformation strategies should include:

- Continuous parameter changes.
- Growth-mediated changes.
- Lifecycle replacement.
- Offscreen replacement.
- Distance-based regeneration.
- Ecological succession.

Only nearby interactive organisms require full entity state.

---

## 12. Spatial Data Structures

The engine will likely use several specialized structures.

### 12.1 Region Quadtree or Sparse Grid

Stores:

- Region state.
- Possibility control points.
- Aggregate environment.
- Persistence metadata.
- Route information.
- Discovery status.

### 12.2 Terrain and Ecology Clipmaps

Stores multiresolution fields for:

- Height.
- Slope.
- Curvature.
- Moisture.
- Soil.
- Biomass.
- Canopy.
- Species mix.
- Morphology.

### 12.3 Local Spatial Index

A hashed grid, loose quadtree, or BVH for active local features and organisms.

### 12.4 Sparse Persistent Feature Store

Stores only deviations from deterministic generation.

Examples:

- Named features.
- Photographed discoveries.
- Preserved regions.
- Modified terrain.
- Dead or replaced organisms.
- Shared route markers.
- Player-built structures.

---

## 13. Routes Through Possibility Space

A route consists of both physical coordinates and possibility-space coordinates.

A route node may contain:

```text
physical position
possibility signature
transition cost
usage count
anchor signature
region stability
```

Frequently used routes should create a soft attraction field rather than forcing exact replay.

The route system should support:

- Following known expeditions.
- Searching for target ecosystems.
- Community-discovered corridors.
- Route difficulty.
- Shared exploration.
- Persistent paths through possibility space.

---

## 14. Transition Movement and Resonance

Reality-transition movement will be slower and more deliberate than local travel.

The player orb will interact with nearby environmental features through a transient resonance graph.

Graph nodes may include:

- Plants.
- Rocks.
- Terrain formations.
- Organisms.
- Atmospheric phenomena.
- Underground or hidden structures.

Transition capability may depend on:

- Nearby feature density.
- Feature diversity.
- Distance.
- Compatibility with active anchors.
- Line of sight.
- Environmental prominence.

This graph should be generated locally using spatial queries. It should not be stored as a global graph.

---

## 15. Memory and Performance Strategy

The native implementation should target a compact data-oriented world model.

Priorities:

- Structure-of-arrays layouts.
- Region-local arenas.
- Slab allocators.
- Handles instead of pervasive pointers.
- Packed integer fields.
- Quantized values.
- Minimal object overhead.
- Shared immutable descriptors.
- Explicit cache ownership.
- Limited duplicate CPU/GPU data.

Expected high-level memory targets, excluding rendering assets:

```text
possibility metadata
aggregate field caches
local feature records
active simulation entities
spatial indexes
temporary generation buffers
persistent loaded overrides
```

A reasonable initial desktop target is a total authoritative world working set in the low hundreds of megabytes.

The system should support scalable cache sizes rather than assuming a fixed maximum world complexity.

---

## 16. Concurrency and Job Scheduling

The native runtime should use a job system with coarse region and layer tasks.

Example generation flow:

```text
PossibilityUpdate
    ↓
ClimateField
    ↓
SoilField
    ↓
BiomeField
    ↓
EcologyField
    ↓
FeatureCandidates
    ↓
EntityRealization
```

Each job should include:

- Region ID.
- Layer ID.
- Input revision.
- Output revision.
- Dependency requirements.
- Priority.
- Estimated cost.
- Cancellation or supersession state.

The initial native implementation may use Rayon or another existing task library.

The core runtime must not depend directly on a specific scheduler. An abstract task execution interface should allow a later Web Worker implementation.

---

## 17. GPU Compute Strategy

GPU compute is optional for the first world-model prototype.

The first implementation should remain CPU-first and profile-driven.

Likely future GPU compute candidates include:

- Dense climate fields.
- Dense ecology fields.
- High-resolution noise.
- Diffusion.
- Relaxation.
- Erosion approximations.
- Distance fields.
- Candidate spawn scoring.
- High-resolution vegetation distribution.

A useful dual-resolution model is:

```text
CPU authoritative field
    low or medium resolution
    deterministic
    used for gameplay and persistence

GPU derived field
    high resolution
    used for visual refinement
    preserves CPU-level constraints and averages
```

The engine should avoid synchronous GPU readback in normal generation flows.

---

## 18. Persistence

The world is infinite and must not be fully stored.

The persistence layer should store:

- World and algorithm version.
- Player position and possibility state.
- Active anchors.
- Discovered regions.
- Persistent feature overrides.
- Shared routes.
- Named species and landmarks.
- Preserves.
- Community atlas metadata.
- Expedition journals.

Generated base world data should be reconstructed deterministically.

Persistence formats must be:

- Versioned.
- Forward-migratable where practical.
- Independent of native pointers.
- Compatible with browser storage.
- Safe for partial loading.

---

## 19. Browser Portability Requirements

The following constraints apply from the beginning:

- `world-core` must compile for WebAssembly continuously.
- Core algorithms must avoid direct filesystem access.
- Storage APIs must be asynchronous or abstracted.
- Core algorithms must avoid native thread creation.
- SIMD-specialized code must be isolated behind portable interfaces.
- Shaders should use WGSL.
- Renderer bindings should remain WebGPU-compatible.
- Persistent IDs must not depend on platform-specific floating-point results.
- Generation jobs must be resumable and interruptible.
- Large monolithic memory allocations should be avoided.
- Platform-specific acceleration must have portable fallbacks.

Continuous integration should include at least:

```text
native cargo check
wasm32 cargo check
core unit tests
determinism tests
serialization compatibility tests
```

A minimal browser smoke test should be created early, before major native-only assumptions accumulate.

---

## 20. Development Phases

## Phase 0 — Architecture and Technical Spikes

Goals:

- Validate Rust, `wgpu`, and project structure.
- Establish deterministic hashing and region coordinates.
- Validate native and Wasm compilation.
- Benchmark basic field generation.
- Establish profiling.
- Decide initial data layouts.
- Define subsystem interfaces.

Deliverables:

- Empty native application.
- Minimal WebGPU browser smoke test.
- Core crate compiling for native and Wasm.
- Initial benchmark harness.
- Architecture decision records.

---

## Phase 1 — Continuous World Transformation Prototype

Goals:

- Validate the core illusion of a continuously transforming world.
- Avoid detailed organism systems.

Scope:

- Infinite deterministic heightfield.
- Small possibility vector.
- Sparse possibility field.
- Stable near radius.
- Transforming distant radius.
- Basic climate and ecology fields.
- One or two anchor types.
- Region streaming.
- Debug visualization.
- Incremental regeneration.

Success criterion:

The player can move and change possibility state without noticing obvious chunk replacement or landmark contradiction.

---

## Phase 2 — Layered Environmental Generation

Goals:

- Establish the dependency graph.
- Separate stable and dynamic world layers.
- Validate cache invalidation.

Scope:

- Climate.
- Geology expression.
- Hydrology.
- Soils.
- Biomes.
- Aggregate vegetation.
- Region dependency hashes.
- Layer-specific revisioning.
- Temporal generation budgets.

Success criterion:

Changes only recompute relevant layers, and world generation remains stable and reproducible.

---

## Phase 3 — Procedural Genetics and Ecology

Goals:

- Add organism-level richness without simulating everything.

Scope:

- Procedural genomes.
- Species archetypes.
- Aggregate populations.
- Food-web graphs.
- Ecological plausibility constraints.
- Near-field organism realization.
- Lifecycle and succession replacement.
- Stable organism identities.

Success criterion:

The world produces diverse but internally coherent ecosystems that respond meaningfully to possibility changes.

---

## Phase 4 — Anchors and Player Steering

Goals:

- Make possibility-space navigation understandable and intentional.

Scope:

- Anchor capture.
- Trait masks.
- Emphasis and suppression.
- Constraint projection.
- Anchor combination.
- Transition controls.
- Debug visualization of influence.

Success criterion:

Players can intentionally steer world evolution while outcomes remain surprising and ecologically coherent.

---

## Phase 5 — Routes, Persistence, and Social Model

Goals:

- Support lasting exploration history.

Scope:

- Sparse feature persistence.
- Expedition routes.
- Possibility-space route graph.
- Named discoveries.
- Preserves.
- Shared anchors.
- Community atlas schema.
- Server-compatible persistence model.

Success criterion:

Exploration creates durable, shareable structure without storing generated world geometry.

---

## Phase 6 — Performance and Scale

Goals:

- Increase world density and active simulation scale.

Scope:

- Data-layout optimization.
- SIMD kernels.
- Improved region arenas.
- Custom scheduling if justified.
- GPU field refinement.
- Resource-tier detection.
- Cache tuning.
- Large-world stress tests.
- Deterministic replay tests.

Success criterion:

The engine maintains stable frame and generation budgets across target native hardware tiers.

---

## Phase 7 — Browser Runtime

Status: the static streamed viewer and later native/web alignment recorded by
ADR 0028 have landed. The worker executor and durable browser-vault backend
remain incomplete.

Goals:

- Deliver the existing world model through modern desktop browsers.

Scope:

- Wasm runtime integration.
- Web Worker scheduling.
- Shared memory where available.
- Browser persistence.
- Browser asset streaming.
- Suspension and recovery.
- WebGPU feature tiers.
- Browser-specific memory budgets.
- Startup benchmarking.
- Reduced compatibility profiles.

Success criterion:

The browser version preserves the same world model and core experience while scaling simulation density and cache sizes to device capabilities.

---

## 21. Lower-Level Plans Required Before Implementation

Before implementing each major subsystem, create a focused implementation plan covering:

- Goals and non-goals.
- Public interfaces.
- Data layout.
- Algorithms.
- Memory ownership.
- Threading model.
- Determinism requirements.
- Cache invalidation.
- Failure handling.
- Profiling metrics.
- Native and browser constraints.
- Testing strategy.
- Incremental milestones.

Expected subsystem plans include:

```text
region-streaming-plan.md
possibility-space-plan.md
anchor-system-plan.md
world-layer-dependency-plan.md
terrain-generation-plan.md
hydrology-plan.md
ecology-field-plan.md
procedural-genetics-plan.md
entity-realization-plan.md
persistence-plan.md
route-system-plan.md
job-system-plan.md
renderer-plan.md
gpu-compute-plan.md
browser-runtime-plan.md
determinism-and-versioning-plan.md
profiling-and-benchmarking-plan.md
```

---

## 22. Initial Implementation Priorities

The first development effort should focus on the smallest prototype capable of answering the central technical question:

> Can a deterministic region-based world transform continuously through possibility space while preserving nearby stability and avoiding visible regeneration artifacts?

Initial priorities:

1. Repository and crate boundaries.
2. Native application shell.
3. Minimal browser compilation path.
4. Deterministic coordinate hashing.
5. Region identity and lifecycle.
6. Possibility state representation.
7. Stable and transition radii.
8. Incremental field regeneration.
9. Debug and profiling tools.
10. A visually simple but technically representative prototype.

Detailed ecology, social systems, persistence, and procedural organisms should follow only after the continuity model is validated.

---

## 23. Primary Risks

### 23.1 Continuity Risk

The world may appear to pop, contradict itself, or regenerate visibly.

Mitigation:

- Stable inner regions.
- Revisioned realized state.
- Distance- and visibility-aware transition.
- Layer-specific transformation strategies.
- Strong debugging visualization.

### 23.2 Scope Risk

Building a custom engine may consume effort before the central game experience is validated.

Mitigation:

- Narrow Phase 1 prototype.
- Reuse mature libraries where possible.
- Delay custom schedulers and allocators until profiling justifies them.
- Avoid detailed ecology before continuity works.

### 23.3 Dependency Explosion

Changes to possibility state may trigger excessive recomputation.

Mitigation:

- Explicit dependency graph.
- Layer-specific hashes.
- Dirty-region tracking.
- Stable topology layers.
- Temporal budgets.

### 23.4 Platform Divergence

Native development may accumulate assumptions that prevent browser support.

Mitigation:

- Continuous Wasm compilation.
- Early browser smoke tests.
- Portable WGSL renderer.
- Abstract storage and task interfaces.
- CPU-authoritative world state.

### 23.5 Determinism Drift

Native, browser, CPU, and GPU implementations may disagree.

Mitigation:

- Integer-based feature identity.
- Versioned algorithms.
- Quantization.
- Golden deterministic test fixtures.
- GPU results treated as derived unless proven portable.

### 23.6 Memory Growth

Cached fields and feature records may grow without bound.

Mitigation:

- Explicit cache budgets.
- Region eviction.
- Sparse persistence.
- Packed fields.
- Recomputation where cheaper than storage.
- Continuous memory telemetry.

---

## 24. Architectural Decision Summary

The project will:

- Retain the native Rust implementation as the profiling/debug foundation while
  shipping a static browser viewer/runtime over the same world model and
  reporting unfinished worker/vault services explicitly.
- Use `wgpu` and WGSL.
- Maintain a CPU-authoritative world model.
- Use a data-oriented architecture.
- Generate the world deterministically.
- Represent ecology hierarchically.
- Store only sparse persistent deviations.
- Recompute incrementally through explicit dependencies.
- Use temporal budgets instead of immediate regeneration.
- Treat GPU compute as an optimization, not a requirement.
- Compile core systems and shared viewer contracts for WebAssembly.
- Keep native and browser shells as thin environment adapters around one
  traveler/controller, one Map/POV/Split layout model, and one multi-view
  surface frame.
- Produce lower-level implementation plans before subsystem implementation begins.
