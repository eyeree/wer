//! POV mode (3d-phase-1-plan.md): the fly camera, the pure region mesher,
//! and the chunk lifecycle manager.
//!
//! **Derived presentation only (ADR 0017).** Every height the mesher emits is
//! `world_core::terrain::elevation` through its bit-identical SIMD row twin
//! (ADR 0016); every color is the 2D Composite per-cell logic
//! ([`crate::viz::composite_cell_color`]) over the settled field tiles; the
//! baked per-vertex light (sun visibility from a heightfield horizon march,
//! ambient occlusion from multi-scale concavity) is float presentation math
//! over the same elevation kernel and the fixed [`SUN_DIR`]. Nothing here
//! feeds back into world state, hashing, or persistence.
//!
//! The mesher is a pure function of value snapshots (plan §6.1): no
//! filesystem, no threads, no GPU, no `RegionMap` — so it is unit-testable,
//! `Send`-friendly for the executor jobs, and hoistable to a neutral crate
//! for the Phase 7 browser port without rework.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use renderer::pov::{
    skirt_core_index, CORE_VERTS, DETAIL_OCTAVES, POV_GRID, POV_MESH_RES, VERTS_PER_CHUNK,
};
use renderer::{PovFrameParams, PovVertex, TerrainChunkUpload};
use world_core::layer::{layer_decl, LAYER_TERRAIN};
use world_core::{mix, simd, Biome, FieldTile, PossibilityVector, RegionCoord, REGION_SIZE};
use world_runtime::{RegionMap, TaskExecutor, TaskPriority, CHANNEL_RIVER, CHANNEL_WETNESS};

use crate::gpumap::AtlasManager;
use crate::viz::composite_cell_color;

/// Vertex spacing in world units (`REGION_SIZE / POV_MESH_RES` = 4.0).
const SPACING: f64 = REGION_SIZE / POV_MESH_RES as f64;

/// The fixed sun direction (normalized, pointing from the sun toward the
/// ground), shared by [`frame_params`] and the mesher's baked shadow march —
/// the shading and the baked shadows must agree on one sun. Same azimuth as
/// plan §4's original sun, elevation lowered to 20° (permanent late
/// afternoon): this world's heightfield is smooth below ~100-unit
/// wavelengths and its steepest flanks sit near 20°, so only a sun at or
/// below that angle lets ridges cast real shadows or slope shading develop
/// contrast — the definition the near-noon sun washed out entirely.
const SUN_DIR: [f32; 3] = [0.840_446, 0.420_223, -0.342_020_14];

/// Extra sample ring around the 65×65 vertex lattice for the
/// central-difference normals (plan §6.3).
const GRID_MARGIN: usize = 1;

/// Sample-grid edge length: the vertex lattice plus the margin rings.
const SAMPLE_GRID: usize = POV_GRID + 2 * GRID_MARGIN;

/// Baked-shadow march: horizon samples along the horizontal toward-sun
/// direction at exponentially spaced distances `BASE · GROWTH^k` — 8, 14.4,
/// …, ≈490 world units, so nearby banks and distant ridgelines both cast
/// while the per-chunk cost stays bounded (8 row-kernel calls per vertex
/// row). Features beyond ~490 units (≈2 regions) cannot cast onto this
/// chunk; at that range fog has mostly eaten the contrast anyway.
const SHADOW_STEPS: usize = 8;
const SHADOW_STEP_BASE: f64 = 2.0 * SPACING;
const SHADOW_STEP_GROWTH: f64 = 1.8;

/// Self-shadow bias in world units, subtracted from every horizon sample so
/// a surface does not speckle-shadow itself right at the terminator.
const SHADOW_BIAS: f32 = 0.5;

/// Penumbra half-width in horizon-tangent space: the shadow edge fades over
/// `sun_tan ± SOFTNESS` instead of cutting hard, hiding the 4-unit lattice
/// stepping along shadow borders.
const SHADOW_SOFTNESS: f32 = 0.15;

/// Valley-scale AO lattice. The elevation field is smooth below ~100-unit
/// wavelengths (pure low-octave fBm — measured, and visible in any
/// transect), so occlusion lives at hollow/valley scale, far wider than the
/// 4-unit vertex lattice can affordably tap. AO therefore reads a second,
/// coarse height lattice: 32-unit spacing, [`AO_RADII`] taps at 64/128/256
/// world units, margin sized so the widest tap stays on real samples.
const COARSE_SPACING: f64 = 32.0;
const COARSE_MARGIN: usize = 8;
const COARSE_CELLS: usize = (REGION_SIZE / COARSE_SPACING) as usize;
const COARSE_GRID: usize = COARSE_CELLS + 1 + 2 * COARSE_MARGIN;

/// Baked-AO tap radii in coarse-lattice steps (64/128/256 world units).
/// Must not exceed [`COARSE_MARGIN`].
const AO_RADII: [usize; 3] = [2, 4, 8];

/// Baked-AO response: occlusion slope × strength, capped so even the
/// deepest hollow keeps some hemisphere fill. Tuned high — fBm concavities
/// are shallow (slopes of a few percent), and AO is most of the definition
/// valley floors get.
const AO_STRENGTH: f32 = 2.5;
const AO_MAX: f32 = 0.55;

/// Detail-normal gain over the terrain spectrum's exact continuation
/// (1.0 = the amplitude the missing octaves would really have).
const DETAIL_STRENGTH: f32 = 1.0;

/// How far the skirt's bottom ring hangs below the perimeter. The plan sized
/// this at one grid step (4.0) for *drift* steps, but the dominant border
/// step is the possibility-field gradient: adjacent regions differ by up to
/// `1 / cell_regions` (= 1/8) per dimension, which moves elevation by up to
/// `BASE_AMPLITUDE · |relief| · Δgeology + SEA_SHIFT_RANGE · Δplanetary`
/// ≈ 90 world units in the worst case. The wall's unused depth is occluded
/// by the neighbor's terrain, so a deep skirt costs nothing.
pub const POV_SKIRT_DROP: f32 = 128.0;

/// Finished meshes integrated per frame (plan §7.3; tuned against the
/// `docs/perf-baseline.md` methodology).
pub const POV_UPLOADS_PER_FRAME: usize = 4;

/// Chunk-capacity hysteresis above the radius window (plan §7.4), like the
/// region caches: leaving POV keeps chunks; eviction happens on pressure.
const POV_CHUNK_SLACK: usize = 8;

/// Base fly speed in world units per second — person-ish but brisk;
/// `PLAYER_SPEED` is a map-scale constant, wrong for eye level (plan §8.3).
pub const POV_FLY_SPEED: f64 = 40.0;

/// Scroll-wheel speed multiplier bounds (plan §8.3).
const POV_SPEED_RANGE: (f64, f64) = (2.0, 2000.0);

/// Mouse-look sensitivity, radians per raw device pixel.
const LOOK_SENSITIVITY: f32 = 0.0025;

/// Pitch clamp, ±89° in radians (plan §8.2).
const PITCH_LIMIT: f32 = 89.0 * core::f32::consts::PI / 180.0;

/// Eye height above the sampled ground on POV entry (presentation-only; real
/// `ground_height` collision is 3D-2).
const ENTRY_EYE_HEIGHT: f64 = 25.0;

// ---------------------------------------------------------------------------
// Fly camera (plan §4, §8.3)
// ---------------------------------------------------------------------------

/// Presentation-side camera state only — nothing here touches world state,
/// saves, or the vault beyond the player-position recentering the shell does
/// (plan §8.1). Position is f64: world coordinates are f64 everywhere else,
/// and precision at ±10⁶ units matters.
#[derive(Debug)]
pub struct PovCamera {
    /// World position (Z-up: `world_x → X`, `world_y → Y`, elevation → Z).
    pub pos: glam::DVec3,
    /// Radians about +Z; 0 faces +X (east), π/2 faces +Y (north).
    pub yaw: f32,
    /// Radians, clamped to ±89°.
    pub pitch: f32,
    /// Current fly speed, world units per second (scroll-adjusted).
    pub speed: f64,
}

impl PovCamera {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pos: glam::DVec3::ZERO,
            yaw: core::f32::consts::FRAC_PI_2, // facing north, like the map
            pitch: 0.0,
            speed: POV_FLY_SPEED,
        }
    }

    /// Place the camera over a world position at entry: `ground` is the
    /// sampled terrain height there (presentation math; no identity).
    pub fn enter_at(&mut self, world: (f64, f64), ground: f64) {
        self.pos = glam::DVec3::new(world.0, world.1, ground + ENTRY_EYE_HEIGHT);
    }

    /// Apply a raw mouse-motion delta (plan §8.2).
    pub fn look(&mut self, dx: f64, dy: f64) {
        self.yaw -= dx as f32 * LOOK_SENSITIVITY;
        self.pitch = (self.pitch - dy as f32 * LOOK_SENSITIVITY).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// One scroll notch: multiply/divide the fly speed by 1.5, clamped.
    pub fn scroll_speed(&mut self, up: bool) {
        let factor = if up { 1.5 } else { 1.0 / 1.5 };
        self.speed = (self.speed * factor).clamp(POV_SPEED_RANGE.0, POV_SPEED_RANGE.1);
    }

    /// The full 3D view direction (pitch included — it is a fly camera).
    #[must_use]
    pub fn forward(&self) -> glam::DVec3 {
        let (sy, cy) = (f64::from(self.yaw.sin()), f64::from(self.yaw.cos()));
        let (sp, cp) = (f64::from(self.pitch.sin()), f64::from(self.pitch.cos()));
        glam::DVec3::new(cy * cp, sy * cp, sp)
    }

    /// Strafe direction in the yaw plane (horizontal, pitch-independent).
    #[must_use]
    pub fn right(&self) -> glam::DVec3 {
        let (sy, cy) = (f64::from(self.yaw.sin()), f64::from(self.yaw.cos()));
        glam::DVec3::new(sy, -cy, 0.0)
    }

    /// The camera-relative view-projection (plan §4): `look_to` from the
    /// origin (the translation rides in the per-chunk offsets, in f64),
    /// 60° vertical FOV, 0.1..2048 depth. glam's `perspective_rh` produces
    /// exactly wgpu's 0..1 clip depth.
    #[must_use]
    pub fn view_proj(&self, aspect: f32) -> [[f32; 4]; 4] {
        let dir = self.forward().as_vec3();
        let view = glam::camera::rh::view::look_to_mat4(glam::Vec3::ZERO, dir, glam::Vec3::Z);
        // The DirectX/WebGPU convention: right-handed view, clip depth 0..1.
        let proj = glam::camera::rh::proj::directx::perspective(
            60f32.to_radians(),
            aspect.max(1e-3),
            0.1,
            2048.0,
        );
        (proj * view).to_cols_array_2d()
    }
}

impl Default for PovCamera {
    fn default() -> Self {
        Self::new()
    }
}

/// The per-frame renderer parameters for the camera at `radius` regions
/// (plan §4): fog from `0.55·R` to `0.95·R` with `R = (radius + 0.5) ·
/// REGION_SIZE`, fog color = the clear color so geometry dissolves into sky,
/// the fixed sun ([`SUN_DIR`]) and hemisphere ambients tuned so flat ground
/// roughly matches the 2D palette's value range.
#[must_use]
pub fn frame_params(
    camera: &PovCamera,
    aspect: f32,
    radius: i32,
    clear: [f64; 4],
) -> PovFrameParams {
    let reach = (f64::from(radius) + 0.5) * REGION_SIZE;
    let sun = glam::Vec3::from_array(SUN_DIR);
    PovFrameParams {
        view_proj: camera.view_proj(aspect),
        camera_pos: [camera.pos.x, camera.pos.y, camera.pos.z],
        sun_dir: [sun.x, sun.y, sun.z],
        detail: detail_octaves(),
        fog_color: [clear[0] as f32, clear[1] as f32, clear[2] as f32],
        fog_start: (0.55 * reach) as f32,
        fog_end: (0.95 * reach) as f32,
        // Near the 3D-1 fill values: flat ground now sits at ~0.75 of full
        // exposure (sun 0.41 + sky fill) instead of the old ~1.05 — the
        // original tuning overexposed flat ground, clipping every
        // sun-facing slope to the same white and erasing relief. The
        // headroom is what lets slope contrast read in both directions.
        sky_ambient: [0.34, 0.36, 0.40],
        ground_ambient: [0.15, 0.14, 0.13],
    }
}

/// The per-frame detail-normal octave parameters `[frac_x, frac_y,
/// 1/wavelength, slope]` — the POV analogue of `gpumap::refinement_octaves`:
/// continue the terrain gradient spectrum at the octaves above its
/// authoritative top (128/64/32-unit wavelengths), where this world's
/// elevation function is otherwise perfectly smooth. Amplitude halves with
/// wavelength, so each octave's apparent slope is the same constant; it is
/// scaled by [`NORMAL_EXAGGERATION`] to match the vertex normals and by
/// [`DETAIL_STRENGTH`] for taste, and uses the neutral relief amplitude
/// (per-region tectonic scaling isn't worth per-chunk parameters for a
/// shading-only term). Derived presentation (ADR 0017).
fn detail_octaves() -> [[f32; 4]; DETAIL_OCTAVES] {
    use world_core::terrain::{octave_offset, BASE_AMPLITUDE, BASE_WAVELENGTH, OCTAVES};
    // pov_terrain.wgsl hashes the continued octaves as literal `5 + k`.
    const _: () = assert!(OCTAVES == 5, "update pov_terrain.wgsl octave indices");
    let norm: f32 = (0..OCTAVES).map(|k| 0.5f32.powi(k as i32)).sum();
    let mut out = [[0f32; 4]; DETAIL_OCTAVES];
    for (k, slot) in out.iter_mut().enumerate() {
        let octave = OCTAVES + k as u32;
        let wavelength = BASE_WAVELENGTH / f64::from(1u32 << octave);
        let (ox, oy) = octave_offset(octave);
        let amplitude = BASE_AMPLITUDE * 0.5f32.powi(octave as i32) / norm;
        let slope = amplitude / wavelength as f32 * NORMAL_EXAGGERATION * DETAIL_STRENGTH;
        *slot = [
            (ox - ox.floor()) as f32,
            (oy - oy.floor()) as f32,
            (1.0 / wavelength) as f32,
            slope,
        ];
    }
    out
}

/// Per-octave 64-bit base lattice indices of a region's origin for the
/// detail-normal noise — the chunk half of the anchoring ([`detail_octaves`]
/// carries the fractional part, which is octave-global). A region origin is
/// an exact multiple of every detail wavelength, so the base is exact
/// integer math at any world coordinate; the shader adds only the small
/// chunk-local lattice fraction in f32 (the map's refinement anchoring
/// scheme, per chunk instead of per view).
fn detail_base(coord: RegionCoord) -> [[u32; 4]; DETAIL_OCTAVES] {
    use world_core::terrain::{octave_offset, BASE_WAVELENGTH, OCTAVES};
    let mut out = [[0u32; 4]; DETAIL_OCTAVES];
    for (k, slot) in out.iter_mut().enumerate() {
        let octave = OCTAVES + k as u32;
        let wavelength = BASE_WAVELENGTH / f64::from(1u32 << octave);
        let cells = (REGION_SIZE / wavelength) as i64; // exact: 2, 4, 8
        let (ox, oy) = octave_offset(octave);
        let bx = (i64::from(coord.x) * cells + ox.floor() as i64) as u64;
        let by = (i64::from(coord.y) * cells + oy.floor() as i64) as u64;
        *slot = [bx as u32, (bx >> 32) as u32, by as u32, (by >> 32) as u32];
    }
    out
}

/// The terrain height under a world position for POV-entry camera placement:
/// the authoritative `elevation` under the covering region's realized vector
/// (neutral if the region is not resident yet). Presentation-only camera
/// placement — never an identity.
#[must_use]
pub fn entry_ground(map: &RegionMap, world: (f64, f64)) -> f64 {
    let coord = RegionCoord::from_world(world.0, world.1);
    let p = map
        .get(coord)
        .map_or_else(PossibilityVector::neutral, terrain_vector);
    f64::from(world_core::elevation(world.0, world.1, &p)).max(f64::from(world_core::SEA_LEVEL))
}

// ---------------------------------------------------------------------------
// The mesher (plan §6): pure function of value snapshots
// ---------------------------------------------------------------------------

/// Value snapshot a mesh job carries (plan §6.1). The tiles arrive as `Arc`
/// clones held by the job; `p` is the region's terrain-quantized possibility
/// vector (§6.2) — the exact reconstruction `generate.rs` performs for the
/// terrain generator, so mesh heights are bit-equal to what produced the
/// `ELEVATION` tile.
#[derive(Debug)]
pub struct ChunkMeshInputs<'a> {
    pub coord: RegionCoord,
    pub p: PossibilityVector,
    /// `CHANNEL_RIVER`, sampled bilinearly per vertex.
    pub river: &'a FieldTile<f32>,
    /// `CHANNEL_WETNESS`, sampled bilinearly per vertex.
    pub wetness: &'a FieldTile<f32>,
    /// Biome ids, nearest-cell (categorical).
    pub biome: &'a FieldTile<u8>,
    /// Resolved dominant-species ids per cell (row-major, `res²`), 0 = no
    /// tint. The 2D tint is `species_seed(signature, dominant_index)`, whose
    /// signature reads three more tiles — the shell resolves ids at schedule
    /// time so the mesher's inputs stay a small value snapshot.
    pub dominant_ids: &'a [u64],
}

/// A meshed chunk: the GPU vertices plus the CPU-side core heights 3D-2's
/// `ground_height` will start from (plan §1.2).
#[derive(Debug)]
pub struct ChunkMesh {
    /// Exactly [`VERTS_PER_CHUNK`] vertices in the shared topology's order.
    pub vertices: Vec<PovVertex>,
    /// 65×65 core vertex heights, row-major (`j * POV_GRID + i`).
    pub heights: Vec<f32>,
}

/// Mesh one region chunk (plan §6). Deterministic by construction: a pure
/// function of value inputs, fixed iteration order, no RNG, no time.
///
/// The uncancellable entry point — the mesher's spec, exercised by the unit
/// tests and the Phase 7 hoisting surface (design §2); the executor jobs go
/// through [`mesh_region_chunk_cancellable`].
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub fn mesh_region_chunk(inputs: &ChunkMeshInputs<'_>) -> ChunkMesh {
    mesh_region_chunk_cancellable(inputs, &AtomicBool::new(false)).expect("never cancelled")
}

/// [`mesh_region_chunk`] with the job-side cancellation token, checked
/// between row batches (plan §7.2): a cancelled mesh returns `None` and the
/// job no-ops, the exact pattern generation jobs use.
#[must_use]
pub fn mesh_region_chunk_cancellable(
    inputs: &ChunkMeshInputs<'_>,
    cancel: &AtomicBool,
) -> Option<ChunkMesh> {
    // 67×67 sample grid: the 65×65 vertex lattice plus one ring for central
    // differences, at SPACING, spanning [origin − 4, origin + 260] (§6.3).
    const S: usize = SAMPLE_GRID;
    let (ox, oy) = inputs.coord.origin();
    let margin = GRID_MARGIN as f64;
    let xs: Vec<f64> = (0..S).map(|g| ox + (g as f64 - margin) * SPACING).collect();
    let mut h = vec![0f32; S * S];
    for g in 0..S {
        if g % 16 == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        let y = oy + (g as f64 - margin) * SPACING;
        // One batched kernel call per row — the same kernel generation uses,
        // bit-identical to scalar `elevation` (ADR 0016), so vertex heights
        // are *exactly* `elevation(x, y, p)` (asserted by unit test).
        simd::elevation_row(&xs, y, &inputs.p, &mut h[g * S..(g + 1) * S]);
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    // Baked sun visibility (the shadow half of the vertex `light` bytes).
    let sunvis = bake_sun_visibility(&h, (ox, oy), &inputs.p, cancel)?;

    // Coarse height lattice for valley-scale AO (the other half): 25×25 at
    // 32-unit spacing spanning [origin − 256, origin + 512].
    let xsc: Vec<f64> = (0..COARSE_GRID)
        .map(|g| ox + (g as f64 - COARSE_MARGIN as f64) * COARSE_SPACING)
        .collect();
    let mut hc = vec![0f32; COARSE_GRID * COARSE_GRID];
    for g in 0..COARSE_GRID {
        let y = oy + (g as f64 - COARSE_MARGIN as f64) * COARSE_SPACING;
        simd::elevation_row(
            &xsc,
            y,
            &inputs.p,
            &mut hc[g * COARSE_GRID..(g + 1) * COARSE_GRID],
        );
    }
    let occlusion = valley_occlusion(&hc);
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    let res = inputs.river.resolution();
    debug_assert_eq!(inputs.wetness.resolution(), res);
    debug_assert_eq!(inputs.biome.resolution(), res);
    debug_assert_eq!(
        inputs.dominant_ids.len(),
        usize::from(res) * usize::from(res)
    );

    let mut vertices = Vec::with_capacity(VERTS_PER_CHUNK);
    let mut heights = Vec::with_capacity(CORE_VERTS);
    for j in 0..POV_GRID {
        for i in 0..POV_GRID {
            // Sample-grid index of vertex (i, j) is (i + MARGIN, j + MARGIN).
            let (gi, gj) = (i + GRID_MARGIN, j + GRID_MARGIN);
            let e = h[gj * S + gi];
            let normal = vertex_normal(
                h[gj * S + gi - 1],
                h[gj * S + gi + 1],
                h[(gj - 1) * S + gi],
                h[(gj + 1) * S + gi],
            );
            let (lx, ly) = (i as f64 * SPACING, j as f64 * SPACING);
            let river = bilinear(inputs.river, lx, ly);
            let wetness = bilinear(inputs.wetness, lx, ly);
            let (cx, cy) = (nearest_cell(res, lx), nearest_cell(res, ly));
            let biome = Biome::from_id(inputs.biome.get(cx, cy));
            let id = inputs.dominant_ids[usize::from(cy) * usize::from(res) + usize::from(cx)];
            let rgb = composite_cell_color(e, biome, river, wetness, (id != 0).then_some(id));
            vertices.push(PovVertex {
                position: [lx as f32, ly as f32, e],
                normal,
                color: [rgb[0], rgb[1], rgb[2], 255],
                light: [
                    quantize_light(sunvis[j * POV_GRID + i]),
                    quantize_light(vertex_ao(&occlusion, lx, ly)),
                    0,
                    0,
                ],
            });
            heights.push(e);
        }
    }
    // The skirt bottom ring (plan §6.5): same (x, y), normal, color, and
    // baked light as the perimeter vertex above — the skirt reads as the
    // terrain continuing, not as a wall — z lowered by one grid step.
    for edge in 0..4 {
        for k in 0..POV_GRID {
            let mut v = vertices[skirt_core_index(edge, k)];
            v.position[2] -= POV_SKIRT_DROP;
            vertices.push(v);
        }
    }
    debug_assert_eq!(vertices.len(), VERTS_PER_CHUNK);
    Some(ChunkMesh { vertices, heights })
}

/// Baked sun visibility per core vertex — the terrain self-shadow term of
/// the vertex `light` bytes. From each vertex, march the heightfield along
/// the horizontal toward-sun direction at the [`SHADOW_STEPS`] exponential
/// distances (one batched row-kernel call per vertex row per step — the same
/// bit-identical kernel the height lattice uses), track the highest horizon
/// tangent seen, and soft-threshold it against the sun's elevation tangent.
///
/// Derived presentation only (ADR 0017): deterministic float math over the
/// region's terrain-quantized `p`, never an identity. Samples past the
/// region border evaluate under *this* region's `p`, exactly like the
/// heights themselves — the resulting border step is the one the skirt
/// already hides (plan §6.4).
fn bake_sun_visibility(
    h: &[f32],
    origin: (f64, f64),
    p: &PossibilityVector,
    cancel: &AtomicBool,
) -> Option<Vec<f32>> {
    // Horizontal unit vector pointing toward the sun, and the tangent of the
    // sun's elevation above the horizon (1.0 at the 45° SUN_DIR).
    let horiz = f64::hypot(f64::from(SUN_DIR[0]), f64::from(SUN_DIR[1]));
    let toward = (
        -f64::from(SUN_DIR[0]) / horiz,
        -f64::from(SUN_DIR[1]) / horiz,
    );
    let sun_tan = (-f64::from(SUN_DIR[2]) / horiz) as f32;

    // March distances, and each step's vertex-row x positions — x depends
    // only on the column and the step, so each row is built once and reused
    // by all 65 vertex rows.
    let mut dists = [0f64; SHADOW_STEPS];
    let mut d = SHADOW_STEP_BASE;
    for slot in &mut dists {
        *slot = d;
        d *= SHADOW_STEP_GROWTH;
    }
    let step_xs: Vec<Vec<f64>> = dists
        .iter()
        .map(|d| {
            (0..POV_GRID)
                .map(|i| origin.0 + i as f64 * SPACING + toward.0 * d)
                .collect()
        })
        .collect();

    let mut vis = vec![0f32; CORE_VERTS];
    let mut row = vec![0f32; POV_GRID];
    let mut horizon = vec![0f32; POV_GRID];
    for j in 0..POV_GRID {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let y = origin.1 + j as f64 * SPACING;
        horizon.fill(f32::NEG_INFINITY);
        for (k, &dist) in dists.iter().enumerate() {
            simd::elevation_row(&step_xs[k], y + toward.1 * dist, p, &mut row);
            let inv_d = (1.0 / dist) as f32;
            for i in 0..POV_GRID {
                let e = h[(j + GRID_MARGIN) * SAMPLE_GRID + i + GRID_MARGIN];
                horizon[i] = horizon[i].max((row[i] - e - SHADOW_BIAS) * inv_d);
            }
        }
        for i in 0..POV_GRID {
            let lit = 1.0
                - smoothstep(
                    sun_tan - SHADOW_SOFTNESS,
                    sun_tan + SHADOW_SOFTNESS,
                    horizon[i],
                );
            vis[j * POV_GRID + i] = lit;
        }
    }
    Some(vis)
}

/// Valley-scale occlusion over the region's own coarse nodes (a 9×9 patch):
/// multi-scale concavity, how steeply the mean of the four axial neighbors
/// at each [`AO_RADII`] radius rises above the node. Hollows and valley
/// floors read positive; ridges and flats read zero. [`COARSE_MARGIN`]
/// keeps every tap on real samples.
fn valley_occlusion(hc: &[f32]) -> Vec<f32> {
    let n = COARSE_CELLS + 1;
    let mut out = vec![0f32; n * n];
    for j in 0..n {
        for i in 0..n {
            let (gi, gj) = (i + COARSE_MARGIN, j + COARSE_MARGIN);
            let e = hc[gj * COARSE_GRID + gi];
            let mut occl = 0.0f32;
            for &r in &AO_RADII {
                let mean = 0.25
                    * (hc[gj * COARSE_GRID + gi - r]
                        + hc[gj * COARSE_GRID + gi + r]
                        + hc[(gj - r) * COARSE_GRID + gi]
                        + hc[(gj + r) * COARSE_GRID + gi]);
                occl += ((mean - e) / (r as f32 * COARSE_SPACING as f32)).max(0.0);
            }
            out[j * n + i] = occl / AO_RADII.len() as f32;
        }
    }
    out
}

/// The baked-AO byte value for a vertex at chunk-local `(lx, ly)`: bilinear
/// over the [`valley_occlusion`] patch, mapped through strength and cap.
fn vertex_ao(occlusion: &[f32], lx: f64, ly: f64) -> f32 {
    let max = COARSE_CELLS as f64;
    let gx = (lx / COARSE_SPACING).clamp(0.0, max);
    let gy = (ly / COARSE_SPACING).clamp(0.0, max);
    let (x0, y0) = (gx.floor().min(max - 1.0), gy.floor().min(max - 1.0));
    let (i0, j0) = (x0 as usize, y0 as usize);
    let (tx, ty) = ((gx - x0) as f32, (gy - y0) as f32);
    let n = COARSE_CELLS + 1;
    let v00 = occlusion[j0 * n + i0];
    let v10 = occlusion[j0 * n + i0 + 1];
    let v01 = occlusion[(j0 + 1) * n + i0];
    let v11 = occlusion[(j0 + 1) * n + i0 + 1];
    let top = v00 + (v10 - v00) * tx;
    let bottom = v01 + (v11 - v01) * tx;
    let occl = top + (bottom - top) * ty;
    1.0 - (AO_STRENGTH * occl).min(AO_MAX)
}

/// The GLSL/WGSL `smoothstep`, for the baked shadow's soft threshold.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A `[0, 1]` light factor to its `Unorm8` vertex byte (round to nearest).
fn quantize_light(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// How much steeper the shading normals lean than the true surface. The
/// plan §6.3 formula (`z = 2 · spacing · 2`) halved apparent slopes — the
/// stray `· 2` read as terrain with no relief at all; `1.0` here is the
/// mathematically true central-difference normal, and values above it
/// deliberately exaggerate slope shading so this world's gentle fBm hills
/// stay readable under the fixed sun. Presentation-only.
const NORMAL_EXAGGERATION: f32 = 1.5;

/// Central-difference normal (plan §6.3, with [`NORMAL_EXAGGERATION`]):
/// `normalize((west − east, south − north, 2 · spacing / exaggeration))`.
/// Presentation-only float math; a flat heightfield yields exactly
/// `(0, 0, 1)`.
fn vertex_normal(west: f32, east: f32, south: f32, north: f32) -> [f32; 3] {
    let (nx, ny) = (west - east, south - north);
    let nz = 2.0 * SPACING as f32 / NORMAL_EXAGGERATION;
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    [nx / len, ny / len, nz / len]
}

/// Bilinear sample over the four nearest cell centers of the region's own
/// tile (centers at `(c + 0.5) · cell`), coordinates clamped to the region
/// interior — a chunk never reads a neighbor's tiles (plan §6.4); the skirt
/// hides any hairline color step at borders exactly as it hides the height
/// step. At a cell center this returns the cell's exact value.
fn bilinear(tile: &FieldTile<f32>, lx: f64, ly: f64) -> f32 {
    let res = tile.resolution();
    let cell = REGION_SIZE / f64::from(res);
    let max = f64::from(res - 1);
    let gx = (lx / cell - 0.5).clamp(0.0, max);
    let gy = (ly / cell - 0.5).clamp(0.0, max);
    let (x0, y0) = (gx.floor(), gy.floor());
    let (cx0, cy0) = (x0 as u16, y0 as u16);
    let (cx1, cy1) = ((cx0 + 1).min(res - 1), (cy0 + 1).min(res - 1));
    let (tx, ty) = ((gx - x0) as f32, (gy - y0) as f32);
    let v00 = tile.get(cx0, cy0);
    let v10 = tile.get(cx1, cy0);
    let v01 = tile.get(cx0, cy1);
    let v11 = tile.get(cx1, cy1);
    let top = v00 + (v10 - v00) * tx;
    let bottom = v01 + (v11 - v01) * tx;
    top + (bottom - top) * ty
}

/// The cell containing local coordinate `l` (categorical channels sample
/// nearest-cell; blending ids is meaningless, plan §6.4).
fn nearest_cell(res: u16, l: f64) -> u16 {
    let cell = REGION_SIZE / f64::from(res);
    // Negative float→int casts saturate to 0, so the clamp is total.
    ((l / cell) as u16).min(res - 1)
}

// ---------------------------------------------------------------------------
// Chunk lifecycle (plan §7)
// ---------------------------------------------------------------------------

/// The chunk key (plan §7.1): the atlas dependency-hash key of the region's
/// tiles folded with the terrain-domain quantized buckets, so a drift step
/// that flips a terrain bucket forces a remesh in the same breath that it
/// dirties the tiles. Steady state: same tiles, same buckets ⇒ same key ⇒
/// zero remesh traffic — exact by the same argument that makes atlas
/// upload-skipping exact (ADR 0008).
///
/// `None` until the tiles the mesher needs are present; holes at the loading
/// frontier are acceptable in 3D-1 and hide in fog (plan §7.1).
fn chunk_key(map: &RegionMap, coord: RegionCoord) -> Option<u64> {
    let tiles = map.cache().get(coord)?;
    tiles.channels[CHANNEL_RIVER].as_ref()?;
    tiles.channels[CHANNEL_WETNESS].as_ref()?;
    tiles.biome.as_ref()?;
    tiles.dominant.as_ref()?;
    let region_key = AtlasManager::region_key(map, coord)?;
    let state = map.get(coord)?;
    let buckets = state
        .current
        .quantized_domains(layer_decl(LAYER_TERRAIN).domains);
    Some(chunk_key_from(region_key, &buckets))
}

/// The pure fold of [`chunk_key`], separated for unit tests.
fn chunk_key_from(region_key: u64, buckets: &[u16]) -> u64 {
    let mut h = mix(0x3D01_C4A5_B00C_0001, region_key);
    for &bucket in buckets {
        h = mix(h, u64::from(bucket));
    }
    h
}

/// The terrain-quantized possibility vector for a region's mesh (plan §6.2):
/// exactly the reconstruction `generate.rs` performs for the terrain
/// generator, so mesh heights agree bit-exactly with the `ELEVATION` tile.
fn terrain_vector(state: &world_runtime::RegionState) -> PossibilityVector {
    let decl = layer_decl(LAYER_TERRAIN);
    let buckets = state.current.quantized_domains(decl.domains);
    PossibilityVector::from_quantized(decl.domains, &buckets)
}

/// Lifecycle counters (plan §7.5): telemetry only — never gating (ADR 0018
/// posture). The steady-state exit criterion reads these: travel stopped ⇒
/// `remeshed` stays flat.
#[derive(Debug, Default, Clone, Copy)]
pub struct PovCounters {
    /// Chunks meshed for the first time.
    pub meshed: u64,
    /// Chunks re-meshed after a key change.
    pub remeshed: u64,
    /// In-flight jobs cancelled (superseded or out of radius).
    pub cancelled: u64,
    /// Finished meshes dropped because their key was no longer wanted.
    pub dropped_stale: u64,
    /// Finished meshes deferred past the per-frame upload cap.
    pub uploads_deferred: u64,
    /// Worker-side mesh milliseconds, accumulated.
    pub mesh_ms: f64,
}

/// A finished mesh coming back from the executor.
struct MeshResult {
    coord: RegionCoord,
    key: u64,
    mesh: ChunkMesh,
}

/// A resident chunk: its key, its renderer handle, and the CPU-side core
/// heights (kept for 3D-2's `ground_height`, plan §1.2).
#[derive(Debug)]
struct ChunkEntry {
    key: u64,
    handle: u64,
    #[allow(dead_code)] // consumed by 3D-2's ground_height
    heights: Vec<f32>,
}

/// An in-flight mesh job: the key it will produce and its cancellation token.
#[derive(Debug)]
struct InFlight {
    key: u64,
    cancel: Arc<AtomicBool>,
}

/// Chunk lifecycle manager (plan §7), mirroring `AtlasManager`: walk resident
/// regions, compare keys, schedule stale work, amortize uploads, evict
/// farthest — with the one structural difference that meshing is
/// *asynchronous* (Background-lane CPU work) where atlas packing is
/// synchronous.
#[derive(Debug)]
pub struct PovChunkManager {
    chunks: HashMap<RegionCoord, ChunkEntry>,
    in_flight: HashMap<RegionCoord, InFlight>,
    pending: VecDeque<MeshResult>,
    tx: mpsc::Sender<MeshResult>,
    rx: mpsc::Receiver<MeshResult>,
    next_handle: u64,
    /// Worker-side mesh time, microseconds (atomic: workers accumulate).
    mesh_micros: Arc<AtomicU64>,
    counters: PovCounters,
}

impl std::fmt::Debug for MeshResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshResult")
            .field("coord", &self.coord)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl PovChunkManager {
    #[must_use]
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            chunks: HashMap::new(),
            in_flight: HashMap::new(),
            pending: VecDeque::new(),
            tx,
            rx,
            next_handle: 1,
            mesh_micros: Arc::new(AtomicU64::new(0)),
            counters: PovCounters::default(),
        }
    }

    /// This frame's counter snapshot (plan §7.5).
    #[must_use]
    pub fn counters(&self) -> PovCounters {
        let mut counters = self.counters;
        counters.mesh_ms = self.mesh_micros.load(Ordering::Relaxed) as f64 / 1000.0;
        counters
    }

    /// Resident chunk count (telemetry).
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether nothing is in flight or awaiting integration — with an inline
    /// executor, repeated `sync` calls until `idle` fully settle the ring
    /// (the `--pov-script` snapshot path).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.in_flight.is_empty()
    }

    /// Per-frame sync (plan §7.2–§7.4): drain finished meshes, schedule
    /// stale/missing chunks within `radius` regions of the camera on the
    /// executor's Background lane, integrate at most
    /// [`POV_UPLOADS_PER_FRAME`] results, and evict farthest-first over
    /// capacity. Returns the uploads and evicted handles for
    /// [`renderer::Renderer::render_pov`].
    pub fn sync(
        &mut self,
        map: &RegionMap,
        camera: (f64, f64),
        radius: i32,
        executor: &dyn TaskExecutor,
    ) -> (Vec<TerrainChunkUpload>, Vec<u64>) {
        // Drain finished meshes; a result whose key is no longer the one in
        // flight for its region (superseded or cancelled-after-start) drops.
        // The in-flight entry stays until integration so the scheduling walk
        // below sees a finished-but-not-yet-integrated mesh as satisfied.
        self.drain();

        // Walk the radius window in fixed row-major order (reproducible
        // scheduling; not identity-relevant) and schedule stale work.
        let center = RegionCoord::from_world(camera.0, camera.1);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let coord = RegionCoord::new(center.x + dx, center.y + dy);
                let Some(key) = chunk_key(map, coord) else {
                    continue; // not settled yet: hole at the frontier, hidden in fog
                };
                if self.chunks.get(&coord).is_some_and(|c| c.key == key) {
                    continue; // steady state
                }
                match self.in_flight.get(&coord) {
                    Some(job) if job.key == key => continue, // already meshing
                    Some(job) => {
                        // Superseded: cancel the old job (plan §7.2 step 4).
                        job.cancel.store(true, Ordering::Relaxed);
                        self.counters.cancelled += 1;
                    }
                    None => {}
                }
                self.schedule(map, coord, key, executor);
            }
        }

        // Cancel in-flight jobs for regions that left the radius.
        let visible =
            |c: &RegionCoord| (c.x - center.x).abs() <= radius && (c.y - center.y).abs() <= radius;
        let gone: Vec<RegionCoord> = self
            .in_flight
            .keys()
            .filter(|c| !visible(c))
            .copied()
            .collect();
        for coord in gone {
            if let Some(job) = self.in_flight.remove(&coord) {
                job.cancel.store(true, Ordering::Relaxed);
                self.counters.cancelled += 1;
            }
        }

        // Amortized integration (plan §7.3): at most POV_UPLOADS_PER_FRAME
        // finished meshes become uploads; the rest stay queued. A remesh
        // keeps its handle, so the old chunk draws until the swap lands.
        // Drain again first: under `--inline`, this frame's jobs finished
        // synchronously inside `submit` and their results are ready now.
        self.drain();
        let mut uploads = Vec::new();
        while uploads.len() < POV_UPLOADS_PER_FRAME {
            let Some(result) = self.pending.pop_front() else {
                break;
            };
            // Superseded between drain and integration: drop, the newer job
            // is already in flight.
            match self.in_flight.get(&result.coord) {
                Some(job) if job.key == result.key => {
                    self.in_flight.remove(&result.coord);
                }
                Some(_) => {
                    self.counters.dropped_stale += 1;
                    continue;
                }
                None => {} // job was cancelled out-of-radius; integrate anyway
            }
            let handle = match self.chunks.get(&result.coord) {
                Some(entry) => {
                    self.counters.remeshed += 1;
                    entry.handle
                }
                None => {
                    self.counters.meshed += 1;
                    let handle = self.next_handle;
                    self.next_handle += 1;
                    handle
                }
            };
            self.chunks.insert(
                result.coord,
                ChunkEntry {
                    key: result.key,
                    handle,
                    heights: result.mesh.heights,
                },
            );
            let (ox, oy) = result.coord.origin();
            uploads.push(TerrainChunkUpload {
                handle,
                origin: [ox, oy],
                detail_base: detail_base(result.coord),
                vertices: result.mesh.vertices,
            });
        }
        self.counters.uploads_deferred += self.pending.len() as u64;

        // Farthest-first eviction over capacity (plan §7.4, the Phase 6
        // cache discipline); the handle returns the buffer to the pool.
        let span = 2 * radius as usize + 1;
        let capacity = span * span + POV_CHUNK_SLACK;
        let mut removes = Vec::new();
        while self.chunks.len() > capacity {
            let Some(coord) = farthest_region(self.chunks.keys().copied(), camera) else {
                break;
            };
            if let Some(entry) = self.chunks.remove(&coord) {
                removes.push(entry.handle);
            }
        }
        (uploads, removes)
    }

    /// Pull finished meshes off the channel. A result whose key does not
    /// match the job in flight for its region — superseded, or cancelled
    /// after it already started — is dropped and counted (plan §7.2 step 4).
    fn drain(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            match self.in_flight.get(&result.coord) {
                Some(job) if job.key == result.key => self.pending.push_back(result),
                _ => self.counters.dropped_stale += 1,
            }
        }
    }

    /// Snapshot a region's inputs and submit its mesh job on the Background
    /// lane — the same `TaskExecutor` the world update uses, so `--inline`
    /// runs meshing synchronously too, and Background priority keeps meshing
    /// behind the Critical/Normal generation work the chunk itself depends
    /// on (plan §7.2).
    fn schedule(
        &mut self,
        map: &RegionMap,
        coord: RegionCoord,
        key: u64,
        executor: &dyn TaskExecutor,
    ) {
        let Some(tiles) = map.cache().get(coord) else {
            return;
        };
        let (Some(river), Some(wetness), Some(biome), Some(dominant)) = (
            tiles.channels[CHANNEL_RIVER].clone(),
            tiles.channels[CHANNEL_WETNESS].clone(),
            tiles.biome.clone(),
            tiles.dominant.clone(),
        ) else {
            return;
        };
        let Some(state) = map.get(coord) else {
            return;
        };
        let p = terrain_vector(state);
        // Resolve the dominant-species tint ids here, where the map is in
        // reach; 0 = no tint (ecology inputs not settled for that cell).
        let res = dominant.resolution();
        let mut dominant_ids = Vec::with_capacity(usize::from(res) * usize::from(res));
        for cy in 0..res {
            for cx in 0..res {
                dominant_ids.push(map.dominant_species_id(coord, cx, cy).unwrap_or(0));
            }
        }

        let cancel = Arc::new(AtomicBool::new(false));
        self.in_flight.insert(
            coord,
            InFlight {
                key,
                cancel: Arc::clone(&cancel),
            },
        );
        let tx = self.tx.clone();
        let micros = Arc::clone(&self.mesh_micros);
        executor.submit(
            TaskPriority::Background,
            Box::new(move || {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let start = std::time::Instant::now();
                let inputs = ChunkMeshInputs {
                    coord,
                    p,
                    river: &river,
                    wetness: &wetness,
                    biome: &biome,
                    dominant_ids: &dominant_ids,
                };
                let Some(mesh) = mesh_region_chunk_cancellable(&inputs, &cancel) else {
                    return;
                };
                micros.fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);
                // The receiver may be gone during shutdown; nothing to do.
                let _ = tx.send(MeshResult { coord, key, mesh });
            }),
        );
    }
}

impl Default for PovChunkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The resident region farthest from the camera (region-center distance),
/// coordinate-ordered on ties for determinism.
fn farthest_region(
    coords: impl Iterator<Item = RegionCoord>,
    camera: (f64, f64),
) -> Option<RegionCoord> {
    let mut best: Option<(f64, RegionCoord)> = None;
    for coord in coords {
        let (ox, oy) = coord.origin();
        let (cx, cy) = (ox + REGION_SIZE * 0.5, oy + REGION_SIZE * 0.5);
        let d = f64::hypot(cx - camera.0, cy - camera.1);
        let better = match &best {
            None => true,
            Some((bd, bc)) => d > *bd || (d == *bd && (coord.x, coord.y) > (bc.x, bc.y)),
        };
        if better {
            best = Some((d, coord));
        }
    }
    best.map(|(_, coord)| coord)
}

// ---------------------------------------------------------------------------
// The scripted headless POV driver (`wer --pov-script`, ADR 0021)
// ---------------------------------------------------------------------------

/// One instruction of a `--pov-script` sequence: simulate camera input, let
/// the world settle, or capture a snapshot. Parsed by [`parse_pov_script`];
/// executed by the shell's headless runner. Every camera-affecting
/// instruction goes through the *same* [`PovCamera`] code paths the live
/// shell drives, so a scripted capture reproduces live behavior.
#[derive(Debug, Clone, PartialEq)]
pub enum PovInstr {
    /// `size:WxH` — capture resolution (before the first `snap`; default
    /// 1024×768).
    Size(u32, u32),
    /// `pos:x,y[,z]` — place the camera; without `z` it sits at entry eye
    /// height over the sampled ground.
    Pos(f64, f64, Option<f64>),
    /// `mouse:dx,dy` — a simulated raw mouse-look delta in pixels, through
    /// [`PovCamera::look`].
    Mouse(f64, f64),
    /// `move:f[,r[,u]]` — fly `f` world units along the view direction, `r`
    /// strafing right, `u` straight up (the held-key movement basis).
    Move { forward: f64, right: f64, up: f64 },
    /// `settle[:n]` — run `n` (default 8) zero-travel world updates at the
    /// camera position, so tiles generate/regenerate.
    Settle(u32),
    /// `snap:path.ppm` — settle the chunk ring and write a snapshot.
    Snap(String),
}

/// Parse a `--pov-script` instruction sequence: instructions separated by
/// `;`, each `op` or `op:args` with comma-separated args, e.g.
/// `"pos:300,-10; mouse:120,40; snap:a.ppm; move:200; settle; snap:b.ppm"`.
pub fn parse_pov_script(script: &str) -> Result<Vec<PovInstr>, String> {
    let mut out = Vec::new();
    for raw in script.split(';') {
        let instr = raw.trim();
        if instr.is_empty() {
            continue;
        }
        let (op, args) = match instr.split_once(':') {
            Some((op, args)) => (op.trim(), args.trim()),
            None => (instr, ""),
        };
        let nums = || -> Result<Vec<f64>, String> {
            args.split(',')
                .map(|a| {
                    a.trim()
                        .parse::<f64>()
                        .map_err(|_| format!("bad number {a:?} in {instr:?}"))
                })
                .collect()
        };
        out.push(match op {
            "size" => {
                let dims: Vec<&str> = args.split(['x', ',']).map(str::trim).collect();
                match dims[..] {
                    [w, h] => match (w.parse::<u32>(), h.parse::<u32>()) {
                        (Ok(w), Ok(h)) if w > 0 && h > 0 => PovInstr::Size(w, h),
                        _ => return Err(format!("bad size {args:?} (want WxH)")),
                    },
                    _ => return Err(format!("bad size {args:?} (want WxH)")),
                }
            }
            "pos" => match nums()?[..] {
                [x, y] => PovInstr::Pos(x, y, None),
                [x, y, z] => PovInstr::Pos(x, y, Some(z)),
                _ => return Err(format!("pos wants x,y[,z], got {args:?}")),
            },
            "mouse" => match nums()?[..] {
                [dx, dy] => PovInstr::Mouse(dx, dy),
                _ => return Err(format!("mouse wants dx,dy, got {args:?}")),
            },
            "move" => match nums()?[..] {
                [f] => PovInstr::Move {
                    forward: f,
                    right: 0.0,
                    up: 0.0,
                },
                [f, r] => PovInstr::Move {
                    forward: f,
                    right: r,
                    up: 0.0,
                },
                [f, r, u] => PovInstr::Move {
                    forward: f,
                    right: r,
                    up: u,
                },
                _ => return Err(format!("move wants f[,r[,u]], got {args:?}")),
            },
            "settle" => {
                if args.is_empty() {
                    PovInstr::Settle(8)
                } else {
                    match args.parse::<u32>() {
                        Ok(n) => PovInstr::Settle(n),
                        Err(_) => return Err(format!("settle wants a count, got {args:?}")),
                    }
                }
            }
            "snap" => {
                if args.is_empty() {
                    return Err(String::from("snap wants a file path"));
                }
                PovInstr::Snap(String::from(args))
            }
            other => return Err(format!("unknown instruction {other:?}")),
        });
    }
    if out.is_empty() {
        return Err(String::from("empty script"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderer::pov::chunk_indices;
    use world_core::terrain::elevation;
    use world_core::{PossibilityField, POSSIBILITY_DIMS};
    use world_runtime::{Budget, InlineExecutor, StreamConfig, CHANNEL_ELEVATION};

    /// A small fully-settled window (the `gpumap.rs` test fixture).
    fn settled_map() -> RegionMap {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 8,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let mut map = RegionMap::new(cfg);
        for _ in 0..6 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        map
    }

    /// Owned mesher inputs snapshotted from a settled region, the way
    /// `PovChunkManager::schedule` builds them.
    struct Snapshot {
        coord: RegionCoord,
        p: PossibilityVector,
        river: Arc<FieldTile<f32>>,
        wetness: Arc<FieldTile<f32>>,
        biome: Arc<FieldTile<u8>>,
        dominant_ids: Vec<u64>,
    }

    impl Snapshot {
        fn of(map: &RegionMap, coord: RegionCoord) -> Self {
            let tiles = map.cache().get(coord).expect("region settled");
            let res = map.config().field_resolution;
            let mut dominant_ids = Vec::new();
            for cy in 0..res {
                for cx in 0..res {
                    dominant_ids.push(map.dominant_species_id(coord, cx, cy).unwrap_or(0));
                }
            }
            Self {
                coord,
                p: terrain_vector(map.get(coord).expect("resident")),
                river: tiles.channels[CHANNEL_RIVER].clone().expect("river tile"),
                wetness: tiles.channels[CHANNEL_WETNESS].clone().expect("wetness"),
                biome: tiles.biome.clone().expect("biome tile"),
                dominant_ids,
            }
        }

        fn inputs(&self) -> ChunkMeshInputs<'_> {
            ChunkMeshInputs {
                coord: self.coord,
                p: self.p,
                river: &self.river,
                wetness: &self.wetness,
                biome: &self.biome,
                dominant_ids: &self.dominant_ids,
            }
        }
    }

    #[test]
    fn pov_script_parses_the_documented_forms() {
        let script = "size:640x360; pos:300,-10; mouse:120,-40; snap:a.ppm; \
                      move:200; move:0,-50,25; settle; settle:3; snap:b.ppm";
        let parsed = parse_pov_script(script).expect("valid script");
        assert_eq!(
            parsed,
            vec![
                PovInstr::Size(640, 360),
                PovInstr::Pos(300.0, -10.0, None),
                PovInstr::Mouse(120.0, -40.0),
                PovInstr::Snap(String::from("a.ppm")),
                PovInstr::Move {
                    forward: 200.0,
                    right: 0.0,
                    up: 0.0
                },
                PovInstr::Move {
                    forward: 0.0,
                    right: -50.0,
                    up: 25.0
                },
                PovInstr::Settle(8),
                PovInstr::Settle(3),
                PovInstr::Snap(String::from("b.ppm")),
            ]
        );
        assert!(parse_pov_script("").is_err());
        assert!(parse_pov_script("teleport:1,2").is_err());
        assert!(parse_pov_script("mouse:1").is_err());
        assert!(parse_pov_script("snap").is_err());
        assert!(parse_pov_script("size:0x4").is_err());
    }

    #[test]
    fn mesher_is_deterministic() {
        // Plan §10 check 1: identical inputs ⇒ byte-identical output.
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let a = mesh_region_chunk(&snap.inputs());
        let b = mesh_region_chunk(&snap.inputs());
        assert_eq!(
            bytemuck_bytes(&a.vertices),
            bytemuck_bytes(&b.vertices),
            "vertex bytes must be identical"
        );
        let bits = |h: &[f32]| h.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&a.heights), bits(&b.heights));
    }

    fn bytemuck_bytes(vertices: &[PovVertex]) -> &[u8] {
        bytemuck::cast_slice(vertices)
    }

    #[test]
    fn vertex_heights_equal_scalar_elevation_bit_exactly() {
        // Plan §10 check 2: the ADR 0016 twin guarantee, re-asserted at the
        // consumer — and cell centers match the ELEVATION tile bit-exactly.
        let map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let snap = Snapshot::of(&map, coord);
        let mesh = mesh_region_chunk(&snap.inputs());
        let (ox, oy) = coord.origin();
        for j in 0..POV_GRID {
            for i in 0..POV_GRID {
                let expected = elevation(ox + i as f64 * SPACING, oy + j as f64 * SPACING, &snap.p);
                let got = mesh.vertices[j * POV_GRID + i].position[2];
                assert_eq!(got.to_bits(), expected.to_bits(), "vertex ({i}, {j})");
                assert_eq!(mesh.heights[j * POV_GRID + i].to_bits(), expected.to_bits());
            }
        }
        // Cell centers: same function, same quantized vector as generation.
        let res = map.config().field_resolution;
        let tiles = map.cache().get(coord).expect("settled");
        let elev = tiles.channels[CHANNEL_ELEVATION].as_ref().expect("tile");
        let stride = POV_GRID / usize::from(res); // vertices per cell
        for cy in 0..res {
            for cx in 0..res {
                let i = usize::from(cx) * stride + stride / 2;
                let j = usize::from(cy) * stride + stride / 2;
                let vertex = mesh.vertices[j * POV_GRID + i].position[2];
                assert_eq!(
                    vertex.to_bits(),
                    elev.get(cx, cy).to_bits(),
                    "cell ({cx}, {cy}) center"
                );
            }
        }
    }

    #[test]
    fn cell_center_colors_match_the_2d_composite() {
        // Plan §10 check 3: at a cell center the vertex color equals the 2D
        // Composite pixel color for that cell, by construction.
        let map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let snap = Snapshot::of(&map, coord);
        let mesh = mesh_region_chunk(&snap.inputs());
        let res = map.config().field_resolution;
        let tiles = map.cache().get(coord).expect("settled");
        let elev = tiles.channels[CHANNEL_ELEVATION].as_ref().expect("tile");
        let stride = POV_GRID / usize::from(res);
        for cy in 0..res {
            for cx in 0..res {
                let expected = composite_cell_color(
                    elev.get(cx, cy),
                    Biome::from_id(snap.biome.get(cx, cy)),
                    snap.river.get(cx, cy),
                    snap.wetness.get(cx, cy),
                    map.dominant_species_id(coord, cx, cy),
                );
                let i = usize::from(cx) * stride + stride / 2;
                let j = usize::from(cy) * stride + stride / 2;
                let got = mesh.vertices[j * POV_GRID + i].color;
                assert_eq!(
                    [got[0], got[1], got[2]],
                    expected,
                    "cell ({cx}, {cy}) center color"
                );
                assert_eq!(got[3], 255, "alpha reserved at 255");
            }
        }
    }

    #[test]
    fn skirt_is_watertight() {
        // Plan §10 check 4: every perimeter core vertex has a skirt partner
        // at identical (x, y, color), z lowered by exactly POV_SKIRT_DROP.
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        assert_eq!(mesh.vertices.len(), VERTS_PER_CHUNK);
        assert_eq!(mesh.heights.len(), CORE_VERTS);
        for edge in 0..4 {
            for k in 0..POV_GRID {
                let top = mesh.vertices[skirt_core_index(edge, k)];
                let bottom = mesh.vertices[CORE_VERTS + edge * POV_GRID + k];
                assert_eq!(top.position[0], bottom.position[0]);
                assert_eq!(top.position[1], bottom.position[1]);
                assert_eq!(top.color, bottom.color);
                assert_eq!(top.normal, bottom.normal);
                assert_eq!(top.light, bottom.light);
                assert_eq!(
                    top.position[2] - bottom.position[2],
                    POV_SKIRT_DROP,
                    "skirt drop at edge {edge}, k {k}"
                );
            }
        }
    }

    #[test]
    fn core_triangles_wind_ccw_from_above() {
        // Plan §10 check 5: positive-z cross product in chunk-local space,
        // for back-face culling. Heightfields never overhang, so this holds
        // on real terrain, not just on a flat grid.
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        let indices = chunk_indices();
        let core = POV_MESH_RES * POV_MESH_RES * 6;
        for tri in indices[..core].chunks_exact(3) {
            let p = |v: u32| mesh.vertices[v as usize].position;
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let cross_z = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(cross_z > 0.0, "triangle {tri:?} winds clockwise");
        }
    }

    #[test]
    fn normals_are_unit_length_and_flat_ground_points_up() {
        // Plan §10 check 6.
        assert_eq!(vertex_normal(0.0, 0.0, 0.0, 0.0), [0.0, 0.0, 1.0]);
        assert_eq!(vertex_normal(5.5, 5.5, 5.5, 5.5), [0.0, 0.0, 1.0]);
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        for (i, v) in mesh.vertices.iter().enumerate() {
            let n = v.normal;
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "vertex {i} normal length {len}");
        }
    }

    #[test]
    fn baked_ao_is_full_on_flats_and_ridges_and_dims_hollows() {
        // Flat coarse lattice: zero concavity, full ambient everywhere.
        let flat = vec![100.0f32; COARSE_GRID * COARSE_GRID];
        let occl = valley_occlusion(&flat);
        assert!(occl.iter().all(|&o| o == 0.0));
        assert_eq!(vertex_ao(&occl, 128.0, 128.0), 1.0);
        // A ridge node (above its neighbors) is convex: still full.
        let center = COARSE_GRID / 2;
        let mut ridge = flat.clone();
        ridge[center * COARSE_GRID + center] = 200.0;
        let node = (COARSE_CELLS / 2, COARSE_CELLS / 2);
        let occl = valley_occlusion(&ridge);
        assert_eq!(occl[node.1 * (COARSE_CELLS + 1) + node.0], 0.0);
        // A deep hollow loses fill, floored at AO_MAX; a shallow one dims
        // gently. The hollow node sits at the region-center vertex.
        let mut hollow = flat.clone();
        hollow[center * COARSE_GRID + center] = -300.0;
        let deep = vertex_ao(&valley_occlusion(&hollow), 128.0, 128.0);
        assert!((deep - (1.0 - AO_MAX)).abs() < 1e-6, "deep hollow at cap");
        let mut dip = flat;
        dip[center * COARSE_GRID + center] = 96.0;
        let shallow = vertex_ao(&valley_occlusion(&dip), 128.0, 128.0);
        assert!(shallow < 1.0 && shallow > deep, "shallow dip dims gently");
    }

    #[test]
    fn detail_bases_are_lattice_continuous_across_regions() {
        // Neighboring regions must anchor the same world lattice: stepping
        // one region east advances octave k's x base by exactly
        // REGION_SIZE / wavelength cells (2, 4, 8), and the shared
        // fractional offset stays in [0, 1).
        let a = detail_base(RegionCoord::new(10, -3));
        let b = detail_base(RegionCoord::new(11, -3));
        let c = detail_base(RegionCoord::new(10, -2));
        for (k, cells) in [(0usize, 2i64), (1, 4), (2, 8)] {
            let x = |s: &[u32; 4]| (u64::from(s[1]) << 32 | u64::from(s[0])) as i64;
            let y = |s: &[u32; 4]| (u64::from(s[3]) << 32 | u64::from(s[2])) as i64;
            assert_eq!(x(&b[k]) - x(&a[k]), cells, "octave {k} x step");
            assert_eq!(y(&c[k]) - y(&a[k]), cells, "octave {k} y step");
        }
        for octave in detail_octaves() {
            assert!((0.0..1.0).contains(&octave[0]));
            assert!((0.0..1.0).contains(&octave[1]));
            assert!(octave[2] > 0.0 && octave[3] > 0.0);
        }
        // The spectrum continuation: halving wavelength keeps slope equal.
        let d = detail_octaves();
        assert!((d[0][3] - d[1][3]).abs() < 1e-6);
        assert!((d[0][2] * 2.0 - d[1][2]).abs() < 1e-9);
    }

    #[test]
    fn baked_light_quantization_and_smoothstep_are_sane() {
        assert_eq!(quantize_light(0.0), 0);
        assert_eq!(quantize_light(1.0), 255);
        assert_eq!(quantize_light(-0.5), 0, "clamped below");
        assert_eq!(quantize_light(2.0), 255, "clamped above");
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    }

    #[test]
    fn open_terrain_bakes_mostly_lit_vertices() {
        // Rolling settled terrain under the 45° sun: most vertices see the
        // sun, every light byte pair is populated, and none exceeds full.
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        let lit = mesh.vertices[..CORE_VERTS]
            .iter()
            .filter(|v| v.light[0] == 255)
            .count();
        assert!(
            lit * 2 > CORE_VERTS,
            "most open-terrain vertices are sunlit, got {lit}/{CORE_VERTS}"
        );
        assert!(mesh
            .vertices
            .iter()
            .all(|v| v.light[2] == 0 && v.light[3] == 0));
    }

    #[test]
    fn cancelled_mesh_returns_none() {
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let cancelled = AtomicBool::new(true);
        assert!(mesh_region_chunk_cancellable(&snap.inputs(), &cancelled).is_none());
    }

    #[test]
    fn chunk_key_folds_the_terrain_buckets() {
        // Plan §10 check 7 (keying): same tiles + same buckets ⇒ same key;
        // a bucket flip ⇒ a new key.
        assert_eq!(
            chunk_key_from(42, &[100, 2000]),
            chunk_key_from(42, &[100, 2000])
        );
        assert_ne!(
            chunk_key_from(42, &[100, 2000]),
            chunk_key_from(42, &[101, 2000]),
            "a flipped terrain bucket must force a remesh"
        );
        assert_ne!(
            chunk_key_from(42, &[100, 2000]),
            chunk_key_from(43, &[100, 2000]),
            "a changed tile dep-hash must force a remesh"
        );
    }

    #[test]
    fn steady_state_stops_all_remesh_traffic() {
        // Plan §10 check 7 + exit criterion: travel stopped ⇒ remeshed flat.
        let map = settled_map();
        let mut manager = PovChunkManager::new();
        // Settle: with the inline executor jobs finish inside sync; the
        // amortization cap spreads integration over a few frames.
        for _ in 0..8 {
            let _ = manager.sync(&map, (0.0, 0.0), 1, &InlineExecutor);
        }
        let before = manager.counters();
        assert_eq!(before.meshed, 9, "the full radius-1 window meshed once");
        assert_eq!(before.remeshed, 0);
        let (uploads, removes) = manager.sync(&map, (0.0, 0.0), 1, &InlineExecutor);
        let after = manager.counters();
        assert!(uploads.is_empty(), "steady state must upload zero chunks");
        assert!(removes.is_empty());
        assert_eq!(after.meshed, before.meshed);
        assert_eq!(after.remeshed, 0);
    }

    #[test]
    fn integration_respects_the_uploads_per_frame_cap() {
        // Plan §10 check 7 (amortization): 9 finished meshes drain 4/frame.
        let map = settled_map();
        let mut manager = PovChunkManager::new();
        let (uploads, _) = manager.sync(&map, (0.0, 0.0), 1, &InlineExecutor);
        assert_eq!(uploads.len(), POV_UPLOADS_PER_FRAME);
        assert!(manager.counters().uploads_deferred >= 5);
        let (uploads2, _) = manager.sync(&map, (0.0, 0.0), 1, &InlineExecutor);
        assert_eq!(uploads2.len(), POV_UPLOADS_PER_FRAME);
        let (uploads3, _) = manager.sync(&map, (0.0, 0.0), 1, &InlineExecutor);
        assert_eq!(uploads3.len(), 1, "9 = 4 + 4 + 1");
    }

    #[test]
    fn stale_results_are_dropped_and_counted() {
        // Plan §10 check 7 (stale drop): a result whose key is no longer in
        // flight (superseded/cancelled) must not become a chunk.
        let mut manager = PovChunkManager::new();
        let mesh = ChunkMesh {
            vertices: vec![
                PovVertex {
                    position: [0.0; 3],
                    normal: [0.0, 0.0, 1.0],
                    color: [0; 4],
                    light: [255, 255, 0, 0],
                };
                VERTS_PER_CHUNK
            ],
            heights: vec![0.0; CORE_VERTS],
        };
        manager
            .tx
            .clone()
            .send(MeshResult {
                coord: RegionCoord::new(40, 40),
                key: 0xDEAD,
                mesh,
            })
            .expect("channel open");
        let (uploads, _) = manager.sync(
            &RegionMap::new(StreamConfig::default()),
            (0.0, 0.0),
            0,
            &InlineExecutor,
        );
        assert!(uploads.is_empty());
        assert_eq!(manager.counters().dropped_stale, 1);
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn eviction_is_farthest_first() {
        // Plan §10 check 7 (eviction): over capacity, the farthest chunks
        // from the camera are evicted first, emitting their handles.
        let mut manager = PovChunkManager::new();
        // Radius 0 ⇒ capacity 1 + POV_CHUNK_SLACK = 9. Insert 12 chunks at
        // increasing distance east of the camera.
        for x in 0..12 {
            manager.chunks.insert(
                RegionCoord::new(x, 0),
                ChunkEntry {
                    key: 1,
                    handle: 100 + x as u64,
                    heights: Vec::new(),
                },
            );
        }
        let empty = RegionMap::new(StreamConfig::default());
        let (uploads, removes) = manager.sync(&empty, (0.0, 0.0), 0, &InlineExecutor);
        assert!(uploads.is_empty());
        assert_eq!(removes, vec![111, 110, 109], "farthest handles, in order");
        assert_eq!(manager.len(), 9);
    }

    #[test]
    fn out_of_radius_jobs_are_cancelled() {
        // Plan §7.2 step 4: a region leaving the radius cancels its job; a
        // cancelled job produces nothing.
        use std::cell::RefCell;
        struct QueueExecutor {
            jobs: RefCell<Vec<Box<dyn FnOnce() + Send>>>,
        }
        impl TaskExecutor for QueueExecutor {
            fn submit(&self, _priority: TaskPriority, job: Box<dyn FnOnce() + Send>) {
                self.jobs.borrow_mut().push(job);
            }
            fn parallelism(&self) -> usize {
                1
            }
        }
        let map = settled_map();
        let executor = QueueExecutor {
            jobs: RefCell::new(Vec::new()),
        };
        let mut manager = PovChunkManager::new();
        let _ = manager.sync(&map, (0.0, 0.0), 0, &executor);
        assert_eq!(manager.in_flight.len(), 1, "center region scheduled");
        // The camera leaves; the pending job is cancelled before it runs.
        let _ = manager.sync(&map, (5000.0, 5000.0), 0, &executor);
        assert_eq!(manager.counters().cancelled, 1);
        for job in executor.jobs.take() {
            job();
        }
        let (uploads, _) = manager.sync(&map, (5000.0, 5000.0), 0, &executor);
        assert!(uploads.is_empty(), "cancelled jobs must produce nothing");
        assert_eq!(manager.counters().dropped_stale, 0);
    }
}
