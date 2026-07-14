# Phase 3D-4 — Organisms: Implementation Plan

This is the lower-level plan for the fourth phase of
[`3d-design.md`](3d-design.md) (§6 there): realized near-field organisms
become visible in POV as lit box and sphere instances whose primitive,
proportions, size, color, and pose come directly from the existing organism
and genome presentation data.

Read [`AGENTS.md`](../../../AGENTS.md) first. This plan assumes the crate
boundary, determinism, and CI contracts there. It builds on the **landed**
3D-1 through 3D-3 implementation rather than the earlier design's
pre-implementation sketch: terrain vertices are 32 bytes, collision reads the
resident 65×65 render lattice, water draws terrain → river overlay → sea,
POV supports reduced-resolution rendering, and both F12 and `--pov-script`
render through `PovCapture`. The implementation must preserve all of those
surfaces.

Two rules govern the phase. First, **organisms are another derived,
upload-only presentation of `RegionMap::organisms()`**: the renderer receives
primitive meshes and packed instances; it never reads regions, genomes,
ecology tiles, or world state, and nothing rendered is read back into
simulation. Second, **direct-light visibility moves to a GPU directional
shadow map now**, while terrain ambient occlusion remains the static CPU-baked
term. This establishes the caster/receiver path future animated meshes need
and removes the expensive CPU horizon-shadow march instead of teaching a new
instance format about a temporary baked sun value. The only neutral-crate
change is an already-existing genome trait copied onto `Expressed` for
presentation. No generation formula, identity fold, persistence record,
dependency hash, world/layer algorithm version, or golden fixture changes.

This intentionally pulls directional shadows forward from the original
deferral in `3d-design.md` §8. The scope change is narrow: one fixed-sun,
camera-centered presentation shadow map, not a general light/material system.
It lands with the first independent mesh casters so terrain, organisms, and
future animated meshes do not acquire competing lighting paths.

---

## 1. Goals and non-goals

### 1.1 Goals (design §6.3, restated as deliverables)

- Draw every POV-eligible realized organism with **two instanced draw batches**:
  one unit box mesh and one two-subdivision icosphere. There is no draw call per
  organism and no fixed instance limit.
- Add one camera-centered **GPU directional shadow map** shared by terrain and
  organisms. Terrain and both primitive batches cast into it; both opaque
  shaders sample it for direct-sun visibility with comparison sampling and
  small PCF. The terrain-conformal river overlay also receives it for glint
  visibility. Animated meshes can later join the same caster/receiver contract.
- Remove `bake_sun_visibility` from CPU chunk meshing after the GPU path passes
  visual and native-Windows performance A/B. Keep the existing coarse terrain
  AO bake: it is low-frequency, cheap, and attenuates ambient rather than
  standing in for a dynamic shadow.
- Map genome to geometry with one pure function:
  `form` bit 0 selects box/sphere, bits 1–3 select one of eight explicit
  proportion pairs, `Expressed::size` supplies characteristic body size, and
  low `id` bits supply static yaw.
- Use exactly the 2D organism marker's expressed RGB as the body's base albedo.
  The helper is shared rather than duplicated, so byte equality is testable.
- Place each body's x/y at `Organism::world_pos` and its bottom on the same
  resident terrain triangles that `ground_surface`/`ground_height` and walk
  mode use. Terrain remeshing must update organism z and ground AO even when
  realization identity did not move.
- Keep instance positions stable and precise far from the origin. Camera motion
  updates only a small frame uniform; it must not force all instances to be
  rebuilt or uploaded every frame.
- Rebuild and upload instance lists only when their exact visual inputs change:
  realization publication/retirement, tier-density expansion, expressed
  traits, distance-cull membership, rendered ground height, or ground AO. At
  rest, both rebuild and GPU-upload traffic are zero.
- Preserve correct depth interaction with terrain and water. Opaque organisms
  draw after terrain and before translucent river/sea passes, so hills occlude
  them and the sea tints bodies below its surface. Terrain and organisms also
  cast and receive the same directional shadows.
- Include organisms in the live renderer, F12 POV dumps, and `--pov-script`
  snapshots through the same upload and draw path. Scripted and F12 captures
  remain time-frozen and reproducible.
- Record CPU mesh/assembly cost, shadow-pass cost, GPU bytes, instance counts,
  and native Windows frame cost in [`perf-baseline.md`](perf-baseline.md)
  before sign-off.

### 1.2 Non-goals (design §8 or later work)

- Authored meshes, skeletal animation, billboards, textures, materials beyond
  solid color, terrain-normal alignment, or per-species mesh assets. The phase
  builds the shadow interface animated meshes will use, not animation itself.
- Cascaded shadow maps, point/spot lights, a moving sun, contact shadows,
  screen-space ambient occlusion, or a general deferred-lighting system. One
  directional map covers the current fixed-radius/fogged POV.
- Physics or gameplay collision with organisms. Walk/fly movement continues to
  collide only with terrain; bodies may overlap each other and the camera.
- LOD meshes, impostors, occlusion queries, GPU-driven culling, indirect draws,
  or a spatial acceleration structure. The realized near window is deliberately
  small enough to begin with a linear CPU scan and two ordinary instanced draws.
- Changes to realization probability, species rosters, food webs, ecology,
  organism identity/placement, resource-tier density, or persistence.
- Browser POV integration. The shader remains WGSL and the renderer API remains
  platform-agnostic, but `platform-web` gains no 3D path in this phase.
- A mandatory activity animation. The optional bob in design §6.2 has a
  measured go/no-go milestone (§10.5); static organisms satisfy the phase.
- Changing the 2D marker palette. A producer cue may modulate the 3D shader,
  but the packed base RGB remains byte-identical to the map marker (§5.4).

## 2. Contracts this phase must not break

- **Identity and generated output.** `AppearanceGenes::form` is already part
  of `Genome::fingerprint`; copying it to `Expressed` creates no new value.
  `Organism::id`, species, slot, cell, placement, and every existing expressed
  float remain untouched. `WORLD_ALGORITHM_VERSION` and every layer revision
  stay at their current values, and no determinism fixture is re-blessed.
- **Persistence and replay.** `Expressed` is transient and is not serialized by
  the record codec. `tools::replay::state_hash` currently folds the five
  existing expressed floats explicitly; it also folds `Organism::species`,
  from which that species' genome and `form` are deterministically derived. It
  must not gain a redundant `form` fold, which would move the settled harness
  hash for a presentation-only field.
- **Native/wasm parity.** `platform_web::genome_sample()` calls
  `Genome::fingerprint()`, which already includes `AppearanceGenes::form`.
  Its expected value and all wasm parity fixtures remain unchanged. Adding a
  field to `Expressed` must compile on wasm without adding a platform
  dependency.
- **Crate direction.** `world-core` gets only the pure `form` passthrough.
  `world-runtime` needs no change: the native shell consumes its existing
  `organisms()`, `organism_count()`, and organism fields. The renderer receives
  plain geometry/instance uploads and never imports `world-core` or
  `world-runtime`.
- **No GPU authority.** The organism path adds no readback API. `PovCapture`
  remains the ADR 0021 diagnostics carve-out and only returns final image bytes
  to a file-producing caller.
- **Lighting ownership.** The GPU transforms organism normals and evaluates
  Lambert direct light, dynamic shadow visibility, material response, and fog
  from the current terrain/organism normals. The CPU keeps only low-frequency
  terrain AO and couriers one ground-AO value to each organism. No final lit
  color or direct-sun visibility is baked into an organism instance.
- **Terrain agreement.** Body placement uses
  a shared `PovChunkManager::ground_surface` query (height plus AO), not
  analytic elevation. An organism whose terrain chunk is not resident is
  temporarily omitted; it appears when the exact rendered lattice is available
  rather than floating over a frontier hole. Existing `ground_height` delegates
  to the same interpolation helper so walk and organism placement cannot drift.
- **Shadow presentation only.** The shadow texture is render-only GPU state:
  no readback, hash, persistence, or simulation input. Camera/light matrices
  are computed camera-relative so adding shadows does not reintroduce the
  far-origin precision problem solved in 3D-1.
- **Map mode.** Existing map composition, marker positions, and marker RGB are
  unchanged. No input binding changes are required. All existing terrain,
  walk, water, screenshot, and map-pixel tests stay green unmodified.
- **Capture policy.** Live animation may use wrapped display time, but
  `--pov-script` and F12 pass time 0. A repeated capture from the same settled
  map, pose, toggles, and scale must be byte-comparable.
- **CI.** The full native, wasm parity/web build, and Windows MSVC build matrix
  must pass under `RUSTFLAGS=-D warnings`. No warning-only or native-only
  workaround is acceptable.

## 3. New and touched surfaces

| Surface | Change |
|---------|--------|
| `crates/world-core/src/genome.rs` | Add `Expressed::form: u8`; copy `self.appearance.form` in `Genome::express`; unit-test that all biases preserve it and existing expressed values are unchanged. |
| `crates/platform-native/src/viz.rs` | Make the existing HSV/expressed-color helper `pub(crate)` (or expose only `expressed_color`) so map markers and POV instances use one conversion implementation. No formula or map call-site change. |
| `crates/platform-native/src/pov.rs` | Add the pure genome→geometry mapping, `PovOrganismManager`, exact visual keys, distance/ground filtering, stable ordering, counters, and unit tests. Retain the core AO lattice with each resident chunk; replace the standalone height query with a shared height+AO surface sampler; compute the stabilized camera-relative light matrix. Remove the CPU horizon-shadow march after GPU sign-off. |
| `crates/renderer/shaders/pov_terrain.wgsl` | Add a depth-only terrain shadow entry point; sample the directional shadow map in the main fragment path instead of consuming CPU-baked sun visibility. Keep baked AO, detail normals, wet response, and fog. |
| `crates/renderer/shaders/pov_organism.wgsl` (new) | Shared rigid-instance transform used by color and depth-only shadow entry points; inverse-scale normal transform; shadow sampling; Lambert + hemisphere lighting; ground AO; optional producer modulation/bob; terrain-matched fog. |
| `crates/renderer/src/pov.rs` | Add the shadow depth target, comparison sampler/bind group, terrain and organism shadow pipelines, light-frame uniform, primitive/instance upload types, cube and icosphere geometry, two color draw batches, grow-only box/sphere instance buffers, shadow → terrain → organisms → overlay → sea ordering, and device-free geometry/buffer/matrix tests. |
| `crates/renderer/src/lib.rs` | Re-export the organism shader/upload surface and extend `PovFrameParams`/`Renderer::render_pov` with shadow matrix, resolution/enable state, and an optional replacement instance upload (`None` means retain, `Some(empty)` means clear). |
| `crates/renderer/shaders/pov_water.wgsl` | Keep the shared frame-uniform layout identical; make the terrain-conformal river overlay sample dynamic shadow visibility for its sun glint after `light.x` is retired. The sea remains neither caster nor receiver. |
| `crates/renderer/tests/wgsl.rs` | Parse and validate updated terrain/water plus new organism WGSL and their color/shadow entry points with naga. |
| `crates/platform-native/src/main.rs` | Own a `PovOrganismManager`; sync it after terrain chunk integration; pass changed lists and shadow parameters into `render_pov`; include actual instance bytes/counts in telemetry; preserve `B` as the shadows/AO A/B toggle; settle and upload organisms in `--pov-script`. |
| `crates/platform-native/src/dump.rs` | Build the organism list and shadow parameters against the dump's freshly meshed terrain ring, apply them to `PovCapture`, and record published/drawn/waiting counts. |
| `README.md` | Extend the existing POV row with organism rendering and describe `B` as the shadows/AO diagnostic toggle; no new key or environment variable. |
| `docs/plans/prototype/perf-baseline.md` | Add CPU shadow-bake removal, shadow-map resolution/pass cost, 3D-4 instance counts/bytes/rebuild behavior, primitive triangle cost, and same-machine native Windows A/B. |

No `Cargo.toml`, `world-runtime`, record codec, ADR, input map, or environment
variable change is expected.

## 4. The `form` presentation passthrough (design §6.2)

### 4.1 Data change

Extend the transient presentation struct and its sole constructor:

```rust
pub struct Expressed {
    pub hue: f32,
    pub luminance: f32,
    pub size: f32,
    pub activity: f32,
    pub aggression: f32,
    /// Morphology archetype copied from `AppearanceGenes::form` (`0..=15`).
    /// Presentation only; never identity or persistence.
    pub form: u8,
}

// In Genome::express:
Expressed {
    hue,
    luminance,
    size,
    activity,
    aggression,
    form: self.appearance.form,
}
```

`form` is not biased. Morphology bias continues to modulate body size only;
the species archetype does not jump between box/sphere or proportion classes
as possibility state drifts.

### 4.2 Version and parity proof

Before landing M1, keep an explicit audit in the review:

1. `Genome::fingerprint()` already folds `appearance.form`; do not change its
   fold order or expected value.
2. `platform_web::genome_sample()` fingerprints the genome, not `Expressed`;
   its native/wasm golden remains unchanged.
3. `tools::replay::state_hash` folds the existing expressed fields one by one.
   Leave that fold unchanged because `form` is presentation-only and already
   implied by the folded organism species id.
4. No serde/postcard record contains `Expressed`; therefore
   `RECORD_FORMAT_VERSION` does not move.
5. Existing struct comparisons now include `form` through derived
   `PartialEq`, which is desirable: expression equality should include every
   value the renderer can observe.

If the code changes before implementation and a generic serialization or hash
of all `Expressed` bytes appears, stop and re-audit. The design's fallback is
to put `form` directly on `Organism`; do not silently move a golden hash for
this phase.

## 5. Pure organism-to-geometry mapping

### 5.1 Mapping boundary

Keep all world knowledge in `platform-native/src/pov.rs`:

```rust
fn organism_visual(
    organism: &world_runtime::Organism,
    ground: GroundSurface,
) -> PovOrganismVisual;
```

`GroundSurface` is the resident terrain triangle's interpolated height and
ambient-occlusion factor. `PovOrganismVisual` contains only renderer-ready
facts: primitive kind, world-space body center, non-uniform scale, yaw, base
RGBA/flags, ground AO, and (only if M5 ships) bob parameters. It has no
reference to the map, chunks, species roster, or GPU. This function is pure
and table-driven so all 16 `form` values can be exhaustively tested.

### 5.2 Primitive and proportions

The mapping is exact:

```text
primitive = form & 1
shape     = (form >> 1) & 7
```

- `primitive == 0`: box.
- `primitive == 1`: sphere.
- `shape` indexes the following `(xy multiplier, z multiplier)` table.
  Values are fixed literals, approximately volume-preserving, and span a flat
  slab through a tall pillar without a runtime `powf`:

| shape | xy | z | height / width |
|------:|---:|--:|---------------:|
| 0 | 1.42 | 0.50 | 0.35 |
| 1 | 1.26 | 0.63 | 0.50 |
| 2 | 1.13 | 0.79 | 0.70 |
| 3 | 1.00 | 1.00 | 1.00 |
| 4 | 0.89 | 1.25 | 1.40 |
| 5 | 0.82 | 1.48 | 1.80 |
| 6 | 0.76 | 1.74 | 2.29 |
| 7 | 0.69 | 2.08 | 3.01 |

For characteristic size `s = organism.expressed.size`, instance scale is
`[s * xy, s * xy, s * z]`. Both canonical primitive meshes fit a unit body
centered at the origin (box spans `[-0.5, 0.5]`; icosphere radius is `0.5`),
so `s` has the same meaning for both primitives. Do not impose a visual
minimum: the 0.1-unit end of the size ladder is intentionally tiny, while the
12.8-unit end should be unmistakably large.

### 5.3 Position and facing

- x/y are copied exactly from `Organism::world_pos` as `f64`.
- Body-center z is
  `f64::from(ground.height) + 0.5 * f64::from(scale[2])`; therefore the
  canonical mesh's lowest point is exactly the rendered ground before optional
  bobbing. `ground.ambient_occlusion` is copied separately and never changes
  geometry.
- Static yaw is
  `TAU * ((organism.id & 0xffff) as f32 / 65536.0)`. It is a pose variation,
  not identity and not draw-time RNG. The renderer packs sine/cosine once when
  the list changes.
- x/y scale is equal, so yaw never changes an archetype's dimensions. Members
  of one species retain the same primitive/proportions; only pose and already-
  existing expression bias vary per instance/region.

### 5.4 Color and trophic cue

Hoist, without changing, the existing map formula:

```rust
expressed_color(expressed) =
    hsv_to_rgb(expressed.hue, 0.75, 0.45 + 0.55 * expressed.luminance)
```

Those three bytes are the instance's base RGB and are asserted equal to the
2D marker helper. This is the checkable meaning of the design's “same color”
criterion; final screen pixels are naturally changed by 3D lighting and fog.

The design also proposes desaturating producers toward green-gray. Preserve
both requirements by putting an `is_producer` flag in the instance alpha byte
and applying at most an 8% green-gray mix **in the organism fragment shader**.
The underlying RGB stays exact, consumers use the unmodified albedo, and map
pixels do not change. If the tint obscures genome color in M5 review, set its
strength to zero; color parity is the stronger phase-exit contract.

## 6. Renderer implementation

### 6.1 Lighting split

Use a forward-lit, shadow-mapped pipeline with a deliberately narrow split:

```text
direct sun          = GPU Lambert(normal, sun) × GPU shadow visibility
ambient             = GPU hemisphere(normal) × CPU-baked terrain AO
terrain/river glint = existing GPU term × GPU shadow visibility
sea response        = existing unshadowed Fresnel/glint
fog                 = existing GPU distance term
```

For terrain, the normal is the existing CPU mesh normal plus optional GPU
detail perturbation. For organisms, it is the rigidly transformed primitive
normal; a future skinned mesh supplies its skinned normal at the same point in
the pipeline. No final light value is stored in either vertex/instance format.

The one retained CPU term is terrain AO. It is a static, low-frequency
concavity estimate and costs little compared with the current horizon march.
Terrain consumes it per vertex exactly as today; an organism consumes the
terrain AO interpolated under its body. This is environmental ambient
attenuation, not a substitute for an organism's future self-occlusion.

### 6.2 Directional shadow target and pass

`Pov` owns one depth texture with
`RENDER_ATTACHMENT | TEXTURE_BINDING`, a depth view, and a comparison sampler.
Start with `Depth32Float`, `LessEqual`, clamp-to-edge, and manual 3×3 PCF in
WGSL. The shadow resolution is selected by the native shell from resource tier:

| Tier | Initial map size |
|------|------------------|
| Low | 1024×1024 |
| Mid | 2048×2048 |
| High | 2048×2048 |

These are tuning starts, not identity or persisted configuration. Native
Windows M5 measurements decide whether Mid/High should differ. The shadow map
is independent of `WER_POV_SCALE`: reducing color-buffer resolution must not
make shadow texels swim or force target recreation.

When the existing `B` diagnostic is on, each POV frame records a depth-only
shadow pass before the color pass:

1. Terrain core triangles for every resident draw handle. Do not draw skirts
   into the shadow map; their defensive vertical walls would cast artificial
   seam shadows. Add a `CORE_INDICES` count and draw only that prefix of the
   shared terrain index buffer.
2. Every box instance.
3. Every sphere instance.

River overlays and the sea plane do not cast. The sparse terrain-conformal
overlay does receive the map in its fragment path so shaded river glint stays
consistent after the old `light.x` visibility byte is retired; the broad sea
plane remains unshadowed. Static primitives still render the map every frame:
this is intentionally the dynamic path future animation will use, and it keeps
correctness independent of a shadow-cache invalidation scheme. A cache can be
measured later if static-scene cost warrants it.

`pov_terrain.wgsl` gains `vs_shadow`, sharing its chunk-relative position
helper with `vs_main`. `pov_organism.wgsl` gains both color and shadow vertex
entry points that call the same `organism_position` function, including
optional bob. When skinning arrives, there is exactly one deformation helper
to extend; color and shadow silhouettes cannot diverge.

### 6.3 Light-space fit, precision, and stability

The shell computes the directional light matrix because it owns camera/world
scale and the CPU-side resident bounds. The renderer receives only a plain
camera-relative matrix and shadow parameters.

Extend each `ChunkEntry` with the core height minimum/maximum retained at mesh
integration. `PovChunkManager::shadow_bounds` unions resident chunk x/y bounds,
their height bounds (including `POV_SKIRT_DROP` only for conservative depth,
not as caster geometry), and the current organism top heights. Transform the
eight corners of that camera-relative AABB into a light basis built from
`SUN_DIR`, pad every axis, and fit an orthographic projection.

The matrix operates on the same camera-relative positions as the color pass:
terrain chunk origins are subtracted from `camera_pos` in f64 before upload,
and organisms use their high/low split. No absolute large `f32` coordinate
enters shadow projection.

Stabilize the map to prevent camera shimmer:

- round light-space x/y extent up to a whole `REGION_SIZE` increment so chunks
  integrating one at a time do not continuously rescale texels;
- snap the light-space center to one shadow texel after the padded extent is
  known;
- keep near/far depth padding fixed and conservative;
- treat a projected point outside the valid shadow UV/depth volume as fully
  lit, with a one-texel border fade rather than sampling a clamped edge shadow.

Pure matrix tests cover every resident corner, center snapping, unchanged
matrix under sub-texel camera motion, radius/tier changes, empty bounds, and
large positive/negative world positions.

### 6.4 Shadow sampling and bias

The terrain, organism, and river-overlay fragment shaders receive the
light-space position from their vertex stages. After perspective divide
(orthographic today, but do not assume `w == 1` in the helper), they sample
nine neighboring texels with `textureSampleCompareLevel` and average
visibility. Terrain uses the result for diffuse sun and wet glint, organisms
for diffuse sun, and the river overlay for glint. Hemisphere ambient remains
AO-controlled, not shadow-controlled.

Bias combines a small constant depth offset and a normal/sun slope term. Keep
both as renderer-owned presentation constants and tune them on native Windows
against flat ground, steep slopes, box feet, sphere contact, and far-origin
captures. The acceptance rule is behavioral rather than a magic initial
number: no widespread acne at grazing sun angles, no visibly detached
“Peter Pan” shadows at organism feet, and no light leaks large enough to erase
box/sphere contact.

`B` off skips the shadow pass entirely and makes both shaders use
`shadow_visibility = 1` and `ambient_occlusion = 1`. Rename internal
`baked_light` fields/log text to `shadow_ao` so the control remains truthful;
the key binding does not change. This is both the visual diagnostic and the
native-Windows performance A/B.

### 6.5 Retire the CPU sun-visibility bake

Once M2's GPU terrain-shadow path passes its visual/capture/performance gate:

- remove `bake_sun_visibility`, its march constants/scratch allocations, and
  its unit tests from `platform-native/src/pov.rs`;
- keep `valley_occlusion`/`vertex_ao` unchanged;
- keep the 32-byte `PovVertex` layout, setting `light[0] = 255` as a reserved
  neutral byte, `light[1] = quantize_light(vertex_ao(...))`, and retaining
  river/wetness in `light[2..=3]`;
- update `PovVertex`/WGSL comments and tests so no code implies `light.x` still
  contains a shadow term;
- leave chunk provenance unchanged. Shadows depend on current GPU geometry and
  frame parameters, so camera motion never remeshes a chunk.

The migration is one-way in the landed phase: use the old CPU bake for A/B
during M2 development, then delete it. Do not permanently maintain two shadow
authorities or add CPU sun visibility to organism instances.

### 6.6 Public organism upload surface

Add world-agnostic types beside `TerrainChunkUpload`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovOrganismInstance {
    pub position: [f64; 3],
    pub scale: [f32; 3],
    pub yaw: f32,
    pub color: [u8; 4], // expressed RGB + producer flag
    pub ambient_occlusion: u8,
    pub bob: [f32; 2],  // amplitude, phase; zeroed unless M5 ships
}

#[derive(Debug, Default)]
pub struct PovOrganismUpload {
    pub boxes: Vec<PovOrganismInstance>,
    pub spheres: Vec<PovOrganismInstance>,
}
```

`Renderer::render_pov` and `PovCapture::apply` take
`Option<&PovOrganismUpload>`:

- `None`: retain the resident instance buffers/counts; steady-state path.
- `Some(non-empty)`: replace both lists and upload them.
- `Some(empty)`: set both counts to zero, clearing retired organisms even
  though no bytes are written.

This tri-state must be explicit. Treating an empty slice as “no update” would
leave stale organisms on screen after moving into a barren region.

### 6.7 Stable far-origin instance format

Do not upload absolute world x/y as one `f32`; that repeats the precision bug
3D-1 avoided for terrain. On replacement upload, the renderer splits every
`f64` component into two floats:

```rust
hi = value as f32;
lo = (value - f64::from(hi)) as f32;
```

The private, 64-byte `#[repr(C)] + Pod` GPU instance stores:

```text
position_hi.xyz + sin(yaw)       16 B
position_lo.xyz + cos(yaw)       16 B
scale.xyz       + bob amplitude  16 B
base RGBA + AO/flags + bob phase + padding    16 B
```

An organism-only frame uniform similarly stores split `camera_pos` high/low,
plus the existing view-projection, light view-projection, sun, shadow-map,
fog, and hemisphere values. The shader reconstructs the small camera-relative
center as
`(position_hi - camera_hi) + (position_lo - camera_lo)` before adding the
scaled primitive vertex. This keeps instance buffers stable while the camera
moves and bounds precision at large world coordinates.

Use a separate `OrganismParamsRaw` for the high/low camera fields, but keep
light matrix/shadow parameters layout-compatible wherever terrain and water
share `PovParamsRaw`. Update all Rust/WGSL mirrors together and add size/offset
assertions; the water shader declares but does not consume the shadow fields.
`Pov::write_frame` writes every frame uniform from one `PovFrameParams`.

### 6.8 Canonical primitive meshes

Build both meshes once in `Pov::new`:

- **Box:** 24 vertices (four per face, face normals) and 36 `u16` indices.
  Shared corner vertices are deliberately duplicated so Lambert lighting is
  flat across each face.
- **Sphere:** indexed icosphere, two midpoint-normalization subdivisions:
  162 vertices, 320 triangles, 960 `u16` indices. Vertex normals are the
  normalized position before the radius-0.5 scale, producing smooth lighting.

The icosphere builder is a pure renderer helper. Deduplicate edge midpoints by
the ordered endpoint pair so counts are fixed; assert every index is in range,
all positions have radius 0.5 within tolerance, normals are unit length, and
triangle winding faces outward. Do not commit a 960-index hand-maintained
literal.

Both meshes use one 24-byte primitive vertex layout (`position: Float32x3`,
`normal: Float32x3`). The instance buffer is vertex slot 1 with
`step_mode = Instance`. Create named box and sphere pipelines through one
pipeline-construction helper; they share layout/shader/state but remain two
explicit draw batches as the design specifies.

### 6.9 Organism color shader and normal transform

`pov_organism.wgsl` performs:

1. Scale the canonical vertex by instance scale.
2. Rotate x/y by the precomputed sine/cosine yaw.
3. Add the reconstructed camera-relative center.
4. Transform normals by inverse non-uniform scale, then yaw, then normalize.
   Multiplying normals directly by scale is incorrect for slab/pillar forms.
5. Decode base sRGB with the same `pow(2.2)` approximation as terrain and apply
   the optional producer modulation.
6. Multiply Lambert direct sun by the shared GPU shadow visibility; multiply
   hemisphere ambient by the instance's normalized ground-AO byte; apply the
   same distance fog as terrain.

Ground AO is deliberately constant over one body. Dynamic normals and shadow
visibility still vary per vertex/fragment, so rigid animation and future
skinning light correctly. A later mesh/material phase can add self-AO without
changing the shadow contract.

### 6.10 Color depth and draw order

Organism color pipelines are opaque, back-face culled, `Depth32Float`, compare
`Less`, depth-write on, and no blending. The frame becomes:

```text
shadow depth pass: terrain core + boxes + spheres
color pass:
terrain (opaque, writes depth)
box organisms (opaque, writes depth, one instanced draw if count > 0)
sphere organisms (opaque, writes depth, one instanced draw if count > 0)
river overlays (blended, tests depth, no depth write)
sea (blended, tests depth, no depth write)
```

This ordering matters. Drawing organisms after the sea would paint submerged
bodies opaquely over the water. Drawing them before water lets the plane blend
over body fragments below `z = 0` while above-water fragments occlude it.

### 6.11 Instance-buffer lifetime

Each primitive owns one GPU instance buffer, a capacity, and a live count.
On `Some(upload)`:

- Grow capacity to `next_power_of_two(required.max(1))` when required exceeds
  capacity; otherwise reuse the buffer.
- Pack and write only the live prefix.
- Set count to the exact replacement length, including zero.
- Never shrink during a session. Resource-tier expansion and travel reuse the
  high-water allocation instead of churning buffers.

There is deliberately **no 1,600-instance allocation cap**.
`Budget::max_realize_organisms = 1_600` is a per-update work budget, and one
whole-region overshoot is allowed; it is not a bound on the resident
population. Buffers must grow from actual list length. If measured populations
are materially larger than expected, address that with measured LOD/culling
later rather than silently dropping valid organisms.

## 7. Native lifecycle and grounding

### 7.1 One rendered-surface query for height and AO

Extend `ChunkMesh`/`ChunkEntry` with the core 65×65 AO lattice already
calculated while packing `PovVertex.light[1]`. Store quantized `u8`, not a
second `f32` copy. Integration atomically swaps heights, AO, min/max height,
and the corresponding GPU vertex upload under the same chunk key.

Replace duplicated lookup math with:

```rust
pub struct GroundSurface {
    pub height: f32,
    pub ambient_occlusion: u8,
}

pub fn ground_surface(&self, wx: f64, wy: f64) -> Option<GroundSurface>;
pub fn ground_height(&self, wx: f64, wy: f64) -> Option<f32> {
    self.ground_surface(wx, wy).map(|surface| surface.height)
}
```

Use the exact v00→v11 triangle split and barycentric weights already pinned by
the walk tests. Height interpolates as today. AO converts its four vertex bytes
to `[0, 1]`, interpolates over the selected triangle, then requantizes once for
the instance. This matches the attribute field the terrain shader sees at that
surface point closely enough for a body/ground lighting transition, while
keeping the persistent CPU copy compact.

### 7.2 `PovOrganismManager`

Add a shell-side manager alongside `PovChunkManager`. It owns reusable scratch
vectors, the last exact visual keys, the current renderer upload lists, and
telemetry counters. It does no asynchronous work: mapping at the expected near-
window size is an O(n) presentation scan and is cheaper and simpler than adding
another executor lifecycle.

Per POV frame, after `PovChunkManager::sync` has integrated this frame's terrain
uploads:

1. Iterate `map.organisms()`; do not use `authoritative_organisms()`, because
   higher density slots are intentionally visible presentation.
2. Distance-cull against `fog_end + body_bounding_radius`. Do not frustum-cull
   on CPU: camera rotation must remain a uniform-only change, and the GPU
   already clips primitives outside the view.
3. Query `chunks.ground_surface(world_x, world_y)`. Omit the organism if it is
   `None`; count it as `waiting_for_ground` for telemetry.
4. Run `organism_visual`; pair the result with `organism.id` and `slot`.
5. Partition box/sphere and sort each list by `(id, slot)`. Runtime traversal
   is already stable, but an explicit total order makes instance bytes and
   equal-depth behavior independent of container implementation.
6. Build exact comparison keys from integer fields and `to_bits()` for every
   float, including ground-derived center z and the AO byte. Compare vectors,
   not a hash, so change detection has no collision argument.
7. If both key vectors match the previous frame, retain the GPU lists and
   return `None`. Otherwise swap/reuse scratch storage, update counters, and
   return `Some(&current_upload)`.

Including center-z and AO is load-bearing: a Planetary/Geology terrain remesh
can move or re-occlude the ground without changing the organism's identity or
M/B/A expression. The next frame rebuilds body placement/ambient attenuation
from the newly integrated render lattice even in that case. Direct shadow
visibility is absent from these keys because it is evaluated from current GPU
geometry every frame.

### 7.3 Distance and count contract

Use the same fog reach calculation as `frame_params`; extract a small shared
helper rather than duplicating `0.95 * (radius + 0.5) * REGION_SIZE`. Culling
uses horizontal distance because fog uses full 3D distance in the shader; the
body-radius margin prevents a large organism from disappearing while part of
it is still visible.

The exact count assertion is therefore:

```text
drawn ids = all ids from RegionMap::organisms()
            whose body intersects the POV fog radius
            and whose rendered terrain chunk is resident
```

There is no density-slot, species, trophic, or arbitrary count filter. In a
fully settled default radius-3 ring, every realized near-window organism has a
covering terrain chunk; bodies outside fog are already visually dissolved.

### 7.4 Live shell integration

`App` gains `pov_organisms: PovOrganismManager`. In `frame_pov`:

- Integrate/schedule chunks first.
- Sync organisms against the post-integration chunk set and current camera.
- Union chunk/organism bounds, build the stabilized camera-relative light
  matrix, and select 1024/2048 shadow resolution from the current tier.
- Add instance assembly time to the existing presentation `Mesh` pass timing.
- Add raw instance bytes only when an upload exists to the upload telemetry.
- Pass the optional replacement upload and shadow frame parameters to
  `Renderer::render_pov`; `Pov` records the shadow pass before its color pass.
- Extend the once-per-second POV line with resident/drawn/waiting counts and
  rebuild/upload deltas. Counters are observational only and never gate work.

Map mode does not need to run the manager. On first return to POV, its exact
scan catches every realization/terrain change that happened while the manager
was dormant.

## 8. Capture integration

### 8.1 `--pov-script`

The current snap path settles a fixed eight world updates before meshing. That
is enough for terrain under the inline executor but not a proof that the fixed
one-region-per-update authoritative organism pass has published the whole near
window. For an organism capture:

1. Run zero-travel updates, bounded at 128, until
   `map.authoritative_realization_complete(camera_xy)` is true. Error with a
   useful message if the bound is exhausted; do not take a partial “settled”
   snapshot silently.
2. Drain the terrain chunk manager exactly as today.
3. Sync a `PovOrganismManager` against those resident chunks.
4. Build the same stabilized shadow bounds/matrix and tier-selected resolution
   as the live path.
5. Apply terrain and organism replacements to `PovCapture`.
6. Render shadow and color passes with time 0 and default toggles.

The script uses `StreamConfig::default()` (one organism slot per cell), so
canonical completion is also complete visual density there. If the capture
harness later honors `WER_TIER`, add a separate visual-expansion completion
observation rather than assuming `max_realize_organisms` is a population cap.

### 8.2 F12 dump

`dump_pov_screenshot` already remeshes the live map into a fresh local chunk
manager because live GPU buffers cannot be read back. After that loop, build a
fresh local organism manager against the same chunks and apply its first
replacement to the capture. Build shadow parameters from those same local
chunk/organism bounds; `PovCapture` owns and renders its own depth map before
the color target. Do not mutate or settle the live world in F12; the dump must
show the exact currently published organism set, including a legitimate
partially expanded tier state.

Keep time 0 and the live diagnostic toggles. Extend the screenshot description
in `state.txt` with `N organisms drawn / M published / K waiting for ground` so
a frontier omission is diagnosable from the dump.

## 9. Performance posture

### 9.1 Performance reference

The primary 3D-4 performance reference is the **native Windows release build
on a hardware GPU**, not WSL2/llvmpipe. Every committed measurement records:

- CPU, GPU, adapter name, driver version, Windows version, and Rust commit;
- `--release`, fixed `WER_WINDOW`, `WER_POV_RADIUS=3`,
  `WER_POV_SCALE=1`, and `WER_PRESENT_MODE=immediate` so v-sync does not hide
  frame-time differences;
- resource tier and resulting shadow-map resolution;
- at least one terrain-only vantage and one organism-dense High-tier vantage;
- warmed median and p95 over the same camera pose/duration, with `B` shadow/AO
  off/on A/B in the same executable.

The GitHub `cargo xwin` job proves Windows compilation only; it is not a
performance runner. WSL2/llvmpipe remains a functional smoke environment for
pipeline creation and gross rendering failures, but it does not choose shadow
quality or gate this phase.

### 9.2 CPU budget

- **Chunk meshing:** the current native mesher performs 65 rows ×
  `SHADOW_STEPS` analytic height samples per chunk for sun visibility. Removing
  that horizon march should materially reduce worker mesh time and scratch
  allocation. Record radius-3 cold-ring total and ms/chunk before/after on the
  same Windows machine; if the reduction is not obvious, profile before
  deleting the M2 comparison branch.
- **Organism assembly:** one linear scan of the published near-field set per
  POV frame, one O(1) height+AO interpolation per eligible organism, small
  table lookups, and exact-key comparison. Reuse all vectors. Measure before
  considering a public realization epoch or per-region delta API in
  `world-runtime`.
- **World generation protection:** POV meshing remains on the Background lane,
  while shadow rendering consumes GPU command work. Native measurements report
  world-update CPU separately from whole-frame render time; adding organisms
  or shadows must not move deterministic budgets or generation scheduling.
- **No new worker job:** instance mapping stays on the frame thread initially.
  If its measured cost matters, move to per-region replacement before adding
  another asynchronous lifecycle; never truncate valid organisms to hit a
  timing target.

### 9.3 GPU budget

- **Shadow pass:** every enabled frame redraws resident terrain core geometry
  plus boxes/spheres into one 1024² or 2048² depth target. This exchanges a
  burst of CPU horizon work on remesh for continuous GPU work that naturally
  handles moving geometry.
- **Color pass:** two additional instanced draws; boxes cost 12 triangles each,
  two-subdivision spheres 320. Terrain and organism fragments, plus sparse
  river-overlay fragments, add nine depth comparisons for 3×3 PCF.
- **Upload:** 64 bytes per live instance only on exact visual change. At 1,600
  instances a full replacement is 100 KiB; steady state is 0 B/frame. Record
  actual population/high-water bytes because 1,600 is not a hard cap.
- **Memory:** a `Depth32Float` map is approximately 4 MiB at 1024² or 16 MiB at
  2048², plus two grow-only instance buffers and tiny static primitive meshes.
  Report allocated capacity and live bytes separately.
- **Resolution gate:** compare 1024 and 2048 on the native Windows reference.
  Keep 2048 for Mid/High only when its contact/terrain-shadow improvement is
  visible and its warmed p95 cost is proportionate. If 2048 adds more than 15%
  over the same scene at 1024 without a clear visual win, use 1024 for that
  tier. Do not solve a shadow-map bottleneck by lowering organism count.
- **No premature cache:** the phase renders shadows every frame so future
  animation is the baseline contract. Add shadow-map caching only after a
  profile shows the static case matters and the invalidation rules include
  terrain uploads, instance replacements, animation, light, radius, and
  snapped projection changes.

Before sign-off, add a 3D-4 table to `perf-baseline.md` containing the recorded
Windows environment, old/new mesh ms/chunk, shadow resolution and memory,
terrain-only and dense-vantage `B` off/on median/p95, world-update CPU,
published/drawn box/sphere counts, instance live/capacity bytes, first upload,
steady-state upload, and CPU instance-scan time.

## 10. Testing

### 10.1 Neutral data tests

1. `Genome::express` copies `appearance.form` for neutral, minimum, and maximum
   M/B/A biases; every pre-existing field equals the value produced by the old
   formula.
2. All generated forms remain in `0..=15`; changing bias never changes form.
3. Existing genome fingerprint and wasm parity expected constants pass
   unchanged. Existing replay/scale/vault hashes pass unchanged; no fixture
   file is edited.

### 10.2 Pure mapping and manager tests

4. Exhaust all 16 form values: primitive parity and shape index are exact;
   each adjacent shape has a strictly larger height/width ratio; box and sphere
   use the same scale for the same form/size.
5. Size scales linearly. A body at known `GroundSurface` has
   `center_z - scale_z / 2 == ground.height` within one f32 ULP and carries the
   exact AO byte; values are finite over the full current size ladder.
6. Yaw is a pure function of `id`, stable across calls, in `[0, TAU)`, and does
   not alter scale/color.
7. POV base RGB equals `viz::expressed_color` byte-for-byte across hue wrap,
   luminance endpoints, and representative genomes. Producer modulation is a
   separate flag and never changes those bytes.
8. On a settled map with resident synthetic chunks, manager output ids equal
   the independently filtered `map.organisms()` ids; box+sphere count equals
   expected count; all additive slots survive; no fixed maximum truncates a
   synthetic list larger than 1,600.
9. Manager delta behavior: first sync returns a replacement; identical sync
   returns `None`; camera movement that crosses no cull boundary still returns
   `None`; an expression, tier-slot, cull membership, ground-height bit, or AO
   byte change returns a replacement; dynamic shadow changes do **not** rebuild
   instances; transition to no organisms returns `Some(empty)` exactly once.
10. Missing terrain omits an organism and increments `waiting_for_ground`;
    adding the chunk emits a replacement with the body bottom on that chunk's
    drawn triangles and matching interpolated AO. Cross-region and
    cell-diagonal points reuse the existing `ground_height` topology tests.

### 10.3 Terrain-lighting and shadow-matrix tests

11. `ground_surface` height agrees exactly with the old `ground_height` at
    vertices, both cell triangles, diagonal, cell edge, and region border;
    `ground_height` delegates and returns the same bits.
12. AO interpolation uses the same selected triangle/weights, maps all-0 and
    all-255 lattices exactly, and stays within its three vertex endpoint range.
13. Meshed core vertices keep the pre-change AO byte in `light[1]`, reserve
    neutral `light[0] == 255`, preserve river/wetness bytes and skirts, and no
    longer call or allocate the horizon-shadow bake.
14. Camera-relative shadow fitting contains every padded resident/organism
    bound; empty bounds disable shadows safely; all matrix elements remain
    finite at large positive/negative world coordinates.
15. Light-space center snapping produces an identical matrix under sub-texel
    camera movement and moves by integral texels after crossing a boundary;
    extent rounding changes only at a `REGION_SIZE` step.
16. Projected inside points map to valid UV/depth; outside/border points select
    the fully-lit path. Low/Mid/High select the documented resolution.

### 10.4 Renderer tests

17. Updated terrain/water and new organism WGSL parse and validate under naga;
    terrain and organism modules expose their expected color/shadow entry
    points, and river overlay consumes the shared shadow binding while sea does
    not.
18. Cube geometry has 24 vertices/36 valid indices, face-unit normals, outward
    winding, and bounds `[-0.5, 0.5]`.
19. Two-subdivision icosphere has 162 vertices/960 valid indices, radius 0.5,
    unit normals, no duplicate midpoint per edge, and outward winding.
20. The f64 high/low split reconstructs camera-relative positions within a
    documented tolerance near the origin and at large positive/negative world
    coordinates; camera-only movement does not change packed instance bytes.
21. Compile-time/runtime layout assertions pin the private instance raw size to
    64 bytes and check every vertex attribute and scene/shadow uniform offset
    against its Rust/WGSL mirror.
22. Device-free buffer/target state tests cover first allocation,
    power-of-two instance growth, reuse below high-water, independent
    box/sphere counts, explicit empty replacement, shadow target reuse at one
    resolution, and recreation on a tier/resolution change.
23. Terrain's shadow draw count uses exactly the core index prefix and excludes
    skirts; `B` off omits the shadow pass and sets both shadow/AO factors to
    neutral. No unit test requires a GPU adapter in CI.

### 10.5 Optional activity bob gate

Only after static organisms and GPU shadows meet the visual/performance gates,
assess the design's optional bob. If it materially improves readability
without a material native-Windows regression, enable it for non-producers:

```text
amplitude = min(0.08 * body_height, 0.25) * activity
phase     = (id low 16 bits) / 65536
period    = 4 seconds (exactly divides WOBBLE_PERIOD = 32 seconds)
offset_z  = amplitude * (0.5 + 0.5 * sin(TAU * (time / period + phase)))
```

Producers remain grounded (`amplitude = 0`). Captures at time 0 are still
deterministic, clock wrap is seamless, and the shared transform makes the
shadow move with the body. If the gate fails, keep `bob = 0` and record “not
built” in the milestone; static rendering remains complete.

### 10.6 CI and manual verification

Run exactly the repository gates:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
RUSTFLAGS="-D warnings" cargo check --workspace
RUSTFLAGS="-D warnings" cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
cargo run --bin web-build
node crates/platform-web/web/smoke.mjs target/web-dist
```

The GitHub Windows `cargo xwin build --release --bin wer` job must also pass.
Run performance/manual sign-off with the native Windows release executable on
the recorded hardware adapter. WSL2/llvmpipe receives a secondary functional
launch/capture check only.

Manual walkthrough on native Windows, at Low and High tier:

- Toggle map/POV over an organism-bearing region and compare marker/body x/y,
  expressed base RGB, and count (accounting for the documented fog/ground
  filter).
- Inspect all 16 forms with a deterministic test fixture or chosen roster;
  verify box/sphere and flat→tall silhouettes remain readable at eye level.
- Walk around and through bodies; terrain following remains unchanged and
  hills occlude organisms correctly. Verify boxes/spheres cast onto terrain,
  terrain casts onto organisms, and organisms cast onto one another.
- Toggle `B`: the shadow pass and AO disappear together without remeshing or
  instance upload. Compare 1024/2048 captures during tuning; inspect contact
  shadows for acne, detachment, and light leaks.
- Translate/rotate slowly and check shadow edges for shimmer. Cross a chunk
  integration boundary and the edge of the shadow volume; no clamped-border
  stripe or stale CPU shadow should appear.
- Fly above and below sea level; above-water bodies occlude the sea, submerged
  bodies are seen through it, and river overlays do not paint over bodies.
- Stop moving after a settled frame and confirm organism rebuild/upload
  counters stay flat while the GPU shadow pass, camera look, and water
  animation continue.
- Trigger a possibility change that remeshes terrain without changing M/B/A;
  bodies move with the new rendered ground and do not retain stale z.
- Capture the same `--pov-script` pose twice and byte-diff the PPMs; inspect an
  F12 POV dump and its published/drawn/waiting counts.
- Repeat at a far positive and negative coordinate to expose color/shadow
  jitter.
- Run the fixed-window immediate-present measurement matrix from §9.1 and
  commit the environment and results to `perf-baseline.md`.

## 11. Milestones

Each milestone lands green on the full CI matrix and does not edit a golden
fixture.

- **M1 — Presentation contract.** Add `Expressed::form`, the version/parity
  proof tests, and the shared expressed-color helper. *Exit:* native and wasm
  fingerprint/parity values and replay/scale hashes are unchanged; map marker
  bytes are unchanged.
- **M2 — GPU directional shadows and CPU handoff.** Add the depth target,
  stabilized camera-relative light fit, terrain caster/receiver path, PCF/bias,
  truthful `B` A/B, capture support, and matrix/WGSL tests. Compare against the
  CPU horizon bake on native Windows, then remove `bake_sun_visibility` while
  retaining AO. *Exit:* terrain shadows are stable and visually sound; CPU
  mesh time falls materially; the selected shadow resolution meets §9; no
  camera movement causes remeshing.
- **M3 — Static end-to-end organisms.** Add pure mapping, ground AO, canonical
  primitive geometry, upload/raw formats, organism color+shadow entries, two
  color draw batches, grow-only buffers, draw ordering, and a minimal live
  replacement upload. *Exit:* boxes/spheres render at correct x/y/size/color,
  receive ground AO and terrain shadow, and cast through the shared depth pass;
  mapping, geometry, precision, and buffer tests pass.
- **M4 — Exact lifecycle, grounding, and captures.** Add
  `PovOrganismManager` exact keys/culling/counters, rendered-lattice grounding,
  AO refresh, shadow-bound union, zero steady-state upload,
  `--pov-script` authoritative settle, F12 upload, and telemetry. *Exit:*
  independently filtered ids/counts match, terrain-only remesh moves body z/AO,
  shadows update without instance replacement, empty replacement clears, and
  repeated captures match.
- **M5 — Tune and sign off on native Windows.** Tune map resolution/bias/PCF,
  assess producer tint and optional activity bob, and keep only changes that
  preserve color readability and the §9 Windows performance posture. Update
  README and `perf-baseline.md`; run the full manual walkthrough plus the
  llvmpipe functional smoke. *Exit:* every §12 checkbox has evidence.

## 12. Phase exit criteria (design §6.3, made checkable)

- [ ] For a settled POV ring, the rendered id set equals every
      `RegionMap::organisms()` id inside the fog/body-radius filter with a
      resident terrain chunk; no tier slot or count cap silently drops an
      eligible organism (tests 8–10).
- [ ] Each rendered body's x/y equals `Organism::world_pos`, its bottom equals
      `PovChunkManager::ground_surface().height`, its ambient factor equals the
      same surface's interpolated AO, and terrain remeshes refresh both even
      when realization identity/expression does not change (tests 5, 9–12;
      manual terrain-only steer scenario).
- [ ] Base RGB bytes exactly equal the existing 2D expressed marker color;
      `form` selects the documented primitive/proportion table; size is linear;
      yaw is id-derived and static (tests 4–7).
- [ ] Members of one species share primitive/proportions; all 16 archetypes are
      distinguishable as eight proportion classes across two primitives.
- [ ] One GPU directional map is rendered before color: terrain core and both
      organism primitives cast; terrain/organisms and river-overlay glint
      receive with stable camera-relative coordinates, bounded PCF, and tuned
      bias. Skirts, river overlays, and sea do not cast; sea remains explicitly
      unshadowed (tests 14–17 and 23; manual A/B).
- [ ] CPU `bake_sun_visibility` and its duplicate shadow authority are removed;
      terrain `light[0]` is neutral/reserved, CPU AO remains in `light[1]`, and
      direct visibility is never packed into an organism (tests 12–13).
- [ ] Color rendering uses at most one box and one sphere instanced draw per
      frame, with terrain → organisms → river → sea depth/blend ordering. Hills
      and above-water bodies occlude correctly; submerged bodies tint through
      sea.
- [ ] A stationary settled POV has zero organism replacement uploads and no
      instance-buffer allocations. Camera translation/look updates frame
      uniforms without rebuilding stable lists unless a distance boundary is
      crossed.
- [ ] `--pov-script` waits for canonical realization completion, F12 captures
      the currently published live set, and two identical time-0 scripted
      captures are byte-equal.
- [ ] `Expressed::form` is proven presentation-only: existing genome/wasm
      parity constants, record format, replay/scale/vault hashes, world/layer
      versions, and golden fixtures are unchanged.
- [ ] `cargo fmt`, clippy/check/test under `-D warnings`, wasm checks and Node
      parity, static web build/smoke, and the Windows MSVC build all pass.
- [ ] `perf-baseline.md` records Low/Mid/High counts/bytes, zero steady upload,
      old/new CPU mesh time, shadow resolution/memory, native-Windows
      terrain-only and dense `B` off/on median/p95, world-update CPU, and
      instance-scan time on the documented hardware adapter.

## 13. Risks and open questions

- **Continuous shadow cost.** The old horizon work happened only on chunk
  remesh; the new depth pass runs every visible frame. That is intentional for
  animation readiness, but native-Windows `B` A/B must justify 1024/2048 and
  PCF cost. Resolution, PCF width, and later caching are valid responses; a
  hidden organism cap is not.
- **Sphere cost.** At two subdivisions, every sphere is 320 triangles and is
  submitted in both shadow and color passes. If the native-Windows High-tier
  dense vantage materially regresses, first test one subdivision (42 vertices,
  80 triangles) and record silhouette/performance tradeoff.
- **Shadow acne versus detachment.** A fixed low sun and exaggerated terrain
  normals make bias tuning sensitive. Tune constant+slope bias against both
  terrain and primitives; do not use a large global offset that makes
  organisms float above their shadows.
- **Shadow-map shimmer.** Camera-relative math fixes precision but not texel
  movement. Extent rounding and texel-snapped centers are phase requirements.
  Cascades and temporal filtering remain later work.
- **Single-map coverage.** Low-angle terrain outside the resident ring cannot
  cast into it, unlike the old analytic horizon march. The fixed radius, caster
  padding, fully-lit border, and fog bound the difference. If native captures
  expose missing long shadows near the viewer, enlarge caster retention or add
  a second cascade in a separate phase; do not sample unloaded world state in
  the renderer.
- **Depth portability.** `Depth32Float` comparison sampling must work through
  native Vulkan/DX12 and WebGPU-compatible wgpu validation even though browser
  POV is deferred. If an adapter rejects the chosen format/usage, select a
  supported sampled depth format at renderer initialization and keep the
  comparison contract unchanged.
- **Linear exact-key scan.** Scanning every published organism each POV frame
  buys exact lifecycle detection without exposing runtime internals. If it is
  measurable, the follow-up is a public monotonically increasing presentation
  epoch plus terrain-chunk generation epochs, or per-region instance lists;
  neither is justified before the baseline.
- **Frontier pop-in.** Realization may publish before its POV chunk upload is
  integrated. Omitting the body until `ground_surface` exists creates honest
  pop-in rather than a floating body. Its shadow appears in the same frame as
  the body upload; fog and the four-chunk upload cap contain the transition,
  and `waiting_for_ground` makes it observable.
- **Large bodies and terrain slope.** Bodies remain world-up and touch the
  terrain at their center point; a wide slab can intersect a steep slope or
  hover at corners. Terrain-normal alignment or multi-point footing would
  change the visual language and is deferred.
- **Producer color ambiguity in the design.** Exact 2D/3D color parity and a
  producer-only tint cannot both describe final lit pixels. This plan pins the
  testable base RGB and makes the subtle shader modulation disposable at M5.
- **Transparent-water ordering.** The fixed opaque-before-water order is
  correct for the current single sea plane. A future multi-layer transparent
  material system will need sorting or order-independent transparency; Phase
  3D-4 does not introduce one.
- **Open-sea shadow omission.** The terrain-conformal river overlay receives
  shadowed sun glint, but the sea remains unshadowed to avoid comparison
  sampling over a large blended plane without an off-ring caster policy. This
  can make coastal shadows end at the shoreline; record the limitation in
  captures and add sea receiving only with explicit coverage and performance
  gates in a later phase.
- **Optional bob and grounding.** A bob intentionally lifts consumers off the
  exact ground after base placement. The grounding exit criterion applies to
  the static base transform; if that distinction reads as a bug in review,
  leave bob disabled.
