# 3D POV Rendering Mode — Design

A first-person, three-dimensional view of the world, added alongside the
existing top-down debug map. This is **derived presentation only** in the
strict ADR 0017 sense: the 3D mode reads settled world state through the same
CPU sampling surfaces the 2D map uses, uploads geometry to the GPU, and never
feeds anything back. It changes no generated output for any input —
`WORLD_ALGORITHM_VERSION` stays at 2, every `algorithm_revision` stays 0, and
no golden fixture is re-blessed. The work is broken into four phases:

1. **3D-1 — Terrain, lighting, free camera.** A lit terrain mesh with full
   3-axis fly-through camera movement. No collision.
2. **3D-2 — Ground collision and terrain following.** A walk mode that keeps
   the camera on the surface.
3. **3D-3 — Water.** The sea surface, plus river/wetland expression on land.
4. **3D-4 — Organisms.** Near-field organisms as instanced primitive geometry
   (boxes and spheres, solid colors) whose shape, size, and color reflect
   species and expressed genome.

Read [`AGENTS.md`](../../../AGENTS.md) first; this design assumes the crate
boundary rule, the determinism invariant, and the CI contract, and calls out
where each phase touches them.

---

## 1. Context: what already exists and what the 3D mode consumes

The design leans entirely on data surfaces that landed in Phases 2–6. Nothing
new is generated; the 3D mode is a second *reader*.

- **Continuous terrain height.** `world_core::terrain::elevation(world_x,
  world_y, p) -> f32` (`terrain.rs`) is a pure, continuous function of a
  world position and a `PossibilityVector` — five octaves of integer-hash
  gradient noise, Geology scaling amplitude, Planetary shifting the land/sea
  balance. Critically, the output is expressed **relative to `SEA_LEVEL =
  0.0`** — the Planetary sea shift is folded into `elevation_from_relief`,
  so `is_water(e) == (e < 0.0)` holds for every possibility state. This is
  the single load-bearing affordance: the mesh can be sampled at any
  resolution, at any position, without a readback and without touching the
  cached field tiles. The SIMD twin `simd::elevation_row` (ADR 0016,
  bit-identical to the scalar path) is the batch sampler the mesher uses.
- **Per-region possibility.** The generation pipeline evaluates each region's
  tiles under that region's quantized possibility vector
  (`PossibilityVector::from_quantized(decl.domains, &inputs.quantized)`,
  `world-runtime/src/generate.rs`). The 3D mesher must do the same — one mesh
  chunk per region, sampled under that region's vector — so the 3D ground
  agrees exactly with the 2D map and the field tiles. Possibility drift
  between adjacent regions can therefore step vertex heights slightly at
  region borders; §3.4 handles this with skirts, not by blending (blending
  would invent an elevation no authoritative path ever computes).
- **Per-cell fields for materials.** `RegionMap::cache().channel(coord, c)`
  returns the settled `FieldTile<f32>` for the 13 channels
  (`generate.rs`: ELEVATION, TEMPERATURE, MOISTURE, RIVER, WETNESS,
  FERTILITY, VEGETATION, CANOPY, …) at `FIELD_RES = 32` cells per region
  (cell = 8.0 world units), plus `biome(coord)` / `dominant(coord)`. This is
  the same read pattern as `build_cursor_info` in the native shell and the
  `AtlasManager` packing path.
- **Water.** Ocean is `elevation < 0` (see above). Rivers and wetlands are
  the Phase 2 hydrology expression: `CHANNEL_RIVER` (width intensity in
  [0, 1]) and `CHANNEL_WETNESS`, backed by the pinned integer drainage
  topology (ADR 0009) with `DrainageTile::accum_bilinear` for continuous
  flow accumulation. There is **no water-depth or lake-surface field**; lakes
  are implicit `FLOW_NONE` sinks. §6 scopes water accordingly.
- **Organisms.** `RegionMap::organisms()` iterates realized near-field
  organisms (pinned near regions, `organisms_per_cell` scaled by resource
  tier). Each `Organism` (`world-runtime/src/realize.rs`) carries `id`,
  `species`, `trophic`, `world_pos: (f64, f64)`, and `expressed:
  world_core::Expressed` — hue [0, 1), luminance [0, 1], body size in world
  units (~0.1–12.8 via the size-class ladder), activity, aggression. The
  genome's `form: u8` (0..15 morphology archetype, `AppearanceGenes`) exists
  but is not yet surfaced in `Expressed`; §7.2 adds a passthrough.
- **Renderer and shell.** `crates/renderer` is wgpu/WGSL only (WebGPU
  portable), currently 2D: the CPU-composed debug map blit and the
  GPU-composed atlas (`GpuMap`, `render_map_gpu`, delta uploads keyed by
  region dependency hash). **No pipeline today uses a depth buffer**
  (`depth_stencil: None` throughout) — 3D-1 introduces one. The winit shell
  (`platform-native/src/main.rs`) owns input, the top-down camera, and the
  `AtlasManager` that decides *what* to upload; the renderer only accepts
  uploads and draws (ADR 0017: no readback API, and none is added here).

### 1.1 Scale reference

`REGION_SIZE = 256.0` world units, field cells 8.0 units, terrain amplitude
±~600 (Geology-scaled 0.5–1.5×), base terrain wavelength 4096 units,
organisms 0.1–12.8 units. A "person-scale" eye height of ~1.7 units (§5) sits
comfortably in this range.

## 2. Architecture: where the pieces live

The boundary follows the `AtlasManager` precedent exactly — the renderer
stays world-agnostic, the shell packs world data into upload structs:

| Piece | Crate | Notes |
|-------|-------|-------|
| 3D pipelines, depth buffer, camera uniform, draw calls | `crates/renderer` (`src/pov.rs`, `shaders/pov_*.wgsl`) | Upload-only API, mirrors `GpuMap`. Knows vertices, not regions. |
| Chunk mesher, chunk cache/eviction, camera controller, walk physics | `crates/platform-native` (`src/pov.rs`) | Reads `RegionMap` + `elevation()`, produces `TerrainChunkUpload`s. |
| Terrain/genome/hydrology sampling | `world-core` / `world-runtime` | Unchanged, except the §7.2 `form` passthrough in `world-core`. |

The mesher is written as a **pure function** of `(region coord, possibility
vector, field tiles) -> (vertices, indices)` with no filesystem, thread, or
GPU dependency, so Phase 7 can hoist it into a neutral crate for the browser
shell without rework. It lives in `platform-native` for now because its
*inputs* come from the native shell's streaming state, matching how
`pack_region` lives beside `AtlasManager`.

Neutral crates gain no GPU or platform dependency; the wasm CI check is
unaffected. All new shaders are WGSL under `renderer/shaders/`.

### 2.1 Mode switching and controls

`Tab` toggles Map ↔ POV. The 2D map path is untouched and remains the
default; `WER_POV=1` starts in POV mode. In POV mode:

| Input | Fly (3D-1) | Walk (3D-2+) |
|-------|------------|--------------|
| Mouse (cursor grabbed) | look (yaw/pitch, pitch clamped ±89°) | same |
| `W`/`A`/`S`/`D` | move along view/strafe | move along ground plane |
| `Space` / `LShift` | up / down | (reserved: jump / crouch later) |
| Scroll wheel | movement speed ×/÷ | same |
| `F` | — | toggle fly ↔ walk (from 3D-2) |
| `Tab` | back to map | back to map |
| `Escape` | release cursor grab, then exit (as today) | same |

Entering POV grabs and hides the cursor (`winit`
`CursorGrabMode::Confined`/`Locked` with the documented fallback chain);
leaving POV or pressing `Escape` once releases it. The existing map
keybindings (`V`, `,`, `.`, digits, `Q`/`E`) stay map-mode-only; the possibility
HUD and cursor-info panel are hidden in POV for the initial phases.

### 2.2 Scheduling and budgets

Chunk meshing is CPU work proportional to visible regions, so it runs off the
frame thread on the **LaneExecutor** background lane with cancellation tokens,
exactly like generation passes — never on the render thread. Remeshing is
delta-driven: each chunk is keyed by the same region dependency-hash key the
`AtlasManager` uses (`region_key`), so steady-state traffic is ~0 and a
possibility drift or anchor edit remeshes only the regions whose hashes
changed (ADR 0008 doing presentation work). Uploads are amortized: at most
`N` chunk uploads per frame (start at 4; tune against
`docs/perf-baseline.md` methodology). Meshing passes get `pass-timing` spans
like every other pass.

**Local hardware note:** the primary dev environment is WSL2 with
llvmpipe-only Vulkan (software rasterization). Triangle budgets below are
chosen to stay interactive there; `WER_POV_RADIUS` (region draw radius,
default = the existing near radius 3) is the escape hatch.

## 3. Phase 3D-1 — Terrain, lighting, free camera

**Goal:** stand in the world. A lit, colored terrain mesh out to the near
radius, a fly camera with full 3-axis movement, depth-correct rendering, fog
to the horizon. No collision: the camera can fly through hills.

### 3.1 Renderer groundwork

- Add a `Depth32Float` depth texture, recreated on resize, and a
  `render_pov(...)` entry point that clears color + depth and draws the POV
  pipelines. The existing 2D entry points are untouched.
- Camera uniform: view-projection matrix (f32, standard perspective,
  `zfar` ≈ fog distance, `znear` 0.1), camera world position, sun direction,
  fog parameters. Matrix math via `glam` (add to
  `[workspace.dependencies]`; wasm-clean).
- `pov_terrain.wgsl`: vertex = position + normal + RGBA color; Lambert
  diffuse from a fixed directional sun (e.g. normalized (0.4, 0.2, −0.9)
  toward the ground) plus a hemispherical ambient term (sky color above,
  ground color below), distance fog blending into the clear color. No
  shadows, no textures, no PBR — flat-shaded-looking vertex-lit ground is
  the deliverable.

### 3.2 Chunk meshing

One chunk per region, a uniform grid of `POV_MESH_RES = 64` quads per edge
(4.0-unit spacing, 65×65 = 4 225 vertices, 8 192 triangles). At radius 3
that is 49 chunks ≈ 400 k triangles — acceptable on llvmpipe, trivial on
hardware. Per vertex:

- **Height:** `elevation(x, y, p_region)` via `simd::elevation_row` over each
  vertex row — the same math, batched. Sampling at 2× field resolution is
  free detail the 2D map doesn't show; it is still the authoritative CPU
  spectrum (the GPU refinement octaves of `compose_map.wgsl` are *not*
  ported in this phase — if they ever displace 3D vertices they must come
  from the CPU twin of the refinement math, ADR 0016/0017; deferred).
- **Normal:** central differences of the same `elevation` samples (one extra
  row/column of samples per edge), normalized on CPU.
- **Color:** bilinearly sample the region's field tiles at the vertex —
  reuse the 2D map's Composite-channel color logic (`viz.rs`) as a shared
  helper: biome base color modulated by vegetation/canopy, wetness darkening,
  snow/rock by temperature and hardness. Underwater ground gets its
  sediment color; the water *surface* is 3D-3.

Vertex format: `position: [f32; 3]`, `normal: [f32; 3]`, `color: [u8; 4]`
(28 bytes). Indices `u32`. Chunks own GPU buffers pooled/reused on eviction
(the tile-pool discipline of Phase 6 applied to vertex buffers).

### 3.3 Chunk lifecycle

`PovChunkManager` (platform-native) mirrors `AtlasManager`: it walks resident
regions within `WER_POV_RADIUS`, compares each region's dep-hash key against
the mesh it holds, schedules stale/missing chunks on the background lane
(cancellation-checked), and hands finished `TerrainChunkUpload`s to the
renderer under the per-frame amortization cap. Eviction is
farthest-first, same as the caches. A region whose tiles are not yet settled
simply has no chunk yet — holes at the loading frontier are acceptable in
3D-1 (fog hides most of it), and shrink as generation catches up.

### 3.4 Region-border seams

Adjacent regions can carry different quantized possibility vectors, so shared
border positions can disagree in height by a small amount (drift is smooth
and quantized; steps are sub-unit in practice). The fix is the standard one:
each chunk extends a **vertical skirt** one grid step down around its
perimeter. No cross-region blending, no averaging — the mesh shows exactly
what the authoritative sampler computes, and the skirt hides the hairline.
This also covers the (rarer) step at a preserve boundary.

### 3.5 Exit criteria

- POV mode toggles on/off with `Tab`; map mode is pixel-identical to before.
- Fly camera: full 3-axis movement, mouse look, speed control.
- Terrain out to radius 3 with lighting, fog, and per-vertex material color
  consistent with the 2D Composite channel over the same region.
- Depth-correct rendering (hills occlude), no seam cracks at region borders
  (skirts), steady-state remesh traffic is zero (log counter, same
  methodology as the atlas delta counter).
- `cargo test --workspace` green with **no golden fixture changes**; mesher
  unit tests assert determinism (same inputs → byte-identical vertex buffer)
  and skirt watertightness. CI wasm check unaffected.

## 4. Phase 3D-2 — Ground collision and terrain following

**Goal:** a walk mode. The camera rides the terrain surface; fly mode
remains available (`F` toggles).

### 4.1 Ground height query

Collision height comes from the **render lattice, not the analytic
function**: barycentric interpolation over the same 64×64 chunk grid that is
drawn, using the chunk's CPU-side height array (kept by the
`PovChunkManager`; the mesher already computes it). This guarantees the
camera never visually sinks into or floats above the rendered triangles,
which analytic `elevation()` at the camera point would not (the mesh is a
piecewise-linear approximation of it). Where no chunk exists yet (loading
frontier), walk mode falls back to analytic `elevation()` under the region's
vector — correct to within one mesh cell, and transient.

`fn ground_height(&self, wx: f64, wy: f64) -> f32` on `PovChunkManager`:
locate region → chunk → cell → triangle → barycentric. Pure, unit-testable
against the mesher (exact agreement at vertices, bounded error mid-cell).

### 4.2 Walk kinematics

Deliberately minimal — this is terrain following, not a character
controller:

- Eye height `EYE_HEIGHT = 1.7` world units above ground.
- Horizontal movement integrates WASD in the yaw plane; each frame the
  camera's `z` is set to `ground_height(x, y) + EYE_HEIGHT` — hard terrain
  following, no gravity/falling state needed while grounded.
- A simple vertical-speed clamp (max climb rate) so cliff faces feel like
  slopes being climbed rather than teleports; no slide, no step-height
  logic, no lateral collision (you can walk through organisms and steep
  walls — fine for this phase).
- Walking below `z = 0` is allowed and just means walking on the sea floor
  (water is 3D-3; wading/swimming rules can layer on later).
- Toggling `F` back to fly keeps the current position/orientation.

All of this is presentation-side camera state in `platform-native`; nothing
touches world state, saves, or the vault.

### 4.3 Exit criteria

- `F` toggles walk/fly; walk mode holds the eye exactly `EYE_HEIGHT` above
  the rendered surface across region borders, skirt edges, and chunk
  boundaries (no pops beyond the possibility-drift step itself).
- `ground_height` unit tests: vertex-exact, mid-cell bounded, cross-border
  continuity within the drift step.

## 5. Phase 3D-3 — Water

**Goal:** the ocean reads as water, rivers and wetlands read on the land.
Scope is honest about the model: there is no water-depth field and no lake
surface, so this phase renders what the model actually knows.

### 5.1 Sea surface

A translucent horizontal plane at `z = SEA_LEVEL = 0.0` — exactly correct
for every possibility state because `elevation()` already folds the
Planetary sea shift in (§1). Implementation: a camera-centered grid (or
single large quad) drawn after terrain with depth-test on / depth-write off,
alpha-blended, in its own `pov_water.wgsl`:

- Color from view angle (Fresnel-ish lerp between deep-water blue and sky).
- Depth cue: darken with distance between the water plane and the terrain
  beneath (the terrain height is in the vertex color path already; simplest
  is sampling scene depth — but ADR 0017 forbids nothing here since it is
  render-target-to-render-target, never CPU readback. If depth-sampling
  complicates the pass, ship flat translucency first; depth tint is polish).
- A little time-based normal wobble in the shader for specular glints —
  display-only animation, explicitly allowed (frame time never feeds back
  into world state).

### 5.2 Rivers and wetlands

Rivers stay **on the terrain**, not as separate free-surface geometry
(carving channels or fitting sloped water surfaces to the drainage graph is
real hydraulic work — deferred, §8). Two steps, in order of payoff:

1. **Material pass (cheap, ship first):** strengthen the 3D-1 vertex-color
   treatment — vertices with high `CHANNEL_RIVER`/`CHANNEL_WETNESS` get
   water coloring plus a `wetness` vertex attribute the terrain shader uses
   for a glossy/darkened response. Rivers read as blue-green ribbons
   following the (already elevation-consistent) drainage lines.
2. **Overlay strips (if ribbons look too painted):** a second translucent
   mesh extruded a few centimeters above the terrain along cells where
   river intensity exceeds a threshold, width from `river`, sharing the
   water shader's wobble. Still terrain-conformal, still no depth field.

Lakes (`FLOW_NONE` sinks) remain terrain-colored wet basins in this phase; a
proper basin-fill derivation (pour-point scan over the drainage tile → flat
lake surface per sink) is listed as follow-on work in §8.

### 5.3 Exit criteria

- Coastlines are correct by construction at every possibility state (plane
  at 0 vs. sea-shift-folded elevation); flying across an anchor-steered
  Planetary change shows the sea rise/fall against the same shore.
- Rivers visible in POV match the 2D map's river channel over the same
  regions (same tiles, same threshold).
- Walk mode (3D-2) is unchanged by water: the sea floor is still the ground.

## 6. Phase 3D-4 — Organisms

**Goal:** the realized near-field organisms appear in POV as simple solid-
color primitives whose shape, proportions, size, and color are readable
functions of species and expressed genome — the 3D analog of the 2D map's
expressed-color markers.

### 6.1 Rendering

Two instanced pipelines, one draw call each per frame:

- **Box** (unit cube, scaled per-instance to a rectangular prism).
- **Sphere** (one icosphere mesh, ~2 subdivisions, scaled per-instance).

Instance data: world position (x, y from `Organism::world_pos`, z from
`ground_height` + half-height so bodies sit on the surface), non-uniform
scale, yaw, RGBA color. Instances are rebuilt when a region's realization
changes (the same rebuild-on-L8-change event that drives the 2D markers) and
culled by distance; at High tier the cap is `max_realize_organisms = 1600`,
well within a single instanced draw. Lit by the same sun/ambient as terrain.

### 6.2 Mapping genome → geometry

`Expressed` gains a passthrough field: `form: u8` copied verbatim from
`AppearanceGenes::form` in `Genome::express`. This is a pure copy — no
existing expressed field changes value, `Expressed` is documented
presentation-never-identity, and the `genome_sample` parity export folds
genome fields, not `Expressed` — so no version bump and no fixture change.
(Verify that claim against the parity test when implementing; if anything
does fold `Expressed`, surface `form` on `Organism` instead.)

The visual mapping, chosen to be legible at a glance:

| Trait | Source | Visual |
|-------|--------|--------|
| Primitive | `form` bit 0 | even → box, odd → sphere |
| Proportions | `form` bits 1–3 | box: height/width elongation ladder (flat slab → tall pillar); sphere: vertical squash/stretch |
| Overall size | `Expressed::size` | uniform scale (0.1–12.8 world units — the size ladder becomes directly visible) |
| Color | `Expressed::hue`, `luminance` | the existing `hsv_to_rgb(hue, 0.75, 0.45 + 0.55·luminance)` from `viz.rs`, hoisted to a shared helper so 2D markers and 3D bodies match exactly |
| Trophic tier | `Organism::trophic` | producers sit flush with the ground and get a slight desaturation toward green-gray; consumers float their pivot at half-height and keep full saturation (a subtle plants-vs-creatures cue that doesn't override genome color) |
| Facing | `id` low bits | static yaw (deterministic variety, no RNG at draw time) |

Optional polish, display-only: `activity` drives a small sinusoidal bob
(amplitude ∝ activity, phase from `id`), giving live-feeling fauna for free.
Frame time feeds the shader only.

### 6.3 Exit criteria

- Every organism the 2D overlay shows in a region appears in POV at the same
  world position with the same color; count and identity match
  `RegionMap::organisms()` exactly at every tier.
- Two organisms of the same species look identical except size/color drift
  from bias; different `form` archetypes are visually distinct.
- No change to realization, rosters, food webs, or any hash; the `form`
  passthrough lands with proof (parity exports and golden fixtures
  untouched).

## 7. Cross-cutting rules

- **Determinism.** No new identity derives from anything in this design.
  Every height, color, and position the 3D mode shows is computed by
  existing authoritative CPU functions or pure presentation math on their
  outputs. GPU work remains derived-only; **no readback API is added**
  (ADR 0017 governs; if a future phase wants GPU-displaced terrain detail,
  it follows the ADR 0016 CPU-twin discipline first).
- **Crate boundaries.** `world-core`/`world-runtime` stay platform-free (the
  only touch is the §7.2 pure `form` passthrough). Renderer stays
  world-agnostic and upload-only. Mesher stays a pure function for the
  Phase 7 hoist.
- **CI.** Every phase lands green on the full CI matrix (`fmt`, `clippy`
  with `-D warnings`, native tests, wasm check of the neutral crates +
  `platform-web`). New pure logic (mesher, `ground_height`, genome→geometry
  mapping) gets unit tests; golden determinism fixtures are never touched.
- **Performance.** Meshing on the background lane with cancellation;
  amortized uploads; dep-hash-keyed remesh (steady state ~0); pooled GPU
  buffers; `pass-timing` spans; numbers recorded against
  `docs/perf-baseline.md` before/after each phase on the WSL2/llvmpipe
  reference environment.
- **Env vars.** `WER_POV=1` (start in POV), `WER_POV_RADIUS=n` (chunk draw
  radius), existing `WER_CPU_MAP`/`WER_TIER`/`WER_CACHE_MB` unaffected.

## 8. Deferred (explicitly out of scope)

- LOD rings / clipmaps beyond the fixed per-region grid (fog + radius cap is
  the 3D-1 answer; revisit when `WER_POV_RADIUS` wants to grow).
- GPU refinement-octave displacement of the terrain mesh (needs the CPU twin
  per ADR 0016 so collision and visuals agree).
- Carved river channels, sloped river surfaces, and lake basin-fill from
  `FLOW_NONE` sinks (needs a derived water-surface pass over the drainage
  tile).
- Lateral collision, jumping, swimming, and any character-controller physics
  beyond terrain following.
- Organism meshes beyond primitives, textures, shadows, and any lighting
  model past Lambert + hemisphere + fog.
- Browser (Phase 7) integration — the mesher's purity and the WGSL-only
  shader rule keep the door open; nothing here lands wasm code.
