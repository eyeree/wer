//! Native application shell for the Phase 1 continuity prototype
//! (phase-1-plan.md sections 4.4 and 10, milestones M5–M6).
//!
//! Opens a window and drives the frame loop: player input moves through the
//! infinite world, keys nudge possibility dimensions and drop anchors, and the
//! renderer presents a top-down false-color map of the streaming window. The
//! platform crate owns windowing, timing, and the concrete Rayon
//! [`world_runtime::TaskExecutor`]; `world-core`/`world-runtime` stay neutral.
//!
//! Controls:
//! - `WASD` / arrows — move (hold `Shift` to sprint)
//! - `1`–`8` — nudge a possibility dimension up (`Shift` = down); order:
//!   Planetary, Climate, Geology, Hydrology, Ecology, Morphology, Behavior,
//!   Aesthetics
//! - `Z` — reset all nudges
//! - `E` / `Q` — drop an Emphasize / Suppress anchor at the player
//! - `C` — clear anchors
//! - `V` — cycle the visualized channel; `G` grid, `N` rings, `X`
//!   changed-while-pinned flash
//! - `Esc` — quit

mod executor;
mod viz;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use renderer::Renderer;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};
use world_core::{
    domain_mask, Anchor, AnchorKind, PossibilityDomain, PossibilityField, POSSIBILITY_DIMS,
};
use world_runtime::{Budget, FrameStats, RegionMap, StreamConfig};

use executor::RayonExecutor;
use viz::{Channel, MapComposer, Overlays};

/// Letterbox color around the square map (linear RGBA).
const CLEAR_COLOR: [f64; 4] = [0.02, 0.02, 0.04, 1.0];

/// Player speed in world units per second (sprint multiplies by 4).
const PLAYER_SPEED: f64 = 500.0;

/// Bias step per possibility-nudge keypress.
const NUDGE_STEP: f32 = 0.05;

/// Anchor parameters for the two Phase 1 anchor kinds.
const ANCHOR_STRENGTH: f32 = 0.8;
const ANCHOR_RADIUS: f64 = 2048.0;

/// The world-simulation half of the app (everything that isn't winit/wgpu).
struct World {
    map: RegionMap,
    field: PossibilityField,
    anchors: Vec<Anchor>,
    bias: [f32; POSSIBILITY_DIMS],
    player: (f64, f64),
    executor: RayonExecutor,
    budget: Budget,
}

impl World {
    fn new() -> Self {
        Self {
            map: RegionMap::new(StreamConfig::default()),
            field: PossibilityField::default(),
            anchors: Vec::new(),
            bias: [0.0; POSSIBILITY_DIMS],
            player: (0.0, 0.0),
            executor: RayonExecutor,
            budget: Budget::per_frame(16.6),
        }
    }

    fn update(&mut self) -> FrameStats {
        self.map.update(
            self.player,
            &self.field,
            &self.anchors,
            &self.bias,
            &self.budget,
            &self.executor,
        )
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    world: World,
    composer: MapComposer,
    channel: Channel,
    overlays: Overlays,
    keys_down: HashSet<KeyCode>,
    modifiers: ModifiersState,
    last_frame: Instant,
    // Rolling telemetry (phase-1-plan.md section 12).
    frame_count: u64,
    stats_accum: FrameStats,
    stats_frames: u32,
    update_time_accum: f64,
    last_stats_log: Instant,
}

impl App {
    fn new() -> Self {
        let cfg = StreamConfig::default();
        let half_regions = (cfg.load_radius / world_core::REGION_SIZE).ceil() as i32;
        Self {
            window: None,
            renderer: None,
            world: World::new(),
            composer: MapComposer::new(half_regions, cfg.field_resolution),
            channel: Channel::Biome,
            overlays: Overlays::default(),
            keys_down: HashSet::new(),
            modifiers: ModifiersState::empty(),
            last_frame: Instant::now(),
            frame_count: 0,
            stats_accum: FrameStats::default(),
            stats_frames: 0,
            update_time_accum: 0.0,
            last_stats_log: Instant::now(),
        }
    }

    /// Continuous movement from held keys, scaled by real elapsed time.
    fn apply_movement(&mut self, dt: f64) {
        let mut dx = 0.0;
        let mut dy = 0.0;
        let down = |code| self.keys_down.contains(&code);
        if down(KeyCode::KeyW) || down(KeyCode::ArrowUp) {
            dy += 1.0;
        }
        if down(KeyCode::KeyS) || down(KeyCode::ArrowDown) {
            dy -= 1.0;
        }
        if down(KeyCode::KeyA) || down(KeyCode::ArrowLeft) {
            dx -= 1.0;
        }
        if down(KeyCode::KeyD) || down(KeyCode::ArrowRight) {
            dx += 1.0;
        }
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let len = f64::hypot(dx, dy);
        let sprint = if self.modifiers.shift_key() { 4.0 } else { 1.0 };
        let step = PLAYER_SPEED * sprint * dt / len;
        self.world.player.0 += dx * step;
        self.world.player.1 += dy * step;
    }

    /// One-shot actions on key press.
    fn handle_press(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        let nudge_domain = match code {
            KeyCode::Digit1 => Some(PossibilityDomain::Planetary),
            KeyCode::Digit2 => Some(PossibilityDomain::Climate),
            KeyCode::Digit3 => Some(PossibilityDomain::Geology),
            KeyCode::Digit4 => Some(PossibilityDomain::Hydrology),
            KeyCode::Digit5 => Some(PossibilityDomain::Ecology),
            KeyCode::Digit6 => Some(PossibilityDomain::Morphology),
            KeyCode::Digit7 => Some(PossibilityDomain::Behavior),
            KeyCode::Digit8 => Some(PossibilityDomain::Aesthetics),
            _ => None,
        };
        if let Some(domain) = nudge_domain {
            let step = if self.modifiers.shift_key() {
                -NUDGE_STEP
            } else {
                NUDGE_STEP
            };
            let dim = &mut self.world.bias[domain.index()];
            *dim = (*dim + step).clamp(-1.0, 1.0);
            log::info!("bias {:?} -> {:+.2}", domain, *dim);
            return;
        }

        match code {
            KeyCode::KeyZ => {
                self.world.bias = [0.0; POSSIBILITY_DIMS];
                log::info!("bias reset");
            }
            KeyCode::KeyE | KeyCode::KeyQ => {
                let kind = if code == KeyCode::KeyE {
                    AnchorKind::Emphasize
                } else {
                    AnchorKind::Suppress
                };
                self.world.anchors.push(Anchor {
                    world_pos: self.world.player,
                    mask: domain_mask(&[
                        PossibilityDomain::Climate,
                        PossibilityDomain::Hydrology,
                        PossibilityDomain::Ecology,
                    ]),
                    kind,
                    strength: ANCHOR_STRENGTH,
                    falloff_radius: ANCHOR_RADIUS,
                });
                log::info!(
                    "dropped {kind:?} anchor at ({:.0}, {:.0}) ({} total)",
                    self.world.player.0,
                    self.world.player.1,
                    self.world.anchors.len()
                );
            }
            KeyCode::KeyC => {
                self.world.anchors.clear();
                log::info!("anchors cleared");
            }
            KeyCode::KeyV => {
                self.channel = self.channel.next();
                log::info!("channel: {}", self.channel.name());
            }
            KeyCode::KeyG => {
                self.overlays.grid = !self.overlays.grid;
            }
            KeyCode::KeyN => {
                self.overlays.rings = !self.overlays.rings;
            }
            KeyCode::KeyX => {
                self.overlays.pinned_flash = !self.overlays.pinned_flash;
            }
            KeyCode::Escape => event_loop.exit(),
            _ => {}
        }
    }

    /// Accumulate and periodically log the per-frame counters
    /// (phase-1-plan.md section 12).
    fn log_stats(&mut self, stats: FrameStats, update_seconds: f64) {
        self.stats_accum.loaded += stats.loaded;
        self.stats_accum.evicted += stats.evicted;
        self.stats_accum.converged += stats.converged;
        self.stats_accum.layers_dispatched += stats.layers_dispatched;
        self.stats_accum.layers_regenerated += stats.layers_regenerated;
        self.stats_accum.deferred_regens += stats.deferred_regens;
        self.stats_frames += 1;
        self.update_time_accum += update_seconds;

        if self.last_stats_log.elapsed().as_secs_f64() >= 1.0 && self.stats_frames > 0 {
            let a = &self.stats_accum;
            log::info!(
                "{} fps | update {:.2} ms avg | regions {} | cache {:.1} MB | \
                 +{} -{} regions/s | converged {}/s | regen {} dispatched {} integrated/s \
                 (deferred {}) | pinned violations {}",
                self.stats_frames,
                1000.0 * self.update_time_accum / f64::from(self.stats_frames),
                stats.active_regions,
                stats.cache_bytes as f64 / (1024.0 * 1024.0),
                a.loaded,
                a.evicted,
                a.converged,
                a.layers_dispatched,
                a.layers_regenerated,
                a.deferred_regens,
                self.composer.pinned_violations,
            );
            self.stats_accum = FrameStats::default();
            self.stats_frames = 0;
            self.update_time_accum = 0.0;
            self.last_stats_log = Instant::now();
        }
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        self.frame_count += 1;

        self.apply_movement(dt);

        let update_start = Instant::now();
        let stats = self.world.update();
        let update_seconds = update_start.elapsed().as_secs_f64();

        let side = self.composer.side();
        let pixels = self.composer.compose(
            &self.world.map,
            self.world.player,
            self.channel,
            self.overlays,
        );
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.render_map(pixels, side, side, CLEAR_COLOR);
        }

        self.log_stats(stats, update_seconds);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // already initialized (e.g. resume after suspend)
        }

        let attributes = Window::default_attributes()
            .with_title("Infinite World Exploration — Phase 1 continuity prototype");
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );

        let size = window.inner_size();
        let renderer = pollster::block_on(Renderer::new(window.clone(), size.width, size.height))
            .expect("failed to initialize renderer");

        log::info!(
            "world algorithm version {} | streaming {:?}",
            world_core::WORLD_ALGORITHM_VERSION,
            self.world.map.config()
        );

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        repeat,
                        ..
                    },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.keys_down.insert(code);
                    if !repeat {
                        self.handle_press(code, event_loop);
                    }
                }
                ElementState::Released => {
                    self.keys_down.remove(&code);
                }
            },
            WindowEvent::RedrawRequested => {
                self.frame();
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    if let Err(err) = event_loop.run_app(&mut app) {
        log::error!("event loop exited with error: {err}");
    }
}
