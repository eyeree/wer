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
//! - `E` / `Q` — drop a manual Emphasize / Suppress anchor at the player
//! - `T` / `Y` / `K` — cycle the capture trait category / toggle polarity /
//!   capture the feature under the player into an anchor (phase-4-plan.md §7.1)
//! - `R` — toggle transition movement mode (slow, resonance-gated steering)
//! - `C` — clear anchors
//! - `V` — cycle the visualized channel (includes the anchor `influence`
//!   field); `G` grid, `N` rings, `X` changed-while-pinned flash
//! - `Esc` — quit
//! - Mouse over the map — the info panel shows the cell under the cursor
//!   (world/region coordinates, streaming state, field samples, biome)
//!
//! An information panel to the right of the map shows frame/streaming
//! telemetry, the selected channel, bias and anchor state, cursor data, and
//! the key bindings ([`panel`]).
//!
//! Headless screenshot mode (no window, for debugging the generators):
//! `wer --screenshot <out.ppm> [channel] [x y]` settles the streaming window
//! at the given position and writes the composed map + panel as a binary PPM.

mod executor;
mod panel;
mod viz;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use renderer::{letterbox_viewport, Renderer};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, Biome, PossibilityDomain,
    PossibilityField, RegionCoord, TraitCategory, LAYER_COUNT, POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    Budget, FrameStats, GenerationStatus, RegionMap, StreamConfig, CHANNEL_CANOPY,
    CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_MOISTURE, CHANNEL_RIVER,
    CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

use executor::RayonExecutor;
use panel::{CursorInfo, EcologyInfo, Hud, PanelInfo};
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
    /// Player position at the previous update; the distance to the current
    /// position is the travel that fuels convergence (ADR 0006).
    last_player: (f64, f64),
    /// Deliberate slow-steering movement mode vs fast free exploration
    /// (phase-4-plan.md §8.2). Toggled with `R`.
    transition_mode: bool,
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
            last_player: (0.0, 0.0),
            transition_mode: false,
            executor: RayonExecutor,
            budget: Budget::per_frame(16.6),
        }
    }

    fn update(&mut self) -> FrameStats {
        let travel = f64::hypot(
            self.player.0 - self.last_player.0,
            self.player.1 - self.last_player.1,
        );
        self.last_player = self.player;
        self.map.update(
            self.player,
            travel,
            &self.field,
            &self.anchors,
            &self.bias,
            &self.budget,
            &self.executor,
            self.transition_mode,
        )
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    world: World,
    composer: MapComposer,
    hud: Hud,
    channel: Channel,
    overlays: Overlays,
    /// The trait category a capture (`K`) anchors, cycled with `T`.
    capture_category: TraitCategory,
    /// Whether a capture emphasizes or suppresses, toggled with `Y`.
    capture_polarity: AnchorKind,
    keys_down: HashSet<KeyCode>,
    modifiers: ModifiersState,
    /// Mouse position in window physical pixels, when over the window.
    cursor_pos: Option<(f64, f64)>,
    /// Cumulative regenerated-tile counts per layer (panel telemetry).
    regen_totals: [u64; LAYER_COUNT as usize],
    last_frame: Instant,
    // Rolling telemetry (phase-1-plan.md section 12), displayed by the info
    // panel; per-second counters are no longer logged.
    stats_frames: u32,
    update_time_accum: f64,
    last_telemetry: Instant,
    /// Snapshot of the last completed telemetry second, for the HUD.
    fps: u32,
    update_ms: f64,
}

impl App {
    fn new() -> Self {
        let cfg = StreamConfig::default();
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        let composer = MapComposer::new(half_regions, cfg.field_resolution);
        let hud = Hud::new(composer.side() as usize);
        Self {
            window: None,
            renderer: None,
            world: World::new(),
            composer,
            hud,
            channel: Channel::Composite,
            overlays: Overlays::default(),
            capture_category: TraitCategory::Morphology,
            capture_polarity: AnchorKind::Emphasize,
            keys_down: HashSet::new(),
            modifiers: ModifiersState::empty(),
            cursor_pos: None,
            regen_totals: [0; LAYER_COUNT as usize],
            last_frame: Instant::now(),
            stats_frames: 0,
            update_time_accum: 0.0,
            last_telemetry: Instant::now(),
            fps: 0,
            update_ms: 0.0,
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
                // Manual debug anchor: the Phase 1 behaviour is the bound-target
                // special case — Emphasize pulls the masked domains toward 1.0,
                // Suppress pushes them away from it (phase-4-plan.md §4.1).
                let mask = domain_mask(&[
                    PossibilityDomain::Climate,
                    PossibilityDomain::Hydrology,
                    PossibilityDomain::Ecology,
                ]);
                self.world.anchors.push(Anchor {
                    world_pos: self.world.player,
                    target: bound_target(mask, 1.0),
                    mask,
                    kind,
                    strength: ANCHOR_STRENGTH,
                    falloff_radius: ANCHOR_RADIUS,
                    source: AnchorSource::Manual,
                });
                log::info!(
                    "dropped {kind:?} anchor at ({:.0}, {:.0}) ({} total)",
                    self.world.player.0,
                    self.world.player.1,
                    self.world.anchors.len()
                );
            }
            KeyCode::KeyK => {
                // Capture the feature under the player into a run-local anchor
                // (phase-4-plan.md §7.1): reads the covering region's baseline,
                // the nearest realized organism or the environment channels, and
                // nudges the target toward what makes the discovery distinctive.
                let mask = self.capture_category.mask_bit();
                match self.world.map.capture_at(
                    self.world.player,
                    mask,
                    self.capture_polarity,
                    ANCHOR_STRENGTH,
                    ANCHOR_RADIUS,
                ) {
                    Some(anchor) => {
                        log::info!(
                            "captured {} {:?} from {:?} ({} anchors)",
                            self.capture_category.name(),
                            self.capture_polarity,
                            anchor.source,
                            self.world.anchors.len() + 1,
                        );
                        self.world.anchors.push(anchor);
                    }
                    None => log::info!("nothing capturable under the player yet"),
                }
            }
            KeyCode::KeyT => {
                let all = TraitCategory::ALL;
                let i = all
                    .iter()
                    .position(|&c| c == self.capture_category)
                    .unwrap_or(0);
                self.capture_category = all[(i + 1) % all.len()];
                log::info!("capture category: {}", self.capture_category.name());
            }
            KeyCode::KeyY => {
                self.capture_polarity = match self.capture_polarity {
                    AnchorKind::Emphasize => AnchorKind::Suppress,
                    AnchorKind::Suppress => AnchorKind::Emphasize,
                };
                log::info!("capture polarity: {:?}", self.capture_polarity);
            }
            KeyCode::KeyR => {
                self.world.transition_mode = !self.world.transition_mode;
                log::info!(
                    "movement mode: {}",
                    if self.world.transition_mode {
                        "transition (slow, deliberate steering)"
                    } else {
                        "free exploration"
                    }
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
            KeyCode::KeyM => {
                self.overlays.organisms = !self.overlays.organisms;
            }
            KeyCode::Escape => event_loop.exit(),
            _ => {}
        }
    }

    /// Everything the panel shows for the map cell at `world`.
    fn sample_cursor(map: &RegionMap, world: (f64, f64)) -> CursorInfo {
        let coord = RegionCoord::from_world(world.0, world.1);
        let (stability, revision, status) = match map.get(coord) {
            Some(r) => (
                r.stability,
                r.revision,
                match r.status {
                    GenerationStatus::Unloaded => "unloaded",
                    GenerationStatus::Generating => "generating",
                    GenerationStatus::Ready => "ready",
                },
            ),
            None => (0.0, 0, "not resident"),
        };
        let res = map.config().field_resolution;
        let (ox, oy) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);
        // Negative float→int casts saturate to 0, so the clamp is total.
        let cx = (((world.0 - ox) / cell) as u16).min(res - 1);
        let cy = (((world.1 - oy) / cell) as u16).min(res - 1);
        let sample = |channel: usize| map.cache().channel(coord, channel).map(|t| t.get(cx, cy));
        CursorInfo {
            world,
            region: (coord.x, coord.y),
            stability,
            revision,
            status,
            elevation: sample(CHANNEL_ELEVATION),
            temperature: sample(CHANNEL_TEMPERATURE),
            moisture: sample(CHANNEL_MOISTURE),
            hardness: sample(CHANNEL_HARDNESS),
            river: sample(CHANNEL_RIVER),
            wetness: sample(CHANNEL_WETNESS),
            soil_depth: sample(CHANNEL_SOIL_DEPTH),
            fertility: sample(CHANNEL_FERTILITY),
            vegetation: sample(CHANNEL_VEGETATION),
            canopy: sample(CHANNEL_CANOPY),
            biome: map
                .cache()
                .biome(coord)
                .map(|t| Biome::from_id(t.get(cx, cy)).name()),
            ecology: map.cell_ecology(coord, cx, cy).map(|e| EcologyInfo {
                roster_size: e.roster.roster.species.len(),
                dominant_id: e.dominant_id,
                trophic_counts: e.trophic_counts,
                herbivore: e.herbivore.unwrap_or(0.0),
                predator: e.predator.unwrap_or(0.0),
                diversity: e.diversity.unwrap_or(0.0),
            }),
        }
    }

    /// Map the mouse (window physical pixels) through the letterbox viewport
    /// onto the composed image, then onto the world.
    fn cursor_world(&self) -> Option<(f64, f64)> {
        let (mx, my) = self.cursor_pos?;
        let surface = self.renderer.as_ref()?.size();
        let image = self.hud.size();
        let (vx, vy, vw, _) = letterbox_viewport(surface, image);
        let scale = f64::from(vw) / f64::from(image.0);
        let ix = (mx - f64::from(vx)) / scale;
        let iy = (my - f64::from(vy)) / scale;
        self.composer.pixel_to_world(self.world.player, ix, iy)
    }

    /// Roll per-frame timings into the once-a-second fps / update-time
    /// snapshot the info panel displays (phase-1-plan.md section 12). The
    /// panel replaced the old periodic telemetry log line; continuity
    /// violations still warn via the composer's detector.
    fn update_telemetry(&mut self, update_seconds: f64) {
        self.stats_frames += 1;
        self.update_time_accum += update_seconds;

        if self.last_telemetry.elapsed().as_secs_f64() >= 1.0 && self.stats_frames > 0 {
            self.fps = self.stats_frames;
            self.update_ms = 1000.0 * self.update_time_accum / f64::from(self.stats_frames);
            self.stats_frames = 0;
            self.update_time_accum = 0.0;
            self.last_telemetry = Instant::now();
        }
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;

        self.apply_movement(dt);

        let update_start = Instant::now();
        let stats = self.world.update();
        let update_seconds = update_start.elapsed().as_secs_f64();
        for (total, &count) in self
            .regen_totals
            .iter_mut()
            .zip(&stats.regenerated_by_layer)
        {
            *total += count as u64;
        }

        let cursor = self
            .cursor_world()
            .map(|world| Self::sample_cursor(&self.world.map, world));

        self.composer.compose(
            &self.world.map,
            self.world.player,
            self.channel,
            self.overlays,
            &self.world.anchors,
        );
        let info = PanelInfo {
            fps: self.fps,
            update_ms: self.update_ms,
            stats,
            regen_totals: &self.regen_totals,
            macro_tiles: self.world.map.macro_cache().len(),
            rosters: self.world.map.roster_cache().len(),
            organisms: self.world.map.organism_count(),
            jobs_in_flight: self.world.map.jobs_in_flight(),
            pinned_violations: self.composer.pinned_violations,
            channel: self.channel,
            player: self.world.player,
            bias: &self.world.bias,
            anchors: &self.world.anchors,
            capture_category: self.capture_category.name(),
            capture_polarity: self.capture_polarity,
            transition_mode: self.world.transition_mode,
            cursor,
        };
        let (width, height) = self.hud.size();
        let pixels = self.hud.compose(self.composer.pixels(), &info);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.render_map(pixels, width, height, CLEAR_COLOR);
        }

        self.update_telemetry(update_seconds);
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
        // The renderer gets a source of fresh surface targets (not a single
        // surface) so it can rebuild the swapchain if the platform loses it —
        // which WSLg does routinely.
        let surface_window = window.clone();
        let renderer = pollster::block_on(Renderer::new(
            move || surface_window.clone().into(),
            size.width,
            size.height,
        ))
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
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
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

/// Headless screenshot: settle the streaming window at `pos` and write the
/// composed false-color map as a binary PPM (P6). No window, no GPU — the map
/// is CPU-composed, which is exactly what makes it inspectable in tests and
/// from the command line.
fn run_screenshot(path: &str, channel: Channel, pos: (f64, f64)) -> Result<(), String> {
    let cfg = StreamConfig::default();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let mut map = RegionMap::new(cfg);
    // Unbudgeted warm-up with the inline executor: fully loaded and generated.
    let mut stats = FrameStats::default();
    let mut regen_totals = [0u64; LAYER_COUNT as usize];
    for _ in 0..8 {
        // Zero travel: fresh regions snap to target at load, and regeneration
        // is not gated on movement, so the window still settles fully.
        stats = map.update(
            pos,
            0.0,
            &field,
            &[],
            &bias,
            &Budget::unlimited(),
            &world_runtime::InlineExecutor,
            false,
        );
        for (total, &count) in regen_totals.iter_mut().zip(&stats.regenerated_by_layer) {
            *total += count as u64;
        }
    }

    let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
    let mut composer = MapComposer::new(half_regions, cfg.field_resolution);
    let overlays = Overlays {
        grid: false,
        rings: false,
        pinned_flash: false,
        organisms: true,
    };
    composer.compose(&map, pos, channel, overlays, &[]);

    // Include the info panel (cursor pinned at the given position) so HUD
    // rendering is inspectable headlessly too.
    let mut hud = Hud::new(composer.side() as usize);
    let info = PanelInfo {
        fps: 0,
        update_ms: 0.0,
        stats,
        regen_totals: &regen_totals,
        macro_tiles: map.macro_cache().len(),
        rosters: map.roster_cache().len(),
        organisms: map.organism_count(),
        jobs_in_flight: map.jobs_in_flight(),
        pinned_violations: composer.pinned_violations,
        channel,
        player: pos,
        bias: &bias,
        anchors: &[],
        capture_category: world_core::TraitCategory::Morphology.name(),
        capture_polarity: AnchorKind::Emphasize,
        transition_mode: false,
        cursor: Some(App::sample_cursor(&map, pos)),
    };
    let (width, height) = hud.size();
    let pixels = hud.compose(composer.pixels(), &info);

    let mut out = Vec::with_capacity(pixels.len() / 4 * 3 + 32);
    out.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for px in pixels.chunks_exact(4) {
        out.extend_from_slice(&px[..3]);
    }
    std::fs::write(path, out).map_err(|e| format!("write {path}: {e}"))?;
    log::info!(
        "wrote {width}x{height} {} map+panel at ({}, {}) to {path}",
        channel.name(),
        pos.0,
        pos.1
    );
    Ok(())
}

/// Build the event loop, preferring X11 over Wayland under WSL.
///
/// WSLg's Wayland compositor resets the client connection a few seconds after
/// a Vulkan swapchain comes up on the llvmpipe adapter (observed as
/// `ERROR_SURFACE_LOST_KHR` followed by "Connection reset by peer"), killing
/// the app. The same session is stable through XWayland, so under WSL we force
/// the X11 backend; set `WER_FORCE_WAYLAND=1` to opt back in.
fn build_event_loop() -> EventLoop<()> {
    #[cfg(target_os = "linux")]
    {
        let on_wsl = std::env::var_os("WSL_DISTRO_NAME").is_some()
            || std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"));
        let wayland_session = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11_available = std::env::var_os("DISPLAY").is_some();
        let overridden = std::env::var_os("WER_FORCE_WAYLAND").is_some();
        if on_wsl && wayland_session && x11_available && !overridden {
            use winit::platform::x11::EventLoopBuilderExtX11;
            log::info!("WSL detected: using the X11 backend (WER_FORCE_WAYLAND=1 to override)");
            match EventLoop::builder().with_x11().build() {
                Ok(event_loop) => return event_loop,
                Err(err) => log::warn!("X11 event loop failed ({err}); using default backend"),
            }
        }
    }
    EventLoop::new().expect("failed to create event loop")
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(rest) = args
        .split_first()
        .and_then(|(first, rest)| (first == "--screenshot").then_some(rest))
    {
        let usage = "usage: wer --screenshot <out.ppm> [channel] [x y]";
        let (path, channel, pos) = match rest {
            [path] => (path, Channel::Composite, (0.0, 0.0)),
            [path, channel] => match Channel::parse(channel) {
                Some(c) => (path, c, (0.0, 0.0)),
                None => {
                    eprintln!("unknown channel {channel:?}\n{usage}");
                    std::process::exit(1);
                }
            },
            [path, channel, x, y] => {
                match (Channel::parse(channel), x.parse::<f64>(), y.parse::<f64>()) {
                    (Some(c), Ok(x), Ok(y)) => (path, c, (x, y)),
                    _ => {
                        eprintln!("bad channel or coordinates\n{usage}");
                        std::process::exit(1);
                    }
                }
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(1);
            }
        };
        if let Err(err) = run_screenshot(path, channel, pos) {
            eprintln!("screenshot failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let event_loop = build_event_loop();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    if let Err(err) = event_loop.run_app(&mut app) {
        log::error!("event loop exited with error: {err}");
    }
}
