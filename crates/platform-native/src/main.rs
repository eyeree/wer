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
//! - `H` — toggle persistent path tracking (off by default; enables route
//!   recording, traversal detection, the attraction field, and map polylines)
//! - `J` — start / finish recording an expedition route (needs `H` on)
//! - `U` — toggle the route attraction field (recorded corridors steer softly)
//! - `Delete` — clear all recorded routes from the vault
//! - `F` — toggle the discovered-region dimming overlay
//! - `V` — cycle the visualized channel (includes the anchor `influence`
//!   field); `G` grid, `N` rings, `X` changed-while-pinned flash
//! - `Tab` — toggle the 3D POV mode (3d-phase-1-plan.md): a fly camera over
//!   the meshed near-field terrain. In POV: hold the **left mouse button**
//!   and drag to look, `WASD` along view/strafe, `Space`/`LShift` up/down,
//!   wheel adjusts the active mode's speed. `F` toggles walk ↔ fly
//!   (3d-phase-2-plan.md): walk rides the rendered terrain at eye height
//!   (`Space`/`LShift` reserved, cliffs climb as fast ramps, the sea floor
//!   is walkable); toggling back to fly keeps the pose. Every map binding
//!   above is map-mode-only. `WER_POV=1` starts in POV; `WER_POV_RADIUS`
//!   sets the chunk draw radius in regions (default 3).
//! - `F12` — write a debug dump into `./dump/<UTC datetime>/`: a screenshot
//!   of the active view (map or POV) plus `state.txt` with the player/camera
//!   state, steering, telemetry, dep-hash chain, and vault counters
//!   ([`dump`]). Works in both map and POV modes.
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
//!
//! Headless POV screenshot mode (offscreen GPU, ADR 0021):
//! `wer --pov-script "<instructions>"` drives the POV camera through a
//! `;`-separated instruction sequence and captures snapshots — the
//! debugging/testing harness for POV rendering. Instructions:
//! `size:WxH` (capture size, before the first snap), `pos:x,y[,z]`,
//! `mouse:dx,dy` (simulated look drag, pixels), `move:f[,r[,u]]` (fly
//! forward/right/up in world units; in walk mode `f`/`r` move in the walk
//! basis, `u` is ignored, and the eye snaps to the ground at the
//! destination), `walk` / `fly` (toggle the 3D-2 walk mode, exactly like
//! the live `F` key), `settle[:n]` (world updates), and `snap:file.ppm`.
//! Example:
//! `wer --pov-script "pos:300,-10; walk; move:200; snap:walk-a.ppm; mouse:400,0; move:200; snap:walk-b.ppm"`

mod dump;
mod executor;
mod gpumap;
mod panel;
mod pov;
mod viz;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use renderer::{letterbox_viewport, Renderer};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, Biome, PossibilityDomain,
    PossibilityField, PossibilitySignature, RegionCoord, TraitCategory, Trophic, LAYER_COUNT,
    POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    apply_session_regions, compare_session_runtime, session_runtime_record,
    stream_config_from_record, AdapterClass, Budget, FrameStats, GenerationStatus, RegionMap,
    ResourceTier, RouteRecorder, RouteTracker, SessionCompatibility, SessionSnapshotInput, Storage,
    StreamConfig, TierInputs, Vault, VaultPersistenceError, VaultStats, CHANNEL_CANOPY,
    CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_MOISTURE, CHANNEL_RIVER,
    CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

use executor::LaneExecutor;
use gpumap::AtlasManager;
use panel::{CursorInfo, EcologyInfo, Hud, OrganismInfo, PanelInfo, VaultInfo};
use pov::{
    PovCamera, PovChunkManager, PovCounters, PovOrganismCounters, PovOrganismManager, PovToggles,
};
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

/// Native touchpad pixels treated as one wheel notch. Kept as a named seam so
/// the alignment characterization can replay line and pixel deltas exactly.
const WHEEL_PIXELS_PER_NOTCH: f64 = 40.0;

/// Raw map navigation components and their length from held physical keys.
/// Keeping the length separate preserves the native movement operation order.
fn map_navigation_components(keys_down: &HashSet<KeyCode>) -> (f64, f64, f64) {
    let down = |code| keys_down.contains(&code);
    let mut dx = 0.0;
    let mut dy = 0.0;
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
    let len = f64::hypot(dx, dy);
    (dx, dy, len)
}

/// Final map movement delta with the pre-alignment floating-point operation
/// order. Travel feeds convergence, so even a one-ULP reassociation here is an
/// unintended behavior change during characterization.
fn map_movement_delta(keys_down: &HashSet<KeyCode>, sprint: bool, dt: f64) -> Option<(f64, f64)> {
    let (dx, dy, len) = map_navigation_components(keys_down);
    if len == 0.0 {
        return None;
    }
    let sprint = if sprint { 4.0 } else { 1.0 };
    let step = PLAYER_SPEED * sprint * dt / len;
    Some((dx * step, dy * step))
}

/// Forward/strafe/vertical intent from held POV keys. The camera basis is
/// applied afterwards because pitched forward and world-up are not generally
/// orthogonal; the resulting world vector retains the existing normalization.
fn pov_navigation_axis(keys_down: &HashSet<KeyCode>, walk: bool) -> (f64, f64, f64) {
    let down = |code| keys_down.contains(&code);
    let forward = i8::from(down(KeyCode::KeyW) || down(KeyCode::ArrowUp))
        - i8::from(down(KeyCode::KeyS) || down(KeyCode::ArrowDown));
    let strafe = i8::from(down(KeyCode::KeyD) || down(KeyCode::ArrowRight))
        - i8::from(down(KeyCode::KeyA) || down(KeyCode::ArrowLeft));
    let vertical = if walk {
        0
    } else {
        i8::from(down(KeyCode::Space)) - i8::from(down(KeyCode::ShiftLeft))
    };
    (f64::from(forward), f64::from(strafe), f64::from(vertical))
}

/// Add fractional wheel input and return complete signed notches, retaining
/// the sub-notch remainder exactly as the native event path does today.
fn accumulate_wheel(accum: &mut f64, delta: f64) -> i32 {
    *accum += delta;
    let mut notches = 0;
    while *accum >= 1.0 {
        notches += 1;
        *accum -= 1.0;
    }
    while *accum <= -1.0 {
        notches -= 1;
        *accum += 1.0;
    }
    notches
}

/// Cursor delta while a primary-button drag is active. `None` is both the
/// pre-press and post-release/cancel gate: pointer transport alone cannot look.
fn primary_drag_delta(last: Option<(f64, f64)>, current: (f64, f64)) -> Option<(f64, f64)> {
    last.map(|(x, y)| (current.0 - x, current.1 - y))
}

/// One-shot key presses dispatch only on their first non-repeat event.
const fn dispatch_one_shot(repeat: bool) -> bool {
    !repeat
}

/// Which presentation the shell drives (3d-phase-1-plan.md §8.1). Map mode
/// is byte-for-byte the pre-POV path; POV renders the meshed terrain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Map,
    Pov,
}

/// The two one-shot bindings exercised by the Milestone 0 semantic trace.
/// The full registry moves to `viewer-host` in Milestone 2; until then this
/// narrow seam makes the trace execute the production key mapping/reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterizedOneShot {
    CycleMapChannel,
    TogglePovShadowAo,
}

fn characterized_one_shot(mode: ViewMode, code: KeyCode) -> Option<CharacterizedOneShot> {
    match (mode, code) {
        (ViewMode::Map, KeyCode::KeyV) => Some(CharacterizedOneShot::CycleMapChannel),
        (ViewMode::Pov, KeyCode::KeyB) => Some(CharacterizedOneShot::TogglePovShadowAo),
        _ => None,
    }
}

fn reduce_characterized_one_shot(
    action: CharacterizedOneShot,
    channel: &mut Channel,
    pov_shadow_ao: &mut bool,
) {
    match action {
        CharacterizedOneShot::CycleMapChannel => *channel = channel.next(),
        CharacterizedOneShot::TogglePovShadowAo => *pov_shadow_ao = !*pov_shadow_ao,
    }
}

/// Arm/release the native POV look gate from an actual mouse-button event.
fn update_primary_drag_gate(
    mode: ViewMode,
    button: MouseButton,
    pressed: bool,
    cursor: Option<(f64, f64)>,
    drag_from: &mut Option<(f64, f64)>,
) {
    if mode == ViewMode::Pov && button == MouseButton::Left {
        *drag_from = if pressed { cursor } else { None };
    }
}

fn cancel_primary_drag(drag_from: &mut Option<(f64, f64)>) {
    *drag_from = None;
}

/// Remove the runtime's effective preserve owner and only that record's
/// contributions. Errors distinguish impossible runtime/vault drift from a
/// retryable persistence failure; neither deletes an arbitrary covering record
/// or changes runtime ownership (ADR 0020, ADR 0022).
#[derive(Debug)]
enum PreserveRemovalError {
    MissingVaultRecord(u64),
    Persistence(VaultPersistenceError),
}

fn remove_effective_preserve<S: Storage>(
    map: &mut RegionMap,
    vault: &mut Vault<S>,
    coord: RegionCoord,
) -> Result<Option<(u64, String)>, PreserveRemovalError> {
    let Some((id, _)) = map.effective_preserve(coord) else {
        return Ok(None);
    };
    let Some(record) = vault.preserves().get(&id).cloned() else {
        return Err(PreserveRemovalError::MissingVaultRecord(id));
    };
    let removed = vault
        .remove_preserve(id)
        .map_err(PreserveRemovalError::Persistence)?;
    debug_assert!(removed, "record cloned from the vault must be removable");
    for (region, _) in record.regions {
        map.remove_preserve_contribution(id, region);
    }
    Ok(Some((id, record.name)))
}

#[derive(Debug)]
struct RouteRemovalOutcome {
    removed: usize,
    total: usize,
    error: Option<VaultPersistenceError>,
}

/// Remove routes in ascending id order, stopping at the first durability
/// failure. Tracker state is retained exactly for records still in the vault.
fn remove_routes<S: Storage>(
    vault: &mut Vault<S>,
    tracker: &mut RouteTracker,
) -> RouteRemovalOutcome {
    let ids: Vec<u64> = vault.routes().keys().copied().collect();
    let total = ids.len();
    let mut removed = 0;
    let mut error = None;
    for id in ids {
        match vault.remove_route(id) {
            Ok(true) => removed += 1,
            Ok(false) => unreachable!("id came from the vault"),
            Err(found) => {
                error = Some(found);
                break;
            }
        }
    }
    tracker.retain(|id| vault.routes().contains_key(&id));
    RouteRemovalOutcome {
        removed,
        total,
        error,
    }
}

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
    /// Whether the frame loop has already logged the active persistence
    /// failure. Repeats remain visible through bounded HUD telemetry.
    vault_failure_logged: bool,
    /// Live expedition recording (`J` starts/finishes; phase-5-plan.md §7.3).
    recorder: Option<RouteRecorder>,
    /// Traversal detection over the recorded routes (usage bumps, §7.4).
    tracker: RouteTracker,
    /// Master switch for the persistent path subsystem (`H` toggles, off by
    /// default): route recording, traversal tracking, the attraction field,
    /// and the map polylines are all dormant while this is false. Recorded
    /// routes stay in the vault either way — the records are the truth
    /// (ADR 0015); `Delete` clears them.
    path_tracking: bool,
    /// Whether recorded routes project their attraction field (`U` toggles;
    /// only effective while [`Self::path_tracking`] is on).
    route_attraction: bool,
    /// The lane executor by default; `wer --inline` swaps in the synchronous
    /// [`world_runtime::InlineExecutor`] for A/B comparison (ADR 0018 makes
    /// the settled world identical either way — only pacing differs).
    executor: Box<dyn world_runtime::TaskExecutor>,
    budget: Budget,
    tier: ResourceTier,
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
            vault_failure_logged: false,
            recorder: None,
            tracker: RouteTracker::new(),
            path_tracking: false,
            route_attraction: true,
            executor: if inline {
                Box::new(world_runtime::InlineExecutor)
            } else {
                Box::new(LaneExecutor::auto())
            },
            budget: tier.budget(),
            tier,
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
        if self.path_tracking && self.route_attraction {
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
        // Expedition recording samples the frame the map just produced (§7.3,
        // ADR 0025), including the exact route-derived anchors that steered it.
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.observe(
                &self.map,
                self.player,
                travel,
                &effective,
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
            // usage once per leg (§7.4). Dormant while path tracking is off.
            if self.path_tracking {
                let traversed = self.tracker.observe(vault.routes().values(), self.player);
                for id in traversed {
                    vault.bump_route_usage(id);
                    log::info!("route {id:#018x} traversed (usage bumped)");
                }
            }
            // Budgeted trickle of dirty records (§7.7); saves marked by `O`
            // and event-driven records drain here.
            match vault.flush(&self.budget) {
                Ok(flush) => {
                    self.vault_stats = flush;
                    if self.vault_failure_logged && vault.active_persistence_issue().is_none() {
                        log::info!("vault persistence recovered");
                        self.vault_failure_logged = false;
                    }
                }
                Err(error) => {
                    self.vault_stats = error.progress();
                    if error.persistence_error().occurrences() == 1 {
                        log::warn!("vault persistence: {error}");
                    }
                    self.vault_failure_logged = true;
                }
            }
            stats.pass_ms[world_runtime::Pass::Flush.index()] +=
                flush_start.elapsed().as_secs_f32() * 1000.0;
        }
        stats
    }

    /// The `H` key: turn the persistent path subsystem on or off. Turning it
    /// off mid-recording discards the unfinished expedition (nothing was
    /// persisted yet); routes already in the vault are untouched.
    fn toggle_path_tracking(&mut self) {
        self.path_tracking = !self.path_tracking;
        if !self.path_tracking && self.recorder.take().is_some() {
            log::info!("path tracking off; in-progress recording discarded");
        }
        log::info!(
            "path tracking {}",
            if self.path_tracking { "on" } else { "off" }
        );
    }

    /// The `Delete` key: erase every recorded route from the vault (and any
    /// in-progress recording). Discoveries, preserves, and the seen set are
    /// untouched — this clears paths only.
    fn clear_routes(&mut self) {
        if self.recorder.take().is_some() {
            log::info!("in-progress recording discarded");
        }
        let Some(vault) = self.vault.as_mut() else {
            log::warn!("no vault open; no recorded paths to clear");
            return;
        };
        let outcome = remove_routes(vault, &mut self.tracker);
        if let Some(error) = outcome.error {
            log::warn!(
                "route clear stopped after {}/{} durable removal(s): {error}",
                outcome.removed,
                outcome.total
            );
        } else {
            log::info!("cleared {} recorded route(s)", outcome.removed);
        }
    }

    /// The `J` key: start recording an expedition, or finish and persist it.
    fn toggle_route_recording(&mut self) {
        if !self.path_tracking {
            log::info!("path tracking is off (H to enable)");
            return;
        }
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
                let id = match vault.record_route(nodes, discoveries, name.clone()) {
                    Ok(id) => id,
                    Err(error) => {
                        log::warn!("route discarded: {error}");
                        return;
                    }
                };
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
        let id = match vault.record_discovery(&anchor, signature_seed, name.clone()) {
            Ok(id) => id,
            Err(error) => {
                log::warn!("discovery not recorded: {error}");
                return;
            }
        };
        // A discovery made mid-expedition joins the route's journal (§7.3).
        if let Some(recorder) = self.recorder.as_mut() {
            recorder.attach_discovery(id);
        }
        log::info!("recorded {name} ({id:#018x}) into the vault");
    }

    /// Synchronize every vault preserve into the live map (startup and session
    /// load) in canonical `(content id, region coordinate)` order. The runtime
    /// installs the complete batch before reconciling each touched resident
    /// once, so repeated synchronization is idempotent and reversed record
    /// traversal cannot create intermediate revision epochs (ADR 0020).
    fn apply_preserves(&mut self) {
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        let contributions: Vec<(u64, RegionCoord, PossibilitySignature)> = vault
            .preserves()
            .iter()
            .flat_map(|(&id, preserve)| {
                preserve
                    .regions
                    .iter()
                    .map(move |&(coord, signature)| (id, coord, signature))
            })
            .collect();
        self.map.apply_preserve_contributions(contributions);
    }

    /// The `P` key: standing inside a preserve deletes it (no snap — regions
    /// resume steering from where the preserve held them); otherwise the
    /// pinned near window is preserved — each region's possibility state,
    /// quantized, a few dozen bytes per region and zero geometry
    /// (phase-5-plan.md §7.5).
    fn toggle_preserve(&mut self) {
        if self.vault.is_none() {
            log::warn!("no vault open; set WER_VAULT_DIR or create ./wer-vault");
            return;
        }
        let player_region = RegionCoord::from_world(self.player.0, self.player.1);
        match remove_effective_preserve(
            &mut self.map,
            self.vault.as_mut().expect("checked above"),
            player_region,
        ) {
            Ok(Some((id, name))) => {
                log::info!(
                    "deleted preserve {name} ({id:#018x}); overlaps selected their next owner, final regions resume steering with no snap"
                );
                return;
            }
            Err(PreserveRemovalError::MissingVaultRecord(id)) => {
                log::warn!(
                    "runtime preserve owner {id:#018x} is absent from the vault; deletion skipped"
                );
                return;
            }
            Err(PreserveRemovalError::Persistence(error)) => {
                log::warn!("preserve deletion failed; runtime state retained: {error}");
                return;
            }
            Ok(None) => {}
        }

        let vault = self.vault.as_mut().expect("checked above");
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
        let id = match vault.record_preserve(regions.clone(), name.clone()) {
            Ok(id) => id,
            Err(error) => {
                log::warn!("preserve not created: {error}");
                return;
            }
        };
        let count = regions.len();
        self.map.apply_preserve_contributions(
            regions
                .into_iter()
                .map(|(coord, signature)| (id, coord, signature)),
        );
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
        let runtime = session_runtime_record(
            self.map.config(),
            &self.budget,
            Some(self.tier),
            self.path_tracking,
            self.route_attraction,
        );
        let recorder = self.recorder.as_ref().map(RouteRecorder::snapshot);
        let tracker = if self.path_tracking {
            self.tracker.snapshot()
        } else {
            world_core::RouteTrackerSnapshot::default()
        };
        if let Err(error) = vault.snapshot_session(SessionSnapshotInput {
            map: &self.map,
            player: self.player,
            last_player: self.last_player,
            bias: &self.bias,
            transition_mode: self.transition_mode,
            anchors: &self.anchors,
            runtime,
            recorder,
            tracker,
        }) {
            log::warn!("session save rejected before persistence: {error}");
            return;
        }
        match vault.flush_all() {
            Ok(stats) => {
                debug_assert!(stats.is_clean());
                self.vault_stats = stats;
                self.vault_failure_logged = false;
                log::info!(
                    "session saved: {} records, {} bytes ({} regions, {} anchors)",
                    stats.flushed,
                    stats.bytes,
                    self.map.len(),
                    self.anchors.len()
                );
            }
            Err(error) => {
                self.vault_stats = error.progress();
                self.vault_failure_logged = true;
                log::warn!("session save failed; dirty records remain retryable: {error}");
            }
        }
    }

    /// Restore the saved session (the `L` key): rebuild the streaming window
    /// from the snapshot's bit-exact region states; caches, rosters, and
    /// organisms re-derive over the following frames. `last_player` snaps to
    /// the restored position so the first update after a load is the zero-
    /// travel settle — loading is not an event (phase-5-plan.md §12.2).
    /// Exact canonical gameplay availability requires further zero-travel
    /// updates until `authoritative_realization_complete`; the shell guarantees
    /// the first such update but does not freeze later player input (ADR 0024).
    fn load_session(&mut self) {
        let Some(snap) = self.vault.as_ref().and_then(|v| v.session().cloned()) else {
            log::info!("no saved session in the vault");
            return;
        };
        let compatibility = compare_session_runtime(
            &snap.runtime,
            self.map.config(),
            &self.budget,
            Some(self.tier),
            self.path_tracking,
            self.route_attraction,
        );
        match compatibility {
            SessionCompatibility::Exact => log::info!("session metadata exact-compatible"),
            SessionCompatibility::CompatibleNotExact => {
                log::warn!(
                    "session metadata differs in pacing-only budget fields; restore is not exact"
                )
            }
            SessionCompatibility::Incompatible => {
                log::warn!("session metadata is incompatible with this run; restore is non-exact")
            }
        }
        let stream = if compatibility == SessionCompatibility::Exact {
            match stream_config_from_record(&snap.runtime.stream) {
                Ok(stream) => stream,
                Err(error) => {
                    log::warn!(
                        "session stream config is not representable on this platform ({error:?}); using current config"
                    );
                    *self.map.config()
                }
            }
        } else {
            *self.map.config()
        };
        let mut map = RegionMap::new(stream);
        apply_session_regions(&mut map, &snap);
        self.map = map;
        self.apply_preserves();
        self.player = snap.player;
        self.last_player = snap.player;
        self.bias = snap.bias;
        self.transition_mode = snap.transition_mode;
        self.anchors = snap.anchors.iter().map(|a| a.to_anchor()).collect();
        if compatibility != SessionCompatibility::Incompatible {
            self.path_tracking = snap.runtime.path_tracking;
            self.route_attraction = snap.runtime.route_attraction;
            self.recorder = snap.recorder.map(RouteRecorder::from_snapshot);
            self.tracker = RouteTracker::from_snapshot(snap.tracker);
        } else {
            self.recorder = None;
            self.tracker = RouteTracker::new();
        }
        log::info!(
            "session loaded: {} regions, {} anchors at ({:.0}, {:.0}); canonical organisms settle while held still",
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
    /// Map vs POV presentation; `Tab` toggles (3d-phase-1-plan.md §8.1).
    view_mode: ViewMode,
    /// The POV fly camera (presentation-side state only).
    pov_camera: PovCamera,
    /// POV chunk lifecycle: keying, Background-lane meshing, amortized
    /// upload, farthest-first eviction (3d-phase-1-plan.md §7).
    pov_chunks: PovChunkManager,
    /// Exact upload-only presentation of the currently published organisms.
    /// It scans only in POV and retains renderer replacement lists at rest.
    pov_organisms: PovOrganismManager,
    /// Chunk draw radius in regions (`WER_POV_RADIUS`, default 3).
    pov_radius: i32,
    /// Mouse look is a left-button drag (WSLg/XWayland delivers unusable
    /// raw `DeviceEvent` deltas — absolute jumps — so look input reads
    /// window-space cursor deltas instead). `Some(last cursor position)`
    /// while the button is held in POV.
    pov_look_from: Option<(f64, f64)>,
    /// The previous telemetry second's POV counters, for the delta log line.
    pov_counters_last: PovCounters,
    /// Previous organism lifecycle totals for the POV telemetry delta line.
    pov_organism_counters_last: PovOrganismCounters,
    /// Live POV diagnostic toggles (`B` directional shadows + terrain AO,
    /// `N` detail normals, `V` water) — presentation-only switches for
    /// chasing render cost.
    pov_toggles: PovToggles,
    /// The POV HUD chip (fps + frame-budget split), rebuilt only when the
    /// displayed text changes: `(rgba, width, height, text it shows)`.
    pov_fps_chip: Option<(Vec<u8>, u32, u32, String)>,
    /// POV render-scale (`WER_POV_SCALE`, clamped [0.25, 1.0]): fraction of
    /// the surface resolution the 3D pass rasterizes at before the linear
    /// upscale blit — the practical llvmpipe fps knob, since software
    /// rasterization cost scales with pixel count.
    pov_scale: f32,
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
    /// The most recent `World::update` counters, kept for the `F12` debug
    /// dump ([`dump`]) — the live frame consumes its stats by value.
    last_stats: FrameStats,
    last_frame: Instant,
    /// App start, for the water-wobble clock (3d-phase-3-plan.md §7.1):
    /// wrapped at `renderer::pov::WOBBLE_PERIOD` before it reaches the
    /// shader. Display-only animation; captures pass 0.0 instead.
    start: Instant,
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
            view_mode: if std::env::var_os("WER_POV").is_some_and(|v| v != "0") {
                ViewMode::Pov
            } else {
                ViewMode::Map
            },
            pov_camera: PovCamera::new(),
            pov_chunks: PovChunkManager::new(),
            pov_organisms: PovOrganismManager::new(),
            // The llvmpipe escape hatch (3d-phase-1-plan.md §8.5).
            pov_radius: std::env::var("WER_POV_RADIUS")
                .ok()
                .and_then(|v| v.parse::<i32>().ok())
                .map_or(3, |r| r.clamp(1, 8)),
            pov_look_from: None,
            pov_counters_last: PovCounters::default(),
            pov_organism_counters_last: PovOrganismCounters::default(),
            pov_toggles: PovToggles::default(),
            pov_fps_chip: None,
            pov_scale: std::env::var("WER_POV_SCALE")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .map_or(1.0, |s| s.clamp(0.25, 1.0)),
            overlay_hash: 0,
            panel_hash: 0,
            keys_down: HashSet::new(),
            modifiers: ModifiersState::empty(),
            cursor_pos: None,
            zoom: 1,
            scroll_accum: 0.0,
            regen_totals: [0; LAYER_COUNT as usize],
            last_stats: FrameStats::default(),
            last_frame: Instant::now(),
            start: Instant::now(),
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
        let Some((dx, dy)) = map_movement_delta(&self.keys_down, self.modifiers.shift_key(), dt)
        else {
            return;
        };
        self.world.player.0 += dx;
        self.world.player.1 += dy;
    }

    /// Toggle Map ↔ POV (`Tab`, 3d-phase-1-plan.md §8.1). Entering POV
    /// places the camera at eye level over the player. Chunks are kept
    /// across toggles (cheap to hold, instant on re-entry; §7.4).
    ///
    /// Mouse look is a **left-button drag** — no cursor grab. The plan's
    /// grab + raw-delta scheme is unusable on the reference environment:
    /// WSLg/XWayland reports raw `DeviceEvent::MouseMotion` as absolute
    /// jumps, which slammed the pitch to −89° on the first mouse move.
    /// Window-space drag deltas are well-defined everywhere.
    fn toggle_view_mode(&mut self) {
        match self.view_mode {
            ViewMode::Map => {
                self.view_mode = ViewMode::Pov;
                let ground = pov::entry_ground(&self.world.map, self.world.player);
                self.pov_camera.enter_at(self.world.player, ground);
                // Re-entering with walk mode still on grounds immediately
                // (the §5.3 snap) instead of ramping down from entry height.
                if self.pov_camera.walk {
                    let (ground, _) = self.pov_ground();
                    self.pov_camera.snap_to_ground(ground);
                }
                log::info!(
                    "view: POV at ({:.0}, {:.0}) radius {} (hold the left button to look; Tab returns to the map)",
                    self.world.player.0,
                    self.world.player.1,
                    self.pov_radius
                );
            }
            ViewMode::Pov => {
                self.view_mode = ViewMode::Map;
                self.pov_look_from = None;
                log::info!("view: map");
            }
        }
    }

    /// POV movement from held keys. Fly (plan §8.3): `W`/`S` along the full
    /// 3D view direction (it is a fly camera), `A`/`D` strafe in the yaw
    /// plane, `Space`/`LShift` world up/down. Walk (3d-phase-2-plan.md
    /// §5.3): `W`/`S` along the horizontal yaw direction — looking at your
    /// feet must not stop you — `A`/`D` strafe, `Space`/`LShift` consumed
    /// and ignored (reserved, design §2.1), then the terrain-following step
    /// toward `ground + EYE_HEIGHT` under the vertical-rate clamp. No
    /// lateral collision in either mode. Bypasses `apply_movement` entirely.
    fn apply_pov_movement(&mut self, dt: f64) {
        let walk = self.pov_camera.walk;
        let (forward, strafe, vertical) = pov_navigation_axis(&self.keys_down, walk);
        let mut mv = glam::DVec3::ZERO;
        mv += self.pov_forward() * forward;
        mv += self.pov_camera.right() * strafe;
        mv.z += vertical;
        if mv != glam::DVec3::ZERO {
            let speed = if walk {
                self.pov_camera.walk_speed
            } else {
                self.pov_camera.speed
            };
            self.pov_camera.pos += mv.normalize() * (speed * dt);
        }
        if walk {
            let (ground, _) = self.pov_ground();
            self.pov_camera.follow_ground(ground + pov::EYE_HEIGHT, dt);
        }
    }

    /// The active mode's forward direction: the pitched view direction in
    /// fly mode, the horizontal yaw direction in walk mode.
    fn pov_forward(&self) -> glam::DVec3 {
        if self.pov_camera.walk {
            self.pov_camera.walk_forward()
        } else {
            self.pov_camera.forward()
        }
    }

    /// The ground under the camera (3d-phase-2-plan.md §4.4): the rendered
    /// mesh where the chunk is resident, the analytic fallback at the
    /// loading frontier. The bool is the mesh-vs-analytic telemetry tag.
    fn pov_ground(&self) -> (f64, bool) {
        pov::walk_ground(
            &self.pov_chunks,
            &self.world.map,
            (self.pov_camera.pos.x, self.pov_camera.pos.y),
        )
    }

    /// `F` in POV (3d-phase-2-plan.md §6.1): toggle walk ↔ fly. Entering
    /// walk snaps the eye to the ground under the camera; returning to fly
    /// keeps position and orientation.
    fn toggle_walk(&mut self) {
        let walk = !self.pov_camera.walk;
        let (ground, mesh) = self.pov_ground();
        self.pov_camera.set_walk(walk, ground);
        if walk {
            log::info!(
                "pov: walk mode (ground {:.1}, {}; F returns to fly)",
                ground,
                if mesh { "mesh" } else { "analytic" }
            );
        } else {
            log::info!("pov: fly mode");
        }
    }

    /// One-shot actions on key press.
    fn handle_press(&mut self, code: KeyCode, event_loop: &ActiveEventLoop) {
        if let Some(action) = characterized_one_shot(self.view_mode, code) {
            reduce_characterized_one_shot(
                action,
                &mut self.channel,
                &mut self.pov_toggles.shadow_ao,
            );
            match action {
                CharacterizedOneShot::CycleMapChannel => {
                    log::info!("channel: {}", self.channel.name());
                }
                CharacterizedOneShot::TogglePovShadowAo => {
                    log::info!(
                        "pov: directional shadows and terrain AO {}",
                        onoff(self.pov_toggles.shadow_ao)
                    );
                }
            }
            return;
        }
        // The POV keybinding gate (3d-phase-1-plan.md §8.4): in POV only
        // `Tab`, `F` (walk ↔ fly, 3d-phase-2-plan.md §6.1), the `B`/`N`/`V`
        // diagnostic toggles, `Escape`, and the `F12` debug dump are handled;
        // every map binding below stays map-mode-only. This gate is the whole
        // guarantee that map mode is pixel-identical — no map state can even
        // be touched from POV (the dump only reads and writes files).
        if self.view_mode == ViewMode::Pov {
            match code {
                KeyCode::Tab => self.toggle_view_mode(),
                KeyCode::KeyF => self.toggle_walk(),
                KeyCode::KeyN => {
                    self.pov_toggles.detail_normals = !self.pov_toggles.detail_normals;
                    log::info!(
                        "pov: detail normals {} (per-fragment lattice hashing)",
                        onoff(self.pov_toggles.detail_normals)
                    );
                }
                KeyCode::KeyV => {
                    self.pov_toggles.water = !self.pov_toggles.water;
                    log::info!(
                        "pov: water passes {} (sea plane + river overlays)",
                        onoff(self.pov_toggles.water)
                    );
                }
                KeyCode::F12 => self.debug_dump(),
                KeyCode::Escape => event_loop.exit(),
                _ => {}
            }
            return;
        }
        if code == KeyCode::Tab {
            self.toggle_view_mode();
            return;
        }
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
                // the nearest authoritative slot-0 organism or the environment
                // channels, and nudges the target toward what makes the
                // discovery distinctive (ADR 0024).
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
            KeyCode::KeyH => self.world.toggle_path_tracking(),
            KeyCode::Delete => self.world.clear_routes(),
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
            KeyCode::F12 => self.debug_dump(),
            KeyCode::Escape => event_loop.exit(),
            _ => {}
        }
    }

    /// The panel's vault counters, shared by the live frame and the `F12`
    /// debug dump.
    fn vault_panel_info(&self) -> Option<VaultInfo> {
        self.world.vault.as_ref().map(|v| VaultInfo {
            records: v.discoveries().len() + v.routes().len() + v.preserves().len(),
            dirty: v.dirty_records(),
            seen: v.seen_count(),
            issues: v.issue_count(),
            suppressed_issues: v.suppressed_issue_count(),
            persistence_retries: v
                .active_persistence_issue()
                .map_or(0, world_runtime::VaultIssue::occurrences),
        })
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
        // Route polylines are part of the optional path subsystem: while
        // tracking is off the map shows no paths (the records stay in the
        // vault, just undrawn).
        let routes = if self.world.path_tracking {
            vault
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
                .collect()
        } else {
            Vec::new()
        };
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

        match self.view_mode {
            ViewMode::Map => self.apply_movement(dt),
            ViewMode::Pov => {
                // The POV camera *is* the player (plan §8.1): recenter the
                // world before `update` so streaming, retarget, and
                // realization follow the camera — travel-fueled drift works
                // identically to map-mode travel.
                self.apply_pov_movement(dt);
                self.world.player = (self.pov_camera.pos.x, self.pov_camera.pos.y);
            }
        }

        let update_start = Instant::now();
        let stats = self.world.update();
        let update_seconds = update_start.elapsed().as_secs_f64();
        self.last_stats = stats;
        for (total, &count) in self
            .regen_totals
            .iter_mut()
            .zip(&stats.regenerated_by_layer)
        {
            *total += count as u64;
        }

        if self.view_mode == ViewMode::Pov {
            self.frame_pov(stats, update_seconds);
            return;
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
            vault: self.vault_panel_info(),
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

    /// The POV half of [`Self::frame`] (3d-phase-1-plan.md §8.1): sync the
    /// chunk lifecycle, build the frame parameters with glam, and present
    /// through [`Renderer::render_pov`]. HUD, panel, and cursor info are
    /// skipped in POV (design §2.1); the telemetry accumulators still run.
    fn frame_pov(&mut self, mut stats: FrameStats, update_seconds: f64) {
        // The frame-side POV work (scheduling + amortized integration) fills
        // the Mesh pass, following the Flush precedent (plan §8.1); the
        // worker-side mesh milliseconds ride the manager's counters instead.
        let mesh_start = Instant::now();
        let (uploads, removes) = self.pov_chunks.sync(
            &self.world.map,
            (self.pov_camera.pos.x, self.pov_camera.pos.y),
            self.pov_radius,
            self.world.executor.as_ref(),
        );
        let organisms_changed = self.pov_organisms.sync(
            &self.world.map,
            &self.pov_chunks,
            (self.pov_camera.pos.x, self.pov_camera.pos.y),
            pov::pov_fog_end(self.pov_radius),
        );
        let organism_upload = organisms_changed.then(|| self.pov_organisms.upload());
        let mut upload_bytes = (uploads.len()
            * renderer::pov::VERTS_PER_CHUNK
            * core::mem::size_of::<renderer::PovVertex>()
            + uploads
                .iter()
                .map(|u| u.river_indices.len() * 4)
                .sum::<usize>()) as u64;
        stats.pass_ms[world_runtime::Pass::Mesh.index()] +=
            mesh_start.elapsed().as_secs_f32() * 1000.0;
        self.last_stats = stats;

        let render_start = Instant::now();
        let mut organism_buffer_stats = None;
        if let Some(renderer) = self.renderer.as_mut() {
            let (w, h) = renderer.size();
            // The water-wobble clock (3d-phase-3-plan.md §7.1): wrapped at
            // the shader's period so f32 never loses phase precision.
            let time = self
                .start
                .elapsed()
                .as_secs_f64()
                .rem_euclid(f64::from(renderer::pov::WOBBLE_PERIOD)) as f32;
            let resolution = pov::shadow_resolution(self.tier);
            let shadow = pov::shadow_frame(
                &self.pov_camera,
                &self.pov_chunks,
                self.pov_organisms.shadow_bounds(),
                resolution,
            );
            let params = pov::frame_params(
                &self.pov_camera,
                w as f32 / h.max(1) as f32,
                self.pov_radius,
                CLEAR_COLOR,
                time,
                self.pov_toggles,
                shadow,
            );
            // The corner HUD chip: fps plus the frame-budget split — `upd`
            // is the world update, `gpu` is the whole render+present call —
            // so a present-bound frame (the llvmpipe/WSLg regime, where
            // window pixels dominate and shading toggles barely move fps)
            // is visible at a glance. Rebuilt only when the once-per-second
            // telemetry roll changes the text.
            let text = format!(
                "{:>4} fps  upd {:>5.1}ms  gpu {:>5.1}ms",
                self.fps, self.update_ms, self.render_ms
            );
            if !matches!(&self.pov_fps_chip, Some((_, _, _, shown)) if *shown == text) {
                let (rgba, cw, ch) = panel::hud_chip(&text);
                self.pov_fps_chip = Some((rgba, cw, ch, text));
            }
            let hud = self
                .pov_fps_chip
                .as_ref()
                .map(|(rgba, cw, ch, _)| (rgba.as_slice(), *cw, *ch));
            renderer.render_pov(
                &params,
                &uploads,
                &removes,
                organism_upload,
                CLEAR_COLOR,
                hud,
                self.pov_scale,
            );
            organism_buffer_stats = renderer.pov_organism_stats();
            if let Some(buffers) = organism_buffer_stats {
                upload_bytes += buffers.replacement_bytes;
            }
        }
        let render_seconds = render_start.elapsed().as_secs_f64();

        // The once-per-second POV log line (plan §7.5): the steady-state
        // exit criterion reads these — travel stopped ⇒ remeshed stays flat.
        if self.last_telemetry.elapsed().as_secs_f64() >= 1.0 {
            let c = self.pov_chunks.counters();
            let last = self.pov_counters_last;
            let organisms = self.pov_organisms.counters();
            let last_organisms = self.pov_organism_counters_last;
            // The mode tail: the walk form's mesh-vs-analytic tag is the
            // observable for the frontier-fallback exit criterion
            // (3d-phase-2-plan.md §6.2).
            let mode = if self.pov_camera.walk {
                let (ground, mesh) = self.pov_ground();
                format!(
                    "walk {:.0}u/s (ground {:.1}, {})",
                    self.pov_camera.walk_speed,
                    ground,
                    if mesh { "mesh" } else { "analytic" }
                )
            } else {
                format!("fly {:.0}u/s", self.pov_camera.speed)
            };
            let buffers = organism_buffer_stats.map_or_else(
                || String::from("gpu buffers pending"),
                |stats| {
                    format!(
                        "gpu {} box + {} sphere, {:.1}/{:.1} KiB live/cap",
                        stats.box_count,
                        stats.sphere_count,
                        stats.live_bytes as f64 / 1024.0,
                        stats.capacity_bytes as f64 / 1024.0,
                    )
                },
            );
            log::info!(
                "pov: {} chunks | +meshed {} +remeshed {} +cancelled {} +stale {} +deferred {} | mesh {:.1}ms worker total | organisms {}/{} (box {}, sphere {}, waiting {}, culled {}) +rebuild {} +upload {} inst/{:.1} KiB, {buffers} | {mode}",
                self.pov_chunks.len(),
                c.meshed - last.meshed,
                c.remeshed - last.remeshed,
                c.cancelled - last.cancelled,
                c.dropped_stale - last.dropped_stale,
                c.uploads_deferred - last.uploads_deferred,
                c.mesh_ms,
                organisms.drawn(),
                organisms.published,
                organisms.boxes,
                organisms.spheres,
                organisms.waiting_for_ground,
                organisms.distance_culled,
                organisms.rebuilds - last_organisms.rebuilds,
                organisms.uploaded_instances - last_organisms.uploaded_instances,
                (organisms.uploaded_bytes - last_organisms.uploaded_bytes) as f64 / 1024.0,
            );
            self.pov_counters_last = c;
            self.pov_organism_counters_last = organisms;
        }
        self.update_telemetry(
            update_seconds,
            0.0,
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

        let mut attributes = Window::default_attributes()
            .with_title("Infinite World Exploration — Phase 1 continuity prototype");
        // `WER_WINDOW=WxH`: fixed window size, for reproducible performance
        // measurements (fragment and present cost scale with pixel count on
        // a software rasterizer, so comparable numbers need a pinned size).
        if let Ok(v) = std::env::var("WER_WINDOW") {
            if let Some((w, h)) = v.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    attributes = attributes.with_inner_size(winit::dpi::PhysicalSize::new(w, h));
                }
            }
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .expect("failed to create window"),
        );

        // `WER_START=x,y`: spawn the player at a world position — jump
        // straight to a scene of interest (a coast, a river) for debugging
        // and measurements without flying there first.
        if let Ok(v) = std::env::var("WER_START") {
            if let Some((x, y)) = v.split_once(',') {
                if let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) {
                    self.world.player = (x, y);
                }
            }
        }
        let size = window.inner_size();
        // The renderer gets a source of fresh surface targets (not a single
        // surface) so it can rebuild the swapchain if the platform loses it —
        // which WSLg does routinely.
        let surface_window = window.clone();
        let renderer = pollster::block_on(Renderer::new(
            Box::new(move || surface_window.clone().into()),
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
        // `WER_POV=1` starts directly in POV (plan §8.5).
        if self.view_mode == ViewMode::Pov {
            let ground = pov::entry_ground(&self.world.map, self.world.player);
            self.pov_camera.enter_at(self.world.player, ground);
            log::info!("starting in POV (WER_POV set), radius {}", self.pov_radius);
        }
        if self.pov_scale < 1.0 {
            log::info!(
                "POV render scale {} (WER_POV_SCALE): 3D rasterizes at {:.0}% of the window pixels",
                self.pov_scale,
                f64::from(self.pov_scale * self.pov_scale) * 100.0
            );
        }
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
                // POV drag look: window-space deltas while the left button
                // is held — the same math `--pov-script` simulates with
                // `mouse:dx,dy`. (Raw device deltas are unusable under
                // WSLg/XWayland: they arrive as absolute jumps.)
                let current = (position.x, position.y);
                if let Some((dx, dy)) = primary_drag_delta(self.pov_look_from, current) {
                    self.pov_camera.look(dx, dy);
                    self.pov_look_from = Some((position.x, position.y));
                }
                self.cursor_pos = Some(current);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_pos = None;
                cancel_primary_drag(&mut self.pov_look_from);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // Mouse look is active only while the left button is held
                // in POV; releasing (or leaving the window) ends the drag.
                update_primary_drag_gate(
                    self.view_mode,
                    button,
                    state == ElementState::Pressed,
                    self.cursor_pos,
                    &mut self.pov_look_from,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    // Touchpads scroll in pixels; ~40 px per wheel notch.
                    MouseScrollDelta::PixelDelta(pos) => pos.y / WHEEL_PIXELS_PER_NOTCH,
                };
                let notches = accumulate_wheel(&mut self.scroll_accum, delta);
                if self.view_mode == ViewMode::Pov {
                    // Wheel = fly-speed multiplier in POV (plan §8.3); the
                    // map zoom below stays map-mode-only.
                    for _ in 0..notches.max(0) {
                        self.pov_camera.scroll_speed(true);
                    }
                    for _ in 0..(-notches).max(0) {
                        self.pov_camera.scroll_speed(false);
                    }
                    return;
                }
                let before = self.zoom;
                for _ in 0..notches.max(0) {
                    self.zoom = (self.zoom * 2).min(MAX_ZOOM);
                }
                for _ in 0..(-notches).max(0) {
                    self.zoom = (self.zoom / 2).max(1);
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
                    if dispatch_one_shot(repeat) {
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
    dump::write_ppm(std::path::Path::new(path), pixels, width, height)?;
    log::info!(
        "wrote {width}x{height} {} map+panel at ({}, {}) to {path}",
        channel.name(),
        pos.0,
        pos.1
    );
    Ok(())
}

/// Headless scripted POV capture (`wer --pov-script`, ADR 0021): drive the
/// camera through the *same* [`PovCamera`] paths the live shell uses
/// (`mouse:` goes through `look`, `move:` through the fly-movement basis),
/// settle the world with the inline executor, mesh with the same
/// [`PovChunkManager`], render offscreen, and write binary PPMs — the
/// debugging/testing harness for POV rendering. No window, no event loop.
fn run_pov_script(script: &str) -> Result<(), String> {
    let instrs = pov::parse_pov_script(script)?;
    let cfg = StreamConfig::default();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let mut map = RegionMap::new(cfg);
    let mut camera = PovCamera::new();
    let mut chunks = PovChunkManager::new();
    let mut organisms = PovOrganismManager::new();
    let radius = std::env::var("WER_POV_RADIUS")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .map_or(3, |r| r.clamp(1, 8));
    // Honor the live render-scale knob so scaled frames are inspectable
    // headlessly (the upscale-blit path, not just the shading).
    let scale = std::env::var("WER_POV_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .map_or(1.0, |s| s.clamp(0.25, 1.0));
    let mut size = (1024u32, 768u32);
    let mut capture: Option<renderer::pov::PovCapture> = None;

    fn settle_world(
        map: &mut RegionMap,
        pos: (f64, f64),
        field: &PossibilityField,
        bias: &[f32; POSSIBILITY_DIMS],
        n: u32,
    ) {
        for _ in 0..n {
            // Zero travel, unbudgeted, inline: the screenshot-path settle.
            map.update(
                pos,
                0.0,
                field,
                &[],
                bias,
                &Budget::unlimited(),
                &world_runtime::InlineExecutor,
                false,
            );
        }
    }

    // Default start: over the origin at entry eye height (like `WER_POV=1`).
    camera.enter_at((0.0, 0.0), pov::entry_ground(&map, (0.0, 0.0)));

    for instr in instrs {
        match instr {
            pov::PovInstr::Size(w, h) => {
                if capture.is_some() {
                    return Err(String::from("size must come before the first snap"));
                }
                size = (w, h);
            }
            pov::PovInstr::Pos(x, y, z) => match z {
                Some(z) => camera.pos = glam::DVec3::new(x, y, z),
                None => {
                    // Ground placement wants the covering region's realized
                    // vector; one settle makes it resident first. In walk
                    // mode `pos:` grounds at eye height (the §5.3 snap)
                    // instead of hovering at entry height.
                    camera.pos.x = x;
                    camera.pos.y = y;
                    settle_world(&mut map, (x, y), &field, &bias, 1);
                    if camera.walk {
                        let (ground, _) = pov::walk_ground(&chunks, &map, (x, y));
                        camera.snap_to_ground(ground);
                    } else {
                        camera.enter_at((x, y), pov::entry_ground(&map, (x, y)));
                    }
                }
            },
            instr @ (pov::PovInstr::Mouse(..)
            | pov::PovInstr::Move { .. }
            | pov::PovInstr::Walk
            | pov::PovInstr::Fly) => {
                // The shared camera semantics (3d-phase-2-plan.md §6.4):
                // `walk`/`fly` through the live toggle path, walk-mode
                // `move` snapping to the settled ground at the destination.
                let _ = pov::apply_camera_instr(&mut camera, &instr, &mut |x, y| {
                    settle_world(&mut map, (x, y), &field, &bias, 1);
                    pov::walk_ground(&chunks, &map, (x, y)).0
                });
            }
            pov::PovInstr::Settle(n) => {
                settle_world(&mut map, (camera.pos.x, camera.pos.y), &field, &bias, n);
            }
            pov::PovInstr::Snap(path) => {
                // Canonical near-field realization advances at a bounded
                // number of regions per update. Wait for its explicit
                // completion observation rather than assuming the old fixed
                // eight-update terrain settle also published every organism.
                let camera_xy = (camera.pos.x, camera.pos.y);
                // Populate the field-active near window before consulting the
                // completion observation; an entirely empty fresh map would
                // otherwise be vacuously complete.
                settle_world(&mut map, camera_xy, &field, &bias, 8);
                let mut realization_updates = 8u32;
                while realization_updates < 128
                    && !map.authoritative_realization_complete(camera_xy)
                {
                    settle_world(&mut map, camera_xy, &field, &bias, 1);
                    realization_updates += 1;
                }
                if !map.authoritative_realization_complete(camera_xy) {
                    return Err(format!(
                        "POV snapshot at ({:.1}, {:.1}) did not complete authoritative organism realization after 128 zero-travel updates",
                        camera_xy.0, camera_xy.1
                    ));
                }
                if capture.is_none() {
                    capture = Some(
                        renderer::pov::PovCapture::new(size.0, size.1)
                            .map_err(|e| format!("pov capture init: {e}"))?,
                    );
                }
                let cap = capture.as_mut().expect("just ensured");
                for _ in 0..256 {
                    let (uploads, removes) = chunks.sync(
                        &map,
                        (camera.pos.x, camera.pos.y),
                        radius,
                        &world_runtime::InlineExecutor,
                    );
                    let done = uploads.is_empty() && chunks.is_idle();
                    cap.apply(&uploads, &removes, None);
                    if done {
                        break;
                    }
                }
                let organisms_changed =
                    organisms.sync(&map, &chunks, camera_xy, pov::pov_fog_end(radius));
                cap.apply(&[], &[], organisms_changed.then(|| organisms.upload()));
                let aspect = size.0 as f32 / size.1 as f32;
                // Time-frozen captures (3d-phase-3-plan.md §4.3): two snaps
                // of the same pose are byte-comparable; toggles all-on.
                let shadow = pov::shadow_frame(
                    &camera,
                    &chunks,
                    organisms.shadow_bounds(),
                    pov::shadow_resolution(ResourceTier::Low),
                );
                let params = pov::frame_params(
                    &camera,
                    aspect,
                    radius,
                    CLEAR_COLOR,
                    0.0,
                    pov::PovToggles::default(),
                    shadow,
                );
                let rgba = cap.snapshot_at_scale(&params, CLEAR_COLOR, scale);
                dump::write_ppm(std::path::Path::new(&path), &rgba, size.0, size.1)?;
                let organism_counts = organisms.counters();
                let organism_buffers = cap.organism_stats();
                log::info!(
                    "pov snapshot {path}: {}x{} at ({:.1}, {:.1}, {:.1}) yaw {:.1}° pitch {:.1}° | {} chunks | {}/{} organisms drawn (box {}, sphere {}, waiting {}, culled {}; realization {} updates) | instances {:.1}/{:.1} KiB live/cap, {:.1} KiB replacement",
                    size.0,
                    size.1,
                    camera.pos.x,
                    camera.pos.y,
                    camera.pos.z,
                    camera.yaw.to_degrees(),
                    camera.pitch.to_degrees(),
                    chunks.len(),
                    organism_counts.drawn(),
                    organism_counts.published,
                    organism_counts.boxes,
                    organism_counts.spheres,
                    organism_counts.waiting_for_ground,
                    organism_counts.distance_culled,
                    realization_updates,
                    organism_buffers.live_bytes as f64 / 1024.0,
                    organism_buffers.capacity_bytes as f64 / 1024.0,
                    organism_buffers.replacement_bytes as f64 / 1024.0,
                );
            }
        }
    }
    Ok(())
}

/// "on"/"off" for toggle log lines.
fn onoff(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
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
    if let Some(rest) = args
        .split_first()
        .and_then(|(first, rest)| (first == "--pov-script").then_some(rest))
    {
        let usage = "usage: wer --pov-script \"pos:300,-10; snap:a.ppm; mouse:200,-50; move:150; snap:b.ppm\"\n\
                     instructions: size:WxH | pos:x,y[,z] | mouse:dx,dy | move:f[,r[,u]] | settle[:n] | snap:file.ppm";
        match rest {
            [script] => {
                if let Err(err) = run_pov_script(script) {
                    eprintln!("pov script failed: {err}\n{usage}");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(1);
            }
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

#[cfg(test)]
mod preserve_lifecycle_tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use world_runtime::{MemoryStorage, StorageError};

    #[derive(Debug, Clone)]
    struct FailingRemoveStorage {
        inner: MemoryStorage,
        fail_remove_at: Rc<Cell<Option<usize>>>,
        remove_calls: Rc<Cell<usize>>,
    }

    impl FailingRemoveStorage {
        fn new() -> Self {
            Self {
                inner: MemoryStorage::new(),
                fail_remove_at: Rc::new(Cell::new(None)),
                remove_calls: Rc::new(Cell::new(0)),
            }
        }

        fn fail_remove_call(&self, index: usize) {
            self.fail_remove_at.set(Some(index));
        }
    }

    impl Storage for FailingRemoveStorage {
        fn load(&self, key: &[u8]) -> Result<Vec<u8>, StorageError> {
            self.inner.load(key)
        }

        fn store(&mut self, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
            self.inner.store(key, value)
        }

        fn remove(&mut self, key: &[u8]) -> Result<(), StorageError> {
            let call = self.remove_calls.get();
            self.remove_calls.set(call + 1);
            if self.fail_remove_at.get() == Some(call) {
                self.fail_remove_at.set(None);
                return Err(StorageError::Backend("native remove fault".into()));
            }
            self.inner.remove(key)
        }

        fn keys_with_prefix(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>, StorageError> {
            self.inner.keys_with_prefix(prefix)
        }
    }

    #[test]
    fn native_deletion_removes_effective_owner_and_reveals_successor() {
        let coord = RegionCoord::new(0, 0);
        let first = PossibilitySignature::of(world_core::PossibilityVector::neutral());
        let mut second = first;
        second.buckets[PossibilityDomain::Aesthetics.index()] = 4000;
        let mut vault = Vault::open(MemoryStorage::new()).unwrap();
        vault
            .record_preserve(vec![(coord, first)], "first".into())
            .unwrap();
        vault
            .record_preserve(vec![(coord, second)], "second".into())
            .unwrap();
        let (&winner_id, winner) = vault.preserves().first_key_value().unwrap();
        let winner_name = winner.name.clone();
        let winner_signature = winner.regions[0].1;
        let (&successor_id, successor) = vault.preserves().last_key_value().unwrap();
        let successor_signature = successor.regions[0].1;

        let mut map = RegionMap::new(StreamConfig::default());
        let contributions: Vec<_> = vault
            .preserves()
            .iter()
            .rev()
            .flat_map(|(&id, preserve)| {
                preserve
                    .regions
                    .iter()
                    .map(move |&(region, signature)| (id, region, signature))
            })
            .collect();
        map.apply_preserve_contributions(contributions);
        assert_eq!(
            map.effective_preserve(coord),
            Some((winner_id, winner_signature))
        );

        assert_eq!(
            remove_effective_preserve(&mut map, &mut vault, coord).unwrap(),
            Some((winner_id, winner_name))
        );
        assert!(!vault.preserves().contains_key(&winner_id));
        assert_eq!(
            map.effective_preserve(coord),
            Some((successor_id, successor_signature))
        );
    }

    #[test]
    fn failed_native_preserve_delete_keeps_vault_and_map_until_retry() {
        let coord = RegionCoord::new(0, 0);
        let first = PossibilitySignature::of(world_core::PossibilityVector::neutral());
        let mut second = first;
        second.buckets[PossibilityDomain::Aesthetics.index()] = 4000;
        let storage = FailingRemoveStorage::new();
        let control = storage.clone();
        let mut vault = Vault::open(storage).unwrap();
        vault
            .record_preserve(vec![(coord, first)], "first".into())
            .unwrap();
        vault
            .record_preserve(vec![(coord, second)], "second".into())
            .unwrap();
        vault.flush_all().unwrap();
        let (&winner_id, winner) = vault.preserves().first_key_value().unwrap();
        let winner_signature = winner.regions[0].1;
        let (&successor_id, successor) = vault.preserves().last_key_value().unwrap();
        let successor_signature = successor.regions[0].1;
        let mut map = RegionMap::new(StreamConfig::default());
        map.apply_preserve_contributions(vault.preserves().iter().flat_map(|(&id, preserve)| {
            preserve
                .regions
                .iter()
                .map(move |&(region, signature)| (id, region, signature))
        }));

        control.fail_remove_call(0);
        assert!(matches!(
            remove_effective_preserve(&mut map, &mut vault, coord),
            Err(PreserveRemovalError::Persistence(_))
        ));
        assert!(vault.preserves().contains_key(&winner_id));
        assert_eq!(
            map.effective_preserve(coord),
            Some((winner_id, winner_signature))
        );

        assert!(remove_effective_preserve(&mut map, &mut vault, coord)
            .unwrap()
            .is_some());
        assert!(!vault.preserves().contains_key(&winner_id));
        assert_eq!(
            map.effective_preserve(coord),
            Some((successor_id, successor_signature))
        );
    }

    #[test]
    fn route_clear_failure_retains_failed_and_unvisited_routes_and_tracking() {
        let storage = FailingRemoveStorage::new();
        let control = storage.clone();
        let mut vault = Vault::open(storage).unwrap();
        for marker in 0..3 {
            vault
                .record_route(
                    vec![world_core::RouteNode {
                        pos_q: (0, 0),
                        signature: PossibilitySignature::of(
                            world_core::PossibilityVector::neutral(),
                        ),
                        current_signature: None,
                        cost_q: marker,
                        stability_q: 0,
                        anchor_sig: u64::from(marker),
                        distance_q: 0,
                    }],
                    vec![],
                    format!("route-{marker}"),
                )
                .unwrap();
        }
        vault.flush_all().unwrap();
        let ids: Vec<u64> = vault.routes().keys().copied().collect();
        let mut tracker = RouteTracker::new();
        assert!(tracker
            .observe(vault.routes().values(), (0.0, 0.0))
            .is_empty());

        control.fail_remove_call(1);
        let outcome = remove_routes(&mut vault, &mut tracker);
        assert_eq!((outcome.removed, outcome.total), (1, 3));
        assert!(outcome.error.is_some());
        assert!(!vault.routes().contains_key(&ids[0]));
        assert!(vault.routes().contains_key(&ids[1]));
        assert!(vault.routes().contains_key(&ids[2]));

        let completed = tracker.observe(vault.routes().values(), (10_000.0, 10_000.0));
        assert_eq!(
            completed,
            ids[1..],
            "tracking survives for every retained route"
        );
    }

    #[test]
    fn route_recording_signs_the_effective_explicit_and_derived_anchors() {
        let run = |reverse_explicit: bool, reverse_routes: bool, suffix: &str| {
            let path = std::env::temp_dir().join(format!(
                "wer-a7-effective-route-{}-{suffix}",
                std::process::id()
            ));
            let storage = FileStorage::open(&path).unwrap();
            let mut vault = Vault::open(storage).unwrap();
            let route_nodes = |bucket: u16, cost| {
                let mut possibility = world_core::PossibilityVector::neutral();
                possibility.set(PossibilityDomain::Ecology, f32::from(bucket) / 4096.0);
                (0..16)
                    .map(|_| world_core::RouteNode {
                        pos_q: (0, 0),
                        signature: PossibilitySignature::of(possibility),
                        current_signature: None,
                        cost_q: cost,
                        stability_q: 200,
                        anchor_sig: 0,
                        distance_q: 0,
                    })
                    .collect()
            };
            let mut routes = vec![(3000, 10), (3800, 20)];
            if reverse_routes {
                routes.reverse();
            }
            for (bucket, cost) in routes {
                vault
                    .record_route(
                        route_nodes(bucket, cost),
                        vec![],
                        format!("nearby source route {bucket}"),
                    )
                    .unwrap();
            }

            let tier = ResourceTier::Low;
            let mask = domain_mask(&[PossibilityDomain::Ecology]);
            let suppress = Anchor {
                world_pos: (32.0, -16.0),
                target: bound_target(mask, 0.88),
                mask,
                kind: AnchorKind::Suppress,
                strength: 0.7,
                falloff_radius: 1400.0,
                source: AnchorSource::Landform,
            };
            let emphasize = Anchor {
                world_pos: (-24.0, 8.0),
                target: bound_target(mask, 0.72),
                kind: AnchorKind::Emphasize,
                strength: 0.35,
                ..suppress
            };
            let mut explicit = vec![suppress, emphasize];
            if reverse_explicit {
                explicit.reverse();
            }
            let mut world = World {
                map: RegionMap::new(tier.stream_config()),
                field: PossibilityField::default(),
                anchors: Vec::new(),
                bias: [0.0; POSSIBILITY_DIMS],
                player: (0.0, 0.0),
                last_player: (0.0, 0.0),
                transition_mode: false,
                vault: Some(vault),
                vault_stats: VaultStats::default(),
                vault_failure_logged: false,
                recorder: None,
                tracker: RouteTracker::new(),
                path_tracking: false,
                route_attraction: true,
                executor: Box::new(world_runtime::InlineExecutor),
                budget: tier.budget(),
                tier,
            };
            // Publish canonical organisms and establish the unsteered current
            // before the same effective slice refreshes target and resonance.
            for _ in 0..128 {
                world.update();
                if world.map.authoritative_realization_complete(world.player) {
                    break;
                }
            }
            assert!(world.map.authoritative_realization_complete(world.player));
            world.anchors = explicit;
            world.path_tracking = true;
            world.recorder = Some(RouteRecorder::new());

            let vault = world.vault.as_ref().unwrap();
            let derived = world_core::attraction_anchors(
                vault.routes().values(),
                world.player,
                world.budget.max_route_attraction_nodes,
            );
            assert_eq!(derived.len(), 32);
            assert!(derived
                .iter()
                .all(|anchor| anchor.strength < world_core::route_pull(0)));
            assert!(world_core::anchor_influence_profile(&derived, world.player)
                .into_iter()
                .all(|pull| pull <= world_core::ROUTE_PULL_CAP));
            let mut effective = world.anchors.clone();
            let explicit_only = world_core::anchor_set_signature(&effective);
            effective.extend(derived.iter().copied());
            let expected_signature = world_core::anchor_set_signature(&effective);
            assert_ne!(expected_signature, explicit_only);

            let stats = world.update();
            let resonance = world.map.resonance_at(world.player, &effective);
            assert!(!resonance.nodes.is_empty());
            assert!(resonance.anchor_compatibility < 1.0);
            assert_eq!(
                stats.resonance_strength.to_bits(),
                resonance.strength.to_bits()
            );
            let coord = RegionCoord::from_world(world.player.0, world.player.1);
            let target_bits = world.map.get(coord).unwrap().target.dims.map(f32::to_bits);
            let (nodes, discoveries) = world.recorder.take().unwrap().finish();
            assert_eq!(nodes.len(), 1);
            assert_eq!(nodes[0].anchor_sig, expected_signature);
            assert_eq!(
                nodes[0].cost_q,
                ((1.0 - stats.resonance_strength.clamp(0.0, 1.0)) * 255.0) as u8
            );
            effective.reverse();
            assert_eq!(
                nodes[0].anchor_sig,
                world_core::anchor_set_signature(&effective)
            );
            let record = world_core::RouteRecord::new(
                nodes.clone(),
                discoveries,
                99,
                "permutation probe".into(),
            );
            let bytes = world_core::encode_record(world_core::RecordKind::Route, &record);
            let mut strength_bits: Vec<_> = derived
                .iter()
                .map(|anchor| anchor.strength.to_bits())
                .collect();
            strength_bits.sort_unstable();
            let image = (
                target_bits,
                resonance.anchor_compatibility.to_bits(),
                resonance.strength.to_bits(),
                nodes[0].cost_q,
                nodes[0].anchor_sig,
                record.id,
                bytes,
                strength_bits,
            );

            drop(world);
            std::fs::remove_dir_all(path).unwrap();
            image
        };

        let forward = run(false, false, "forward");
        let reversed = run(true, true, "reversed");
        assert_eq!(forward, reversed);
    }
}

#[cfg(test)]
mod alignment_characterization_tests {
    use super::*;
    use std::fmt::Write as _;
    use world_runtime::{Budget, InlineExecutor};

    fn settled_map() -> RegionMap {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 8,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let mut map = RegionMap::new(cfg);
        for _ in 0..6 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        map
    }

    fn option_f32_bits(value: Option<f32>) -> String {
        value.map_or_else(|| String::from("none"), |v| format!("{:08x}", v.to_bits()))
    }

    #[test]
    fn map_movement_helper_preserves_legacy_evaluation_order() {
        let controls = [
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyA,
            KeyCode::KeyD,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
        ];
        let dts = [0.0, 0.000_002_999_991, 0.007, 0.1, 1.0 / 60.0];
        for mask in 0u16..(1 << controls.len()) {
            let keys: HashSet<_> = controls
                .iter()
                .enumerate()
                .filter_map(|(index, &key)| (mask & (1 << index) != 0).then_some(key))
                .collect();
            for sprint in [false, true] {
                for dt in dts {
                    // The pre-characterization `apply_movement` expression,
                    // copied as an independent bit-exact oracle.
                    let down = |code| keys.contains(&code);
                    let mut dx: f64 = 0.0;
                    let mut dy: f64 = 0.0;
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
                    let len = f64::hypot(dx, dy);
                    let expected = if len == 0.0 {
                        None
                    } else {
                        let multiplier = if sprint { 4.0 } else { 1.0 };
                        let step = PLAYER_SPEED * multiplier * dt / len;
                        Some((dx * step, dy * step))
                    };
                    let actual = map_movement_delta(&keys, sprint, dt);
                    assert_eq!(
                        actual.map(|(x, y)| (x.to_bits(), y.to_bits())),
                        expected.map(|(x, y)| (x.to_bits(), y.to_bits())),
                        "mask {mask:#05x}, sprint {sprint}, dt {dt:?}"
                    );
                }
            }
        }
    }

    /// Pins the semantic source values that the native panel samples before
    /// the model and conversion code move to `viewer-host` (alignment M0).
    /// Float bits are recorded exactly; this is not a formatted HUD-pixel
    /// fixture and it does not extend presentation values into world identity.
    #[test]
    fn native_panel_source_characterization() {
        let map = settled_map();
        let source = map
            .organisms()
            .min_by_key(|organism| (organism.id, organism.slot))
            .copied()
            .expect("settled characterization map has organisms");
        let cursor = App::sample_cursor(&map, source.world_pos);
        let organism = App::pick_organism(&map, source.world_pos)
            .expect("sampling at a rendered organism selects an organism");
        assert_eq!(organism.id, source.id);
        assert_eq!(organism.species, source.species);
        let ecology = cursor
            .ecology
            .as_ref()
            .expect("the selected organism's settled cell has ecology");

        let mut actual = String::from("native-panel-source-characterization-v1\n");
        writeln!(
            &mut actual,
            "cursor.world {:016x} {:016x}",
            cursor.world.0.to_bits(),
            cursor.world.1.to_bits()
        )
        .unwrap();
        writeln!(
            &mut actual,
            "cursor.region {} {}",
            cursor.region.0, cursor.region.1
        )
        .unwrap();
        writeln!(&mut actual, "cursor.status {}", cursor.status).unwrap();
        writeln!(
            &mut actual,
            "cursor.stability {:08x}",
            cursor.stability.to_bits()
        )
        .unwrap();
        writeln!(&mut actual, "cursor.revision {}", cursor.revision).unwrap();
        for (name, value) in [
            ("elevation", cursor.elevation),
            ("temperature", cursor.temperature),
            ("moisture", cursor.moisture),
            ("hardness", cursor.hardness),
            ("river", cursor.river),
            ("wetness", cursor.wetness),
            ("soil-depth", cursor.soil_depth),
            ("fertility", cursor.fertility),
            ("vegetation", cursor.vegetation),
            ("canopy", cursor.canopy),
        ] {
            writeln!(&mut actual, "cursor.{name} {}", option_f32_bits(value)).unwrap();
        }
        writeln!(
            &mut actual,
            "cursor.biome {}",
            cursor.biome.unwrap_or("none")
        )
        .unwrap();
        writeln!(&mut actual, "ecology.roster-size {}", ecology.roster_size).unwrap();
        writeln!(&mut actual, "ecology.dominant {:016x}", ecology.dominant_id).unwrap();
        writeln!(
            &mut actual,
            "ecology.trophic {} {} {} {} {}",
            ecology.trophic_counts[0],
            ecology.trophic_counts[1],
            ecology.trophic_counts[2],
            ecology.trophic_counts[3],
            ecology.trophic_counts[4]
        )
        .unwrap();
        writeln!(
            &mut actual,
            "ecology.pressure {:08x} {:08x} {:08x}",
            ecology.herbivore.to_bits(),
            ecology.predator.to_bits(),
            ecology.diversity.to_bits()
        )
        .unwrap();
        writeln!(&mut actual, "organism.id {:016x}", organism.id).unwrap();
        writeln!(&mut actual, "organism.slot {}", source.slot).unwrap();
        writeln!(
            &mut actual,
            "organism.cell {} {}",
            source.cell.cx, source.cell.cy
        )
        .unwrap();
        writeln!(&mut actual, "organism.species {:016x}", organism.species).unwrap();
        writeln!(&mut actual, "organism.trophic {}", organism.trophic).unwrap();
        writeln!(
            &mut actual,
            "organism.world {:016x} {:016x}",
            organism.world.0.to_bits(),
            organism.world.1.to_bits()
        )
        .unwrap();
        writeln!(
            &mut actual,
            "organism.expressed {:08x} {:08x} {:08x} {:08x} {:08x}",
            organism.hue.to_bits(),
            organism.luminance.to_bits(),
            organism.size.to_bits(),
            organism.activity.to_bits(),
            organism.aggression.to_bits()
        )
        .unwrap();

        assert_eq!(
            actual.trim_end(),
            include_str!("../tests/fixtures/native_panel_source_characterization.txt").trim_end()
        );
    }

    /// A semantic Map/POV input trace over the pure seams used by the current
    /// winit adapter. Milestone 2 replays the same fixture through the shared
    /// mapper; this freezes held movement, diagonal normalization, one-shot
    /// repeat suppression, fractional wheels, and primary-held POV look.
    #[test]
    fn native_input_characterization() {
        let mut actual = String::from("native-input-characterization-v1\n");
        let mut keys = HashSet::new();
        let dt = 0.1f64;

        keys.insert(KeyCode::KeyW);
        let components_w = map_navigation_components(&keys);
        let axis_w = (
            components_w.0 / components_w.2,
            components_w.1 / components_w.2,
        );
        let delta_w = map_movement_delta(&keys, false, dt).expect("W is active");
        writeln!(
            &mut actual,
            "map held=KeyW axis={:016x},{:016x} delta={:016x},{:016x}",
            axis_w.0.to_bits(),
            axis_w.1.to_bits(),
            delta_w.0.to_bits(),
            delta_w.1.to_bits()
        )
        .unwrap();

        keys.insert(KeyCode::KeyD);
        let components_wd = map_navigation_components(&keys);
        let axis_wd = (
            components_wd.0 / components_wd.2,
            components_wd.1 / components_wd.2,
        );
        let delta_wd = map_movement_delta(&keys, false, dt).expect("W+D is active");
        writeln!(
            &mut actual,
            "map held=KeyD+KeyW axis={:016x},{:016x} delta={:016x},{:016x}",
            axis_wd.0.to_bits(),
            axis_wd.1.to_bits(),
            delta_wd.0.to_bits(),
            delta_wd.1.to_bits()
        )
        .unwrap();

        let mut channel = Channel::Composite;
        let mut shadow_ao = true;
        let first_action = dispatch_one_shot(false)
            .then(|| characterized_one_shot(ViewMode::Map, KeyCode::KeyV))
            .flatten();
        if let Some(action) = first_action {
            reduce_characterized_one_shot(action, &mut channel, &mut shadow_ao);
        }
        let repeat_action = dispatch_one_shot(true)
            .then(|| characterized_one_shot(ViewMode::Map, KeyCode::KeyV))
            .flatten();
        if let Some(action) = repeat_action {
            reduce_characterized_one_shot(action, &mut channel, &mut shadow_ao);
        }
        writeln!(
            &mut actual,
            "map one-shot=KeyV first={} repeat={} channel={}",
            first_action.is_some(),
            repeat_action.is_some(),
            channel.name()
        )
        .unwrap();

        let mut wheel = 0.0;
        let first_notches = accumulate_wheel(&mut wheel, 15.0 / WHEEL_PIXELS_PER_NOTCH);
        writeln!(
            &mut actual,
            "map wheel-pixel=15 notches={first_notches} remainder={:016x}",
            wheel.to_bits()
        )
        .unwrap();
        let second_notches = accumulate_wheel(&mut wheel, 30.0 / WHEEL_PIXELS_PER_NOTCH);
        writeln!(
            &mut actual,
            "map wheel-pixel=30 notches={second_notches} remainder={:016x}",
            wheel.to_bits()
        )
        .unwrap();
        let reverse_notches = accumulate_wheel(&mut wheel, -50.0 / WHEEL_PIXELS_PER_NOTCH);
        writeln!(
            &mut actual,
            "map wheel-pixel=-50 notches={reverse_notches} remainder={:016x}",
            wheel.to_bits()
        )
        .unwrap();

        let mut camera = PovCamera::new();
        let (forward, strafe, vertical) = pov_navigation_axis(&keys, false);
        let mut pov_delta =
            camera.forward() * forward + camera.right() * strafe + glam::DVec3::Z * vertical;
        pov_delta = pov_delta.normalize() * (camera.speed * dt);
        writeln!(
            &mut actual,
            "pov held=KeyD+KeyW axis={:016x},{:016x},{:016x} delta={:016x},{:016x},{:016x}",
            forward.to_bits(),
            strafe.to_bits(),
            vertical.to_bits(),
            pov_delta.x.to_bits(),
            pov_delta.y.to_bits(),
            pov_delta.z.to_bits()
        )
        .unwrap();

        let unheld = primary_drag_delta(None, (100.0, 100.0));
        writeln!(&mut actual, "pov move-unheld look={}", unheld.is_some()).unwrap();
        let mut drag_from = None;
        update_primary_drag_gate(
            ViewMode::Pov,
            MouseButton::Left,
            true,
            Some((100.0, 100.0)),
            &mut drag_from,
        );
        let drag = primary_drag_delta(drag_from, (112.0, 92.0)).expect("primary drag");
        camera.look(drag.0, drag.1);
        drag_from = Some((112.0, 92.0));
        assert_eq!(drag_from, Some((112.0, 92.0)));
        writeln!(
            &mut actual,
            "pov drag delta={:016x},{:016x} yaw={:08x} pitch={:08x}",
            drag.0.to_bits(),
            drag.1.to_bits(),
            camera.yaw.to_bits(),
            camera.pitch.to_bits()
        )
        .unwrap();
        update_primary_drag_gate(
            ViewMode::Pov,
            MouseButton::Left,
            false,
            Some((112.0, 92.0)),
            &mut drag_from,
        );
        let after_release = primary_drag_delta(drag_from, (140.0, 140.0));
        writeln!(
            &mut actual,
            "pov move-released look={} yaw={:08x} pitch={:08x}",
            after_release.is_some(),
            camera.yaw.to_bits(),
            camera.pitch.to_bits()
        )
        .unwrap();
        update_primary_drag_gate(
            ViewMode::Pov,
            MouseButton::Left,
            true,
            Some((140.0, 140.0)),
            &mut drag_from,
        );
        cancel_primary_drag(&mut drag_from); // CursorLeft / pointer cancellation.
        let after_cancel = primary_drag_delta(drag_from, (150.0, 150.0));
        writeln!(
            &mut actual,
            "pov move-cancelled look={}",
            after_cancel.is_some()
        )
        .unwrap();

        let pov_first = dispatch_one_shot(false)
            .then(|| characterized_one_shot(ViewMode::Pov, KeyCode::KeyB))
            .flatten();
        if let Some(action) = pov_first {
            reduce_characterized_one_shot(action, &mut channel, &mut shadow_ao);
        }
        let pov_repeat = dispatch_one_shot(true)
            .then(|| characterized_one_shot(ViewMode::Pov, KeyCode::KeyB))
            .flatten();
        if let Some(action) = pov_repeat {
            reduce_characterized_one_shot(action, &mut channel, &mut shadow_ao);
        }
        writeln!(
            &mut actual,
            "pov one-shot=KeyB first={} repeat={} shadow-ao={shadow_ao}",
            pov_first.is_some(),
            pov_repeat.is_some()
        )
        .unwrap();

        wheel = 0.0;
        let pov_first_wheel = accumulate_wheel(&mut wheel, 20.0 / WHEEL_PIXELS_PER_NOTCH);
        writeln!(
            &mut actual,
            "pov wheel-pixel=20 notches={pov_first_wheel} remainder={:016x} speed={:016x}",
            wheel.to_bits(),
            camera.speed.to_bits()
        )
        .unwrap();
        let pov_second_wheel = accumulate_wheel(&mut wheel, 25.0 / WHEEL_PIXELS_PER_NOTCH);
        for _ in 0..pov_second_wheel.max(0) {
            camera.scroll_speed(true);
        }
        writeln!(
            &mut actual,
            "pov wheel-pixel=25 notches={pov_second_wheel} remainder={:016x} speed={:016x}",
            wheel.to_bits(),
            camera.speed.to_bits()
        )
        .unwrap();

        assert_eq!(
            actual.trim_end(),
            include_str!("../tests/fixtures/native_input_characterization.txt").trim_end()
        );
    }
}
