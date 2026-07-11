//! `renderer` — the wgpu-backed presentation layer.
//!
//! For the Phase 0 shell this owns the GPU device, queue, and surface and does
//! nothing more than clear the frame to a chosen color. It is deliberately thin;
//! the terrain/ecology render graph, clipmaps, and GPU field refinement
//! (sections 12.2 and 17) will be built on top of this foundation.
//!
//! The crate stays WebGPU-compatible: it targets `wgpu` (which maps to native
//! Vulkan/Metal/DX and to WebGPU in the browser) and uses only WGSL shaders.

use core::fmt;

/// A WGSL shader kept for the eventual first draw pipeline. Not yet used by the
/// clear-only bootstrap renderer, but compiled into the binary so the shader
/// path exists from day one.
pub const SHADER_TRIANGLE: &str = include_str!("../shaders/triangle.wgsl");

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

/// Owns the GPU objects needed to present frames to a single surface.
#[derive(Debug)]
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
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

        Ok(Self {
            surface,
            device,
            queue,
            config,
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

    /// Acquire the next frame and clear it to `color` (linear RGBA, 0..=1).
    ///
    /// Transient states (outdated/lost/occluded/timeout) reconfigure or skip the
    /// frame rather than propagating; returns `false` when no frame was drawn.
    pub fn render_clear(&mut self, color: [f64; 4]) -> bool {
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(frame) => frame,
            Cst::Suboptimal(frame) => {
                // Usable this frame; reconfigure for the next.
                self.surface.configure(&self.device, &self.config);
                frame
            }
            Cst::Outdated | Cst::Lost => {
                self.surface.configure(&self.device, &self.config);
                return false;
            }
            Cst::Timeout | Cst::Occluded => return false,
            Cst::Validation => {
                log::error!("surface get_current_texture validation error");
                return false;
            }
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
}
