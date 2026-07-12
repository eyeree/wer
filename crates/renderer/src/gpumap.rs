//! The GPU debug-map path (phase-6-plan.md §6.5): the region-tile
//! [`FieldAtlas`], delta uploads, and the `compose_map.wgsl` pipeline — the
//! first node of the eventual render graph and the proof of its rules.
//!
//! **ADR 0017 — derived-only, structurally.** This module exposes uploads and
//! draws; it exposes *no readback API of any kind*. Nothing composed or
//! refined on the GPU can flow back into authoritative state, hashing,
//! persistence, or gameplay — the type surface makes the violation
//! unwritable from the shell, and review guards the crate boundary.
//!
//! The CPU composer remains the headless/screenshot/test path and the
//! correctness reference; this path must only *look* the same at tile
//! resolution (the A/B toggle is the parity eyeball).

/// The GPU composition shader (fullscreen pass; false color + overlay blend +
/// refinement octaves).
pub const SHADER_COMPOSE_MAP: &str = include_str!("../shaders/compose_map.wgsl");

/// One region-tile upload: the packed channel planes for one atlas slot.
/// Produced by the shell only for tiles whose dependency-hash key changed
/// (delta uploads — steady-state traffic is ~zero).
#[derive(Debug)]
pub struct MapTileUpload {
    /// Destination atlas slot.
    pub slot: u32,
    /// Four rgba32float planes, each `res × res` texels × 4 channels,
    /// row-major: (elev, hard, temp, moist), (river, wet, depth, fert),
    /// (veg, canopy, herb, pred), (diversity, presence mask, 0, 0).
    pub planes: [Vec<f32>; 4],
    /// `res × res` texels × 2 u16: (biome id, dominant species index).
    pub ints: Vec<u16>,
}

/// One refinement octave's parameters (phase-6-plan.md §6.5): the WGSL side
/// continues the terrain gradient spectrum from a 64-bit base lattice index
/// plus an in-lattice fraction, both computed by the shell in f64.
#[derive(Debug, Clone, Copy, Default)]
pub struct RefineOctaveParams {
    /// Base lattice index (bit pattern of the i64) of the view's NW corner.
    pub base_ix: u64,
    /// Same for the y axis.
    pub base_iy: u64,
    /// Fractional lattice position at the NW corner.
    pub frac: [f32; 2],
    /// Reciprocal wavelength in map-cell units.
    pub inv_wavelength_cells: f32,
    /// Display amplitude, world units (zero-mean detail around the sample).
    pub amplitude: f32,
    /// Terrain octave index this continues (≥ `world_core::terrain::OCTAVES`).
    pub octave: u32,
}

/// Per-frame parameters of the GPU composition.
#[derive(Debug, Clone, Copy)]
pub struct GpuMapParams {
    /// Window half-extent in regions.
    pub half_regions: i32,
    /// Cells per region edge.
    pub resolution: u32,
    /// Channel selector (must be one of the GPU-supported channels; the
    /// shell falls back to the CPU composer otherwise).
    pub channel: u32,
    /// Draw the region grid.
    pub grid: bool,
    /// Refinement octaves (0..=3 used).
    pub refine: [RefineOctaveParams; 3],
    /// How many refinement octaves are active.
    pub refine_count: u32,
}

/// std140-compatible mirror of the WGSL `RefineOctave`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RefineOctaveRaw {
    base_ix: [u32; 2],
    base_iy: [u32; 2],
    frac: [f32; 2],
    inv_wavelength_cells: f32,
    amplitude: f32,
    octave: u32,
    _pad: [u32; 3],
}

/// std140-compatible mirror of the WGSL `MapParams`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MapParamsRaw {
    half_regions: i32,
    resolution: i32,
    side_cells: f32,
    atlas_tiles_x: u32,
    channel: u32,
    grid: u32,
    refine_octave_count: u32,
    _pad: u32,
    refine: [RefineOctaveRaw; 3],
}

/// GPU state for the atlas-composed map: the channel-plane atlases, the slot
/// lookup, the overlay texture, and the composition pipeline.
#[derive(Debug)]
pub struct GpuMap {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    slot_buffer: wgpu::Buffer,
    planes: [wgpu::Texture; 4],
    ints: wgpu::Texture,
    overlay: wgpu::Texture,
    /// Atlas slots per row (slot → tile x/y).
    pub tiles_x: u32,
    /// Total atlas slot capacity.
    pub capacity: u32,
    /// Cells per region edge at atlas build time.
    pub resolution: u32,
    /// Map image side (cells) at atlas build time.
    pub side: u32,
    /// Slot-lookup capacity in entries.
    slot_entries: u64,
}

impl GpuMap {
    /// Build the atlas and pipeline for a window of `capacity` region slots
    /// at `resolution` cells per region, presenting `side`-cell maps.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        capacity: u32,
        resolution: u32,
        side: u32,
    ) -> Self {
        let tiles_x = (capacity as f64).sqrt().ceil() as u32;
        let tiles_y = capacity.div_ceil(tiles_x);
        let tex_size = wgpu::Extent3d {
            width: tiles_x * resolution,
            height: tiles_y * resolution,
            depth_or_array_layers: 1,
        };
        let plane_desc = |label| wgpu::TextureDescriptor {
            label: Some(label),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let planes = [
            device.create_texture(&plane_desc("atlas-plane0")),
            device.create_texture(&plane_desc("atlas-plane1")),
            device.create_texture(&plane_desc("atlas-plane2")),
            device.create_texture(&plane_desc("atlas-plane3")),
        ];
        let ints = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas-ints"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg16Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let overlay = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("map-overlay"),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let span = u64::from(side / resolution) * u64::from(side / resolution);
        let slot_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map-slots"),
            size: span * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("map-params"),
            size: core::mem::size_of::<MapParamsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose-map-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_COMPOSE_MAP.into()),
        });
        let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let float_tex = wgpu::TextureSampleType::Float { filterable: false };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose-map-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(2, float_tex),
                texture_entry(3, float_tex),
                texture_entry(4, float_tex),
                texture_entry(5, float_tex),
                texture_entry(6, wgpu::TextureSampleType::Uint),
                texture_entry(7, float_tex),
            ],
        });
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose-map-bind-group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: slot_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view(&planes[0])),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view(&planes[1])),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&view(&planes[2])),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&view(&planes[3])),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&view(&ints)),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&view(&overlay)),
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose-map-layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose-map-pipeline"),
            layout: Some(&layout),
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

        Self {
            pipeline,
            bind_group,
            uniforms,
            slot_buffer,
            planes,
            ints,
            overlay,
            tiles_x,
            capacity,
            resolution,
            side,
            slot_entries: span,
        }
    }

    /// Upload changed region tiles into their slots; returns bytes written.
    pub fn upload_tiles(&self, queue: &wgpu::Queue, uploads: &[MapTileUpload]) -> u64 {
        let res = self.resolution;
        let mut bytes = 0u64;
        for upload in uploads {
            if upload.slot >= self.capacity {
                continue;
            }
            let origin = wgpu::Origin3d {
                x: (upload.slot % self.tiles_x) * res,
                y: (upload.slot / self.tiles_x) * res,
                z: 0,
            };
            let extent = wgpu::Extent3d {
                width: res,
                height: res,
                depth_or_array_layers: 1,
            };
            for (texture, plane) in self.planes.iter().zip(&upload.planes) {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(plane),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(res * 16),
                        rows_per_image: Some(res),
                    },
                    extent,
                );
                bytes += u64::from(res) * u64::from(res) * 16;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.ints,
                    mip_level: 0,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&upload.ints),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(res * 4),
                    rows_per_image: Some(res),
                },
                extent,
            );
            bytes += u64::from(res) * u64::from(res) * 4;
        }
        bytes
    }

    /// Upload the CPU-drawn sparse overlay (map-cell resolution RGBA8).
    pub fn upload_overlay(&self, queue: &wgpu::Queue, rgba: &[u8]) -> u64 {
        if rgba.len() != (self.side * self.side * 4) as usize {
            log::error!("overlay upload size mismatch ({} bytes)", rgba.len());
            return 0;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.overlay,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.side * 4),
                rows_per_image: Some(self.side),
            },
            wgpu::Extent3d {
                width: self.side,
                height: self.side,
                depth_or_array_layers: 1,
            },
        );
        u64::from(self.side) * u64::from(self.side) * 4
    }

    /// Write this frame's slot lookup + parameters.
    pub fn write_frame(&self, queue: &wgpu::Queue, params: &GpuMapParams, slots: &[i32]) {
        let mut refine = [RefineOctaveRaw {
            base_ix: [0; 2],
            base_iy: [0; 2],
            frac: [0.0; 2],
            inv_wavelength_cells: 0.0,
            amplitude: 0.0,
            octave: 0,
            _pad: [0; 3],
        }; 3];
        for (raw, p) in refine.iter_mut().zip(&params.refine) {
            raw.base_ix = [(p.base_ix & 0xFFFF_FFFF) as u32, (p.base_ix >> 32) as u32];
            raw.base_iy = [(p.base_iy & 0xFFFF_FFFF) as u32, (p.base_iy >> 32) as u32];
            raw.frac = p.frac;
            raw.inv_wavelength_cells = p.inv_wavelength_cells;
            raw.amplitude = p.amplitude;
            raw.octave = p.octave;
        }
        let span = 2 * params.half_regions + 1;
        let raw = MapParamsRaw {
            half_regions: params.half_regions,
            resolution: params.resolution as i32,
            side_cells: (span * params.resolution as i32) as f32,
            atlas_tiles_x: self.tiles_x,
            channel: params.channel,
            grid: u32::from(params.grid),
            refine_octave_count: params.refine_count,
            _pad: 0,
            refine,
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&raw));
        let n = (self.slot_entries as usize).min(slots.len());
        queue.write_buffer(&self.slot_buffer, 0, bytemuck::cast_slice(&slots[..n]));
    }

    /// Record the composition draw into `pass` (the caller sets the viewport).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
