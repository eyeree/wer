//! Native application shell for the Phase 1 continuity prototype
//! (phase-1-plan.md sections 4.4 and 10, milestones M5–M6).
//!
//! Opens a window and drives the frame loop: player input moves through the
//! infinite world, keys nudge possibility dimensions and drop anchors, and the
//! renderer presents a top-down false-color map of the streaming window. The
//! platform crate owns windowing, timing, and the concrete lane-executor
//! [`world_runtime::TaskExecutor`] (`--inline` for the synchronous A/B);
//! `world-core`/`world-runtime` stay neutral.
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
//! - `O` / `L` — save / load the session through the vault (phase-5-plan.md
//!   §5.3; store directory `WER_VAULT_DIR`, default `./wer-vault`)
//! - `B` — record the most recent anchor into the vault as a named discovery
//! - `I` — summon every vault discovery as an active anchor (shared steering)
//! - `P` — preserve the pinned near window (or delete the preserve you stand in)
//! - `J` — start / finish recording an expedition route
//! - `U` — toggle the route attraction field (recorded corridors steer softly)
//! - `F` — toggle the discovered-region dimming overlay
//! - `V` — cycle the visualized channel (includes the anchor `influence`
//!   field); `G` grid, `N` rings, `X` changed-while-pinned flash
//! - `Esc` — quit
//! - Mouse over the map — the info panel shows the cell under the cursor
//!   (world/region coordinates, streaming state, field samples, biome)
//! - Mouse wheel — zoom the map view in/out (presentation-only magnification
//!   about the view center); zoomed in past x4, hovering an organism marker
//!   shows that organism in the panel instead of the region info
//!
//! An information panel to the right of the map shows frame/streaming
//! telemetry, the selected channel, bias and anchor state, cursor data, and
//! the key bindings ([`panel`]).
//!
//! Headless screenshot mode (no window, for debugging the generators):
//! `wer --screenshot <out.ppm> [channel] [x y [zoom]]` settles the streaming
//! window at the given position and writes the composed map + panel as a
//! binary PPM. A zoom past the organism threshold also picks the organism
//! nearest the center position, exercising the zoomed panel readout.

mod executor;
mod gpumap;
mod panel;
mod viz;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use renderer::{letterbox_viewport, Renderer};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, Biome, PossibilityDomain,
    PossibilityField, PossibilitySignature, RegionCoord, TraitCategory, Trophic, LAYER_COUNT,
    POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    apply_session_regions, AdapterClass, Budget, FrameStats, GenerationStatus, RegionMap,
    ResourceTier, RouteRecorder, RouteTracker, StreamConfig, TierInputs, Vault, VaultStats,
    CHANNEL_CANOPY, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_MOISTURE,
    CHANNEL_RIVER, CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

use executor::LaneExecutor;
use gpumap::AtlasManager;
use panel::{CursorInfo, EcologyInfo, Hud, OrganismInfo, PanelInfo, VaultInfo};
use tools::FileStorage;
use viz::{Channel, MapComposer, MapDecor, Overlays};

/// Letterbox color around the square map (linear RGBA).
const CLEAR_COLOR: [f64; 4] = [0.02, 0.02, 0.04, 1.0];

/// Player speed in world units per second (sprint multiplies by 4).
const PLAYER_SPEED: f64 = 500.0;

/// Bias step per possibility-nudge keypress.
const NUDGE_STEP: f32 = 0.05;

/// Anchor parameters for the two Phase 1 anchor kinds.
const ANCHOR_STRENGTH: f32 = 0.8;
const ANCHOR_RADIUS: f64 = 2048.0;

/// Largest mouse-wheel view magnification (powers of two from 1).
const MAX_ZOOM: u32 = 16;

/// Zoom level at (and past) which hovering an organism marker shows that
/// organism in the panel instead of the region info.
const ORGANISM_INFO_ZOOM: u32 = 4;

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
    /// The persistent record store (phase-5-plan.md §5.3): `O` saves the
    /// session, `L` restores it. `None` if the store directory was unusable.
    vault: Option<Vault<FileStorage>>,
    /// The last flush's counters, for the panel.
    vault_stats: VaultStats,
    /// Live expedition recording (`J` starts/finishes; phase-5-plan.md §7.3).
    recorder: Option<RouteRecorder>,
    /// Traversal detection over the recorded routes (usage bumps, §7.4).
    tracker: RouteTracker,
    /// Whether recorded routes project their attraction field (`U` toggles).
    route_attraction: bool,
    /// The lane executor by default; `wer --inline` swaps in the synchronous
    /// [`world_runtime::InlineExecutor`] for A/B comparison (ADR 0018 makes
    /// the settled world identical either way — only pacing differs).
    executor: Box<dyn world_runtime::TaskExecutor>,
    budget: Budget,
}

impl World {
    fn new(inline: bool, tier: ResourceTier) -> Self {
        // The store location: WER_VAULT_DIR, or ./wer-vault next to the cwd.
        let vault_dir =
            std::env::var("WER_VAULT_DIR").unwrap_or_else(|_| String::from("wer-vault"));
        let vault = match FileStorage::open(&vault_dir)
            .map_err(world_runtime::VaultError::from)
            .and_then(Vault::open)
        {
            Ok(vault) => {
                for issue in vault.issues() {
                    log::warn!("vault: {issue}");
                }
                log::info!(
                    "vault open at {vault_dir}: {} discoveries, {} routes, {} preserves, {} seen",
                    vault.discoveries().len(),
                    vault.routes().len(),
                    vault.preserves().len(),
                    vault.seen_count(),
                );
                Some(vault)
            }
            Err(err) => {
                log::warn!("vault unavailable ({vault_dir}): {err}; running without persistence");
                None
            }
        };
        // Tier presets scale pacing and capacity, never identity (ADR 0018);
        // WER_CACHE_MB overrides the field-cache ceiling for profiling runs.
        let mut stream = tier.stream_config();
        if let Some(mb) = std::env::var("WER_CACHE_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            stream.max_field_cache_bytes = mb * 1024 * 1024;
        }
        let mut world = Self {
            map: RegionMap::new(stream),
            field: PossibilityField::default(),
            anchors: Vec::new(),
            bias: [0.0; POSSIBILITY_DIMS],
            player: (0.0, 0.0),
            last_player: (0.0, 0.0),
            transition_mode: false,
            vault,
            vault_stats: VaultStats::default(),
            recorder: None,
            tracker: RouteTracker::new(),
            route_attraction: true,
            executor: if inline {
                Box::new(world_runtime::InlineExecutor)
            } else {
                Box::new(LaneExecutor::auto())
            },
            budget: tier.budget(),
        };
        // A preserved region realizes its recorded buckets from the very
        // first frame, wherever the run begins (phase-5-plan.md §7.5).
        world.apply_preserves();
        world
    }

    fn update(&mut self) -> FrameStats {
        let travel = f64::hypot(
            self.player.0 - self.last_player.0,
            self.player.1 - self.last_player.1,
        );
        self.last_player = self.player;
        // Route attraction (phase-5-plan.md §7.4): recorded corridors near the
        // player contribute derived weak anchors, riding the same
        // order-independent steer as the player's own.
        let mut effective = self.anchors.clone();
        if self.route_attraction {
            if let Some(vault) = self.vault.as_ref() {
                effective.extend(world_core::attraction_anchors(
                    vault.routes().values(),
                    self.player,
                    self.budget.max_route_attraction_nodes,
                ));
            }
        }
        let mut stats = self.map.update(
            self.player,
            travel,
            &self.field,
            &effective,
            &self.bias,
            &self.budget,
            self.executor.as_ref(),
            self.transition_mode,
        );
        // Expedition recording samples the frame the map just produced (§7.3);
        // the node remembers the player's own anchors, not the derived ones.
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.observe(
                &self.map,
                self.player,
                travel,
                &self.anchors,
                stats.resonance_strength,
            );
        }
        if let Some(vault) = self.vault.as_mut() {
            // The persistence work is the pipeline's Flush pass; the shell
            // times it into the same per-pass table (phase-6-plan.md §5.2).
            let flush_start = Instant::now();
            // Seen-set recording (phase-5-plan.md §5.3): the region under the
            // player is discovered. O(1) and idempotent.
            vault.mark_seen(RegionCoord::from_world(self.player.0, self.player.1));
            // Traversal detection: re-walking a recorded corridor bumps its
            // usage once per leg (§7.4).
            let traversed = self.tracker.observe(vault.routes().values(), self.player);
            for id in traversed {
                vault.bump_route_usage(id);
                log::info!("route {id:#018x} traversed (usage bumped)");
            }
            // Budgeted trickle of dirty records (§7.7); saves marked by `O`
            // and event-driven records drain here.
            self.vault_stats = vault.flush(&self.budget);
            stats.pass_ms[world_runtime::Pass::Flush.index()] +=
                flush_start.elapsed().as_secs_f32() * 1000.0;
        }
        stats
    }

    /// The `J` key: start recording an expedition, or finish and persist it.
    fn toggle_route_recording(&mut self) {
        match self.recorder.take() {
            None => {
                self.recorder = Some(RouteRecorder::new());
                log::info!("route recording started (J again to finish)");
            }
            Some(recorder) => {
                let (nodes, discoveries) = recorder.finish();
                if nodes.len() < 2 {
                    log::info!("route too short ({} nodes); discarded", nodes.len());
                    return;
                }
                let Some(vault) = self.vault.as_mut() else {
                    log::warn!("no vault open; route discarded");
                    return;
                };
                let difficulty = world_core::route_difficulty(&nodes);
                let name = format!("route-{}", vault.routes().len() + 1);
                let count = nodes.len();
                let id = vault.record_route(nodes, discoveries, name.clone());
                log::info!(
                    "recorded {name} ({id:#018x}): {count} nodes, difficulty {difficulty:.2}"
                );
            }
        }
    }

    /// Record the most recent anchor into the vault as a named discovery
    /// (the `B` key; phase-5-plan.md §7.1). Naming is a debug action with an
    /// auto-generated placeholder name (§1.4); the record is the quantized
    /// shareable shadow of the anchor (ADR 0013).
    fn record_last_anchor(&mut self) {
        let Some(anchor) = self.anchors.last().copied() else {
            log::info!("no anchor to record; capture one first (K)");
            return;
        };
        let Some(vault) = self.vault.as_mut() else {
            log::warn!("no vault open; set WER_VAULT_DIR or create ./wer-vault");
            return;
        };
        // The capture cell's habitat identity (0 until its tiles settle).
        let coord = RegionCoord::from_world(anchor.world_pos.0, anchor.world_pos.1);
        let res = self.map.config().field_resolution;
        let (ox, oy) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);
        let cx = (((anchor.world_pos.0 - ox) / cell) as u16).min(res - 1);
        let cy = (((anchor.world_pos.1 - oy) / cell) as u16).min(res - 1);
        let signature_seed = self
            .map
            .cell_signature(coord, cx, cy)
            .map_or(0, |s| s.seed());
        let name = format!("discovery-{}", vault.discoveries().len() + 1);
        let id = vault.record_discovery(&anchor, signature_seed, name.clone());
        // A discovery made mid-expedition joins the route's journal (§7.3).
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.attach_discovery(id);
        }
        log::info!("recorded {name} ({id:#018x}) into the vault");
    }

    /// Pin every vault preserve's regions into the live map (startup and
    /// session load): a preserved region always realizes its recorded buckets,
    /// wherever the run begins (phase-5-plan.md §7.5).
    fn apply_preserves(&mut self) {
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        let pairs: Vec<(RegionCoord, PossibilitySignature)> = vault
            .preserves()
            .values()
            .flat_map(|p| p.regions.iter().copied())
            .collect();
        for (coord, sig) in pairs {
            self.map.set_override(coord, sig);
        }
    }

    /// The `P` key: standing inside a preserve deletes it (no snap — regions
    /// resume steering from where the preserve held them); otherwise the
    /// pinned near window is preserved — each region's possibility state,
    /// quantized, a few dozen bytes per region and zero geometry
    /// (phase-5-plan.md §7.5).
    fn toggle_preserve(&mut self) {
        let Some(vault) = self.vault.as_mut() else {
            log::warn!("no vault open; set WER_VAULT_DIR or create ./wer-vault");
            return;
        };
        let player_region = RegionCoord::from_world(self.player.0, self.player.1);
        let covering = vault
            .preserves()
            .iter()
            .find(|(_, p)| p.regions.iter().any(|(c, _)| *c == player_region))
            .map(|(&id, p)| (id, p.name.clone(), p.regions.clone()));
        if let Some((id, name, regions)) = covering {
            vault.remove_preserve(id);
            for (coord, _) in regions {
                self.map.clear_override(coord);
            }
            log::info!("deleted preserve {name} ({id:#018x}); regions resume steering, no snap");
            return;
        }

        let regions: Vec<(RegionCoord, PossibilitySignature)> = self
            .map
            .iter_active()
            .filter(|r| r.stability >= 1.0 && !self.map.is_overridden(r.coord))
            .map(|r| (r.coord, PossibilitySignature::of(r.current)))
            .collect();
        if regions.is_empty() {
            log::info!("nothing to preserve: no pinned regions here yet");
            return;
        }
        let name = format!("preserve-{}", vault.preserves().len() + 1);
        let id = vault.record_preserve(regions.clone(), name.clone());
        let count = regions.len();
        for (coord, sig) in regions {
            self.map.set_override(coord, sig);
        }
        log::info!("created {name} ({id:#018x}): {count} regions pinned");
    }

    /// Add every vault discovery as an active anchor (the `I` key) — loaded
    /// and imported records steering the live world through the unchanged
    /// order-independent `steer` (phase-5-plan.md §7.1). Idempotent: anchors
    /// already active are skipped.
    fn summon_discoveries(&mut self) {
        let Some(vault) = self.vault.as_ref() else {
            log::warn!("no vault open");
            return;
        };
        let mut added = 0;
        for record in vault.discoveries().values() {
            let anchor = record.to_anchor();
            if !self.anchors.contains(&anchor) {
                self.anchors.push(anchor);
                added += 1;
            }
        }
        log::info!(
            "summoned {added} discovery anchors ({} active)",
            self.anchors.len()
        );
    }

    /// Snapshot the session tier and flush everything (the `O` key).
    fn save_session(&mut self) {
        let Some(vault) = self.vault.as_mut() else {
            log::warn!("no vault open; set WER_VAULT_DIR or create ./wer-vault");
            return;
        };
        vault.snapshot_session(
            &self.map,
            self.player,
            self.last_player,
            &self.bias,
            self.transition_mode,
            &self.anchors,
        );
        let stats = vault.flush_all();
        log::info!(
            "session saved: {} records, {} bytes ({} regions, {} anchors)",
            stats.flushed,
            stats.bytes,
            self.map.len(),
            self.anchors.len()
        );
    }

    /// Restore the saved session (the `L` key): rebuild the streaming window
    /// from the snapshot's bit-exact region states; caches, rosters, and
    /// organisms re-derive over the following frames. `last_player` snaps to
    /// the restored position so the first update after a load is the zero-
    /// travel settle — loading is not an event (phase-5-plan.md §12.2).
    fn load_session(&mut self) {
        let Some(snap) = self.vault.as_ref().and_then(|v| v.session().cloned()) else {
            log::info!("no saved session in the vault");
            return;
        };
        let mut map = RegionMap::new(*self.map.config());
        apply_session_regions(&mut map, &snap);
        self.map = map;
        self.apply_preserves();
        self.player = snap.player;
        self.last_player = snap.player;
        self.bias = snap.bias;
        self.transition_mode = snap.transition_mode;
        self.anchors = snap.anchors.iter().map(|a| a.to_anchor()).collect();
        log::info!(
            "session loaded: {} regions, {} anchors at ({:.0}, {:.0})",
            snap.regions.len(),
            snap.anchors.len(),
            snap.player.0,
            snap.player.1
        );
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
    /// The detected (or overridden) resource tier (phase-6-plan.md §6.7).
    tier: ResourceTier,
    /// GPU-composed map (phase-6-plan.md §6.5) vs the CPU composer; `,`
    /// toggles for the A/B parity eyeball. CPU-only channels fall back
    /// automatically.
    gpu_compose: bool,
    /// GPU refinement octaves above FIELD_RES (`.` toggles; GPU mode only).
    refinement: bool,
    /// Atlas slot assignment + delta-upload keys for the GPU map.
    atlas: AtlasManager,
    /// Content hashes of the last uploaded overlay/panel strips, so an
    /// unchanged strip uploads nothing (steady-state upload ≈ 0, §6.5).
    overlay_hash: u64,
    panel_hash: u64,
    keys_down: HashSet<KeyCode>,
    modifiers: ModifiersState,
    /// Mouse position in window physical pixels, when over the window.
    cursor_pos: Option<(f64, f64)>,
    /// Mouse-wheel view magnification (powers of two, 1..=MAX_ZOOM).
    /// Presentation only; past [`ORGANISM_INFO_ZOOM`] the cursor picks
    /// organisms. Zoomed views compose on the CPU so the base field and the
    /// overlays stay aligned.
    zoom: u32,
    /// Fractional scroll accumulated toward the next zoom step (touchpads
    /// deliver many small pixel deltas per notch).
    scroll_accum: f64,
    /// Cumulative regenerated-tile counts per layer (panel telemetry).
    regen_totals: [u64; LAYER_COUNT as usize],
    last_frame: Instant,
    // Rolling telemetry (phase-1-plan.md section 12; phase-6-plan.md §12),
    // displayed by the info panel; per-second counters are no longer logged.
    stats_frames: u32,
    update_time_accum: f64,
    compose_time_accum: f64,
    render_time_accum: f64,
    pass_ms_accum: [f32; world_runtime::PASS_COUNT],
    last_telemetry: Instant,
    /// Snapshot of the last completed telemetry second, for the HUD.
    fps: u32,
    update_ms: f64,
    /// CPU map+HUD composition ms over the last second (phase-6-plan.md §12).
    compose_ms: f64,
    /// Present ms over the last second — includes the vsync wait, which is
    /// idle pacing, not work (separable now that the busy-loop is gone).
    render_ms: f64,
    /// Mean per-pass ms over the last second.
    pass_ms: [f32; world_runtime::PASS_COUNT],
    upload_accum: u64,
    /// Mean atlas/overlay/panel upload KB per frame over the last second
    /// (GPU path; phase-6-plan.md §12).
    upload_kb: f64,
}

impl App {
    fn new(inline: bool, tier: ResourceTier) -> Self {
        let cfg = tier.stream_config();
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        let composer = MapComposer::new(half_regions, cfg.field_resolution);
        let hud = Hud::new(composer.side() as usize);
        Self {
            window: None,
            renderer: None,
            world: World::new(inline, tier),
            tier,
            composer,
            hud,
            channel: Channel::Composite,
            overlays: Overlays::default(),
            capture_category: TraitCategory::Morphology,
            capture_polarity: AnchorKind::Emphasize,
            // GPU-composed map by default; `,` toggles live, WER_CPU_MAP=1
            // starts in CPU mode (profiling A/B, phase-6-plan.md §6.5).
            gpu_compose: std::env::var_os("WER_CPU_MAP").is_none(),
            refinement: tier.refinement(),
            atlas: AtlasManager::default(),
            overlay_hash: 0,
            panel_hash: 0,
            keys_down: HashSet::new(),
            modifiers: ModifiersState::empty(),
            cursor_pos: None,
            zoom: 1,
            scroll_accum: 0.0,
            regen_totals: [0; LAYER_COUNT as usize],
            last_frame: Instant::now(),
            stats_frames: 0,
            update_time_accum: 0.0,
            compose_time_accum: 0.0,
            render_time_accum: 0.0,
            pass_ms_accum: [0.0; world_runtime::PASS_COUNT],
            last_telemetry: Instant::now(),
            fps: 0,
            update_ms: 0.0,
            compose_ms: 0.0,
            render_ms: 0.0,
            pass_ms: [0.0; world_runtime::PASS_COUNT],
            upload_accum: 0,
            upload_kb: 0.0,
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
            KeyCode::KeyO => self.world.save_session(),
            KeyCode::KeyL => self.world.load_session(),
            KeyCode::KeyB => self.world.record_last_anchor(),
            KeyCode::KeyI => self.world.summon_discoveries(),
            KeyCode::KeyP => self.world.toggle_preserve(),
            KeyCode::KeyJ => self.world.toggle_route_recording(),
            KeyCode::KeyU => {
                self.world.route_attraction = !self.world.route_attraction;
                log::info!(
                    "route attraction {}",
                    if self.world.route_attraction {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            KeyCode::KeyF => {
                self.overlays.discovered = !self.overlays.discovered;
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
            KeyCode::Comma => {
                self.gpu_compose = !self.gpu_compose;
                log::info!(
                    "map compose: {} (A/B parity toggle, phase-6-plan.md §6.5)",
                    if self.gpu_compose { "GPU" } else { "CPU" }
                );
            }
            KeyCode::Period => {
                self.refinement = !self.refinement;
                log::info!(
                    "GPU refinement octaves {}",
                    if self.refinement { "on" } else { "off" }
                );
            }
            KeyCode::Escape => event_loop.exit(),
            _ => {}
        }
    }

    /// The vault-derived map decorations for this frame (phase-5-plan.md
    /// §11): the visible window's discovered set, preserve outlines, and
    /// route polylines. Empty when no vault is open.
    fn build_decor(&self) -> MapDecor {
        let Some(vault) = self.world.vault.as_ref() else {
            return MapDecor::default();
        };
        let center = RegionCoord::from_world(self.world.player.0, self.world.player.1);
        let half = self.composer.half_regions();
        let mut seen = std::collections::BTreeSet::new();
        for dy in -half..=half {
            for dx in -half..=half {
                let coord = RegionCoord::new(center.x + dx, center.y + dy);
                if vault.is_seen(coord) {
                    seen.insert(coord);
                }
            }
        }
        let preserves = vault
            .preserves()
            .values()
            .flat_map(|p| p.regions.iter().map(|(coord, _)| *coord))
            .collect();
        let routes = vault
            .routes()
            .values()
            .map(|r| {
                (
                    r.nodes
                        .iter()
                        .map(|n| (n.pos_q.0 as f64, n.pos_q.1 as f64))
                        .collect(),
                    r.usage,
                )
            })
            .collect();
        MapDecor {
            seen: Some(seen),
            preserves,
            routes,
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

    /// The realized organism whose marker sits under `world`, if any: the
    /// nearest one within one field cell (a marker is one base-map pixel, i.e.
    /// one cell). Reads the transient near-field realization (phase-3-plan.md
    /// §7.6) — a debug readout, never a source of identity.
    fn pick_organism(map: &RegionMap, world: (f64, f64)) -> Option<OrganismInfo> {
        let cell = REGION_SIZE / f64::from(map.config().field_resolution);
        let mut best: Option<(f64, &world_runtime::Organism)> = None;
        for org in map.organisms() {
            let d = f64::hypot(org.world_pos.0 - world.0, org.world_pos.1 - world.1);
            if d <= cell && best.is_none_or(|(nearest, _)| d < nearest) {
                best = Some((d, org));
            }
        }
        best.map(|(_, org)| OrganismInfo {
            id: org.id,
            species: org.species,
            trophic: match org.trophic {
                Trophic::Producer => "producer",
                Trophic::Herbivore => "herbivore",
                Trophic::Omnivore => "omnivore",
                Trophic::Carnivore => "carnivore",
                Trophic::Decomposer => "decomposer",
            },
            world: org.world_pos,
            hue: org.expressed.hue,
            luminance: org.expressed.luminance,
            size: org.expressed.size,
            activity: org.expressed.activity,
            aggression: org.expressed.aggression,
        })
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
    /// snapshot the info panel displays (phase-1-plan.md section 12;
    /// phase-6-plan.md §12 adds the per-pass, compose, and present splits).
    /// The panel replaced the old periodic telemetry log line; continuity
    /// violations still warn via the composer's detector.
    fn update_telemetry(
        &mut self,
        update_seconds: f64,
        compose_seconds: f64,
        render_seconds: f64,
        pass_ms: &[f32; world_runtime::PASS_COUNT],
        upload_bytes: u64,
    ) {
        self.stats_frames += 1;
        self.update_time_accum += update_seconds;
        self.compose_time_accum += compose_seconds;
        self.render_time_accum += render_seconds;
        self.upload_accum += upload_bytes;
        for (accum, &ms) in self.pass_ms_accum.iter_mut().zip(pass_ms) {
            *accum += ms;
        }

        if self.last_telemetry.elapsed().as_secs_f64() >= 1.0 && self.stats_frames > 0 {
            let frames = f64::from(self.stats_frames);
            self.fps = self.stats_frames;
            self.update_ms = 1000.0 * self.update_time_accum / frames;
            self.compose_ms = 1000.0 * self.compose_time_accum / frames;
            self.render_ms = 1000.0 * self.render_time_accum / frames;
            self.upload_kb = self.upload_accum as f64 / 1024.0 / frames;
            self.upload_accum = 0;
            log::debug!(
                "telemetry: fps {} update {:.2}ms compose {:.2}ms present {:.2}ms upload {:.0}KB/f",
                self.fps,
                self.update_ms,
                self.compose_ms,
                self.render_ms,
                self.upload_kb
            );
            for (avg, accum) in self.pass_ms.iter_mut().zip(&mut self.pass_ms_accum) {
                *avg = *accum / self.stats_frames as f32;
                *accum = 0.0;
            }
            self.stats_frames = 0;
            self.update_time_accum = 0.0;
            self.compose_time_accum = 0.0;
            self.render_time_accum = 0.0;
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

        let cursor_world = self.cursor_world();
        let cursor = cursor_world.map(|world| Self::sample_cursor(&self.world.map, world));
        // Zoomed in far enough, the cursor picks the organism marker under it
        // (the region info stays whenever no organism is under the mouse).
        let organism = if self.zoom >= ORGANISM_INFO_ZOOM {
            cursor_world.and_then(|world| Self::pick_organism(&self.world.map, world))
        } else {
            None
        };

        let decor = self.build_decor();
        let gpu_channel = gpumap::gpu_channel(self.channel);
        // A zoomed view composes on the CPU: the GPU shader has no zoom
        // transform, and the CPU path magnifies field and overlays together.
        self.composer.set_zoom(self.zoom);
        let use_gpu =
            self.gpu_compose && self.zoom == 1 && gpu_channel.is_some() && self.renderer.is_some();
        let compose_start = Instant::now();
        if use_gpu {
            // GPU path (phase-6-plan.md §6.5): CPU draws only the sparse
            // overlay; the field false-color composes per screen pixel from
            // the atlas.
            self.composer.compose_overlays(
                &self.world.map,
                self.world.player,
                self.overlays,
                &decor,
            );
        } else {
            self.composer.compose(
                &self.world.map,
                self.world.player,
                self.channel,
                self.overlays,
                &self.world.anchors,
                &decor,
            );
        }
        let info = PanelInfo {
            fps: self.fps,
            update_ms: self.update_ms,
            compose_ms: self.compose_ms,
            render_ms: self.render_ms,
            upload_kb: self.upload_kb,
            gpu_compose: use_gpu,
            tier: self.tier.name(),
            cache_ceiling_bytes: self.world.map.config().max_field_cache_bytes,
            pass_ms: self.pass_ms,
            workers: self.world.executor.parallelism(),
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
            vault: self.world.vault.as_ref().map(|v| VaultInfo {
                records: v.discoveries().len() + v.routes().len() + v.preserves().len(),
                dirty: v.dirty_records(),
                seen: v.seen_count(),
                issues: v.issues().len(),
            }),
            zoom: self.zoom,
            cursor,
            organism,
        };
        let mut upload_bytes = 0u64;
        let (compose_seconds, render_seconds) = if use_gpu {
            // Delta uploads: only regions whose dependency-hash key changed,
            // plus the overlay/panel strips only when their bytes changed.
            let (panel_w, panel_h) = {
                let (panel, w, h) = self.hud.panel_image(&info);
                let hash = hash_bytes(panel);
                let changed = hash != self.panel_hash;
                self.panel_hash = hash;
                (changed.then_some(w).map(|w| (w, h)), (w, h))
            };
            let _ = panel_h;
            let overlay_hash = hash_bytes(self.composer.pixels());
            let overlay_changed = overlay_hash != self.overlay_hash;
            self.overlay_hash = overlay_hash;

            let center = RegionCoord::from_world(self.world.player.0, self.world.player.1);
            let half = self.composer.half_regions();
            let res = self.world.map.config().field_resolution;
            let (slots, uploads) = self.atlas.sync(&self.world.map, center, half, res);
            let (west, north) = (
                f64::from(center.x - half) * REGION_SIZE,
                f64::from(center.y + half + 1) * REGION_SIZE,
            );
            let (refine, refine_count) = if self.refinement {
                gpumap::refinement_octaves(west, north, res, 3)
            } else {
                (Default::default(), 0)
            };
            let params = renderer::GpuMapParams {
                half_regions: half,
                resolution: u32::from(res),
                channel: gpu_channel.expect("use_gpu checked"),
                grid: self.overlays.grid,
                refine,
                refine_count,
            };
            let compose_seconds = compose_start.elapsed().as_secs_f64();
            let render_start = Instant::now();
            if let Some(renderer) = self.renderer.as_mut() {
                let overlay = overlay_changed.then(|| self.composer.pixels());
                let panel = panel_w.map(|(w, h)| (self.hud.panel_pixels(), w, h));
                if let Some(bytes) =
                    renderer.render_map_gpu(&params, &slots, &uploads, overlay, panel, CLEAR_COLOR)
                {
                    upload_bytes = bytes;
                }
            }
            (compose_seconds, render_start.elapsed().as_secs_f64())
        } else {
            let (width, height) = self.hud.size();
            let pixels = self.hud.compose(self.composer.pixels(), &info);
            let compose_seconds = compose_start.elapsed().as_secs_f64();
            let render_start = Instant::now();
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.render_map(pixels, width, height, CLEAR_COLOR);
            }
            (compose_seconds, render_start.elapsed().as_secs_f64())
        };

        self.update_telemetry(
            update_seconds,
            compose_seconds,
            render_seconds,
            &stats.pass_ms,
            upload_bytes,
        );
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
            WindowEvent::MouseWheel { delta, .. } => {
                self.scroll_accum += match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    // Touchpads scroll in pixels; ~40 px per wheel notch.
                    MouseScrollDelta::PixelDelta(pos) => pos.y / 40.0,
                };
                let before = self.zoom;
                while self.scroll_accum >= 1.0 {
                    self.zoom = (self.zoom * 2).min(MAX_ZOOM);
                    self.scroll_accum -= 1.0;
                }
                while self.scroll_accum <= -1.0 {
                    self.zoom = (self.zoom / 2).max(1);
                    self.scroll_accum += 1.0;
                }
                if self.zoom != before {
                    log::info!(
                        "zoom x{}{}",
                        self.zoom,
                        if self.zoom >= ORGANISM_INFO_ZOOM {
                            " (organism picking active)"
                        } else {
                            ""
                        }
                    );
                }
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

/// Order-stable content hash of a pixel strip, for skipping unchanged
/// overlay/panel uploads (phase-6-plan.md §6.5).
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0x0DDB_1A5E_D0F0_0006;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        h = world_core::mix(
            h,
            u64::from_le_bytes(chunk.try_into().expect("8-byte chunk")),
        );
    }
    for &b in chunks.remainder() {
        h = world_core::mix(h, u64::from(b));
    }
    h
}

/// Headless screenshot: settle the streaming window at `pos` and write the
/// composed false-color map as a binary PPM (P6). No window, no GPU — the map
/// is CPU-composed, which is exactly what makes it inspectable in tests and
/// from the command line.
fn run_screenshot(path: &str, channel: Channel, pos: (f64, f64), zoom: u32) -> Result<(), String> {
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
    composer.set_zoom(zoom);
    let overlays = Overlays {
        grid: false,
        rings: false,
        pinned_flash: false,
        organisms: true,
        discovered: false,
    };
    composer.compose(&map, pos, channel, overlays, &[], &MapDecor::default());

    // The organism readout, exactly as the live cursor picks it (the
    // "cursor" sits at the given position).
    let organism = if zoom >= ORGANISM_INFO_ZOOM {
        let picked = App::pick_organism(&map, pos);
        match &picked {
            Some(o) => log::info!(
                "picked organism {:#018x} ({} at {:.0}, {:.0})",
                o.id,
                o.trophic,
                o.world.0,
                o.world.1
            ),
            None => log::info!("no organism within a cell of ({}, {})", pos.0, pos.1),
        }
        picked
    } else {
        None
    };

    // Include the info panel (cursor pinned at the given position) so HUD
    // rendering is inspectable headlessly too.
    let mut hud = Hud::new(composer.side() as usize);
    let info = PanelInfo {
        fps: 0,
        update_ms: 0.0,
        compose_ms: 0.0,
        render_ms: 0.0,
        upload_kb: 0.0,
        gpu_compose: false,
        tier: "low",
        cache_ceiling_bytes: cfg.max_field_cache_bytes,
        pass_ms: stats.pass_ms,
        workers: 1,
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
        vault: None,
        zoom,
        cursor: Some(App::sample_cursor(&map, pos)),
        organism,
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

/// Gather the tier inputs (phase-6-plan.md §6.7) — cores, adapter class via
/// a throwaway wgpu probe, and the `WER_TIER` override — and decide the tier
/// through the pure `world-runtime` table.
fn detect_tier() -> ResourceTier {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let adapter = match renderer::probe_adapter() {
        renderer::ProbedAdapter::Discrete => AdapterClass::Discrete,
        renderer::ProbedAdapter::Integrated => AdapterClass::Integrated,
        renderer::ProbedAdapter::Cpu => AdapterClass::Cpu,
        renderer::ProbedAdapter::Unknown => AdapterClass::Unknown,
    };
    let override_tier = std::env::var("WER_TIER")
        .ok()
        .and_then(|v| ResourceTier::parse(&v));
    let tier = ResourceTier::detect(&TierInputs {
        cores,
        adapter,
        override_tier,
    });
    log::info!(
        "resource tier: {} ({cores} cores, adapter {adapter:?}{})",
        tier.name(),
        if override_tier.is_some() {
            ", WER_TIER override"
        } else {
            ""
        }
    );
    tier
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--inline`: run generation synchronously on the main thread (the
    // harness substrate) instead of the LaneExecutor — the A/B switch for
    // schedule-independence spot checks (phase-6-plan.md §5.3).
    let inline = args.iter().any(|a| a == "--inline");
    args.retain(|a| a != "--inline");
    if let Some(rest) = args
        .split_first()
        .and_then(|(first, rest)| (first == "--screenshot").then_some(rest))
    {
        let usage = "usage: wer --screenshot <out.ppm> [channel] [x y [zoom]]";
        let (path, channel, pos, zoom) = match rest {
            [path] => (path, Channel::Composite, (0.0, 0.0), 1),
            [path, channel] => match Channel::parse(channel) {
                Some(c) => (path, c, (0.0, 0.0), 1),
                None => {
                    eprintln!("unknown channel {channel:?}\n{usage}");
                    std::process::exit(1);
                }
            },
            [path, channel, x, y, zoom @ ..] if zoom.len() <= 1 => {
                let zoom = match zoom {
                    [z] => z.parse::<u32>().ok().filter(|z| *z >= 1),
                    _ => Some(1),
                };
                match (
                    Channel::parse(channel),
                    x.parse::<f64>(),
                    y.parse::<f64>(),
                    zoom,
                ) {
                    (Some(c), Ok(x), Ok(y), Some(z)) => (path, c, (x, y), z),
                    _ => {
                        eprintln!("bad channel, coordinates, or zoom\n{usage}");
                        std::process::exit(1);
                    }
                }
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(1);
            }
        };
        if let Err(err) = run_screenshot(path, channel, pos, zoom) {
            eprintln!("screenshot failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    let event_loop = build_event_loop();
    // Frame pacing (phase-6-plan.md M1): the redraw chain — present under
    // FIFO/vsync, then `request_redraw` — is the pacer, so the event loop
    // sleeps between events instead of busy-polling.
    event_loop.set_control_flow(ControlFlow::Wait);

    let tier = detect_tier();
    let mut app = App::new(inline, tier);
    if let Err(err) = event_loop.run_app(&mut app) {
        log::error!("event loop exited with error: {err}");
    }
}
