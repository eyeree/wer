# Phase 3D-1 — Terrain, Lighting, Free Camera: Implementation Plan

This is the lower-level plan for the first phase of
[`3d-design.md`](3d-design.md) (§3 there): a lit, colored terrain mesh out to
the near radius, a free fly camera with full 3-axis movement, depth-correct
rendering, and fog to the horizon. No collision (3D-2), no water surface
(3D-3), no organisms (3D-4).

Read [`AGENTS.md`](../../../AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it
assumes them. One sentence of orientation up front, because it governs
everything below: **3D-1 is derived presentation only** (ADR 0017). Every
height and color it shows is computed by existing authoritative CPU functions
(`world_core::terrain::elevation` via its bit-identical SIMD twin, the settled
field tiles) or pure presentation math on their outputs. `WORLD_ALGORITHM_VERSION`
stays at 2, every `algorithm_revision` stays 0, `RECORD_FORMAT_VERSION` stays
at 1, zero golden fixtures are re-blessed, and no readback API is added.

---

## 1. Goals and non-goals

### 1.1 Goals (design §3.5, restated as deliverables)

- A **POV mode** toggled with `Tab` (currently unbound in `handle_press`,
  `platform-native/src/main.rs`), started directly with `WER_POV=1`. Map mode
  is pixel-identical to today when POV is off.
- A **fly camera**: mouse look (yaw/pitch, pitch clamped ±89°) under cursor
  grab, `W`/`A`/`S`/`D` along view/strafe, `Space`/`LShift` up/down, scroll
  wheel speed multiplier.
- A **terrain mesh** out to `WER_POV_RADIUS` (default 3) regions: one chunk
  per region at `POV_MESH_RES = 64` quads per edge, vertex-lit (Lambert sun +
  hemisphere ambient), per-vertex material color consistent with the 2D
  Composite channel, distance fog into the clear color.
- **Depth-correct rendering** — the renderer's first depth buffer (today
  `depth_stencil: None` at every one of the five sites in
  `renderer/src/lib.rs` and `renderer/src/gpumap.rs`).
- **Seam-free region borders** via per-chunk vertical skirts (no cross-region
  blending — the mesh shows exactly what the authoritative sampler computes).
- **Zero steady-state remesh traffic**, proven by a counter with the same
  methodology as the atlas delta counter, with meshing on the LaneExecutor
  background lane, cancellation-checked, uploads amortized per frame, and GPU
  buffers pooled.

### 1.2 Non-goals (deferred to later 3D phases or design §8)

- Ground collision, `ground_height`, walk mode, the `F` toggle (3D-2 — but
  §5.4 below keeps the CPU-side height array the mesher computes, so 3D-2
  starts from data that already exists).
- Sea surface, river/wetland material strengthening (3D-3). Underwater ground
  gets its sediment color in 3D-1; the water *surface* does not exist yet.
- Organism instancing and the `Expressed::form` passthrough (3D-4). No
  `world-core`/`world-runtime` generation surface changes at all in 3D-1.
- LOD rings/clipmaps, GPU refinement-octave displacement of vertices (needs
  the ADR 0016 CPU twin first), shadows, textures, PBR, POV screenshots, the
  possibility HUD / cursor-info panel in POV mode.
- Browser integration. The mesher is written as a pure function so Phase 7
  can hoist it (design §2), but nothing here lands wasm code.

## 2. Contracts this phase must not break

- **Determinism.** No new identity derives from anything here. Heights come
  from `simd::elevation_row` (bit-identical to scalar `elevation`, ADR 0016);
  colors from the settled field tiles through the same per-cell logic the 2D
  Composite channel uses. Nothing feeds back: the renderer's POV path, like
  `GpuMap`, exposes no readback (ADR 0017).
- **Crate boundaries.** The renderer stays world-agnostic and upload-only —
  it learns about vertices and opaque chunk handles, never `RegionCoord` or
  `RegionMap`, exactly the `GpuMap`/`AtlasManager` split. The mesher and all
  world-reading logic live in `platform-native`. Neutral crates gain no
  platform or GPU dependency; the only neutral-crate touch is the optional
  `Pass::Mesh` timing variant (§8.1), which is feature-gated wasm-clean
  telemetry, not world logic.
- **CI.** Lands green on the full matrix: `fmt --check`, `clippy` with
  `-D warnings`, `cargo test --workspace` with **no golden fixture changes**,
  and the wasm32 check of `world-core`/`world-runtime`/`platform-web`
  unaffected. New WGSL is validated GPU-free by `renderer/tests/wgsl.rs`
  (naga), like the two existing shaders.

## 3. New and touched surfaces

| Surface | Change |
|---------|--------|
| `crates/renderer/src/pov.rs` (new) | POV pipeline, depth texture, camera/lighting uniform, chunk buffer table + vertex-buffer pool, shared index buffer, `TerrainChunkUpload` / `PovFrameParams`. |
| `crates/renderer/shaders/pov_terrain.wgsl` (new) | Vertex-lit terrain: Lambert sun + hemisphere ambient + distance fog. |
| `crates/renderer/src/lib.rs` | `pov: Option<Pov>` field, depth-texture recreation in `resize`, new `render_pov(...)` entry point. Existing 2D entry points untouched. |
| `crates/renderer/tests/wgsl.rs` | naga parse+validate of `SHADER_POV_TERRAIN`. |
| `crates/platform-native/src/pov.rs` (new) | `PovCamera` (fly controller), pure mesher `mesh_region_chunk(...)`, `PovChunkManager` (keying, scheduling, amortized upload, eviction), inline unit tests. |
| `crates/platform-native/src/viz.rs` | Hoist the Composite per-cell color into a shared `pub(crate) fn composite_cell_color(...)` used by both `MapComposer::paint_region` and the mesher (pure visibility/refactor — 2D output byte-identical, guarded by a test). |
| `crates/platform-native/src/main.rs` | `ViewMode` state, `Tab` toggle, cursor grab, `device_event` mouse-look handler, POV movement, mode-gated keybindings, `WER_POV`/`WER_POV_RADIUS`, POV branch in `frame()`. |
| `crates/platform-native/Cargo.toml` | `glam.workspace = true` (already in `[workspace.dependencies]` at 0.33, `default-features = false, features = ["libm"]`; declared-but-unused in `world-core` — no new version negotiation). |
| `crates/world-runtime/src/timing.rs` | Optional: `Pass::Mesh` variant (§8.1), following the shell-filled `Pass::Flush` precedent. |
| `docs/perf-baseline.md` | New POV section (mesh ms/chunk, bytes/chunk, llvmpipe frame ms at radius 3). |

The renderer keeps **no dependency on winit or glam**: the shell computes the
view-projection matrix with `glam` and hands the renderer plain
`[[f32; 4]; 4]` / `[f32; 3]` arrays (bytemuck-Pod), the same
world-agnostic posture `GpuMapParams` already takes.

## 4. Coordinate system and camera math

- **World space is right-handed Z-up**: `world_x → X`, `world_y → Y`,
  `elevation → Z`. This matches the design throughout ("the camera's `z` is
  set to `ground_height + EYE_HEIGHT`", §4.2) and keeps 3D positions a
  trivial lift of the existing 2D `(f64, f64)` world positions. The camera
  holds `pos: glam::DVec3` (f64 — world coordinates are f64 everywhere else;
  precision at ±10⁶ units matters), `yaw: f32`, `pitch: f32` (radians, pitch
  clamped to ±89°).
- **Render-space translation.** Vertices are uploaded in **chunk-local
  coordinates** (region origin at 0,0) and the shell passes each chunk's
  region origin *relative to the camera* per draw via a per-chunk uniform
  offset (or push-constant-free: a second small uniform buffer with dynamic
  offset). Camera-relative rendering keeps every f32 the GPU sees small
  (≤ ~1 000), so f32 vertex positions are exact-enough at any world
  coordinate — the standard fix for far-from-origin jitter, and it means a
  chunk's vertex buffer never depends on the camera (mesh once, draw
  anywhere).
- **Matrices** (computed in the shell with `glam`, f64 for the view
  translation then truncated): `view = look_to` from camera-relative origin
  along the yaw/pitch direction, `proj = Mat4::perspective_rh(fov_y = 60°,
  aspect, znear = 0.1, zfar = 2048.0)`. wgpu clip space is 0..1 depth
  (`perspective_rh` in glam produces exactly that). `Depth32Float` at
  znear 0.1 / zfar 2048 has ample precision; reversed-Z is noted as a
  follow-on if far-field artifacts ever appear, not built now.
- **Fog** runs from `POV_FOG_START` to `POV_FOG_END`, defaulting to
  `0.55 · R` and `0.95 · R` where `R = (WER_POV_RADIUS + 0.5) · REGION_SIZE`
  (radius 3 → fog fully opaque by ~900 units, comfortably inside the meshed
  ring so the loading frontier and the world edge hide in fog). Fog color =
  the clear color, so geometry dissolves into sky.

## 5. Renderer work (`crates/renderer/src/pov.rs`)

### 5.1 Depth target

`Renderer` gains a lazily-created `Depth32Float` texture sized to the surface
config, recreated inside `resize()` (which already reconfigures the surface)
and on first `render_pov` call. It is used only by the POV pass; the 2D
passes keep `depth_stencil_attachment: None` and are untouched.

### 5.2 Vertex format and upload structs

```rust
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PovVertex {
    pub position: [f32; 3], // chunk-local: x,y in [0, REGION_SIZE], z = elevation
    pub normal:   [f32; 3],
    pub color:    [u8; 4],  // sRGB bytes, VertexFormat::Unorm8x4 → 0..1 in shader
} // 28 bytes

pub struct TerrainChunkUpload {
    pub handle: u64,              // opaque chunk id assigned by the shell
    pub origin: [f64; 2],         // region origin in world units (for camera-relative offset)
    pub vertices: Vec<PovVertex>, // fixed count: VERTS_PER_CHUNK
}

pub struct PovFrameParams {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f64; 3],     // world; renderer subtracts per-chunk origin in f64 then truncates
    pub sun_dir: [f32; 3],        // normalized (0.4, 0.2, -0.9).normalize() toward ground
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
    pub sky_ambient: [f32; 3],    // hemisphere term, above
    pub ground_ambient: [f32; 3], // hemisphere term, below
}
```

Because every chunk is the same uniform grid, the **index buffer is identical
for all chunks**: it is built once at `Pov` creation (core grid + skirt,
§6.5, `u32` indices) and shared by every draw. `TerrainChunkUpload` therefore
carries no indices, and every vertex buffer has the same fixed size —
which makes pooling trivial (§5.5).

### 5.3 Pipeline

One render pipeline (`pov_terrain.wgsl`, loaded via `include_str!` as
`pub const SHADER_POV_TERRAIN`, same as the two existing shaders): vertex
buffer layout = the 28-byte `PovVertex` (`Float32x3`, `Float32x3`,
`Unorm8x4`), bind group 0 = one uniform buffer (`PovParamsRaw`, the std140
mirror of `PovFrameParams` minus per-chunk data), bind group 1 = a small
per-chunk uniform (camera-relative chunk offset, `vec3<f32>` + pad) using
one buffer with dynamic offsets, written once per frame for all resident
chunks. Depth state: `Depth32Float`, `depth_write_enabled: true`, compare
`Less`. Cull mode `Back`, front face `Ccw` — the mesher emits consistent
CCW-when-viewed-from-above winding (unit-tested, §7.2 check 5). Color target
`config.format` like the existing pipelines.

### 5.4 `render_pov`

```rust
pub fn render_pov(
    &mut self,
    frame: &PovFrameParams,
    uploads: &[TerrainChunkUpload], // already amortized by the shell
    removes: &[u64],                // chunk handles to evict/pool
    clear: [f64; 4],
) -> bool
```

Mirrors `render_map_gpu`'s shape: lazily build `Pov` on first call, apply
`removes` (return the vertex buffer to the pool), apply `uploads`
(`queue.write_buffer` into a pooled buffer; a re-upload to an existing handle
swaps contents in place), write the frame uniform, then one render pass that
clears color + depth and draws every resident chunk with the shared index
buffer. Returns false on surface loss like the other entry points. No
readback, no API that could ever produce one.

### 5.5 Buffer pooling

`Pov` keeps `chunks: HashMap<u64, ChunkSlot>` and `free: Vec<wgpu::Buffer>`.
All vertex buffers are `VERTS_PER_CHUNK * 28` bytes, created with
`COPY_DST | VERTEX`; eviction pushes the buffer onto `free`, upload pops or
creates. This is the Phase 6 tile-pool discipline applied to vertex buffers:
steady-state travel allocates zero new GPU buffers once the pool warms up.
Pool size is naturally bounded by the chunk capacity the shell enforces
(§7.4); `Pov` never decides retention — the shell does, via `removes`.

### 5.6 `pov_terrain.wgsl`

- **Vertex stage:** position → camera-relative via chunk offset →
  `view_proj`; passes world-ish position (camera-relative), normal, color,
  and view distance to the fragment stage. No displacement, no noise — the
  GPU refinement octaves of `compose_map.wgsl` are deliberately *not* ported
  (they would displace visuals away from the CPU ground truth that 3D-2's
  collision will use; ADR 0016/0017, design §3.2).
- **Fragment stage:** decode vertex color from sRGB to linear
  (`pow((c + 0.055)/1.055, 2.4)` piecewise or the cheap `pow(c, 2.2)` —
  pick one and note it; the surface is `Rgba8UnormSrgb`-family via
  `get_default_config`, so the hardware re-encodes on write). Lighting =
  `color * (sun * max(dot(n, -sun_dir), 0.0) + mix(ground_ambient,
  sky_ambient, n.z * 0.5 + 0.5))`. Fog = `mix(lit, fog_color,
  smoothstep(fog_start, fog_end, dist))`. Flat-shaded-looking vertex-lit
  ground is the deliverable — no specular, no shadows.

### 5.7 Renderer tests

`renderer/tests/wgsl.rs` gains `SHADER_POV_TERRAIN` (naga parse + validate,
GPU-free, the CI-side shader gate). Buffer-pool bookkeeping (`free` reuse,
handle swap, remove-then-upload) gets `#[cfg(test)]` unit tests where they
can run without a device (pure bookkeeping extracted into a plain struct),
following the `AtlasManager` test precedent.

## 6. The mesher (`platform-native/src/pov.rs`, pure function)

### 6.1 Signature and purity

```rust
pub const POV_MESH_RES: usize = 64;             // quads per region edge (4.0-unit spacing)
pub const VERTS_PER_CHUNK: usize = 65 * 65 + 4 * 65; // core grid + skirt bottom ring

pub struct ChunkMeshInputs<'a> {
    pub coord: RegionCoord,
    pub p: PossibilityVector,        // the region's terrain-quantized vector (§6.2)
    pub river: &'a FieldTile<f32>,   // CHANNEL_RIVER
    pub wetness: &'a FieldTile<f32>, // CHANNEL_WETNESS
    pub biome: &'a FieldTile<u8>,
    pub dominant: &'a FieldTile<u16>,
}

pub struct ChunkMesh {
    pub vertices: Vec<PovVertex>,
    pub heights: Vec<f32>, // 65×65 core heights, kept CPU-side for 3D-2's ground_height
}

pub fn mesh_region_chunk(inputs: &ChunkMeshInputs<'_>) -> ChunkMesh
```

No filesystem, no threads, no GPU, no `RegionMap` — inputs are value
snapshots (the `FieldTile`s arrive as `Arc` clones held by the job), so the
function is `Send`-friendly, unit-testable, and hoistable to a neutral crate
for Phase 7 without rework (design §2). The shared index buffer's topology
(`fn chunk_indices() -> Vec<u32>`) is likewise pure and lives beside it.

### 6.2 The possibility snapshot

The tiles a region shows were generated under that region's **quantized**
vector — `generate.rs` builds
`PossibilityVector::from_quantized(decl.domains, &inputs.quantized)` per
layer. The mesher must sample elevation under the same vector or the 3D
ground will disagree with the 2D map and the field tiles. Concretely, when
scheduling a mesh job the shell reads the region's
`RegionState::current` (`map.get(coord)`, `region.rs`) and derives:

```rust
let decl = layer_decl(LAYER_TERRAIN);            // domains = Geology | Planetary
let buckets = state.current.quantized_domains(decl.domains);
let p = PossibilityVector::from_quantized(decl.domains, &buckets);
```

— exactly the reconstruction `generate.rs:359` performs for the terrain
generator, so mesh heights are bit-equal to what produced the `ELEVATION`
tile (drainage, hydrology, and every other layer are irrelevant to mesh
*height*; their expression arrives through the color tiles). The buckets are
also folded into the chunk key (§7.1) so a drift step that flips a terrain
bucket forces a remesh in the same breath that it dirties the tiles.

Transient honesty: between a bucket flip and the regenerated tiles landing,
the 2D map shows old tiles and the 3D mode shows an old mesh — both remesh/
re-upload when the dep hashes settle. No blending, no interpolation between
possibility states (that would invent an elevation no authoritative path
computes; design §3.4).

### 6.3 Height sampling and normals

A 67×67 sample grid (the 65×65 vertex lattice plus one extra ring for
central differences) at 4.0-unit spacing spanning
`[origin − 4.0, origin + 260.0]`:

- One `simd::elevation_row(xs, world_y, &p, out)` call per row (67 calls,
  4 489 samples/chunk) — `world-core/src/simd.rs:186`, the same batched
  kernel generation uses, bit-identical to scalar `elevation` (ADR 0016).
  Vertex heights are therefore *exactly* `elevation(x, y, p)` — asserted by
  unit test, never approximated.
- Sampling the analytic function at 2× field resolution is free detail the
  2D map doesn't show, and is still the authoritative CPU spectrum
  (design §3.2).
- Normal at vertex (i, j): `n = normalize((h[i−1][j] − h[i+1][j],
  h[i][j−1] − h[i][j+1], 2 · spacing · 2))` — i.e. central differences over
  the extra ring, normalized on CPU, f32 throughout. Presentation-only
  float math; no identity derives from it.

### 6.4 Vertex color

Hoist the 2D Composite per-cell logic into one shared helper in `viz.rs`
(same crate, `pub(crate)` — no crate-boundary implications):

```rust
pub(crate) fn composite_cell_color(
    e: f32, biome: Biome, river: f32, wetness: f32, dominant: u16,
) -> [u8; 3]
```

which is today's `composite_color(e, biome, river, wetness)`
(`viz.rs:392`: sea → `elevation_color` sediment ramp; land → `biome_color`
lerped toward river blue by `river * 0.8`, toward wet-dark by
`wetness * 0.25`, high-rock fade above e = 500) **plus** the dominant-species
tint the Composite arm of `paint_region` adds
(`lerp_rgb(rgb, species_color(id), 0.18)`). `paint_region` is refactored to
call the helper; a unit test pins the refactor byte-identical for the 2D
path.

Per POV vertex:

- `e` = the vertex's own analytic height (§6.3) — coastlines and the rock
  fade resolve at mesh precision, and at cell centers this equals the
  `ELEVATION` tile value bit-exactly (same function, same quantized vector).
- `river`, `wetness` = **bilinear** over the four nearest cell centers of
  the region's own tiles (cell centers at `(c + 0.5) · 8.0`; coordinates
  clamped to the region interior — a chunk never reads a neighbor's tiles,
  preserving per-region purity; the skirt hides any hairline color step at
  borders exactly as it hides the height step).
- `biome`, `dominant` = **nearest cell** (categorical — blending ids is
  meaningless; color continuity across biome edges is not a 3D-1 goal, the
  2D map has hard biome edges too).
- Alpha byte = 255 (reserved; 3D-3's wetness-gloss attribute can repurpose
  it without a format change).

At a cell center, the vertex color equals the 2D Composite pixel color for
that cell by construction — the concrete meaning of "consistent with the 2D
Composite channel" in the exit criteria, and a unit test (§10).

### 6.5 Skirts

Each chunk extends a vertical skirt around its perimeter (design §3.4): the
skirt reuses the 260 perimeter vertices as its top edge and adds a bottom
ring of 4 × 65 new vertices at the same (x, y) with
`z − POV_SKIRT_DROP` (`POV_SKIRT_DROP = 4.0`, one grid step — possibility
drift between adjacent quantized vectors steps borders by sub-unit amounts
in practice, so one grid step is generous), same normal and color as the
vertex above (the skirt should read as the terrain continuing, not as a
wall). Skirt quads are part of the shared index topology. Totals per chunk:
65×65 + 260 = 4 485 vertices (~125 KB), 8 192 core + 512 skirt = 8 704
triangles (~104 KB of u32 indices, built once, shared). At radius 3 that is
49 chunks ≈ 427 k triangles and ≈ 6 MB of vertex data — inside the llvmpipe
budget the design sized (§2.2), trivial on hardware.

### 6.6 Determinism of the mesh itself

`mesh_region_chunk` is deterministic by construction (pure function of value
inputs, fixed iteration order, no HashMap iteration, no RNG, no time). The
unit test asserts byte-identical output (`bytemuck::cast_slice` over
`vertices`) across two invocations and against a small pinned checksum-style
expectation is **not** used — no golden fixture is created for presentation
output; the assertions are structural (§10), so future presentation tuning
(palette, lighting) never trips a determinism gate. What must be exact is
pinned exactly: vertex height == `elevation()`, cell-center color ==
`composite_cell_color`.

## 7. Chunk lifecycle (`PovChunkManager`)

Mirrors `AtlasManager` (`platform-native/src/gpumap.rs`) — walk resident
regions, compare keys, schedule stale work, amortize uploads, evict farthest
— with the one structural difference that meshing is *asynchronous* (CPU
work on the executor) where atlas packing is synchronous.

### 7.1 Keying

```rust
fn chunk_key(map: &RegionMap, state: &RegionState) -> Option<u64>
```

= `mix` of `AtlasManager::region_key(map, coord)` (the fold of every present
tile's `dep_hash` + presence mask, `gpumap.rs:61` — ADR 0008 doing
presentation work) with the terrain-domain quantized buckets from §6.2.
`None` (tiles not yet settled/present: needs `CHANNEL_RIVER`,
`CHANNEL_WETNESS`, `biome`, `dominant`) means *no chunk yet* — holes at the
loading frontier are acceptable in 3D-1 and hide in fog, shrinking as
generation catches up (design §3.3). Steady state: same tiles, same buckets
⇒ same key ⇒ zero remesh traffic, exact by the same argument that makes
atlas upload-skipping exact.

### 7.2 Scheduling and cancellation

Per frame (POV mode only), `PovChunkManager::sync(map, center, radius,
executor)`:

1. For each region within `WER_POV_RADIUS` (walked in a fixed row-major
   order — determinism of *scheduling order* is not identity-relevant, but
   fixed order keeps behavior reproducible), compute `chunk_key`.
2. If the held mesh's key matches, do nothing (steady state).
3. Otherwise, if no job with that key is in flight, snapshot the inputs
   (`Arc` tile clones + the derived `PossibilityVector` + key), create an
   `Arc<AtomicBool>` cancellation token, and
   `executor.submit(TaskPriority::Background, job)` — the same
   `Box<dyn TaskExecutor>` the world update uses (`World.executor`), so
   `wer --inline` remains the A/B that runs meshing synchronously too. The
   job checks the token before starting and between row batches, no-ops if
   cancelled (the exact pattern generation jobs use; there is deliberately
   no executor-level cancel API), and sends `(coord, key, ChunkMesh)` back
   over an `mpsc` channel.
4. Superseding: if a region's key changes while a job for the old key is in
   flight, cancel the old token and schedule the new job. If a region
   leaves the radius, cancel its in-flight job. Stale results arriving
   anyway (key no longer wanted) are dropped on receive and counted.

Meshing never runs on the render thread; a mesh job is ~4.5 k
`elevation` samples + color packing, well under a generation pass, and
Background priority keeps it behind Critical/Normal generation work by
construction (`stream.rs` assigns Critical inside near radius — terrain
tiles a chunk needs always outrank the chunk itself).

### 7.3 Amortized integration and upload

Each frame, drain at most `POV_UPLOADS_PER_FRAME = 4` finished meshes from
the channel (start at 4, tune against `docs/perf-baseline.md` methodology —
design §2.2) into `TerrainChunkUpload`s handed to `render_pov`. A remesh of
an existing region keeps drawing the old chunk until the replacement upload
lands (swap, not hole). Remaining finished meshes stay queued for later
frames.

### 7.4 Eviction

Capacity = `(2 · radius + 1)² + POV_CHUNK_SLACK` (slack 8, hysteresis like
the region caches). When over, evict **farthest-first** from the camera
(same discipline as every Phase 6 cache), emitting the handle in `removes`
so the renderer pools the buffer. Leaving POV mode does *not* drop chunks
(cheap to keep, instant on re-entry); a full drop happens only on capacity
pressure or radius shrink.

### 7.5 Counters

`PovChunkManager` keeps totals: `meshed`, `remeshed`, `cancelled`,
`dropped_stale`, `uploads_deferred`, worker-side mesh milliseconds
(accumulated via atomic, telemetry only — never gating, ADR 0018 posture).
Logged on the existing once-per-second telemetry cadence. The steady-state
exit criterion reads these: travel stopped ⇒ `remeshed` stays flat.

## 8. Shell integration (`platform-native/src/main.rs`)

### 8.1 Mode state and frame branch

`enum ViewMode { Map, Pov }` on `App` (default `Map`; `WER_POV` set ⇒
`Pov`). `App::frame()` branches after `world.update()`:

- **Map:** exactly today's path, byte-for-byte — the GPU/CPU compose branch,
  HUD, panel, cursor info all untouched.
- **Pov:** update the fly camera from input (§8.3), set
  `world.player = (camera.pos.x, camera.pos.y)` **before** `world.update()`
  so streaming, retarget, and realization recenter on the camera (travel-
  fueled drift then works identically to map-mode travel — the POV camera
  *is* the player); then `pov_chunks.sync(...)`, build `PovFrameParams`
  (glam math, §4), and `renderer.render_pov(...)`. HUD, panel, and cursor
  info are skipped in POV (design §2.1); telemetry accumulators still run.

Timing: the frame-side POV work (sync + uploads) is wrapped in a
`Pass::Mesh` span filled by the shell, following the exact `Pass::Flush`
precedent (`main.rs` fills `stats.pass_ms[Pass::Flush.index()]` itself).
This adds one variant to `Pass`/`PASS_COUNT` in `world-runtime/src/timing.rs`
— wasm-clean (the feature-gated span helper), no world logic, all
`[_; PASS_COUNT]` arrays resize automatically. If review prefers zero
world-runtime churn in a presentation phase, the fallback is a shell-local
stopwatch in the POV log line; the plan's default is the `Pass` variant so
the map-mode panel shows meshing cost like every other pass.

### 8.2 Input: cursor grab and mouse look

- Entering POV: `window.set_cursor_grab(CursorGrabMode::Locked)`, falling
  back to `Confined` (the documented winit fallback chain — X11 under WSL2,
  the primary dev environment, supports Confined; with Confined, re-center
  the cursor on each motion event as the standard workaround), then
  `set_cursor_visible(false)`.
- Mouse look reads **raw deltas** via a new `device_event` handler on the
  `ApplicationHandler` impl (`DeviceEvent::MouseMotion`) — the shell has no
  `device_event` today; deltas apply `yaw -= dx · sens`,
  `pitch = (pitch − dy · sens).clamp(±89°)`, active only in POV with grab
  held.
- `Escape` in POV: first press releases the grab and shows the cursor
  (stay in POV, look frozen); a click inside the window re-grabs; a second
  `Escape` (ungrabbed) exits as today. Map mode `Escape` is unchanged.
- `Tab` toggles Map ↔ POV; leaving POV always releases the grab.

### 8.3 Fly movement

Presentation-side camera state only — nothing touches world state, saves,
or the vault beyond the player-position recentering in §8.1:

- `W`/`S` along the full 3D view direction (pitch included — it is a fly
  camera), `A`/`D` strafe in the yaw plane, `Space`/`LShift` world up/down,
  integrated per frame from `keys_down` (the existing held-key pattern in
  `apply_movement`, which is bypassed entirely in POV).
- Speed: base `POV_FLY_SPEED = 40.0` world units/s (person-ish but brisk;
  `PLAYER_SPEED = 500.0` is a map-scale constant, wrong for eye level),
  scroll wheel multiplies/divides by 1.5 per notch, clamped to
  `[2.0, 2000.0]`. The map-mode zoom handling of `MouseWheel` is gated to
  Map.
- No collision of any kind: the camera flies through hills. `Shift` is
  consumed by down-movement, so map-mode sprint semantics don't apply in
  POV.

### 8.4 Keybinding gating

`handle_press` gains an early mode gate: in POV, only `Tab`, `Escape`, and
(from 3D-2) `F` are handled; every existing binding — digits, `V`, `,`, `.`,
`Q`/`E`, `K`/`T`/`Y`, `O`/`L`, `G`/`N`/`X`/`M`, etc. — remains
**map-mode-only** (design §2.1). Held-key movement is dispatched per mode.
This gate is the whole guarantee that map mode is pixel-identical: no map
state can even be touched from POV.

### 8.5 Environment variables

- `WER_POV=1` — start in POV mode.
- `WER_POV_RADIUS=n` — chunk draw radius in regions, default 3 (the
  existing near radius), clamped to `[1, 8]`; also drives fog distances
  (§4) and chunk capacity (§7.4). The llvmpipe escape hatch.
- Existing `WER_CPU_MAP`, `WER_TIER`, `WER_CACHE_MB`, `WER_PRESENT_MODE`
  unaffected.

## 9. Performance posture

- **Budgets:** 49 chunks / ~427 k triangles / ~11 MB GPU (vertices +
  shared indices) at radius 3 — chosen for interactivity on the
  WSL2/llvmpipe reference environment (design §2.2). One pipeline, one bind
  group switch, ≤ 49 draws (or one multi-draw later; not needed now).
- **Steady state:** zero mesh jobs, zero uploads (dep-hash keying, §7.1),
  zero GPU buffer allocation (pool, §5.5), fixed per-frame cost = camera
  math + uniform write + draws.
- **Measurements recorded in `docs/perf-baseline.md` before sign-off:**
  mesh ms/chunk (worker-side), chunk bytes, uploads/frame during cold
  entry and during a drift storm, llvmpipe frame ms in POV at radius 3 vs.
  the 2D GPU-map baseline, `wer --inline` A/B sanity.

## 10. Testing and CI

Unit tests inline (`#[cfg(test)]` in `pov.rs`/`viz.rs`, the
`gpumap.rs`/`viz.rs` precedent — platform-native is bin-only, no `tests/`
dir):

1. **Mesher determinism:** two calls with identical inputs ⇒ byte-identical
   vertex bytes (`bytemuck::cast_slice`) and heights.
2. **Height exactness:** every core vertex height equals scalar
   `world_core::terrain::elevation(x, y, &p)` bit-exactly (the ADR 0016
   twin guarantee, re-asserted at the consumer).
3. **Color agreement:** for tiles built from a known vector, the vertex at
   each cell center equals `composite_cell_color` of that cell; and the
   refactored 2D `paint_region` output is byte-identical to the
   pre-refactor snapshot for a fixed input (pins the hoist).
4. **Skirt watertightness:** every perimeter core vertex has a skirt
   partner at identical (x, y, color), z lowered by exactly
   `POV_SKIRT_DROP`; the shared index topology references every boundary
   edge exactly once; index count and vertex count equal the published
   constants.
5. **Winding:** every core triangle's cross product has positive z in
   chunk-local space (consistent CCW for back-face culling).
6. **Normals:** unit length within 1e-5; flat input (constant-height stub —
   test the normal helper on a synthetic heightfield) ⇒ exactly (0, 0, 1).
7. **Chunk keying/lifecycle** (pure bookkeeping, no GPU, the
   `AtlasManager` test style): same tiles + same buckets ⇒ same key;
   bucket flip ⇒ new key; stale-result drop; farthest-first eviction
   order; amortization cap respected.
8. **Renderer:** naga validation of `pov_terrain.wgsl` in
   `renderer/tests/wgsl.rs`; buffer-pool bookkeeping unit tests (§5.7).

CI matrix: everything above runs in plain `cargo test --workspace`; **no
golden fixture is added or changed** (`world-core/tests/determinism.rs`
untouched — presentation output is structurally asserted, never golden-
blessed, §6.6); wasm check unaffected (`world-runtime` change, if taken, is
the feature-gated `Pass` variant only); `RUSTFLAGS="-D warnings" cargo
clippy --workspace --all-targets` clean; `cargo fmt --all -- --check`.

Manual verification on the reference environment (WSL2/llvmpipe, X11):
enter POV, fly a region border and confirm no cracks; hold still and
confirm the remesh counter stays flat; drop an anchor from map mode, re-
enter POV, and confirm only dep-hash-dirtied regions remesh; resize the
window (depth texture recreation); `Tab` back and confirm the map renders
as before.

## 11. Milestones

Each lands independently green on the full CI matrix.

- **M1 — Renderer groundwork.** `pov.rs`, depth texture + resize handling,
  pipeline, `pov_terrain.wgsl` + naga test, `render_pov` drawing a
  hardcoded synthetic chunk (procedural sine heightfield built shell-side)
  under a temporary fixed camera. Proves: depth correctness, lighting, fog,
  vertex format, uniform plumbing — before any world data is involved.
  *Exit:* synthetic terrain renders depth-correct with lighting and fog;
  wgsl test green; 2D paths untouched.
- **M2 — Mesher.** Pure `mesh_region_chunk` + `chunk_indices` + the
  `composite_cell_color` hoist, with unit tests 1–6 and the 2D-refactor
  pin. No shell wiring yet. *Exit:* all mesher tests green; `paint_region`
  refactor byte-identical.
- **M3 — End to end.** `ViewMode`, `Tab`, cursor grab + `device_event`
  mouse look, fly movement, player recentering, `PovChunkManager` (sync,
  Background-lane jobs, integration, upload) wired to `render_pov`.
  *Exit:* fly through the world at radius 3; region borders crack-free;
  holes only at the loading frontier; map mode pixel-identical; `--inline`
  works.
- **M4 — Lifecycle hardening.** Cancellation + supersession, stale-result
  drops, farthest-first eviction + buffer pooling, amortization cap,
  counters + log line, `Pass::Mesh` span, lifecycle unit tests (7).
  *Exit:* steady-state remesh counter is zero; a scripted drift storm
  remeshes only dep-hash-dirtied regions; cold entry respects the
  uploads-per-frame cap.
- **M5 — Sign-off.** `WER_POV`/`WER_POV_RADIUS` finalized, fog constants
  tuned on llvmpipe, `docs/perf-baseline.md` POV section recorded, README
  controls note, design-doc exit-criteria walkthrough (§12).

## 12. Phase exit criteria (design §3.5, restated checkable)

- [ ] `Tab` toggles POV on/off; map mode is pixel-identical to before
      (guaranteed structurally by the §8.4 gate; spot-checked with the
      existing `--screenshot` path, which never enters POV).
- [ ] Fly camera: full 3-axis movement, mouse look under grab, wheel speed
      control, pitch clamped.
- [ ] Terrain out to radius 3 with Lambert + hemisphere lighting, distance
      fog, and per-vertex material color consistent with the 2D Composite
      channel over the same regions (cell-center equality, tested).
- [ ] Depth-correct rendering (hills occlude); no seam cracks at region
      borders (skirts, tested watertight).
- [ ] Steady-state remesh traffic is zero (counter, logged — atlas-delta
      methodology).
- [ ] `cargo test --workspace` green with **no golden fixture changes**;
      mesher determinism and skirt tests green; wasm CI check unaffected;
      clippy/fmt clean under `-D warnings`.
- [ ] `docs/perf-baseline.md` updated with the POV numbers on the
      WSL2/llvmpipe reference environment.

## 13. Risks and open questions

- **llvmpipe fill rate.** 427 k triangles vertex-lit is sized to be fine,
  but llvmpipe is fragment-bound at high resolutions. Mitigations already
  in the design: `WER_POV_RADIUS`, fog-tightening, and (if needed later) a
  half-resolution render target — not built speculatively.
- **Cursor grab under WSLg/X11.** `Locked` may be unsupported on X11; the
  Confined + re-center fallback is specified (§8.2) and must be tested on
  the reference environment early (M3), since it is the primary dev box.
- **Color-space drift between 2D and 3D.** The 2D map shows raw sRGB bytes;
  the 3D path lights in linear and re-encodes. Colors will read *related*,
  not identical, on screen — the exit criterion is defined at the data
  level (cell-center byte equality pre-lighting), which is the testable
  claim. Lighting constants (sun/ambient) are tuned so mid-day flat ground
  roughly matches the 2D palette's value range.
- **Mesh/tile transient mismatch during drift.** Between a terrain-bucket
  flip and tile regeneration, mesh height (new buckets) and colors (old
  tiles) can disagree for a few frames. §7.1 keys the chunk on both, so
  the chunk remeshes again when tiles settle; the window is short and
  visually minor (colors lag height). Accepted for 3D-1; noted so 3D-3's
  water work doesn't build on it by accident.
- **`Pass::Mesh` in world-runtime.** Smallest possible neutral-crate touch
  (telemetry enum), but a reviewer may prefer zero; the shell-local
  fallback is specified (§8.1). Decide at M4, not before.
- **Per-chunk uniform mechanics.** Dynamic-offset uniform vs. one small
  uniform write per draw: start with dynamic offsets (one buffer, one
  `write_buffer` per frame); if wgpu alignment padding makes it awkward,
  per-draw writes at ≤ 49 draws are equally fine. Implementation detail,
  flagged so it isn't bikeshedded in review.
