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
    PovFrameParams, PovOrganismBufferStats, PovOrganismInstance, PovOrganismUpload, PovVertex,
    TerrainChunkUpload, DETAIL_OCTAVES, POV_ORGANISM_INSTANCE_BYTES, SHADER_POV_ORGANISM,
    SHADER_POV_SKY, SHADER_POV_TERRAIN, SHADER_POV_WATER,
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

/// An exact physical-pixel viewport on the presentation surface.
///
/// Layout ownership stays in `viewer-host`; this renderer-side transport type
/// only prevents platform adapters from losing integer edge coordinates while
/// handing the resolved rectangle to wgpu.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceViewport {
    /// Left edge in surface pixels.
    pub x: u32,
    /// Top edge in surface pixels.
    pub y: u32,
    /// Width in surface pixels.
    pub width: u32,
    /// Height in surface pixels.
    pub height: u32,
}

/// Optional new pixels for the shell-owned POV information texture.
#[derive(Debug, Clone, Copy)]
pub struct PovInformationUpload<'a> {
    /// sRGB-encoded RGBA8 pixels in row-major order.
    pub rgba: &'a [u8],
    /// Source texture width.
    pub width: u32,
    /// Source texture height.
    pub height: u32,
}

/// One shell-owned information surface composed after the POV pass.
#[derive(Debug, Clone, Copy)]
pub struct PovInformationSurface<'a> {
    /// Replacement pixels, or `None` to retain the existing texture.
    pub upload: Option<PovInformationUpload<'a>>,
    /// Exact destination on the presentation surface.
    pub viewport: SurfaceViewport,
}

impl SurfaceViewport {
    /// Construct an exact physical viewport.
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether this non-empty viewport is wholly inside a surface.
    #[must_use]
    pub const fn is_contained_by(self, surface_width: u32, surface_height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self.x <= surface_width
            && self.y <= surface_height
            && self.width <= surface_width - self.x
            && self.height <= surface_height - self.y
    }

    fn set(self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_viewport(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
            0.0,
            1.0,
        );
    }
}

fn scale_viewport(
    viewport: SurfaceViewport,
    source: (u32, u32),
    destination: (u32, u32),
) -> SurfaceViewport {
    debug_assert!(source.0 > 0 && source.1 > 0);
    debug_assert!(destination.0 > 0 && destination.1 > 0);
    let scale_edge = |edge: u32, source: u32, destination: u32| {
        ((u64::from(edge) * u64::from(destination) + u64::from(source) / 2) / u64::from(source))
            as u32
    };
    let scale_axis = |start: u32, length: u32, source: u32, destination: u32| {
        // Rounding both edges can collapse a small but valid pane at a low
        // render scale. Keep at least one destination pixel: WebGPU rejects a
        // zero-area viewport, and one pixel is the faithful lower bound.
        let scaled_start = scale_edge(start, source, destination).min(destination - 1);
        let scaled_end = scale_edge(start + length, source, destination)
            .max(scaled_start + 1)
            .min(destination);
        (scaled_start, scaled_end)
    };
    let (left, right) = scale_axis(viewport.x, viewport.width, source.0, destination.0);
    let (top, bottom) = scale_axis(viewport.y, viewport.height, source.1, destination.1);
    SurfaceViewport::new(
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    )
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
    /// The format every pipeline targets and surface views are created
    /// with: the sRGB twin of the swapchain format. Equal to
    /// `config.format` everywhere except the WebGPU canvas route, where the
    /// swapchain must stay non-sRGB and the encode happens through a view
    /// format instead.
    render_format: wgpu::TextureFormat,
    /// Observed platform-surface loss events. Each event attempts recreation;
    /// timeouts, occlusion, and resize races do not increment this diagnostic
    /// counter.
    surface_losses: u32,
    debug_map: DebugMapPipeline,
    /// Second blit pipeline for the HUD panel strip in the GPU-map path.
    panel_blit: DebugMapPipeline,
    /// Third blit pipeline for the shell-owned POV information surface.
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
    /// Exact swapchain format selected for this platform surface. Kept as a
    /// display string so viewer telemetry does not expose a graphics-API type
    /// across the renderer boundary.
    #[must_use]
    pub fn surface_format_name(&self) -> String {
        format!("{:?}", self.config.format)
    }

    /// Number of platform-surface loss events observed by this renderer.
    #[must_use]
    pub const fn surface_losses(&self) -> u32 {
        self.surface_losses
    }

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
        // Every pipeline here writes linear color and relies on an sRGB
        // render target to encode it. Native surfaces default to an sRGB
        // swapchain format (the first arm), but a WebGPU canvas only
        // accepts the non-sRGB variant as its swapchain format — presenting
        // linear values raw, which reads as a uniformly dark scene — and
        // instead exposes the sRGB twin as a *view* format. `render_format`
        // is what every pipeline targets and every surface view is created
        // with, so both routes encode identically.
        let capabilities = surface.get_capabilities(&adapter);
        let srgb = config.format.add_srgb_suffix();
        let render_format = if srgb == config.format {
            config.format
        } else if capabilities.formats.contains(&srgb) {
            log::info!("surface format {:?} -> sRGB {:?}", config.format, srgb);
            config.format = srgb;
            srgb
        } else {
            log::info!(
                "surface format {:?} stays; rendering through sRGB view {:?}",
                config.format,
                srgb
            );
            config.view_formats.push(srgb);
            srgb
        };
        // Frame pacing (phase-6-plan.md M1): vsync (FIFO) is the pacer — the
        // shell blocks in `get_current_texture`/`present` instead of
        // busy-looping. `WER_PRESENT_MODE` overrides for profiling runs
        // (`immediate`/`mailbox` uncap the frame rate to expose true frame
        // cost; wall-clock remains telemetry, never an output input).
        config.present_mode = present_mode_from_env(&capabilities);
        log::info!("present mode: {:?}", config.present_mode);
        surface.configure(&device, &config);

        let debug_map = DebugMapPipeline::new(&device, render_format);
        let panel_blit = DebugMapPipeline::new(&device, render_format);
        let pov_hud_blit = DebugMapPipeline::new(&device, render_format);

        Ok(Self {
            instance,
            surface_source,
            surface,
            device,
            queue,
            config,
            render_format,
            surface_losses: 0,
            debug_map,
            panel_blit,
            pov_hud_blit,
            gpu_map: None,
            pov: None,
        })
    }

    /// A render-target view of an acquired surface frame, in
    /// [`Self::render_format`](field) — the sRGB view that makes the WebGPU
    /// canvas route encode like the native sRGB swapchain.
    fn surface_view(&self, frame: &wgpu::SurfaceTexture) -> wgpu::TextureView {
        frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.render_format),
            ..Default::default()
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
        if width == 0 || height == 0 || (self.config.width == width && self.config.height == height)
        {
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
                self.surface_losses = self.surface_losses.saturating_add(1);
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

        let view = self.surface_view(&frame);
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
        let (x, y, viewport_width, viewport_height) =
            letterbox_viewport((self.config.width, self.config.height), (width, height));
        self.render_map_in(
            rgba,
            width,
            height,
            SurfaceViewport::new(
                x.round() as u32,
                y.round() as u32,
                viewport_width.round() as u32,
                viewport_height.round() as u32,
            ),
            clear,
        )
    }

    /// Upload and present a CPU-composed map into an exact physical viewport
    /// resolved by the shared viewer layout.
    pub fn render_map_in(
        &mut self,
        rgba: &[u8],
        width: u32,
        height: u32,
        viewport: SurfaceViewport,
        clear: [f64; 4],
    ) -> bool {
        self.debug_map
            .upload(&self.device, &self.queue, rgba, width, height);
        if self.debug_map.texture.is_none()
            || !viewport.is_contained_by(self.config.width, self.config.height)
        {
            if self.debug_map.texture.is_some() {
                log::error!(
                    "map viewport {viewport:?} escapes surface {:?}",
                    self.size()
                );
            }
            return false;
        }

        let Some(frame) = self.acquire_frame() else {
            return false;
        };
        let (_, bind_group, _, _) = self.debug_map.texture.as_ref().expect("uploaded above");
        let view = self.surface_view(&frame);
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
            viewport.set(&mut pass);
            pass.set_pipeline(&self.debug_map.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        true
    }

    /// Upload a CPU-composed square map and bitmap panel separately, then
    /// present both in exact shared-layout viewports during one surface pass.
    /// Keeping the sources separate makes the map/panel seam integer-exact, so
    /// the Map viewport used for pointer inversion is precisely what was drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn render_map_and_panel_in(
        &mut self,
        map_rgba: &[u8],
        map_width: u32,
        map_height: u32,
        panel_rgba: &[u8],
        panel_width: u32,
        panel_height: u32,
        map_viewport: SurfaceViewport,
        panel_viewport: SurfaceViewport,
        clear: [f64; 4],
    ) -> bool {
        self.debug_map
            .upload(&self.device, &self.queue, map_rgba, map_width, map_height);
        self.panel_blit.upload(
            &self.device,
            &self.queue,
            panel_rgba,
            panel_width,
            panel_height,
        );
        if self.debug_map.texture.is_none()
            || self.panel_blit.texture.is_none()
            || !map_viewport.is_contained_by(self.config.width, self.config.height)
            || !panel_viewport.is_contained_by(self.config.width, self.config.height)
        {
            if self.debug_map.texture.is_some() && self.panel_blit.texture.is_some() {
                log::error!(
                    "map/panel viewports {map_viewport:?}/{panel_viewport:?} escape surface {:?}",
                    self.size()
                );
            }
            return false;
        }

        let Some(frame) = self.acquire_frame() else {
            return false;
        };
        let (_, map_bind_group, _, _) = self.debug_map.texture.as_ref().expect("uploaded above");
        let (_, panel_bind_group, _, _) = self.panel_blit.texture.as_ref().expect("uploaded above");
        let view = self.surface_view(&frame);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame-map-panel"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("debug-map-panel"),
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
            map_viewport.set(&mut pass);
            pass.set_pipeline(&self.debug_map.pipeline);
            pass.set_bind_group(0, map_bind_group, &[]);
            pass.draw(0..3, 0..1);

            panel_viewport.set(&mut pass);
            pass.set_pipeline(&self.panel_blit.pipeline);
            pass.set_bind_group(0, panel_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        true
    }

    /// Present one frame through the Phase 6 GPU-composed map path
    /// (phase-6-plan.md §6.5): delta-upload changed region tiles into the
    /// atlas, optionally refresh the CPU-drawn pre/post-grid overlay and panel
    /// strips, and compose per screen pixel in `compose_map.wgsl`.
    ///
    /// Returns the bytes uploaded this frame (`None` when no frame was
    /// drawn). No readback of any kind exists on this path (ADR 0017).
    #[allow(clippy::too_many_arguments)]
    pub fn render_map_gpu(
        &mut self,
        params: &GpuMapParams,
        slots: &[i32],
        uploads: &[MapTileUpload],
        pre_grid_overlay: Option<&[u8]>,
        post_grid_overlay: Option<&[u8]>,
        panel: Option<(&[u8], u32, u32)>,
        clear: [f64; 4],
    ) -> Option<u64> {
        let span = (2 * params.half_regions + 1) as u32;
        let side = span * params.resolution;
        let panel_width = panel.map_or_else(
            || {
                self.panel_blit
                    .texture
                    .as_ref()
                    .map_or(0, |&(_, _, width, _)| width)
            },
            |(_, width, _)| width,
        );
        let (x, y, width, height) = letterbox_viewport(
            (self.config.width, self.config.height),
            (side + panel_width, side),
        );
        let scale = width / (side + panel_width) as f32;
        let map_width = side as f32 * scale;
        let map_viewport = SurfaceViewport::new(
            x.round() as u32,
            y.round() as u32,
            map_width.round() as u32,
            height.round() as u32,
        );
        let panel_viewport = (panel_width > 0).then_some(SurfaceViewport::new(
            (x + map_width).round() as u32,
            y.round() as u32,
            (panel_width as f32 * scale).round() as u32,
            height.round() as u32,
        ));
        self.render_map_gpu_in(
            params,
            slots,
            uploads,
            pre_grid_overlay,
            post_grid_overlay,
            panel,
            map_viewport,
            panel_viewport,
            clear,
        )
    }

    /// Present a GPU-composed map in exact shared-layout rectangles.
    ///
    /// `panel_viewport` is used only when a panel upload is supplied. Browser
    /// Map mode passes the fitted square and no panel; the temporary native
    /// single-view wrapper above retains its existing map-plus-panel layout.
    #[allow(clippy::too_many_arguments)]
    pub fn render_map_gpu_in(
        &mut self,
        params: &GpuMapParams,
        slots: &[i32],
        uploads: &[MapTileUpload],
        pre_grid_overlay: Option<&[u8]>,
        post_grid_overlay: Option<&[u8]>,
        panel: Option<(&[u8], u32, u32)>,
        map_viewport: SurfaceViewport,
        panel_viewport: Option<SurfaceViewport>,
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
                self.render_format,
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
        if let Some(rgba) = pre_grid_overlay {
            bytes += gpu_map.upload_pre_grid_overlay(&self.queue, rgba);
        }
        if let Some(rgba) = post_grid_overlay {
            bytes += gpu_map.upload_post_grid_overlay(&self.queue, rgba);
        }
        gpu_map.write_frame(&self.queue, params, slots);
        if let Some((rgba, w, h)) = panel {
            self.panel_blit
                .upload(&self.device, &self.queue, rgba, w, h);
            bytes += u64::from(w) * u64::from(h) * 4;
        }

        if !map_viewport.is_contained_by(self.config.width, self.config.height)
            || panel_viewport.is_some_and(|viewport| {
                !viewport.is_contained_by(self.config.width, self.config.height)
            })
        {
            log::error!(
                "GPU map/panel viewports {map_viewport:?}/{panel_viewport:?} escape surface {:?}",
                self.size()
            );
            return None;
        }

        let frame = self.acquire_frame()?;
        let view = self.surface_view(&frame);
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
            map_viewport.set(&mut pass);
            let gpu_map = self.gpu_map.as_ref().expect("ensured above");
            gpu_map.draw(&mut pass);
            if let (Some(viewport), Some((_, bind_group, _, _))) =
                (panel_viewport, self.panel_blit.texture.as_ref())
            {
                viewport.set(&mut pass);
                pass.set_pipeline(&self.panel_blit.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Some(bytes)
    }

    /// Present one POV frame (3d-phase-4-plan.md §6), mirroring
    /// [`Self::render_map_gpu`]'s shape: lazily build the POV state on first
    /// call, apply `removes` (vertex buffers return to the pool) and
    /// `uploads` (a re-upload to a live handle swaps contents in place),
    /// write the frame uniforms, then record the optional directional caster
    /// pass followed by terrain, organism, river, and sea color draws.
    /// `organisms` is tri-state: `None` retains the grow-only buffers,
    /// `Some(non-empty)` replaces both batches, and `Some(empty)` clears them.
    ///
    /// Returns `false` when no frame was drawn (surface loss), like the
    /// other entry points. No readback, no API that could ever produce one
    /// (ADR 0017).
    ///
    /// `pov_viewport` is the exact pane used for both projection and raster;
    /// opaque information pixels never cover the camera's center.
    ///
    /// `information` is an optional RGBA8 surface plus exact destination. Its
    /// inner upload is tri-state: `Some` replaces the texture, `None` retains
    /// and redraws the existing texture, and the outer `None` draws no panel.
    ///
    /// `pov_scale` (already clamped by the shell, `WER_POV_SCALE`) renders
    /// the POV pass at a fraction of the surface resolution and stretches it
    /// up with a linear blit — on a software rasterizer fragment cost is CPU
    /// cost, so 0.5 cuts the raster bill ~4×. The information surface is
    /// composed at the destination viewport's full resolution.
    #[allow(clippy::too_many_arguments)] // Upload tri-state stays explicit at the renderer boundary.
    pub fn render_pov(
        &mut self,
        frame: &PovFrameParams,
        uploads: &[TerrainChunkUpload],
        removes: &[u64],
        organisms: Option<&PovOrganismUpload>,
        clear: [f64; 4],
        pov_viewport: SurfaceViewport,
        information: Option<PovInformationSurface<'_>>,
        pov_scale: f32,
    ) -> bool {
        let surface_size = (self.config.width, self.config.height);
        if !pov_viewport.is_contained_by(surface_size.0, surface_size.1)
            || information.is_some_and(|information| {
                !information
                    .viewport
                    .is_contained_by(surface_size.0, surface_size.1)
            })
        {
            log::error!(
                "POV/information viewports {pov_viewport:?}/{:?} escape surface {surface_size:?}",
                information.map(|information| information.viewport)
            );
            return false;
        }
        if self.pov.is_none() {
            self.pov = Some(pov::Pov::new(&self.device, &self.queue, self.render_format));
            log::info!(
                "pov pipeline built: {} verts/chunk, {} indices shared",
                pov::VERTS_PER_CHUNK,
                pov::INDICES_PER_CHUNK
            );
        }
        let scaled_active = pov_scale < 1.0;
        let (rw, rh) = pov::Pov::render_size(self.config.width, self.config.height, pov_scale);
        let draw_viewport = if scaled_active {
            scale_viewport(pov_viewport, surface_size, (rw, rh))
        } else {
            pov_viewport
        };
        let pov = self.pov.as_mut().expect("just ensured");
        pov.ensure_depth(&self.device, rw, rh);
        pov.ensure_shadow(&self.device, frame.shadow_resolution);
        if scaled_active {
            pov.ensure_scaled(&self.device, rw, rh);
        }
        pov.apply(&self.device, &self.queue, uploads, removes);
        pov.apply_organisms(&self.device, &self.queue, organisms);
        let draws = pov.write_frame(&self.device, &self.queue, frame);

        let Some(surface_frame) = self.acquire_frame() else {
            return false;
        };
        let view = self.surface_view(&surface_frame);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-frame-pov"),
            });
        {
            let pov = self.pov.as_ref().expect("ensured above");
            if scaled_active {
                let target = pov.scaled_view().expect("ensure_scaled ran");
                pov.draw(
                    &mut encoder,
                    target,
                    draw_viewport,
                    &draws,
                    clear,
                    frame.water,
                    frame.shadow_ao && frame.shadow_resolution > 0,
                );
                pov.blit_scaled(&mut encoder, &view);
            } else {
                pov.draw(
                    &mut encoder,
                    &view,
                    draw_viewport,
                    &draws,
                    clear,
                    frame.water,
                    frame.shadow_ao && frame.shadow_resolution > 0,
                );
            }
        }
        // The information surface: a second pass loading the POV result and
        // blitting the shell-owned bitmap into its exact shared viewport.
        if let Some(information) = information {
            if let Some(upload) = information.upload {
                self.pov_hud_blit.upload(
                    &self.device,
                    &self.queue,
                    upload.rgba,
                    upload.width,
                    upload.height,
                );
            }
            if let Some((_, bind_group, _, _)) = self.pov_hud_blit.texture.as_ref() {
                let (sw, sh) = (self.config.width, self.config.height);
                let viewport = information.viewport;
                if viewport.is_contained_by(sw, sh) {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("pov-information-surface"),
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
                    viewport.set(&mut pass);
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

    /// Packed live/capacity counts for the lazily built POV organism buffers.
    #[must_use]
    pub fn pov_organism_stats(&self) -> Option<PovOrganismBufferStats> {
        self.pov.as_ref().map(pov::Pov::organism_stats)
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::{scale_viewport, SurfaceViewport};

    #[test]
    fn physical_viewport_containment_handles_edges_odds_and_overflow() {
        assert!(SurfaceViewport::new(0, 0, 901, 701).is_contained_by(901, 701));
        assert!(SurfaceViewport::new(337, 0, 227, 227).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(0, 0, 0, 10).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(900, 700, 2, 1).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(u32::MAX, 0, 2, 1).is_contained_by(901, 701));
    }

    #[test]
    fn reduced_resolution_viewport_preserves_shared_edges_and_containment() {
        let surface = (901, 701);
        let reduced = (451, 351);
        let left = scale_viewport(SurfaceViewport::new(37, 19, 337, 663), surface, reduced);
        let right = scale_viewport(SurfaceViewport::new(374, 19, 490, 663), surface, reduced);

        assert!(left.is_contained_by(reduced.0, reduced.1));
        assert!(right.is_contained_by(reduced.0, reduced.1));
        assert_eq!(left.x + left.width, right.x);
        assert_eq!(left.y, right.y);
        assert_eq!(left.height, right.height);
    }

    #[test]
    fn full_surface_viewport_scales_to_full_render_target() {
        assert_eq!(
            scale_viewport(
                SurfaceViewport::new(0, 0, 1280, 721),
                (1280, 721),
                (640, 361),
            ),
            SurfaceViewport::new(0, 0, 640, 361)
        );
    }

    #[test]
    fn every_nonempty_interval_stays_nonempty_when_reduced() {
        for source in 1..=12 {
            for destination in 1..=12 {
                for start in 0..source {
                    for length in 1..=source - start {
                        let scaled = scale_viewport(
                            SurfaceViewport::new(start, start, length, length),
                            (source, source),
                            (destination, destination),
                        );
                        assert!(
                            scaled.is_contained_by(destination, destination),
                            "{start}+{length}/{source} -> {scaled:?} in {destination}",
                        );
                    }
                }
            }
        }
    }
}
