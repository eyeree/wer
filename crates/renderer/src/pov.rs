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

/// One terrain vertex (32 bytes; plan §5.2 + the baked-light extension).
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
    /// Baked lighting, `Unorm8x4`: `[sun visibility, ambient occlusion,
    /// reserved, reserved]`. The mesher ray-marches the heightfield toward
    /// the fixed sun for `x` and measures multi-scale concavity for `y`;
    /// both are derived presentation only (ADR 0017) — the shader multiplies
    /// them into the sun and ambient terms respectively.
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

/// Per-frame POV parameters (plan §5.2). All plain arrays: the shell computes
/// matrices with glam and hands over Pod data, the same world-agnostic
/// posture [`crate::GpuMapParams`] takes.
#[derive(Debug, Clone, Copy)]
pub struct PovFrameParams {
    /// Camera-relative view-projection (view translation excluded; it rides
    /// in the per-chunk offsets).
    pub view_proj: [[f32; 4]; 4],
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
    /// Apply the baked per-vertex sun-visibility/AO terms (off = fully lit).
    /// A live diagnostic toggle; shader-side, so no remesh is needed.
    pub baked_light: bool,
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
    /// `(baked light on, detail normals on, reserved, reserved)` — the WGSL
    /// `toggles` vec4; 1.0/0.0 flags for the live diagnostic switches.
    toggles: [f32; 4],
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

/// GPU state for the POV pass: pipeline, depth target, frame + per-chunk
/// uniforms, the shared index buffer, and the pooled chunk table.
#[derive(Debug)]
pub(crate) struct Pov {
    pipeline: wgpu::RenderPipeline,
    /// River-overlay pipeline (3d-phase-3-plan.md §6.2): the terrain vertex
    /// layout drawn through per-chunk overlay index buffers, lifted and
    /// shaded as water. Blended, depth-write off.
    overlay_pipeline: wgpu::RenderPipeline,
    /// Sea-plane pipeline (3d-phase-3-plan.md §4.1): a vertex-shader-generated
    /// camera-centered quad. Blended, depth-write off, cull-off (the camera
    /// may stand on the sea floor and look up).
    sea_pipeline: wgpu::RenderPipeline,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    chunk_bgl: wgpu::BindGroupLayout,
    chunk_uniform: wgpu::Buffer,
    chunk_bind_group: wgpu::BindGroup,
    /// Chunk-uniform slots the buffer currently holds.
    chunk_capacity: u32,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    table: ChunkTable<wgpu::Buffer>,
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
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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

        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pov-frame-uniform"),
            size: core::mem::size_of::<PovParamsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pov-frame-bind-group"),
            layout: &frame_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });

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

        Self {
            pipeline,
            overlay_pipeline,
            sea_pipeline,
            frame_uniform,
            frame_bind_group,
            chunk_bgl,
            chunk_uniform,
            chunk_bind_group,
            chunk_capacity: initial_capacity,
            index_buffer,
            index_count: indices.len() as u32,
            table: ChunkTable::default(),
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
        let raw = PovParamsRaw {
            view_proj: frame.view_proj,
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
            toggles: [
                f32::from(frame.baked_light),
                f32::from(frame.detail_normals),
                0.0,
                0.0,
            ],
        };
        queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&raw));

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

    /// Record the POV pass: clear color + depth, draw every resident chunk
    /// with the shared index buffer, then the translucent water passes in
    /// fixed order — river overlay, then sea (3d-phase-3-plan.md §4.4): the
    /// overlay hugs the terrain, so wherever both cover a pixel the sea is
    /// the nearer surface when the camera is above water.
    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        draws: &[(u64, u32)],
        clear: [f64; 4],
        water: bool,
    ) {
        let (depth_view, _, _) = self.depth.as_ref().expect("ensure_depth ran");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pov-terrain"),
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

    /// Apply chunk uploads/evictions, exactly like the live path.
    pub fn apply(&mut self, uploads: &[TerrainChunkUpload], removes: &[u64]) {
        self.pov.apply(&self.device, &self.queue, uploads, removes);
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
            self.pov
                .draw(&mut encoder, target, &draws, clear, frame.water);
            self.pov.blit_scaled(&mut encoder, &view);
        } else {
            self.pov
                .draw(&mut encoder, &view, &draws, clear, frame.water);
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

    #[test]
    fn topology_counts_match_the_published_constants() {
        let indices = chunk_indices();
        assert_eq!(indices.len(), INDICES_PER_CHUNK);
        assert_eq!(VERTS_PER_CHUNK, 65 * 65 + 4 * 65);
        // Every index addresses a real vertex.
        assert!(indices.iter().all(|&i| (i as usize) < VERTS_PER_CHUNK));
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
}
