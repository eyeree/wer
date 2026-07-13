# Phase 3D-3 — Water: Implementation Plan

This is the lower-level plan for the third phase of
[`3d-design.md`](3d-design.md) (§5 there): the ocean reads as water, rivers
and wetlands read on the land. No organisms (3D-4), no carved channels, no
lake basin-fill, no swimming (design §8). The scope is honest about the
model: there is **no water-depth field and no lake surface**, so this phase
renders exactly what the model knows — `elevation < SEA_LEVEL` is open
water, `CHANNEL_RIVER`/`CHANNEL_WETNESS` are the hydrology expression on
land.

Read [`AGENTS.md`](../../../AGENTS.md) first. This plan does not repeat the
crate-boundary rule, the determinism invariant, or the CI contract — it
assumes them, and it builds on the **landed** 3D-1/3D-2 implementation
(`platform-native/src/pov.rs`, `renderer/src/pov.rs`,
`renderer/shaders/pov_terrain.wgsl`), not on the earlier plans'
pre-implementation guesses. Where those diverged (32-byte `PovVertex` with
baked `light` bytes, left-button-drag look, `--pov-script`/`PovCapture`,
`POV_SKIRT_DROP = 128`), this plan follows the code.

One sentence of orientation, because it governs everything below: **3D-3 is
derived presentation only, and it is cheap by construction** — the sea
surface is a single translucent quad at `z = SEA_LEVEL = 0.0`, which is
exactly correct for every possibility state because `elevation()` already
folds the Planetary sea shift in (design §1, `elevation_from_relief`,
`world-core/src/terrain.rs`); rivers and wetlands stay **on the terrain** as
material response and an optional terrain-conformal overlay. No new data is
generated, no neutral crate is touched at all, `WORLD_ALGORITHM_VERSION`
stays at 2, every `algorithm_revision` stays at its current value, zero
golden fixtures are re-blessed, and no readback API is added (ADR 0017; the
`PovCapture` debug carve-out is ADR 0021 and is reused unchanged).

---

## 1. Goals and non-goals

### 1.1 Goals (design §5.3, restated as deliverables)

- A **sea surface**: a camera-centered translucent quad at
  `z = SEA_LEVEL = 0.0`, drawn after terrain with depth-test on /
  depth-write off, alpha-blended, in its own `pov_water.wgsl`. Fresnel-ish
  color between deep-water blue and the sky (fog color), a sun glint from a
  time-wobbled normal (display-only animation — frame time never feeds back
  into world state), fogged like terrain. Coastlines are **correct by
  construction** at every possibility state: the plane at 0 against the
  sea-shift-folded elevation is the same `is_water(e) == (e < 0.0)` the
  whole stack uses.
- A **depth cue for free**: the 3D-1 terrain already paints underwater
  ground with the `elevation_color` sediment ramp (deep → dark blue,
  `viz.rs`), so a fixed-translucency plane over it reads deeper where the
  bottom is deeper. Scene-depth sampling is **explicitly not built** (the
  design allows shipping flat translucency first; the ramp makes even that
  depth-graded).
- A **wet-material terrain response**: the mesher packs bilinear
  `CHANNEL_RIVER` and `CHANNEL_WETNESS` into the two reserved vertex
  `light` bytes; `pov_terrain.wgsl` adds a specular sun-glint term gated by
  wetness and the baked sun visibility. Rivers read as glossy blue ribbons
  along the (already elevation-consistent) drainage lines; the ribbon
  *color* is already there from `composite_cell_color` — the gloss is what
  the 2D map cannot show. Vertex albedo does **not** change, so the 3D-1
  cell-center color-equality guarantee stands untouched.
- **River overlay strips** (design §5.2 step 2, built behind a milestone
  decision point): a translucent mesh lifted a few decimeters above the
  terrain over exactly the core-lattice triangles whose river intensity
  exceeds a threshold — terrain-conformal, reusing the terrain vertex
  buffers, feathered to zero alpha at the threshold so there is no hard
  edge. Ships only if the material-pass ribbons "look too painted" on the
  reference environment; the design orders the two steps by payoff.
- **Capture parity**: the F12 POV dump and every `--pov-script` snap render
  the water passes automatically (they drive the same `Pov::draw`), with a
  **fixed time of 0** so snapshots stay reproducible.
- **Walk mode unchanged**: the sea floor is still the ground. Nothing in
  `ground_height`, `walk_ground`, or the kinematics is touched.

### 1.2 Non-goals (deferred to later 3D phases or design §8)

- Carved river channels, sloped river free surfaces, and lake basin-fill
  from `FLOW_NONE` sinks (needs a derived water-surface pass over the
  drainage tile — design §8). Lakes remain terrain-colored wet basins.
- Wading, swimming, underwater tinting/fog/caustics, reflections, refracted
  or depth-sampled water. Walking below `z = 0` keeps working exactly as
  3D-2 shipped it; the underwater *view* (camera below the plane) merely
  shows the plane from beneath, untuned.
- Any shadowing of open water (the sea has no baked `light` bytes; it takes
  the full sun term — accepted, noted in §12).
- Organisms (3D-4), the `Expressed::form` passthrough, any
  `world-core`/`world-runtime` change of any kind.
- New env vars, new script instructions, new counters. The phase adds
  surfaces to draw, not lifecycle machinery.

## 2. Contracts this phase must not break

- **Determinism.** No new identity, no new persistence, no generation-path
  change. The sea plane is a constant; the wet-material bytes and overlay
  triangles are pure functions of the same settled tiles the mesher already
  snapshots (same `chunk_provenance` key — same inputs ⇒ same mesh ⇒ zero
  new remesh traffic). Shader time is display-only and never leaves the
  GPU; captures pin it to 0.
- **Crate boundaries.** `world-core` and `world-runtime` are untouched —
  this phase has **no neutral-crate diff at all**. The renderer stays
  world-agnostic: it never learns `SEA_LEVEL`; the shell passes the water
  plane's camera-relative height as a plain float, the same posture as
  every other `PovFrameParams` field.
- **CI.** Lands green on the full matrix: `fmt --check`, `clippy` with
  `-D warnings`, `cargo test --workspace` with **no golden fixture
  changes**, wasm check unaffected by construction. New WGSL is validated
  GPU-free in `renderer/tests/wgsl.rs` (naga), like the three existing
  shaders.
- **Map mode and walk mode.** The `handle_press` POV gate is not modified;
  no map binding changes; the 2D map path is byte-identical. The walk path
  reads the same `heights` lattices — sea-floor walking is design-mandated
  and already works.
- **The 3D-1 color guarantee.** `composite_cell_color` is not modified and
  vertex `color` bytes do not change — the existing
  `cell_center_colors_match_the_2d_composite` test is the guard, unchanged.

## 3. New and touched surfaces

| Surface | Change |
|---------|--------|
| `crates/renderer/shaders/pov_water.wgsl` (new) | Sea-plane entry points (`vs_sea`/`fs_sea`) and river-overlay entry points (`vs_overlay`/`fs_overlay`); shared wobble/Fresnel helpers; its own copy of the `PovParams`/`ChunkOffset` structs (layout-identical to `pov_terrain.wgsl`'s). |
| `crates/renderer/shaders/pov_terrain.wgsl` | `light.zw` consumed as river/wetness; camera-relative position interpolator; wet specular sun-glint term. |
| `crates/renderer/src/pov.rs` | `SHADER_POV_WATER`; two new pipelines (sea, overlay) built beside the terrain pipeline; `PovParamsRaw` + WGSL `PovParams` gain one `water: vec4<f32>` (time, plane z, wobble-anchor fraction ×2, §4.3); optional per-chunk overlay index buffer in `ChunkSlot`/`ChunkTable` (exact-size, freed not pooled, §6.3); `TerrainChunkUpload::river_indices`; draw order terrain → overlay → sea in `Pov::draw`. `PovCapture` inherits everything through `Pov`. |
| `crates/renderer/src/lib.rs` | Re-export `SHADER_POV_WATER`. `render_pov` signature unchanged. |
| `crates/renderer/tests/wgsl.rs` | naga parse+validate of `pov_water.wgsl`. |
| `crates/platform-native/src/pov.rs` | Mesher packs river/wetness into `light[2]`/`light[3]` and emits `ChunkMesh::river_indices` (§6.1); new constants (§6.2); `frame_params` gains a `time` argument and fills `PovFrameParams::time`/`water_z`; unit tests. |
| `crates/platform-native/src/main.rs` | App-start `Instant` → wrapped shader time (§7.1); `frame_pov` passes it; `run_pov_script` and `dump_pov_screenshot` pass 0.0; overlay index bytes counted into the upload-bytes telemetry. |
| `README.md` | One line in the POV note (water surface; rivers glint). |
| `docs/perf-baseline.md` | POV water numbers (§8): llvmpipe frame ms at radius 3 before/after, overlay index bytes over a river-heavy ring. |

Nothing else. In particular `viz.rs`, `dump.rs`'s `state.txt`, the chunk
keying, scheduling, eviction, and every walk/fly path are untouched.

## 4. The sea surface (design §5.1)

### 4.1 Geometry: one camera-centered quad, generated in the vertex shader

A single quad, not a grid — the wobble perturbs **normals only**, never
positions, so there is nothing for a grid to tessellate. The vertex stage
generates 4 corners from `@builtin(vertex_index)` (triangle-strip, no vertex
buffer, no index buffer): camera-relative
`(±R, ±R, water_z)` with `R = fog_end` — at `fog_end` every fragment is pure
fog color (= the clear color = the sky), so the quad's rim is invisible by
construction, and the loading frontier and world edge hide exactly as they
do for terrain. At `WER_POV_RADIUS = 8` the quad's corners exceed
`zfar = 2048` and clip; the clipped fragments would have been pure sky
color, so nothing visible is lost (same argument as the rim; noted in §12).

`water_z` is the **camera-relative** plane height, `SEA_LEVEL − camera.z`,
computed by the shell in f64 inside `frame_params` and truncated — the same
far-from-origin discipline as the per-chunk offsets, and the reason the
renderer never needs to know `SEA_LEVEL` exists.

The pipeline: `pov_water.wgsl` `vs_sea`/`fs_sea`, bind group 0 only (the
frame uniform), **no cull** (the camera may be below the plane — walking
the sea floor is allowed), depth compare `Less` with
`depth_write_enabled: false`, standard alpha blending
(`BlendState::ALPHA_BLENDING`), drawn **last** (§4.4).

### 4.2 Shading

- **Fresnel-ish color and opacity.** `cos θ = |view.z|` against the plane
  normal (view = normalized fragment→camera). Schlick:
  `F = 0.02 + 0.98 · (1 − cos θ)⁵`. Color
  `mix(DEEP_WATER, fog_color, F)` with `DEEP_WATER` a shader const chosen
  beside `elevation_color`'s deep anchor (`[8, 16, 64]` sRGB, decoded the
  same cheap `pow 2.2` way the terrain shader decodes vertex color); alpha
  `mix(0.55, 0.88, F)` — looking straight down you see the sediment ramp
  through the surface (the free depth cue, §1.1), at grazing angles the
  surface goes opaque-ish sky, which is what real water does.
- **Sun glint.** The same `SUN_DIR`/`SUN_STRENGTH` regime as terrain:
  specular `pow(max(dot(reflect(sun_dir, n), view), 0), 60) `, with `n` the
  wobbled normal (§4.3), added on top of the Fresnel color. No baked sun
  visibility exists for open water; the glint is unshadowed (§12).
- **Fog.** Identical formula to terrain: `mix(lit, fog_color,
  smoothstep(fog_start, fog_end, dist))`. The plane dissolves into the sky
  at the same range the ground does.

All tuning values are shader `const`s (the `SUN_STRENGTH` precedent), not
uniforms — nothing per-frame varies except time and `water_z`.

### 4.3 Time and the wobble anchor

`PovFrameParams` gains `time: f32`; `PovParamsRaw` and both WGSL `PovParams`
structs gain one `water: vec4<f32>` = `(time, water_z, frac_x, frac_y)` —
one vec4, appended after `detail`, no other layout churn.

- **Time** comes from the shell: seconds since app start, **wrapped at
  `WOBBLE_PERIOD = 32.0` s** (`elapsed.rem_euclid(32.0)`), and every wobble
  angular frequency in the shader is an integer multiple of `2π / 32` — so
  the wrap is seamless and f32 never accumulates precision loss. Captures
  (`run_pov_script`, `dump_pov_screenshot`) pass `0.0`: a scripted snap is
  reproducible, and two snaps of the same pose are comparable.
- **The wobble spatial anchor.** The wobble must be pinned in *world* space
  or it would swim as the camera moves, but fragment positions are
  camera-relative and `camera_pos` alone exceeds f32 at ±10⁶. The renderer
  (which already holds `camera_pos` in f64) writes
  `frac = camera_pos.xy.rem_euclid(64.0)` into the vec4; the shader's
  wobble input is `pos.xy + frac`, and every wobble wavelength divides 64
  (powers of two: 32, 16, 8 units). For a fixed world point that sum is
  constant between 64-unit camera tile crossings and jumps by exactly one
  whole number of periods at a crossing — invisible, the per-chunk analogue
  of the detail-noise integer anchoring already in `pov_terrain.wgsl`. This
  is float-precision plumbing, not world knowledge; the renderer stays
  world-agnostic.
- **The wobble itself**: 3 summed directional sines (distinct directions,
  wavelengths 32/16/8, small apparent-slope amplitudes ~0.02–0.05), normal
  reconstructed analytically as `normalize(-∂w/∂x, -∂w/∂y, 1)`, amplitude
  faded by `1 / (1 + dist · k)` so distant water doesn't shimmer on
  llvmpipe's per-pixel grid. Pure display math; frame time reaches nothing
  but this shader (design §5.1 allows exactly this).

### 4.4 Draw order and blending

`Pov::draw` records, in one render pass: **terrain (opaque, depth-write on)
→ river overlay (blended, depth-write off, §6) → sea (blended, depth-write
off)**. Fixed order, not sorted: the overlay hugs the terrain, so wherever
both it and the sea cover a pixel the sea is the nearer surface when the
camera is above water (river mouths seen from the air tint through the sea,
correctly) — and when the camera is *below* the plane the error is confined
to the untuned underwater view that is out of scope anyway (§1.2). Two
extra `set_pipeline` calls per frame; the frame bind group is shared, the
overlay reuses the already-bound chunk bind group scheme.

## 5. Wet-material terrain response (design §5.2 step 1)

### 5.1 The two reserved light bytes

`PovVertex.light` is `[sun visibility, ambient occlusion, reserved,
reserved]`. The mesher already computes bilinear river and wetness per
vertex for the albedo (`bilinear(inputs.river, …)`, `mesh_region_chunk`);
it now also packs them:

```rust
light: [
    quantize_light(sunvis[...]),
    quantize_light(vertex_ao(...)),
    quantize_light(river),   // was 0 (reserved)
    quantize_light(wetness), // was 0 (reserved)
],
```

The skirt ring copies whole vertices, so the existing
`skirt_is_watertight` assertion (`top.light == bottom.light`) holds without
modification. 3D-4 needs neither byte (organisms are a separate instanced
pipeline; the `form` passthrough rides `Expressed`), so spending both here
is free.

Two bytes rather than one premixed "gloss" byte because the overlay (§6)
feathers on **river alone** — folding wetness in CPU-side would bleed
marsh into the river ribbon's alpha.

### 5.2 The shader response

`pov_terrain.wgsl` changes, all fragment-stage:

- `VsOut` gains the camera-relative position (`@location(5)`), and `light`
  widens from `vec2` to `vec4` — the vertex stage passes `in.light`
  through whole.
- `wet = max(light.z, 0.6 · light.w)` — rivers dominate; wetlands get a
  weaker version of the same response.
- **Specular sun glint**: `spec = wet · light.x ·
  pow(max(dot(reflect(sun_dir, n), view), 0), 40) · 0.5`, added to the lit
  color before fog. Gated by the baked sun visibility (`light.x`) so
  shadowed water doesn't sparkle; uses the detail-perturbed normal `n`
  that's already computed, so wet ground glitters with the terrain's
  micro-relief for free.
- **No albedo change.** The blue-ribbon and marsh-darkening color response
  already arrived in 3D-1 through `composite_cell_color` (river lerp ×0.8,
  wetness ×0.25) — doubling it here would desynchronize the 3D ground from
  the 2D map. The gloss term is exactly the part the 2D map cannot express,
  which is why it is the whole of this step.

This is design §5.2's "cheap, ship first" step, and it is the phase's
default answer for rivers: elevation-consistent (the drainage solver ran on
the same terrain), zero new geometry, zero new lifecycle.

## 6. River overlay strips (design §5.2 step 2 — behind the M3 decision)

Built only if the material-pass ribbons read as painted-on rather than as
water on the reference environment (the design's own criterion). The design
asks for translucent geometry "a few centimeters above the terrain along
cells where river intensity exceeds a threshold, width from `river`,
sharing the water shader's wobble — still terrain-conformal, still no depth
field." The cheapest correct shape for that:

### 6.1 Reuse the terrain lattice: an index-subset overlay

The overlay is **a subset of the core terrain triangles, drawn again**
through a second pipeline that lifts them by `RIVER_LIFT` and shades them
as water. No new vertices, no new mesher sampling — the mesher already has
per-vertex river values (§5.1); it additionally emits:

```rust
pub struct ChunkMesh {
    pub vertices: Vec<PovVertex>,
    pub heights: Vec<f32>,
    /// Core-topology triangles (index triples into `vertices`) where any
    /// corner's river intensity reaches RIVER_OVERLAY_MIN and at least one
    /// corner is at or above sea level — the river-overlay draw list.
    /// Empty ⇒ no overlay draw for this chunk.
    pub river_indices: Vec<u32>,
}
```

Selection rule, applied while walking the same `j`/`i` quad loop (and the
same v00→v11 diagonal split) as `renderer::pov::chunk_indices`:

- include a triangle iff `max(river at its 3 corners) ≥ RIVER_OVERLAY_MIN`
  — the *any-corner* rule plus per-vertex alpha feathering (§6.2) means the
  ribbon edge fades to zero exactly at the threshold, so no hard boundary
  exists to alias;
- **and** `max(elevation at its 3 corners) ≥ SEA_LEVEL` — fully submerged
  river cells near a coast are already under the sea plane; drawing a
  second water surface beneath the first would just darken the estuary.
  Straddling coastline triangles stay, and feather into the sea.

`TerrainChunkUpload` gains `river_indices: Vec<u32>` (empty for most
chunks); everything rides the existing upload/supersede/evict lifecycle
untouched — same provenance key, same amortization, same counters. Border
honesty: river values come from the region's own tiles clamped at the
border (the 3D-1 `bilinear` rule), so a ribbon can step by a hair at a
region seam exactly as its *color* already does; accepted, same rationale.

### 6.2 The overlay pipeline and shading

`pov_water.wgsl` `vs_overlay`/`fs_overlay`: bind groups 0 + 1 (it needs the
chunk offset), the full 32-byte `PovVertex` layout, cull back (terrain
winding, seen from above), depth `Less` / depth-write off, alpha blend,
drawn between terrain and sea (§4.4).

- Vertex stage: terrain transform plus `pos.z += RIVER_LIFT`
  (`RIVER_LIFT = 0.2` world units — high enough that `Depth32Float` at
  these ranges never z-fights, low enough to read as surface; a
  `DepthBiasState` fallback is noted in §12, not built).
- Fragment: water color `mix(RIVER_SHALLOW, fog_color, F)` with the same
  Schlick term against the *terrain* normal, the shared wobble (anchored in
  **chunk-local** coordinates — `in.position.xy` is exact and continuous,
  and every wobble wavelength divides `REGION_SIZE`, so ribbons are
  seamless across borders without the §4.3 camera-frac trick), the sun
  glint gated by `light.x`, and alpha
  `0.45 · smoothstep(RIVER_OVERLAY_MIN, RIVER_OVERLAY_FULL, light.z)` —
  the feather that makes the any-corner selection edge invisible and the
  ribbon width track intensity, "width from `river`" without any geometry
  fitting.

### 6.3 Renderer buffer management

`ChunkSlot` gains `overlay: Option<(wgpu::Buffer, u32)>` (buffer + index
count). Overlay index buffers are **variable-size, exact-size, and not
pooled** — pooling was trivial for vertex buffers because every chunk is
the same size; that invariant is deliberately not stretched. Dropping them
on evict is correct because steady-state remesh traffic is zero (the 3D-1
exit criterion), so steady state allocates nothing; a re-upload replaces
the option wholesale (including `Some → None` when a remesh loses its
river). The pure `ChunkTable` bookkeeping stays device-free and keeps its
unit tests (§9, test 7).

### 6.4 Constants (tuned at M3 on the reference environment)

```rust
/// River intensity where the overlay begins (feathered to zero here).
pub const RIVER_OVERLAY_MIN: f32 = 0.08;
/// River intensity of a fully opaque-alpha ribbon core.
pub const RIVER_OVERLAY_FULL: f32 = 0.30;
/// Overlay lift above the terrain surface, world units.
pub const RIVER_LIFT: f32 = 0.2;
```

The 2D map draws the river channel continuously (`river · 0.8` color lerp,
no threshold); the overlay threshold is a new, overlay-only constant. The
design's "same tiles, same threshold" exit criterion is met at the data
level: both views read the identical `CHANNEL_RIVER` tile, and the POV
ribbon (material pass) is continuous exactly like the map's.

## 7. Shell integration

### 7.1 Time plumbing

`App` gains `start: Instant` (set once). `frame_pov` computes
`time = start.elapsed().as_secs_f64().rem_euclid(WOBBLE_PERIOD) as f32` and
passes it to `frame_params`, whose signature grows by that one argument and
which also fills `water_z = f64::from(world_core::SEA_LEVEL) −
camera.pos.z` (truncated). `run_pov_script` and `dump_pov_screenshot` pass
`0.0` — captures are time-frozen by policy (§4.3). No other shell change:
no new keybindings, no gate change, no dump fields (the camera pose already
in `state.txt` fully determines a water frame at time 0).

### 7.2 Telemetry

The upload-bytes figure in `frame_pov` adds each upload's
`river_indices.len() * 4` so the existing once-per-second line stays
honest. No new counters — the overlay creates no new lifecycle events, and
`PovCounters` is untouched.

## 8. Performance posture

- **Sea plane:** 2 triangles, but a large blended fragment footprint —
  the one genuine llvmpipe risk in this phase (§12). It is depth-tested
  against already-drawn terrain, so above-water land occludes it; the cost
  concentrates where water is actually visible.
- **Wet material:** a handful of fragment ALU ops on the existing terrain
  pass; two vertex bytes that were already uploaded as zeros. Zero
  measurable delta expected.
- **Overlay:** river cells are sparse (a few percent of triangles in
  river-bearing regions); upload adds `4 · indices` bytes per such chunk.
  Steady state remains zero jobs / zero uploads / zero allocations — the
  overlay changes nothing about keying or scheduling.
- **Recorded in `docs/perf-baseline.md` before sign-off:** llvmpipe frame
  ms at radius 3 over an ocean vantage (worst-case sea fill) and a river
  valley, against the 3D-2 baseline; overlay index bytes over a
  river-heavy ring; confirmation that mesh ms/chunk moved only noise
  (the mesher's new work is two quantizations and one comparison per
  vertex).

## 9. Testing

Unit tests inline in `pov.rs` (`#[cfg(test)]`, the existing `settled_map`
fixture) and the renderer's device-free suites:

1. **WGSL validation.** `pov_water.wgsl` parses and validates under naga
   (`renderer/tests/wgsl.rs`), both entry-point pairs.
2. **Light-byte packing.** For a settled chunk, `light[2]` /`light[3]` at
   every core vertex equal `quantize_light` of the same `bilinear`
   river/wetness the albedo used; at cell centers they equal the quantized
   tile values exactly (the bilinear-at-center identity the 3D-1 tests
   already rely on).
3. **Overlay selection.** Recompute the §6.1 rule independently per
   triangle and assert `river_indices` matches; every emitted triple is a
   triangle of `renderer::pov::chunk_indices()`'s core section with the
   same winding (the §4.2-of-3D-2 style guard: a future diagonal flip
   fails here, not on screen); an all-zero river tile ⇒ empty; a synthetic
   all-above-threshold tile over land ⇒ every core triangle; corners all
   below sea level ⇒ excluded.
4. **Mesher determinism, extended.** The existing byte-identity test also
   asserts `river_indices` equality across two runs.
5. **Unchanged guards stay green, unmodified.**
   `cell_center_colors_match_the_2d_composite` (albedo untouched, §5.2)
   and `skirt_is_watertight` (light bytes copied to the skirt ring) are
   the structural proof that this phase didn't move 3D-1's contracts.
6. **`frame_params` plumbing.** `water_z == SEA_LEVEL − camera.pos.z`;
   `time` passes through verbatim; the wrap constant divides every wobble
   frequency (a `const _:` assertion beside the constants, matching the
   `OCTAVES == 5` precedent).
7. **Renderer bookkeeping.** `ChunkTable` overlay lifecycle on the pure
   generic table: upsert with `Some` indices then re-upsert with `None`
   clears it; remove drops it; vertex-buffer pooling behavior is
   unchanged by the presence of an overlay.

CI: all of the above in plain `cargo test --workspace`; no golden fixture
added or changed; wasm check unaffected (no neutral-crate diff exists);
clippy/fmt clean under `-D warnings`.

Manual verification on the reference environment (WSL2/llvmpipe, X11):
fly from high altitude down to a coast — the sea reads as translucent
water over the sediment ramp, glints toward the sun, fogs at the horizon;
drop a **Planetary anchor** in map mode, `Tab` back, and watch the sea
rise/fall against the same shore (the design's §5.3 possibility-state
scenario — the plane never moves, the land does); follow a river valley
and confirm the ribbon glints and tracks the 2D map's river channel over
the same regions (`Tab` back and forth); press `F` and walk into the sea —
the floor keeps holding the camera below the surface; `--pov-script` snap
twice at the same pose and diff the files (time-frozen reproducibility);
F12 in POV near water and check the dump screenshot shows it.

## 10. Milestones

Each lands independently green on the full CI matrix.

- **M1 — Sea surface.** `pov_water.wgsl` (sea entry points), the sea
  pipeline, the `water` vec4 through `PovParamsRaw`/`PovFrameParams`, the
  `frame_params` signature change with all three call sites, draw-order
  wiring, naga test, tests 1 and 6. *Exit:* ocean reads as water live and
  in captures; the Planetary-steer scenario behaves; map mode and 2D
  screenshots byte-identical.
- **M2 — Wet material.** Light-byte packing in the mesher, the terrain
  shader's wet glint, tests 2, 4, 5. *Exit:* rivers/wetlands glint along
  drainage lines; color-equality and skirt tests green unmodified.
- **M3 — Overlay strips, behind the decision.** First, assess M2 on the
  reference environment against the design's criterion ("if ribbons look
  too painted"). If they pass, record the decision in this file and skip
  to M4 — §6 stays specified for a later phase to pick up. If not:
  `river_indices` in mesher and upload, the overlay pipeline and entry
  points, renderer slot changes, constants tuning, tests 3 and 7,
  telemetry byte accounting. *Exit:* ribbons read as water surfaces from
  eye level; no z-fighting at any distance within fog; steady-state
  traffic still zero.
- **M4 — Sign-off.** `docs/perf-baseline.md` water numbers, README line,
  the §9 manual walkthrough, design-doc exit-criteria check (§11).

## 11. Phase exit criteria (design §5.3, restated checkable)

- [ ] Coastlines are correct by construction at every possibility state:
      the plane sits at `z = 0` while `elevation()` folds the sea shift,
      so `is_water` and the visible waterline agree everywhere; flying
      across an anchor-steered Planetary change shows the sea rise/fall
      against the same shore (manual scenario, §9).
- [ ] Rivers visible in POV match the 2D map's river channel over the
      same regions — same `CHANNEL_RIVER` tiles, bilinear per vertex,
      colors from the shared `composite_cell_color`, gloss/overlay
      feathered on the same values (tests 2 and 3).
- [ ] Walk mode (3D-2) is unchanged by water: the sea floor is still the
      ground; no walk-path code was touched (structural), and the 3D-2
      test suite is green unmodified.
- [ ] Captures (`--pov-script`, F12) render water time-frozen and
      reproducibly; map mode is pixel-identical to 3D-2.
- [ ] `cargo test --workspace` green with no golden fixture changes; wasm
      check unaffected; clippy/fmt clean under `-D warnings`;
      `docs/perf-baseline.md` updated on the reference environment.

## 12. Risks and open questions

- **llvmpipe blended fill rate.** A near-fullscreen alpha-blended quad is
  the worst case for a software rasterizer. Mitigations, in order:
  depth-tested occlusion by land already limits it to visible water; the
  wobble is a few ALU ops, not texture taps; if an ocean vantage still
  drops below interactive, shrink `WER_POV_RADIUS` (fog\_end, and with it
  the quad, shrinks too). A "skip the sea draw when no resident chunk dips
  below 0" shell-side cull is a cheap follow-on (the manager already holds
  every `heights` lattice) — noted, not built speculatively.
- **Unshadowed sea glint.** Open water has no baked sun visibility, so a
  sea in a mountain's shadow still glints. Correct data exists (the shadow
  march) but only per terrain vertex; extending it to the plane means a
  new sampled surface — out of scope, cosmetically minor at this world's
  relief scale.
- **Underwater camera.** Below the plane the quad shows its Fresnel
  backside and no underwater tint exists. The design defers
  wading/swimming wholesale; the only guarantee here is that nothing
  breaks (cull-off keeps the surface visible, walk mode keeps working).
- **Overlay z-fighting at grazing distance.** `RIVER_LIFT = 0.2` against
  `Depth32Float` at ≤ ~900 units of fog range leaves generous margin; if
  llvmpipe's depth precision surprises, the fallback is a small constant
  `DepthBiasState` on the overlay pipeline (one struct literal, flagged
  here so it isn't bikeshedded in review).
- **Radius-8 corner clipping.** The sea quad's corners pass `zfar` at
  `WER_POV_RADIUS = 8` (§4.1). Invisible today (fully fogged), but if
  `zfar` or the fog curve ever changes, the quad extent needs the
  `min(fog_end, 0.99 · zfar)` clamp. One-line fix, documented rather than
  pre-built.
- **Estuary blend order.** Overlay-then-sea is correct from above and
  merely untuned from below (§4.4). If a later phase builds the
  underwater view, revisit the fixed order then.
- **Wobble tiling artifacts.** Three sines can moiré. If visible, swap in
  the shader's existing integer-hash gradient noise (already ported for
  detail normals) keyed off the same anchoring — costlier per fragment,
  so only if the cheap version fails on screen.
