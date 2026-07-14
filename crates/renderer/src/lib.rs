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

pub mod gpumap;
pub mod pov;
pub use gpumap::{GpuMap, GpuMapParams, MapTileUpload, RefineOctaveParams};
pub use pov::{
    PovFrameParams, PovVertex, TerrainChunkUpload, DETAIL_OCTAVES, SHADER_POV_TERRAIN,
    SHADER_POV_WATER,
};

/// The debug-map presentation shader (fullscreen textured triangle).
pub const SHADER_DEBUG_MAP: &str = include_str!("../shaders/debug_map.wgsl");

/// The `(x, y, width, height)` of the largest centered viewport that fits an
/// `image`-sized picture inside a `surface`-sized window without distortion.
///
/// Used by [`Renderer::render_map`] for presentation; exposed so callers can
/// run the same mapping in reverse to translate mouse coordinates back into
/// image pixels.
#[must_use]
pub fn letterbox_viewport(surface: (u32, u32), image: (u32, u32)) -> (f32, f32, f32, f32) {
    let (sw, sh) = (surface.0.max(1) as f32, surface.1.max(1) as f32);
    let (iw, ih) = (image.0.max(1) as f32, image.1.max(1) as f32);
    let scale = (sw / iw).min(sh / ih);
    let (w, h) = (iw * scale, ih * scale);
    ((sw - w) * 0.5, (sh - h) * 0.5, w, h)
}

/// The present mode for the surface: FIFO (vsync) by default — the Phase 6
/// frame pacer — overridable through `WER_PRESENT_MODE`
/// (`fifo`/`mailbox`/`immediate`), falling back to FIFO when the platform
/// does not support the requested mode. FIFO support is guaranteed by the
/// WebGPU/wgpu contract, so the fallback is always available.
fn present_mode_from_env(caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let requested = std::env::var("WER_PRESENT_MODE").ok();
    let mode = match requested.as_deref() {
        Some("mailbox") => wgpu::PresentMode::Mailbox,
        Some("immediate") => wgpu::PresentMode::Immediate,
        Some("fifo") | None => wgpu::PresentMode::Fifo,
        Some(other) => {
            log::warn!("unknown WER_PRESENT_MODE {other:?}; using fifo");
            wgpu::PresentMode::Fifo
        }
    };
    if caps.present_modes.contains(&mode) {
        mode
    } else {
        log::warn!("present mode {mode:?} unsupported here; using fifo");
        wgpu::PresentMode::Fifo
    }
}

/// Coarse adapter class, for resource-tier detection (phase-6-plan.md §6.7).
/// A renderer-owned enum so the shell needs no direct wgpu dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbedAdapter {
    /// A discrete GPU.
    Discrete,
    /// An integrated or virtual GPU.
    Integrated,
    /// A software rasterizer.
    Cpu,
    /// Nothing usable detected.
    Unknown,
}

/// Probe the default adapter's class with a throwaway instance (blocking;
/// used once at startup for tier detection, phase-6-plan.md §6.7).
#[must_use]
pub fn probe_adapter() -> ProbedAdapter {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    match pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) {
        Ok(adapter) => match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => ProbedAdapter::Discrete,
            wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::VirtualGpu => {
                ProbedAdapter::Integrated
            }
            wgpu::DeviceType::Cpu => ProbedAdapter::Cpu,
            wgpu::DeviceType::Other => ProbedAdapter::Unknown,
        },
        Err(_) => ProbedAdapter::Unknown,
    }
}

/// A tiny local block_on (avoids a pollster dependency here): the adapter
/// future is driven by wgpu's own executor on native and resolves promptly.
/// `pub(crate)` for the headless [`pov::PovCapture`] bring-up (ADR 0021).
pub(crate) fn pollster_block<F: core::future::Future>(fut: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn raw() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        RawWaker::new(
            core::ptr::null(),
            &RawWakerVTable::new(clone, no_op, no_op, no_op),
        )
    }
    // SAFETY: the vtable functions are all no-ops over a null pointer.
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = core::pin::pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

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

/// Produces a fresh surface target on demand — typically a clone of an
/// `Arc<winit::window::Window>`. The renderer keeps it so a *lost* surface
/// (compositor restart, WSLg/driver hiccup) can be recreated from scratch
/// instead of crashing the app: reconfiguring a dead `VkSurfaceKHR` is a fatal
/// validation error, recreating it is routine.
/// The surface-source callback (see [`Renderer::new`]). Native window
/// handles are `Send + Sync` and the bound keeps the renderer usable from
/// any thread; on wasm the browser canvas is inherently single-threaded, so
/// the bounds are dropped — WebGPU itself runs on the main thread there.
#[cfg(not(target_arch = "wasm32"))]
pub type SurfaceSource = Box<dyn Fn() -> wgpu::SurfaceTarget<'static> + Send + Sync>;
/// The surface-source callback (see [`Renderer::new`]), wasm variant.
#[cfg(target_arch = "wasm32")]
pub type SurfaceSource = Box<dyn Fn() -> wgpu::SurfaceTarget<'static>>;

/// A surface source over a browser canvas (phase-7-plan.md §9.9): "the
/// platform shell creates or receives browser-specific canvas/surface
/// handles and passes them into renderer initialization." Cloning the
/// handle per call keeps surface recreation possible, exactly like the
/// native window closure.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn canvas_surface_source(canvas: web_sys::HtmlCanvasElement) -> SurfaceSource {
    Box::new(move || wgpu::SurfaceTarget::Canvas(canvas.clone()))
}

/// Owns the GPU objects needed to present frames to a single surface.
pub struct Renderer {
    instance: wgpu::Instance,
    surface_source: SurfaceSource,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    debug_map: DebugMapPipeline,
    /// Second blit pipeline for the HUD panel strip in the GPU-map path.
    panel_blit: DebugMapPipeline,
    /// Third blit pipeline for the small POV HUD chip (FPS counter), drawn
    /// pixel-exact in the top-right corner after the POV pass.
    pov_hud_blit: DebugMapPipeline,
    /// The Phase 6 atlas-composed map path (phase-6-plan.md §6.5), built
    /// lazily on the first GPU-mode frame.
    gpu_map: Option<GpuMap>,
    /// The POV terrain path (3d-phase-1-plan.md §5), built lazily on the
    /// first POV frame. Owns the renderer's only depth target.
    pov: Option<pov::Pov>,
}

impl fmt::Debug for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Renderer")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// Bring up the renderer for a window of the given pixel size.
    ///
    /// `source` must return a fresh surface target for the same window each
    /// time it is called — e.g. `Box::new(move || window.clone().into())` for
    /// an `Arc<winit::window::Window>`, or a canvas-cloning closure in the
    /// browser — so the surface can be recreated if the platform loses it.
    /// This is async because adapter and device acquisition are async on
    /// WebGPU; native callers can drive it with `pollster::block_on`, the
    /// browser shell with `wasm_bindgen_futures`.
    ///
    /// Backend selection honors the standard wgpu environment variables
    /// (`WGPU_BACKEND=vulkan|gl|dx12|metal`, `WGPU_POWER_PREF`, ...), which is
    /// the escape hatch for platforms with a flaky default driver.
    pub async fn new(
        surface_source: SurfaceSource,
        width: u32,
        height: u32,
    ) -> Result<Self, RendererError> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let surface = instance
            .create_surface(surface_source())
            .map_err(RendererError::Surface)?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(|_| RendererError::NoAdapter)?;

        let info = adapter.get_info();
        log::info!(
            "adapter: {} ({:?}, driver: {} {})",
            info.name,
            info.backend,
            info.driver,
            info.driver_info
        );

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("wer-device"),
                ..Default::default()
            })
            .await
            .map_err(RendererError::Device)?;

        // This is a debug shell: a driver error should be loud in the log but
        // must not abort the app (wgpu's default handler panics). Surface loss
        // in particular is expected on WSLg and is recovered in
        // `acquire_frame` by recreating the surface.
        device.on_uncaptured_error(std::sync::Arc::new(|error: wgpu::Error| {
            log::error!("wgpu uncaptured error: {error}");
        }));

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or(RendererError::NoSurfaceConfig)?;
        // Frame pacing (phase-6-plan.md M1): vsync (FIFO) is the pacer — the
        // shell blocks in `get_current_texture`/`present` instead of
        // busy-looping. `WER_PRESENT_MODE` overrides for profiling runs
        // (`immediate`/`mailbox` uncap the frame rate to expose true frame
        // cost; wall-clock remains telemetry, never an output input).
        config.present_mode = present_mode_from_env(&surface.get_capabilities(&adapter));
        log::info!("present mode: {:?}", config.present_mode);
        surface.configure(&device, &config);

        let debug_map = DebugMapPipeline::new(&device, config.format);
        let panel_blit = DebugMapPipeline::new(&device, config.format);
        let pov_hud_blit = DebugMapPipeline::new(&device, config.format);

        Ok(Self {
            instance,
            surface_source,
            surface,
            device,
            queue,
            config,
            debug_map,
            panel_blit,
            pov_hud_blit,
            gpu_map: None,
            pov: None,
        })
    }

    /// Replace a lost surface with a freshly created one and configure it.
    /// Returns `false` if the platform refused to create a surface (nothing to
    /// draw to this frame; retried on the next).
    fn recreate_surface(&mut self) -> bool {
        match self.instance.create_surface((self.surface_source)()) {
            Ok(surface) => {
                log::warn!("surface lost; recreated");
                surface.configure(&self.device, &self.config);
                self.surface = surface;
                true
            }
            Err(err) => {
                log::error!("failed to recreate lost surface: {err}");
                false
            }
        }
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
        // The POV depth target tracks the surface size (3d-phase-1-plan.md
        // §5.1); the 2D passes keep no depth attachment and are untouched.
        if let Some(pov) = self.pov.as_mut() {
            pov.ensure_depth(&self.device, width, height);
        }
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
            Cst::Outdated => {
                // The surface itself is alive; the swapchain just needs a
                // reconfigure (resize race, mode change).
                self.surface.configure(&self.device, &self.config);
                None
            }
            Cst::Lost => {
                // The underlying platform surface is gone (compositor
                // restart, WSLg hiccup). Reconfiguring a dead surface is a
                // validation error — build a new one instead.
                self.recreate_surface();
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

    /// Upload a CPU-composed RGBA8 image (`width`×`height`, row-major) and
    /// present it, letterboxed to preserve its aspect ratio regardless of
    /// window shape (phase-1-plan.md section 10). Returns `false` when no
    /// frame was drawn.
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
            // Aspect-preserving viewport: world cells stay square, HUD text
            // stays undistorted. `letterbox_viewport` is public so input code
            // can invert the same mapping for mouse picking.
            let (x, y, w, h) =
                letterbox_viewport((self.config.width, self.config.height), (width, height));
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_pipeline(&self.debug_map.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        true
    }

    /// Present one frame through the Phase 6 GPU-composed map path
    /// (phase-6-plan.md §6.5): delta-upload changed region tiles into the
    /// atlas, optionally refresh the CPU-drawn overlay and panel strips, and
    /// compose per screen pixel in `compose_map.wgsl`.
    ///
    /// Returns the bytes uploaded this frame (`None` when no frame was
    /// drawn). No readback of any kind exists on this path (ADR 0017).
    #[allow(clippy::too_many_arguments)]
    pub fn render_map_gpu(
        &mut self,
        params: &GpuMapParams,
        slots: &[i32],
        uploads: &[MapTileUpload],
        overlay: Option<&[u8]>,
        panel: Option<(&[u8], u32, u32)>,
        clear: [f64; 4],
    ) -> Option<u64> {
        let span = (2 * params.half_regions + 1) as u32;
        let side = span * params.resolution;
        let rebuild = !matches!(
            &self.gpu_map,
            Some(m) if m.capacity == span * span && m.resolution == params.resolution && m.side == side
        );
        if rebuild {
            self.gpu_map = Some(GpuMap::new(
                &self.device,
                self.config.format,
                span * span,
                params.resolution,
                side,
            ));
            log::info!(
                "gpu map atlas built: {span}x{span} slots at {}²",
                params.resolution
            );
        }
        let gpu_map = self.gpu_map.as_ref().expect("just ensured");

        let mut bytes = gpu_map.upload_tiles(&self.queue, uploads);
        if let Some(rgba) = overlay {
            bytes += gpu_map.upload_overlay(&self.queue, rgba);
        }
        gpu_map.write_frame(&self.queue, params, slots);
        let mut panel_size = self.panel_blit.texture.as_ref().map(|&(_, _, w, h)| (w, h));
        if let Some((rgba, w, h)) = panel {
            self.panel_blit
                .upload(&self.device, &self.queue, rgba, w, h);
            bytes += u64::from(w) * u64::from(h) * 4;
            panel_size = Some((w, h));
        }

        let frame = self.acquire_frame()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame-gpu-map"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose-map"),
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
            let panel_w = panel_size.map_or(0, |(w, _)| w);
            let (x, y, vw, vh) = letterbox_viewport(
                (self.config.width, self.config.height),
                (side + panel_w, side),
            );
            let scale = vw / (side + panel_w) as f32;
            let map_w = side as f32 * scale;
            pass.set_viewport(x, y, map_w, vh, 0.0, 1.0);
            let gpu_map = self.gpu_map.as_ref().expect("ensured above");
            gpu_map.draw(&mut pass);
            if let (Some((pw, _)), Some((_, bind_group, _, _))) =
                (panel_size, self.panel_blit.texture.as_ref())
            {
                pass.set_viewport(x + map_w, y, pw as f32 * scale, vh, 0.0, 1.0);
                pass.set_pipeline(&self.panel_blit.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Some(bytes)
    }

    /// Present one POV terrain frame (3d-phase-1-plan.md §5.4), mirroring
    /// [`Self::render_map_gpu`]'s shape: lazily build the POV state on first
    /// call, apply `removes` (vertex buffers return to the pool) and
    /// `uploads` (a re-upload to a live handle swaps contents in place),
    /// write the frame uniform, then one depth-tested pass that clears color
    /// + depth and draws every resident chunk with the shared index buffer.
    ///
    /// Returns `false` when no frame was drawn (surface loss), like the
    /// other entry points. No readback, no API that could ever produce one
    /// (ADR 0017).
    ///
    /// `hud` is an optional small RGBA8 chip (image, width, height) blitted
    /// pixel-exact into the top-right corner after the POV pass — the shell's
    /// FPS counter. World-agnostic like every upload: the renderer just
    /// draws the image it is handed.
    ///
    /// `pov_scale` (already clamped by the shell, `WER_POV_SCALE`) renders
    /// the POV pass at a fraction of the surface resolution and stretches it
    /// up with a linear blit — on a software rasterizer fragment cost is CPU
    /// cost, so 0.5 cuts the raster bill ~4×. The HUD chip stays full-res.
    pub fn render_pov(
        &mut self,
        frame: &PovFrameParams,
        uploads: &[TerrainChunkUpload],
        removes: &[u64],
        clear: [f64; 4],
        hud: Option<(&[u8], u32, u32)>,
        pov_scale: f32,
    ) -> bool {
        if self.pov.is_none() {
            self.pov = Some(pov::Pov::new(&self.device, &self.queue, self.config.format));
            log::info!(
                "pov pipeline built: {} verts/chunk, {} indices shared",
                pov::VERTS_PER_CHUNK,
                pov::INDICES_PER_CHUNK
            );
        }
        let scaled_active = pov_scale < 1.0;
        let (rw, rh) = pov::Pov::render_size(self.config.width, self.config.height, pov_scale);
        let pov = self.pov.as_mut().expect("just ensured");
        pov.ensure_depth(&self.device, rw, rh);
        if scaled_active {
            pov.ensure_scaled(&self.device, rw, rh);
        }
        pov.apply(&self.device, &self.queue, uploads, removes);
        let draws = pov.write_frame(&self.device, &self.queue, frame);

        let Some(surface_frame) = self.acquire_frame() else {
            return false;
        };
        let view = surface_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame-pov"),
            });
        {
            let pov = self.pov.as_ref().expect("ensured above");
            if scaled_active {
                let target = pov.scaled_view().expect("ensure_scaled ran");
                pov.draw(&mut encoder, target, &draws, clear, frame.water);
                pov.blit_scaled(&mut encoder, &view);
            } else {
                pov.draw(&mut encoder, &view, &draws, clear, frame.water);
            }
        }
        // The HUD chip: a second tiny pass loading the POV result and
        // blitting the image into the top-right corner, pixel-exact.
        if let Some((rgba, w, h)) = hud {
            self.pov_hud_blit
                .upload(&self.device, &self.queue, rgba, w, h);
            if let Some((_, bind_group, w, h)) = self.pov_hud_blit.texture.as_ref() {
                let (sw, sh) = (self.config.width, self.config.height);
                const PAD: u32 = 8;
                if *w + PAD <= sw && *h + PAD <= sh {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("pov-hud-chip"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_viewport(
                        (sw - w - PAD) as f32,
                        PAD as f32,
                        *w as f32,
                        *h as f32,
                        0.0,
                        1.0,
                    );
                    pass.set_pipeline(&self.pov_hud_blit.pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        surface_frame.present();
        true
    }
}
