//! `renderer` — the wgpu-backed presentation layer.
//!
//! Phase 1 owns the GPU device, queue, and surface, and presents a single
//! CPU-composed debug map texture per frame (phase-1-plan.md section 10) — the
//! cheapest visualization that makes chunk replacement obvious. It is
//! deliberately thin; the terrain/ecology render graph, clipmaps, and GPU
//! field refinement (implementation-plan.md sections 12.2 and 17) will be
//! built on top of this foundation, and the clear-only path stays for shells
//! that draw nothing.
//!
//! The crate stays WebGPU-compatible: it targets `wgpu` (which maps to native
//! Vulkan/Metal/DX and to WebGPU in the browser) and uses only WGSL shaders.

use core::fmt;

/// The debug-map presentation shader (fullscreen textured triangle).
pub const SHADER_DEBUG_MAP: &str = include_str!("../shaders/debug_map.wgsl");

/// Errors that can occur bringing the renderer up.
#[derive(Debug)]
pub enum RendererError {
    /// No suitable GPU adapter was available.
    NoAdapter,
    /// The surface could not be created for the given window.
    Surface(wgpu::CreateSurfaceError),
    /// The logical device could not be requested.
    Device(wgpu::RequestDeviceError),
    /// The surface produced no usable default configuration.
    NoSurfaceConfig,
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendererError::NoAdapter => write!(f, "no suitable GPU adapter found"),
            RendererError::Surface(e) => write!(f, "failed to create surface: {e}"),
            RendererError::Device(e) => write!(f, "failed to request device: {e}"),
            RendererError::NoSurfaceConfig => write!(f, "surface has no default configuration"),
        }
    }
}

impl core::error::Error for RendererError {}

/// GPU state for the debug-map pipeline: the presentation pipeline itself plus
/// the current map texture (recreated when the CPU-side map size changes).
#[derive(Debug)]
struct DebugMapPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// `(texture, bind_group, width, height)` for the last uploaded map.
    texture: Option<(wgpu::Texture, wgpu::BindGroup, u32, u32)>,
}

impl DebugMapPipeline {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("debug-map-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_DEBUG_MAP.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug-map-bgl"),
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("debug-map-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("debug-map-pipeline"),
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

        // Nearest sampling: field cells stay crisp, so a regenerating tile or
        // a seam is visible instead of being blurred away — which is exactly
        // what the debug map exists to catch.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("debug-map-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            texture: None,
        }
    }

    /// Upload the CPU-composed map, (re)creating the texture on size change,
    /// and return whether a drawable texture exists.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) {
        if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
            log::error!(
                "debug map upload with inconsistent dimensions ({width}x{height}, {} bytes)",
                rgba.len()
            );
            return;
        }
        let needs_new = !matches!(&self.texture, Some((_, _, w, h)) if *w == width && *h == height);
        if needs_new {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("debug-map-texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("debug-map-bind-group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.texture = Some((texture, bind_group, width, height));
        }
        let (texture, _, _, _) = self.texture.as_ref().expect("texture just ensured");
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Owns the GPU objects needed to present frames to a single surface.
#[derive(Debug)]
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    debug_map: DebugMapPipeline,
}

impl Renderer {
    /// Bring up the renderer for a window-like `target` of the given pixel size.
    ///
    /// `target` is anything wgpu can build a `'static` surface from — typically an
    /// `Arc<winit::window::Window>` on native. This is async because adapter and
    /// device acquisition are async on WebGPU; native callers can drive it with
    /// `pollster::block_on`.
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Self, RendererError> {
        let instance = wgpu::Instance::default();

        let surface = instance
            .create_surface(target)
            .map_err(RendererError::Surface)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|_| RendererError::NoAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("wer-device"),
                ..Default::default()
            })
            .await
            .map_err(RendererError::Device)?;

        let config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or(RendererError::NoSurfaceConfig)?;
        surface.configure(&device, &config);

        let debug_map = DebugMapPipeline::new(&device, config.format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            debug_map,
        })
    }

    /// Current surface size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Reconfigure the surface after the window is resized. Zero dimensions are
    /// ignored (minimized windows).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Acquire the next surface frame, handling transient states
    /// (outdated/lost/occluded/timeout) by reconfiguring or skipping the frame
    /// rather than propagating. `None` means "draw nothing this frame".
    fn acquire_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        use wgpu::CurrentSurfaceTexture as Cst;
        match self.surface.get_current_texture() {
            Cst::Success(frame) => Some(frame),
            Cst::Suboptimal(frame) => {
                // Usable this frame; reconfigure for the next.
                self.surface.configure(&self.device, &self.config);
                Some(frame)
            }
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            Cst::Timeout | Cst::Occluded => None,
            Cst::Validation => {
                log::error!("surface get_current_texture validation error");
                None
            }
        }
    }

    /// Acquire the next frame and clear it to `color` (linear RGBA, 0..=1).
    /// Returns `false` when no frame was drawn.
    pub fn render_clear(&mut self, color: [f64; 4]) -> bool {
        let Some(frame) = self.acquire_frame() else {
            return false;
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame"),
            });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color[0],
                            g: color[1],
                            b: color[2],
                            a: color[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        true
    }

    /// Upload a CPU-composed RGBA8 debug map (`width`×`height`, row-major,
    /// row 0 = north edge) and present it, letterboxed to a centered square so
    /// world cells stay square regardless of window shape
    /// (phase-1-plan.md section 10). Returns `false` when no frame was drawn.
    pub fn render_map(&mut self, rgba: &[u8], width: u32, height: u32, clear: [f64; 4]) -> bool {
        self.debug_map
            .upload(&self.device, &self.queue, rgba, width, height);
        if self.debug_map.texture.is_none() {
            return false;
        }

        let Some(frame) = self.acquire_frame() else {
            return false;
        };
        let (_, bind_group, _, _) = self.debug_map.texture.as_ref().expect("uploaded above");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("debug-map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Centered square viewport: the map is square in world space.
            let side = self.config.width.min(self.config.height) as f32;
            let x = (self.config.width as f32 - side) * 0.5;
            let y = (self.config.height as f32 - side) * 0.5;
            pass.set_viewport(x, y, side, side, 0.0, 1.0);
            pass.set_pipeline(&self.debug_map.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        true
    }
}
