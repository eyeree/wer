//! `renderer` — the wgpu-backed presentation layer.
//!
//! The renderer owns the GPU device, queue, and surface plus the final
//! presentation transaction: one acquired frame can contain Map, POV, or both
//! in Split, followed by the shared information surface and focus decoration.
//! Map accepts either shell-composed CPU pixels or the delta-uploaded region
//! atlas with derived WGSL refinement (ADR 0017); POV renders pane-sized
//! terrain, water, organisms, and shadows before blitting into its destination
//! rectangle. `viewer-host` remains the authority for semantic state and exact
//! physical layout. The live renderer exposes no readback; diagnostic POV
//! pixels use the explicitly file-bound capture path from ADR 0021.
//!
//! The crate stays WebGPU-compatible: it targets `wgpu` (which maps to native
//! Vulkan/Metal/DX and to WebGPU in the browser) and uses only WGSL shaders.

use core::fmt;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

pub mod gpumap;
pub mod pov;
pub use gpumap::{GpuMap, GpuMapParams, MapTileUpload, RefineOctaveParams};
pub use pov::{
    PovFrameParams, PovOrganismBufferStats, PovOrganismInstance, PovOrganismUpload, PovVertex,
    TerrainChunkUpload, DETAIL_OCTAVES, POV_ORGANISM_INSTANCE_BYTES, SHADER_POV_ORGANISM,
    SHADER_POV_TERRAIN, SHADER_POV_WATER,
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

/// Fit an image without distortion inside an arbitrary parent rectangle.
///
/// This is the rectangle form of [`letterbox_viewport`]. Keeping the parent
/// origin in the result lets Map use the same helper in a full surface or one
/// half of a multi-view frame.
#[must_use]
pub fn letterbox_viewport_in(parent: SurfaceViewport, image: (u32, u32)) -> SurfaceViewport {
    let (x, y, width, height) = letterbox_viewport((parent.width, parent.height), image);
    SurfaceViewport::new(
        parent.x.saturating_add(x.round() as u32),
        parent.y.saturating_add(y.round() as u32),
        width.round() as u32,
        height.round() as u32,
    )
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

/// Optional new pixels for a shell-owned information texture.
#[derive(Debug, Clone, Copy)]
pub struct InformationUpload<'a> {
    /// sRGB-encoded RGBA8 pixels in row-major order.
    pub rgba: &'a [u8],
    /// Source texture width.
    pub width: u32,
    /// Source texture height.
    pub height: u32,
}

/// One shell-owned information surface composed after the view passes.
#[derive(Debug, Clone, Copy)]
pub struct InformationSurface<'a> {
    /// Replacement pixels, or `None` to retain the existing texture.
    pub upload: Option<InformationUpload<'a>>,
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

    /// Whether this rectangle shares at least one physical pixel with
    /// `other`. Touching half-open edges do not overlap.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x.saturating_add(other.width)
            && other.x < self.x.saturating_add(self.width)
            && self.y < other.y.saturating_add(other.height)
            && other.y < self.y.saturating_add(self.height)
    }

    pub(crate) fn set(self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_viewport(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(self.x, self.y, self.width, self.height);
    }
}

/// Logical passes recorded for one live surface frame.
///
/// The list is device-free test evidence for the ordering contract; POV's
/// offscreen color/depth clear is distinct from the single surface clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePassKind {
    /// Clear the presentation surface once.
    SurfaceClear,
    /// Compose the CPU or atlas Map pane, loading the cleared surface.
    Map,
    /// Populate the independent directional shadow target.
    PovShadow,
    /// Render POV color/depth into its pane-sized offscreen target.
    PovOffscreen,
    /// Composite the POV offscreen color into its destination pane.
    PovComposite,
    /// Draw the Map-associated bitmap information surface.
    MapInformation,
    /// Draw the POV-associated bitmap information surface.
    PovInformation,
    /// Draw focus decoration last.
    Focus,
}

/// Names the non-overlapping surface regions in a frame-plan error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRegion {
    /// Map destination.
    Map,
    /// POV destination.
    Pov,
    /// Map information/HUD destination.
    MapInformation,
    /// POV information/HUD destination.
    PovInformation,
    /// Focus decoration bounds.
    Focus,
}

/// A device-free request for the passes and rectangles in one frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FramePlanRequest {
    /// Surface dimensions in physical pixels.
    pub surface: (u32, u32),
    /// Optional Map destination.
    pub map: Option<SurfaceViewport>,
    /// Optional POV destination.
    pub pov: Option<SurfaceViewport>,
    /// Optional Map information destination.
    pub map_information: Option<SurfaceViewport>,
    /// Optional POV information destination.
    pub pov_information: Option<SurfaceViewport>,
    /// Whether the POV pass records its independent shadow pass.
    pub pov_shadows: bool,
    /// Optional focus decoration bounds. It may overlap the focused pane.
    pub focus: Option<SurfaceViewport>,
}

/// Why a multi-view frame layout cannot be recorded safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePlanError {
    /// A non-empty region escaped the surface.
    OutsideSurface(FrameRegion, SurfaceViewport),
    /// Two independently owned color regions overlap.
    Overlap(FrameRegion, FrameRegion),
}

/// Ordered, validated recording plan for one live frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePassPlan {
    passes: [FramePassKind; 8],
    len: u8,
}

impl FramePassPlan {
    /// Validate rectangles and construct the fixed recording order.
    pub fn new(request: FramePlanRequest) -> Result<Self, FramePlanError> {
        let regions = [
            (FrameRegion::Map, request.map),
            (FrameRegion::Pov, request.pov),
            (FrameRegion::MapInformation, request.map_information),
            (FrameRegion::PovInformation, request.pov_information),
        ];
        for (region, viewport) in regions
            .into_iter()
            .filter_map(|(kind, rect)| rect.map(|r| (kind, r)))
        {
            if !viewport.is_contained_by(request.surface.0, request.surface.1) {
                return Err(FramePlanError::OutsideSurface(region, viewport));
            }
        }
        if let Some(focus) = request.focus {
            if !focus.is_contained_by(request.surface.0, request.surface.1) {
                return Err(FramePlanError::OutsideSurface(FrameRegion::Focus, focus));
            }
        }
        for first in 0..regions.len() {
            let Some(first_rect) = regions[first].1 else {
                continue;
            };
            for second in first + 1..regions.len() {
                let Some(second_rect) = regions[second].1 else {
                    continue;
                };
                if first_rect.overlaps(second_rect) {
                    return Err(FramePlanError::Overlap(regions[first].0, regions[second].0));
                }
            }
        }

        let mut passes = [FramePassKind::SurfaceClear; 8];
        let mut len = 1usize;
        let mut push = |pass| {
            passes[len] = pass;
            len += 1;
        };
        if request.map.is_some() {
            push(FramePassKind::Map);
        }
        if request.pov.is_some() {
            if request.pov_shadows {
                push(FramePassKind::PovShadow);
            }
            push(FramePassKind::PovOffscreen);
            push(FramePassKind::PovComposite);
        }
        if request.map_information.is_some() {
            push(FramePassKind::MapInformation);
        }
        if request.pov_information.is_some() {
            push(FramePassKind::PovInformation);
        }
        if request.focus.is_some() {
            push(FramePassKind::Focus);
        }
        Ok(Self {
            passes,
            len: len as u8,
        })
    }

    /// Fixed pass order used by the renderer.
    #[must_use]
    pub fn passes(&self) -> &[FramePassKind] {
        &self.passes[..usize::from(self.len)]
    }

    /// Successful surface lifecycle for any plan: exactly one of each.
    #[must_use]
    pub const fn successful_surface_lifecycle(&self) -> FrameLifecycleCounters {
        FrameLifecycleCounters {
            acquire_attempts: 1,
            acquired: 1,
            surface_clears: 1,
            submissions: 1,
            presents: 1,
        }
    }
}

/// Cumulative (or plan-local) surface-frame lifecycle counters.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FrameLifecycleCounters {
    /// Calls that attempted to acquire a swapchain image.
    pub acquire_attempts: u64,
    /// Swapchain images successfully acquired.
    pub acquired: u64,
    /// Clears recorded against the presentation surface.
    pub surface_clears: u64,
    /// Command-buffer submissions.
    pub submissions: u64,
    /// Presented swapchain images.
    pub presents: u64,
}

/// A CPU or atlas-composed Map source for [`MultiViewFrame`].
#[derive(Debug, Clone, Copy)]
pub enum MapFrameSource<'a> {
    /// CPU-composed sRGB RGBA8 pixels.
    Cpu {
        /// Pixel bytes.
        rgba: &'a [u8],
        /// Image width.
        width: u32,
        /// Image height.
        height: u32,
    },
    /// GPU atlas inputs and delta uploads.
    Gpu {
        /// Per-frame composition uniforms.
        params: &'a GpuMapParams,
        /// Atlas slot table.
        slots: &'a [i32],
        /// Changed tile uploads.
        uploads: &'a [MapTileUpload],
        /// Changed pre-grid overlay, if any.
        pre_grid_overlay: Option<&'a [u8]>,
        /// Changed post-grid overlay, if any.
        post_grid_overlay: Option<&'a [u8]>,
    },
}

/// One optional Map pane in a multi-view frame.
#[derive(Debug, Clone, Copy)]
pub struct MapFramePane<'a> {
    /// CPU or GPU source.
    pub source: MapFrameSource<'a>,
    /// Exact destination rectangle.
    pub viewport: SurfaceViewport,
    /// Optional final bitmap panel/HUD associated with Map.
    pub information: Option<InformationSurface<'a>>,
}

/// One optional POV pane in a multi-view frame.
#[derive(Debug, Clone, Copy)]
pub struct PovFramePane<'a> {
    /// Camera/light/fog uniforms.
    pub frame: &'a PovFrameParams,
    /// Changed terrain chunks.
    pub uploads: &'a [TerrainChunkUpload],
    /// Evicted chunk handles.
    pub removes: &'a [u64],
    /// Organism replacement tri-state.
    pub organisms: Option<&'a PovOrganismUpload>,
    /// Exact destination rectangle.
    pub viewport: SurfaceViewport,
    /// Optional final bitmap panel/HUD associated with POV.
    pub information: Option<InformationSurface<'a>>,
    /// Pane-relative render scale.
    pub render_scale: f32,
}

/// Optional final focus-border decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusDecoration {
    /// Bounds of the focused pane.
    pub viewport: SurfaceViewport,
    /// Border thickness in physical pixels.
    pub thickness: u32,
}

/// Everything recorded and presented through one live surface frame.
#[derive(Debug, Clone, Copy)]
pub struct MultiViewFrame<'a> {
    /// Linear clear color for the presentation surface and POV sky.
    pub clear: [f64; 4],
    /// Optional Map pane.
    pub map: Option<MapFramePane<'a>>,
    /// Optional POV pane.
    pub pov: Option<PovFramePane<'a>>,
    /// Optional focus border, drawn last.
    pub focus: Option<FocusDecoration>,
}

/// Outcome of one multi-view surface attempt.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MultiViewFrameResult {
    /// Whether a surface image was submitted and presented.
    pub presented: bool,
    /// Atlas/overlay/information bytes uploaded while preparing Map.
    pub map_upload_bytes: u64,
    /// Whether a requested Map pane reached the presented frame.
    pub map_drawn: bool,
    /// Whether a requested POV pane reached the presented frame.
    pub pov_drawn: bool,
}

#[cfg(test)]
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
    /// Device-loss callbacks can arrive asynchronously (including from the
    /// browser WebGPU implementation), so hosts poll this shared counter at
    /// the start of a logical frame and reduce capability loss before the one
    /// world update. Unlike surface loss, a lost device cannot be repaired by
    /// recreating only the swapchain.
    device_losses: Arc<AtomicU32>,
    debug_map: DebugMapPipeline,
    /// Second blit pipeline for the HUD panel strip in the GPU-map path.
    panel_blit: DebugMapPipeline,
    /// Third blit pipeline for the shell-owned POV information surface.
    pov_hud_blit: DebugMapPipeline,
    /// Opaque one-pixel source used to draw final focus-border strips.
    focus_blit: DebugMapPipeline,
    /// The Phase 6 atlas-composed map path (phase-6-plan.md §6.5), built
    /// lazily on the first GPU-mode frame.
    gpu_map: Option<GpuMap>,
    /// The POV terrain path (3d-phase-1-plan.md §5), built lazily on the
    /// first POV frame. Owns the renderer's only depth target.
    pov: Option<pov::Pov>,
    /// Observable proof that all live wrappers share one surface lifecycle.
    frame_lifecycle: FrameLifecycleCounters,
}

impl fmt::Debug for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Renderer")
            .field("config", &self.config)
            .field("surface_losses", &self.surface_losses)
            .field("device_losses", &self.device_losses())
            .finish_non_exhaustive()
    }
}

fn record_device_loss(counter: &AtomicU32) {
    // A renderer normally observes at most one loss because wgpu installs a
    // one-shot callback per device. Saturation still keeps this cumulative
    // diagnostic well-defined if a backend invokes it repeatedly or a test
    // seeds the counter near its limit.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |losses| {
        Some(losses.saturating_add(1))
    });
}

fn valid_rgba_upload(rgba: &[u8], width: u32, height: u32) -> bool {
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4));
    width > 0 && height > 0 && u64::try_from(rgba.len()).ok() == expected
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InformationPreflightError {
    InvalidUpload,
    MissingRetainedTexture,
}

fn preflight_information(
    information: Option<InformationSurface<'_>>,
    retained_texture_exists: bool,
) -> Result<(), InformationPreflightError> {
    let Some(information) = information else {
        return Ok(());
    };
    match information.upload {
        Some(upload) if valid_rgba_upload(upload.rgba, upload.width, upload.height) => Ok(()),
        Some(_) => Err(InformationPreflightError::InvalidUpload),
        None if retained_texture_exists => Ok(()),
        None => Err(InformationPreflightError::MissingRetainedTexture),
    }
}

fn horizontal_split(
    combined: SurfaceViewport,
    left_units: u32,
    total_units: u32,
) -> (SurfaceViewport, SurfaceViewport) {
    debug_assert!(total_units > 0 && left_units <= total_units);
    let left_width = ((u64::from(combined.width) * u64::from(left_units)
        + u64::from(total_units) / 2)
        / u64::from(total_units)) as u32;
    let left = SurfaceViewport::new(combined.x, combined.y, left_width, combined.height);
    let right = SurfaceViewport::new(
        left.x.saturating_add(left.width),
        combined.y,
        combined.width.saturating_sub(left.width),
        combined.height,
    );
    (left, right)
}

fn record_bitmap_surface(
    encoder: &mut wgpu::CommandEncoder,
    surface: &wgpu::TextureView,
    pipeline: &DebugMapPipeline,
    viewport: SurfaceViewport,
    label: &'static str,
) {
    let Some((_, bind_group, _, _)) = pipeline.texture.as_ref() else {
        return;
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: surface,
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
    pass.set_pipeline(&pipeline.pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

fn focus_border_rects(focus: FocusDecoration) -> [Option<SurfaceViewport>; 4] {
    let rect = focus.viewport;
    let thickness = focus.thickness.min(rect.width).min(rect.height);
    if thickness == 0 {
        return [None; 4];
    }
    let bottom_y = rect.y.saturating_add(rect.height.saturating_sub(thickness));
    let inner_height = rect.height.saturating_sub(thickness.saturating_mul(2));
    let right_x = rect.x.saturating_add(rect.width.saturating_sub(thickness));
    [
        Some(SurfaceViewport::new(rect.x, rect.y, rect.width, thickness)),
        (bottom_y != rect.y).then_some(SurfaceViewport::new(
            rect.x, bottom_y, rect.width, thickness,
        )),
        (inner_height > 0).then_some(SurfaceViewport::new(
            rect.x,
            rect.y.saturating_add(thickness),
            thickness,
            inner_height,
        )),
        (inner_height > 0 && right_x != rect.x).then_some(SurfaceViewport::new(
            right_x,
            rect.y.saturating_add(thickness),
            thickness,
            inner_height,
        )),
    ]
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

    /// Number of asynchronous GPU device-loss callbacks observed.
    #[must_use]
    pub fn device_losses(&self) -> u32 {
        self.device_losses.load(Ordering::Relaxed)
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
        let device_losses = Arc::new(AtomicU32::new(0));
        // Keep the callback independent of `Renderer`'s lifetime. In
        // particular, wgpu's browser backend retains a JS `device.lost`
        // continuation; a weak reference prevents that continuation from
        // keeping renderer diagnostics alive after an ordinary shutdown.
        let callback_losses = Arc::downgrade(&device_losses);
        device.set_device_lost_callback(move |reason, message| {
            if let Some(losses) = callback_losses.upgrade() {
                record_device_loss(&losses);
                log::error!("wgpu device lost ({reason:?}): {message}");
            }
        });

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
        let mut focus_blit = DebugMapPipeline::new(&device, render_format);
        focus_blit.upload(&device, &queue, &[72, 190, 255, 255], 1, 1);

        Ok(Self {
            instance,
            surface_source,
            surface,
            device,
            queue,
            config,
            render_format,
            surface_losses: 0,
            device_losses,
            debug_map,
            panel_blit,
            pov_hud_blit,
            focus_blit,
            gpu_map: None,
            pov: None,
            frame_lifecycle: FrameLifecycleCounters::default(),
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
        // Pane-local POV color/depth resources are resized lazily from the
        // next frame's POV rectangle. A surface-only resize need not churn
        // them when the pane's physical render size is unchanged.
    }

    /// Acquire the next surface frame, handling transient states
    /// (outdated/lost/occluded/timeout) by reconfiguring or skipping the frame
    /// rather than propagating. `None` means "draw nothing this frame".
    fn acquire_frame(&mut self) -> Option<wgpu::SurfaceTexture> {
        use wgpu::CurrentSurfaceTexture as Cst;
        self.frame_lifecycle.acquire_attempts =
            self.frame_lifecycle.acquire_attempts.saturating_add(1);
        match self.surface.get_current_texture() {
            Cst::Success(frame) => {
                self.frame_lifecycle.acquired = self.frame_lifecycle.acquired.saturating_add(1);
                Some(frame)
            }
            Cst::Suboptimal(frame) => {
                // Usable this frame; reconfigure for the next.
                self.surface.configure(&self.device, &self.config);
                self.frame_lifecycle.acquired = self.frame_lifecycle.acquired.saturating_add(1);
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

    /// Cumulative surface lifecycle counters for live multi-view attempts.
    #[must_use]
    pub const fn frame_lifecycle(&self) -> FrameLifecycleCounters {
        self.frame_lifecycle
    }

    /// Prepare every requested view, then acquire, encode, submit, and present
    /// exactly one live surface frame.
    ///
    /// Queue writes and pane-local resource growth happen before acquisition.
    /// The presentation surface is cleared once; Map, POV composite,
    /// information, and focus passes load that result. Live rendering exposes
    /// no readback (ADR 0017).
    #[must_use]
    pub fn render_frame(&mut self, frame: MultiViewFrame<'_>) -> MultiViewFrameResult {
        // Validate every borrowed bitmap before mutating any retained GPU
        // resource. Callers may already have consumed dirty keys while
        // assembling the packet, so a late failure must not apply only the
        // earlier half of a multi-view update.
        if let Some(MapFramePane {
            source:
                MapFrameSource::Cpu {
                    rgba,
                    width,
                    height,
                },
            ..
        }) = frame.map
        {
            if !valid_rgba_upload(rgba, width, height) {
                log::error!(
                    "map upload with inconsistent dimensions ({width}x{height}, {} bytes)",
                    rgba.len()
                );
                return MultiViewFrameResult::default();
            }
        }
        if let Err(error) = preflight_information(
            frame.map.and_then(|map| map.information),
            self.panel_blit.texture.is_some(),
        ) {
            log::error!("map information preflight failed: {error:?}");
            return MultiViewFrameResult::default();
        }
        if let Err(error) = preflight_information(
            frame.pov.and_then(|pov| pov.information),
            self.pov_hud_blit.texture.is_some(),
        ) {
            log::error!("POV information preflight failed: {error:?}");
            return MultiViewFrameResult::default();
        }

        let surface = (self.config.width, self.config.height);
        let focus = frame.focus.filter(|focus| focus.thickness > 0);
        let plan_request = FramePlanRequest {
            surface,
            map: frame.map.map(|map| map.viewport),
            pov: frame.pov.map(|pov| pov.viewport),
            map_information: frame
                .map
                .and_then(|map| map.information.map(|information| information.viewport)),
            pov_information: frame
                .pov
                .and_then(|pov| pov.information.map(|information| information.viewport)),
            pov_shadows: frame
                .pov
                .is_some_and(|pov| pov.frame.shadow_ao && pov.frame.shadow_resolution > 0),
            focus: focus.map(|focus| focus.viewport),
        };
        let Ok(_plan) = FramePassPlan::new(plan_request) else {
            log::error!("invalid multi-view frame layout: {plan_request:?}");
            return MultiViewFrameResult::default();
        };

        // Resource preparation: no surface image exists yet.
        let mut map_upload_bytes = 0u64;
        if let Some(map) = frame.map {
            match map.source {
                MapFrameSource::Cpu {
                    rgba,
                    width,
                    height,
                } => {
                    self.debug_map
                        .upload(&self.device, &self.queue, rgba, width, height);
                }
                MapFrameSource::Gpu {
                    params,
                    slots,
                    uploads,
                    pre_grid_overlay,
                    post_grid_overlay,
                } => {
                    let span = (2 * params.half_regions + 1) as u32;
                    let side = span * params.resolution;
                    let rebuild = !matches!(
                        &self.gpu_map,
                        Some(existing)
                            if existing.capacity == span * span
                                && existing.resolution == params.resolution
                                && existing.side == side
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
                    let gpu_map = self.gpu_map.as_ref().expect("GPU map just ensured");
                    map_upload_bytes = gpu_map.upload_tiles(&self.queue, uploads);
                    if let Some(rgba) = pre_grid_overlay {
                        map_upload_bytes += gpu_map.upload_pre_grid_overlay(&self.queue, rgba);
                    }
                    if let Some(rgba) = post_grid_overlay {
                        map_upload_bytes += gpu_map.upload_post_grid_overlay(&self.queue, rgba);
                    }
                    gpu_map.write_frame(&self.queue, params, slots);
                }
            }
            if let Some(information) = map.information {
                if let Some(upload) = information.upload {
                    self.panel_blit.upload(
                        &self.device,
                        &self.queue,
                        upload.rgba,
                        upload.width,
                        upload.height,
                    );
                    map_upload_bytes += u64::from(upload.width) * u64::from(upload.height) * 4;
                }
            }
        }

        let mut pov_draws = Vec::new();
        let mut pov_render_size = None;
        if let Some(pov_frame) = frame.pov {
            if self.pov.is_none() {
                self.pov = Some(pov::Pov::new(&self.device, &self.queue, self.render_format));
                log::info!(
                    "pov pipeline built: {} verts/chunk, {} indices shared",
                    pov::VERTS_PER_CHUNK,
                    pov::INDICES_PER_CHUNK
                );
            }
            let scale = if pov_frame.render_scale.is_finite() {
                pov_frame.render_scale.clamp(0.25, 1.0)
            } else {
                1.0
            };
            let (width, height) =
                pov::Pov::render_size(pov_frame.viewport.width, pov_frame.viewport.height, scale);
            let pov = self.pov.as_mut().expect("POV just ensured");
            pov.ensure_color(&self.device, width, height);
            pov.ensure_depth(&self.device, width, height);
            pov.ensure_shadow(&self.device, pov_frame.frame.shadow_resolution);
            pov.apply(
                &self.device,
                &self.queue,
                pov_frame.uploads,
                pov_frame.removes,
            );
            pov.apply_organisms(&self.device, &self.queue, pov_frame.organisms);
            pov_draws = pov.write_frame(&self.device, &self.queue, pov_frame.frame);
            pov_render_size = Some((width, height));
            if let Some(information) = pov_frame.information {
                if let Some(upload) = information.upload {
                    self.pov_hud_blit.upload(
                        &self.device,
                        &self.queue,
                        upload.rgba,
                        upload.width,
                        upload.height,
                    );
                }
            }
        }

        let Some(surface_frame) = self.acquire_frame() else {
            return MultiViewFrameResult {
                map_upload_bytes,
                ..MultiViewFrameResult::default()
            };
        };
        let surface_view = self.surface_view(&surface_frame);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("wer-multi-view-frame"),
            });

        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-surface-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: frame.clear[0],
                            g: frame.clear[1],
                            b: frame.clear[2],
                            a: frame.clear[3],
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
        self.frame_lifecycle.surface_clears = self.frame_lifecycle.surface_clears.saturating_add(1);

        if let Some(map) = frame.map {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
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
            map.viewport.set(&mut pass);
            match map.source {
                MapFrameSource::Cpu { .. } => {
                    let (_, bind_group, _, _) = self
                        .debug_map
                        .texture
                        .as_ref()
                        .expect("validated CPU map upload");
                    pass.set_pipeline(&self.debug_map.pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
                MapFrameSource::Gpu { .. } => {
                    self.gpu_map
                        .as_ref()
                        .expect("GPU map ensured")
                        .draw(&mut pass);
                }
            }
        }

        if let Some(pov_frame) = frame.pov {
            let (width, height) = pov_render_size.expect("POV render size prepared");
            let pov = self.pov.as_ref().expect("POV prepared");
            pov.draw(
                &mut encoder,
                pov.color_view().expect("POV color target prepared"),
                SurfaceViewport::new(0, 0, width, height),
                &pov_draws,
                frame.clear,
                pov_frame.frame.water,
                pov_frame.frame.shadow_ao && pov_frame.frame.shadow_resolution > 0,
            );
            pov.blit_color(&mut encoder, &surface_view, pov_frame.viewport);
        }

        if let Some(information) = frame.map.and_then(|map| map.information) {
            record_bitmap_surface(
                &mut encoder,
                &surface_view,
                &self.panel_blit,
                information.viewport,
                "frame-map-information",
            );
        }
        if let Some(information) = frame.pov.and_then(|pov| pov.information) {
            record_bitmap_surface(
                &mut encoder,
                &surface_view,
                &self.pov_hud_blit,
                information.viewport,
                "frame-pov-information",
            );
        }
        if let Some(focus) = focus {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame-focus"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
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
            let (_, bind_group, _, _) = self.focus_blit.texture.as_ref().expect("initialized");
            pass.set_pipeline(&self.focus_blit.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            for viewport in focus_border_rects(focus).into_iter().flatten() {
                viewport.set(&mut pass);
                pass.draw(0..3, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.frame_lifecycle.submissions = self.frame_lifecycle.submissions.saturating_add(1);
        surface_frame.present();
        self.frame_lifecycle.presents = self.frame_lifecycle.presents.saturating_add(1);
        MultiViewFrameResult {
            presented: true,
            map_upload_bytes,
            map_drawn: frame.map.is_some(),
            pov_drawn: frame.pov.is_some(),
        }
    }

    /// Acquire the next frame and clear it to `color` (linear RGBA, 0..=1).
    /// Returns `false` when no frame was drawn.
    pub fn render_clear(&mut self, color: [f64; 4]) -> bool {
        self.render_frame(MultiViewFrame {
            clear: color,
            map: None,
            pov: None,
            focus: None,
        })
        .presented
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
        self.render_frame(MultiViewFrame {
            clear,
            map: Some(MapFramePane {
                source: MapFrameSource::Cpu {
                    rgba,
                    width,
                    height,
                },
                viewport,
                information: None,
            }),
            pov: None,
            focus: None,
        })
        .presented
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
        self.render_frame(MultiViewFrame {
            clear,
            map: Some(MapFramePane {
                source: MapFrameSource::Cpu {
                    rgba: map_rgba,
                    width: map_width,
                    height: map_height,
                },
                viewport: map_viewport,
                information: Some(InformationSurface {
                    upload: Some(InformationUpload {
                        rgba: panel_rgba,
                        width: panel_width,
                        height: panel_height,
                    }),
                    viewport: panel_viewport,
                }),
            }),
            pov: None,
            focus: None,
        })
        .presented
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
        let source_width = side + panel_width;
        let combined = letterbox_viewport_in(
            SurfaceViewport::new(0, 0, self.config.width, self.config.height),
            (source_width, side),
        );
        // Resolve the shared seam once. Rounding map width and panel origin
        // independently can overlap by one physical pixel on odd fits.
        let (map_viewport, fitted_panel) = horizontal_split(combined, side, source_width);
        let panel_viewport = (panel_width > 0).then_some(fitted_panel);
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
    /// `panel_viewport` requests the information surface; `panel` optionally
    /// replaces its retained pixels. Browser Map mode passes the fitted square
    /// and no panel viewport; the temporary single-view wrapper above retains
    /// its existing map-plus-panel layout.
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
        let information = panel_viewport.map(|viewport| InformationSurface {
            upload: panel.map(|(rgba, width, height)| InformationUpload {
                rgba,
                width,
                height,
            }),
            viewport,
        });
        let result = self.render_frame(MultiViewFrame {
            clear,
            map: Some(MapFramePane {
                source: MapFrameSource::Gpu {
                    params,
                    slots,
                    uploads,
                    pre_grid_overlay,
                    post_grid_overlay,
                },
                viewport: map_viewport,
                information,
            }),
            pov: None,
            focus: None,
        });
        result.presented.then_some(result.map_upload_bytes)
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
    /// the POV pass at a fraction of the pane resolution and stretches it up
    /// with a linear blit — on a software rasterizer fragment cost is CPU
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
        information: Option<InformationSurface<'_>>,
        pov_scale: f32,
    ) -> bool {
        self.render_frame(MultiViewFrame {
            clear,
            map: None,
            pov: Some(PovFramePane {
                frame,
                uploads,
                removes,
                organisms,
                viewport: pov_viewport,
                information,
                render_scale: pov_scale,
            }),
            focus: None,
        })
        .presented
    }

    /// Packed live/capacity counts for the lazily built POV organism buffers.
    #[must_use]
    pub fn pov_organism_stats(&self) -> Option<PovOrganismBufferStats> {
        self.pov.as_ref().map(pov::Pov::organism_stats)
    }
}

#[cfg(test)]
mod viewport_tests {
    use super::{
        focus_border_rects, horizontal_split, letterbox_viewport_in, preflight_information,
        scale_viewport, FocusDecoration, FrameLifecycleCounters, FramePassKind, FramePassPlan,
        FramePlanError, FramePlanRequest, FrameRegion, InformationPreflightError,
        InformationSurface, InformationUpload, SurfaceViewport,
    };

    #[test]
    fn physical_viewport_containment_handles_edges_odds_and_overflow() {
        assert!(SurfaceViewport::new(0, 0, 901, 701).is_contained_by(901, 701));
        assert!(SurfaceViewport::new(337, 0, 227, 227).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(0, 0, 0, 10).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(900, 700, 2, 1).is_contained_by(901, 701));
        assert!(!SurfaceViewport::new(u32::MAX, 0, 2, 1).is_contained_by(901, 701));
    }

    #[test]
    fn arbitrary_parent_letterbox_preserves_origin_and_aspect() {
        assert_eq!(
            letterbox_viewport_in(SurfaceViewport::new(10, 20, 100, 100), (16, 9)),
            SurfaceViewport::new(10, 42, 100, 56)
        );
        assert_eq!(
            letterbox_viewport_in(SurfaceViewport::new(301, 17, 99, 61), (1, 1)),
            SurfaceViewport::new(320, 17, 61, 61)
        );
        for parent_width in 1..=100 {
            for parent_height in 1..=100 {
                let parent = SurfaceViewport::new(13, 17, parent_width, parent_height);
                for image_width in 1..=16 {
                    for image_height in 1..=16 {
                        let fitted = letterbox_viewport_in(parent, (image_width, image_height));
                        assert!(
                            fitted.x >= parent.x
                                && fitted.y >= parent.y
                                && fitted.x + fitted.width <= parent.x + parent.width
                                && fitted.y + fitted.height <= parent.y + parent.height,
                            "{parent:?} fitting {image_width}x{image_height} produced {fitted:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fitted_map_panel_seam_is_exact_for_odd_sizes() {
        for width in 1..=101 {
            let combined = SurfaceViewport::new(7, 11, width, 63);
            let (map, panel) = horizontal_split(combined, 73, 104);
            assert_eq!(map.x, combined.x);
            assert_eq!(map.x + map.width, panel.x);
            assert_eq!(panel.x + panel.width, combined.x + combined.width);
            assert!(!map.overlaps(panel));
        }
    }

    #[test]
    fn information_preflight_requires_valid_pixels_or_a_retained_texture() {
        let viewport = SurfaceViewport::new(0, 0, 2, 2);
        assert_eq!(
            preflight_information(
                Some(InformationSurface {
                    upload: None,
                    viewport,
                }),
                false,
            ),
            Err(InformationPreflightError::MissingRetainedTexture)
        );
        assert!(preflight_information(
            Some(InformationSurface {
                upload: None,
                viewport,
            }),
            true,
        )
        .is_ok());
        assert_eq!(
            preflight_information(
                Some(InformationSurface {
                    upload: Some(InformationUpload {
                        rgba: &[0; 15],
                        width: 2,
                        height: 2,
                    }),
                    viewport,
                }),
                false,
            ),
            Err(InformationPreflightError::InvalidUpload)
        );
        assert!(preflight_information(
            Some(InformationSurface {
                upload: Some(InformationUpload {
                    rgba: &[0; 16],
                    width: 2,
                    height: 2,
                }),
                viewport,
            }),
            false,
        )
        .is_ok());
    }

    #[test]
    fn focus_border_strips_stay_inside_the_focused_pane() {
        for viewport in [
            SurfaceViewport::new(3, 5, 100, 80),
            SurfaceViewport::new(3, 5, 1, 1),
            SurfaceViewport::new(3, 5, 2, 7),
        ] {
            let strips = focus_border_rects(FocusDecoration {
                viewport,
                thickness: 3,
            });
            for strip in strips.into_iter().flatten() {
                assert!(strip
                    .is_contained_by(viewport.x + viewport.width, viewport.y + viewport.height));
                assert!(strip.x >= viewport.x && strip.y >= viewport.y);
            }
        }
    }

    #[test]
    fn multi_view_plan_is_disjoint_ordered_and_single_present() {
        let map = SurfaceViewport::new(0, 0, 400, 600);
        let pov = SurfaceViewport::new(400, 0, 400, 600);
        let information = SurfaceViewport::new(800, 0, 200, 600);
        let plan = FramePassPlan::new(FramePlanRequest {
            surface: (1000, 600),
            map: Some(map),
            pov: Some(pov),
            map_information: Some(information),
            pov_information: None,
            pov_shadows: true,
            focus: Some(pov),
        })
        .expect("adjacent panes are valid");
        assert_eq!(
            plan.passes(),
            &[
                FramePassKind::SurfaceClear,
                FramePassKind::Map,
                FramePassKind::PovShadow,
                FramePassKind::PovOffscreen,
                FramePassKind::PovComposite,
                FramePassKind::MapInformation,
                FramePassKind::Focus,
            ]
        );
        assert_eq!(
            plan.successful_surface_lifecycle(),
            FrameLifecycleCounters {
                acquire_attempts: 1,
                acquired: 1,
                surface_clears: 1,
                submissions: 1,
                presents: 1,
            }
        );
    }

    #[test]
    fn map_only_and_pov_only_keep_the_same_upload_passes() {
        let map = FramePassPlan::new(FramePlanRequest {
            surface: (900, 700),
            map: Some(SurfaceViewport::new(100, 0, 700, 700)),
            ..FramePlanRequest::default()
        })
        .unwrap();
        assert_eq!(
            map.passes(),
            &[FramePassKind::SurfaceClear, FramePassKind::Map]
        );

        let pov = FramePassPlan::new(FramePlanRequest {
            surface: (900, 700),
            pov: Some(SurfaceViewport::new(0, 0, 700, 700)),
            pov_information: Some(SurfaceViewport::new(700, 0, 200, 700)),
            ..FramePlanRequest::default()
        })
        .unwrap();
        assert_eq!(
            pov.passes(),
            &[
                FramePassKind::SurfaceClear,
                FramePassKind::PovOffscreen,
                FramePassKind::PovComposite,
                FramePassKind::PovInformation,
            ]
        );
    }

    #[test]
    fn frame_plan_rejects_escape_and_color_overlap_but_allows_focus_overlay() {
        let outside = FramePassPlan::new(FramePlanRequest {
            surface: (100, 80),
            map: Some(SurfaceViewport::new(99, 0, 2, 1)),
            ..FramePlanRequest::default()
        });
        assert_eq!(
            outside,
            Err(FramePlanError::OutsideSurface(
                FrameRegion::Map,
                SurfaceViewport::new(99, 0, 2, 1)
            ))
        );

        let overlap = FramePassPlan::new(FramePlanRequest {
            surface: (100, 80),
            map: Some(SurfaceViewport::new(0, 0, 60, 80)),
            pov: Some(SurfaceViewport::new(59, 0, 41, 80)),
            ..FramePlanRequest::default()
        });
        assert_eq!(
            overlap,
            Err(FramePlanError::Overlap(FrameRegion::Map, FrameRegion::Pov))
        );

        assert!(FramePassPlan::new(FramePlanRequest {
            surface: (100, 80),
            map: Some(SurfaceViewport::new(0, 0, 100, 80)),
            focus: Some(SurfaceViewport::new(0, 0, 100, 80)),
            ..FramePlanRequest::default()
        })
        .is_ok());
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

#[cfg(test)]
mod device_loss_tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::record_device_loss;

    #[test]
    fn asynchronous_loss_diagnostic_saturates_instead_of_wrapping() {
        let losses = AtomicU32::new(u32::MAX - 1);
        record_device_loss(&losses);
        assert_eq!(losses.load(Ordering::Relaxed), u32::MAX);
        record_device_loss(&losses);
        assert_eq!(losses.load(Ordering::Relaxed), u32::MAX);
    }
}
