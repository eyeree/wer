//! Native application shell for Infinite World Exploration.
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
//! - `Tab` — toggle Map/POV in a single view, or move focus between panes in
//!   Split. `WER_VIEW=map|pov|split` selects the startup presentation; in
//!   Split, clicking a pane also focuses it and the visible cyan border shows
//!   which pane owns keyboard/wheel input. Both panes follow one traveler and
//!   one post-update world state. POV is a fly camera over the meshed
//!   near-field terrain: hold the **left mouse button** and drag to look,
//!   `WASD` along view/strafe, `Space`/`LShift` up/down, and wheel adjusts the
//!   focused mode's speed. `F` toggles walk ↔ fly
//!   (3d-phase-2-plan.md): walk rides the rendered terrain at eye height
//!   (`Space`/`LShift` reserved, cliffs climb as fast ramps, the sea floor
//!   is walkable); toggling back to fly keeps the pose. Every map binding
//!   above is active when Map is focused. Legacy `WER_POV=1` still starts in
//!   POV when `WER_VIEW` is absent; `WER_POV_RADIUS` sets the chunk draw radius
//!   in regions (default 3).
//! - `F12` — write a debug dump into `./dump/<UTC datetime>/`: a screenshot
//!   of the active Map, POV, or Split surface (including the shared panel and
//!   focus border) plus `state.txt` with mode/focus/pane rectangles, both
//!   traveler and camera poses, hover, steering, telemetry, dependency hashes,
//!   and vault counters ([`dump`]).
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
//! Headless POV/Split screenshot mode (offscreen GPU, ADR 0021):
//! `wer --pov-script "<instructions>"` drives the POV camera through a
//! `;`-separated instruction sequence and captures snapshots — the
//! debugging/testing harness for POV rendering. Instructions:
//! `size:WxH` (capture size, before the first snap), `pos:x,y[,z]`,
//! `mouse:dx,dy` (simulated look drag, pixels), `move:f[,r[,u]]` (fly
//! forward/right/up in world units; in walk mode `f`/`r` move in the walk
//! basis, `u` is ignored, and the eye snaps to the ground at the
//! destination), `walk` / `fly` (toggle the 3D-2 walk mode, exactly like
//! the live `F` key), `settle[:n]` (world updates), `snap:file.ppm` (POV
//! only), and `split:file.ppm` (Map + POV + shared panel, Map-focused).
//! `WER_TIER=low|mid|high` selects the explicit capture tier; unset defaults
//! to Low so established headless output remains stable. The same selector is
//! honored by `--screenshot`.
//! Example:
//! `wer --pov-script "size:1024x768; pos:300,-10; mouse:-60,100; split:aligned.ppm"`

mod dump;
mod input;
mod panel;

use std::sync::Arc;
use std::time::{Duration, Instant};

use renderer::{
    FocusDecoration, InformationSurface, InformationUpload, MapFramePane, MapFrameSource,
    MultiViewFrame, PovFramePane, Renderer, SurfaceViewport,
};
use viewer_host::action::{
    PreserveMutation, ServiceRequestId, ViewerAction, ViewerEffect, WorkerBackend,
};
use viewer_host::atlas::{AtlasManager, RefinementRequest};
use viewer_host::controller::{
    CapturePreferences, GroundSample, LoadedSession, PovGroundSampler, PresentationDirty,
    ServiceNotification, ServiceResponse, ServiceResponseResult, ServiceResponseSequence,
    TickInput, TickOutput, ViewerController,
};
use viewer_host::input::InputFrame;
use viewer_host::input::{InputContext, InputMapper, NormalizedInputEvent};
use viewer_host::inspect::{HoverInfo, PovHoverCache};
use viewer_host::layout::{
    resolve_view_layout, MapViewportProjection, PixelRect, PresentationMode, ResolvedViewLayout,
    ViewKind, ViewLayout,
};
use viewer_host::map::{
    Channel, MapBackend, MapComposer, MapDecor, MapRenderRequest, Overlays, PreparedMapSource,
};
use viewer_host::panel::{
    build_panel_document, PanelBuildInput, PanelDocument, PanelDocumentCache, PanelDocumentKey,
    PerformanceInfo, PersistenceInfo, PlatformTelemetry, RendererInfo, Severity,
    StreamingSupplement, VaultInfo, ViewerWarning, WarningRegistry,
};
use viewer_host::world::{
    ExplorationWorld, NoopWorldTickHook, WorldPostUpdate, WorldPreUpdate, WorldServiceInput,
    WorldTickHook,
};
use viewer_host::ORGANISM_PICK_ZOOM as ORGANISM_INFO_ZOOM;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};
#[cfg(test)]
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, PossibilityDomain,
    PossibilitySignature,
};
use world_core::{PossibilityField, RegionCoord, LAYER_COUNT, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{
    compare_session_runtime, stream_config_from_record, AdapterClass, Budget, FrameStats,
    RegionMap, ResourceTier, SessionCompatibility, TierInputs, Vault, VaultStats,
};
#[cfg(test)]
use world_runtime::{RouteTracker, Storage, StreamConfig, VaultPersistenceError};

use panel::Hud;
use pov_host as pov;
use pov_host::{PovCamera, PovChunkManager, PovCounters, PovOrganismCounters, PovOrganismManager};
use tools::{FileStorage, LaneExecutor};

/// Letterbox color around the square map (linear RGBA).
const CLEAR_COLOR: [f64; 4] = [0.02, 0.02, 0.04, 1.0];

/// Resolve the native startup compatibility knobs without letting the legacy
/// boolean override the explicit three-mode selector.
fn initial_presentation(
    requested: Option<&str>,
    legacy_pov: bool,
) -> Result<Option<PresentationMode>, &str> {
    match requested {
        Some(id) => PresentationMode::parse(id).map(Some).ok_or(id),
        None if legacy_pov => Ok(Some(PresentationMode::Pov)),
        None => Ok(None),
    }
}

/// Build a fresh device/surface renderer for an existing native window.
/// Device-loss recovery uses the same path as initial resume so a successful
/// fallback can actually present the surviving Map world and panel.
fn build_window_renderer(window: &Arc<Window>) -> Result<Renderer, renderer::RendererError> {
    let size = window.inner_size();
    let surface_window = Arc::clone(window);
    pollster::block_on(Renderer::new(
        Box::new(move || surface_window.clone().into()),
        size.width,
        size.height,
    ))
}

/// A capability loss changes the focused input context from POV to Map before
/// the next tick. Clear held navigation at that boundary so a key/controller
/// axis pressed for POV cannot be reinterpreted as Map travel on the fallback
/// frame; queued one-shot actions retain their reducer order.
fn clear_renderer_loss_input(mapper: &mut InputMapper, adapter: &mut input::WinitInputAdapter) {
    mapper.clear_held_state();
    adapter.cancel_pointer_gesture();
}

/// Live native presentation geometry: one aspect-fitted view deck followed
/// by the bitmap information panel. A single view contributes one square
/// source column; Split contributes two, so each 50/50 pane remains square
/// instead of squeezing both panes into the legacy one-column map slot.
#[derive(Debug, Clone, Copy, PartialEq)]
struct NativeFrameRects {
    combined: PixelRect,
    view_deck: PixelRect,
    panel: PixelRect,
    views: ResolvedViewLayout,
}

impl NativeFrameRects {
    fn resolve(
        surface: PixelRect,
        map_side: u32,
        panel_width: u32,
        layout: ViewLayout,
    ) -> Option<Self> {
        let view_columns = if layout.mode == PresentationMode::Split {
            2
        } else {
            1
        };
        let view_source_width = map_side.checked_mul(view_columns)?;
        let source_width = view_source_width.checked_add(panel_width)?;
        let combined = surface.fitted_aspect(source_width, map_side)?;
        if combined.width < 2 || combined.height == 0 {
            return None;
        }
        // Split the already-fitted destination at one shared source-space
        // seam. Rounding the deck width and panel origin separately can
        // overlap or leave a one-pixel hole on odd surfaces.
        let deck_width = ((u64::from(combined.width) * u64::from(view_source_width)
            + u64::from(source_width) / 2)
            / u64::from(source_width)) as u32;
        let deck_width = deck_width.clamp(1, combined.width - 1);
        let view_deck = PixelRect::new(combined.x, combined.y, deck_width, combined.height);
        let panel = PixelRect::new(
            view_deck.right(),
            combined.y,
            combined.width - deck_width,
            combined.height,
        );
        let views = resolve_view_layout(view_deck, layout);
        if panel.width == 0
            || panel.height == 0
            || views
                .map_pane
                .is_some_and(|pane| pane.width == 0 || pane.height == 0)
            || views
                .pov_pane
                .is_some_and(|pane| pane.width == 0 || pane.height == 0)
        {
            return None;
        }
        Some(Self {
            combined,
            view_deck,
            panel,
            views,
        })
    }

    const fn panel_viewport(self) -> SurfaceViewport {
        SurfaceViewport::new(
            self.panel.x,
            self.panel.y,
            self.panel.width,
            self.panel.height,
        )
    }

    fn map_viewport(self) -> Option<SurfaceViewport> {
        self.views.map_content.map(surface_viewport)
    }

    fn pov_viewport(self) -> Option<SurfaceViewport> {
        self.views.pov_pane.map(surface_viewport)
    }

    fn focus_decoration(self) -> Option<FocusDecoration> {
        (self.views.mode == PresentationMode::Split)
            .then(|| self.views.focus_border(self.views.focused))
            .flatten()
            .map(|viewport| FocusDecoration {
                viewport: surface_viewport(viewport),
                thickness: 3,
            })
    }
}

const fn surface_viewport(rect: PixelRect) -> SurfaceViewport {
    SurfaceViewport::new(rect.x, rect.y, rect.width, rect.height)
}

#[cfg(test)]
mod native_map_rect_tests {
    use super::*;

    #[test]
    fn explicit_start_view_covers_all_modes_and_overrides_legacy_pov() {
        assert_eq!(initial_presentation(None, false), Ok(None));
        assert_eq!(
            initial_presentation(None, true),
            Ok(Some(PresentationMode::Pov))
        );
        for mode in [
            PresentationMode::Map,
            PresentationMode::Pov,
            PresentationMode::Split,
        ] {
            assert_eq!(
                initial_presentation(Some(mode.as_str()), true),
                Ok(Some(mode))
            );
        }
        assert_eq!(initial_presentation(Some("POV"), true), Err("POV"));
    }

    #[test]
    fn headless_tier_defaults_low_and_accepts_only_named_presets() {
        assert_eq!(parse_headless_tier(None), Ok(ResourceTier::Low));
        assert_eq!(parse_headless_tier(Some("LOW")), Ok(ResourceTier::Low));
        assert_eq!(parse_headless_tier(Some("mid")), Ok(ResourceTier::Mid));
        assert_eq!(parse_headless_tier(Some("High")), Ok(ResourceTier::High));
        let error = parse_headless_tier(Some("auto")).expect_err("headless tier is explicit");
        assert!(error.contains("expected low, mid, or high"));
    }

    #[test]
    fn live_view_hit_routing_uses_exact_panes_and_ignores_panel() {
        let surface = PixelRect::new(0, 0, 1280, 720);
        let rects = NativeFrameRects::resolve(
            surface,
            600,
            300,
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.5,
            },
        )
        .unwrap();
        let map = rects.views.map_pane.unwrap();
        let pov = rects.views.pov_pane.unwrap();
        assert_eq!(
            rects
                .views
                .hit_view([f64::from(map.x) + 0.5, f64::from(map.y) + 0.5]),
            Some(ViewKind::Map)
        );
        assert_eq!(
            rects
                .views
                .hit_view([f64::from(pov.x) + 0.5, f64::from(pov.y) + 0.5]),
            Some(ViewKind::Pov)
        );
        assert_eq!(
            rects.views.hit_view([
                f64::from(rects.panel.x) + 0.5,
                f64::from(rects.panel.y) + 0.5,
            ]),
            None
        );
    }

    #[test]
    fn live_modes_fit_one_deck_and_split_keeps_two_square_columns() {
        for (width, height) in [(1280, 720), (901, 701), (509, 701), (127, 511)] {
            let surface = PixelRect::new(0, 0, width, height);
            for mode in [
                PresentationMode::Map,
                PresentationMode::Pov,
                PresentationMode::Split,
            ] {
                let focused = if mode == PresentationMode::Pov {
                    ViewKind::Pov
                } else {
                    ViewKind::Map
                };
                let rects = NativeFrameRects::resolve(
                    surface,
                    600,
                    300,
                    ViewLayout {
                        mode,
                        focused,
                        split_ratio: 0.5,
                    },
                )
                .expect("visible deck and panel");
                assert!(surface.contains_rect(rects.combined));
                assert!(rects.combined.contains_rect(rects.view_deck));
                assert!(rects.combined.contains_rect(rects.panel));
                assert_eq!(rects.view_deck.right(), rects.panel.x);
                assert_eq!(rects.panel.right(), rects.combined.right());
                assert!(!rects.view_deck.overlaps(rects.panel));
                for pane in [rects.views.map_pane, rects.views.pov_pane]
                    .into_iter()
                    .flatten()
                {
                    assert!(rects.view_deck.contains_rect(pane));
                    assert!(pane.width.abs_diff(pane.height) <= 1);
                }
                if mode == PresentationMode::Split {
                    let map = rects.views.map_pane.unwrap();
                    let pov = rects.views.pov_pane.unwrap();
                    assert_eq!(map.right(), pov.x);
                    assert!(!map.overlaps(pov));
                    assert!(rects.focus_decoration().is_some());
                } else {
                    assert!(rects.focus_decoration().is_none());
                }
            }
        }
    }

    #[test]
    fn split_frame_geometry_plans_one_surface_lifecycle() {
        let rects = NativeFrameRects::resolve(
            PixelRect::new(0, 0, 1280, 720),
            600,
            300,
            ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Pov,
                split_ratio: 0.5,
            },
        )
        .unwrap();
        let plan = renderer::FramePassPlan::new(renderer::FramePlanRequest {
            surface: (1280, 720),
            map: rects.map_viewport(),
            pov: rects.pov_viewport(),
            map_information: Some(rects.panel_viewport()),
            pov_information: None,
            pov_shadows: true,
            focus: rects.focus_decoration().map(|focus| focus.viewport),
        })
        .expect("native Split rectangles form one valid renderer frame");
        assert_eq!(
            plan.successful_surface_lifecycle(),
            renderer::FrameLifecycleCounters {
                acquire_attempts: 1,
                acquired: 1,
                surface_clears: 1,
                submissions: 1,
                presents: 1,
            }
        );
    }

    #[test]
    fn tiny_surfaces_without_both_visible_source_panes_are_skipped() {
        for surface in [
            PixelRect::new(0, 0, 0, 0),
            PixelRect::new(0, 0, 1, 1),
            PixelRect::new(0, 0, 1, 2),
        ] {
            assert_eq!(
                NativeFrameRects::resolve(
                    surface,
                    600,
                    300,
                    ViewLayout {
                        mode: PresentationMode::Split,
                        focused: ViewKind::Map,
                        split_ratio: 0.5,
                    },
                ),
                None
            );
        }
    }

    #[test]
    fn pov_panel_upload_retries_after_failed_present_then_retains() {
        let mut uploaded_revision = None;
        let revision = 0x1234;
        let dimensions = (840, 800);

        assert_eq!(
            commit_pov_panel_upload(&mut uploaded_revision, revision, dimensions, false),
            0
        );
        assert_eq!(uploaded_revision, None, "failed present must leave a retry");
        assert_eq!(
            commit_pov_panel_upload(&mut uploaded_revision, revision, dimensions, true),
            u64::from(dimensions.0) * u64::from(dimensions.1) * 4
        );
        assert_eq!(uploaded_revision, Some(revision));
        assert_eq!(
            commit_pov_panel_upload(&mut uploaded_revision, revision, dimensions, true),
            0,
            "unchanged panel retains its texture"
        );
    }

    #[test]
    fn pov_camera_center_and_aspect_come_from_the_uncovered_pane() {
        for (width, height) in [(1280, 720), (901, 701), (509, 701)] {
            let rects = NativeFrameRects::resolve(
                PixelRect::new(0, 0, width, height),
                800,
                panel::PANEL_WIDTH as u32,
                ViewLayout {
                    mode: PresentationMode::Pov,
                    focused: ViewKind::Pov,
                    split_ratio: 0.5,
                },
            )
            .expect("visible POV and panel panes");
            let pov = rects.views.pov_pane.unwrap();
            let center = (pov.x + pov.width / 2, pov.y + pov.height / 2);
            assert!(pov.contains(center.0, center.1));
            assert!(!rects.panel.contains(center.0, center.1));
            assert_eq!(
                rects.views.pov_aspect,
                Some(pov.width as f32 / pov.height as f32),
            );
            assert_eq!(
                pov.width as f32 / pov.height as f32,
                1.0,
                "POV projection uses the square physical pane"
            );
        }
    }
}

#[cfg(test)]
const PLAYER_SPEED: f64 = viewer_host::world::MAP_MOVEMENT_SPEED;

/// Native persistence bridge around the platform-neutral single world tick.
///
/// The shared controller owns exploration, recorder, and traversal state. This
/// service retains only the concrete file-backed vault and contributes route
/// records/derived anchors through [`WorldTickHook`].
struct NativeWorldServices {
    vault: Option<Vault<FileStorage>>,
    vault_stats: VaultStats,
    vault_failure_logged: bool,
    response_sequence: u64,
    flush_budget: Budget,
}

impl NativeWorldServices {
    fn open() -> Self {
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
        Self {
            vault,
            vault_stats: VaultStats::default(),
            vault_failure_logged: false,
            response_sequence: 0,
            flush_budget: Budget::default(),
        }
    }

    fn apply_preserves(&self, world: &mut ExplorationWorld) {
        let Some(vault) = self.vault.as_ref() else {
            return;
        };
        let contributions = vault
            .preserves()
            .iter()
            .flat_map(|(&id, preserve)| {
                preserve
                    .regions
                    .iter()
                    .map(move |&(coord, signature)| (id, coord, signature))
            })
            .collect();
        world.apply_preserve_contributions(contributions);
    }

    fn response(
        &mut self,
        request_id: ServiceRequestId,
        result: ServiceResponseResult,
    ) -> ServiceResponse {
        self.response_sequence = self.response_sequence.saturating_add(1);
        ServiceResponse {
            sequence: ServiceResponseSequence(self.response_sequence),
            request_id,
            result,
        }
    }

    fn pov_availability(&mut self, supported: bool) -> ServiceNotification {
        self.pov_availability_with_reason(supported, None)
    }

    fn pov_availability_with_reason(
        &mut self,
        supported: bool,
        reason: Option<ViewerWarning>,
    ) -> ServiceNotification {
        self.response_sequence = self.response_sequence.saturating_add(1);
        ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(self.response_sequence),
            supported,
            reason,
        }
    }
}

impl WorldTickHook for NativeWorldServices {
    fn before_world_update(&mut self, input: WorldPreUpdate<'_>) -> WorldServiceInput {
        self.flush_budget = *input.budget;
        let Some(vault) = self.vault.as_ref() else {
            return WorldServiceInput::default();
        };
        let derived_anchors = if input.path_tracking && input.route_attraction {
            world_core::attraction_anchors(
                vault.routes().values(),
                input.traveler,
                input.budget.max_route_attraction_nodes,
            )
        } else {
            Vec::new()
        };
        let active_routes = if input.path_tracking {
            vault.routes().values().cloned().collect()
        } else {
            Vec::new()
        };
        WorldServiceInput {
            derived_anchors,
            active_routes,
        }
    }

    fn after_world_update(&mut self, output: WorldPostUpdate<'_>) {
        let Some(vault) = self.vault.as_mut() else {
            return;
        };
        let flush_start = Instant::now();
        vault.mark_seen(RegionCoord::from_world(
            output.traveler.0,
            output.traveler.1,
        ));
        for &id in output.traversed_route_ids {
            vault.bump_route_usage(id);
            log::info!("route {id:#018x} traversed (usage bumped)");
        }
        match vault.flush(&self.flush_budget) {
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
        output.stats.pass_ms[world_runtime::Pass::Flush.index()] +=
            flush_start.elapsed().as_secs_f32() * 1000.0;
    }
}

struct NativeGroundSampler<'a> {
    chunks: &'a PovChunkManager,
}

impl PovGroundSampler for NativeGroundSampler<'_> {
    fn sample_ground(&self, map: &RegionMap, position: (f64, f64)) -> GroundSample {
        let (height, mesh_resident) = pov::walk_ground(self.chunks, map, position);
        GroundSample {
            height,
            mesh_resident,
        }
    }
}

/// Native invalidation and warning state around the shared panel cache.
/// Wall-clock cadence remains a shell concern; model construction and all
/// formatting remain in `viewer-host`.
#[derive(Debug)]
struct NativePanelState {
    cache: PanelDocumentCache,
    warnings: WarningRegistry,
    state_revision: u64,
    hover_revision: u64,
    telemetry_revision: u64,
    platform_revision: u64,
    document_revision: u64,
    last_state_frame: Option<u64>,
    last_hover: HoverInfo,
    last_renderer: RendererInfo,
    last_persistence: PersistenceInfo,
    last_streaming: StreamingSupplement,
    last_warning_revision: u64,
}

impl Default for NativePanelState {
    fn default() -> Self {
        Self {
            cache: PanelDocumentCache::default(),
            warnings: WarningRegistry::default(),
            state_revision: 0,
            hover_revision: 0,
            telemetry_revision: 0,
            platform_revision: 0,
            document_revision: 0,
            last_state_frame: None,
            last_hover: HoverInfo::None,
            last_renderer: RendererInfo::default(),
            last_persistence: PersistenceInfo::default(),
            last_streaming: StreamingSupplement::default(),
            last_warning_revision: 0,
        }
    }
}

impl NativePanelState {
    fn retain_warning(&mut self, warning: ViewerWarning) {
        self.warnings.upsert(warning);
    }

    fn telemetry_rolled(&mut self) {
        self.telemetry_revision = self.telemetry_revision.saturating_add(1);
    }

    fn renderer_for_pov(
        &self,
        requested: MapBackend,
        surface_format: Option<String>,
        surface_losses: u32,
    ) -> RendererInfo {
        RendererInfo {
            requested_map_backend: requested,
            surface_format,
            surface_losses,
            ..self.last_renderer.clone()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn document(
        &mut self,
        tick: &TickOutput,
        world: &ExplorationWorld,
        hover: HoverInfo,
        performance: PerformanceInfo,
        streaming: StreamingSupplement,
        persistence: PersistenceInfo,
        renderer: RendererInfo,
        capture: CapturePreferences,
        split_ratio: f32,
    ) -> PanelDocument {
        let view_changed = self.cache.document().is_some_and(|document| {
            let view = &document.model.view;
            document.model.frame.traveler != tick.traveler
                || view.mode != tick.mode
                || view.focused != tick.focused
                || view.map_channel != tick.map.channel
                || view.map_zoom != tick.map.zoom
                || view.map_overlays != tick.map.overlays
                || view.map_refinement != tick.map.refinement
                || view.camera != tick.pov.into()
                || view.split_ratio != split_ratio
        });
        if (tick.dirty.panel || view_changed) && self.last_state_frame != Some(tick.frame) {
            self.state_revision = self.state_revision.saturating_add(1);
            self.last_state_frame = Some(tick.frame);
        }
        if hover != self.last_hover {
            self.last_hover = hover.clone();
            self.hover_revision = self.hover_revision.saturating_add(1);
        }
        if renderer != self.last_renderer
            || persistence != self.last_persistence
            || streaming != self.last_streaming
        {
            self.last_renderer = renderer.clone();
            self.last_persistence = persistence.clone();
            self.last_streaming = streaming;
            self.platform_revision = self.platform_revision.saturating_add(1);
        }
        let warning_revision = self.warnings.revision();
        if warning_revision != self.last_warning_revision {
            self.last_warning_revision = warning_revision;
            self.platform_revision = self.platform_revision.saturating_add(1);
        }

        let key = PanelDocumentKey {
            state: self.state_revision,
            hover: self.hover_revision,
            telemetry: self.telemetry_revision,
            platform: self.platform_revision,
        };
        let next_revision = self.document_revision.saturating_add(1);
        let warnings = self.warnings.warnings();
        let (document, built) = self.cache.get_or_build(key, || {
            build_panel_document(PanelBuildInput {
                tick,
                world,
                hover,
                performance,
                streaming,
                persistence,
                renderer,
                capture,
                warnings,
                split_ratio,
                revision: next_revision,
            })
        });
        let document = document.clone();
        if built {
            self.document_revision = next_revision;
        }
        document
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    controller: ViewerController,
    services: NativeWorldServices,
    /// The concrete native scheduler remains outside the shared viewer host.
    executor: Box<dyn world_runtime::TaskExecutor>,
    composer: MapComposer,
    hud: Hud,
    /// One semantic document cache shared by Map, POV, screenshots, and dumps.
    panel_state: NativePanelState,
    /// Latest controller output, retained so an F12 effect in the same tick and
    /// windowless dump tests use the canonical model builder.
    last_tick_output: Option<TickOutput>,
    /// The detected (or overridden) resource tier (phase-6-plan.md §6.7).
    tier: ResourceTier,
    /// Atlas slot assignment + delta-upload keys for the GPU map.
    atlas: AtlasManager,
    /// POV chunk lifecycle: keying, Background-lane meshing, amortized
    /// upload, farthest-first eviction (3d-phase-1-plan.md §7).
    pov_chunks: PovChunkManager,
    /// Exact upload-only presentation of the currently published organisms.
    /// It scans only in POV and retains renderer replacement lists at rest.
    pov_organisms: PovOrganismManager,
    /// Cached CPU-side hit against the exact resident POV geometry.
    pov_hover: PovHoverCache,
    /// Chunk draw radius in regions (`WER_POV_RADIUS`, default 3).
    pov_radius: i32,
    /// Canonical binding, held-state, wheel, and primary-drag authority.
    input: InputMapper,
    /// Thin winit spelling/coordinate adapter; its cursor is also the
    /// presentation-only map-hover position.
    winit_input: input::WinitInputAdapter,
    /// The previous telemetry second's POV counters, for the delta log line.
    pov_counters_last: PovCounters,
    /// Previous organism lifecycle totals for the POV telemetry delta line.
    pov_organism_counters_last: PovOrganismCounters,
    /// Content hashes of the last uploaded overlay strips, so an unchanged
    /// strip uploads nothing (steady-state upload ≈ 0, §6.5).
    overlay_hashes: [u64; 2],
    /// Shared-document revision last uploaded by the GPU-map panel pass.
    panel_revision: Option<u64>,
    /// Shared-document revision last uploaded for POV; `None` forces the
    /// initial upload while unchanged frames retain the renderer texture.
    pov_panel_revision: Option<u64>,
    /// Map hover in physical surface pixels, sampled from shared frame input.
    cursor_pos: Option<(f64, f64)>,
    /// Persistent POV hover in physical surface pixels. The shared input mapper
    /// owns pointer routing; this shell retains its sampled value so geometry
    /// changes can refresh hover even when the mouse itself did not move.
    pov_pointer: Option<[f64; 2]>,
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
    /// Device losses survive dropping the failed renderer so the shared
    /// information panel keeps reporting the reason for Map fallback.
    device_losses: u32,
    /// A replacement device is ready, but capability restoration is queued
    /// only after the loss tick has irrevocably produced Map/Map output.
    pov_recovery_ready: bool,
    /// Device recreation can fail transiently. Keep retrying at a bounded
    /// cadence while the world continues ticking in unsupported Map mode.
    renderer_recovery_pending: bool,
    renderer_retry_at: Option<Instant>,
    /// Keep the device-loss warning visible for the fallback/recovery frame,
    /// then remove its stale "returned to Map" claim. The cumulative device
    /// loss field remains in renderer diagnostics.
    clear_recovered_renderer_warning: bool,
}

impl App {
    fn new(inline: bool, tier: ResourceTier) -> Self {
        // Native help remains descriptor-validated outside the bitmap panel;
        // the M6 panel renderer no longer embeds or formats control rows.
        debug_assert!(input::NATIVE_HELP_ROWS.iter().all(|row| {
            !row.keys.is_empty() && !row.action.is_empty() && !row.binding_ids.is_empty()
        }));
        let mut cfg = tier.stream_config();
        if let Some(mb) = std::env::var("WER_CACHE_MB")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
        {
            cfg.max_field_cache_bytes = mb * 1024 * 1024;
        }
        let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
        let composer = MapComposer::new(half_regions, cfg.field_resolution);
        let hud = Hud::new(composer.side() as usize);
        let services = NativeWorldServices::open();
        let mut world = ExplorationWorld::with_runtime(cfg, tier.budget(), tier);
        if let Ok(value) = std::env::var("WER_START") {
            if let Some((x, y)) = value.split_once(',') {
                if let (Ok(x), Ok(y)) = (x.trim().parse::<f64>(), y.trim().parse::<f64>()) {
                    world.restore_traveler((x, y), (x, y));
                }
            }
        }
        services.apply_preserves(&mut world);
        let mut controller = ViewerController::new(world);
        if std::env::var_os("WER_CPU_MAP").is_some() {
            controller.enqueue_action(ViewerAction::SetMapBackend(MapBackend::Cpu));
        }
        let requested_view = std::env::var("WER_VIEW").ok();
        let legacy_pov = std::env::var_os("WER_POV").is_some_and(|value| value != "0");
        match initial_presentation(requested_view.as_deref(), legacy_pov) {
            Ok(Some(mode)) => {
                controller.enqueue_action(ViewerAction::SetPresentation(mode));
            }
            Ok(None) => {}
            Err(id) => {
                log::warn!("ignoring invalid WER_VIEW={id:?}; expected map, pov, or split");
                if legacy_pov {
                    controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
                }
            }
        }
        controller.set_pov_render_scale(
            std::env::var("WER_POV_SCALE")
                .ok()
                .and_then(|value| value.parse::<f32>().ok())
                .map_or(1.0, |scale| scale.clamp(0.25, 1.0)),
        );
        Self {
            window: None,
            renderer: None,
            controller,
            services,
            executor: if inline {
                Box::new(world_runtime::InlineExecutor)
            } else {
                Box::new(LaneExecutor::auto())
            },
            tier,
            composer,
            hud,
            panel_state: NativePanelState::default(),
            last_tick_output: None,
            atlas: AtlasManager::default(),
            pov_chunks: PovChunkManager::new(),
            pov_organisms: PovOrganismManager::new(),
            pov_hover: PovHoverCache::new(),
            // The llvmpipe escape hatch (3d-phase-1-plan.md §8.5).
            pov_radius: std::env::var("WER_POV_RADIUS")
                .ok()
                .and_then(|v| v.parse::<i32>().ok())
                .map_or(3, |r| r.clamp(1, 8)),
            input: InputMapper::default(),
            winit_input: input::WinitInputAdapter::default(),
            pov_counters_last: PovCounters::default(),
            pov_organism_counters_last: PovOrganismCounters::default(),
            overlay_hashes: [0; 2],
            panel_revision: None,
            pov_panel_revision: None,
            cursor_pos: None,
            pov_pointer: None,
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
            device_losses: 0,
            pov_recovery_ready: false,
            renderer_recovery_pending: false,
            renderer_retry_at: None,
            clear_recovered_renderer_warning: false,
        }
    }

    fn input_context(&self) -> InputContext {
        self.controller
            .input_context(self.winit_input.surface_focused())
    }

    /// One ordered consumer for keyboard, pointer, wheel, and future native
    /// controls. Events enqueue through the shared mapper; only typed actions
    /// reach the shared controller.
    fn handle_input_event(&mut self, event: NormalizedInputEvent, _event_loop: &ActiveEventLoop) {
        let context = self.input_context();
        self.input.handle_event(event, context);
        self.enqueue_actions();
    }

    fn enqueue_actions(&mut self) {
        for action in self.input.drain_actions() {
            self.controller.enqueue_action(action);
        }
    }

    /// The ground under the camera (3d-phase-2-plan.md §4.4): the rendered
    /// mesh where the chunk is resident, the analytic fallback at the
    /// loading frontier. The bool is the mesh-vs-analytic telemetry tag.
    fn pov_ground(&self) -> (f64, bool) {
        pov::walk_ground(
            &self.pov_chunks,
            self.controller.world().map(),
            (
                self.controller.pov_camera().pos.x,
                self.controller.pov_camera().pos.y,
            ),
        )
    }

    fn handle_effects(&mut self, effects: Vec<ViewerEffect>, event_loop: &ActiveEventLoop) {
        for effect in effects {
            match effect {
                ViewerEffect::Exit => event_loop.exit(),
                ViewerEffect::WriteDebugCapture(request) => {
                    let result = self.debug_dump().map_or_else(
                        |error| {
                            self.service_failure(
                                "debug-capture-failed",
                                format!("Debug capture failed: {error}"),
                            )
                        },
                        |_| ServiceResponseResult::Completed,
                    );
                    self.enqueue_service_result(request.request_id, result);
                }
                ViewerEffect::PersistSession(request) => {
                    let request = *request;
                    let request_id = request.request_id;
                    let result = self.persist_session(request.snapshot);
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::LoadSession(request_id) => {
                    let result = self.load_session_result();
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::WriteDiscovery(request) => {
                    let result = self.write_discovery(&request);
                    self.enqueue_service_result(request.request_id, result);
                }
                ViewerEffect::LoadDiscoveries(request_id) => {
                    let anchors = self.services.vault.as_ref().map(|vault| {
                        vault
                            .discoveries()
                            .values()
                            .map(world_core::DiscoveryRecord::to_anchor)
                            .filter(|anchor| !self.controller.world().anchors().contains(anchor))
                            .collect()
                    });
                    let result = anchors.map_or_else(
                        || self.service_failure("vault-unavailable", "No native vault is open."),
                        ServiceResponseResult::DiscoveriesLoaded,
                    );
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::MutatePreserve(request) => {
                    let result = self.mutate_preserve(request.mutation);
                    self.enqueue_service_result(request.request_id, result);
                }
                ViewerEffect::WriteRoute(request) => {
                    let result = self.write_route(&request.nodes, &request.discoveries);
                    self.enqueue_service_result(request.request_id, result);
                }
                ViewerEffect::ClearRoutes(request_id) => {
                    let result = self.clear_routes_result();
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::ConfigurePathTracking {
                    request_id,
                    enabled,
                } => {
                    log::info!("path tracking {}", if enabled { "on" } else { "off" });
                    self.enqueue_service_result(request_id, ServiceResponseResult::Completed);
                }
                ViewerEffect::OpenAtlasImport(request_id) => {
                    let result = self.service_failure(
                        "native-atlas-import",
                        "Use wer-atlas for native atlas imports.",
                    );
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::DownloadAtlasBundle(request_id) => {
                    let result = self.service_failure(
                        "native-atlas-export",
                        "Use wer-atlas for native atlas exports.",
                    );
                    self.enqueue_service_result(request_id, result);
                }
                ViewerEffect::ConfigureWorkerBackend(_) => {
                    log::warn!("worker backend changes require a native restart");
                }
                ViewerEffect::CancelSupersededJobs => {
                    log::warn!("native task cancellation is scheduler-owned");
                }
                ViewerEffect::ConfigureStorage { enabled } => {
                    if enabled != self.services.vault.is_some() {
                        log::warn!("native vault availability is fixed when the process starts");
                    }
                }
                ViewerEffect::ResetLocalVault => {
                    log::warn!("native vault reset is not exposed as an in-view operation");
                }
                ViewerEffect::SelectMapBackend(backend) => {
                    log::info!(
                        "map compose: {}",
                        if backend == MapBackend::GpuAtlas {
                            "GPU"
                        } else {
                            "CPU"
                        }
                    );
                }
                ViewerEffect::RunTierBenchmark => {
                    log::warn!("the native shell has no live tier-benchmark service");
                }
                ViewerEffect::ConfigureResourceTier(tier) => {
                    if tier != self.tier {
                        log::warn!(
                            "resource tier changes require a native restart (active: {}, requested: {})",
                            self.tier.name(),
                            tier.name()
                        );
                    }
                }
                ViewerEffect::ReportWarning(warning) => {
                    match warning.severity {
                        Severity::Info => log::info!("{}: {}", warning.id, warning.message),
                        Severity::Warning => log::warn!("{}: {}", warning.id, warning.message),
                        Severity::Error => log::error!("{}: {}", warning.id, warning.message),
                    }
                    self.panel_state.retain_warning(warning);
                }
            }
        }
    }

    fn enqueue_service_result(
        &mut self,
        request_id: ServiceRequestId,
        result: ServiceResponseResult,
    ) {
        let response = self.services.response(request_id, result);
        self.controller.enqueue_service_response(response);
    }

    fn service_failure(
        &self,
        id: &'static str,
        message: impl Into<String>,
    ) -> ServiceResponseResult {
        ServiceResponseResult::Failed(ViewerWarning {
            id,
            message: message.into(),
            severity: Severity::Warning,
        })
    }

    fn persist_session(
        &mut self,
        snapshot: world_runtime::SessionSnapshotOwnedInput,
    ) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_mut() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        let region_count = snapshot.regions.len();
        let anchor_count = snapshot.anchors.len();
        if let Err(error) = vault.snapshot_session_owned(snapshot) {
            return self.service_failure(
                "session-save-rejected",
                format!("Session save was rejected: {error}"),
            );
        }
        match vault.flush_all() {
            Ok(stats) => {
                self.services.vault_stats = stats;
                self.services.vault_failure_logged = false;
                log::info!(
                    "session saved: {} records, {} bytes ({} regions, {} anchors)",
                    stats.flushed,
                    stats.bytes,
                    region_count,
                    anchor_count
                );
                ServiceResponseResult::Completed
            }
            Err(error) => {
                self.services.vault_stats = error.progress();
                self.services.vault_failure_logged = true;
                self.service_failure(
                    "session-save-failed",
                    format!("Session save failed; dirty records remain retryable: {error}"),
                )
            }
        }
    }

    fn load_session_result(&self) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_ref() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        let Some(snapshot) = vault.session().cloned() else {
            return self
                .service_failure("session-missing", "No saved session exists in the vault.");
        };
        let world = self.controller.world();
        let compatibility = compare_session_runtime(
            &snapshot.runtime,
            world.map().config(),
            world.budget(),
            Some(world.tier()),
            world.path_tracking(),
            world.route_attraction(),
        );
        let stream_config = if compatibility == SessionCompatibility::Exact {
            match stream_config_from_record(&snapshot.runtime.stream) {
                Ok(config) => Some(config),
                Err(error) => {
                    log::warn!(
                        "session stream config is not representable on this platform ({error:?}); using current config"
                    );
                    None
                }
            }
        } else {
            None
        };
        let preserve_contributions = vault
            .preserves()
            .iter()
            .flat_map(|(&id, preserve)| {
                preserve
                    .regions
                    .iter()
                    .map(move |&(coord, signature)| (id, coord, signature))
            })
            .collect();
        ServiceResponseResult::SessionLoaded(LoadedSession {
            snapshot: Box::new(snapshot),
            stream_config,
            restore_route_state: compatibility != SessionCompatibility::Incompatible,
            preserve_contributions,
        })
    }

    fn write_discovery(
        &mut self,
        request: &viewer_host::action::DiscoveryWriteRequest,
    ) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_mut() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        let name = format!("discovery-{}", vault.discoveries().len() + 1);
        match vault.record_discovery(&request.anchor, request.signature_seed, name.clone()) {
            Ok(id) => {
                log::info!("recorded {name} ({id:#018x}) into the vault");
                ServiceResponseResult::DiscoveryWritten { id }
            }
            Err(error) => self.service_failure(
                "discovery-write-failed",
                format!("Discovery was not recorded: {error}"),
            ),
        }
    }

    fn mutate_preserve(&mut self, mutation: PreserveMutation) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_mut() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        match mutation {
            PreserveMutation::Create { regions } => {
                let name = format!("preserve-{}", vault.preserves().len() + 1);
                match vault.record_preserve(regions.clone(), name.clone()) {
                    Ok(id) => {
                        log::info!(
                            "created {name} ({id:#018x}): {} regions pinned",
                            regions.len()
                        );
                        ServiceResponseResult::PreserveCreated { id, regions }
                    }
                    Err(error) => self.service_failure(
                        "preserve-create-failed",
                        format!("Preserve was not created: {error}"),
                    ),
                }
            }
            PreserveMutation::Remove { id } => {
                let Some(record) = vault.preserves().get(&id).cloned() else {
                    return self.service_failure(
                        "preserve-owner-missing",
                        format!("Runtime preserve owner {id:#018x} is absent from the vault."),
                    );
                };
                match vault.remove_preserve(id) {
                    Ok(true) => {
                        let regions = record.regions.iter().map(|(coord, _)| *coord).collect();
                        log::info!("deleted preserve {} ({id:#018x})", record.name);
                        ServiceResponseResult::PreserveRemoved { id, regions }
                    }
                    Ok(false) => self.service_failure(
                        "preserve-owner-missing",
                        format!("Preserve {id:#018x} disappeared before removal."),
                    ),
                    Err(error) => self.service_failure(
                        "preserve-remove-failed",
                        format!("Preserve deletion failed; runtime state retained: {error}"),
                    ),
                }
            }
        }
    }

    fn write_route(
        &mut self,
        nodes: &[world_core::RouteNode],
        discoveries: &[u64],
    ) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_mut() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        let name = format!("route-{}", vault.routes().len() + 1);
        let difficulty = world_core::route_difficulty(nodes);
        match vault.record_route(nodes.to_vec(), discoveries.to_vec(), name.clone()) {
            Ok(id) => {
                log::info!(
                    "recorded {name} ({id:#018x}): {} nodes, difficulty {difficulty:.2}",
                    nodes.len()
                );
                ServiceResponseResult::Completed
            }
            Err(error) => self.service_failure(
                "route-write-failed",
                format!("Route was discarded: {error}"),
            ),
        }
    }

    fn clear_routes_result(&mut self) -> ServiceResponseResult {
        let Some(vault) = self.services.vault.as_mut() else {
            return self.service_failure("vault-unavailable", "No native vault is open.");
        };
        let ids: Vec<_> = vault.routes().keys().copied().collect();
        let total = ids.len();
        let mut removed = 0;
        let mut failure = None;
        for id in ids {
            match vault.remove_route(id) {
                Ok(true) => removed += 1,
                Ok(false) => unreachable!("route id came from the vault"),
                Err(error) => {
                    failure = Some(error.to_string());
                    break;
                }
            }
        }
        let remaining_route_ids = vault.routes().keys().copied().collect();
        let warning = failure.map(|error| ViewerWarning {
            id: "route-clear-partial",
            message: format!(
                "Route clear stopped after {removed}/{total} durable removal(s): {error}"
            ),
            severity: Severity::Warning,
        });
        ServiceResponseResult::RoutesCleared {
            remaining_route_ids,
            warning,
        }
    }

    fn persistence_info(&self) -> PersistenceInfo {
        let vault = self.services.vault.as_ref().map(|v| VaultInfo {
            records: v.discoveries().len() + v.routes().len() + v.preserves().len(),
            dirty: v.dirty_records(),
            seen: v.seen_count(),
            issues: v.issue_count(),
            suppressed_issues: v.suppressed_issue_count(),
            persistence_retries: v
                .active_persistence_issue()
                .map_or(0, world_runtime::VaultIssue::occurrences),
        });
        PersistenceInfo {
            mode: if vault.is_some() {
                String::from("filesystem")
            } else {
                String::from("unavailable")
            },
            available: vault.is_some(),
            vault,
            pending_writes: 0,
            failures: 0,
            path_tracking: false,
            route_recording: false,
            route_attraction: false,
        }
    }

    fn platform_telemetry(&self) -> PlatformTelemetry {
        PlatformTelemetry {
            present_ms: self.render_ms,
            dom_updates: 0,
            surface_format: self
                .renderer
                .as_ref()
                .map(renderer::Renderer::surface_format_name),
            executor_backend: if self.executor.parallelism() == 1 {
                WorkerBackend::Inline
            } else {
                WorkerBackend::Workers
            },
            workers: self.executor.parallelism(),
            storage_available: self.services.vault.is_some(),
        }
    }

    fn panel_performance(&self) -> PerformanceInfo {
        PerformanceInfo {
            fps: self.fps,
            update_ms: self.update_ms,
            compose_ms: self.compose_ms,
            present_ms: self.render_ms,
            upload_kib_per_frame: self.upload_kb,
            pass_ms: self.pass_ms,
            dom_updates: 0,
        }
    }

    fn streaming_supplement(&self, pinned_violations: u64) -> StreamingSupplement {
        let map = self.controller.world().map();
        StreamingSupplement {
            regen_totals: self.regen_totals,
            macro_tiles: map.macro_cache().len(),
            rosters: map.roster_cache().len(),
            organisms: map.organism_count(),
            jobs_in_flight: map.jobs_in_flight(),
            pinned_violations,
        }
    }

    fn synthetic_tick_output(&self) -> TickOutput {
        let layout = self.controller.layout();
        TickOutput {
            frame: 0,
            update_serial: 0,
            mode: layout.mode,
            focused: layout.focused,
            traveler: self.controller.world().traveler().position,
            travel: 0.0,
            stats: self.last_stats,
            map: self.controller.map_preferences(),
            pov: self.controller.pov_state(),
            effects: Vec::new(),
            platform: self.platform_telemetry(),
            dirty: PresentationDirty {
                map: false,
                pov: false,
                panel: true,
            },
            needs_frame: false,
        }
    }

    fn panel_document_for(&mut self, hover: HoverInfo, renderer: RendererInfo) -> PanelDocument {
        let tick = self
            .last_tick_output
            .clone()
            .unwrap_or_else(|| self.synthetic_tick_output());
        let performance = self.panel_performance();
        let streaming = self.streaming_supplement(self.composer.pinned_violations);
        let persistence = self.persistence_info();
        let capture = self.controller.capture_preferences();
        let split_ratio = self.controller.layout().split_ratio;
        self.panel_state.document(
            &tick,
            self.controller.world(),
            hover,
            performance,
            streaming,
            persistence,
            renderer,
            capture,
            split_ratio,
        )
    }

    fn debug_panel_document(&mut self) -> PanelDocument {
        let preferences = self.controller.map_preferences();
        let layout = self.controller.layout();
        let hover = match (layout.mode, layout.focused) {
            (PresentationMode::Map, _) | (PresentationMode::Split, ViewKind::Map) => {
                viewer_host::map_hover(
                    self.controller.world().map(),
                    Some(self.controller.world().traveler().position),
                    preferences.zoom,
                )
            }
            (PresentationMode::Pov, _) | (PresentationMode::Split, ViewKind::Pov) => {
                self.pov_hover.hover().clone()
            }
        };
        let surface_format = self
            .renderer
            .as_ref()
            .map(renderer::Renderer::surface_format_name);
        let surface_losses = self
            .renderer
            .as_ref()
            .map_or(0, renderer::Renderer::surface_losses);
        let renderer =
            self.panel_state
                .renderer_for_pov(preferences.backend, surface_format, surface_losses);
        self.panel_document_for(hover, renderer)
    }

    /// The vault-derived map decorations for this frame (phase-5-plan.md
    /// §11): the visible window's discovered set, preserve outlines, and
    /// route polylines. Empty when no vault is open.
    fn build_decor(&self) -> MapDecor {
        let Some(vault) = self.services.vault.as_ref() else {
            return MapDecor::default();
        };
        let traveler = self.controller.world().traveler().position;
        let center = RegionCoord::from_world(traveler.0, traveler.1);
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
        let routes = if self.controller.world().path_tracking() {
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

    /// Resolve one live view deck and the native information panel. Rendering,
    /// pointer routing, map inversion, and POV picking all consume the same
    /// shared pane rectangles.
    fn native_frame_layout(&self, layout: ViewLayout) -> Option<NativeFrameRects> {
        let (width, height) = self.renderer.as_ref()?.size();
        let map_side = self.composer.side();
        let panel_width = self.hud.size().0.checked_sub(map_side)?;
        NativeFrameRects::resolve(
            PixelRect::new(0, 0, width, height),
            map_side,
            panel_width,
            layout,
        )
    }

    /// Pending mode/focus actions already influence raw-event routing before
    /// the next logical frame. Split ratio has no native live control yet, so
    /// its committed shared value is sufficient for this first fixed layout.
    fn pointer_frame_layout(&self) -> Option<NativeFrameRects> {
        let context = self.input_context();
        let mut layout = self.controller.layout();
        layout.mode = context.mode;
        layout.focused = context.focused;
        self.native_frame_layout(layout)
    }

    /// Resolve this frame's exact map projection without reconstructing the
    /// map fit separately from the shared view layout.
    fn map_frame_layout(
        &self,
        player: (f64, f64),
        zoom: u32,
    ) -> Option<(NativeFrameRects, MapViewportProjection)> {
        let rects = self.native_frame_layout(self.controller.layout())?;
        let projection = MapViewportProjection::new(
            rects.views.map_content?,
            player,
            self.composer.half_regions(),
            self.controller.world().map().config().field_resolution,
            zoom,
        )?;
        Some((rects, projection))
    }

    fn pointer_hit_view(&self) -> Option<ViewKind> {
        self.winit_input
            .cursor_position()
            .and_then(|point| self.pointer_hit_at(point))
    }

    fn pointer_hit_at(&self, point: [f64; 2]) -> Option<ViewKind> {
        self.pointer_frame_layout()
            .and_then(|layout| layout.views.hit_view(point))
    }

    /// Map the mouse in physical surface pixels through the exact destination
    /// used by this frame. Letterbox and panel points deliberately return
    /// `None`.
    fn cursor_world_in(&self, projection: MapViewportProjection) -> Option<(f64, f64)> {
        let point = self.cursor_pos?;
        projection.physical_to_world(point)
    }

    /// Current hover used by the debug-dump module outside the live frame.
    /// Live presentation passes its already-resolved frame projection through
    /// [`Self::cursor_world_in`] instead.
    fn cursor_world(&self) -> Option<(f64, f64)> {
        let world = self.controller.world();
        let (_, projection) = self.map_frame_layout(
            world.traveler().position,
            self.controller.map_preferences().zoom,
        )?;
        self.cursor_world_in(projection)
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
            self.panel_state.telemetry_rolled();
        }
    }

    /// Convert an asynchronous wgpu device loss into the shared typed
    /// capability transition before this frame's action/input reduction.
    /// Only renderer-side caches are discarded; the controller, world, panel
    /// document cache, traveler, and persisted state remain intact.
    fn recover_lost_renderer(&mut self) {
        let losses = self
            .renderer
            .as_ref()
            .map_or(0, renderer::Renderer::device_losses);
        if losses > 0 {
            self.device_losses = self.device_losses.saturating_add(losses);
            clear_renderer_loss_input(&mut self.input, &mut self.winit_input);
            self.renderer = None;
            self.atlas = AtlasManager::default();
            self.overlay_hashes = [0; 2];
            self.panel_revision = None;
            self.pov_panel_revision = None;
            self.pov_chunks = PovChunkManager::new();
            self.pov_organisms = PovOrganismManager::new();
            self.pov_hover = PovHoverCache::new();
            self.pov_recovery_ready = false;
            self.renderer_recovery_pending = true;
            self.renderer_retry_at = None;
            let notification = self.services.pov_availability_with_reason(
                false,
                Some(ViewerWarning {
                    id: "renderer-device-loss",
                    message: String::from("GPU device lost; presentation returned to Map"),
                    severity: Severity::Warning,
                }),
            );
            self.controller.enqueue_service_notification(notification);
        }
        if !self.renderer_recovery_pending {
            return;
        }
        let now = Instant::now();
        if self
            .renderer_retry_at
            .is_some_and(|retry_at| now < retry_at)
        {
            return;
        }

        let replacement = self
            .window
            .as_ref()
            .ok_or_else(|| String::from("native window is unavailable"))
            .and_then(|window| build_window_renderer(window).map_err(|error| error.to_string()));
        match replacement {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                // Do not advertise the replacement before the loss tick:
                // queued presentation actions run after service inputs and
                // could otherwise re-enter POV/Split immediately.
                self.pov_recovery_ready = true;
                self.renderer_recovery_pending = false;
                self.renderer_retry_at = None;
                self.clear_recovered_renderer_warning = true;
                log::info!("GPU device recovered; Map presentation resumed");
            }
            Err(error) => {
                self.renderer_retry_at = Some(now + Duration::from_secs(1));
                log::error!("GPU device recovery failed; world remains alive in Map mode: {error}");
            }
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        self.recover_lost_renderer();
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f64();
        self.last_frame = now;

        self.enqueue_actions();
        let previous_layout = self.controller.layout();
        let context = self.input_context();
        self.input.set_context(context);
        let input = self.input.take_frame();
        self.cursor_pos = input.map_pointer.map(|point| (point[0], point[1]));
        self.pov_pointer = input.pov_pointer;
        let platform = self.platform_telemetry();
        let update_start = Instant::now();
        let ground = NativeGroundSampler {
            chunks: &self.pov_chunks,
        };
        let mut output = self.controller.tick(
            TickInput {
                dt_seconds: dt,
                input,
                platform,
            },
            self.executor.as_ref(),
            &mut self.services,
            &ground,
        );
        if std::mem::take(&mut self.pov_recovery_ready) {
            let notification = self.services.pov_availability(true);
            self.controller.enqueue_service_notification(notification);
        }
        let update_seconds = update_start.elapsed().as_secs_f64();
        self.composer
            .update_for_tick(output.update_serial, self.controller.world().map());
        let mut stats = output.stats;
        self.last_stats = stats;
        for (total, &count) in self
            .regen_totals
            .iter_mut()
            .zip(&stats.regenerated_by_layer)
        {
            *total += count as u64;
        }

        let context = self.input_context();
        self.input.set_context(context);
        if previous_layout.focused == ViewKind::Pov && output.focused != ViewKind::Pov {
            self.input.handle_event(
                NormalizedInputEvent::PointerCancelled {
                    pointer: input::MOUSE_POINTER_ID,
                },
                context,
            );
        }
        let effects = std::mem::take(&mut output.effects);
        // A POV debug capture must observe the geometry/hover prepared for
        // this same tick. Split retains the pre-M9 Map-capture behavior until
        // the aligned multi-view dump lands in M10.
        let (deferred_captures, effects): (Vec<_>, Vec<_>) = if output.mode == PresentationMode::Pov
        {
            effects
                .into_iter()
                .partition(|effect| matches!(effect, ViewerEffect::WriteDebugCapture(_)))
        } else {
            (Vec::new(), effects)
        };
        self.last_tick_output = Some(output.clone());
        self.handle_effects(effects, event_loop);

        let Some(frame_rects) = self.native_frame_layout(self.controller.layout()) else {
            self.handle_effects(deferred_captures, event_loop);
            return;
        };
        let map_visible = output.mode != PresentationMode::Pov;
        let pov_visible = output.mode != PresentationMode::Map;
        let map_projection = map_visible
            .then(|| {
                MapViewportProjection::new(
                    frame_rects
                        .views
                        .map_content
                        .expect("visible Map has fitted content"),
                    output.traveler,
                    self.composer.half_regions(),
                    self.controller.world().map().config().field_resolution,
                    output.map.zoom,
                )
            })
            .flatten();
        let map_hover = map_projection.map_or(HoverInfo::None, |projection| {
            viewer_host::map_hover(
                self.controller.world().map(),
                self.cursor_world_in(projection),
                output.map.zoom,
            )
        });

        let mut pov_uploads = Vec::new();
        let mut pov_removes = Vec::new();
        let mut organisms_changed = false;
        let mut pov_params = None;
        let mut upload_bytes = 0u64;
        if pov_visible {
            // POV presentation is lazy: entering POV or Split starts its
            // resident ring, but Map remains available in the same frame.
            let camera_xy = {
                let camera = self.controller.pov_camera();
                (camera.pos.x, camera.pos.y)
            };
            let fog_end = f64::from(pov::pov_fog_end(self.pov_radius) as f32);
            let mesh_start = Instant::now();
            (pov_uploads, pov_removes) = self.pov_chunks.sync(
                self.controller.world().map(),
                camera_xy,
                self.pov_radius,
                self.executor.as_ref(),
            );
            organisms_changed = self.pov_organisms.sync(
                self.controller.world().map(),
                &self.pov_chunks,
                camera_xy,
                fog_end,
            );
            upload_bytes = (pov_uploads.len()
                * renderer::pov::VERTS_PER_CHUNK
                * core::mem::size_of::<renderer::PovVertex>()
                + pov_uploads
                    .iter()
                    .map(|upload| upload.river_indices.len() * 4)
                    .sum::<usize>()) as u64;
            stats.pass_ms[world_runtime::Pass::Mesh.index()] +=
                mesh_start.elapsed().as_secs_f32() * 1000.0;
            self.last_stats = stats;

            let time = self
                .start
                .elapsed()
                .as_secs_f64()
                .rem_euclid(f64::from(renderer::pov::WOBBLE_PERIOD)) as f32;
            let resolution = pov::shadow_resolution(self.tier);
            let shadow = pov::shadow_frame(
                self.controller.pov_camera(),
                &self.pov_chunks,
                self.pov_organisms.shadow_bounds(),
                resolution,
            );
            let params = pov::frame_params(
                self.controller.pov_camera(),
                frame_rects
                    .views
                    .pov_aspect
                    .expect("visible POV has an aspect"),
                self.pov_radius,
                time,
                self.controller.pov_toggles(),
                shadow,
            );
            debug_assert_eq!(f64::from(params.fog_end), fog_end);
            self.pov_hover.update(
                self.controller.world().map(),
                self.controller.pov_camera(),
                &self.pov_chunks,
                &self.pov_organisms,
                self.pov_pointer,
                frame_rects.views.pov_pane,
                f64::from(params.fog_end),
                params.time,
            );
            pov_params = Some(params);
        }
        let pov_hover = if pov_visible {
            self.pov_hover.hover().clone()
        } else {
            HoverInfo::None
        };
        let hover = match output.focused {
            ViewKind::Map => map_hover,
            ViewKind::Pov => pov_hover,
        };

        output.stats = stats;
        self.last_tick_output = Some(output.clone());
        let decor = if map_visible {
            self.build_decor()
        } else {
            MapDecor::default()
        };
        if map_visible {
            self.composer.set_zoom(output.map.zoom);
        }
        let capture = self.controller.capture_preferences();
        let performance = self.panel_performance();
        let streaming = self.streaming_supplement(self.composer.pinned_violations);
        let persistence = self.persistence_info();
        let split_ratio = self.controller.layout().split_ratio;
        let surface_losses = self
            .renderer
            .as_ref()
            .map_or(0, renderer::Renderer::surface_losses);
        let compose_start = Instant::now();
        let map_packet = map_projection.map(|projection| {
            let world = self.controller.world();
            self.composer.prepare_render(
                &mut self.atlas,
                MapRenderRequest {
                    map: world.map(),
                    player: output.traveler,
                    destination: projection.destination,
                    channel: output.map.channel,
                    overlays: output.map.overlays,
                    anchors: world.anchors(),
                    decor: &decor,
                    requested_backend: output.map.backend,
                    gpu_available: self.renderer.is_some(),
                    refinement: RefinementRequest {
                        enabled: output.map.refinement,
                        octave_count: 3,
                    },
                    dirty_key: output.update_serial,
                },
            )
        });
        let renderer_info = map_packet.as_ref().map_or_else(
            || {
                let mut info = self.panel_state.renderer_for_pov(
                    output.map.backend,
                    output.platform.surface_format.clone(),
                    surface_losses,
                );
                info.device_losses = self.device_losses;
                info
            },
            |packet| RendererInfo {
                requested_map_backend: packet.requested_backend,
                effective_map_backend: packet.backend,
                map_fallback: packet.fallback,
                surface_format: output.platform.surface_format.clone(),
                device_losses: self.device_losses,
                surface_losses,
            },
        );
        let document = self.panel_state.document(
            &output,
            self.controller.world(),
            hover,
            performance,
            streaming,
            persistence,
            renderer_info,
            capture,
            split_ratio,
        );
        let map_is_cpu = map_packet
            .as_ref()
            .is_some_and(|packet| matches!(&packet.source, PreparedMapSource::Cpu(_)));
        let (panel_width, panel_height) = if map_is_cpu {
            let (_, width, height) = self.hud.panel_image(&document.sections);
            (width, height)
        } else {
            let (_, width, height, _) = self
                .hud
                .panel_image_for(document.revision, &document.sections);
            (width, height)
        };
        let panel_rgba = self.hud.panel_pixels();
        let panel_on_map = map_visible;
        let panel_changed = map_is_cpu
            || if panel_on_map {
                self.panel_revision != Some(document.revision)
            } else {
                self.pov_panel_revision != Some(document.revision)
            };
        let information = InformationSurface {
            upload: panel_changed.then_some(InformationUpload {
                rgba: panel_rgba,
                width: panel_width,
                height: panel_height,
            }),
            viewport: frame_rects.panel_viewport(),
        };
        let map_pane = map_packet.as_ref().map(|packet| {
            let source = match &packet.source {
                PreparedMapSource::Cpu(cpu) => MapFrameSource::Cpu {
                    rgba: cpu.rgba,
                    width: packet.projection.side,
                    height: packet.projection.side,
                },
                PreparedMapSource::GpuAtlas(gpu) => {
                    debug_assert_eq!(packet.pixel_hash, gpu.overlay_hash);
                    MapFrameSource::Gpu {
                        params: &gpu.params,
                        slots: &gpu.slots,
                        uploads: &gpu.uploads,
                        pre_grid_overlay: (gpu.pre_grid_hash != self.overlay_hashes[0])
                            .then_some(gpu.pre_grid_rgba),
                        post_grid_overlay: (gpu.post_grid_hash != self.overlay_hashes[1])
                            .then_some(gpu.post_grid_rgba),
                    }
                }
            };
            MapFramePane {
                source,
                viewport: frame_rects
                    .map_viewport()
                    .expect("visible Map has a viewport"),
                information: panel_on_map.then_some(information),
            }
        });
        let organism_upload = organisms_changed.then(|| self.pov_organisms.upload());
        let pov_pane = pov_params.as_ref().map(|params| PovFramePane {
            frame: params,
            uploads: &pov_uploads,
            removes: &pov_removes,
            organisms: organism_upload,
            viewport: frame_rects
                .pov_viewport()
                .expect("visible POV has a viewport"),
            information: (!panel_on_map).then_some(information),
            render_scale: output.pov.render_scale,
        });
        let compose_seconds = compose_start.elapsed().as_secs_f64();
        let render_start = Instant::now();
        let result = self.renderer.as_mut().map_or_else(
            renderer::MultiViewFrameResult::default,
            |renderer| {
                renderer.render_frame(MultiViewFrame {
                    clear: CLEAR_COLOR,
                    map: map_pane,
                    pov: pov_pane,
                    focus: frame_rects.focus_decoration(),
                })
            },
        );
        let render_seconds = render_start.elapsed().as_secs_f64();
        debug_assert!(!result.presented || !map_visible || result.map_drawn);
        debug_assert!(!result.presented || !pov_visible || result.pov_drawn);
        upload_bytes = upload_bytes.saturating_add(result.map_upload_bytes);

        if let Some(packet) = map_packet.as_ref() {
            if result.presented {
                self.panel_revision = Some(document.revision);
                if let PreparedMapSource::GpuAtlas(gpu) = &packet.source {
                    self.overlay_hashes = [gpu.pre_grid_hash, gpu.post_grid_hash];
                }
            } else {
                self.panel_revision = None;
                if matches!(&packet.source, PreparedMapSource::GpuAtlas(_)) {
                    // Atlas keys were consumed while preparing this packet;
                    // retry a complete upload after surface recovery.
                    self.atlas = AtlasManager::default();
                    self.overlay_hashes = [0; 2];
                }
            }
        } else {
            upload_bytes = upload_bytes.saturating_add(commit_pov_panel_upload(
                &mut self.pov_panel_revision,
                document.revision,
                (panel_width, panel_height),
                result.presented,
            ));
        }
        let organism_buffer_stats = self
            .renderer
            .as_ref()
            .and_then(renderer::Renderer::pov_organism_stats);
        if let Some(buffers) = organism_buffer_stats {
            upload_bytes = upload_bytes.saturating_add(buffers.replacement_bytes);
        }

        // The once-per-second POV log line (plan §7.5): the steady-state
        // exit criterion reads these — travel stopped ⇒ remeshed stays flat.
        if pov_visible && self.last_telemetry.elapsed().as_secs_f64() >= 1.0 {
            let c = self.pov_chunks.counters();
            let last = self.pov_counters_last;
            let organisms = self.pov_organisms.counters();
            let last_organisms = self.pov_organism_counters_last;
            // The mode tail: the walk form's mesh-vs-analytic tag is the
            // observable for the frontier-fallback exit criterion
            // (3d-phase-2-plan.md §6.2).
            let camera = self.controller.pov_camera();
            let mode = if camera.walk {
                let (ground, mesh) = self.pov_ground();
                format!(
                    "walk {:.0}u/s (ground {:.1}, {})",
                    camera.walk_speed,
                    ground,
                    if mesh { "mesh" } else { "analytic" }
                )
            } else {
                format!("fly {:.0}u/s", camera.speed)
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
            compose_seconds,
            render_seconds,
            &stats.pass_ms,
            upload_bytes,
        );
        self.handle_effects(deferred_captures, event_loop);
        if std::mem::take(&mut self.clear_recovered_renderer_warning) {
            self.panel_state.warnings.remove("renderer-device-loss");
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return; // already initialized (e.g. resume after suspend)
        }

        let mut attributes = Window::default_attributes().with_title("Infinite World Exploration");
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

        // The renderer gets a source of fresh surface targets (not a single
        // surface) so it can rebuild the swapchain if the platform loses it —
        // which WSLg does routinely.
        let renderer = build_window_renderer(&window).expect("failed to initialize renderer");

        log::info!(
            "world algorithm version {} | streaming {:?}",
            world_core::WORLD_ALGORITHM_VERSION,
            self.controller.world().map().config()
        );

        self.window = Some(window);
        self.renderer = Some(renderer);
        let notification = self.services.pov_availability(true);
        self.controller.enqueue_service_notification(notification);
        if std::env::var_os("WER_CPU_MAP").is_none() {
            self.controller
                .enqueue_action(ViewerAction::SetMapBackend(MapBackend::GpuAtlas));
        }
        let pov_scale = self.controller.pov_state().render_scale;
        if pov_scale < 1.0 {
            log::info!(
                "POV render scale {} (WER_POV_SCALE): 3D rasterizes at {:.0}% of the window pixels",
                pov_scale,
                f64::from(pov_scale * pov_scale) * 100.0
            );
        }
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.controller.enqueue_action(ViewerAction::RequestExit);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let event = self.winit_input.modifiers_changed(modifiers.state());
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::Focused(focused) => {
                let event = self.winit_input.focus_changed(focused);
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let hit = self.pointer_hit_at([position.x, position.y]);
                let event = self.winit_input.cursor_moved(position, hit);
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::CursorLeft { .. } => {
                let event = self.winit_input.cursor_left();
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let hit = self.pointer_hit_view();
                if let Some(event) = self.winit_input.mouse_input(state, button, hit) {
                    self.handle_input_event(event, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(view) = self.pointer_hit_view() {
                    let event = self.winit_input.wheel(delta, view);
                    self.handle_input_event(event, event_loop);
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
            } => {
                if let Some(event) = self.winit_input.key_event(code, state, repeat) {
                    self.handle_input_event(event, event_loop);
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame(event_loop);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Commit a POV information-texture upload only after the surface frame was
/// actually drawn. A lost/occluded first frame must retry its initial upload;
/// unchanged successful frames retain the texture and report zero bytes.
fn commit_pov_panel_upload(
    uploaded_revision: &mut Option<u64>,
    current_revision: u64,
    dimensions: (u32, u32),
    rendered: bool,
) -> u64 {
    if !rendered || *uploaded_revision == Some(current_revision) {
        return 0;
    }
    *uploaded_revision = Some(current_revision);
    u64::from(dimensions.0) * u64::from(dimensions.1) * 4
}

/// Headless captures default to Low so their established bytes and cost stay
/// stable. An explicit `WER_TIER` opts Map, POV, and Split into the same Mid or
/// High stream/density presets used by the live shell; unlike live startup,
/// headless work does not probe an adapter and therefore rejects bad values
/// instead of silently choosing a hardware-derived tier.
fn parse_headless_tier(explicit: Option<&str>) -> Result<ResourceTier, String> {
    explicit.map_or(Ok(ResourceTier::Low), |value| {
        ResourceTier::parse(value)
            .ok_or_else(|| format!("invalid WER_TIER={value:?}; expected low, mid, or high"))
    })
}

fn headless_tier() -> Result<ResourceTier, String> {
    match std::env::var("WER_TIER") {
        Ok(value) => parse_headless_tier(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_headless_tier(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(String::from("WER_TIER is not valid Unicode"))
        }
    }
}

/// Build the same semantic panel projection used by the live shell for a
/// deterministic scripted Split capture. The rendered Map/POV both consume
/// `map`; this lightweight world value supplies the builder's viewer settings
/// and tier metadata without introducing a second runtime update authority.
fn headless_script_panel(
    map: &RegionMap,
    camera: &PovCamera,
    tier: ResourceTier,
    pinned_violations: u64,
) -> PanelDocument {
    let camera_xy = (camera.pos.x, camera.pos.y);
    let mut world = ExplorationWorld::with_runtime(tier.stream_config(), Budget::unlimited(), tier);
    world.restore_traveler(camera_xy, camera_xy);
    let controller = ViewerController::new(world);
    let mut map_preferences = controller.map_preferences();
    map_preferences.backend = MapBackend::Cpu;
    let mut pov_state = controller.pov_state();
    pov_state.position = camera.pos.to_array();
    pov_state.yaw = camera.yaw;
    pov_state.pitch = camera.pitch;
    pov_state.fly_speed = camera.speed;
    pov_state.walk = camera.walk;
    pov_state.walk_speed = camera.walk_speed;
    pov_state.initialized = true;
    pov_state.supported = true;
    let tick = TickOutput {
        frame: 1,
        update_serial: 1,
        mode: PresentationMode::Split,
        focused: ViewKind::Map,
        traveler: camera_xy,
        travel: 0.0,
        stats: FrameStats::default(),
        map: map_preferences,
        pov: pov_state,
        effects: Vec::new(),
        platform: PlatformTelemetry {
            executor_backend: WorkerBackend::Inline,
            workers: 1,
            ..PlatformTelemetry::default()
        },
        dirty: PresentationDirty {
            map: true,
            pov: true,
            panel: true,
        },
        needs_frame: false,
    };
    build_panel_document(PanelBuildInput {
        tick: &tick,
        world: controller.world(),
        hover: viewer_host::map_hover(map, Some(camera_xy), map_preferences.zoom),
        performance: PerformanceInfo::default(),
        streaming: StreamingSupplement {
            regen_totals: [0; LAYER_COUNT as usize],
            macro_tiles: map.macro_cache().len(),
            rosters: map.roster_cache().len(),
            organisms: map.organism_count(),
            jobs_in_flight: map.jobs_in_flight(),
            pinned_violations,
        },
        persistence: PersistenceInfo::default(),
        renderer: RendererInfo {
            requested_map_backend: MapBackend::Cpu,
            effective_map_backend: MapBackend::Cpu,
            ..RendererInfo::default()
        },
        capture: controller.capture_preferences(),
        warnings: &[],
        split_ratio: 0.5,
        revision: 1,
    })
}

/// Headless screenshot: settle the streaming window at `pos` and write the
/// composed false-color map as a binary PPM (P6). No window, no GPU — the map
/// is CPU-composed, which is exactly what makes it inspectable in tests and
/// from the command line.
fn run_screenshot(path: &str, channel: Channel, pos: (f64, f64), zoom: u32) -> Result<(), String> {
    let tier = headless_tier()?;
    let cfg = tier.stream_config();
    let mut world = ExplorationWorld::with_runtime(cfg, Budget::unlimited(), tier);
    world.restore_traveler(pos, pos);
    let mut controller = ViewerController::new(world);
    let mut hook = NoopWorldTickHook;
    let ground = viewer_host::AnalyticGroundSampler;
    // Unbudgeted warm-up with the inline executor: fully loaded and generated.
    let mut output = None;
    let mut regen_totals = [0u64; LAYER_COUNT as usize];
    for _ in 0..8 {
        // Zero travel: fresh regions snap to target at load, and regeneration
        // is not gated on movement, so the window still settles fully.
        let tick = controller.tick(
            TickInput {
                dt_seconds: 0.0,
                input: InputFrame::default(),
                platform: PlatformTelemetry::default(),
            },
            &world_runtime::InlineExecutor,
            &mut hook,
            &ground,
        );
        for (total, &count) in regen_totals
            .iter_mut()
            .zip(&tick.stats.regenerated_by_layer)
        {
            *total += count as u64;
        }
        output = Some(tick);
    }
    let mut output = output.expect("headless warm-up runs at least once");
    output.map.channel = channel;
    output.map.zoom = zoom;
    output.map.backend = MapBackend::Cpu;
    output.map.overlays = Overlays {
        grid: false,
        rings: false,
        pinned_flash: false,
        organisms: true,
        discovered: false,
    };

    let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
    let mut composer = MapComposer::new(half_regions, cfg.field_resolution);
    composer.set_zoom(zoom);
    let map = controller.world().map();
    composer.update_for_tick(output.update_serial, map);
    composer.compose(
        map,
        pos,
        channel,
        output.map.overlays,
        controller.world().anchors(),
        &MapDecor::default(),
    );

    let hover = viewer_host::map_hover(map, Some(pos), zoom);
    match &hover {
        HoverInfo::Organism(organism) => {
            log::info!(
                "picked organism {:#018x} ({} at {:.0}, {:.0})",
                organism.id,
                organism.trophic.name(),
                organism.world.0,
                organism.world.1
            );
        }
        HoverInfo::None | HoverInfo::Terrain(_) if zoom >= ORGANISM_INFO_ZOOM => {
            log::info!("no organism within a cell of ({}, {})", pos.0, pos.1);
        }
        HoverInfo::None | HoverInfo::Terrain(_) => {}
    }

    let document = build_panel_document(PanelBuildInput {
        tick: &output,
        world: controller.world(),
        hover,
        performance: PerformanceInfo {
            pass_ms: output.stats.pass_ms,
            ..PerformanceInfo::default()
        },
        streaming: StreamingSupplement {
            regen_totals,
            macro_tiles: map.macro_cache().len(),
            rosters: map.roster_cache().len(),
            organisms: map.organism_count(),
            jobs_in_flight: map.jobs_in_flight(),
            pinned_violations: composer.pinned_violations,
        },
        persistence: PersistenceInfo::default(),
        renderer: RendererInfo {
            requested_map_backend: MapBackend::Cpu,
            effective_map_backend: MapBackend::Cpu,
            ..RendererInfo::default()
        },
        capture: controller.capture_preferences(),
        warnings: &[],
        split_ratio: controller.layout().split_ratio,
        revision: 1,
    });

    let mut hud = Hud::new(composer.side() as usize);
    let (width, height) = hud.size();
    let pixels = hud.compose(composer.pixels(), &document.sections);
    dump::write_ppm(std::path::Path::new(path), pixels, width, height)?;
    log::info!(
        "wrote {width}x{height} {} map+panel at ({}, {}) to {path} ({} tier)",
        channel.name(),
        pos.0,
        pos.1,
        tier.name(),
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
    let tier = headless_tier()?;
    let cfg = tier.stream_config();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let mut map = RegionMap::new(cfg);
    let half_regions = (cfg.load_radius / REGION_SIZE).ceil() as i32;
    let mut composer = MapComposer::new(half_regions, cfg.field_resolution);
    let mut hud = Hud::new(composer.side() as usize);
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
    let mut capture_size = None;

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
                    return Err(String::from("size must come before the first capture"));
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
            instruction @ (pov::PovInstr::Snap(_) | pov::PovInstr::Split(_)) => {
                let split = matches!(instruction, pov::PovInstr::Split(_));
                let path = match instruction {
                    pov::PovInstr::Snap(path) | pov::PovInstr::Split(path) => path,
                    _ => unreachable!("capture alternatives matched above"),
                };
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
                let split_rectangles = if split {
                    let (source_width, source_height) = hud.size();
                    let panel_width = source_width.checked_sub(source_height).ok_or_else(|| {
                        String::from("native panel source is narrower than its map")
                    })?;
                    Some(
                        NativeFrameRects::resolve(
                            PixelRect::new(0, 0, size.0, size.1),
                            source_height,
                            panel_width,
                            ViewLayout {
                                mode: PresentationMode::Split,
                                focused: ViewKind::Map,
                                split_ratio: 0.5,
                            },
                        )
                        .ok_or_else(|| {
                            String::from("capture size is too small for Split views and panel")
                        })?,
                    )
                } else {
                    None
                };
                let pov_size = split_rectangles.map_or(size, |rectangles| {
                    let pane = rectangles
                        .views
                        .pov_pane
                        .expect("Split layout always has a POV pane");
                    (pane.width, pane.height)
                });
                if capture_size != Some(pov_size) {
                    // A capture target owns pane-sized GPU attachments. When
                    // a script switches between `snap:` and `split:`, rebuild
                    // both the file-bound target and its upload-only resident
                    // mirrors so the new target is seeded completely.
                    chunks = PovChunkManager::new();
                    organisms = PovOrganismManager::new();
                    capture = Some(
                        renderer::pov::PovCapture::new(pov_size.0, pov_size.1)
                            .map_err(|e| format!("pov capture init: {e}"))?,
                    );
                    capture_size = Some(pov_size);
                }
                let cap = capture.as_mut().expect("just ensured");
                let mut terrain_uploads = 0usize;
                for _ in 0..256 {
                    let (uploads, removes) = chunks.sync(
                        &map,
                        (camera.pos.x, camera.pos.y),
                        radius,
                        &world_runtime::InlineExecutor,
                    );
                    terrain_uploads += uploads.len();
                    let done = uploads.is_empty() && chunks.is_idle();
                    cap.apply(&uploads, &removes, None);
                    if done {
                        break;
                    }
                }
                let organisms_changed =
                    organisms.sync(&map, &chunks, camera_xy, pov::pov_fog_end(radius));
                cap.apply(&[], &[], organisms_changed.then(|| organisms.upload()));
                let aspect = pov_size.0 as f32 / pov_size.1 as f32;
                // Time-frozen captures (3d-phase-3-plan.md §4.3): two snaps
                // of the same pose are byte-comparable; toggles all-on.
                let shadow = pov::shadow_frame(
                    &camera,
                    &chunks,
                    organisms.shadow_bounds(),
                    pov::shadow_resolution(tier),
                );
                let params = pov::frame_params(
                    &camera,
                    aspect,
                    radius,
                    0.0,
                    pov::PovToggles::default(),
                    shadow,
                );
                let rgba = cap.snapshot_at_scale(&params, CLEAR_COLOR, scale);
                if let Some(rectangles) = split_rectangles {
                    composer.set_zoom(1);
                    composer.compose(
                        &map,
                        camera_xy,
                        Channel::Composite,
                        Overlays {
                            grid: false,
                            rings: false,
                            pinned_flash: false,
                            organisms: true,
                            discovered: false,
                        },
                        &[],
                        &MapDecor::default(),
                    );
                    let document =
                        headless_script_panel(&map, &camera, tier, composer.pinned_violations);
                    let (panel_rgba, panel_width, panel_height) = {
                        let (pixels, panel_width, panel_height, _) =
                            hud.panel_image_for(document.revision, &document.sections);
                        (pixels.to_vec(), panel_width, panel_height)
                    };
                    let combined = dump::compose_capture_surface(
                        size,
                        rectangles,
                        Some((composer.pixels(), (composer.side(), composer.side()))),
                        Some((&rgba, pov_size)),
                        (&panel_rgba, (panel_width, panel_height)),
                    )?;
                    dump::write_ppm(std::path::Path::new(&path), &combined, size.0, size.1)?;
                } else {
                    dump::write_ppm(std::path::Path::new(&path), &rgba, pov_size.0, pov_size.1)?;
                }
                let organism_counts = organisms.counters();
                let organism_buffers = cap.organism_stats();
                let presentation = if split { "split (map focus)" } else { "pov" };
                log::info!(
                    "{presentation} snapshot {path}: {}x{} {} tier at traveler/camera ({:.1}, {:.1}, {:.1}) yaw {:.1}° pitch {:.1}° | {} chunks, {} terrain uploads this capture | {}/{} organisms drawn (box {}, sphere {}, waiting {}, culled {}; realization {} updates) | instances {:.1}/{:.1} KiB live/cap, {:.1} KiB replacement",
                    size.0,
                    size.1,
                    tier.name(),
                    camera.pos.x,
                    camera.pos.y,
                    camera.pos.z,
                    camera.yaw.to_degrees(),
                    camera.pitch.to_degrees(),
                    chunks.len(),
                    terrain_uploads,
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
        let usage = "usage: wer --pov-script \"pos:300,-10; snap:a.ppm; mouse:200,-50; split:both.ppm\"\n\
                     instructions: size:WxH | pos:x,y[,z] | mouse:dx,dy | move:f[,r[,u]] | walk | fly | settle[:n] | snap:file.ppm | split:file.ppm\n\
                     optional environment: WER_TIER=low|mid|high (default low)";
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

    #[derive(Debug)]
    enum TestPreserveRemovalError {
        MissingVaultRecord,
        Persistence,
    }

    fn remove_effective_preserve<S: Storage>(
        map: &mut RegionMap,
        vault: &mut Vault<S>,
        coord: RegionCoord,
    ) -> Result<Option<(u64, String)>, TestPreserveRemovalError> {
        let Some((id, _)) = map.effective_preserve(coord) else {
            return Ok(None);
        };
        let Some(record) = vault.preserves().get(&id).cloned() else {
            return Err(TestPreserveRemovalError::MissingVaultRecord);
        };
        let removed = vault
            .remove_preserve(id)
            .map_err(|_| TestPreserveRemovalError::Persistence)?;
        assert!(removed);
        for (region, _) in record.regions {
            map.remove_preserve_contribution(id, region);
        }
        Ok(Some((id, record.name)))
    }

    #[derive(Debug)]
    struct TestRouteRemovalOutcome {
        removed: usize,
        total: usize,
        error: Option<VaultPersistenceError>,
    }

    fn remove_routes<S: Storage>(
        vault: &mut Vault<S>,
        tracker: &mut RouteTracker,
    ) -> TestRouteRemovalOutcome {
        let ids: Vec<_> = vault.routes().keys().copied().collect();
        let total = ids.len();
        let mut removed = 0;
        let mut error = None;
        for id in ids {
            match vault.remove_route(id) {
                Ok(true) => removed += 1,
                Ok(false) => unreachable!("route id came from the vault"),
                Err(found) => {
                    error = Some(found);
                    break;
                }
            }
        }
        tracker.retain(|id| vault.routes().contains_key(&id));
        TestRouteRemovalOutcome {
            removed,
            total,
            error,
        }
    }

    struct TestVaultHook<S: Storage> {
        vault: Vault<S>,
    }

    impl<S: Storage> WorldTickHook for TestVaultHook<S> {
        fn before_world_update(&mut self, input: WorldPreUpdate<'_>) -> WorldServiceInput {
            WorldServiceInput {
                derived_anchors: if input.path_tracking && input.route_attraction {
                    world_core::attraction_anchors(
                        self.vault.routes().values(),
                        input.traveler,
                        input.budget.max_route_attraction_nodes,
                    )
                } else {
                    Vec::new()
                },
                active_routes: if input.path_tracking {
                    self.vault.routes().values().cloned().collect()
                } else {
                    Vec::new()
                },
            }
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
            Err(TestPreserveRemovalError::Persistence)
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
            let mut hook = TestVaultHook { vault };
            let mut controller = ViewerController::new(ExplorationWorld::new(tier));
            let tick = |controller: &mut ViewerController,
                        hook: &mut TestVaultHook<FileStorage>| {
                controller.tick(
                    TickInput::default(),
                    &world_runtime::InlineExecutor,
                    hook,
                    &viewer_host::controller::AnalyticGroundSampler,
                )
            };
            // Publish canonical organisms and establish the unsteered current.
            for _ in 0..128 {
                tick(&mut controller, &mut hook);
                if controller
                    .world()
                    .map()
                    .authoritative_realization_complete((0.0, 0.0))
                {
                    break;
                }
            }
            assert!(controller
                .world()
                .map()
                .authoritative_realization_complete((0.0, 0.0)));

            let derived = world_core::attraction_anchors(
                hook.vault.routes().values(),
                (0.0, 0.0),
                controller.world().budget().max_route_attraction_nodes,
            );
            assert_eq!(derived.len(), 32);
            assert!(derived
                .iter()
                .all(|anchor| anchor.strength < world_core::route_pull(0)));
            assert!(world_core::anchor_influence_profile(&derived, (0.0, 0.0))
                .into_iter()
                .all(|pull| pull <= world_core::ROUTE_PULL_CAP));
            let mut effective = explicit.clone();
            let explicit_only = world_core::anchor_set_signature(&effective);
            effective.extend(derived.iter().copied());
            let expected_signature = world_core::anchor_set_signature(&effective);
            assert_ne!(expected_signature, explicit_only);

            controller.enqueue_action(ViewerAction::SummonDiscoveries);
            let load_request = tick(&mut controller, &mut hook)
                .effects
                .into_iter()
                .find_map(|effect| match effect {
                    ViewerEffect::LoadDiscoveries(id) => Some(id),
                    _ => None,
                })
                .expect("summon emits a discovery-load request");
            controller.enqueue_service_response(ServiceResponse {
                sequence: ServiceResponseSequence(1),
                request_id: load_request,
                result: ServiceResponseResult::DiscoveriesLoaded(explicit),
            });
            controller.enqueue_action(ViewerAction::TogglePathTracking);
            controller.enqueue_action(ViewerAction::ToggleRouteRecording);
            let recorded = tick(&mut controller, &mut hook);
            let stats = recorded.stats;
            let world = controller.world();
            let resonance = world.map().resonance_at((0.0, 0.0), &effective);
            assert!(!resonance.nodes.is_empty());
            assert!(resonance.anchor_compatibility < 1.0);
            assert_eq!(
                stats.resonance_strength.to_bits(),
                resonance.strength.to_bits()
            );
            let coord = RegionCoord::from_world(0.0, 0.0);
            let target_bits = world
                .map()
                .get(coord)
                .unwrap()
                .target
                .dims
                .map(f32::to_bits);

            for _ in 0..4 {
                controller.tick(
                    TickInput {
                        dt_seconds: 0.1,
                        input: InputFrame {
                            map_axis: [1, 0],
                            ..InputFrame::default()
                        },
                        platform: PlatformTelemetry::default(),
                    },
                    &world_runtime::InlineExecutor,
                    &mut hook,
                    &viewer_host::controller::AnalyticGroundSampler,
                );
            }
            controller.enqueue_action(ViewerAction::ToggleRouteRecording);
            let route_request = tick(&mut controller, &mut hook)
                .effects
                .into_iter()
                .find_map(|effect| match effect {
                    ViewerEffect::WriteRoute(request) => Some(request),
                    _ => None,
                })
                .expect("finishing the recording emits its durable request");
            let nodes = route_request.nodes;
            let discoveries = route_request.discoveries;
            assert!(nodes.len() >= 2);
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

            drop(controller);
            drop(hook);
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
    use viewer_host::input::WHEEL_PIXELS_PER_NOTCH;
    use winit::dpi::PhysicalPosition;
    use winit::event::{ElementState, MouseButton, MouseScrollDelta};
    use winit::keyboard::{KeyCode, ModifiersState};
    use world_runtime::{Budget, InlineExecutor};

    fn context(mode: PresentationMode) -> InputContext {
        let focused = match mode {
            PresentationMode::Map => ViewKind::Map,
            PresentationMode::Pov => ViewKind::Pov,
            PresentationMode::Split => panic!("characterization uses one visible pane"),
        };
        InputContext {
            mode,
            focused,
            surface_focused: true,
        }
    }

    fn send_key(
        adapter: &input::WinitInputAdapter,
        mapper: &mut InputMapper,
        mode: PresentationMode,
        code: KeyCode,
        state: ElementState,
        repeat: bool,
    ) -> bool {
        let event = adapter
            .key_event(code, state, repeat)
            .expect("characterization key is supported");
        mapper.handle_event(event, context(mode))
    }

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

    #[test]
    fn native_panel_cache_builds_for_map_and_pov_semantic_changes() {
        let world = ExplorationWorld::with_runtime(
            StreamConfig {
                near_radius: 0.0,
                far_radius: 0.0,
                load_radius: 0.0,
                unload_radius: 1.0,
                ..StreamConfig::default()
            },
            Budget::unlimited(),
            ResourceTier::Low,
        );
        let mut controller = ViewerController::new(world);
        let mut hook = NoopWorldTickHook;
        let mut tick = controller.tick(
            TickInput {
                dt_seconds: 0.0,
                input: InputFrame::default(),
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &viewer_host::AnalyticGroundSampler,
        );
        let mut panel = NativePanelState::default();
        let map = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo::default(),
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        assert_eq!(map.model.view.mode, PresentationMode::Map);
        assert_eq!(panel.cache.builds(), 1);

        let repeated = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo::default(),
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        assert_eq!(repeated.revision, map.revision);
        assert_eq!(panel.cache.builds(), 1);

        tick.frame = tick.frame.saturating_add(1);
        tick.mode = PresentationMode::Pov;
        tick.focused = ViewKind::Pov;
        tick.pov.supported = true;
        tick.dirty.panel = true;
        let pov = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo::default(),
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        assert_eq!(pov.model.view.mode, PresentationMode::Pov);
        assert_eq!(panel.cache.builds(), 2);
        assert_eq!(
            map.sections
                .iter()
                .map(|section| section.id)
                .collect::<Vec<_>>(),
            pov.sections
                .iter()
                .map(|section| section.id)
                .collect::<Vec<_>>()
        );

        tick.frame = tick.frame.saturating_add(1);
        tick.dirty.panel = false;
        tick.pov.yaw += 0.25;
        let rotated = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo::default(),
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        assert_eq!(rotated.model.view.camera.yaw, tick.pov.yaw);
        assert_eq!(panel.cache.builds(), 3);
    }

    #[test]
    fn native_warning_registry_reaches_the_shared_document() {
        let controller = ViewerController::default();
        let tick = TickOutput {
            frame: 1,
            update_serial: 0,
            mode: PresentationMode::Map,
            focused: ViewKind::Map,
            traveler: (0.0, 0.0),
            travel: 0.0,
            stats: FrameStats::default(),
            map: controller.map_preferences(),
            pov: controller.pov_state(),
            effects: Vec::new(),
            platform: PlatformTelemetry::default(),
            dirty: PresentationDirty {
                map: false,
                pov: false,
                panel: true,
            },
            needs_frame: false,
        };
        let mut panel = NativePanelState::default();
        panel.retain_warning(ViewerWarning {
            id: "native-test-warning",
            message: String::from("retained warning"),
            severity: Severity::Warning,
        });
        let document = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo::default(),
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        assert_eq!(document.model.warnings.len(), 1);
        assert!(document.sections.iter().any(|section| {
            section.fields.iter().any(|field| {
                field.id.as_str() == "warnings.native-test-warning"
                    && field.value == "retained warning"
            })
        }));

        let fallback = panel.document(
            &tick,
            controller.world(),
            HoverInfo::None,
            PerformanceInfo::default(),
            StreamingSupplement::default(),
            PersistenceInfo::default(),
            RendererInfo {
                requested_map_backend: MapBackend::GpuAtlas,
                effective_map_backend: MapBackend::Cpu,
                map_fallback: Some(viewer_host::MapBackendFallback::GpuUnavailable),
                surface_format: Some(String::from("Bgra8UnormSrgb")),
                surface_losses: 1,
                ..RendererInfo::default()
            },
            controller.capture_preferences(),
            controller.layout().split_ratio,
        );
        let field = |id: &str| {
            fallback
                .sections
                .iter()
                .flat_map(|section| &section.fields)
                .find(|field| field.id.as_str() == id)
                .expect("renderer diagnostic field")
        };
        assert_eq!(field("runtime.surface-format").value, "Bgra8UnormSrgb");
        assert_eq!(field("runtime.surface-losses").value, "1");
        assert!(fallback
            .model
            .warnings
            .iter()
            .any(|warning| warning.id == "renderer-map-fallback"));
        assert!(fallback
            .model
            .warnings
            .iter()
            .any(|warning| warning.id == "renderer-surface-loss"));
    }

    fn option_f32_bits(value: Option<f32>) -> String {
        value.map_or_else(|| String::from("none"), |v| format!("{:08x}", v.to_bits()))
    }

    fn tick_controller(controller: &mut ViewerController, input: InputFrame) {
        let mut hook = viewer_host::world::NoopWorldTickHook;
        controller.tick(
            TickInput {
                dt_seconds: 0.0,
                input,
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &viewer_host::controller::AnalyticGroundSampler,
        );
    }

    #[test]
    fn native_adapter_controller_trace_matches_the_wasm_golden() {
        let adapter = input::WinitInputAdapter::default();
        let mut mapper = InputMapper::default();
        let mut controller = ViewerController::new(ExplorationWorld::with_runtime(
            StreamConfig {
                near_radius: 0.0,
                far_radius: 0.0,
                load_radius: 0.0,
                unload_radius: 1.0,
                ..StreamConfig::default()
            },
            Budget::unlimited(),
            ResourceTier::Low,
        ));
        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(1),
            supported: true,
            reason: None,
        });

        for key in [KeyCode::KeyW, KeyCode::Tab, KeyCode::KeyB] {
            let event = adapter
                .key_event(key, ElementState::Pressed, false)
                .expect("trace key is supported by the native adapter");
            assert!(mapper.handle_event(event, controller.input_context(true)));
            for action in mapper.drain_actions() {
                controller.enqueue_action(action);
            }
        }

        let preview = controller.input_context(true);
        assert_eq!(preview.mode, PresentationMode::Pov);
        assert_eq!(preview.focused, ViewKind::Pov);
        mapper.set_context(preview);
        let input = mapper.take_frame();
        assert_eq!(input.map_axis, [0, 0]);
        assert_eq!(input.pov_axis, [0, 1, 0]);
        let mut hook = viewer_host::world::NoopWorldTickHook;
        let output = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input,
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &viewer_host::controller::AnalyticGroundSampler,
        );

        assert_eq!(output.update_serial, 1);
        assert_eq!(output.mode, PresentationMode::Pov);
        assert_eq!(output.focused, ViewKind::Pov);
        assert_eq!(
            format!("[{:.3},{:.3}]", output.traveler.0, output.traveler.1),
            "[-0.000,4.000]"
        );
        assert_eq!(output.travel, 4.0);
        assert!(!output.pov.shadow_ao);
    }

    #[test]
    fn native_renderer_loss_does_not_reinterpret_held_pov_navigation_as_map_travel() {
        let mut adapter = input::WinitInputAdapter::default();
        let mut mapper = InputMapper::default();
        let pov = context(PresentationMode::Pov);
        mapper.set_context(pov);
        let pressed = adapter
            .key_event(KeyCode::KeyW, ElementState::Pressed, false)
            .expect("W is supported");
        assert!(mapper.handle_event(pressed, pov));
        assert!(mapper.has_continuous_input());

        let position = PhysicalPosition::new(80.0, 40.0);
        let moved = adapter.cursor_moved(position, Some(ViewKind::Pov));
        assert!(mapper.handle_event(moved, pov));
        let press = adapter
            .mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .expect("POV primary press");
        assert!(mapper.handle_event(press, pov));

        clear_renderer_loss_input(&mut mapper, &mut adapter);
        let map = context(PresentationMode::Map);
        mapper.set_context(map);
        assert!(!mapper.has_continuous_input());
        assert!(matches!(
            adapter.cursor_moved(position, Some(ViewKind::Map)),
            NormalizedInputEvent::PointerMoved {
                view: ViewKind::Map,
                ..
            }
        ));
        let input = mapper.take_frame();
        assert_eq!(input.map_axis, [0, 0]);
        assert_eq!(input.pov_axis, [0, 0, 0]);

        let mut controller = ViewerController::new(ExplorationWorld::with_runtime(
            StreamConfig {
                near_radius: 0.0,
                far_radius: 0.0,
                load_radius: 0.0,
                unload_radius: 1.0,
                ..StreamConfig::default()
            },
            Budget::unlimited(),
            ResourceTier::Low,
        ));
        let mut hook = viewer_host::world::NoopWorldTickHook;
        let output = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input,
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &viewer_host::controller::AnalyticGroundSampler,
        );
        assert_eq!(output.travel, 0.0);
        assert_eq!(output.traveler, (0.0, 0.0));
    }

    #[test]
    fn shared_map_movement_preserves_legacy_evaluation_order() {
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
            for sprint in [false, true] {
                let mut adapter = input::WinitInputAdapter::default();
                let mut mapper = InputMapper::default();
                for (index, &key) in controls.iter().enumerate() {
                    if mask & (1 << index) != 0 {
                        assert!(send_key(
                            &adapter,
                            &mut mapper,
                            PresentationMode::Map,
                            key,
                            ElementState::Pressed,
                            false,
                        ));
                    }
                }
                if sprint {
                    let event = adapter.modifiers_changed(ModifiersState::SHIFT);
                    mapper.handle_event(event, context(PresentationMode::Map));
                }
                let frame = mapper.take_frame();
                let down = |code| {
                    controls
                        .iter()
                        .enumerate()
                        .any(|(index, &candidate)| candidate == code && mask & (1 << index) != 0)
                };
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
                assert_eq!(frame.map_axis, [dx as i8, dy as i8]);
                assert_eq!(frame.sprint, sprint);
                for dt in dts {
                    // The pre-characterization `apply_movement` expression,
                    // copied as an independent bit-exact oracle.
                    let len = f64::hypot(dx, dy);
                    let expected = if len == 0.0 {
                        None
                    } else {
                        let multiplier = if sprint { 4.0 } else { 1.0 };
                        let step = PLAYER_SPEED * multiplier * dt / len;
                        Some((dx * step, dy * step))
                    };
                    let bridged = frame.map_movement_delta(PLAYER_SPEED, dt);
                    assert_eq!(
                        bridged.map(|(x, y)| (x.to_bits(), y.to_bits())),
                        expected.map(|(x, y)| (x.to_bits(), y.to_bits())),
                        "shared frame mask {mask:#05x}, sprint {sprint}, dt {dt:?}"
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
        let cursor = viewer_host::sample_cell(&map, source.world_pos);
        let organism =
            viewer_host::pick_map_organism_info(&map, source.world_pos, ORGANISM_INFO_ZOOM)
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
            cursor.region.x, cursor.region.y
        )
        .unwrap();
        writeln!(&mut actual, "cursor.status {}", cursor.status.as_str()).unwrap();
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
            ecology.herbivore.expect("settled herbivore").to_bits(),
            ecology.predator.expect("settled predator").to_bits(),
            ecology.diversity.expect("settled diversity").to_bits()
        )
        .unwrap();
        writeln!(&mut actual, "organism.id {:016x}", organism.id).unwrap();
        writeln!(&mut actual, "organism.slot {}", organism.slot).unwrap();
        writeln!(
            &mut actual,
            "organism.cell {} {}",
            organism.cell.cx, organism.cell.cy
        )
        .unwrap();
        writeln!(&mut actual, "organism.species {:016x}", organism.species).unwrap();
        writeln!(&mut actual, "organism.trophic {}", organism.trophic.name()).unwrap();
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

    /// A semantic Map/POV trace through the production winit adapter, shared
    /// mapper, and shared controller reducer. It freezes held movement, diagonal
    /// normalization, one-shot repeat suppression, fractional wheels, and
    /// primary-held POV look without retaining a second binding authority.
    #[test]
    fn native_input_characterization() {
        let mut actual = String::from("native-input-characterization-v1\n");
        let dt = 0.1f64;
        let mut adapter = input::WinitInputAdapter::default();
        let mut mapper = InputMapper::default();
        let mut controller = ViewerController::new(ExplorationWorld::with_runtime(
            StreamConfig {
                near_radius: 0.0,
                far_radius: 0.0,
                load_radius: 0.0,
                unload_radius: 1.0,
                ..StreamConfig::default()
            },
            Budget::unlimited(),
            ResourceTier::Low,
        ));

        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Map,
            KeyCode::KeyW,
            ElementState::Pressed,
            false,
        ));
        let frame_w = mapper.take_frame();
        let (axis_w_x, axis_w_y) = (
            f64::from(frame_w.map_axis[0]),
            f64::from(frame_w.map_axis[1]),
        );
        let axis_w_len = f64::hypot(axis_w_x, axis_w_y);
        let axis_w = (axis_w_x / axis_w_len, axis_w_y / axis_w_len);
        let delta_w = frame_w
            .map_movement_delta(PLAYER_SPEED, dt)
            .expect("W is active");
        writeln!(
            &mut actual,
            "map held=KeyW axis={:016x},{:016x} delta={:016x},{:016x}",
            axis_w.0.to_bits(),
            axis_w.1.to_bits(),
            delta_w.0.to_bits(),
            delta_w.1.to_bits()
        )
        .unwrap();

        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Map,
            KeyCode::KeyD,
            ElementState::Pressed,
            false,
        ));
        let frame_wd = mapper.take_frame();
        let (axis_wd_x, axis_wd_y) = (
            f64::from(frame_wd.map_axis[0]),
            f64::from(frame_wd.map_axis[1]),
        );
        let axis_wd_len = f64::hypot(axis_wd_x, axis_wd_y);
        let axis_wd = (axis_wd_x / axis_wd_len, axis_wd_y / axis_wd_len);
        let delta_wd = frame_wd
            .map_movement_delta(PLAYER_SPEED, dt)
            .expect("W+D is active");
        writeln!(
            &mut actual,
            "map held=KeyD+KeyW axis={:016x},{:016x} delta={:016x},{:016x}",
            axis_wd.0.to_bits(),
            axis_wd.1.to_bits(),
            delta_wd.0.to_bits(),
            delta_wd.1.to_bits()
        )
        .unwrap();

        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Map,
            KeyCode::KeyV,
            ElementState::Pressed,
            false,
        ));
        let first_actions: Vec<_> = mapper.drain_actions().collect();
        let first_action = first_actions == [ViewerAction::CycleMapChannel];
        for action in first_actions {
            controller.apply_action(action);
        }
        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Map,
            KeyCode::KeyV,
            ElementState::Pressed,
            true,
        ));
        let repeat_actions: Vec<_> = mapper.drain_actions().collect();
        writeln!(
            &mut actual,
            "map one-shot=KeyV first={} repeat={} channel={}",
            first_action,
            !repeat_actions.is_empty(),
            controller.map_preferences().channel.name()
        )
        .unwrap();

        let wheel_steps = |actions: &[ViewerAction]| -> i32 {
            actions
                .iter()
                .map(|action| match action {
                    ViewerAction::ZoomIn => 1,
                    ViewerAction::ZoomOut => -1,
                    other => panic!("unexpected wheel action {other:?}"),
                })
                .sum()
        };
        let mut map_pixels = 15.0;
        let event = adapter.wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 15.0)),
            ViewKind::Map,
        );
        assert!(mapper.handle_event(event, context(PresentationMode::Map)));
        let actions: Vec<_> = mapper.drain_actions().collect();
        let first_notches = wheel_steps(&actions);
        let mut map_steps = first_notches;
        for action in actions {
            controller.apply_action(action);
        }
        let remainder = map_pixels / WHEEL_PIXELS_PER_NOTCH - f64::from(map_steps);
        writeln!(
            &mut actual,
            "map wheel-pixel=15 notches={first_notches} remainder={:016x}",
            remainder.to_bits()
        )
        .unwrap();
        map_pixels += 30.0;
        let event = adapter.wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 30.0)),
            ViewKind::Map,
        );
        assert!(mapper.handle_event(event, context(PresentationMode::Map)));
        let actions: Vec<_> = mapper.drain_actions().collect();
        let second_notches = wheel_steps(&actions);
        map_steps += second_notches;
        for action in actions {
            controller.apply_action(action);
        }
        let remainder = map_pixels / WHEEL_PIXELS_PER_NOTCH - f64::from(map_steps);
        writeln!(
            &mut actual,
            "map wheel-pixel=30 notches={second_notches} remainder={:016x}",
            remainder.to_bits()
        )
        .unwrap();
        map_pixels -= 50.0;
        let event = adapter.wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, -50.0)),
            ViewKind::Map,
        );
        assert!(mapper.handle_event(event, context(PresentationMode::Map)));
        let actions: Vec<_> = mapper.drain_actions().collect();
        let reverse_notches = wheel_steps(&actions);
        map_steps += reverse_notches;
        for action in actions {
            controller.apply_action(action);
        }
        let remainder = map_pixels / WHEEL_PIXELS_PER_NOTCH - f64::from(map_steps);
        writeln!(
            &mut actual,
            "map wheel-pixel=-50 notches={reverse_notches} remainder={:016x}",
            remainder.to_bits()
        )
        .unwrap();

        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(1),
            supported: true,
            reason: None,
        });
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        tick_controller(&mut controller, InputFrame::default());
        mapper.set_context(context(PresentationMode::Pov));
        let pov_frame = mapper.take_frame();
        let strafe = f64::from(pov_frame.pov_axis[0]);
        let forward = f64::from(pov_frame.pov_axis[1]);
        let vertical = f64::from(pov_frame.pov_axis[2]);
        let mut pov_delta = controller.pov_camera().forward() * forward
            + controller.pov_camera().right() * strafe
            + glam::DVec3::Z * vertical;
        pov_delta = pov_delta.normalize() * (controller.pov_camera().speed * dt);
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

        let event = adapter.cursor_moved(PhysicalPosition::new(100.0, 100.0), Some(ViewKind::Pov));
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let unheld = mapper.take_frame().look_delta != [0.0, 0.0];
        writeln!(&mut actual, "pov move-unheld look={unheld}").unwrap();
        let event = adapter
            .mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .expect("cursor position arms the drag");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(112.0, 92.0), Some(ViewKind::Pov));
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let drag = mapper.take_frame().look_delta;
        tick_controller(
            &mut controller,
            InputFrame {
                look_delta: drag,
                ..InputFrame::default()
            },
        );
        writeln!(
            &mut actual,
            "pov drag delta={:016x},{:016x} yaw={:08x} pitch={:08x}",
            drag[0].to_bits(),
            drag[1].to_bits(),
            controller.pov_camera().yaw.to_bits(),
            controller.pov_camera().pitch.to_bits()
        )
        .unwrap();
        let event = adapter
            .mouse_input(
                ElementState::Released,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .expect("release retains the cursor position");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(140.0, 140.0), Some(ViewKind::Pov));
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let after_release = mapper.take_frame().look_delta != [0.0, 0.0];
        writeln!(
            &mut actual,
            "pov move-released look={} yaw={:08x} pitch={:08x}",
            after_release,
            controller.pov_camera().yaw.to_bits(),
            controller.pov_camera().pitch.to_bits()
        )
        .unwrap();
        let event = adapter
            .mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .expect("second press retains the cursor position");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_left();
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(150.0, 150.0), Some(ViewKind::Pov));
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let after_cancel = mapper.take_frame().look_delta != [0.0, 0.0];
        writeln!(&mut actual, "pov move-cancelled look={after_cancel}").unwrap();

        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Pov,
            KeyCode::KeyB,
            ElementState::Pressed,
            false,
        ));
        let first_actions: Vec<_> = mapper.drain_actions().collect();
        let pov_first = first_actions == [ViewerAction::TogglePovShadowAo];
        for action in first_actions {
            controller.apply_action(action);
        }
        assert!(send_key(
            &adapter,
            &mut mapper,
            PresentationMode::Pov,
            KeyCode::KeyB,
            ElementState::Pressed,
            true,
        ));
        let repeat_actions: Vec<_> = mapper.drain_actions().collect();
        writeln!(
            &mut actual,
            "pov one-shot=KeyB first={} repeat={} shadow-ao={}",
            pov_first,
            !repeat_actions.is_empty(),
            controller.pov_toggles().shadow_ao
        )
        .unwrap();

        let mut pov_pixels = 20.0;
        let event = adapter.wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 20.0)),
            ViewKind::Pov,
        );
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let pov_first_wheel = mapper.take_frame().wheel_steps;
        let mut pov_steps = pov_first_wheel;
        let remainder = pov_pixels / WHEEL_PIXELS_PER_NOTCH - f64::from(pov_steps);
        writeln!(
            &mut actual,
            "pov wheel-pixel=20 notches={pov_first_wheel} remainder={:016x} speed={:016x}",
            remainder.to_bits(),
            controller.pov_camera().speed.to_bits()
        )
        .unwrap();
        pov_pixels += 25.0;
        let event = adapter.wheel(
            MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 25.0)),
            ViewKind::Pov,
        );
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let pov_second_wheel = mapper.take_frame().wheel_steps;
        pov_steps += pov_second_wheel;
        tick_controller(
            &mut controller,
            InputFrame {
                wheel_steps: pov_second_wheel,
                ..InputFrame::default()
            },
        );
        let remainder = pov_pixels / WHEEL_PIXELS_PER_NOTCH - f64::from(pov_steps);
        writeln!(
            &mut actual,
            "pov wheel-pixel=25 notches={pov_second_wheel} remainder={:016x} speed={:016x}",
            remainder.to_bits(),
            controller.pov_camera().speed.to_bits()
        )
        .unwrap();

        assert_eq!(
            actual.trim_end(),
            include_str!("../tests/fixtures/native_input_characterization.txt").trim_end()
        );
    }
}
