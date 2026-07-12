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

/// Indices in the shared topology: 64×64 core quads + 4×64 skirt quads,
/// two triangles each.
pub const INDICES_PER_CHUNK: usize = (POV_MESH_RES * POV_MESH_RES + 4 * POV_MESH_RES) * 2 * 3;

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
    // CORE_VERTS. The (top_k, bottom_k, top_k+1) pattern faces outward for
    // every edge because the perimeter enumeration is counterclockwise.
    for edge in 0..4 {
        for k in 0..POV_MESH_RES {
            let top0 = skirt_core_index(edge, k) as u32;
            let top1 = skirt_core_index(edge, k + 1) as u32;
            let bot0 = (CORE_VERTS + edge * POV_GRID + k) as u32;
            let bot1 = bot0 + 1;
            indices.extend_from_slice(&[top0, bot0, top1, top1, bot0, bot1]);
        }
    }
    debug_assert_eq!(indices.len(), INDICES_PER_CHUNK);
    indices
}

/// One terrain vertex (28 bytes; plan §5.2).
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
    /// Exactly [`VERTS_PER_CHUNK`] vertices in the shared topology's order.
    pub vertices: Vec<PovVertex>,
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
}

/// std140-compatible mirror of the WGSL `ChunkOffset`, written at a fixed
/// 256-byte stride (the WebGPU-guaranteed dynamic-offset alignment).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ChunkOffsetRaw {
    offset: [f32; 3],
    _pad: f32,
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
    fn upsert(&mut self, handle: u64, origin: [f64; 2], create: impl FnOnce() -> B) -> &mut B {
        let slot = self.chunks.entry(handle).or_insert_with(|| ChunkSlot {
            buffer: self.free.pop().unwrap_or_else(create),
            origin,
        });
        slot.origin = origin;
        &mut slot.buffer
    }

    fn len(&self) -> usize {
        self.chunks.len()
    }
}

/// GPU state for the POV pass: pipeline, depth target, frame + per-chunk
/// uniforms, the shared index buffer, and the pooled chunk table.
#[derive(Debug)]
pub(crate) struct Pov {
    pipeline: wgpu::RenderPipeline,
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
                visibility: wgpu::ShaderStages::VERTEX,
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
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pov-terrain-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: core::mem::size_of::<PovVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
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
                    ],
                }],
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
        }
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
            let buffer = self.table.upsert(upload.handle, upload.origin, || {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("pov-chunk-vertices"),
                    size: CHUNK_BUFFER_BYTES,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            });
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&upload.vertices));
            bytes += CHUNK_BUFFER_BYTES;
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

    /// Record the terrain pass: clear color + depth, draw every resident
    /// chunk with the shared index buffer.
    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        draws: &[(u64, u32)],
        clear: [f64; 4],
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
    fn every_boundary_edge_is_skirted_exactly_once() {
        // Each perimeter lattice edge must appear in exactly one skirt quad
        // (plan §10 check 4): count (top_k, top_k+1) pairs over the skirt
        // triangles, normalized by vertex-index order.
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
        // 4 edges x 64 lattice edges, each referenced by exactly one quad's
        // top-edge triangle.
        assert_eq!(seen.len(), 4 * POV_MESH_RES);
        assert!(seen.values().all(|&count| count == 1));
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
        let first = *table.upsert(1, [0.0, 0.0], &mut create);
        assert_eq!(first, 1);
        // Re-upload to the same handle: same buffer, updated origin.
        let again = *table.upsert(1, [256.0, 0.0], &mut create);
        assert_eq!(again, first, "re-upload must swap contents in place");
        assert_eq!(table.chunks[&1].origin, [256.0, 0.0]);
        // Evict, then upload a different handle: the pooled buffer returns.
        table.remove(1);
        assert_eq!(table.len(), 0);
        let reused = *table.upsert(2, [512.0, 0.0], &mut create);
        assert_eq!(reused, first, "eviction must feed the pool");
        // A second live handle allocates fresh.
        let fresh = *table.upsert(3, [768.0, 0.0], &mut create);
        assert_eq!(fresh, 2);
        // Removing an unknown handle is a no-op.
        table.remove(99);
        assert_eq!(table.len(), 2);
    }
}
