//! The POV terrain path (3d-phase-1-plan.md §5): depth-correct, vertex-lit
//! terrain chunks under a fly camera.
//!
//! The renderer stays world-agnostic and upload-only, exactly the
//! [`GpuMap`](crate::gpumap::GpuMap)/`AtlasManager` split: it learns about
//! vertices and opaque chunk handles, never regions or tiles. All world
//! reading and meshing lives in the shell; this module owns the pipeline, the
//! depth target, the shared index buffer, and a pooled vertex-buffer table.
//! No readback API exists on this path (ADR 0017).
//!
//! Because every chunk is the same uniform grid, the index topology is
//! identical for all chunks: it is built once (core grid + perimeter skirt,
//! plan §6.5) and shared by every draw, and every vertex buffer has the same
//! fixed size — which is what makes pooling trivial (§5.5).

use std::collections::HashMap;

/// The POV terrain shader (vertex-lit, fogged; plan §5.6).
pub const SHADER_POV_TERRAIN: &str = include_str!("../shaders/pov_terrain.wgsl");

/// The POV water shader (3d-phase-3-plan.md): the sea plane and the river
/// overlay, both translucent, depth-tested, depth-write-off.
pub const SHADER_POV_WATER: &str = include_str!("../shaders/pov_water.wgsl");

/// The rigid organism shader shared by the box and two-subdivision icosphere
/// batches (3d-phase-4-plan.md §6.9). Both its color and depth-only entry
/// points use the same transform, so caster and visible silhouettes agree.
pub const SHADER_POV_ORGANISM: &str = include_str!("../shaders/pov_organism.wgsl");

/// The water wobble's time period in seconds (3d-phase-3-plan.md §4.3). The
/// shell wraps its clock at this period before filling
/// [`PovFrameParams::time`]; every wobble frequency in `pov_water.wgsl` is an
/// integer number of cycles per period, so the wrap is seamless and f32 never
/// accumulates precision loss. A property of the shader, owned here.
pub const WOBBLE_PERIOD: f32 = 32.0;

/// Spatial tiling of the wobble in world units: `write_frame` anchors the
/// sea's wobble with `camera mod WOBBLE_TILE` computed in f64 (the same
/// far-from-origin discipline as the chunk offsets), and every wobble
/// wavelength in `pov_water.wgsl` divides this, so anchor jumps at tile
/// crossings are whole periods — invisible (3d-phase-3-plan.md §4.3).
const WOBBLE_TILE: f64 = 64.0;

/// Detail octaves the fragment shader continues above the authoritative
/// terrain spectrum for **normal perturbation only** — the POV analogue of
/// the map's refinement octaves (ADR 0017: derived presentation; vertices
/// are never displaced, so the CPU heightfield stays the ground truth).
pub const DETAIL_OCTAVES: usize = 3;

/// Quads per region edge (4.0-unit spacing at `REGION_SIZE = 256`).
pub const POV_MESH_RES: usize = 64;

/// Vertices per chunk edge (the quad lattice plus one).
pub const POV_GRID: usize = POV_MESH_RES + 1;

/// Core lattice vertices per chunk.
pub const CORE_VERTS: usize = POV_GRID * POV_GRID;

/// Skirt bottom-ring vertices: one per perimeter vertex per edge (corners
/// appear on two edges, so they carry two identical bottom copies — harmless,
/// and it keeps every edge a uniform 65-vertex strip).
pub const SKIRT_VERTS: usize = 4 * POV_GRID;

/// Total vertices per chunk: core grid + skirt bottom ring (plan §6.1).
pub const VERTS_PER_CHUNK: usize = CORE_VERTS + SKIRT_VERTS;

/// Indices in the shared topology: 64×64 core quads (two triangles each)
/// plus 4×64 skirt quads (**four** triangles each — both windings, so a
/// skirt wall survives back-face culling from either side; a one-sided
/// skirt reads as a crack whenever the nearer chunk is the taller one).
pub const INDICES_PER_CHUNK: usize = (POV_MESH_RES * POV_MESH_RES * 2 + 4 * POV_MESH_RES * 4) * 3;

/// Index prefix containing only the core terrain lattice. The directional
/// shadow pass deliberately excludes skirts: their defensive vertical walls
/// are not world geometry and would cast false seam shadows.
pub const CORE_INDICES: usize = POV_MESH_RES * POV_MESH_RES * 2 * 3;

/// The core-lattice vertex that skirt vertex `e * POV_GRID + k` hangs below:
/// the perimeter traversed counterclockwise viewed from above (south edge
/// east, east edge north, north edge west, west edge south), so one triangle
/// pattern gives every skirt quad an outward-facing winding. The mesher emits
/// skirt vertices in exactly this order.
#[inline]
#[must_use]
pub fn skirt_core_index(edge: usize, k: usize) -> usize {
    let last = POV_GRID - 1;
    match edge {
        0 => k,                            // south (y = 0), west -> east
        1 => k * POV_GRID + last,          // east (x = max), south -> north
        2 => last * POV_GRID + (last - k), // north (y = max), east -> west
        _ => (last - k) * POV_GRID,        // west (x = 0), north -> south
    }
}

/// The shared index topology (plan §6.5): core grid quads wound CCW viewed
/// from above (+z), plus the perimeter skirt quads wound to face outward.
/// Pure and deterministic; built once at [`Pov`] creation.
#[must_use]
pub fn chunk_indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(INDICES_PER_CHUNK);
    // Core: vertex (i, j) at j * POV_GRID + i; +x east, +y north.
    for j in 0..POV_MESH_RES {
        for i in 0..POV_MESH_RES {
            let v00 = (j * POV_GRID + i) as u32;
            let v10 = v00 + 1;
            let v01 = ((j + 1) * POV_GRID + i) as u32;
            let v11 = v01 + 1;
            // (+x) × (+y) = +z: CCW from above, for back-face culling.
            indices.extend_from_slice(&[v00, v10, v11, v00, v11, v01]);
        }
    }
    // Skirt: top edge reuses the core perimeter; bottom ring starts at
    // CORE_VERTS. Each quad is emitted with BOTH windings: a skirt wall at a
    // region border must read as terrain from whichever side the camera is
    // on, and back-face culling would otherwise erase it exactly when the
    // viewer stands on the taller chunk looking down the step.
    for edge in 0..4 {
        for k in 0..POV_MESH_RES {
            let top0 = skirt_core_index(edge, k) as u32;
            let top1 = skirt_core_index(edge, k + 1) as u32;
            let bot0 = (CORE_VERTS + edge * POV_GRID + k) as u32;
            let bot1 = bot0 + 1;
            indices.extend_from_slice(&[top0, bot0, top1, top1, bot0, bot1]);
            indices.extend_from_slice(&[top0, top1, bot0, top1, bot1, bot0]);
        }
    }
    debug_assert_eq!(indices.len(), INDICES_PER_CHUNK);
    indices
}

/// One terrain vertex (32 bytes; 3d-phase-4-plan.md §6.5).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PovVertex {
    /// Chunk-local: x, y in `[0, REGION_SIZE]`, z = elevation.
    pub position: [f32; 3],
    /// Unit surface normal (presentation-only float math).
    pub normal: [f32; 3],
    /// sRGB bytes; `VertexFormat::Unorm8x4` decodes to 0..1 in the shader.
    /// Alpha is reserved (255) — 3D-3's wetness gloss can repurpose it
    /// without a format change.
    pub color: [u8; 4],
    /// Presentation attributes, `Unorm8x4`: `[reserved neutral, ambient
    /// occlusion, river, wetness]`. Direct-sun visibility comes from the GPU
    /// directional shadow map; the CPU retains only cheap, low-frequency AO.
    pub light: [u8; 4],
}

/// One chunk's mesh handed to [`crate::Renderer::render_pov`], already
/// amortized by the shell.
#[derive(Debug)]
pub struct TerrainChunkUpload {
    /// Opaque chunk id assigned by the shell. Re-uploading an existing
    /// handle swaps its contents in place.
    pub handle: u64,
    /// Region origin in world units, for the camera-relative offset (the
    /// renderer subtracts the camera in f64, then truncates).
    pub origin: [f64; 2],
    /// Per-octave 64-bit base lattice indices of this chunk's origin for the
    /// shader's detail-normal noise, `[ix.lo, ix.hi, iy.lo, iy.hi]` each.
    /// Computed by the shell (it owns the world's noise scheme); the
    /// renderer couriers them into the chunk uniform, world-agnostic.
    pub detail_base: [[u32; 4]; DETAIL_OCTAVES],
    /// Exactly [`VERTS_PER_CHUNK`] vertices in the shared topology's order.
    pub vertices: Vec<PovVertex>,
    /// River-overlay triangles (3d-phase-3-plan.md §6): index triples into
    /// `vertices` — a subset of the core terrain topology, selected by the
    /// mesher. Empty for most chunks ⇒ no overlay draw. Unlike the vertex
    /// buffer this is variable-size, so its GPU buffer is exact-size and
    /// unpooled (§6.3); steady-state remesh traffic is zero, so steady state
    /// allocates nothing.
    pub river_indices: Vec<u32>,
}

/// One world-agnostic rigid organism handed to the POV renderer.
///
/// The renderer owns precision splitting and GPU packing. World lookup,
/// grounding, genome mapping, culling, and stable ordering remain shell-side
/// (3d-phase-4-plan.md §6.6), preserving the upload-only crate boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovOrganismInstance {
    /// Absolute presentation position. Each component is split into high/low
    /// floats only when a replacement upload reaches the renderer.
    pub position: [f64; 3],
    /// Non-uniform body scale in world units.
    pub scale: [f32; 3],
    /// Static rotation around +z, in radians.
    pub yaw: f32,
    /// Expressed sRGB plus producer flag in alpha.
    pub color: [u8; 4],
    /// Ground-surface ambient occlusion, byte-exact from the shell sampler.
    pub ambient_occlusion: u8,
    /// Optional `(amplitude, phase)` activity bob. Static bodies use zeros.
    pub bob: [f32; 2],
}

/// Replacement lists for the two explicit instanced primitive batches.
///
/// At the API boundary `None` means retain, while `Some(empty)` clears both
/// live counts. This makes organism retirement unambiguous.
#[derive(Debug, Default)]
pub struct PovOrganismUpload {
    pub boxes: Vec<PovOrganismInstance>,
    pub spheres: Vec<PovOrganismInstance>,
}

/// Packed GPU bytes per live organism instance. Exposed for shell telemetry;
/// the private raw layout remains pinned by renderer layout tests.
pub const POV_ORGANISM_INSTANCE_BYTES: u64 = 64;

/// Renderer-owned organism-buffer telemetry. Capacity is the grow-only high
/// water mark; replacement bytes report only the most recent frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PovOrganismBufferStats {
    pub box_count: u32,
    pub sphere_count: u32,
    pub live_bytes: u64,
    pub capacity_bytes: u64,
    pub replacement_bytes: u64,
}

/// Per-frame POV parameters (plan §5.2). All plain arrays: the shell computes
/// matrices with glam and hands over Pod data, the same world-agnostic
/// posture [`crate::GpuMapParams`] takes.
#[derive(Debug, Clone, Copy)]
pub struct PovFrameParams {
    /// Camera-relative view-projection (view translation excluded; it rides
    /// in the per-chunk offsets).
    pub view_proj: [[f32; 4]; 4],
    /// Camera-relative directional-light view-projection. The shell owns the
    /// stabilized fit because it owns world scale and resident bounds.
    pub light_view_proj: [[f32; 4]; 4],
    /// Camera position in world units (f64: the renderer subtracts each
    /// chunk's origin in f64, then truncates to f32).
    pub camera_pos: [f64; 3],
    /// Normalized, pointing from the sun toward the ground.
    pub sun_dir: [f32; 3],
    /// Fog color = the clear color, so geometry dissolves into sky.
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
    /// Hemisphere ambient above...
    pub sky_ambient: [f32; 3],
    /// ...and below.
    pub ground_ambient: [f32; 3],
    /// Detail-normal octaves, `[frac_x, frac_y, inv_wavelength, slope]`
    /// each — opaque to the renderer; the shell derives them from the
    /// terrain spectrum (see its `detail_octaves`).
    pub detail: [[f32; 4]; DETAIL_OCTAVES],
    /// Shader time in seconds, wrapped by the shell at [`WOBBLE_PERIOD`].
    /// Display-only animation (frame time reaches nothing but the water
    /// shader); captures pass 0.0 so snapshots stay reproducible
    /// (3d-phase-3-plan.md §4.3).
    pub time: f32,
    /// Camera-relative height of the sea plane (the shell computes
    /// `SEA_LEVEL − camera.z` in f64 and truncates; the renderer never
    /// learns `SEA_LEVEL` — 3d-phase-3-plan.md §4.1).
    pub water_z: f32,
    /// Directional depth-map edge length. Zero disables the target/pass
    /// safely (useful for an empty shadow fit).
    pub shadow_resolution: u32,
    /// Apply GPU directional shadows and CPU-baked ambient occlusion. This is
    /// the existing `B` diagnostic, renamed to match its actual authorities.
    pub shadow_ao: bool,
    /// Evaluate the per-fragment detail normals (the heaviest fragment work
    /// on a software rasterizer — 64-bit lattice hashing per octave).
    pub detail_normals: bool,
    /// Draw the sea plane and river overlays at all (off skips the blended
    /// passes entirely — fill-rate diagnostic).
    pub water: bool,
}

/// std140-compatible mirror of the WGSL `PovParams`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PovParamsRaw {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    sun_dir: [f32; 3],
    fog_start: f32,
    fog_color: [f32; 3],
    fog_end: f32,
    sky_ambient: [f32; 3],
    _pad0: f32,
    ground_ambient: [f32; 3],
    _pad1: f32,
    detail: [[f32; 4]; DETAIL_OCTAVES],
    /// `(time, water_z, wobble anchor frac x, frac y)` — the WGSL `water`
    /// vec4 (3d-phase-3-plan.md §4.3).
    water: [f32; 4],
    /// `(inverse shadow resolution, enabled, constant bias, slope bias)`.
    shadow: [f32; 4],
    /// `(shadow/AO on, detail normals on, reserved, reserved)` — the WGSL
    /// `toggles` vec4; 1.0/0.0 flags for the live diagnostic switches.
    toggles: [f32; 4],
}

/// Organism-only uniform. Unlike chunk geometry, instances remain absolute
/// and stable in their GPU buffer, so the camera is split high/low here and
/// subtraction happens in the vertex shader (3d-phase-4-plan.md §6.7).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OrganismParamsRaw {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera_hi: [f32; 4],
    camera_lo: [f32; 4],
    sun_dir: [f32; 3],
    fog_start: f32,
    fog_color: [f32; 3],
    fog_end: f32,
    sky_ambient: [f32; 3],
    _pad0: f32,
    ground_ambient: [f32; 3],
    _pad1: f32,
    /// `(inverse shadow resolution, enabled, time, producer tint strength)`.
    shadow: [f32; 4],
}

/// Canonical primitive vertex shared by both organism meshes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct PrimitiveVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

/// Stable packed instance format. Attribute groups intentionally align to
/// four-component boundaries; the final 16 bytes preserve byte-exact color
/// and AO while keeping bob phase as a float.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct OrganismInstanceRaw {
    position_hi_yaw_sin: [f32; 4],
    position_lo_yaw_cos: [f32; 4],
    scale_bob_amplitude: [f32; 4],
    color: [u8; 4],
    ambient_flags: [u8; 4],
    bob_phase: f32,
    _pad: f32,
}

/// Split one f64 so the pair reconstructs it much more accurately than one
/// absolute f32. Public instances stay unchanged when only the camera moves.
#[inline]
fn split_f64(value: f64) -> (f32, f32) {
    let hi = value as f32;
    let lo = (value - f64::from(hi)) as f32;
    (hi, lo)
}

fn split_position(position: [f64; 3]) -> ([f32; 3], [f32; 3]) {
    let (hx, lx) = split_f64(position[0]);
    let (hy, ly) = split_f64(position[1]);
    let (hz, lz) = split_f64(position[2]);
    ([hx, hy, hz], [lx, ly, lz])
}

fn pack_organism(instance: &PovOrganismInstance) -> OrganismInstanceRaw {
    let (hi, lo) = split_position(instance.position);
    let (yaw_sin, yaw_cos) = instance.yaw.sin_cos();
    OrganismInstanceRaw {
        position_hi_yaw_sin: [hi[0], hi[1], hi[2], yaw_sin],
        position_lo_yaw_cos: [lo[0], lo[1], lo[2], yaw_cos],
        scale_bob_amplitude: [
            instance.scale[0],
            instance.scale[1],
            instance.scale[2],
            instance.bob[0],
        ],
        color: instance.color,
        ambient_flags: [instance.ambient_occlusion, 0, 0, 0],
        bob_phase: instance.bob[1],
        _pad: 0.0,
    }
}

/// Pure grow-only bookkeeping used by the GPU buffers and device-free tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct InstanceCapacity {
    capacity: u32,
    count: u32,
}

impl InstanceCapacity {
    const fn initial() -> Self {
        Self {
            capacity: 1,
            count: 0,
        }
    }

    /// Replace the live count and return a new allocation capacity when a
    /// grow is required. Zero explicitly clears without shrinking.
    fn replace(&mut self, required: usize) -> Option<u32> {
        let required = u32::try_from(required).expect("organism count fits u32");
        self.count = required;
        if required <= self.capacity {
            None
        } else {
            let capacity = required.max(1).next_power_of_two();
            self.capacity = capacity;
            Some(capacity)
        }
    }
}

#[derive(Debug)]
struct InstanceBuffer {
    buffer: wgpu::Buffer,
    state: InstanceCapacity,
}

impl InstanceBuffer {
    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        let state = InstanceCapacity::initial();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: u64::from(state.capacity) * core::mem::size_of::<OrganismInstanceRaw>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, state }
    }

    fn replace(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        instances: &[PovOrganismInstance],
    ) -> u64 {
        let packed: Vec<OrganismInstanceRaw> = instances.iter().map(pack_organism).collect();
        if let Some(capacity) = self.state.replace(packed.len()) {
            self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: u64::from(capacity) * core::mem::size_of::<OrganismInstanceRaw>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if packed.is_empty() {
            0
        } else {
            let bytes = bytemuck::cast_slice(&packed);
            queue.write_buffer(&self.buffer, 0, bytes);
            bytes.len() as u64
        }
    }
}

#[derive(Debug)]
struct PrimitiveMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl PrimitiveMesh {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        label: &'static str,
        vertices: &[PrimitiveVertex],
        indices: &[u16],
    ) -> Self {
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: core::mem::size_of_val(vertices) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: core::mem::size_of_val(indices) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vertex_buffer, 0, bytemuck::cast_slice(vertices));
        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(indices));
        Self {
            vertices: vertex_buffer,
            indices: index_buffer,
            index_count: indices.len() as u32,
        }
    }
}

fn cube_geometry() -> (Vec<PrimitiveVertex>, Vec<u16>) {
    // Four independent vertices per face retain flat face normals.
    let faces = [
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
                [0.5, -0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [-0.5, 0.5, 0.5],
                [0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, -0.5, -0.5],
                [-0.5, -0.5, -0.5],
            ],
        ),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, positions) in faces {
        let base = vertices.len() as u16;
        vertices.extend(
            positions
                .into_iter()
                .map(|position| PrimitiveVertex { position, normal }),
        );
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let inv = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().recip();
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn icosphere_geometry() -> (Vec<PrimitiveVertex>, Vec<u16>) {
    let phi = (1.0 + 5.0_f32.sqrt()) * 0.5;
    let base = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let mut vertices: Vec<PrimitiveVertex> = base
        .into_iter()
        .map(|p| {
            let normal = normalize3(p);
            PrimitiveVertex {
                position: [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5],
                normal,
            }
        })
        .collect();
    let mut faces: Vec<[u16; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    // Normalize the source winding defensively, then subdivision preserves it.
    for face in &mut faces {
        let [a, b, c] = *face;
        let pa = vertices[usize::from(a)].position;
        let pb = vertices[usize::from(b)].position;
        let pc = vertices[usize::from(c)].position;
        if dot(cross(sub(pb, pa), sub(pc, pa)), pa) < 0.0 {
            *face = [a, c, b];
        }
    }
    for _ in 0..2 {
        let mut midpoint_cache: HashMap<(u16, u16), u16> = HashMap::new();
        let mut midpoint = |a: u16, b: u16, vertices: &mut Vec<PrimitiveVertex>| {
            let key = (a.min(b), a.max(b));
            *midpoint_cache.entry(key).or_insert_with(|| {
                let pa = vertices[usize::from(a)].normal;
                let pb = vertices[usize::from(b)].normal;
                let normal = normalize3([pa[0] + pb[0], pa[1] + pb[1], pa[2] + pb[2]]);
                let index = u16::try_from(vertices.len()).expect("icosphere fits u16");
                vertices.push(PrimitiveVertex {
                    position: [normal[0] * 0.5, normal[1] * 0.5, normal[2] * 0.5],
                    normal,
                });
                index
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = midpoint(a, b, &mut vertices);
            let bc = midpoint(b, c, &mut vertices);
            let ca = midpoint(c, a, &mut vertices);
            next.extend_from_slice(&[[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        faces = next;
    }
    let indices = faces.into_iter().flatten().collect();
    (vertices, indices)
}

/// std140-compatible mirror of the WGSL `ChunkOffset`, written at a fixed
/// 256-byte stride (the WebGPU-guaranteed dynamic-offset alignment).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ChunkOffsetRaw {
    offset: [f32; 3],
    _pad: f32,
    /// The chunk's [`TerrainChunkUpload::detail_base`], vec4<u32> per octave.
    detail_base: [[u32; 4]; DETAIL_OCTAVES],
}

/// Dynamic-offset stride for the per-chunk uniform. 256 is the largest
/// `min_uniform_buffer_offset_alignment` WebGPU permits, so a constant stride
/// is portable.
const CHUNK_UNIFORM_STRIDE: u64 = 256;

/// Pure slot-table + free-pool bookkeeping, generic over the buffer type so
/// it is unit-testable without a device (plan §5.7, the `AtlasManager` test
/// precedent). Eviction pushes the buffer onto the pool; upload pops or
/// creates; a re-upload to a live handle reuses its buffer in place. The
/// renderer never decides retention — the shell does, via `removes`.
#[derive(Debug)]
struct ChunkTable<B> {
    chunks: HashMap<u64, ChunkSlot<B>>,
    free: Vec<B>,
}

#[derive(Debug)]
struct ChunkSlot<B> {
    buffer: B,
    origin: [f64; 2],
    detail_base: [[u32; 4]; DETAIL_OCTAVES],
    /// River-overlay index buffer and its index count (3d-phase-3-plan.md
    /// §6.3): variable-size, exact-size, **not pooled** — dropped on evict
    /// and replaced wholesale on re-upload (including `Some → None` when a
    /// remesh loses its river). Only the fixed-size vertex buffer pools.
    overlay: Option<(B, u32)>,
}

impl<B> Default for ChunkTable<B> {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            free: Vec::new(),
        }
    }
}

impl<B> ChunkTable<B> {
    /// Evict `handle`, returning its buffer to the pool. Unknown handles are
    /// ignored (the shell may evict a chunk whose upload it superseded).
    fn remove(&mut self, handle: u64) {
        if let Some(slot) = self.chunks.remove(&handle) {
            self.free.push(slot.buffer);
        }
    }

    /// The slot for `handle`, reusing its live buffer on a re-upload, else a
    /// pooled buffer, else a fresh one from `create`.
    fn upsert(
        &mut self,
        handle: u64,
        origin: [f64; 2],
        detail_base: [[u32; 4]; DETAIL_OCTAVES],
        create: impl FnOnce() -> B,
    ) -> &mut B {
        let slot = self.chunks.entry(handle).or_insert_with(|| ChunkSlot {
            buffer: self.free.pop().unwrap_or_else(create),
            origin,
            detail_base,
            overlay: None,
        });
        slot.origin = origin;
        slot.detail_base = detail_base;
        &mut slot.buffer
    }

    /// Replace `handle`'s river-overlay buffer wholesale (`None` clears it).
    /// The old buffer is dropped, never pooled (3d-phase-3-plan.md §6.3).
    /// Unknown handles are ignored, like `remove`.
    fn set_overlay(&mut self, handle: u64, overlay: Option<(B, u32)>) {
        if let Some(slot) = self.chunks.get_mut(&handle) {
            slot.overlay = overlay;
        }
    }

    fn len(&self) -> usize {
        self.chunks.len()
    }
}

/// The scaled offscreen color target for `WER_POV_SCALE < 1.0`: the POV
/// pass rasterizes into this smaller texture and a linear-filtered blit
/// stretches it onto the surface. On a software rasterizer fragment cost is
/// CPU cost and scales with pixel count, so half resolution cuts the raster
/// bill ~4× — the practical llvmpipe knob the shading toggles are not.
#[derive(Debug)]
pub(crate) struct ScaledTarget {
    pub(crate) view: wgpu::TextureView,
    pub(crate) bind_group: wgpu::BindGroup,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The upscale blit for [`ScaledTarget`]: the debug-map fullscreen-triangle
/// shader over a **linear** sampler (the debug-map path samples nearest;
/// an upscale wants smoothing).
#[derive(Debug)]
pub(crate) struct UpscaleBlit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl UpscaleBlit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pov-upscale-shader"),
            source: wgpu::ShaderSource::Wgsl(crate::SHADER_DEBUG_MAP.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pov-upscale-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pov-upscale-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pov-upscale-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pov-upscale-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }
}

/// Device-free resolution state for the fixed-square directional target.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ShadowTargetState {
    resolution: u32,
}

impl ShadowTargetState {
    fn replace(&mut self, resolution: u32) -> bool {
        if resolution == 0 {
            return false;
        }
        if self.resolution == resolution {
            false
        } else {
            self.resolution = resolution;
            true
        }
    }
}

#[derive(Debug)]
struct ShadowTarget {
    // Retained explicitly with its view even though wgpu handles are
    // reference-counted; this documents target ownership and lifetime.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    state: ShadowTargetState,
}

#[derive(Debug)]
struct OrganismPipelines {
    box_color: wgpu::RenderPipeline,
    sphere_color: wgpu::RenderPipeline,
    box_shadow: wgpu::RenderPipeline,
    sphere_shadow: wgpu::RenderPipeline,
}

/// GPU state for the POV pass: pipeline, depth target, frame + per-chunk
/// uniforms, the shared index buffer, and the pooled chunk table.
#[derive(Debug)]
pub(crate) struct Pov {
    pipeline: wgpu::RenderPipeline,
    terrain_shadow_pipeline: wgpu::RenderPipeline,
    /// River-overlay pipeline (3d-phase-3-plan.md §6.2): the terrain vertex
    /// layout drawn through per-chunk overlay index buffers, lifted and
    /// shaded as water. Blended, depth-write off.
    overlay_pipeline: wgpu::RenderPipeline,
    /// Sea-plane pipeline (3d-phase-3-plan.md §4.1): a vertex-shader-generated
    /// camera-centered quad. Blended, depth-write off, cull-off (the camera
    /// may stand on the sea floor and look up).
    sea_pipeline: wgpu::RenderPipeline,
    organism_pipelines: OrganismPipelines,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    terrain_shadow_bind_group: wgpu::BindGroup,
    frame_bgl: wgpu::BindGroupLayout,
    organism_uniform: wgpu::Buffer,
    organism_bind_group: wgpu::BindGroup,
    organism_shadow_bind_group: wgpu::BindGroup,
    organism_bgl: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    shadow: ShadowTarget,
    chunk_bgl: wgpu::BindGroupLayout,
    chunk_uniform: wgpu::Buffer,
    chunk_bind_group: wgpu::BindGroup,
    /// Chunk-uniform slots the buffer currently holds.
    chunk_capacity: u32,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    table: ChunkTable<wgpu::Buffer>,
    box_mesh: PrimitiveMesh,
    sphere_mesh: PrimitiveMesh,
    box_instances: InstanceBuffer,
    sphere_instances: InstanceBuffer,
    organism_stats: PovOrganismBufferStats,
    /// `(view, width, height)`; recreated when the surface size changes.
    depth: Option<(wgpu::TextureView, u32, u32)>,
    /// The linear-filtered upscale blit for reduced-resolution rendering.
    upscale: UpscaleBlit,
    /// The scaled offscreen color target (`WER_POV_SCALE < 1.0`); `None`
    /// at full resolution. Recreated when the target size changes.
    scaled: Option<ScaledTarget>,
    /// Surface format, for recreating the scaled target.
    format: wgpu::TextureFormat,
}

/// Vertex buffer byte size (fixed for every chunk — the pool invariant).
const CHUNK_BUFFER_BYTES: u64 = (VERTS_PER_CHUNK * core::mem::size_of::<PovVertex>()) as u64;

const PRIMITIVE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 12,
        shader_location: 1,
    },
];

const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 48,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 52,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 56,
        shader_location: 7,
    },
];

fn create_shadow_target(device: &wgpu::Device, resolution: u32) -> ShadowTarget {
    let resolution = resolution.max(1);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pov-directional-shadow"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    ShadowTarget {
        _texture: texture,
        view,
        state: ShadowTargetState { resolution },
    }
}

fn create_shadow_bind_group(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    target: &ShadowTarget,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&target.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Uniform-only caster group. Deliberately has no target/sampler parameters,
/// preventing a depth-write/read alias in the shadow pass by construction.
fn create_shadow_caster_bind_group(
    device: &wgpu::Device,
    label: &'static str,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    })
}

fn create_organism_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    label: &'static str,
    shadow: bool,
) -> wgpu::RenderPipeline {
    let primitive_layout = wgpu::VertexBufferLayout {
        array_stride: core::mem::size_of::<PrimitiveVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &PRIMITIVE_ATTRIBUTES,
    };
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: core::mem::size_of::<OrganismInstanceRaw>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &INSTANCE_ATTRIBUTES,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(if shadow { "vs_shadow" } else { "vs_main" }),
            compilation_options: Default::default(),
            buffers: &[primitive_layout, instance_layout],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            front_face: wgpu::FrontFace::Ccw,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(if shadow {
                wgpu::CompareFunction::LessEqual
            } else {
                wgpu::CompareFunction::Less
            }),
            stencil: wgpu::StencilState::default(),
            bias: if shadow {
                wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.5,
                    clamp: 0.0,
                }
            } else {
                wgpu::DepthBiasState::default()
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: (!shadow).then_some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Pov {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pov-terrain-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_POV_TERRAIN.into()),
        });

        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pov-frame-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let organism_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pov-organism-frame-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        // Casters must not bind the sampled shadow texture while that same
        // resource is the depth-write attachment. A dedicated uniform-only
        // layout makes the absence structural and wgpu-validation-visible.
        let shadow_frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pov-shadow-caster-frame-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let chunk_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pov-chunk-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the detail-noise lattice bases.
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: core::num::NonZeroU64::new(
                        core::mem::size_of::<ChunkOffsetRaw>() as u64,
                    ),
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pov-terrain-layout"),
            bind_group_layouts: &[Some(&frame_bgl), Some(&chunk_bgl)],
            immediate_size: 0,
        });
        let terrain_shadow_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pov-terrain-shadow-layout"),
                bind_group_layouts: &[Some(&shadow_frame_bgl), Some(&chunk_bgl)],
                immediate_size: 0,
            });
        // The 32-byte PovVertex layout, shared by the terrain pipeline and
        // the river-overlay pipeline (which re-draws the same buffers).
        let vertex_attributes = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Unorm8x4,
                offset: 28,
                shader_location: 3,
            },
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: core::mem::size_of::<PovVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        };
        // Water passes test against terrain depth but never write it
        // (3d-phase-3-plan.md §4.4).
        let water_depth = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };
        let blended_target = wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pov-terrain-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: core::slice::from_ref(&vertex_layout),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let terrain_shadow_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("pov-terrain-shadow-pipeline"),
                layout: Some(&terrain_shadow_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_shadow"),
                    compilation_options: Default::default(),
                    buffers: core::slice::from_ref(&vertex_layout),
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: Some(wgpu::Face::Back),
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 1,
                        slope_scale: 1.5,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            });

        let water_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pov-water-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_POV_WATER.into()),
        });
        let overlay_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pov-water-overlay-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &water_shader,
                entry_point: Some("vs_overlay"),
                compilation_options: Default::default(),
                buffers: core::slice::from_ref(&vertex_layout),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                front_face: wgpu::FrontFace::Ccw,
                ..Default::default()
            },
            depth_stencil: Some(water_depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &water_shader,
                entry_point: Some("fs_overlay"),
                compilation_options: Default::default(),
                targets: &[Some(blended_target.clone())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sea_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pov-water-sea-layout"),
            bind_group_layouts: &[Some(&frame_bgl)],
            immediate_size: 0,
        });
        let sea_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pov-water-sea-pipeline"),
            layout: Some(&sea_layout),
            vertex: wgpu::VertexState {
                module: &water_shader,
                entry_point: Some("vs_sea"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(water_depth),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &water_shader,
                entry_point: Some("fs_sea"),
                compilation_options: Default::default(),
                targets: &[Some(blended_target)],
            }),
            multiview_mask: None,
            cache: None,
        });

        let organism_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pov-organism-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_POV_ORGANISM.into()),
        });
        let organism_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pov-organism-layout"),
            bind_group_layouts: &[Some(&organism_bgl)],
            immediate_size: 0,
        });
        let organism_shadow_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("pov-organism-shadow-layout"),
                bind_group_layouts: &[Some(&shadow_frame_bgl)],
                immediate_size: 0,
            });
        let organism_pipelines = OrganismPipelines {
            box_color: create_organism_pipeline(
                device,
                &organism_shader,
                &organism_layout,
                format,
                "pov-box-color-pipeline",
                false,
            ),
            sphere_color: create_organism_pipeline(
                device,
                &organism_shader,
                &organism_layout,
                format,
                "pov-sphere-color-pipeline",
                false,
            ),
            box_shadow: create_organism_pipeline(
                device,
                &organism_shader,
                &organism_shadow_layout,
                format,
                "pov-box-shadow-pipeline",
                true,
            ),
            sphere_shadow: create_organism_pipeline(
                device,
                &organism_shader,
                &organism_shadow_layout,
                format,
                "pov-sphere-shadow-pipeline",
                true,
            ),
        };

        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-frame-uniform"),
            size: core::mem::size_of::<PovParamsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let organism_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-organism-frame-uniform"),
            size: core::mem::size_of::<OrganismParamsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pov-shadow-comparison-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow = create_shadow_target(device, 1);
        let frame_bind_group = create_shadow_bind_group(
            device,
            "pov-frame-bind-group",
            &frame_bgl,
            &frame_uniform,
            &shadow,
            &shadow_sampler,
        );
        let organism_bind_group = create_shadow_bind_group(
            device,
            "pov-organism-frame-bind-group",
            &organism_bgl,
            &organism_uniform,
            &shadow,
            &shadow_sampler,
        );
        let terrain_shadow_bind_group = create_shadow_caster_bind_group(
            device,
            "pov-terrain-shadow-frame-bind-group",
            &shadow_frame_bgl,
            &frame_uniform,
        );
        let organism_shadow_bind_group = create_shadow_caster_bind_group(
            device,
            "pov-organism-shadow-frame-bind-group",
            &shadow_frame_bgl,
            &organism_uniform,
        );

        // Shared index topology, built once for every chunk ever drawn.
        let indices = chunk_indices();
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-chunk-indices"),
            size: (indices.len() * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let initial_capacity = 64;
        let (chunk_uniform, chunk_bind_group) =
            Self::chunk_uniform_for(device, &chunk_bgl, initial_capacity);

        queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(&indices));
        let (box_vertices, box_indices) = cube_geometry();
        let (sphere_vertices, sphere_indices) = icosphere_geometry();
        let box_mesh =
            PrimitiveMesh::new(device, queue, "pov-box-mesh", &box_vertices, &box_indices);
        let sphere_mesh = PrimitiveMesh::new(
            device,
            queue,
            "pov-sphere-mesh",
            &sphere_vertices,
            &sphere_indices,
        );

        Self {
            pipeline,
            terrain_shadow_pipeline,
            overlay_pipeline,
            sea_pipeline,
            organism_pipelines,
            frame_uniform,
            frame_bind_group,
            terrain_shadow_bind_group,
            frame_bgl,
            organism_uniform,
            organism_bind_group,
            organism_shadow_bind_group,
            organism_bgl,
            shadow_sampler,
            shadow,
            chunk_bgl,
            chunk_uniform,
            chunk_bind_group,
            chunk_capacity: initial_capacity,
            index_buffer,
            index_count: indices.len() as u32,
            table: ChunkTable::default(),
            box_mesh,
            sphere_mesh,
            box_instances: InstanceBuffer::new(device, "pov-box-instances"),
            sphere_instances: InstanceBuffer::new(device, "pov-sphere-instances"),
            organism_stats: PovOrganismBufferStats::default(),
            depth: None,
            upscale: UpscaleBlit::new(device, format),
            scaled: None,
            format,
        }
    }

    /// The POV pass's render size for a surface of `width`×`height` at
    /// `scale` (clamped by the shell): the scaled offscreen size, or the
    /// surface itself at scale 1.
    pub(crate) fn render_size(width: u32, height: u32, scale: f32) -> (u32, u32) {
        if scale >= 1.0 {
            (width, height)
        } else {
            (
                ((width as f32 * scale).round() as u32).max(1),
                ((height as f32 * scale).round() as u32).max(1),
            )
        }
    }

    /// (Re)create the scaled offscreen target when reduced-resolution
    /// rendering is active and the size changed. Returns whether the scaled
    /// path is active.
    pub(crate) fn ensure_scaled(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if matches!(&self.scaled, Some(t) if t.width == width && t.height == height) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pov-scaled-color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pov-scaled-bind-group"),
            layout: &self.upscale.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.upscale.sampler),
                },
            ],
        });
        self.scaled = Some(ScaledTarget {
            view,
            bind_group,
            width,
            height,
        });
    }

    /// The scaled offscreen target's view, when reduced-resolution rendering
    /// is active ([`Self::ensure_scaled`] ran this frame).
    pub(crate) fn scaled_view(&self) -> Option<&wgpu::TextureView> {
        self.scaled.as_ref().map(|t| &t.view)
    }

    /// Record the upscale blit: stretch the scaled offscreen target over the
    /// full surface with linear filtering.
    pub(crate) fn blit_scaled(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let scaled = self.scaled.as_ref().expect("ensure_scaled ran");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pov-upscale"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // The blit covers every pixel; Load value is irrelevant.
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.upscale.pipeline);
        pass.set_bind_group(0, &scaled.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn chunk_uniform_for(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        capacity: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-chunk-offsets"),
            size: u64::from(capacity) * CHUNK_UNIFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pov-chunk-bind-group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: core::num::NonZeroU64::new(core::mem::size_of::<ChunkOffsetRaw>() as u64),
                }),
            }],
        });
        (buffer, bind_group)
    }

    /// (Re)create the depth target when the surface size changed
    /// (plan §5.1). The 2D passes never touch it.
    pub(crate) fn ensure_depth(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        if matches!(&self.depth, Some((_, w, h)) if *w == width && *h == height) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pov-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.depth = Some((view, width, height));
    }

    /// Recreate only when the tier-selected directional resolution changes.
    /// Color render scale never reaches this path, so shadow quality remains
    /// independent of `WER_POV_SCALE`.
    pub(crate) fn ensure_shadow(&mut self, device: &wgpu::Device, resolution: u32) {
        if resolution == 0 || !self.shadow.state.replace(resolution) {
            return;
        }
        self.shadow = create_shadow_target(device, resolution);
        self.frame_bind_group = create_shadow_bind_group(
            device,
            "pov-frame-bind-group",
            &self.frame_bgl,
            &self.frame_uniform,
            &self.shadow,
            &self.shadow_sampler,
        );
        self.organism_bind_group = create_shadow_bind_group(
            device,
            "pov-organism-frame-bind-group",
            &self.organism_bgl,
            &self.organism_uniform,
            &self.shadow,
            &self.shadow_sampler,
        );
    }

    /// Apply evictions and uploads (plan §5.4): pooled buffers come back on
    /// remove; uploads pop the pool or allocate; re-uploads swap in place.
    /// Returns bytes written.
    pub(crate) fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uploads: &[TerrainChunkUpload],
        removes: &[u64],
    ) -> u64 {
        for &handle in removes {
            self.table.remove(handle);
        }
        let mut bytes = 0u64;
        for upload in uploads {
            if upload.vertices.len() != VERTS_PER_CHUNK {
                log::error!(
                    "pov chunk upload with {} vertices (expected {VERTS_PER_CHUNK}); skipped",
                    upload.vertices.len()
                );
                continue;
            }
            let buffer =
                self.table
                    .upsert(upload.handle, upload.origin, upload.detail_base, || {
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("pov-chunk-vertices"),
                            size: CHUNK_BUFFER_BYTES,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    });
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&upload.vertices));
            bytes += CHUNK_BUFFER_BYTES;
            // The river-overlay index list (3d-phase-3-plan.md §6.3):
            // exact-size, unpooled, replaced wholesale — including
            // `Some → None` when a remesh loses its river.
            let overlay = (!upload.river_indices.is_empty()).then(|| {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("pov-river-overlay-indices"),
                    size: (upload.river_indices.len() * 4) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&upload.river_indices));
                bytes += (upload.river_indices.len() * 4) as u64;
                (buffer, upload.river_indices.len() as u32)
            });
            self.table.set_overlay(upload.handle, overlay);
        }
        bytes
    }

    /// Apply the tri-state organism replacement. `None` is the zero-traffic
    /// steady-state path; `Some(empty)` clears live counts without writing.
    pub(crate) fn apply_organisms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        upload: Option<&PovOrganismUpload>,
    ) {
        let Some(upload) = upload else {
            self.organism_stats.replacement_bytes = 0;
            return;
        };
        let box_bytes =
            self.box_instances
                .replace(device, queue, "pov-box-instances", &upload.boxes);
        let sphere_bytes =
            self.sphere_instances
                .replace(device, queue, "pov-sphere-instances", &upload.spheres);
        let raw_size = POV_ORGANISM_INSTANCE_BYTES;
        self.organism_stats = PovOrganismBufferStats {
            box_count: self.box_instances.state.count,
            sphere_count: self.sphere_instances.state.count,
            live_bytes: u64::from(
                self.box_instances.state.count + self.sphere_instances.state.count,
            ) * raw_size,
            capacity_bytes: u64::from(
                self.box_instances.state.capacity + self.sphere_instances.state.capacity,
            ) * raw_size,
            replacement_bytes: box_bytes + sphere_bytes,
        };
    }

    pub(crate) fn organism_stats(&self) -> PovOrganismBufferStats {
        self.organism_stats
    }

    /// Write the frame uniform and every resident chunk's camera-relative
    /// offset, growing the offset buffer if the chunk count outgrew it.
    /// Returns the draw list as `(vertex buffer handle, dynamic offset)`
    /// pairs — assembled here so the pass body below stays trivial.
    pub(crate) fn write_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &PovFrameParams,
    ) -> Vec<(u64, u32)> {
        let shadow_enabled = frame.shadow_ao && frame.shadow_resolution > 0;
        let inverse_shadow_resolution = if shadow_enabled {
            1.0 / frame.shadow_resolution as f32
        } else {
            0.0
        };
        let raw = PovParamsRaw {
            view_proj: frame.view_proj,
            light_view_proj: frame.light_view_proj,
            sun_dir: frame.sun_dir,
            fog_start: frame.fog_start,
            fog_color: frame.fog_color,
            fog_end: frame.fog_end,
            sky_ambient: frame.sky_ambient,
            _pad0: 0.0,
            ground_ambient: frame.ground_ambient,
            _pad1: 0.0,
            detail: frame.detail,
            // The wobble's world anchor: `camera mod WOBBLE_TILE` in f64, so
            // the f32 the shader adds to camera-relative positions is small
            // and exact-enough at any world coordinate; jumps at tile
            // crossings are whole wobble periods (3d-phase-3-plan.md §4.3).
            water: [
                frame.time,
                frame.water_z,
                frame.camera_pos[0].rem_euclid(WOBBLE_TILE) as f32,
                frame.camera_pos[1].rem_euclid(WOBBLE_TILE) as f32,
            ],
            shadow: [
                inverse_shadow_resolution,
                f32::from(shadow_enabled),
                0.000_35,
                0.001_5,
            ],
            toggles: [
                f32::from(frame.shadow_ao),
                f32::from(frame.detail_normals),
                0.0,
                0.0,
            ],
        };
        queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&raw));

        let (camera_hi, camera_lo) = split_position(frame.camera_pos);
        let organism_raw = OrganismParamsRaw {
            view_proj: frame.view_proj,
            light_view_proj: frame.light_view_proj,
            camera_hi: [camera_hi[0], camera_hi[1], camera_hi[2], 0.0],
            camera_lo: [camera_lo[0], camera_lo[1], camera_lo[2], 0.0],
            sun_dir: frame.sun_dir,
            fog_start: frame.fog_start,
            fog_color: frame.fog_color,
            fog_end: frame.fog_end,
            sky_ambient: frame.sky_ambient,
            _pad0: 0.0,
            ground_ambient: frame.ground_ambient,
            _pad1: 0.0,
            shadow: [
                inverse_shadow_resolution,
                f32::from(frame.shadow_ao),
                frame.time,
                0.08,
            ],
        };
        queue.write_buffer(&self.organism_uniform, 0, bytemuck::bytes_of(&organism_raw));

        let needed = self.table.len() as u32;
        if needed > self.chunk_capacity {
            let capacity = needed.next_power_of_two();
            let (buffer, bind_group) = Self::chunk_uniform_for(device, &self.chunk_bgl, capacity);
            self.chunk_uniform = buffer;
            self.chunk_bind_group = bind_group;
            self.chunk_capacity = capacity;
        }

        // Fixed draw order (sorted handles): draw order is irrelevant to the
        // depth-tested result, but reproducible behavior is cheap at <=~300.
        let mut handles: Vec<u64> = self.table.chunks.keys().copied().collect();
        handles.sort_unstable();
        let mut draws = Vec::with_capacity(handles.len());
        let mut offsets = vec![0u8; handles.len() * CHUNK_UNIFORM_STRIDE as usize];
        for (i, &handle) in handles.iter().enumerate() {
            let slot = &self.table.chunks[&handle];
            // The far-from-origin fix (plan §4): subtract in f64, truncate.
            let raw = ChunkOffsetRaw {
                offset: [
                    (slot.origin[0] - frame.camera_pos[0]) as f32,
                    (slot.origin[1] - frame.camera_pos[1]) as f32,
                    (-frame.camera_pos[2]) as f32,
                ],
                _pad: 0.0,
                detail_base: slot.detail_base,
            };
            let at = i * CHUNK_UNIFORM_STRIDE as usize;
            offsets[at..at + core::mem::size_of::<ChunkOffsetRaw>()]
                .copy_from_slice(bytemuck::bytes_of(&raw));
            draws.push((handle, (i as u64 * CHUNK_UNIFORM_STRIDE) as u32));
        }
        if !offsets.is_empty() {
            queue.write_buffer(&self.chunk_uniform, 0, &offsets);
        }
        draws
    }

    /// Record the fixed 3D-4 order: directional depth, then opaque terrain,
    /// boxes, spheres, translucent river overlays, and finally sea.
    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        draws: &[(u64, u32)],
        clear: [f64; 4],
        water: bool,
        shadows: bool,
    ) {
        if shadows {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pov-directional-shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.terrain_shadow_pipeline);
            pass.set_bind_group(0, &self.terrain_shadow_bind_group, &[]);
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            for &(handle, dynamic_offset) in draws {
                let slot = &self.table.chunks[&handle];
                pass.set_bind_group(1, &self.chunk_bind_group, &[dynamic_offset]);
                pass.set_vertex_buffer(0, slot.buffer.slice(..));
                pass.draw_indexed(0..CORE_INDICES as u32, 0, 0..1);
            }
            if self.box_instances.state.count > 0 {
                pass.set_pipeline(&self.organism_pipelines.box_shadow);
                pass.set_bind_group(0, &self.organism_shadow_bind_group, &[]);
                pass.set_vertex_buffer(0, self.box_mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, self.box_instances.buffer.slice(..));
                pass.set_index_buffer(self.box_mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(
                    0..self.box_mesh.index_count,
                    0,
                    0..self.box_instances.state.count,
                );
            }
            if self.sphere_instances.state.count > 0 {
                pass.set_pipeline(&self.organism_pipelines.sphere_shadow);
                pass.set_bind_group(0, &self.organism_shadow_bind_group, &[]);
                pass.set_vertex_buffer(0, self.sphere_mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, self.sphere_instances.buffer.slice(..));
                pass.set_index_buffer(
                    self.sphere_mesh.indices.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                pass.draw_indexed(
                    0..self.sphere_mesh.index_count,
                    0,
                    0..self.sphere_instances.state.count,
                );
            }
        }

        let (depth_view, _, _) = self.depth.as_ref().expect("ensure_depth ran");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pov-color"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear[0],
                        g: clear[1],
                        b: clear[2],
                        a: clear[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for &(handle, dynamic_offset) in draws {
            let slot = &self.table.chunks[&handle];
            pass.set_bind_group(1, &self.chunk_bind_group, &[dynamic_offset]);
            pass.set_vertex_buffer(0, slot.buffer.slice(..));
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        }

        // Opaque bodies must precede both water layers: terrain depth occludes
        // them, while translucent water correctly tints submerged fragments.
        if self.box_instances.state.count > 0 {
            pass.set_pipeline(&self.organism_pipelines.box_color);
            pass.set_bind_group(0, &self.organism_bind_group, &[]);
            pass.set_vertex_buffer(0, self.box_mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, self.box_instances.buffer.slice(..));
            pass.set_index_buffer(self.box_mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(
                0..self.box_mesh.index_count,
                0,
                0..self.box_instances.state.count,
            );
        }
        if self.sphere_instances.state.count > 0 {
            pass.set_pipeline(&self.organism_pipelines.sphere_color);
            pass.set_bind_group(0, &self.organism_bind_group, &[]);
            pass.set_vertex_buffer(0, self.sphere_mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, self.sphere_instances.buffer.slice(..));
            pass.set_index_buffer(
                self.sphere_mesh.indices.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed(
                0..self.sphere_mesh.index_count,
                0,
                0..self.sphere_instances.state.count,
            );
        }
        // The water passes can be skipped wholesale (the `V` diagnostic
        // toggle): no blended fill, no wobble — terrain only.
        if !water {
            return;
        }
        // River overlays: the same vertex buffers through each chunk's own
        // index list (3d-phase-3-plan.md §6.2). Most chunks have none.
        let mut overlay_bound = false;
        for &(handle, dynamic_offset) in draws {
            let slot = &self.table.chunks[&handle];
            let Some((indices, count)) = &slot.overlay else {
                continue;
            };
            if !overlay_bound {
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, &self.frame_bind_group, &[]);
                overlay_bound = true;
            }
            pass.set_bind_group(1, &self.chunk_bind_group, &[dynamic_offset]);
            pass.set_vertex_buffer(0, slot.buffer.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
        // The sea plane, always last (3d-phase-3-plan.md §4.4). Drawn even
        // with zero resident chunks: below-sea frontier holes legitimately
        // show water, above-sea ones cover when their terrain lands —
        // transient, and mostly inside fog (plan §4.1).
        pass.set_pipeline(&self.sea_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

/// Headless POV capture (ADR 0021): the same terrain pass rendered to an
/// offscreen texture, with the pixels copied back **for image-file output
/// only**. This is the debug-screenshot analogue of the CPU `--screenshot`
/// path — the POV view has no CPU rendering twin, so inspecting it headlessly
/// requires reading the rendered pixels.
///
/// The ADR 0017 discipline survives structurally: the live [`crate::Renderer`]
/// still exposes no readback API; this type is a separate headless
/// construction (no surface, no window) used only by debug tooling
/// (`wer --pov-script`). Captured bytes go to files for humans — never into
/// hashes, persistence, gameplay, or golden fixtures (GPU output is
/// non-portable bits).
#[derive(Debug)]
pub struct PovCapture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pov: Pov,
    color: wgpu::Texture,
    width: u32,
    height: u32,
}

impl PovCapture {
    /// Bring up an offscreen device and the POV pipeline at `width`×`height`.
    /// Blocking (debug tooling); honors the standard `WGPU_*` env vars.
    pub fn new(width: u32, height: u32) -> Result<Self, crate::RendererError> {
        let (width, height) = (width.max(1), height.max(1));
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = crate::pollster_block(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        )
        .map_err(|_| crate::RendererError::NoAdapter)?;
        let info = adapter.get_info();
        log::info!(
            "pov capture adapter: {} ({:?}, driver: {} {})",
            info.name,
            info.backend,
            info.driver,
            info.driver_info
        );
        let (device, queue) =
            crate::pollster_block(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("wer-pov-capture-device"),
                ..Default::default()
            }))
            .map_err(crate::RendererError::Device)?;

        // The same sRGB format family the surface path presents to, so a
        // captured frame matches what the window shows.
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pov-capture-color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut pov = Pov::new(&device, &queue, format);
        pov.ensure_depth(&device, width, height);
        Ok(Self {
            device,
            queue,
            pov,
            color,
            width,
            height,
        })
    }

    /// Capture size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Apply chunk uploads/evictions and an optional organism replacement,
    /// exactly like the live path.
    pub fn apply(
        &mut self,
        uploads: &[TerrainChunkUpload],
        removes: &[u64],
        organisms: Option<&PovOrganismUpload>,
    ) {
        self.pov.apply(&self.device, &self.queue, uploads, removes);
        self.pov
            .apply_organisms(&self.device, &self.queue, organisms);
    }

    /// Current packed organism-buffer counts/capacities.
    #[must_use]
    pub fn organism_stats(&self) -> PovOrganismBufferStats {
        self.pov.organism_stats()
    }

    /// Render one frame offscreen and return its RGBA8 bytes (sRGB-encoded,
    /// row-major, `width × height × 4`) — for writing an image file.
    #[must_use]
    pub fn snapshot(&mut self, frame: &PovFrameParams, clear: [f64; 4]) -> Vec<u8> {
        self.snapshot_at_scale(frame, clear, 1.0)
    }

    /// [`Self::snapshot`] through the reduced-resolution path (`WER_POV_SCALE`):
    /// rasterize at `scale`, upscale-blit to the full-size capture — the same
    /// flow the live `render_pov` runs, so a scaled frame can be inspected
    /// headlessly.
    #[must_use]
    pub fn snapshot_at_scale(
        &mut self,
        frame: &PovFrameParams,
        clear: [f64; 4],
        scale: f32,
    ) -> Vec<u8> {
        let scaled_active = scale < 1.0;
        let (rw, rh) = Pov::render_size(self.width, self.height, scale);
        self.pov.ensure_depth(&self.device, rw, rh);
        self.pov
            .ensure_shadow(&self.device, frame.shadow_resolution);
        if scaled_active {
            self.pov.ensure_scaled(&self.device, rw, rh);
        }
        let draws = self.pov.write_frame(&self.device, &self.queue, frame);
        let view = self
            .color
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pov-capture-frame"),
            });
        if scaled_active {
            let target = self.pov.scaled_view().expect("ensure_scaled ran");
            self.pov.draw(
                &mut encoder,
                target,
                &draws,
                clear,
                frame.water,
                frame.shadow_ao && frame.shadow_resolution > 0,
            );
            self.pov.blit_scaled(&mut encoder, &view);
        } else {
            self.pov.draw(
                &mut encoder,
                &view,
                &draws,
                clear,
                frame.water,
                frame.shadow_ao && frame.shadow_resolution > 0,
            );
        }

        // Copy out with the mandatory 256-byte row alignment, then un-pad.
        let padded = (self.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-capture-readback"),
            size: u64::from(padded) * u64::from(self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .expect("map_async callback ran")
            .expect("readback buffer mapped");
        let data = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity((self.width * self.height * 4) as usize);
        for row in 0..self.height {
            let at = (row * padded) as usize;
            rgba.extend_from_slice(&data[at..at + (self.width * 4) as usize]);
        }
        drop(data);
        readback.unmap();
        rgba
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_outward(vertices: &[PrimitiveVertex], indices: &[u16]) {
        for tri in indices.chunks_exact(3) {
            let a = vertices[usize::from(tri[0])].position;
            let b = vertices[usize::from(tri[1])].position;
            let c = vertices[usize::from(tri[2])].position;
            let center = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            assert!(
                dot(cross(sub(b, a), sub(c, a)), center) > 0.0,
                "inward or degenerate triangle {tri:?}"
            );
        }
    }

    #[test]
    fn topology_counts_match_the_published_constants() {
        let indices = chunk_indices();
        assert_eq!(indices.len(), INDICES_PER_CHUNK);
        assert_eq!(VERTS_PER_CHUNK, 65 * 65 + 4 * 65);
        // Every index addresses a real vertex.
        assert!(indices.iter().all(|&i| (i as usize) < VERTS_PER_CHUNK));
        assert_eq!(CORE_INDICES, 64 * 64 * 6);
        assert!(CORE_INDICES < indices.len());
    }

    #[test]
    fn every_boundary_edge_is_skirted_front_and_back() {
        // Each perimeter lattice edge must appear in exactly one skirt quad,
        // emitted with both windings (plan §10 check 4 + the double-sided
        // fix): count (top_k, top_k+1) pairs over the skirt triangles,
        // normalized by vertex-index order — one front-facing and one
        // back-facing top triangle per edge.
        let indices = chunk_indices();
        let core = POV_MESH_RES * POV_MESH_RES * 6;
        let mut seen = std::collections::HashMap::new();
        for tri in indices[core..].chunks_exact(3) {
            let tops: Vec<u32> = tri
                .iter()
                .copied()
                .filter(|&v| (v as usize) < CORE_VERTS)
                .collect();
            if tops.len() == 2 {
                let key = (tops[0].min(tops[1]), tops[0].max(tops[1]));
                *seen.entry(key).or_insert(0u32) += 1;
            }
        }
        // 4 edges x 64 lattice edges, each referenced by exactly one quad,
        // whose top-edge triangle appears once per winding.
        assert_eq!(seen.len(), 4 * POV_MESH_RES);
        assert!(seen.values().all(|&count| count == 2));
    }

    #[test]
    fn pool_reuses_buffers_and_swaps_in_place() {
        // The plan §5.5 discipline, on the pure bookkeeping (no device):
        // remove pools the buffer, the next upload pops it, a re-upload to a
        // live handle keeps its buffer.
        let mut table: ChunkTable<u32> = ChunkTable::default();
        let mut next = 0u32;
        let mut create = || {
            next += 1;
            next
        };
        let base = [[0u32; 4]; DETAIL_OCTAVES];
        let first = *table.upsert(1, [0.0, 0.0], base, &mut create);
        assert_eq!(first, 1);
        // Re-upload to the same handle: same buffer, updated origin.
        let again = *table.upsert(1, [256.0, 0.0], base, &mut create);
        assert_eq!(again, first, "re-upload must swap contents in place");
        assert_eq!(table.chunks[&1].origin, [256.0, 0.0]);
        // Evict, then upload a different handle: the pooled buffer returns.
        table.remove(1);
        assert_eq!(table.len(), 0);
        let reused = *table.upsert(2, [512.0, 0.0], base, &mut create);
        assert_eq!(reused, first, "eviction must feed the pool");
        // A second live handle allocates fresh.
        let fresh = *table.upsert(3, [768.0, 0.0], base, &mut create);
        assert_eq!(fresh, 2);
        // Removing an unknown handle is a no-op.
        table.remove(99);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn overlay_buffers_replace_wholesale_and_never_pool() {
        // 3d-phase-3-plan.md §9 test 7: the river-overlay slot is replaced
        // wholesale on re-upload (including Some → None when a remesh loses
        // its river), dropped on remove, and never feeds the vertex pool.
        let mut table: ChunkTable<u32> = ChunkTable::default();
        let mut next = 0u32;
        let mut create = || {
            next += 1;
            next
        };
        let base = [[0u32; 4]; DETAIL_OCTAVES];
        let vertex = *table.upsert(1, [0.0, 0.0], base, &mut create);
        assert!(table.chunks[&1].overlay.is_none(), "chunks start dry");
        table.set_overlay(1, Some((77, 12)));
        assert_eq!(table.chunks[&1].overlay, Some((77, 12)));
        // A re-upload that lost its river clears the overlay.
        let again = *table.upsert(1, [0.0, 0.0], base, &mut create);
        assert_eq!(again, vertex);
        table.set_overlay(1, None);
        assert!(table.chunks[&1].overlay.is_none());
        // Remove pools only the vertex buffer; a live overlay is dropped.
        table.set_overlay(1, Some((88, 6)));
        table.remove(1);
        assert_eq!(table.free, vec![vertex], "overlay buffers never pool");
        // Unknown handles are ignored (superseded-then-evicted uploads).
        table.set_overlay(99, Some((5, 3)));
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn canonical_cube_has_flat_faces_and_outward_winding() {
        let (vertices, indices) = cube_geometry();
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
        assert!(indices.iter().all(|&i| usize::from(i) < vertices.len()));
        for vertex in &vertices {
            assert!(vertex.position.iter().all(|v| (-0.5..=0.5).contains(v)));
            assert!((dot(vertex.normal, vertex.normal) - 1.0).abs() < 1e-6);
            assert!((dot(vertex.position, vertex.normal) - 0.5).abs() < 1e-6);
        }
        assert_outward(&vertices, &indices);
    }

    #[test]
    fn two_subdivision_icosphere_has_canonical_topology() {
        let (vertices, indices) = icosphere_geometry();
        assert_eq!(vertices.len(), 162);
        assert_eq!(indices.len(), 960);
        assert!(indices.iter().all(|&i| usize::from(i) < vertices.len()));
        for vertex in &vertices {
            assert!((dot(vertex.position, vertex.position).sqrt() - 0.5).abs() < 1e-5);
            assert!((dot(vertex.normal, vertex.normal) - 1.0).abs() < 1e-5);
            assert!(
                dot(vertex.position, vertex.normal) > 0.499,
                "normal must point radially outward"
            );
        }
        // Midpoint caching is what produces the canonical 10*4^n+2 count;
        // no two normalized vertex positions may be duplicated.
        for (i, a) in vertices.iter().enumerate() {
            assert!(vertices[i + 1..].iter().all(|b| a.position != b.position));
        }
        assert_outward(&vertices, &indices);
    }

    #[test]
    fn packed_instance_is_stable_and_exactly_sixty_four_bytes() {
        assert_eq!(core::mem::size_of::<PrimitiveVertex>(), 24);
        assert_eq!(core::mem::size_of::<OrganismInstanceRaw>(), 64);
        assert_eq!(
            core::mem::size_of::<OrganismInstanceRaw>() as u64,
            POV_ORGANISM_INSTANCE_BYTES
        );
        assert_eq!(
            core::mem::offset_of!(OrganismInstanceRaw, position_hi_yaw_sin),
            0
        );
        assert_eq!(
            core::mem::offset_of!(OrganismInstanceRaw, position_lo_yaw_cos),
            16
        );
        assert_eq!(
            core::mem::offset_of!(OrganismInstanceRaw, scale_bob_amplitude),
            32
        );
        assert_eq!(core::mem::offset_of!(OrganismInstanceRaw, color), 48);
        assert_eq!(
            core::mem::offset_of!(OrganismInstanceRaw, ambient_flags),
            52
        );
        assert_eq!(core::mem::offset_of!(OrganismInstanceRaw, bob_phase), 56);
        let instance = PovOrganismInstance {
            position: [1.0e12 + 0.125, -1.0e12 - 0.375, 123.75],
            scale: [1.0, 2.0, 3.0],
            yaw: 1.25,
            color: [1, 2, 3, 1],
            ambient_occlusion: 177,
            bob: [0.25, 0.75],
        };
        let first = pack_organism(&instance);
        let second = pack_organism(&instance);
        assert_eq!(first, second, "camera state is absent from packed bytes");
        assert_eq!(first.color, instance.color);
        assert_eq!(first.ambient_flags, [177, 0, 0, 0]);
    }

    #[test]
    fn high_low_split_preserves_nearby_deltas_at_far_origins() {
        let cases = [
            ([10.25, -20.5, 3.0], [9.0, -22.0, 1.0]),
            (
                [1.0e12 + 0.125, -1.0e12 - 0.375, 200_000.25],
                [1.0e12 - 17.5, -1.0e12 + 9.25, 199_999.0],
            ),
            (
                [-1.0e12 - 0.125, 1.0e12 + 0.375, -200_000.25],
                [-1.0e12 + 17.5, 1.0e12 - 9.25, -199_999.0],
            ),
        ];
        for (position, camera) in cases {
            let (ph, pl) = split_position(position);
            let (ch, cl) = split_position(camera);
            for axis in 0..3 {
                let reconstructed = f64::from((ph[axis] - ch[axis]) + (pl[axis] - cl[axis]));
                let expected = position[axis] - camera[axis];
                assert!(
                    (reconstructed - expected).abs() <= 2.0e-5 * expected.abs().max(1.0),
                    "axis {axis}: {reconstructed} != {expected}"
                );
            }
        }
    }

    #[test]
    fn grow_only_instance_state_handles_independent_batches_and_clear() {
        let mut boxes = InstanceCapacity::initial();
        let mut spheres = InstanceCapacity::initial();
        assert_eq!(boxes.replace(1), None);
        assert_eq!(
            boxes,
            InstanceCapacity {
                capacity: 1,
                count: 1
            }
        );
        assert_eq!(boxes.replace(3), Some(4));
        assert_eq!(boxes.replace(2), None);
        assert_eq!(
            boxes,
            InstanceCapacity {
                capacity: 4,
                count: 2
            }
        );
        assert_eq!(spheres.replace(5), Some(8));
        assert_eq!(
            spheres,
            InstanceCapacity {
                capacity: 8,
                count: 5
            }
        );
        assert_eq!(boxes.replace(0), None);
        assert_eq!(
            boxes,
            InstanceCapacity {
                capacity: 4,
                count: 0
            }
        );
        assert_eq!(
            spheres,
            InstanceCapacity {
                capacity: 8,
                count: 5
            }
        );
    }

    #[test]
    fn shadow_target_state_reuses_resolution_and_recreates_on_change() {
        let mut state = ShadowTargetState::default();
        assert!(!state.replace(0));
        assert_eq!(state.resolution, 0);
        assert!(state.replace(1024));
        assert!(!state.replace(1024));
        assert!(state.replace(2048));
        assert_eq!(state.resolution, 2048);
        assert!(!state.replace(0));
        assert_eq!(state.resolution, 2048);
    }

    #[test]
    fn rust_uniform_and_vertex_layouts_are_pinned() {
        assert_eq!(core::mem::size_of::<PovParamsRaw>(), 288);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, view_proj), 0);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, light_view_proj), 64);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, sun_dir), 128);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, fog_start), 140);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, fog_color), 144);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, fog_end), 156);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, sky_ambient), 160);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, _pad0), 172);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, ground_ambient), 176);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, _pad1), 188);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, detail), 192);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, water), 240);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, shadow), 256);
        assert_eq!(core::mem::offset_of!(PovParamsRaw, toggles), 272);

        assert_eq!(core::mem::size_of::<OrganismParamsRaw>(), 240);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, view_proj), 0);
        assert_eq!(
            core::mem::offset_of!(OrganismParamsRaw, light_view_proj),
            64
        );
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, camera_hi), 128);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, camera_lo), 144);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, sun_dir), 160);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, fog_start), 172);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, fog_color), 176);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, fog_end), 188);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, sky_ambient), 192);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, _pad0), 204);
        assert_eq!(
            core::mem::offset_of!(OrganismParamsRaw, ground_ambient),
            208
        );
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, _pad1), 220);
        assert_eq!(core::mem::offset_of!(OrganismParamsRaw, shadow), 224);

        assert_eq!(core::mem::size_of::<ChunkOffsetRaw>(), 64);
        assert_eq!(core::mem::offset_of!(ChunkOffsetRaw, offset), 0);
        assert_eq!(core::mem::offset_of!(ChunkOffsetRaw, _pad), 12);
        assert_eq!(core::mem::offset_of!(ChunkOffsetRaw, detail_base), 16);

        assert_eq!(PRIMITIVE_ATTRIBUTES[0].offset, 0);
        assert_eq!(PRIMITIVE_ATTRIBUTES[1].offset, 12);
        assert_eq!(
            PRIMITIVE_ATTRIBUTES[0].format,
            wgpu::VertexFormat::Float32x3
        );
        assert_eq!(
            PRIMITIVE_ATTRIBUTES[1].format,
            wgpu::VertexFormat::Float32x3
        );
        assert_eq!(
            INSTANCE_ATTRIBUTES.map(|attribute| attribute.offset),
            [0, 16, 32, 48, 52, 56]
        );
        assert_eq!(
            INSTANCE_ATTRIBUTES.map(|attribute| attribute.shader_location),
            [2, 3, 4, 5, 6, 7]
        );
        assert_eq!(
            INSTANCE_ATTRIBUTES.map(|attribute| attribute.format),
            [
                wgpu::VertexFormat::Float32x4,
                wgpu::VertexFormat::Float32x4,
                wgpu::VertexFormat::Float32x4,
                wgpu::VertexFormat::Unorm8x4,
                wgpu::VertexFormat::Unorm8x4,
                wgpu::VertexFormat::Float32,
            ]
        );
    }
}
