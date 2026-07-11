//! Native application shell (the Phase 0 "empty native application").
//!
//! Opens a window, brings up the wgpu [`Renderer`], and clears the frame each
//! redraw. Its job for now is only to prove the crate boundaries fit together:
//! the platform crate owns windowing and the concrete platform services, while
//! `world-core` and `world-runtime` remain platform-neutral.

use std::sync::Arc;

use renderer::Renderer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};
use world_core::{feature_hash, FeatureKey, RegionCoord, WORLD_ALGORITHM_VERSION};
use world_runtime::RegionState;

/// Background clear color (linear RGBA) — a calm dusk blue standing in for "sky".
const CLEAR_COLOR: [f64; 4] = [0.04, 0.06, 0.12, 1.0];

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // already initialized (e.g. resume after suspend)
        }

        let attributes = Window::default_attributes()
            .with_title("Infinite World Exploration — Phase 0 shell");
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );

        let size = window.inner_size();
        let renderer = pollster::block_on(Renderer::new(
            window.clone(),
            size.width,
            size.height,
        ))
        .expect("failed to initialize renderer");

        // Prove the platform-neutral crates are wired in: hash a sample feature
        // and converge a demo region. This is throwaway scaffolding.
        demo_world_tick();

        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.render_clear(CLEAR_COLOR);
                }
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Placeholder demonstrating that native code drives the platform-neutral model.
fn demo_world_tick() {
    let key = FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region: RegionCoord::new(0, 0),
        layer: 0,
        feature_index: 0,
        possibility_revision: 0,
    };
    log::info!("origin feature hash: {:#018x}", feature_hash(&key));

    let mut region = RegionState::new(RegionCoord::new(4, 4));
    region.stability = 0.0; // distant, free to transform
    region
        .target
        .set(world_core::PossibilityDomain::Climate, 0.9);
    let changed = region.converge(0.25);
    log::info!(
        "demo region {:?} converged={} revision={}",
        region.coord,
        changed,
        region.revision
    );
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    if let Err(err) = event_loop.run_app(&mut app) {
        log::error!("event loop exited with error: {err}");
    }
}
