//! POV mode (3d-phase-1-plan.md): the fly camera, the pure region mesher,
//! and the chunk lifecycle manager — plus the 3D-2 walk camera
//! (3d-phase-2-plan.md): [`PovChunkManager::ground_height`] rides the same
//! CPU-side height lattices the drawn chunks carry, with
//! [`analytic_ground`] as the loading-frontier fallback. Phase 3D-4 adds
//! GPU-shadow frame fitting and the upload-only organism presentation path;
//! both remain derived presentation and share the resident terrain lattice.
//!
//! **Derived presentation only (ADR 0017).** Every height the mesher emits is
//! the authoritative Terrain P/G halo through the same SIMD relief row and
//! scalar scaling tail as field generation (ADRs 0016 and 0027); every color is
//! the 2D Composite per-cell logic
//! ([`world_runtime::mapcolor::composite_cell_color`]) over the settled field tiles. The
//! baked ambient occlusion is float presentation math over the same halo
//! evaluator, edge-extended for distant probes. Direct-sun visibility is a
//! renderer-owned directional shadow map fitted here from camera-relative
//! resident bounds. Nothing here feeds back into world state, hashing, or
//! persistence.
//!
//! The mesher is a pure function of value snapshots (plan §6.1): no
//! filesystem, no threads, no GPU, no `RegionMap` — so it is unit-testable
//! and `Send`-friendly for the executor jobs. This crate is that Phase 7
//! hoist (phase-7-plan.md §9.9): the whole POV host — camera, mesher, chunk
//! manager — shared by the native shell and the browser shell, which differ
//! only in who owns the surface and who drives the executor.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use renderer::pov::{
    canonical_icosphere_geometry, skirt_core_index, CORE_VERTS, DETAIL_OCTAVES, POV_GRID,
    POV_MESH_RES, VERTS_PER_CHUNK,
};
use renderer::{
    PovFrameParams, PovOrganismInstance, PovOrganismUpload, PovVertex, TerrainChunkUpload,
};
use world_core::{mix, simd, Biome, FieldTile, PossibilityVector, RegionCoord, REGION_SIZE};
use world_runtime::mapcolor::{composite_cell_color, expressed_color, pov_sediment_color};
use world_runtime::{
    Organism, RegionMap, ResourceTier, TaskExecutor, TaskPriority, TerrainPossibilityHalo,
    CHANNEL_RIVER, CHANNEL_WETNESS,
};

/// Vertex spacing in world units (`REGION_SIZE / POV_MESH_RES` = 4.0).
const SPACING: f64 = REGION_SIZE / POV_MESH_RES as f64;

/// The fixed sun direction (normalized, pointing from the sun toward the
/// ground), shared by [`frame_params`] and [`shadow_frame`] — the shading and
/// directional shadow projection must agree on one sun. Same azimuth as
/// plan §4's original sun, elevation lowered to 20° (permanent late
/// afternoon): this world's heightfield is smooth below ~100-unit
/// wavelengths and its steepest flanks sit near 20°, so only a sun at or
/// below that angle lets ridges cast real shadows or slope shading develop
/// contrast — the definition the near-noon sun washed out entirely.
pub const SUN_DIR: [f32; 3] = [0.840_446, 0.420_223, -0.342_020_14];

/// Extra sample ring around the 65×65 vertex lattice for the
/// central-difference normals (plan §6.3).
const GRID_MARGIN: usize = 1;

/// Sample-grid edge length: the vertex lattice plus the margin rings.
const SAMPLE_GRID: usize = POV_GRID + 2 * GRID_MARGIN;

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

/// Eye height above the ground surface in walk mode (3d-phase-2-plan.md
/// §5.2; design §1.1 scale reference: person-scale in a world where
/// 1 unit ≈ 1 m).
pub const EYE_HEIGHT: f64 = 1.7;

/// Base walk speed, world units per second — a brisk run at person scale.
/// [`POV_FLY_SPEED`] (40) is survey speed, wrong for eye-level ground travel.
pub const POV_WALK_SPEED: f64 = 6.0;

/// Scroll-wheel walk-speed bounds (fly keeps [`POV_SPEED_RANGE`]).
const POV_WALK_SPEED_RANGE: (f64, f64) = (1.0, 60.0);

/// Vertical-rate clamp as a multiple of the current walk speed: the eye
/// approaches `ground + EYE_HEIGHT` at ≤ this × walk_speed, both up and
/// down, so a cliff face is climbed as a ≤ ~71° effective ramp and a ledge
/// is descended as a quick drop — never a teleport (design §4.2). Scaling
/// with walk speed keeps the feel constant under the scroll multiplier.
const POV_CLIMB_FACTOR: f64 = 3.0;

/// River intensity where the overlay ribbon begins (3d-phase-3-plan.md
/// §6.4): a core triangle joins the overlay when any corner reaches this.
/// The shader feathers alpha to zero exactly here (`pov_water.wgsl`
/// `RIVER_OVERLAY_MIN` must match), so the selection edge is invisible; the
/// feather's top (`RIVER_OVERLAY_FULL`) and the lift (`RIVER_LIFT`) are
/// shader-side constants in the same file. Raised from the plan's 0.08 after
/// measurement: the feather leaves everything below ~0.12 at alpha ≤ 0.04,
/// so selecting the broad 0.08–0.12 drainage band was invisible llvmpipe
/// fill (48% → 31% of a river-basin ring's core triangles; the wide
/// remainder is the honest field — this world's hydrology paints broad
/// 0.2–0.5 river swaths that the 2D map colors blue too).
pub const RIVER_OVERLAY_MIN: f32 = 0.12;

/// Mouse-look sensitivity, radians per raw device pixel.
const LOOK_SENSITIVITY: f32 = 0.0025;

/// Vertical field of view shared by the visible projection and CPU picking.
pub const POV_VERTICAL_FOV: f32 = 60.0_f32.to_radians();

/// Near clip distance shared by the visible projection and CPU picking.
pub const POV_NEAR_DISTANCE: f64 = 0.1;

/// Far clip distance of the POV projection. The normally shorter fog reach
/// remains the caller-supplied picking limit.
pub const POV_FAR_DISTANCE: f64 = 2048.0;

/// Pitch clamp, ±89° in radians (plan §8.2).
const PITCH_LIMIT: f32 = 89.0 * core::f32::consts::PI / 180.0;

/// Eye height above the sampled ground on POV entry (presentation-only; real
/// `ground_height` collision is 3D-2).
const ENTRY_EYE_HEIGHT: f64 = 25.0;

/// Double-precision world ray used by headless POV picking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovRay {
    /// Absolute world-space origin.
    pub origin: glam::DVec3,
    /// Normalized world-space direction.
    pub direction: glam::DVec3,
    /// Radial distances where this ray crosses the projection's view-axis
    /// near and far planes. Off-axis rays travel farther than the literal
    /// projection depths before reaching those planes.
    pub clip_distances: [f64; 2],
}

impl PovRay {
    /// Evaluate a point along the ray. Because screen rays are normalized,
    /// `distance` is also the camera-space distance used by fog.
    #[must_use]
    pub fn at(self, distance: f64) -> glam::DVec3 {
        self.origin + self.direction * distance
    }
}

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
    /// Walk mode: terrain following at eye height (3d-phase-2-plan.md).
    /// Fly mode when false. Deliberately a `bool`, not an enum — the design
    /// has exactly two modes and reserves nothing that needs a third.
    pub walk: bool,
    /// Walk speed, world units per second (scroll-adjusted, like `speed`;
    /// each mode keeps its own scroll-tuned speed).
    pub walk_speed: f64,
}

impl PovCamera {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pos: glam::DVec3::ZERO,
            yaw: core::f32::consts::FRAC_PI_2, // facing north, like the map
            pitch: 0.0,
            speed: POV_FLY_SPEED,
            walk: false,
            walk_speed: POV_WALK_SPEED,
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

    /// One scroll notch: multiply/divide the active mode's speed by 1.5,
    /// clamped to that mode's range (3d-phase-2-plan.md §5.1 — each mode
    /// keeps its own scroll-tuned speed).
    pub fn scroll_speed(&mut self, up: bool) {
        let factor = if up { 1.5 } else { 1.0 / 1.5 };
        if self.walk {
            self.walk_speed =
                (self.walk_speed * factor).clamp(POV_WALK_SPEED_RANGE.0, POV_WALK_SPEED_RANGE.1);
        } else {
            self.speed = (self.speed * factor).clamp(POV_SPEED_RANGE.0, POV_SPEED_RANGE.1);
        }
    }

    /// Enter or leave walk mode — the shared toggle path behind the live `F`
    /// key and the scripted `walk`/`fly` instructions (3d-phase-2-plan.md
    /// §5.3, §6.4). Entering snaps the eye straight to `ground + EYE_HEIGHT`
    /// (instant grounding from any fly altitude — a clamped descent from a
    /// 600-unit survey height would be a 30-second elevator); leaving keeps
    /// position and orientation unchanged. `ground` is read only when
    /// entering walk.
    pub fn set_walk(&mut self, walk: bool, ground: f64) {
        self.walk = walk;
        if walk {
            self.snap_to_ground(ground);
        }
    }

    /// Set the eye directly to `ground + EYE_HEIGHT`, no clamp — the §5.3
    /// snap cases (`F` into walk, the scripted `pos:`/walk-mode `move:`).
    pub fn snap_to_ground(&mut self, ground: f64) {
        self.pos.z = ground + EYE_HEIGHT;
    }

    /// The walk-mode follow step (3d-phase-2-plan.md §5.3): approach
    /// `target` (= ground + EYE_HEIGHT) at ≤ [`POV_CLIMB_FACTOR`] × the walk
    /// speed, both up and down. On flat and ordinary terrain the clamp never
    /// engages and following is exact; it engages only on cliff faces,
    /// frontier-fallback corrections, remesh swaps, and drift steps —
    /// precisely the cases the design wants softened into ramps.
    pub fn follow_ground(&mut self, target: f64, dt: f64) {
        let max_step = POV_CLIMB_FACTOR * self.walk_speed * dt;
        self.pos.z += (target - self.pos.z).clamp(-max_step, max_step);
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

    /// Walk-mode forward: the horizontal yaw direction, *not* [`forward`],
    /// which pitches — looking at your feet must not stop you
    /// (3d-phase-2-plan.md §5.3, design "move along ground plane").
    ///
    /// [`forward`]: Self::forward
    #[must_use]
    pub fn walk_forward(&self) -> glam::DVec3 {
        let (sy, cy) = (f64::from(self.yaw.sin()), f64::from(self.yaw.cos()));
        glam::DVec3::new(cy, sy, 0.0)
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
            POV_VERTICAL_FOV,
            aspect.max(1e-3),
            POV_NEAR_DISTANCE as f32,
            POV_FAR_DISTANCE as f32,
        );
        (proj * view).to_cols_array_2d()
    }

    /// Build the world-space ray through a physical point local to the POV
    /// pane. This is the CPU counterpart of [`Self::view_proj`]: it uses the
    /// same 60-degree vertical field of view and pane aspect, while retaining
    /// the camera's absolute `f64` origin at far world coordinates.
    ///
    /// The pane is half-open. Invalid, non-finite, or outside coordinates do
    /// not describe rendered pixels and therefore return `None`.
    #[must_use]
    pub fn screen_ray(&self, pane_point: [f64; 2], pane_size: [u32; 2]) -> Option<PovRay> {
        let [width, height] = pane_size;
        let [x, y] = pane_point;
        if width == 0
            || height == 0
            || !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(width)
            || y >= f64::from(height)
        {
            return None;
        }

        let ndc_x = 2.0 * x / f64::from(width) - 1.0;
        let ndc_y = 1.0 - 2.0 * y / f64::from(height);
        // Use the same f32 aspect and trigonometry as the projection before
        // widening to f64; this prevents a platform adapter from inventing a
        // subtly different frustum.
        let aspect = (width as f32 / height as f32).max(1.0e-3);
        let half_height = f64::from((POV_VERTICAL_FOV * 0.5).tan());
        let forward = self.forward();
        let right = self.right();
        let up = right.cross(forward).normalize();
        let direction = (forward
            + right * (ndc_x * f64::from(aspect) * half_height)
            + up * (ndc_y * half_height))
            .normalize();
        let view_depth_per_distance = direction.dot(forward);
        Some(PovRay {
            origin: self.pos,
            direction,
            clip_distances: [
                POV_NEAR_DISTANCE / view_depth_per_distance,
                POV_FAR_DISTANCE / view_depth_per_distance,
            ],
        })
    }
}

impl Default for PovCamera {
    fn default() -> Self {
        Self::new()
    }
}

/// Live POV feature toggles — presentation-only diagnostic switches for
/// chasing llvmpipe (software rasterizer) CPU cost, flipped by POV-mode keys
/// and defaulted all-on. `shadow_ao` and `detail_normals` gate shader
/// terms; `water` skips the sea/overlay draws entirely. `shadow_ao` skips the
/// continuous GPU depth pass and neutralizes retained AO without remeshing;
/// `detail_normals` (per-fragment 64-bit hashing) and `water` (a blended
/// near-fullscreen pass) remain independent per-frame diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct PovToggles {
    /// `B`: GPU directional shadows and CPU-baked ambient occlusion.
    pub shadow_ao: bool,
    /// `N`: per-fragment detail normals (the continued terrain spectrum).
    pub detail_normals: bool,
    /// `V`: the sea plane and river overlays.
    pub water: bool,
}

impl Default for PovToggles {
    fn default() -> Self {
        Self {
            shadow_ao: true,
            detail_normals: true,
            water: true,
        }
    }
}

/// The full fog reach shared by shader parameters and organism distance
/// culling (3d-phase-4-plan.md §7.3).
#[inline]
#[must_use]
pub fn pov_fog_end(radius: i32) -> f64 {
    0.95 * (f64::from(radius) + 0.5) * REGION_SIZE
}

/// Initial directional-shadow map edge selected by the resource tier.
#[inline]
#[must_use]
pub const fn shadow_resolution(tier: ResourceTier) -> u32 {
    match tier {
        ResourceTier::Low => 1024,
        ResourceTier::Mid | ResourceTier::High => 2048,
    }
}

/// World-space axis-aligned bounds used only to conservatively fit the
/// camera-relative directional shadow map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovShadowBounds {
    /// Inclusive minimum world coordinate.
    pub min: [f64; 3],
    /// Inclusive maximum world coordinate.
    pub max: [f64; 3],
}

impl PovShadowBounds {
    /// One finite point, useful when incrementally accumulating a bound.
    #[must_use]
    pub const fn point(point: [f64; 3]) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// Union two bounds. Callers construct bounds only from finite terrain
    /// and organism presentation values.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let mut out = self;
        for axis in 0..3 {
            out.min[axis] = out.min[axis].min(other.min[axis]);
            out.max[axis] = out.max[axis].max(other.max[axis]);
        }
        out
    }

    fn corners(self) -> impl Iterator<Item = [f64; 3]> {
        (0u8..8).map(move |mask| {
            [
                if mask & 1 == 0 {
                    self.min[0]
                } else {
                    self.max[0]
                },
                if mask & 2 == 0 {
                    self.min[1]
                } else {
                    self.max[1]
                },
                if mask & 4 == 0 {
                    self.min[2]
                } else {
                    self.max[2]
                },
            ]
        })
    }
}

/// Stabilized directional-shadow inputs for one POV frame. An empty resident
/// bound yields `resolution == 0` and an identity matrix, a safe disabled
/// state understood by the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovShadowFrame {
    /// Column-major projection consuming camera-relative positions.
    pub light_view_proj: [[f32; 4]; 4],
    /// Depth-map edge length; zero safely disables the shadow path.
    pub resolution: u32,
}

impl PovShadowFrame {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            light_view_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
            resolution: 0,
        }
    }
}

/// Padding around projected casters/receivers. X/Y padding gives PCF and a
/// newly integrated edge chunk room; depth padding is deliberately fixed so
/// near/far behavior does not depend on local relief amplitude.
const SHADOW_XY_PADDING: f64 = REGION_SIZE * 0.25;
const SHADOW_DEPTH_PADDING: f64 = REGION_SIZE;

/// Fit one camera-relative directional shadow matrix to the resident terrain
/// and current organism bounds. The same helper is used by live POV, F12,
/// scripted capture, and the browser host; only callers decide whether they
/// actually submit organisms.
#[must_use]
pub fn shadow_frame(
    camera: &PovCamera,
    chunks: &PovChunkManager,
    organism_bounds: Option<PovShadowBounds>,
    resolution: u32,
) -> PovShadowFrame {
    if resolution == 0 {
        return PovShadowFrame::disabled();
    }
    let bounds = match (chunks.shadow_bounds(), organism_bounds) {
        (Some(terrain), Some(organisms)) => Some(terrain.union(organisms)),
        (terrain @ Some(_), None) => terrain,
        (None, organisms @ Some(_)) => organisms,
        (None, None) => None,
    };
    let Some(bounds) = bounds else {
        return PovShadowFrame::disabled();
    };
    PovShadowFrame {
        light_view_proj: fit_shadow_matrix(camera.pos, bounds, resolution),
        resolution,
    }
}

fn fit_shadow_matrix(
    camera: glam::DVec3,
    bounds: PovShadowBounds,
    resolution: u32,
) -> [[f32; 4]; 4] {
    let forward = glam::DVec3::new(
        f64::from(SUN_DIR[0]),
        f64::from(SUN_DIR[1]),
        f64::from(SUN_DIR[2]),
    )
    .normalize();
    let right = glam::DVec3::Z.cross(forward).normalize();
    // The screen basis must face back toward the sun: `right × up =
    // -forward`. Terrain's CCW +z-facing triangles then remain CCW from the
    // light, matching the shadow pipelines' back-face culling. Using
    // `forward × right` here mirrors projected winding and culls the
    // sun-facing terrain core instead.
    let up = right.cross(forward).normalize();

    let mut min = glam::DVec3::splat(f64::INFINITY);
    let mut max = glam::DVec3::splat(f64::NEG_INFINITY);
    for corner in bounds.corners() {
        let relative = glam::DVec3::from_array(corner) - camera;
        let light = glam::DVec3::new(relative.dot(right), relative.dot(up), relative.dot(forward));
        min = min.min(light);
        max = max.max(light);
    }

    let rounded_extent = |span: f64| {
        ((span + 2.0 * SHADOW_XY_PADDING) / REGION_SIZE)
            .ceil()
            .max(1.0)
            * REGION_SIZE
    };
    let extent_x = rounded_extent(max.x - min.x);
    let extent_y = rounded_extent(max.y - min.y);
    let texel_x = extent_x / f64::from(resolution);
    let texel_y = extent_y / f64::from(resolution);
    let center_x = (((min.x + max.x) * 0.5) / texel_x).round() * texel_x;
    let center_y = (((min.y + max.y) * 0.5) / texel_y).round() * texel_y;
    let near = min.z - SHADOW_DEPTH_PADDING;
    let far = max.z + SHADOW_DEPTH_PADDING;
    let depth = (far - near).max(1.0);

    // Column-major matrix. Input positions are already camera-relative in
    // every POV shader; no large absolute f32 coordinate enters this fit.
    let sx = 2.0 / extent_x;
    let sy = 2.0 / extent_y;
    let sz = 1.0 / depth;
    [
        [
            (right.x * sx) as f32,
            (up.x * sy) as f32,
            (forward.x * sz) as f32,
            0.0,
        ],
        [
            (right.y * sx) as f32,
            (up.y * sy) as f32,
            (forward.y * sz) as f32,
            0.0,
        ],
        [
            (right.z * sx) as f32,
            (up.z * sy) as f32,
            (forward.z * sz) as f32,
            0.0,
        ],
        [
            (-center_x * sx) as f32,
            (-center_y * sy) as f32,
            (-near * sz) as f32,
            1.0,
        ],
    ]
}

/// The per-frame renderer parameters for the camera at `radius` regions
/// (plan §4): fog from `0.55·R` to `0.95·R` with `R = (radius + 0.5) ·
/// REGION_SIZE`, fog color = the clear color so geometry dissolves into sky,
/// the fixed sun ([`SUN_DIR`]) and hemisphere ambients tuned so flat ground
/// roughly matches the 2D palette's value range.
///
/// `time` is the water-wobble clock in seconds, already wrapped by the
/// caller at `renderer::pov::WOBBLE_PERIOD` (3d-phase-3-plan.md §7.1);
/// captures pass `0.0` so snapshots stay reproducible. The sea plane's
/// camera-relative height is computed here — the shell owns `SEA_LEVEL`,
/// the renderer stays world-agnostic (plan §4.1).
#[must_use]
pub fn frame_params(
    camera: &PovCamera,
    aspect: f32,
    radius: i32,
    clear: [f64; 4],
    time: f32,
    toggles: PovToggles,
    shadow: PovShadowFrame,
) -> PovFrameParams {
    let reach = (f64::from(radius) + 0.5) * REGION_SIZE;
    let sun = glam::Vec3::from_array(SUN_DIR);
    PovFrameParams {
        view_proj: camera.view_proj(aspect),
        light_view_proj: shadow.light_view_proj,
        camera_pos: [camera.pos.x, camera.pos.y, camera.pos.z],
        sun_dir: [sun.x, sun.y, sun.z],
        detail: detail_octaves(),
        time,
        water_z: (f64::from(world_core::SEA_LEVEL) - camera.pos.z) as f32,
        shadow_resolution: shadow.resolution,
        shadow_ao: toggles.shadow_ao,
        detail_normals: toggles.detail_normals,
        water: toggles.water,
        fog_color: [clear[0] as f32, clear[1] as f32, clear[2] as f32],
        fog_start: (0.55 * reach) as f32,
        fog_end: pov_fog_end(radius) as f32,
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

/// The authoritative halo-sampled terrain height under a world position —
/// the mesh-free fallback for walk mode at the loading frontier
/// (3d-phase-2-plan.md §4.4), and the shared core of [`entry_ground`]:
/// halo-sampled `PossibilityVector` when the covering region is resident
/// (neutral otherwise), through `world_core::elevation`. Deliberately
/// unclamped — walking the sea floor is allowed (design §4.2).
/// Presentation-only; never an identity.
#[must_use]
pub fn analytic_ground(map: &RegionMap, world: (f64, f64)) -> f64 {
    let coord = RegionCoord::from_world(world.0, world.1);
    let p = map
        .terrain_possibility_halo(coord)
        .map_or_else(PossibilityVector::neutral, |halo| {
            halo.sample_world(world.0, world.1)
        });
    f64::from(world_core::elevation(world.0, world.1, &p))
}

/// The terrain height under a world position for POV-entry camera placement:
/// [`analytic_ground`] floored at sea level (entry hovers over water, it
/// does not dive). Presentation-only camera placement — never an identity.
#[must_use]
pub fn entry_ground(map: &RegionMap, world: (f64, f64)) -> f64 {
    analytic_ground(map, world).max(f64::from(world_core::SEA_LEVEL))
}

/// The ground walk mode stands on (3d-phase-2-plan.md §4.4): the rendered
/// mesh where the covering region's chunk is resident
/// ([`PovChunkManager::ground_height`]), else the analytic fallback at the
/// loading frontier — correct to within one mesh cell, and transient.
/// Returns the height and whether it came from the mesh (the telemetry/dump
/// observable for the frontier-fallback exit criterion).
#[must_use]
pub fn walk_ground(chunks: &PovChunkManager, map: &RegionMap, world: (f64, f64)) -> (f64, bool) {
    match chunks.ground_height(world.0, world.1) {
        Some(h) => (f64::from(h), true),
        None => (analytic_ground(map, world), false),
    }
}

// ---------------------------------------------------------------------------
// Organism presentation (3d-phase-4-plan.md §5, §7)
// ---------------------------------------------------------------------------

/// The two canonical instanced primitive batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PovOrganismPrimitive {
    /// Flat-shaded unit cube.
    Box,
    /// Smooth two-subdivision unit icosphere.
    Sphere,
}

/// Pure renderer-ready organism mapping before it is partitioned into an
/// upload batch. No map, chunk, or GPU reference crosses this boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovOrganismVisual {
    /// Canonical mesh batch.
    pub primitive: PovOrganismPrimitive,
    /// Absolute body center in world space.
    pub position: [f64; 3],
    /// Non-uniform world-space scale.
    pub scale: [f32; 3],
    /// Static rotation around world +z.
    pub yaw: f32,
    /// Shared expressed RGB plus producer flag.
    pub color: [u8; 4],
    /// AO sampled under the body's center.
    pub ambient_occlusion: u8,
    /// Optional activity amplitude/phase; static Phase 3D-4 uses zero.
    pub bob: [f32; 2],
}

impl PovOrganismVisual {
    fn instance(self) -> PovOrganismInstance {
        PovOrganismInstance {
            position: self.position,
            scale: self.scale,
            yaw: self.yaw,
            color: self.color,
            ambient_occlusion: self.ambient_occlusion,
            bob: self.bob,
        }
    }

    fn bounding_radius(self) -> f64 {
        let [x, y, z] = self.scale.map(f64::from);
        0.5 * (x * x + y * y + z * z).sqrt()
    }

    fn shadow_bounds(self) -> PovShadowBounds {
        let radius = self.bounding_radius();
        let half_z = 0.5 * f64::from(self.scale[2]);
        PovShadowBounds {
            min: [
                self.position[0] - radius,
                self.position[1] - radius,
                self.position[2] - half_z,
            ],
            max: [
                self.position[0] + radius,
                self.position[1] + radius,
                self.position[2] + half_z,
            ],
        }
    }

    /// Vertical activity offset using the exact expression evaluated by the
    /// organism WGSL vertex path. Phase 3D-4 visuals currently set amplitude
    /// to zero, but keeping the transform here prevents future picking drift.
    fn bob_offset(self, frame_time: f32) -> f64 {
        let [amplitude, phase] = self.bob;
        f64::from(
            amplitude * (0.5 + 0.5 * (core::f32::consts::TAU * (frame_time * 0.25 + phase)).sin()),
        )
    }

    fn local_ray(self, ray: PovRay, frame_time: f32) -> Option<PovRay> {
        if self
            .scale
            .iter()
            .any(|scale| *scale <= 0.0 || !scale.is_finite())
        {
            return None;
        }
        let center = glam::DVec3::new(
            self.position[0],
            self.position[1],
            self.position[2] + self.bob_offset(frame_time),
        );
        let origin = ray.origin - center;
        let (sin, cos) = self.yaw.sin_cos();
        let (sin, cos) = (f64::from(sin), f64::from(cos));
        let inverse_rotate = |value: glam::DVec3| {
            glam::DVec3::new(
                cos * value.x + sin * value.y,
                -sin * value.x + cos * value.y,
                value.z,
            )
        };
        let scale = glam::DVec3::new(
            f64::from(self.scale[0]),
            f64::from(self.scale[1]),
            f64::from(self.scale[2]),
        );
        Some(PovRay {
            origin: inverse_rotate(origin) / scale,
            // Do not normalize after inverse scale: its parameter remains the
            // distance along the normalized world ray.
            direction: inverse_rotate(ray.direction) / scale,
            clip_distances: ray.clip_distances,
        })
    }
}

/// Fixed, approximately volume-preserving form proportions. Form bit zero
/// selects the primitive; bits 1–3 index this table.
const ORGANISM_PROPORTIONS: [[f32; 2]; 8] = [
    [1.42, 0.50],
    [1.26, 0.63],
    [1.13, 0.79],
    [1.00, 1.00],
    [0.89, 1.25],
    [0.82, 1.48],
    [0.76, 1.74],
    [0.69, 2.08],
];

/// Packed renderer instance size pinned by the renderer's layout tests.
pub const POV_ORGANISM_INSTANCE_BYTES: u64 = 64;

fn organism_scale(organism: &Organism) -> [f32; 3] {
    let shape = usize::from((organism.expressed.form >> 1) & 7);
    let [xy, z] = ORGANISM_PROPORTIONS[shape];
    let size = organism.expressed.size;
    [size * xy, size * xy, size * z]
}

/// Map one realized organism and its resident ground sample to renderer-ready
/// geometry. This is table-driven and has no random or time-varying input.
#[must_use]
pub fn organism_visual(organism: &Organism, ground: GroundSurface) -> PovOrganismVisual {
    let scale = organism_scale(organism);
    let rgb = expressed_color(&organism.expressed);
    PovOrganismVisual {
        primitive: if organism.expressed.form & 1 == 0 {
            PovOrganismPrimitive::Box
        } else {
            PovOrganismPrimitive::Sphere
        },
        position: [
            organism.world_pos.0,
            organism.world_pos.1,
            f64::from(ground.height) + 0.5 * f64::from(scale[2]),
        ],
        scale,
        yaw: core::f32::consts::TAU * ((organism.id & 0xffff) as f32 / 65_536.0),
        color: [
            rgb[0],
            rgb[1],
            rgb[2],
            u8::from(organism.trophic == world_core::Trophic::Producer),
        ],
        ambient_occlusion: ground.ambient_occlusion,
        bob: [0.0; 2],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrganismVisualKey {
    id: u64,
    slot: u16,
    species: u64,
    trophic: world_core::Trophic,
    cell: world_core::LocalPos,
    expressed: [u32; 5],
    form: u8,
    position: [u64; 3],
    scale: [u32; 3],
    yaw: u32,
    color: [u8; 4],
    ambient_occlusion: u8,
    bob: [u32; 2],
}

impl OrganismVisualKey {
    fn new(organism: &Organism, visual: PovOrganismVisual) -> Self {
        Self {
            id: organism.id,
            slot: organism.slot,
            species: organism.species,
            trophic: organism.trophic,
            cell: organism.cell,
            expressed: [
                organism.expressed.hue.to_bits(),
                organism.expressed.luminance.to_bits(),
                organism.expressed.size.to_bits(),
                organism.expressed.activity.to_bits(),
                organism.expressed.aggression.to_bits(),
            ],
            form: organism.expressed.form,
            position: visual.position.map(f64::to_bits),
            scale: visual.scale.map(f32::to_bits),
            yaw: visual.yaw.to_bits(),
            color: visual.color,
            ambient_occlusion: visual.ambient_occlusion,
            bob: visual.bob.map(f32::to_bits),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OrganismScratch {
    key: OrganismVisualKey,
    organism: Organism,
    visual: PovOrganismVisual,
}

/// Closest visible realized-organism intersection for a POV ray. The copied
/// runtime source is the exact identity and expressed data that produced the
/// retained renderer visual; semantic inspection never performs a stale
/// secondary lookup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovOrganismHit {
    /// Distance from the camera along the normalized world ray.
    pub distance: f64,
    /// Source organism paired with the intersected renderer visual.
    pub organism: Organism,
}

/// Shell-side organism presentation telemetry. Counts describe the latest
/// scan; rebuild/upload totals are cumulative and observational only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PovOrganismCounters {
    /// Organisms published by the runtime before presentation filters.
    pub published: usize,
    /// Eligible box instances.
    pub boxes: usize,
    /// Eligible sphere instances.
    pub spheres: usize,
    /// In-range organisms omitted until their terrain chunk is resident.
    pub waiting_for_ground: usize,
    /// Organisms wholly beyond fog plus their conservative body radius.
    pub distance_culled: usize,
    /// Exact-list replacements since manager creation.
    pub rebuilds: u64,
    /// Instances included in all replacement uploads.
    pub uploaded_instances: u64,
    /// Packed 64-byte instance traffic across all replacements.
    pub uploaded_bytes: u64,
}

impl PovOrganismCounters {
    #[must_use]
    pub const fn drawn(self) -> usize {
        self.boxes + self.spheres
    }
}

/// Exact, reusable CPU lifecycle for POV organism replacement uploads.
/// Camera rotation is deliberately absent; translation only matters when an
/// organism crosses the fog/body-radius membership boundary.
#[derive(Debug, Default)]
pub struct PovOrganismManager {
    upload: PovOrganismUpload,
    box_keys: Vec<OrganismVisualKey>,
    sphere_keys: Vec<OrganismVisualKey>,
    box_scratch: Vec<OrganismScratch>,
    sphere_scratch: Vec<OrganismScratch>,
    counters: PovOrganismCounters,
    shadow_bounds: Option<PovShadowBounds>,
    initialized: bool,
    visual_generation: u64,
}

impl PovOrganismManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn counters(&self) -> PovOrganismCounters {
        self.counters
    }

    /// Dirty generation of the exact visual/source lists used for GPU
    /// uploads and CPU picking.
    #[must_use]
    pub const fn visual_generation(&self) -> u64 {
        self.visual_generation
    }

    /// Whether the retained list contains a time-varying bob transform.
    /// Static lists let hover caches ignore frame time entirely.
    #[must_use]
    pub fn has_animated_visuals(&self) -> bool {
        self.box_scratch
            .iter()
            .chain(&self.sphere_scratch)
            .any(|entry| entry.visual.bob[0] != 0.0)
    }

    /// Current complete replacement lists. Call only when [`Self::sync`]
    /// returned true; otherwise the renderer should retain its buffers.
    #[must_use]
    pub const fn upload(&self) -> &PovOrganismUpload {
        &self.upload
    }

    /// Bounds of the organisms selected by the latest scan.
    #[must_use]
    pub const fn shadow_bounds(&self) -> Option<PovShadowBounds> {
        self.shadow_bounds
    }

    /// Intersect the exact retained organism visuals, including yaw,
    /// non-uniform scale, and frame-time bob. Boxes use their oriented slabs;
    /// spheres use the scaled ellipsoid only as a broad phase before testing
    /// the renderer's canonical two-subdivision icosphere triangles.
    #[must_use]
    pub fn raycast(
        &self,
        ray: PovRay,
        frame_time: f32,
        max_distance: f64,
    ) -> Option<PovOrganismHit> {
        let [near_distance, far_distance] = ray.clip_distances;
        let limit = max_distance.min(far_distance);
        if !ray.origin.is_finite()
            || !ray.direction.is_finite()
            || !frame_time.is_finite()
            || !near_distance.is_finite()
            || !far_distance.is_finite()
            || near_distance < 0.0
            || limit <= near_distance
            || (ray.direction.length_squared() - 1.0).abs() > 1.0e-9
        {
            return None;
        }

        let mut best: Option<PovOrganismHit> = None;
        for entry in self.box_scratch.iter().chain(&self.sphere_scratch) {
            let upper = best.map_or(limit, |hit| hit.distance.min(limit));
            let Some(local_ray) = entry.visual.local_ray(ray, frame_time) else {
                continue;
            };
            let distance = match entry.visual.primitive {
                PovOrganismPrimitive::Box => ray_box_interval(
                    local_ray,
                    glam::DVec3::splat(-0.5),
                    glam::DVec3::splat(0.5),
                    f64::NEG_INFINITY,
                    upper,
                )
                .map(|(enter, _)| enter)
                .filter(|distance| *distance >= near_distance),
                PovOrganismPrimitive::Sphere => {
                    raycast_canonical_icosphere(local_ray, near_distance, upper)
                }
            };
            if let Some(distance) = distance {
                if distance < upper {
                    best = Some(PovOrganismHit {
                        distance,
                        organism: entry.organism,
                    });
                }
            }
        }
        best.filter(|hit| hit.distance < limit)
    }

    /// Scan the published realization, distance/ground filter it, build exact
    /// visual keys in stable `(id, slot)` order, and return whether the
    /// renderer needs a full replacement upload.
    pub fn sync(
        &mut self,
        map: &RegionMap,
        chunks: &PovChunkManager,
        camera: (f64, f64),
        fog_end: f64,
    ) -> bool {
        self.sync_organisms(map.organisms(), chunks, camera, fog_end)
    }

    fn sync_organisms<'a>(
        &mut self,
        organisms: impl Iterator<Item = &'a Organism>,
        chunks: &PovChunkManager,
        camera: (f64, f64),
        fog_end: f64,
    ) -> bool {
        self.box_scratch.clear();
        self.sphere_scratch.clear();
        self.shadow_bounds = None;
        self.counters.published = 0;
        self.counters.boxes = 0;
        self.counters.spheres = 0;
        self.counters.waiting_for_ground = 0;
        self.counters.distance_culled = 0;

        for organism in organisms {
            self.counters.published += 1;
            let scale = organism_scale(organism);
            let [x, y, z] = scale.map(f64::from);
            let body_radius = 0.5 * (x * x + y * y + z * z).sqrt();
            let reach = fog_end + body_radius;
            let dx = organism.world_pos.0 - camera.0;
            let dy = organism.world_pos.1 - camera.1;
            if dx * dx + dy * dy > reach * reach {
                self.counters.distance_culled += 1;
                continue;
            }
            let Some(ground) = chunks.ground_surface(organism.world_pos.0, organism.world_pos.1)
            else {
                self.counters.waiting_for_ground += 1;
                continue;
            };
            let visual = organism_visual(organism, ground);
            let entry = OrganismScratch {
                key: OrganismVisualKey::new(organism, visual),
                organism: *organism,
                visual,
            };
            match visual.primitive {
                PovOrganismPrimitive::Box => self.box_scratch.push(entry),
                PovOrganismPrimitive::Sphere => self.sphere_scratch.push(entry),
            }
            let bounds = visual.shadow_bounds();
            self.shadow_bounds = Some(
                self.shadow_bounds
                    .map_or(bounds, |current| current.union(bounds)),
            );
        }

        let order = |entry: &OrganismScratch| (entry.key.id, entry.key.slot);
        self.box_scratch.sort_unstable_by_key(order);
        self.sphere_scratch.sort_unstable_by_key(order);
        self.counters.boxes = self.box_scratch.len();
        self.counters.spheres = self.sphere_scratch.len();

        let boxes_same = self.box_keys.len() == self.box_scratch.len()
            && self
                .box_keys
                .iter()
                .zip(&self.box_scratch)
                .all(|(key, entry)| *key == entry.key);
        let spheres_same = self.sphere_keys.len() == self.sphere_scratch.len()
            && self
                .sphere_keys
                .iter()
                .zip(&self.sphere_scratch)
                .all(|(key, entry)| *key == entry.key);
        let changed = !self.initialized || !boxes_same || !spheres_same;
        self.initialized = true;
        if !changed {
            return false;
        }

        self.box_keys.clear();
        self.box_keys
            .extend(self.box_scratch.iter().map(|entry| entry.key));
        self.sphere_keys.clear();
        self.sphere_keys
            .extend(self.sphere_scratch.iter().map(|entry| entry.key));
        self.upload.boxes.clear();
        self.upload
            .boxes
            .extend(self.box_scratch.iter().map(|entry| entry.visual.instance()));
        self.upload.spheres.clear();
        self.upload.spheres.extend(
            self.sphere_scratch
                .iter()
                .map(|entry| entry.visual.instance()),
        );
        let count = self.counters.drawn() as u64;
        self.counters.rebuilds += 1;
        self.visual_generation = self.visual_generation.wrapping_add(1);
        self.counters.uploaded_instances += count;
        self.counters.uploaded_bytes += count * POV_ORGANISM_INSTANCE_BYTES;
        true
    }
}

/// Nearest CPU-side geometry under a POV pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PovSceneHit {
    /// Exact resident core terrain triangle.
    Terrain(PovTerrainHit),
    /// Exact renderer-ready organism primitive.
    Organism(PovOrganismHit),
}

impl PovSceneHit {
    /// Camera distance used for depth ordering and fog rejection.
    #[must_use]
    pub const fn distance(self) -> f64 {
        match self {
            Self::Terrain(hit) => hit.distance,
            Self::Organism(hit) => hit.distance,
        }
    }
}

/// Compare resident terrain and organism intersections with the renderer's
/// opaque depth rule. A strictly nearer body wins; an equal-depth terrain hit
/// remains visible because organisms draw later with a `Less` comparison.
#[must_use]
pub fn raycast_scene(
    chunks: &PovChunkManager,
    organisms: &PovOrganismManager,
    ray: PovRay,
    frame_time: f32,
    max_distance: f64,
) -> Option<PovSceneHit> {
    let terrain = chunks.raycast(ray, max_distance);
    let organism = organisms.raycast(ray, frame_time, max_distance);
    match (terrain, organism) {
        (Some(terrain), Some(organism)) if organism.distance < terrain.distance => {
            Some(PovSceneHit::Organism(organism))
        }
        (Some(terrain), _) => Some(PovSceneHit::Terrain(terrain)),
        (None, Some(organism)) => Some(PovSceneHit::Organism(organism)),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// The mesher (plan §6): pure function of value snapshots
// ---------------------------------------------------------------------------

/// Value snapshot a mesh job carries (plan §6.1). The tiles arrive as `Arc`
/// clones held by the job; `terrain_halo` is the exact owned snapshot the
/// Terrain generator hashes and samples, so mesh heights agree at field cell
/// centers and remain continuous at ordinary borders (ADR 0027).
#[derive(Debug)]
pub struct ChunkMeshInputs<'a> {
    pub coord: RegionCoord,
    pub terrain_halo: TerrainPossibilityHalo,
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

/// A meshed chunk: GPU vertices plus the compact CPU-side height/AO lattice
/// and extrema used by grounding and directional-shadow fitting.
#[derive(Debug)]
pub struct ChunkMesh {
    /// Exactly [`VERTS_PER_CHUNK`] vertices in the shared topology's order.
    pub vertices: Vec<PovVertex>,
    /// 65×65 core vertex heights, row-major (`j * POV_GRID + i`).
    pub heights: Vec<f32>,
    /// Ambient-occlusion bytes for the same 65×65 core lattice. Keeping the
    /// quantized attribute beside [`Self::heights`] lets organism grounding
    /// sample exactly the presentation field drawn by the terrain shader.
    pub ambient_occlusion: Vec<u8>,
    /// Minimum core height, retained for directional-shadow fitting.
    pub min_height: f32,
    /// Maximum core height, retained for directional-shadow fitting.
    pub max_height: f32,
    /// River-overlay triangles (3d-phase-3-plan.md §6.1): index triples into
    /// `vertices`, a subset of the shared core topology (same order, same
    /// diagonal split, same winding) selected where any corner's river
    /// intensity reaches [`RIVER_OVERLAY_MIN`] and at least one corner is at
    /// or above sea level (fully submerged cells are already under the sea
    /// plane). Empty for most chunks.
    pub river_indices: Vec<u32>,
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

/// Evaluate a presentation height row from an owned Terrain halo. Relief is
/// always sampled at the requested world position. Only the P/G lookup is
/// clamped to the outer region-center rectangle when an AO probe
/// reaches beyond Terrain's authoritative 3×3 simulation halo; this
/// constant edge extension keeps distant presentation work bounded by the
/// same provenance key without changing core or ghost samples (ADR 0027).
fn halo_elevation_row(xs: &[f64], y: f64, halo: &TerrainPossibilityHalo, out: &mut [f32]) {
    assert_eq!(xs.len(), out.len());
    simd::fbm_row(xs, y, out);
    let (ox, oy) = halo.center().origin();
    let min_x = ox - REGION_SIZE * 0.5;
    let max_x = ox + REGION_SIZE * 1.5;
    let min_y = oy - REGION_SIZE * 0.5;
    let max_y = oy + REGION_SIZE * 1.5;
    let sample_y = y.clamp(min_y, max_y);
    for (x, value) in xs.iter().zip(out) {
        let p = halo.sample_world(x.clamp(min_x, max_x), sample_y);
        *value = world_core::terrain::elevation_from_relief(*value, &p);
    }
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
        let row = &mut h[g * S..(g + 1) * S];
        halo_elevation_row(&xs, y, &inputs.terrain_halo, row);
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }

    // Coarse height lattice for valley-scale AO: 25×25 at
    // 32-unit spacing spanning [origin − 256, origin + 512].
    let xsc: Vec<f64> = (0..COARSE_GRID)
        .map(|g| ox + (g as f64 - COARSE_MARGIN as f64) * COARSE_SPACING)
        .collect();
    let mut hc = vec![0f32; COARSE_GRID * COARSE_GRID];
    for g in 0..COARSE_GRID {
        let y = oy + (g as f64 - COARSE_MARGIN as f64) * COARSE_SPACING;
        halo_elevation_row(
            &xsc,
            y,
            &inputs.terrain_halo,
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
    let mut ambient_occlusion = Vec::with_capacity(CORE_VERTS);
    let mut rivers = Vec::with_capacity(CORE_VERTS);
    let mut min_height = f32::INFINITY;
    let mut max_height = f32::NEG_INFINITY;
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
            let submerged = e < world_core::SEA_LEVEL;
            let ao = quantize_light(vertex_ao(&occlusion, lx, ly));
            // Land matches the 2D Composite exactly (3d-phase-1-plan.md
            // §6.4); the sea floor gets a real sediment ramp instead of the
            // map's blue depth legend, so the ocean reads as water over sand
            // rather than blue terrain (`pov_sediment_color`).
            let rgb = if submerged {
                pov_sediment_color(e)
            } else {
                composite_cell_color(e, biome, river, wetness, (id != 0).then_some(id))
            };
            // The zw bytes carry river/wetness for the 3D-3 wet material and
            // overlay feather (3d-phase-3-plan.md §5.1) — zeroed underwater:
            // the sea surface owns the specular there, and a submerged wet
            // glint just blows out through the translucent plane.
            vertices.push(PovVertex {
                position: [lx as f32, ly as f32, e],
                normal,
                color: [rgb[0], rgb[1], rgb[2], 255],
                light: [
                    255, // reserved/neutral: direct visibility is GPU-shadowed
                    ao,
                    if submerged { 0 } else { quantize_light(river) },
                    if submerged {
                        0
                    } else {
                        quantize_light(wetness)
                    },
                ],
            });
            heights.push(e);
            ambient_occlusion.push(ao);
            rivers.push(river);
            min_height = min_height.min(e);
            max_height = max_height.max(e);
        }
    }
    // The skirt bottom ring (plan §6.5): same (x, y), normal, color, and
    // packed material/AO attributes as the perimeter vertex above — the skirt
    // reads as the terrain continuing, not as a wall — z lowered by one grid
    // step.
    for edge in 0..4 {
        for k in 0..POV_GRID {
            let mut v = vertices[skirt_core_index(edge, k)];
            v.position[2] -= POV_SKIRT_DROP;
            vertices.push(v);
        }
    }
    debug_assert_eq!(vertices.len(), VERTS_PER_CHUNK);
    let river_indices = river_overlay_indices(&rivers, &heights);
    Some(ChunkMesh {
        vertices,
        heights,
        ambient_occlusion,
        min_height,
        max_height,
        river_indices,
    })
}

/// The river-overlay triangle selection (3d-phase-3-plan.md §6.1), walking
/// the same quad loop and v00→v11 diagonal split as
/// `renderer::pov::chunk_indices` so every emitted triple is a core triangle
/// of the drawn topology. Pure and deterministic, like the mesher it serves.
fn river_overlay_indices(rivers: &[f32], heights: &[f32]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut tri = |a: usize, b: usize, c: usize| {
        let river = rivers[a].max(rivers[b]).max(rivers[c]);
        let e = heights[a].max(heights[b]).max(heights[c]);
        if river >= RIVER_OVERLAY_MIN && e >= world_core::SEA_LEVEL {
            out.extend_from_slice(&[a as u32, b as u32, c as u32]);
        }
    };
    for j in 0..POV_MESH_RES {
        for i in 0..POV_MESH_RES {
            let v00 = j * POV_GRID + i;
            let v10 = v00 + 1;
            let v01 = v00 + POV_GRID;
            let v11 = v01 + 1;
            tri(v00, v10, v11);
            tri(v00, v11, v01);
        }
    }
    out
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

/// The coherently captured presentation provenance for one chunk. The key
/// folds both the atlas dependency-hash key of the region's current tiles and
/// every bucket of the owned Terrain halo used to generate mesh heights. A
/// neighbor-source change therefore supersedes old mesh work immediately,
/// even before corrected Terrain and its consumers integrate (ADR 0027).
#[derive(Debug)]
struct ChunkProvenance {
    key: u64,
    terrain_halo: TerrainPossibilityHalo,
}

/// Capture one chunk's key and Terrain halo together (plan §7.1). Steady
/// state: same tiles and same halo ⇒ same key ⇒ zero remesh traffic — exact by
/// the same argument that makes atlas upload-skipping exact (ADR 0008).
///
/// `None` until the tiles the mesher needs are present; holes at the loading
/// frontier are acceptable in 3D-1 and hide in fog (plan §7.1).
fn chunk_provenance(map: &RegionMap, coord: RegionCoord) -> Option<ChunkProvenance> {
    let tiles = map.cache().get(coord)?;
    tiles.channels[CHANNEL_RIVER].as_ref()?;
    tiles.channels[CHANNEL_WETNESS].as_ref()?;
    tiles.biome.as_ref()?;
    tiles.dominant.as_ref()?;
    let terrain_halo = map.terrain_possibility_halo(coord)?;
    let mut key = mix(map.presentation_key(coord)?, 0x504F_565F_4841_4C4F);
    for bucket in terrain_halo.dependency_buckets() {
        key = mix(key, u64::from(bucket));
    }
    Some(ChunkProvenance { key, terrain_halo })
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

/// A resident chunk: provenance, renderer handle, and the CPU-side attributes
/// that mirror the drawn core lattice (3d-phase-4-plan.md §7.1).
#[derive(Debug)]
struct ChunkEntry {
    key: u64,
    handle: u64,
    heights: Vec<f32>,
    ambient_occlusion: Vec<u8>,
    min_height: f32,
    max_height: f32,
}

/// Interpolated attributes of the resident terrain triangle below a world
/// position. Both fields use the exact v00→v11 split drawn by the renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundSurface {
    /// Height of the rendered triangle at the query point.
    pub height: f32,
    /// Ambient-occlusion attribute interpolated from the triangle's vertex
    /// bytes and requantized once for an organism instance.
    pub ambient_occlusion: u8,
}

/// Closest visible resident-terrain intersection for a POV ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovTerrainHit {
    /// Distance from the camera along the normalized world ray.
    pub distance: f64,
    /// Absolute world-space intersection point.
    pub position: glam::DVec3,
    /// Resident region whose core lattice supplied the triangle.
    pub region: RegionCoord,
    /// Meshed height-field cell within the 64 by 64 region lattice.
    pub cell: [u8; 2],
    /// Triangle within the cell: zero is v00-v10-v11, one is v00-v11-v01.
    pub triangle: u8,
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
    /// Changes only when the resident CPU/GPU geometry set changes. This is
    /// the precise dirty key for cached picking, distinct from telemetry.
    resident_generation: u64,
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
            resident_generation: 0,
        }
    }

    /// This frame's counter snapshot (plan §7.5).
    #[must_use]
    pub fn counters(&self) -> PovCounters {
        let mut counters = self.counters;
        counters.mesh_ms = self.mesh_micros.load(Ordering::Relaxed) as f64 / 1000.0;
        counters
    }

    /// Dirty generation of the exact resident core lattices used for both
    /// GPU uploads and CPU picking.
    #[must_use]
    pub const fn resident_generation(&self) -> u64 {
        self.resident_generation
    }

    /// Resident chunk count (telemetry).
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Whether no chunks are resident (the clippy `len`/`is_empty` pair;
    /// telemetry like [`Self::len`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Conservative world-space union of resident chunk cores for
    /// directional-shadow fitting. The skirt drop expands depth coverage but
    /// skirts themselves remain excluded from the renderer's caster draw.
    #[must_use]
    pub fn shadow_bounds(&self) -> Option<PovShadowBounds> {
        self.chunks.iter().fold(None, |bounds, (coord, entry)| {
            let (ox, oy) = coord.origin();
            let chunk = PovShadowBounds {
                min: [ox, oy, f64::from(entry.min_height - POV_SKIRT_DROP)],
                max: [
                    ox + REGION_SIZE,
                    oy + REGION_SIZE,
                    f64::from(entry.max_height),
                ],
            };
            Some(bounds.map_or(chunk, |bounds: PovShadowBounds| bounds.union(chunk)))
        })
    }

    /// Whether nothing is in flight or awaiting integration — with an inline
    /// executor, repeated `sync` calls until `idle` fully settle the ring
    /// (the `--pov-script` snapshot path).
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty() && self.in_flight.is_empty()
    }

    /// Height and ambient occlusion of the rendered terrain surface under a
    /// world position. The resident height/AO lattices are swapped atomically
    /// with the matching GPU upload, so an organism cannot observe attributes
    /// from two terrain revisions. `None` means the covering chunk is not yet
    /// resident and callers must omit the organism rather than float it.
    ///
    /// Barycentric interpolation over the 65×65 core lattice, splitting each
    /// cell along the v00→v11 diagonal exactly as
    /// `renderer::pov::chunk_indices` does (§4.2) — mid-cell heights agree
    /// with the drawn triangles, and the interpolant is continuous across
    /// the diagonal, cell edges, and (in steady state, ADR 0027) region
    /// borders. O(1), no allocation, pure over `&self`.
    #[must_use]
    pub fn ground_surface(&self, wx: f64, wy: f64) -> Option<GroundSurface> {
        // Floor semantics put a border position in exactly one region, whose
        // chunk owns that border column.
        let coord = RegionCoord::from_world(wx, wy);
        let entry = self.chunks.get(&coord)?;
        let (ox, oy) = coord.origin();
        let max_cell = (POV_MESH_RES - 1) as f64;
        let gx = (wx - ox) / SPACING;
        let gy = (wy - oy) / SPACING;
        let cx = gx.floor().clamp(0.0, max_cell);
        let cy = gy.floor().clamp(0.0, max_cell);
        let (i, j) = (cx as usize, cy as usize);
        let (fx, fy) = ((gx - cx) as f32, (gy - cy) as f32);
        let samples = |values: &[f32]| {
            [
                values[j * POV_GRID + i],
                values[j * POV_GRID + i + 1],
                values[(j + 1) * POV_GRID + i],
                values[(j + 1) * POV_GRID + i + 1],
            ]
        };
        let ao_samples = [
            entry.ambient_occlusion[j * POV_GRID + i],
            entry.ambient_occlusion[j * POV_GRID + i + 1],
            entry.ambient_occlusion[(j + 1) * POV_GRID + i],
            entry.ambient_occlusion[(j + 1) * POV_GRID + i + 1],
        ]
        .map(|value| f32::from(value) / 255.0);
        Some(GroundSurface {
            height: triangle_interpolate(samples(&entry.heights), fx, fy),
            ambient_occlusion: quantize_light(triangle_interpolate(ao_samples, fx, fy)),
        })
    }

    /// Height-only compatibility query used by walk mode. Delegating to
    /// [`Self::ground_surface`] prevents organism footing and collision from
    /// ever acquiring different triangle math.
    #[must_use]
    pub fn ground_height(&self, wx: f64, wy: f64) -> Option<f32> {
        self.ground_surface(wx, wy).map(|surface| surface.height)
    }

    /// Intersect a normalized world ray with the resident terrain cores.
    ///
    /// Only the exact 65 by 65 CPU lattices paired with integrated renderer
    /// uploads participate. Skirts, analytic loading-frontier terrain, and
    /// pending chunks are deliberately absent, so a hit always describes
    /// geometry visible in the current frame. Chunk-local XY arithmetic
    /// preserves precision at far world coordinates.
    #[must_use]
    pub fn raycast(&self, ray: PovRay, max_distance: f64) -> Option<PovTerrainHit> {
        let [near_distance, far_distance] = ray.clip_distances;
        let limit = max_distance.min(far_distance);
        if !ray.origin.is_finite()
            || !ray.direction.is_finite()
            || !near_distance.is_finite()
            || !far_distance.is_finite()
            || near_distance < 0.0
            || limit <= near_distance
            || (ray.direction.length_squared() - 1.0).abs() > 1.0e-9
        {
            return None;
        }

        let mut candidates = Vec::with_capacity(self.chunks.len());
        for (&coord, entry) in &self.chunks {
            let (ox, oy) = coord.origin();
            let local_ray = PovRay {
                origin: glam::DVec3::new(ray.origin.x - ox, ray.origin.y - oy, ray.origin.z),
                direction: ray.direction,
                clip_distances: ray.clip_distances,
            };
            if let Some((enter, exit)) = ray_box_interval(
                local_ray,
                glam::DVec3::new(0.0, 0.0, f64::from(entry.min_height)),
                glam::DVec3::new(REGION_SIZE, REGION_SIZE, f64::from(entry.max_height)),
                near_distance,
                limit,
            ) {
                candidates.push((enter, exit, coord, local_ray));
            }
        }
        candidates.sort_unstable_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| a.2.x.cmp(&b.2.x))
                .then_with(|| a.2.y.cmp(&b.2.y))
        });

        let mut best = None;
        let mut best_distance = limit;
        for (enter, exit, coord, local_ray) in candidates {
            if enter >= best_distance {
                break;
            }
            let Some(entry) = self.chunks.get(&coord) else {
                continue;
            };
            if let Some(hit) =
                raycast_chunk_core(ray, local_ray, coord, entry, enter, exit.min(best_distance))
            {
                best_distance = hit.distance;
                best = Some(hit);
            }
        }
        best.filter(|hit| hit.distance < limit)
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
        let generation_before = self.resident_generation;
        let mut resident_changed = false;
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
                let Some(provenance) = chunk_provenance(map, coord) else {
                    continue; // not settled yet: hole at the frontier, hidden in fog
                };
                let key = provenance.key;
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
                self.schedule(map, coord, provenance, executor);
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
            let ChunkMesh {
                vertices,
                heights,
                ambient_occlusion,
                min_height,
                max_height,
                river_indices,
            } = result.mesh;
            self.chunks.insert(
                result.coord,
                ChunkEntry {
                    key: result.key,
                    handle,
                    heights,
                    ambient_occlusion,
                    min_height,
                    max_height,
                },
            );
            resident_changed = true;
            let (ox, oy) = result.coord.origin();
            uploads.push(TerrainChunkUpload {
                handle,
                origin: [ox, oy],
                detail_base: detail_base(result.coord),
                vertices,
                river_indices,
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
                resident_changed = true;
            }
        }
        if resident_changed {
            self.resident_generation = generation_before.wrapping_add(1);
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
        provenance: ChunkProvenance,
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
        let ChunkProvenance { key, terrain_halo } = provenance;
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
                // Worker-side mesh telemetry. `Instant` panics on
                // wasm32-unknown-unknown (no monotonic clock without JS
                // bindings), so the browser build simply reports zero —
                // timing is telemetry, never behavior (phase-6-plan.md
                // §12.6).
                #[cfg(not(target_arch = "wasm32"))]
                let start = std::time::Instant::now();
                let inputs = ChunkMeshInputs {
                    coord,
                    terrain_halo,
                    river: &river,
                    wetness: &wetness,
                    biome: &biome,
                    dominant_ids: &dominant_ids,
                };
                let Some(mesh) = mesh_region_chunk_cancellable(&inputs, &cancel) else {
                    return;
                };
                #[cfg(not(target_arch = "wasm32"))]
                micros.fetch_add(start.elapsed().as_micros() as u64, Ordering::Relaxed);
                #[cfg(target_arch = "wasm32")]
                let _ = &micros;
                // The receiver may be gone during shutdown; nothing to do.
                let _ = tx.send(MeshResult { coord, key, mesh });
            }),
        );
    }
}

fn ray_box_interval(
    ray: PovRay,
    min: glam::DVec3,
    max: glam::DVec3,
    min_distance: f64,
    max_distance: f64,
) -> Option<(f64, f64)> {
    let origins = [ray.origin.x, ray.origin.y, ray.origin.z];
    let directions = [ray.direction.x, ray.direction.y, ray.direction.z];
    let mins = [min.x, min.y, min.z];
    let maxs = [max.x, max.y, max.z];
    let mut enter = min_distance;
    let mut exit = max_distance;
    for axis in 0..3 {
        if directions[axis] == 0.0 {
            if origins[axis] < mins[axis] || origins[axis] > maxs[axis] {
                return None;
            }
            continue;
        }
        let inverse = directions[axis].recip();
        let mut near = (mins[axis] - origins[axis]) * inverse;
        let mut far = (maxs[axis] - origins[axis]) * inverse;
        if near > far {
            core::mem::swap(&mut near, &mut far);
        }
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    Some((enter, exit))
}

/// Front-face-only Moller-Trumbore intersection, matching the POV pipelines'
/// counter-clockwise back-face culling. The direction is deliberately not
/// required to be normalized so transformed organism rays retain world `t`.
fn ray_triangle_distance(
    ray: PovRay,
    vertices: [glam::DVec3; 3],
    min_distance: f64,
    max_distance: f64,
) -> Option<f64> {
    let edge1 = vertices[1] - vertices[0];
    let edge2 = vertices[2] - vertices[0];
    let p = ray.direction.cross(edge2);
    let determinant = edge1.dot(p);
    if determinant <= 1.0e-12 {
        return None;
    }
    let inverse = determinant.recip();
    let s = ray.origin - vertices[0];
    let u = s.dot(p) * inverse;
    const EDGE_EPSILON: f64 = 1.0e-10;
    if !(-EDGE_EPSILON..=1.0 + EDGE_EPSILON).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = ray.direction.dot(q) * inverse;
    if v < -EDGE_EPSILON || u + v > 1.0 + EDGE_EPSILON {
        return None;
    }
    let distance = edge2.dot(q) * inverse;
    (distance >= min_distance && distance <= max_distance).then_some(distance)
}

fn raycast_canonical_icosphere(
    local_ray: PovRay,
    min_distance: f64,
    max_distance: f64,
) -> Option<f64> {
    // The canonical sphere is radius 0.5. This quadratic is deliberately
    // broad phase only: the visible silhouette is the faceted canonical mesh.
    let a = local_ray.direction.length_squared();
    let half_b = local_ray.origin.dot(local_ray.direction);
    let c = local_ray.origin.length_squared() - 0.25;
    let discriminant = half_b.mul_add(half_b, -a * c);
    if a == 0.0 || discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let ellipsoid_enter = (-half_b - root) / a;
    let ellipsoid_exit = (-half_b + root) / a;
    if ellipsoid_exit < min_distance || ellipsoid_enter > max_distance {
        return None;
    }

    let (vertices, indices) = canonical_icosphere_geometry();
    let mut best = max_distance;
    let mut found = false;
    for triangle in indices.chunks_exact(3) {
        let vertex = |index: u16| {
            let [x, y, z] = vertices[usize::from(index)].position;
            glam::DVec3::new(f64::from(x), f64::from(y), f64::from(z))
        };
        let triangle = [
            vertex(triangle[0]),
            vertex(triangle[1]),
            vertex(triangle[2]),
        ];
        if let Some(distance) = ray_triangle_distance(local_ray, triangle, min_distance, best) {
            if distance < best {
                best = distance;
                found = true;
            }
        }
    }
    found.then_some(best)
}

fn dda_initial_cell(local: f64, direction: f64) -> i32 {
    let grid = local / SPACING;
    let mut cell = grid.floor() as i32;
    if direction < 0.0 && grid == grid.floor() {
        cell -= 1;
    }
    cell.clamp(0, POV_MESH_RES as i32 - 1)
}

fn dda_next_boundary(origin: f64, direction: f64, cell: i32) -> (f64, f64, i32) {
    if direction > 0.0 {
        (
            ((f64::from(cell) + 1.0) * SPACING - origin) / direction,
            SPACING / direction,
            1,
        )
    } else if direction < 0.0 {
        (
            (f64::from(cell) * SPACING - origin) / direction,
            -SPACING / direction,
            -1,
        )
    } else {
        (f64::INFINITY, f64::INFINITY, 0)
    }
}

fn raycast_chunk_core(
    world_ray: PovRay,
    local_ray: PovRay,
    coord: RegionCoord,
    entry: &ChunkEntry,
    enter: f64,
    exit: f64,
) -> Option<PovTerrainHit> {
    let start = local_ray.at(enter);
    let mut cell_x = dda_initial_cell(start.x, local_ray.direction.x);
    let mut cell_y = dda_initial_cell(start.y, local_ray.direction.y);
    let (mut next_x, delta_x, step_x) =
        dda_next_boundary(local_ray.origin.x, local_ray.direction.x, cell_x);
    let (mut next_y, delta_y, step_y) =
        dda_next_boundary(local_ray.origin.y, local_ray.direction.y, cell_y);
    // Roundoff at the clipped AABB entry may put the first computed boundary
    // microscopically behind us. Advance it without skipping a cell.
    while next_x < enter {
        next_x += delta_x;
    }
    while next_y < enter {
        next_y += delta_y;
    }

    let mut cell_enter = enter;
    let mut best: Option<PovTerrainHit> = None;
    // An axis-aligned line crosses at most 64 cells; a diagonal crosses at
    // most 127. The small guard makes malformed floating input total.
    for _ in 0..=(POV_MESH_RES * 2) {
        if cell_x < 0
            || cell_x >= POV_MESH_RES as i32
            || cell_y < 0
            || cell_y >= POV_MESH_RES as i32
            || cell_enter > exit
        {
            break;
        }
        let cell_exit = next_x.min(next_y).min(exit);
        let i = cell_x as usize;
        let j = cell_y as usize;
        let sample = |x: usize, y: usize| f64::from(entry.heights[y * POV_GRID + x]);
        let x0 = i as f64 * SPACING;
        let y0 = j as f64 * SPACING;
        let x1 = x0 + SPACING;
        let y1 = y0 + SPACING;
        let v00 = glam::DVec3::new(x0, y0, sample(i, j));
        let v10 = glam::DVec3::new(x1, y0, sample(i + 1, j));
        let v01 = glam::DVec3::new(x0, y1, sample(i, j + 1));
        let v11 = glam::DVec3::new(x1, y1, sample(i + 1, j + 1));
        for (triangle, vertices) in [[v00, v10, v11], [v00, v11, v01]].into_iter().enumerate() {
            let upper = best.map_or(cell_exit, |hit| hit.distance.min(cell_exit));
            if let Some(distance) = ray_triangle_distance(local_ray, vertices, cell_enter, upper) {
                if best.is_none_or(|hit| distance < hit.distance) {
                    best = Some(PovTerrainHit {
                        distance,
                        position: world_ray.at(distance),
                        region: coord,
                        cell: [i as u8, j as u8],
                        triangle: triangle as u8,
                    });
                }
            }
        }
        if best.is_some() {
            break;
        }
        if cell_exit >= exit || (!next_x.is_finite() && !next_y.is_finite()) {
            break;
        }

        let tie = next_x.is_finite()
            && next_y.is_finite()
            && (next_x - next_y).abs() <= 1.0e-12 * next_x.abs().max(next_y.abs()).max(1.0);
        let step_along_x = next_x < next_y || tie;
        let step_along_y = next_y < next_x || tie;
        if step_along_x {
            cell_x += step_x;
            cell_enter = cell_enter.max(next_x);
            next_x += delta_x;
        }
        if step_along_y {
            cell_y += step_y;
            cell_enter = cell_enter.max(next_y);
            next_y += delta_y;
        }
    }
    best
}

/// Interpolate one four-corner attribute over the renderer's fixed cell
/// diagonal. `v = [v00, v10, v01, v11]`.
fn triangle_interpolate(v: [f32; 4], fx: f32, fy: f32) -> f32 {
    if fx >= fy {
        v[0] + fx * (v[1] - v[0]) + fy * (v[3] - v[1])
    } else {
        v[0] + fy * (v[2] - v[0]) + fx * (v[3] - v[2])
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
    /// strafing right, `u` straight up (the held-key movement basis). In
    /// walk mode the displacement is in the walk basis instead (`f` along
    /// horizontal yaw, `r` strafe, `u` ignored) and the eye then snaps to
    /// `ground + EYE_HEIGHT` — scripted captures want the settled pose, not
    /// a clamped animation (3d-phase-2-plan.md §6.4).
    Move { forward: f64, right: f64, up: f64 },
    /// `walk` — enter walk mode through the same toggle path the live `F`
    /// key uses, including the snap to ground (3d-phase-2-plan.md §6.4).
    Walk,
    /// `fly` — return to fly mode, keeping position and orientation.
    Fly,
    /// `settle[:n]` — run `n` (default 8) zero-travel world updates at the
    /// camera position, so tiles generate/regenerate.
    Settle(u32),
    /// `snap:path.ppm` — settle the chunk ring and write a snapshot.
    Snap(String),
    /// `split:path.ppm` — settle once, then write the aligned Map + POV +
    /// information-panel surface through the native headless adapter. The
    /// POV pane still uses file-bound [`renderer::pov::PovCapture`]; this
    /// instruction does not introduce live renderer readback (ADR 0021).
    Split(String),
}

/// Parse a `--pov-script` instruction sequence: instructions separated by
/// `;`, each `op` or `op:args` with comma-separated args, e.g.
/// `"pos:300,-10; mouse:120,40; snap:a.ppm; move:200; settle; split:b.ppm"`.
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
            "walk" | "fly" => {
                if !args.is_empty() {
                    return Err(format!("{op} takes no arguments, got {args:?}"));
                }
                if op == "walk" {
                    PovInstr::Walk
                } else {
                    PovInstr::Fly
                }
            }
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
            "split" => {
                if args.is_empty() {
                    return Err(String::from("split wants a file path"));
                }
                PovInstr::Split(String::from(args))
            }
            other => return Err(format!("unknown instruction {other:?}")),
        });
    }
    if out.is_empty() {
        return Err(String::from("empty script"));
    }
    Ok(out)
}

/// Apply one camera-movement script instruction (3d-phase-2-plan.md §6.4) —
/// the shared core of the headless runner and the walk-kinematics tests.
/// `mouse` goes through [`PovCamera::look`]; `move` uses the active mode's
/// movement basis (walk: horizontal yaw + strafe, `u` ignored, then a snap
/// to `ground + EYE_HEIGHT`); `walk`/`fly` go through [`PovCamera::set_walk`],
/// the same toggle path as the live `F` key. `ground(x, y)` supplies the
/// composed walk ground under a world position (the runner settles the world
/// before sampling; tests pass a settled manager or a stub). Returns whether
/// the instruction was camera-affecting; `size`/`pos`/`settle`/`snap`/`split`
/// are the runner's concern and return false.
pub fn apply_camera_instr(
    camera: &mut PovCamera,
    instr: &PovInstr,
    ground: &mut dyn FnMut(f64, f64) -> f64,
) -> bool {
    match *instr {
        PovInstr::Mouse(dx, dy) => camera.look(dx, dy),
        PovInstr::Move { forward, right, up } => {
            if camera.walk {
                camera.pos += camera.walk_forward() * forward + camera.right() * right;
                let g = ground(camera.pos.x, camera.pos.y);
                camera.snap_to_ground(g);
            } else {
                camera.pos += camera.forward() * forward
                    + camera.right() * right
                    + glam::DVec3::new(0.0, 0.0, up);
            }
        }
        PovInstr::Walk => {
            let g = ground(camera.pos.x, camera.pos.y);
            camera.set_walk(true, g);
        }
        PovInstr::Fly => camera.set_walk(false, 0.0),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderer::pov::chunk_indices;
    use world_core::terrain::elevation;
    use world_core::{
        Expressed, LocalPos, PossibilityDomain, PossibilityField, PossibilitySignature, Trophic,
        POSSIBILITY_DIMS, POSSIBILITY_QUANT,
    };
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
        terrain_halo: TerrainPossibilityHalo,
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
                terrain_halo: map
                    .terrain_possibility_halo(coord)
                    .expect("resident Terrain halo"),
                river: tiles.channels[CHANNEL_RIVER].clone().expect("river tile"),
                wetness: tiles.channels[CHANNEL_WETNESS].clone().expect("wetness"),
                biome: tiles.biome.clone().expect("biome tile"),
                dominant_ids,
            }
        }

        fn inputs(&self) -> ChunkMeshInputs<'_> {
            ChunkMeshInputs {
                coord: self.coord,
                terrain_halo: self.terrain_halo.clone(),
                river: &self.river,
                wetness: &self.wetness,
                biome: &self.biome,
                dominant_ids: &self.dominant_ids,
            }
        }
    }

    fn test_organism(id: u64, slot: u16, form: u8, size: f32) -> Organism {
        Organism {
            id,
            species: 9,
            trophic: Trophic::Herbivore,
            slot,
            cell: LocalPos::new(0, 0),
            world_pos: (32.0, 48.0),
            expressed: Expressed {
                hue: 0.37,
                luminance: 0.62,
                size,
                activity: 0.4,
                aggression: 0.2,
                form,
            },
        }
    }

    fn flat_chunk_manager(ao: u8) -> PovChunkManager {
        let mut chunks = PovChunkManager::new();
        chunks.chunks.insert(
            RegionCoord::new(0, 0),
            ChunkEntry {
                key: 1,
                handle: 1,
                heights: vec![10.0; CORE_VERTS],
                ambient_occlusion: vec![ao; CORE_VERTS],
                min_height: 10.0,
                max_height: 10.0,
            },
        );
        chunks
    }

    fn normalized_ray(origin: [f64; 3], direction: [f64; 3]) -> PovRay {
        PovRay {
            origin: glam::DVec3::from_array(origin),
            direction: glam::DVec3::from_array(direction).normalize(),
            clip_distances: [POV_NEAR_DISTANCE, POV_FAR_DISTANCE],
        }
    }

    fn insert_height_chunk(
        chunks: &mut PovChunkManager,
        coord: RegionCoord,
        handle: u64,
        heights: Vec<f32>,
    ) {
        assert_eq!(heights.len(), CORE_VERTS);
        let min_height = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let max_height = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        chunks.chunks.insert(
            coord,
            ChunkEntry {
                key: handle,
                handle,
                heights,
                ambient_occlusion: vec![255; CORE_VERTS],
                min_height,
                max_height,
            },
        );
    }

    fn retained_organisms(
        entries: impl IntoIterator<Item = (Organism, PovOrganismVisual)>,
    ) -> PovOrganismManager {
        let mut manager = PovOrganismManager::new();
        for (organism, visual) in entries {
            let entry = OrganismScratch {
                key: OrganismVisualKey::new(&organism, visual),
                organism,
                visual,
            };
            match visual.primitive {
                PovOrganismPrimitive::Box => manager.box_scratch.push(entry),
                PovOrganismPrimitive::Sphere => manager.sphere_scratch.push(entry),
            }
        }
        let order = |entry: &OrganismScratch| (entry.key.id, entry.key.slot);
        manager.box_scratch.sort_unstable_by_key(order);
        manager.sphere_scratch.sort_unstable_by_key(order);
        manager.visual_generation = 1;
        manager
    }

    fn test_visual(
        primitive: PovOrganismPrimitive,
        position: [f64; 3],
        scale: [f32; 3],
    ) -> PovOrganismVisual {
        PovOrganismVisual {
            primitive,
            position,
            scale,
            yaw: 0.0,
            color: [100, 120, 140, 0],
            ambient_occlusion: 255,
            bob: [0.0; 2],
        }
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} differs from {expected} by more than {tolerance}"
        );
    }

    #[test]
    fn screen_rays_match_camera_center_projection_aspect_and_far_origin() {
        let mut camera = PovCamera::new();
        camera.pos = glam::DVec3::new(1.0e12 + 0.25, -1.0e12 - 0.5, 700.75);
        camera.yaw = 0.7;
        camera.pitch = -0.3;
        let center = camera
            .screen_ray([800.0, 450.0], [1600, 900])
            .expect("center ray");
        assert_eq!(center.origin, camera.pos, "far origin must remain exact");
        assert_close(center.direction.length(), 1.0, 1.0e-12);
        assert!(
            center
                .direction
                .abs_diff_eq(camera.forward().normalize(), 1.0e-12),
            "center ray must equal camera forward"
        );

        let camera = PovCamera::new();
        let forward = camera.forward();
        let right = camera.right();
        let up = right.cross(forward).normalize();
        let half_fov = f64::from((POV_VERTICAL_FOV * 0.5).tan());
        for (size, expected_aspect) in [([1600, 800], 2.0), ([600, 1200], 0.5)] {
            let ray = camera
                .screen_ray([0.0, 0.0], size)
                .expect("top-left edge ray");
            let along = ray.direction.dot(forward);
            assert_close(
                ray.direction.dot(right) / along,
                -expected_aspect * half_fov,
                1.0e-12,
            );
            assert_close(ray.direction.dot(up) / along, half_fov, 1.0e-12);
            assert_close(ray.clip_distances[0], POV_NEAR_DISTANCE / along, 1.0e-12);
            assert_close(ray.clip_distances[1], POV_FAR_DISTANCE / along, 1.0e-9);
        }

        // `view_proj` clamps pathological portrait aspects; picking must use
        // that same frustum rather than the smaller raw width/height ratio.
        let narrow = camera
            .screen_ray([0.0, 1_000.0], [1, 2_000])
            .expect("one-pixel-wide ray");
        let along = narrow.direction.dot(forward);
        assert_close(
            narrow.direction.dot(right) / along,
            -1.0e-3 * half_fov,
            1.0e-10,
        );
    }

    #[test]
    fn screen_ray_rejects_invalid_or_outside_pane_coordinates() {
        let camera = PovCamera::new();
        for (point, size) in [
            ([0.0, 0.0], [0, 10]),
            ([0.0, 0.0], [10, 0]),
            ([-0.001, 5.0], [10, 10]),
            ([10.0, 5.0], [10, 10]),
            ([5.0, 10.0], [10, 10]),
            ([f64::NAN, 5.0], [10, 10]),
            ([5.0, f64::INFINITY], [10, 10]),
        ] {
            assert!(camera.screen_ray(point, size).is_none(), "{point:?}");
        }
    }

    #[test]
    fn off_axis_raycast_uses_projection_plane_clips_not_fixed_radial_clips() {
        let camera = PovCamera::new();
        let ray = camera
            .screen_ray([0.0, 0.0], [1_600, 800])
            .expect("corner ray");
        assert!(ray.clip_distances[0] > POV_NEAR_DISTANCE);
        assert!(ray.clip_distances[1] > POV_FAR_DISTANCE);

        let source = test_organism(0xCC, 0, 0, 1.0);
        let before_near = ray.at(ray.clip_distances[0] * 0.5);
        let near_manager = retained_organisms([(
            source,
            test_visual(
                PovOrganismPrimitive::Box,
                before_near.to_array(),
                [0.001; 3],
            ),
        )]);
        assert!(near_manager.raycast(ray, 0.0, 10.0).is_none());

        // A corner body beyond radial 2048 is still inside the projection's
        // view-axis far plane. A fog reach above 2048 may therefore draw and
        // inspect it (for example a large WER_POV_RADIUS override).
        let visible_distance = POV_FAR_DISTANCE + 25.0;
        let visible = ray.at(visible_distance);
        let far_manager = retained_organisms([(
            source,
            test_visual(PovOrganismPrimitive::Box, visible.to_array(), [2.0; 3]),
        )]);
        let hit = far_manager
            .raycast(ray, 0.0, visible_distance + 10.0)
            .expect("off-axis body before the view-axis far plane");
        assert!(hit.distance > POV_FAR_DISTANCE);
        assert!(hit.distance < ray.clip_distances[1]);
    }

    #[test]
    fn terrain_raycast_hits_flat_sloped_and_both_diagonal_triangles() {
        let flat = flat_chunk_manager(255);
        let hit = flat
            .raycast(normalized_ray([6.0, 7.0, 30.0], [0.0, 0.0, -1.0]), 100.0)
            .expect("flat terrain hit");
        assert_close(hit.distance, 20.0, 1.0e-12);
        assert_eq!(hit.position, glam::DVec3::new(6.0, 7.0, 10.0));
        assert_eq!(hit.cell, [1, 1]);
        assert_eq!(hit.triangle, 1);

        let mut chunks = PovChunkManager::new();
        let mut heights = vec![0.0; CORE_VERTS];
        heights[1] = 4.0;
        heights[POV_GRID] = 8.0;
        heights[POV_GRID + 1] = 12.0;
        insert_height_chunk(&mut chunks, RegionCoord::new(0, 0), 1, heights);
        for (xy, expected_height, expected_triangle) in [
            ([3.0, 1.0], 5.0, 0),
            ([1.0, 3.0], 7.0, 1),
            ([2.0, 2.0], 6.0, 0),
        ] {
            let hit = chunks
                .raycast(
                    normalized_ray([xy[0], xy[1], 20.0], [0.0, 0.0, -1.0]),
                    100.0,
                )
                .expect("sloped terrain hit");
            assert_close(hit.position.z, expected_height, 1.0e-12);
            assert_eq!(hit.cell, [0, 0]);
            assert_eq!(hit.triangle, expected_triangle, "probe {xy:?}");
        }
    }

    #[test]
    fn terrain_raycast_traverses_cells_regions_and_far_world_coordinates() {
        let mut cross_cell = PovChunkManager::new();
        let mut heights = vec![0.0; CORE_VERTS];
        // Expand the broad-phase height range away from the ray so the DDA
        // must walk many flat cells before reaching z=0.
        heights[POV_MESH_RES * POV_GRID] = 10.0;
        insert_height_chunk(&mut cross_cell, RegionCoord::new(0, 0), 1, heights);
        let cross_cell_ray = normalized_ray([1.0, 2.0, 15.0], [1.0, 0.0, -0.21]);
        let hit = cross_cell
            .raycast(cross_cell_ray, 500.0)
            .expect("cross-cell hit");
        assert!(hit.cell[0] >= 17, "DDA stopped too early: {hit:?}");
        assert_close(hit.position.z, 0.0, 1.0e-10);

        let mut cross_region = PovChunkManager::new();
        insert_height_chunk(
            &mut cross_region,
            RegionCoord::new(0, 0),
            1,
            vec![0.0; CORE_VERTS],
        );
        insert_height_chunk(
            &mut cross_region,
            RegionCoord::new(1, 0),
            2,
            vec![0.0; CORE_VERTS],
        );
        let hit = cross_region
            .raycast(normalized_ray([200.0, 20.0, 15.0], [1.0, 0.0, -0.1]), 500.0)
            .expect("cross-region hit");
        assert_eq!(hit.region, RegionCoord::new(1, 0));
        assert_close(hit.position.x, 350.0, 1.0e-9);

        let far_coord = RegionCoord::new(1_000_000, -1_000_000);
        let (ox, oy) = far_coord.origin();
        let mut far = PovChunkManager::new();
        insert_height_chunk(&mut far, far_coord, 7, vec![10.0; CORE_VERTS]);
        let hit = far
            .raycast(
                normalized_ray([ox + 6.25, oy + 7.75, 30.0], [0.0, 0.0, -1.0]),
                100.0,
            )
            .expect("far-origin hit");
        assert_eq!(hit.region, far_coord);
        assert_eq!(hit.position.x, ox + 6.25);
        assert_eq!(hit.position.y, oy + 7.75);
        assert_close(hit.distance, 20.0, 1.0e-12);
    }

    #[test]
    fn terrain_raycast_excludes_missing_skirts_sky_and_beyond_fog() {
        let empty = PovChunkManager::new();
        assert!(empty
            .raycast(normalized_ray([1.0, 1.0, 20.0], [0.0, 0.0, -1.0]), 100.0)
            .is_none());

        let chunks = flat_chunk_manager(255);
        // A horizontal ray below the core would hit the renderer's defensive
        // west skirt, but skirts are intentionally not inspectable terrain.
        assert!(chunks
            .raycast(normalized_ray([-5.0, 5.0, 0.0], [1.0, 0.0, 0.0]), 100.0)
            .is_none());
        assert!(chunks
            .raycast(normalized_ray([5.0, 5.0, 30.0], [0.0, 0.0, 1.0]), 100.0)
            .is_none());
        let down = normalized_ray([5.0, 5.0, 30.0], [0.0, 0.0, -1.0]);
        assert!(chunks.raycast(down, 19.999).is_none());
        assert!(chunks.raycast(down, 20.0).is_none(), "fog edge is hidden");
        assert!(chunks.raycast(down, 20.001).is_some());
    }

    #[test]
    fn yawed_box_uses_oriented_slabs_and_rejects_behind_or_fogged_hits() {
        let organism = test_organism(0xAA, 3, 0, 1.0);
        let mut visual = test_visual(PovOrganismPrimitive::Box, [0.0, 0.0, 0.0], [6.0, 1.0, 2.0]);
        visual.yaw = core::f32::consts::FRAC_PI_4;
        let manager = retained_organisms([(organism, visual)]);
        let along_long_axis = normalized_ray([-10.0, -10.0, 0.0], [1.0, 1.0, 0.0]);
        let hit = manager
            .raycast(along_long_axis, 0.0, 100.0)
            .expect("yawed OBB hit");
        assert_eq!(hit.organism, organism);
        assert!(manager
            .raycast(along_long_axis, 0.0, hit.distance)
            .is_none());
        assert!(manager
            .raycast(along_long_axis, 0.0, hit.distance + 1.0e-9)
            .is_some());

        // This point is inside the rotated body's conservative world AABB,
        // but outside its narrow local-y slab.
        assert!(manager
            .raycast(
                normalized_ray([2.2, -2.2, 5.0], [0.0, 0.0, -1.0]),
                0.0,
                100.0,
            )
            .is_none());
        assert!(manager
            .raycast(
                normalized_ray([-10.0, -10.0, 0.0], [-1.0, -1.0, 0.0]),
                0.0,
                100.0,
            )
            .is_none());
    }

    #[test]
    fn scaled_canonical_icosphere_requires_broad_and_faceted_intersections() {
        let organism = test_organism(0xBB, 2, 1, 1.0);
        let mut visual = test_visual(
            PovOrganismPrimitive::Sphere,
            [10.0, 0.0, 0.0],
            [4.0, 2.0, 6.0],
        );
        visual.yaw = 0.7;
        let manager = retained_organisms([(organism, visual)]);
        let ray = normalized_ray([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        let local = visual.local_ray(ray, 0.0).expect("valid scale");
        let expected = raycast_canonical_icosphere(local, POV_NEAR_DISTANCE, 100.0)
            .expect("canonical facets hit");
        let hit = manager.raycast(ray, 0.0, 100.0).expect("scaled sphere hit");
        assert_close(hit.distance, expected, 1.0e-12);
        assert_eq!(hit.organism, organism);

        let broad_miss = normalized_ray([-2.0, 0.51, 0.0], [1.0, 0.0, 0.0]);
        assert!(raycast_canonical_icosphere(broad_miss, 0.0, 10.0).is_none());

        // A near-tangent ray enters the analytic radius-0.5 sphere but not
        // the inscribed two-subdivision facets. The broad phase must never be
        // promoted to a visible hit.
        let facet_miss = (0..720)
            .map(|step| {
                let angle = f64::from(step) * core::f64::consts::TAU / 720.0;
                normalized_ray(
                    [-2.0, 0.499 * angle.cos(), 0.499 * angle.sin()],
                    [1.0, 0.0, 0.0],
                )
            })
            .find(|ray| raycast_canonical_icosphere(*ray, 0.0, 10.0).is_none())
            .expect("inscribed facets leave a gap inside the analytic sphere");
        let a = facet_miss.direction.length_squared();
        let half_b = facet_miss.origin.dot(facet_miss.direction);
        let c = facet_miss.origin.length_squared() - 0.25;
        assert!(half_b.mul_add(half_b, -a * c) > 0.0);
    }

    #[test]
    fn organism_raycast_selects_nearest_source_identity_and_tracks_bob() {
        let mut far_source = test_organism(1, 1, 0, 1.0);
        far_source.species = 0x1111_2222_3333_4444;
        far_source.cell = LocalPos::new(7, 6);
        far_source.expressed.aggression = 0.875;
        let near_source = test_organism(99, 3, 0, 1.0);
        let far_visual = test_visual(PovOrganismPrimitive::Box, [20.0, 0.0, 0.0], [2.0; 3]);
        let near_visual = test_visual(PovOrganismPrimitive::Box, [10.0, 0.0, 0.0], [2.0; 3]);
        let manager = retained_organisms([(far_source, far_visual), (near_source, near_visual)]);
        let hit = manager
            .raycast(normalized_ray([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), 0.0, 100.0)
            .expect("nearest body");
        assert_eq!(hit.organism, near_source, "distance beats stable id order");
        assert_close(hit.distance, 9.0, 1.0e-12);

        let mut bob_visual =
            test_visual(PovOrganismPrimitive::Box, [0.0, 10.0, 0.0], [2.0, 2.0, 1.0]);
        bob_visual.bob = [2.0, 0.0];
        let bob_manager = retained_organisms([(far_source, bob_visual)]);
        assert!(bob_manager.has_animated_visuals());
        assert!(bob_manager
            .raycast(normalized_ray([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), 0.0, 100.0,)
            .is_some());
        assert!(bob_manager
            .raycast(normalized_ray([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]), 1.0, 100.0,)
            .is_none());
        let bob_hit = bob_manager
            .raycast(normalized_ray([0.0, 0.0, 2.0], [0.0, 1.0, 0.0]), 1.0, 100.0)
            .expect("time-shifted bob hit");
        assert_eq!(bob_hit.organism, far_source);
    }

    #[test]
    fn scene_raycast_respects_body_terrain_occlusion_and_depth_ties() {
        let chunks = flat_chunk_manager(255);
        let source = test_organism(7, 0, 0, 1.0);
        let ray = normalized_ray([32.0, 48.0, 20.0], [0.0, 0.0, -1.0]);

        let behind = retained_organisms([(
            source,
            test_visual(PovOrganismPrimitive::Box, [32.0, 48.0, 9.0], [1.0; 3]),
        )]);
        assert!(matches!(
            raycast_scene(&chunks, &behind, ray, 0.0, 100.0),
            Some(PovSceneHit::Terrain(_))
        ));

        let nearer = retained_organisms([(
            source,
            test_visual(PovOrganismPrimitive::Box, [32.0, 48.0, 12.0], [2.0; 3]),
        )]);
        assert!(matches!(
            raycast_scene(&chunks, &nearer, ray, 0.0, 100.0),
            Some(PovSceneHit::Organism(hit)) if hit.organism == source
        ));

        let tied = retained_organisms([(
            source,
            test_visual(PovOrganismPrimitive::Box, [32.0, 48.0, 9.5], [1.0; 3]),
        )]);
        let hit = raycast_scene(&chunks, &tied, ray, 0.0, 100.0).expect("tie hit");
        assert!(matches!(hit, PovSceneHit::Terrain(_)));
        assert_close(hit.distance(), 10.0, 1.0e-12);
    }

    #[test]
    fn resident_and_visual_generations_change_only_with_pick_geometry_or_source() {
        let map = settled_map();
        let mut chunks = PovChunkManager::new();
        assert_eq!(chunks.resident_generation(), 0);
        let (uploads, _) = chunks.sync(&map, (0.0, 0.0), 0, &InlineExecutor);
        assert_eq!(uploads.len(), 1);
        let integrated = chunks.resident_generation();
        assert_eq!(integrated, 1);
        let (uploads, removes) = chunks.sync(&map, (0.0, 0.0), 0, &InlineExecutor);
        assert!(uploads.is_empty() && removes.is_empty());
        assert_eq!(chunks.resident_generation(), integrated);

        let chunks = flat_chunk_manager(255);
        let mut organism = test_organism(42, 2, 0, 1.0);
        let mut manager = PovOrganismManager::new();
        assert_eq!(manager.visual_generation(), 0);
        assert!(manager.sync_organisms(core::iter::once(&organism), &chunks, (0.0, 0.0), 1_000.0,));
        let first = manager.visual_generation();
        let instances = manager.upload().boxes.clone();
        assert!(!manager.sync_organisms(
            core::iter::once(&organism),
            &chunks,
            (0.01, 0.01),
            1_000.0,
        ));
        assert_eq!(manager.visual_generation(), first);

        // Species is panel-visible source identity but does not alter this
        // visual's primitive/transform/color. It must still dirty picking.
        organism.species ^= 0xDEAD_BEEF;
        assert!(manager.sync_organisms(
            core::iter::once(&organism),
            &chunks,
            (0.01, 0.01),
            1_000.0,
        ));
        assert_eq!(manager.visual_generation(), first + 1);
        assert_eq!(manager.upload().boxes, instances);
    }

    #[test]
    fn organism_forms_exhaust_primitive_and_proportion_mapping() {
        let ground = GroundSurface {
            height: 10.0,
            ambient_occlusion: 137,
        };
        // Keep the acceptance table independent of the production constant:
        // a reordered or mistyped pair must fail even if box and sphere still
        // happen to agree with one another.
        let expected_proportions = [
            [1.42, 0.50],
            [1.26, 0.63],
            [1.13, 0.79],
            [1.00, 1.00],
            [0.89, 1.25],
            [0.82, 1.48],
            [0.76, 1.74],
            [0.69, 2.08],
        ];
        let mut ratios = [0.0f32; 8];
        for shape in 0u8..8 {
            let box_visual = organism_visual(&test_organism(7, 0, shape << 1, 2.0), ground);
            let sphere_visual =
                organism_visual(&test_organism(7, 0, (shape << 1) | 1, 2.0), ground);
            assert_eq!(box_visual.primitive, PovOrganismPrimitive::Box);
            assert_eq!(sphere_visual.primitive, PovOrganismPrimitive::Sphere);
            assert_eq!(box_visual.scale, sphere_visual.scale);
            let [xy, z] = expected_proportions[usize::from(shape)];
            assert_eq!(box_visual.scale, [2.0 * xy, 2.0 * xy, 2.0 * z]);
            ratios[usize::from(shape)] = box_visual.scale[2] / box_visual.scale[0];
            assert_eq!(box_visual.ambient_occlusion, 137);
        }
        assert!(ratios.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn organism_size_grounding_yaw_and_color_are_exact_presentation_inputs() {
        let ground = GroundSurface {
            height: -12.5,
            ambient_occlusion: 201,
        };
        let organism = test_organism(0xabcd, 3, 12, 0.1);
        let small = organism_visual(&organism, ground);
        let large = organism_visual(&test_organism(0xabcd, 3, 12, 12.8), ground);
        for axis in 0..3 {
            assert!((large.scale[axis] / small.scale[axis] - 128.0).abs() < 1e-4);
            assert!(small.scale[axis].is_finite() && large.scale[axis].is_finite());
        }
        assert_eq!(
            small.position[2] - 0.5 * f64::from(small.scale[2]),
            f64::from(ground.height)
        );
        assert_eq!(small.yaw, organism_visual(&organism, ground).yaw);
        assert_eq!(
            small.yaw,
            core::f32::consts::TAU * (f32::from(0xabcdu16) / 65_536.0)
        );
        assert!((0.0..core::f32::consts::TAU).contains(&small.yaw));
        let other_yaw = organism_visual(&test_organism(0xffff, 3, 12, 0.1), ground);
        assert!(other_yaw.yaw < core::f32::consts::TAU);
        assert_eq!(other_yaw.scale, small.scale);
        assert_eq!(other_yaw.color, small.color);
        let rgb = expressed_color(&organism.expressed);
        assert_eq!(&small.color[..3], &rgb);
        assert_eq!(small.color[3], 0);
        let mut producer = organism;
        producer.trophic = Trophic::Producer;
        let producer = organism_visual(&producer, ground);
        assert_eq!(
            &producer.color[..3],
            &rgb,
            "producer cue cannot alter base RGB"
        );
        assert_eq!(producer.color[3], 1);
        assert_eq!(producer.scale, small.scale);
    }

    #[test]
    fn ground_surface_interpolates_height_and_ao_on_one_triangle_topology() {
        let mut chunks = flat_chunk_manager(255);
        let entry = chunks.chunks.get_mut(&RegionCoord::new(0, 0)).unwrap();
        entry.heights[..].fill(0.0);
        entry.ambient_occlusion[..].fill(0);
        // First cell: distinct corners make the selected diagonal observable.
        entry.heights[0] = 10.0;
        entry.heights[1] = 20.0;
        entry.heights[POV_GRID] = 30.0;
        entry.heights[POV_GRID + 1] = 50.0;
        entry.ambient_occlusion[0] = 10;
        entry.ambient_occlusion[1] = 60;
        entry.ambient_occlusion[POV_GRID] = 110;
        entry.ambient_occlusion[POV_GRID + 1] = 210;

        for &(fx, fy) in &[(0.75, 0.25), (0.25, 0.75), (0.5, 0.5)] {
            let surface = chunks
                .ground_surface(fx * SPACING, fy * SPACING)
                .expect("resident");
            let expected_h = triangle_interpolate([10.0, 20.0, 30.0, 50.0], fx as f32, fy as f32);
            let expected_ao = quantize_light(triangle_interpolate(
                [10.0 / 255.0, 60.0 / 255.0, 110.0 / 255.0, 210.0 / 255.0],
                fx as f32,
                fy as f32,
            ));
            assert_eq!(surface.height.to_bits(), expected_h.to_bits());
            assert_eq!(surface.ambient_occlusion, expected_ao);
            assert_eq!(
                chunks
                    .ground_height(fx * SPACING, fy * SPACING)
                    .unwrap()
                    .to_bits(),
                surface.height.to_bits()
            );
        }
        let all_zero = flat_chunk_manager(0).ground_surface(1.0, 1.0).unwrap();
        let all_full = flat_chunk_manager(255).ground_surface(1.0, 1.0).unwrap();
        assert_eq!(all_zero.ambient_occlusion, 0);
        assert_eq!(all_full.ambient_occlusion, 255);
    }

    #[test]
    fn organism_manager_has_exact_delta_empty_and_unbounded_count_semantics() {
        let chunks = flat_chunk_manager(190);
        let mut initially_empty = PovOrganismManager::new();
        assert!(initially_empty.sync_organisms(core::iter::empty(), &chunks, (0.0, 0.0), 1_000.0));
        assert!(!initially_empty.sync_organisms(core::iter::empty(), &chunks, (0.0, 0.0), 1_000.0));
        let mut organisms = (0..1_701u64)
            .map(|id| {
                let mut organism = test_organism(id, (id % 4) as u16, (id % 16) as u8, 1.0);
                organism.world_pos = (32.0 + (id % 8) as f64, 48.0);
                organism
            })
            .collect::<Vec<_>>();
        let mut manager = PovOrganismManager::new();
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (0.0, 0.0), 1_000.0));
        assert_eq!(manager.counters().drawn(), 1_701, "no fixed upload cap");
        assert_eq!(
            manager.upload().boxes.len() + manager.upload().spheres.len(),
            1_701
        );
        assert_eq!(
            manager.counters().uploaded_bytes,
            1_701 * POV_ORGANISM_INSTANCE_BYTES
        );
        let capacities = (
            manager.upload().boxes.capacity(),
            manager.upload().spheres.capacity(),
            manager.box_scratch.capacity(),
            manager.sphere_scratch.capacity(),
        );
        assert!(!manager.sync_organisms(organisms.iter(), &chunks, (0.1, 0.1), 1_000.0));
        assert_eq!(manager.counters().rebuilds, 1);
        assert_eq!(
            capacities,
            (
                manager.upload().boxes.capacity(),
                manager.upload().spheres.capacity(),
                manager.box_scratch.capacity(),
                manager.sphere_scratch.capacity(),
            ),
            "steady scan reuses every instance vector"
        );
        organisms[0].expressed.size = 2.0;
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (0.1, 0.1), 1_000.0));
        assert_eq!(manager.counters().rebuilds, 2);
        let mut added_slot = test_organism(9_999, 7, 3, 1.0);
        added_slot.world_pos = (40.0, 40.0);
        organisms.push(added_slot);
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (0.1, 0.1), 1_000.0));
        assert_eq!(manager.counters().drawn(), 1_702);
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (1.0e6, 1.0e6), 1.0));
        assert_eq!(manager.counters().drawn(), 0);
        assert!(manager.upload().boxes.is_empty() && manager.upload().spheres.is_empty());
        assert!(!manager.sync_organisms(organisms.iter(), &chunks, (1.0e6, 1.0e6), 1.0));
    }

    #[test]
    fn organism_manager_waits_for_ground_then_places_and_sorts_stably() {
        let mut organisms = [
            test_organism(30, 2, 1, 1.0),
            test_organism(10, 3, 1, 1.0),
            test_organism(10, 1, 1, 1.0),
        ];
        let mut manager = PovOrganismManager::new();
        assert!(manager.sync_organisms(
            organisms.iter(),
            &PovChunkManager::new(),
            (0.0, 0.0),
            1_000.0
        ));
        assert_eq!(manager.counters().waiting_for_ground, 3);
        assert_eq!(manager.counters().drawn(), 0);

        let mut chunks = flat_chunk_manager(77);
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (0.0, 0.0), 1_000.0));
        assert_eq!(manager.counters().waiting_for_ground, 0);
        assert_eq!(manager.counters().spheres, 3);
        assert_eq!(
            manager
                .sphere_keys
                .iter()
                .map(|key| (key.id, key.slot))
                .collect::<Vec<_>>(),
            vec![(10, 1), (10, 3), (30, 2)]
        );
        for instance in &manager.upload().spheres {
            assert_eq!(instance.ambient_occlusion, 77);
            assert_eq!(
                instance.position[2] - 0.5 * f64::from(instance.scale[2]),
                10.0
            );
        }
        organisms.reverse();
        assert!(!manager.sync_organisms(organisms.iter(), &chunks, (0.0, 0.0), 1_000.0));
        let chunk = chunks.chunks.get_mut(&RegionCoord::new(0, 0)).unwrap();
        chunk.heights.fill(20.0);
        chunk.ambient_occlusion.fill(88);
        assert!(manager.sync_organisms(organisms.iter(), &chunks, (0.0, 0.0), 1_000.0));
        assert!(manager
            .upload()
            .spheres
            .iter()
            .all(|instance| instance.ambient_occlusion == 88
                && instance.position[2] - 0.5 * f64::from(instance.scale[2]) == 20.0));
    }

    #[test]
    fn organism_distance_cull_keeps_a_body_intersecting_fog() {
        let chunks = flat_chunk_manager(255);
        let mut organism = test_organism(1, 0, 14, 12.8);
        organism.world_pos = (20.0, 0.0);
        let mut manager = PovOrganismManager::new();
        assert!(manager.sync_organisms(core::iter::once(&organism), &chunks, (0.0, 0.0), 10.0));
        assert_eq!(manager.counters().drawn(), 1, "body radius overlaps fog");
        organism.world_pos = (200.0, 0.0);
        assert!(manager.sync_organisms(core::iter::once(&organism), &chunks, (0.0, 0.0), 10.0));
        assert_eq!(manager.counters().drawn(), 0);
        assert_eq!(manager.counters().distance_culled, 1);
    }

    #[test]
    fn organism_manager_matches_independently_filtered_published_map() {
        let map = settled_map();
        let chunks = settled_chunks(&map, (0.0, 0.0), 3);
        let fog_end = pov_fog_end(3);
        let mut expected = map
            .organisms()
            .filter_map(|organism| {
                let scale = organism_scale(organism).map(f64::from);
                let radius = 0.5 * (scale[0].powi(2) + scale[1].powi(2) + scale[2].powi(2)).sqrt();
                let distance = f64::hypot(organism.world_pos.0, organism.world_pos.1);
                (distance <= fog_end + radius
                    && chunks
                        .ground_surface(organism.world_pos.0, organism.world_pos.1)
                        .is_some())
                .then_some((organism.id, organism.slot))
            })
            .collect::<Vec<_>>();
        expected.sort_unstable();

        let mut manager = PovOrganismManager::new();
        assert!(manager.sync(&map, &chunks, (0.0, 0.0), fog_end));
        let mut actual = manager
            .box_keys
            .iter()
            .chain(&manager.sphere_keys)
            .map(|key| (key.id, key.slot))
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        assert_eq!(manager.counters().published, map.organism_count());
        assert_eq!(manager.counters().drawn(), expected.len());
        assert!(!manager.sync(&map, &chunks, (0.01, -0.01), fog_end));
    }

    #[test]
    fn shadow_fit_is_finite_contains_bounds_and_disables_when_empty() {
        assert_eq!(shadow_resolution(ResourceTier::Low), 1024);
        assert_eq!(shadow_resolution(ResourceTier::Mid), 2048);
        assert_eq!(shadow_resolution(ResourceTier::High), 2048);
        assert_eq!(
            flat_chunk_manager(255).shadow_bounds(),
            Some(PovShadowBounds {
                min: [0.0, 0.0, 10.0 - f64::from(POV_SKIRT_DROP)],
                max: [REGION_SIZE, REGION_SIZE, 10.0],
            })
        );
        let camera = glam::DVec3::new(1.0e12, -1.0e12, 700.0);
        let bounds = PovShadowBounds {
            min: [camera.x - 300.0, camera.y - 200.0, -120.0],
            max: [camera.x + 400.0, camera.y + 500.0, 450.0],
        };
        let matrix = fit_shadow_matrix(camera, bounds, 2048);
        assert!(matrix.iter().flatten().all(|value| value.is_finite()));
        let matrix = glam::Mat4::from_cols_array_2d(&matrix);
        for corner in bounds.corners() {
            let relative = (glam::DVec3::from_array(corner) - camera).as_vec3();
            let projected = matrix.project_point3(relative);
            assert!((-1.000_01..=1.000_01).contains(&projected.x));
            assert!((-1.000_01..=1.000_01).contains(&projected.y));
            assert!((-0.000_01..=1.000_01).contains(&projected.z));
        }
        let empty = shadow_frame(&PovCamera::new(), &PovChunkManager::new(), None, 2048);
        assert_eq!(empty, PovShadowFrame::disabled());
    }

    #[test]
    fn shadow_fit_preserves_sun_facing_ccw_winding() {
        let camera = glam::DVec3::new(0.0, 0.0, 100.0);
        let bounds = PovShadowBounds {
            min: [-20.0, -20.0, -10.0],
            max: [20.0, 20.0, 10.0],
        };
        let matrix = glam::Mat4::from_cols_array_2d(&fit_shadow_matrix(camera, bounds, 1024));
        // A +z-facing terrain triangle is wound CCW and faces the fixed sun.
        // Its light-space projection must stay CCW because both shadow
        // pipelines use `FrontFace::Ccw` with back-face culling.
        let project = |point: glam::DVec3| matrix.project_point3((point - camera).as_vec3());
        let a = project(glam::DVec3::new(-5.0, -5.0, 0.0));
        let b = project(glam::DVec3::new(5.0, -5.0, 0.0));
        let c = project(glam::DVec3::new(5.0, 5.0, 0.0));
        let signed_area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert!(signed_area > 0.0, "sun-facing triangle was mirrored");
    }

    #[test]
    fn shadow_fit_snaps_center_and_rounds_extent() {
        let bounds = PovShadowBounds {
            min: [-100.0, -100.0, -50.0],
            max: [100.0, 100.0, 50.0],
        };
        let camera = glam::DVec3::ZERO;
        let first = fit_shadow_matrix(camera, bounds, 1024);
        let forward = glam::DVec3::from_array(SUN_DIR.map(f64::from));
        let right = glam::DVec3::Z.cross(forward).normalize();
        let sub_texel = fit_shadow_matrix(camera + right * 0.01, bounds, 1024);
        assert_eq!(first, sub_texel, "sub-texel light-x motion must be stable");
        let crossed = fit_shadow_matrix(camera + right, bounds, 1024);
        assert_ne!(
            first, crossed,
            "crossing texels advances the snapped center"
        );

        let within_same_extent = PovShadowBounds {
            min: [-110.0, -100.0, -50.0],
            max: [110.0, 100.0, 50.0],
        };
        let wider = PovShadowBounds {
            min: [-500.0, -100.0, -50.0],
            max: [500.0, 100.0, 50.0],
        };
        let same = fit_shadow_matrix(camera, within_same_extent, 1024);
        let wider = fit_shadow_matrix(camera, wider, 1024);
        let xy_scales = |matrix: [[f32; 4]; 4]| {
            [
                (matrix[0][0].powi(2) + matrix[1][0].powi(2) + matrix[2][0].powi(2)).sqrt(),
                (matrix[0][1].powi(2) + matrix[1][1].powi(2) + matrix[2][1].powi(2)).sqrt(),
            ]
        };
        assert_eq!(xy_scales(first), xy_scales(same), "same extent step");
        assert_ne!(xy_scales(first), xy_scales(wider), "extent step crossed");
    }

    #[test]
    fn pov_script_parses_the_documented_forms() {
        let script = "size:640x360; pos:300,-10; mouse:120,-40; snap:a.ppm; \
                      move:200; move:0,-50,25; settle; settle:3; split:both.ppm; snap:b.ppm";
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
                PovInstr::Split(String::from("both.ppm")),
                PovInstr::Snap(String::from("b.ppm")),
            ]
        );
        assert!(parse_pov_script("").is_err());
        assert!(parse_pov_script("teleport:1,2").is_err());
        assert!(parse_pov_script("mouse:1").is_err());
        assert!(parse_pov_script("snap").is_err());
        assert!(parse_pov_script("split").is_err());
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
        assert_eq!(a.river_indices, b.river_indices);
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
                let x = ox + i as f64 * SPACING;
                let y = oy + j as f64 * SPACING;
                let p = snap.terrain_halo.sample_world(x, y);
                let expected = elevation(x, y, &p);
                let got = mesh.vertices[j * POV_GRID + i].position[2];
                assert_eq!(got.to_bits(), expected.to_bits(), "vertex ({i}, {j})");
                assert_eq!(mesh.heights[j * POV_GRID + i].to_bits(), expected.to_bits());
            }
        }
        // Cell centers: same halo sampler and relief/scaling operations as generation.
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
    fn halo_height_rows_edge_extend_only_the_possibility_lookup() {
        let center = RegionCoord::new(-4, 3);
        let mut buckets = [[[0u16; 2]; 3]; 3];
        for (y, row) in buckets.iter_mut().enumerate() {
            for (x, pair) in row.iter_mut().enumerate() {
                let index = (y * 3 + x) as u16;
                *pair = [173 + index * 211, 3_700 - index * 257];
            }
        }
        let halo = TerrainPossibilityHalo::new(center, buckets);
        let (ox, oy) = center.origin();
        let xs = [
            ox - REGION_SIZE * 3.0,
            ox - REGION_SIZE * 0.5,
            ox + REGION_SIZE * 0.375,
            ox + REGION_SIZE * 3.0,
        ];
        let y = oy + REGION_SIZE * 2.5;
        let mut row = [0.0; 4];
        halo_elevation_row(&xs, y, &halo, &mut row);

        let min_x = ox - REGION_SIZE * 0.5;
        let max_x = ox + REGION_SIZE * 1.5;
        let sample_y = oy + REGION_SIZE * 1.5;
        for (x, got) in xs.into_iter().zip(row) {
            let p = halo.sample_world(x.clamp(min_x, max_x), sample_y);
            let expected = elevation(x, y, &p);
            assert_eq!(got.to_bits(), expected.to_bits(), "world x {x}");
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
    fn light_quantization_is_clamped_and_rounded() {
        assert_eq!(quantize_light(0.0), 0);
        assert_eq!(quantize_light(1.0), 255);
        assert_eq!(quantize_light(-0.5), 0, "clamped below");
        assert_eq!(quantize_light(2.0), 255, "clamped above");
    }

    #[test]
    fn direct_light_byte_is_neutral_and_ao_lattice_matches_vertices() {
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        assert_eq!(mesh.ambient_occlusion.len(), CORE_VERTS);
        assert_eq!(
            mesh.min_height.to_bits(),
            mesh.heights
                .iter()
                .copied()
                .reduce(f32::min)
                .unwrap()
                .to_bits()
        );
        assert_eq!(
            mesh.max_height.to_bits(),
            mesh.heights
                .iter()
                .copied()
                .reduce(f32::max)
                .unwrap()
                .to_bits()
        );
        for (index, vertex) in mesh.vertices[..CORE_VERTS].iter().enumerate() {
            assert_eq!(vertex.light[0], 255, "reserved direct byte {index}");
            assert_eq!(vertex.light[1], mesh.ambient_occlusion[index]);
        }
    }

    #[test]
    fn light_bytes_pack_bilinear_river_and_wetness() {
        // 3d-phase-3-plan.md §9 test 2: light.zw carry the same bilinear
        // river/wetness the albedo used, quantized; at cell centers they
        // equal the quantized tile values exactly (bilinear-at-center
        // identity).
        let map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let snap = Snapshot::of(&map, coord);
        let mesh = mesh_region_chunk(&snap.inputs());
        for j in 0..POV_GRID {
            for i in 0..POV_GRID {
                let (lx, ly) = (i as f64 * SPACING, j as f64 * SPACING);
                let v = mesh.vertices[j * POV_GRID + i];
                assert_eq!(
                    v.light[2],
                    quantize_light(bilinear(&snap.river, lx, ly)),
                    "river byte at ({i}, {j})"
                );
                assert_eq!(
                    v.light[3],
                    quantize_light(bilinear(&snap.wetness, lx, ly)),
                    "wetness byte at ({i}, {j})"
                );
            }
        }
        let res = map.config().field_resolution;
        let stride = POV_GRID / usize::from(res);
        for cy in 0..res {
            for cx in 0..res {
                let i = usize::from(cx) * stride + stride / 2;
                let j = usize::from(cy) * stride + stride / 2;
                let v = mesh.vertices[j * POV_GRID + i];
                assert_eq!(v.light[2], quantize_light(snap.river.get(cx, cy)));
                assert_eq!(v.light[3], quantize_light(snap.wetness.get(cx, cy)));
            }
        }
    }

    #[test]
    fn river_overlay_selection_matches_the_rule_and_the_drawn_topology() {
        // 3d-phase-3-plan.md §9 test 3. Synthetic lattices exercise the rule
        // edges; the drawn-topology containment guard runs on a real mesh.
        let flat = vec![10.0f32; CORE_VERTS]; // all land
        let dry = vec![0.0f32; CORE_VERTS];
        assert!(
            river_overlay_indices(&dry, &flat).is_empty(),
            "an all-zero river lattice emits no overlay"
        );
        let wet = vec![1.0f32; CORE_VERTS];
        assert_eq!(
            river_overlay_indices(&wet, &flat).len(),
            POV_MESH_RES * POV_MESH_RES * 2 * 3,
            "saturated river over land emits every core triangle"
        );
        let sunken = vec![-5.0f32; CORE_VERTS];
        assert!(
            river_overlay_indices(&wet, &sunken).is_empty(),
            "fully submerged cells are already under the sea plane"
        );
        // A single wet vertex pulls in exactly the triangles that touch it
        // (any-corner rule): vertex (1, 1) sits on 6 core triangles.
        let mut spot = dry;
        spot[POV_GRID + 1] = RIVER_OVERLAY_MIN;
        let indices = river_overlay_indices(&spot, &flat);
        assert_eq!(indices.len(), 6 * 3);
        assert!(indices
            .chunks_exact(3)
            .all(|t| t.contains(&((POV_GRID + 1) as u32))));

        // Every emitted triple of a real mesh is a core triangle of the
        // shared drawn topology, same order and winding — the guard that the
        // overlay and the terrain share one diagonal split.
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let mesh = mesh_region_chunk(&snap.inputs());
        let indices = chunk_indices();
        let core: std::collections::HashSet<[u32; 3]> = indices[..POV_MESH_RES * POV_MESH_RES * 6]
            .chunks_exact(3)
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        for tri in mesh.river_indices.chunks_exact(3) {
            assert!(
                core.contains(&[tri[0], tri[1], tri[2]]),
                "overlay triangle {tri:?} is not a drawn core triangle"
            );
        }
    }

    #[test]
    fn underwater_vertices_use_sediment_and_drop_wet_bytes() {
        // 3D-3 follow-up: the sea floor gets a real sediment ramp instead of
        // the 2D map's blue depth legend, and its river/wetness bytes are
        // zeroed (the sea surface owns the specular there). Find a settled
        // coastal region so both branches assert.
        use world_runtime::mapcolor::pov_sediment_color;
        let map = settled_map();
        let mut straddling = None;
        'search: for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                let coord = RegionCoord::new(dx, dy);
                if map.cache().get(coord).is_none() || map.terrain_possibility_halo(coord).is_none()
                {
                    continue;
                }
                let snap = Snapshot::of(&map, coord);
                let mesh = mesh_region_chunk(&snap.inputs());
                let wet = mesh.heights.iter().any(|&e| e < world_core::SEA_LEVEL);
                let dry = mesh.heights.iter().any(|&e| e >= world_core::SEA_LEVEL);
                if wet && dry {
                    straddling = Some((snap, mesh));
                    break 'search;
                }
            }
        }
        let (snap, mesh) = straddling.expect("a settled coastal region in the fixture window");
        let mut submerged = 0usize;
        for j in 0..POV_GRID {
            for i in 0..POV_GRID {
                let v = mesh.vertices[j * POV_GRID + i];
                let e = v.position[2];
                let (lx, ly) = (i as f64 * SPACING, j as f64 * SPACING);
                if e < world_core::SEA_LEVEL {
                    submerged += 1;
                    let rgb = pov_sediment_color(e);
                    assert_eq!([v.color[0], v.color[1], v.color[2]], rgb);
                    assert_eq!(v.light[2], 0, "submerged river byte at ({i}, {j})");
                    assert_eq!(v.light[3], 0, "submerged wetness byte at ({i}, {j})");
                } else {
                    assert_eq!(v.light[2], quantize_light(bilinear(&snap.river, lx, ly)));
                    assert_eq!(v.light[3], quantize_light(bilinear(&snap.wetness, lx, ly)));
                }
            }
        }
        assert!(submerged > 0, "the straddling search found underwater land");
    }

    #[test]
    fn frame_params_carry_time_sea_plane_and_toggles() {
        // 3d-phase-3-plan.md §9 test 6: water_z = SEA_LEVEL − camera.z in
        // f64, time passes through verbatim (captures pass 0.0), and the
        // diagnostic toggles ride along.
        let mut cam = PovCamera::new();
        cam.pos = glam::DVec3::new(1.0e6, -2.0e6, 137.5);
        let params = frame_params(
            &cam,
            1.5,
            3,
            [0.1, 0.2, 0.3, 1.0],
            7.25,
            PovToggles::default(),
            PovShadowFrame::disabled(),
        );
        assert_eq!(params.time, 7.25);
        assert_eq!(
            params.water_z,
            (f64::from(world_core::SEA_LEVEL) - 137.5) as f32
        );
        assert!(params.shadow_ao && params.detail_normals && params.water);
        assert_eq!(params.shadow_resolution, 0);
        let off = frame_params(
            &cam,
            1.5,
            3,
            [0.0; 4],
            0.0,
            PovToggles {
                shadow_ao: false,
                detail_normals: false,
                water: false,
            },
            PovShadowFrame::disabled(),
        );
        assert_eq!(off.time, 0.0);
        assert!(!off.shadow_ao && !off.detail_normals && !off.water);
    }

    #[test]
    fn cancelled_mesh_returns_none() {
        let map = settled_map();
        let snap = Snapshot::of(&map, RegionCoord::new(0, 0));
        let cancelled = AtomicBool::new(true);
        assert!(mesh_region_chunk_cancellable(&snap.inputs(), &cancelled).is_none());
    }

    #[test]
    fn chunk_key_folds_atlas_and_owned_halo_provenance() {
        let map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let provenance = chunk_provenance(&map, coord).expect("complete inputs");
        let mut expected = mix(
            map.presentation_key(coord).expect("atlas provenance"),
            0x504F_565F_4841_4C4F,
        );
        for bucket in provenance.terrain_halo.dependency_buckets() {
            expected = mix(expected, u64::from(bucket));
        }
        assert_eq!(provenance.key, expected);
    }

    #[test]
    fn neighbor_source_flip_supersedes_mesh_before_terrain_integration() {
        // A completed old mesh must not publish after a neighbor's P/G
        // authority changes, even though this region's stored Terrain and
        // downstream tiles still carry their old hashes. The replacement job
        // owns the same new halo snapshot folded into its key.
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

        let mut map = settled_map();
        let coord = RegionCoord::new(0, 0);
        let source = RegionCoord::new(1, 0);
        let executor = QueueExecutor {
            jobs: RefCell::new(Vec::new()),
        };
        let mut manager = PovChunkManager::new();
        let old_atlas_key = map.presentation_key(coord).expect("settled atlas");
        let old_key = chunk_provenance(&map, coord).expect("settled chunk").key;

        let (uploads, _) = manager.sync(&map, (0.0, 0.0), 0, &executor);
        assert!(uploads.is_empty());
        assert_eq!(executor.jobs.borrow().len(), 1);
        executor.jobs.borrow_mut().pop().expect("old job")();

        let mut changed = PossibilitySignature::of(map.get(source).expect("source").current);
        for domain in [PossibilityDomain::Planetary, PossibilityDomain::Geology] {
            let bucket = &mut changed.buckets[domain.index()];
            *bucket = if *bucket == 0 {
                POSSIBILITY_QUANT - 1
            } else {
                0
            };
        }
        map.apply_preserve_contribution(7, source, changed);

        assert_eq!(
            map.presentation_key(coord),
            Some(old_atlas_key),
            "source flip precedes corrected Terrain integration"
        );
        let new = chunk_provenance(&map, coord).expect("new halo provenance");
        assert_ne!(new.key, old_key);
        let expected_halo = new.terrain_halo.clone();

        let (uploads, _) = manager.sync(&map, (0.0, 0.0), 0, &executor);
        assert!(uploads.is_empty(), "completed old-key mesh must be dropped");
        assert_eq!(manager.counters().cancelled, 1);
        assert_eq!(manager.counters().dropped_stale, 1);
        assert_eq!(executor.jobs.borrow().len(), 1, "replacement scheduled");
        assert_eq!(
            manager.in_flight.get(&coord).expect("replacement").key,
            new.key
        );

        executor.jobs.borrow_mut().pop().expect("replacement job")();
        let (uploads, _) = manager.sync(&map, (0.0, 0.0), 0, &executor);
        assert_eq!(uploads.len(), 1);
        assert_eq!(manager.chunks.get(&coord).expect("published").key, new.key);
        let origin = coord.origin();
        let expected = elevation(
            origin.0,
            origin.1,
            &expected_halo.sample_world(origin.0, origin.1),
        );
        assert_eq!(
            uploads[0].vertices[0].position[2].to_bits(),
            expected.to_bits()
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
            ambient_occlusion: vec![255; CORE_VERTS],
            min_height: 0.0,
            max_height: 0.0,
            river_indices: Vec::new(),
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
                    ambient_occlusion: Vec::new(),
                    min_height: 0.0,
                    max_height: 0.0,
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

    // -- 3D-2: the ground query and walk kinematics (3d-phase-2-plan.md §8) --

    /// A chunk manager settled over `map` within `radius` of `camera`
    /// (inline executor; enough syncs to pass the upload amortization cap).
    fn settled_chunks(map: &RegionMap, camera: (f64, f64), radius: i32) -> PovChunkManager {
        let mut manager = PovChunkManager::new();
        for _ in 0..16 {
            let _ = manager.sync(map, camera, radius, &InlineExecutor);
        }
        assert!(manager.is_idle(), "chunk ring must settle");
        manager
    }

    /// z of the plane through `a`, `b`, `c` at `(px, py)`, via barycentric
    /// weights — deliberately not the interpolation formula under test.
    fn plane_z(a: [f64; 3], b: [f64; 3], c: [f64; 3], px: f64, py: f64) -> f64 {
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        let wb = ((px - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (py - a[1])) / det;
        let wc = ((b[0] - a[0]) * (py - a[1]) - (px - a[0]) * (b[1] - a[1])) / det;
        (1.0 - wb - wc) * a[2] + wb * b[2] + wc * c[2]
    }

    /// Whether `(px, py)` lies in the 2D projection of triangle `a, b, c`.
    fn triangle_contains(a: [f64; 3], b: [f64; 3], c: [f64; 3], px: f64, py: f64) -> bool {
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        let wb = ((px - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (py - a[1])) / det;
        let wc = ((b[0] - a[0]) * (py - a[1]) - (px - a[0]) * (b[1] - a[1])) / det;
        wb >= -1e-9 && wc >= -1e-9 && wb + wc <= 1.0 + 1e-9
    }

    #[test]
    fn ground_height_is_vertex_exact() {
        // Plan §8 test 1: at every core lattice position the query returns
        // exactly the stored height (which 3D-1's tests pin to halo-sampled
        // elevation()). Radius 1 so the far border columns — owned by the
        // eastern/northern neighbors under floor semantics — resolve too;
        // their bit-exact agreement with this chunk's own column is the
        // ADR 0027 steady-state border-continuity claim, asserted exactly.
        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 1);
        let coord = RegionCoord::new(0, 0);
        let heights = manager
            .chunks
            .get(&coord)
            .expect("resident")
            .heights
            .clone();
        let (ox, oy) = coord.origin();
        for j in 0..POV_GRID {
            for i in 0..POV_GRID {
                let got = manager
                    .ground_height(ox + i as f64 * SPACING, oy + j as f64 * SPACING)
                    .expect("covering chunk resident");
                assert_eq!(
                    got.to_bits(),
                    heights[j * POV_GRID + i].to_bits(),
                    "vertex ({i}, {j})"
                );
            }
        }
    }

    #[test]
    fn ground_height_mid_cell_is_bounded_and_planar() {
        // Plan §8 test 2: interior points stay within the cell's corner
        // range and equal the containing triangle's plane to f32 round-off.
        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 0);
        let coord = RegionCoord::new(0, 0);
        let heights = &manager.chunks.get(&coord).expect("resident").heights;
        let (ox, oy) = coord.origin();
        for &(i, j) in &[(0usize, 0usize), (10, 20), (31, 7), (63, 63)] {
            let h00 = heights[j * POV_GRID + i];
            let h10 = heights[j * POV_GRID + i + 1];
            let h01 = heights[(j + 1) * POV_GRID + i];
            let h11 = heights[(j + 1) * POV_GRID + i + 1];
            let lo = h00.min(h10).min(h01).min(h11);
            let hi = h00.max(h10).max(h01).max(h11);
            for &(fx, fy) in &[
                (0.25, 0.125),
                (0.7, 0.3),
                (0.3, 0.7),
                (0.5, 0.5),
                (0.9, 0.95),
            ] {
                let x = ox + (i as f64 + fx) * SPACING;
                let y = oy + (j as f64 + fy) * SPACING;
                let got = manager.ground_height(x, y).expect("resident");
                assert!(
                    got >= lo - 1e-3 && got <= hi + 1e-3,
                    "cell ({i}, {j}) at ({fx}, {fy}): {got} outside [{lo}, {hi}]"
                );
                let (a, b, c) = if fx >= fy {
                    // South-east triangle v00, v10, v11 (unit cell coords).
                    (
                        [0.0, 0.0, f64::from(h00)],
                        [1.0, 0.0, f64::from(h10)],
                        [1.0, 1.0, f64::from(h11)],
                    )
                } else {
                    // North-west triangle v00, v11, v01.
                    (
                        [0.0, 0.0, f64::from(h00)],
                        [1.0, 1.0, f64::from(h11)],
                        [0.0, 1.0, f64::from(h01)],
                    )
                };
                let expected = plane_z(a, b, c, fx, fy);
                assert!(
                    (f64::from(got) - expected).abs() < 1e-3,
                    "cell ({i}, {j}) at ({fx}, {fy}): {got} vs plane {expected}"
                );
            }
        }
    }

    #[test]
    fn ground_height_agrees_with_the_rendered_topology() {
        // Plan §8 test 3, the diagonal-drift guard: derive the expected
        // surface from chunk_indices() itself — find the core triangle whose
        // 2D projection contains each probe and evaluate its plane. If
        // either side flips its cell split, this fails loudly instead of
        // silently de-synchronizing collision from visuals.
        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 0);
        let coord = RegionCoord::new(0, 0);
        let snap = Snapshot::of(&map, coord);
        let mesh = mesh_region_chunk(&snap.inputs());
        let indices = chunk_indices();
        let core = POV_MESH_RES * POV_MESH_RES * 6;
        let (ox, oy) = coord.origin();
        let vertex = |v: u32| {
            let p = mesh.vertices[v as usize].position;
            [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]
        };
        for a in 0..20 {
            for b in 0..20 {
                let (lx, ly) = (a as f64 * 12.7 + 0.45, b as f64 * 12.7 + 0.85);
                let got = f64::from(
                    manager
                        .ground_height(ox + lx, oy + ly)
                        .expect("chunk resident"),
                );
                let expected = indices[..core]
                    .chunks_exact(3)
                    .find_map(|tri| {
                        let (ta, tb, tc) = (vertex(tri[0]), vertex(tri[1]), vertex(tri[2]));
                        triangle_contains(ta, tb, tc, lx, ly).then(|| plane_z(ta, tb, tc, lx, ly))
                    })
                    .expect("probe inside a core triangle");
                assert!(
                    (got - expected).abs() < 1e-3,
                    "probe ({lx}, {ly}): {got} vs rendered {expected}"
                );
            }
        }
    }

    #[test]
    fn ground_height_is_continuous_across_edges_diagonal_and_border() {
        // Plan §8 test 4: along a transect crossing cell edges, the cell
        // diagonals, and the region border at x = 256 (chunks (0,0) and
        // (1,0), both settled), consecutive samples differ by O(ε · slope) —
        // no jumps. (Bit-exact border-column agreement is asserted by the
        // vertex-exactness test; this asserts the interpolant numerically.)
        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 1);
        let (ox, oy) = RegionCoord::new(0, 0).origin();
        let eps = 0.01;
        let mut prev: Option<f64> = None;
        let mut t = 0.0;
        while t <= 32.0 {
            let x = ox + 240.0 + t;
            let y = oy + 100.5 + 0.37 * t;
            let h = f64::from(manager.ground_height(x, y).expect("both chunks resident"));
            if let Some(prev) = prev {
                assert!((h - prev).abs() < 0.1, "jump at t = {t}: {prev} -> {h}");
            }
            prev = Some(h);
            t += eps;
        }
    }

    #[test]
    fn frontier_fallback_is_analytic_and_entry_ground_keeps_its_clamp() {
        // Plan §8 test 5: no chunk ⇒ None; walk_ground composes the mesh
        // first, the analytic fallback at the frontier; analytic_ground is
        // unclamped halo-sampled elevation() (neutral off-map); and the
        // entry_ground refactor is pinned as analytic.max(SEA_LEVEL).
        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 0);
        let frontier = (3.5 * REGION_SIZE, 8.0);
        assert!(manager.ground_height(frontier.0, frontier.1).is_none());
        let (g, mesh) = walk_ground(&manager, &map, frontier);
        assert!(!mesh, "frontier ground is analytic");
        assert_eq!(g, analytic_ground(&map, frontier));
        let (g, mesh) = walk_ground(&manager, &map, (10.0, 20.0));
        assert!(mesh, "resident chunk answers from the mesh");
        assert_eq!(
            g,
            f64::from(manager.ground_height(10.0, 20.0).expect("resident"))
        );

        let far = (1.0e6, -1.0e6);
        assert!(
            map.terrain_possibility_halo(RegionCoord::from_world(far.0, far.1))
                .is_none(),
            "far position is really off-map"
        );
        for world in [(10.0, 20.0), frontier, far] {
            let coord = RegionCoord::from_world(world.0, world.1);
            let p = map
                .terrain_possibility_halo(coord)
                .map_or_else(PossibilityVector::neutral, |halo| {
                    halo.sample_world(world.0, world.1)
                });
            let expected = f64::from(elevation(world.0, world.1, &p));
            assert_eq!(analytic_ground(&map, world), expected);
            assert_eq!(
                entry_ground(&map, world),
                expected.max(f64::from(world_core::SEA_LEVEL))
            );
        }
    }

    #[test]
    fn walk_kinematics_follow_snap_and_round_trip() {
        // Plan §8 test 6: pure camera math against stub grounds.
        let mut cam = PovCamera::new();
        cam.pos = glam::DVec3::new(10.0, 20.0, 100.0);
        cam.yaw = 0.7;
        cam.pitch = -1.2;
        // Pitch must not affect the walk basis: horizontal, unit length.
        let fwd = cam.walk_forward();
        assert_eq!(fwd.z, 0.0);
        assert!((fwd.length() - 1.0).abs() < 1e-6); // f32 sin/cos, like look()
        assert!((fwd.x - f64::from(cam.yaw.cos())).abs() < 1e-9);
        cam.pitch = 0.9;
        assert_eq!(cam.walk_forward(), fwd, "walk basis ignores pitch");

        // Follow step: exact within the clamp, rate-limited beyond it.
        cam.walk = true;
        let dt = 0.1;
        let max_step = POV_CLIMB_FACTOR * cam.walk_speed * dt;
        cam.pos.z = 100.0;
        cam.follow_ground(100.5, dt);
        assert_eq!(cam.pos.z, 100.5, "within the clamp: exact following");
        cam.pos.z = 0.0;
        cam.follow_ground(1000.0, dt);
        assert_eq!(cam.pos.z, max_step, "rate-limited climbing");
        cam.pos.z = 0.0;
        cam.follow_ground(-1000.0, dt);
        assert_eq!(cam.pos.z, -max_step, "rate-limited descending");

        // `F` snap grounds immediately; fly→walk→fly keeps the pose.
        let mut cam = PovCamera::new();
        cam.pos = glam::DVec3::new(-4.0, 9.0, 600.0);
        cam.yaw = 1.3;
        cam.pitch = -0.4;
        let (x, y, yaw, pitch) = (cam.pos.x, cam.pos.y, cam.yaw, cam.pitch);
        cam.set_walk(true, 42.0);
        assert!(cam.walk);
        assert_eq!(cam.pos.z, 42.0 + EYE_HEIGHT, "instant grounding");
        cam.set_walk(false, -999.0);
        assert!(!cam.walk);
        assert_eq!(
            (cam.pos.x, cam.pos.y, cam.yaw, cam.pitch),
            (x, y, yaw, pitch)
        );
        assert_eq!(cam.pos.z, 42.0 + EYE_HEIGHT, "leaving walk keeps position");
    }

    #[test]
    fn wheel_adjusts_only_the_active_modes_speed() {
        // Plan §8 test 6 (wheel): each mode keeps its own scroll-tuned
        // speed, clamped to its own range.
        let mut cam = PovCamera::new();
        assert!(!cam.walk);
        cam.scroll_speed(true);
        assert!(cam.speed > POV_FLY_SPEED);
        assert_eq!(cam.walk_speed, POV_WALK_SPEED, "fly scroll leaves walk");
        cam.walk = true;
        let fly_speed = cam.speed;
        for _ in 0..100 {
            cam.scroll_speed(true);
        }
        assert_eq!(cam.speed, fly_speed, "walk scroll leaves fly");
        assert_eq!(cam.walk_speed, POV_WALK_SPEED_RANGE.1, "clamped above");
        for _ in 0..200 {
            cam.scroll_speed(false);
        }
        assert_eq!(cam.walk_speed, POV_WALK_SPEED_RANGE.0, "clamped below");
    }

    #[test]
    fn pov_script_walk_fly_and_walk_move_snap() {
        // Plan §8 test 7: parsing, and the walk-mode `move` snap semantics
        // driven through the parsed instructions against a settled map.
        let parsed = parse_pov_script("pos:10,20; walk; move:10,5,7; fly; move:10; snap:x.ppm")
            .expect("valid");
        assert_eq!(parsed[1], PovInstr::Walk);
        assert_eq!(parsed[3], PovInstr::Fly);
        assert!(parse_pov_script("walk:1").is_err(), "walk takes no args");
        assert!(parse_pov_script("fly:now").is_err(), "fly takes no args");

        let map = settled_map();
        let manager = settled_chunks(&map, (0.0, 0.0), 1);
        let mut ground = |x: f64, y: f64| walk_ground(&manager, &map, (x, y)).0;
        let mut camera = PovCamera::new();
        camera.pos = glam::DVec3::new(10.0, 20.0, 500.0);
        camera.look(-80.0, 260.0); // yaw off-axis, pitch well below level
        assert!(camera.pitch < -0.5);

        // walk: the F-key toggle path, snapping to the mesh ground.
        assert!(apply_camera_instr(&mut camera, &parsed[1], &mut ground));
        assert!(camera.walk);
        let (g0, mesh) = walk_ground(&manager, &map, (10.0, 20.0));
        assert!(mesh);
        assert_eq!(camera.pos.z, g0 + EYE_HEIGHT);

        // move:10,5,7 in walk mode: the walk basis (pitch-independent),
        // `u` ignored, then the snap at the destination.
        let expected_xy = camera.pos + (camera.walk_forward() * 10.0 + camera.right() * 5.0);
        assert!(apply_camera_instr(&mut camera, &parsed[2], &mut ground));
        assert_eq!(camera.pos.x, expected_xy.x);
        assert_eq!(camera.pos.y, expected_xy.y);
        let g1 = ground(camera.pos.x, camera.pos.y);
        assert_eq!(camera.pos.z, g1 + EYE_HEIGHT);

        // fly: pose kept exactly, both ways.
        let before = camera.pos;
        let (yaw, pitch) = (camera.yaw, camera.pitch);
        assert!(apply_camera_instr(&mut camera, &parsed[3], &mut ground));
        assert!(!camera.walk);
        assert_eq!(camera.pos, before);
        assert_eq!((camera.yaw, camera.pitch), (yaw, pitch));

        // move:10 back in fly mode pitches with the view again.
        let v = camera.forward() * 10.0 + camera.right() * 0.0 + glam::DVec3::new(0.0, 0.0, 0.0);
        let expected = camera.pos + v;
        assert!(apply_camera_instr(&mut camera, &parsed[4], &mut ground));
        assert_eq!(camera.pos, expected);

        // Non-camera instructions are the runner's concern.
        assert!(!apply_camera_instr(&mut camera, &parsed[0], &mut ground));
        assert!(!apply_camera_instr(&mut camera, &parsed[5], &mut ground));
        assert!(!apply_camera_instr(
            &mut camera,
            &PovInstr::Settle(1),
            &mut ground
        ));
    }
}
