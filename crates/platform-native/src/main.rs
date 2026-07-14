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
mod input;
mod panel;
mod pov;
mod viz;

use std::sync::Arc;
use std::time::Instant;

use renderer::{PovInformationSurface, PovInformationUpload, Renderer, SurfaceViewport};
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
use viewer_host::layout::{MapViewportProjection, PixelRect, PresentationMode, ViewKind};
use viewer_host::map::{MapBackend, MapRenderRequest, PreparedMapSource};
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
    RegionMap, ResourceTier, SessionCompatibility, StreamConfig, TierInputs, Vault, VaultStats,
};
#[cfg(test)]
use world_runtime::{RouteTracker, Storage, VaultPersistenceError};

use executor::LaneExecutor;
use panel::Hud;
use pov::{PovCamera, PovChunkManager, PovCounters, PovOrganismCounters, PovOrganismManager};
use tools::FileStorage;
use viz::{Channel, MapComposer, MapDecor, Overlays};

/// Letterbox color around the square map (linear RGBA).
const CLEAR_COLOR: [f64; 4] = [0.02, 0.02, 0.04, 1.0];

/// Exact physical rectangles for the native map plus its temporary bitmap
/// information strip. The combined rectangle preserves the source HUD aspect,
/// while the square left edge remains the single draw/pick destination used by
/// the shared map projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeMapRects {
    combined: PixelRect,
    map: PixelRect,
    panel: PixelRect,
}

impl NativeMapRects {
    fn resolve(surface: PixelRect, map_side: u32, panel_width: u32) -> Option<Self> {
        let source_width = map_side.checked_add(panel_width)?;
        let combined = surface.fitted_aspect(source_width, map_side)?;
        // The source is at least square, so its fitted physical rectangle is
        // at least as wide as it is tall (including degenerate tiny surfaces).
        let map_width = combined.height.min(combined.width);
        let map = PixelRect::new(combined.x, combined.y, map_width, map_width);
        let panel = PixelRect::new(
            map.right(),
            combined.y,
            combined.width.saturating_sub(map.width),
            combined.height,
        );
        if map.width == 0 || map.height == 0 || panel.width == 0 || panel.height == 0 {
            return None;
        }
        Some(Self {
            combined,
            map,
            panel,
        })
    }

    const fn map_viewport(self) -> SurfaceViewport {
        SurfaceViewport::new(self.map.x, self.map.y, self.map.width, self.map.height)
    }

    const fn panel_viewport(self) -> Option<SurfaceViewport> {
        if self.panel.width == 0 || self.panel.height == 0 {
            None
        } else {
            Some(SurfaceViewport::new(
                self.panel.x,
                self.panel.y,
                self.panel.width,
                self.panel.height,
            ))
        }
    }
}

/// Gate POV gestures through the same half-open rectangle used by rendering
/// and picking. The temporary native information strip is not a view surface;
/// routing it as Map makes POV presses/wheels inert in single-POV mode while
/// still letting a release clear an already-held drag by pointer identity.
fn routed_native_pointer_view(
    focused: ViewKind,
    pov_pane: Option<PixelRect>,
    pointer: Option<[f64; 2]>,
) -> ViewKind {
    if focused == ViewKind::Pov
        && !pointer
            .zip(pov_pane)
            .is_some_and(|(point, pane)| pane.contains_f64(point[0], point[1]))
    {
        ViewKind::Map
    } else {
        focused
    }
}

#[cfg(test)]
mod native_map_rect_tests {
    use super::*;

    #[test]
    fn native_map_and_panel_partition_one_aspect_fitted_rectangle() {
        for (width, height) in [(1280, 720), (900, 700), (701, 509), (320, 200), (127, 511)] {
            let surface = PixelRect::new(0, 0, width, height);
            let rects = NativeMapRects::resolve(surface, 600, 300).unwrap();
            assert!(surface.contains_rect(rects.combined));
            assert_eq!(rects.map.x, rects.combined.x);
            assert_eq!(rects.map.y, rects.combined.y);
            assert_eq!(rects.map.width, rects.map.height);
            assert_eq!(rects.map.height, rects.combined.height);
            assert_eq!(rects.panel.x, rects.map.right());
            assert_eq!(rects.panel.y, rects.combined.y);
            assert_eq!(rects.panel.right(), rects.combined.right());
            assert_eq!(rects.panel.bottom(), rects.combined.bottom());
            assert!(!rects.map.overlaps(rects.panel));

            let expected_panel = (u64::from(rects.combined.height) * 300 + 300) / 600;
            assert!(u64::from(rects.panel.width).abs_diff(expected_panel) <= 1);
        }
    }

    #[test]
    fn panel_and_letterbox_are_outside_the_map_pick_projection() {
        let surface = PixelRect::new(0, 0, 1280, 720);
        let rects = NativeMapRects::resolve(surface, 600, 300).unwrap();
        let projection = MapViewportProjection::new(rects.map, (12.5, -8.25), 3, 16, 8)
            .expect("valid map geometry");

        let map_center = (
            f64::from(rects.map.x) + f64::from(rects.map.width) * 0.5,
            f64::from(rects.map.y) + f64::from(rects.map.height) * 0.5,
        );
        assert!(projection.physical_to_world(map_center).is_some());
        assert!(projection
            .physical_to_world((f64::from(rects.map.right()), map_center.1))
            .is_none());
        assert!(projection
            .physical_to_world((f64::from(rects.combined.x) - 0.5, map_center.1))
            .is_none());
    }

    #[test]
    fn pov_gestures_route_only_inside_the_exact_render_pane() {
        let surface = PixelRect::new(0, 0, 1280, 720);
        let rects = NativeMapRects::resolve(surface, 600, 300).unwrap();
        assert_eq!(
            routed_native_pointer_view(
                ViewKind::Pov,
                Some(rects.map),
                Some([f64::from(rects.map.x) + 0.5, f64::from(rects.map.y) + 0.5,]),
            ),
            ViewKind::Pov
        );
        assert_eq!(
            routed_native_pointer_view(
                ViewKind::Pov,
                Some(rects.map),
                Some([f64::from(rects.panel.x) + 0.5, f64::from(rects.panel.y)]),
            ),
            ViewKind::Map
        );
        assert_eq!(
            routed_native_pointer_view(ViewKind::Pov, Some(rects.map), None),
            ViewKind::Map
        );
        assert_eq!(
            routed_native_pointer_view(ViewKind::Map, Some(rects.map), None),
            ViewKind::Map
        );
    }

    #[test]
    fn tiny_surfaces_without_both_visible_source_panes_are_skipped() {
        for surface in [
            PixelRect::new(0, 0, 0, 0),
            PixelRect::new(0, 0, 1, 1),
            PixelRect::new(0, 0, 1, 2),
        ] {
            assert_eq!(NativeMapRects::resolve(surface, 600, 300), None);
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
            let rects = NativeMapRects::resolve(
                PixelRect::new(0, 0, width, height),
                800,
                panel::PANEL_WIDTH as u32,
            )
            .expect("visible POV and panel panes");
            let center = (
                rects.map.x + rects.map.width / 2,
                rects.map.y + rects.map.height / 2,
            );
            assert!(rects.map.contains(center.0, center.1));
            assert!(!rects.panel.contains(center.0, center.1));
            assert_eq!(
                rects.map.width as f32 / rects.map.height as f32,
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
        self.response_sequence = self.response_sequence.saturating_add(1);
        ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(self.response_sequence),
            supported,
            reason: None,
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
        if std::env::var_os("WER_POV").is_some_and(|value| value != "0") {
            controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
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
        let hover = match self.controller.layout().mode {
            PresentationMode::Map | PresentationMode::Split => viewer_host::map_hover(
                self.controller.world().map(),
                Some(self.controller.world().traveler().position),
                preferences.zoom,
            ),
            PresentationMode::Pov => self.pov_hover.hover().clone(),
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

    /// Resolve this frame's native HUD rectangles and exact map projection.
    /// Rendering and pointer inversion both consume this value; neither path
    /// reconstructs letterboxing or reads the composer's previous zoom.
    fn map_frame_layout(
        &self,
        player: (f64, f64),
        zoom: u32,
    ) -> Option<(NativeMapRects, MapViewportProjection)> {
        let (width, height) = self.renderer.as_ref()?.size();
        let map_side = self.composer.side();
        let panel_width = self.hud.size().0.checked_sub(map_side)?;
        let rects =
            NativeMapRects::resolve(PixelRect::new(0, 0, width, height), map_side, panel_width)?;
        let projection = MapViewportProjection::new(
            rects.map,
            player,
            self.composer.half_regions(),
            self.controller.world().map().config().field_resolution,
            zoom,
        )?;
        Some((rects, projection))
    }

    /// Resolve the exact native POV/panel fit before either picking or panel
    /// construction. The returned square `map` rectangle is the live POV pane;
    /// both the camera ray and renderer viewport consume it unchanged.
    fn pov_frame_layout(&self) -> Option<NativeMapRects> {
        let (width, height) = self.renderer.as_ref()?.size();
        let (source_width, source_height) = self.hud.size();
        let panel_width = source_width.checked_sub(source_height)?;
        NativeMapRects::resolve(
            PixelRect::new(0, 0, width, height),
            source_height,
            panel_width,
        )
    }

    fn pointer_gesture_view(&self) -> ViewKind {
        let focused = self.input_context().focused;
        let pov_pane = (focused == ViewKind::Pov)
            .then(|| self.pov_frame_layout().map(|rects| rects.map))
            .flatten();
        routed_native_pointer_view(focused, pov_pane, self.winit_input.cursor_position())
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

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
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
        let update_seconds = update_start.elapsed().as_secs_f64();
        self.composer
            .update_for_tick(output.update_serial, self.controller.world().map());
        let stats = output.stats;
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
        // this same tick. Other effects keep their ordinary pre-presentation
        // handling; only F12 capture is deferred until frame_pov has synced,
        // picked, built the panel, and submitted the matching view.
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

        if output.mode == PresentationMode::Pov {
            self.frame_pov(&output, update_seconds);
            self.handle_effects(deferred_captures, event_loop);
            return;
        }

        let Some((frame_rects, map_projection)) =
            self.map_frame_layout(output.traveler, output.map.zoom)
        else {
            return;
        };
        let cursor_world = self.cursor_world_in(map_projection);
        let hover =
            viewer_host::map_hover(self.controller.world().map(), cursor_world, output.map.zoom);

        let decor = self.build_decor();
        self.composer.set_zoom(output.map.zoom);
        let capture = self.controller.capture_preferences();
        let pinned_violations = self.composer.pinned_violations;
        let performance = self.panel_performance();
        let streaming = self.streaming_supplement(pinned_violations);
        let persistence = self.persistence_info();
        let split_ratio = self.controller.layout().split_ratio;
        let compose_start = Instant::now();
        let packet = {
            let world = self.controller.world();
            self.composer.prepare_render(
                &mut self.atlas,
                MapRenderRequest {
                    map: world.map(),
                    player: output.traveler,
                    destination: map_projection.destination,
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
        };
        let prepared_pixel_hash = packet.pixel_hash;
        let map_side = packet.projection.side;
        let renderer_info = RendererInfo {
            requested_map_backend: packet.requested_backend,
            effective_map_backend: packet.backend,
            map_fallback: packet.fallback,
            surface_format: output.platform.surface_format.clone(),
            device_losses: 0,
            surface_losses: self
                .renderer
                .as_ref()
                .map_or(0, renderer::Renderer::surface_losses),
        };
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
        let mut upload_bytes = 0u64;
        let (compose_seconds, render_seconds) = match packet.source {
            PreparedMapSource::GpuAtlas(gpu) => {
                // Delta uploads: only regions whose dependency-hash key
                // changed, plus overlay strips or a shared panel document
                // whose semantic revision changed.
                let (_, panel_width, panel_height, _) = self
                    .hud
                    .panel_image_for(document.revision, &document.sections);
                let panel_w = (self.panel_revision != Some(document.revision))
                    .then_some((panel_width, panel_height));
                debug_assert_eq!(prepared_pixel_hash, gpu.overlay_hash);
                let pre_grid_changed = gpu.pre_grid_hash != self.overlay_hashes[0];
                let post_grid_changed = gpu.post_grid_hash != self.overlay_hashes[1];

                let compose_seconds = compose_start.elapsed().as_secs_f64();
                let render_start = Instant::now();
                if let Some(renderer) = self.renderer.as_mut() {
                    let pre_grid = pre_grid_changed.then_some(gpu.pre_grid_rgba);
                    let post_grid = post_grid_changed.then_some(gpu.post_grid_rgba);
                    let panel =
                        panel_w.map(|(width, height)| (self.hud.panel_pixels(), width, height));
                    if let Some(bytes) = renderer.render_map_gpu_in(
                        &gpu.params,
                        &gpu.slots,
                        &gpu.uploads,
                        pre_grid,
                        post_grid,
                        panel,
                        frame_rects.map_viewport(),
                        frame_rects.panel_viewport(),
                        CLEAR_COLOR,
                    ) {
                        self.panel_revision = Some(document.revision);
                        self.overlay_hashes = [gpu.pre_grid_hash, gpu.post_grid_hash];
                        upload_bytes = bytes;
                    } else {
                        // Atlas keys were consumed while preparing this packet;
                        // retry a complete upload after surface recovery.
                        self.atlas = AtlasManager::default();
                        self.panel_revision = None;
                        self.overlay_hashes = [0; 2];
                    }
                }
                (compose_seconds, render_start.elapsed().as_secs_f64())
            }
            PreparedMapSource::Cpu(cpu) => {
                let (panel, panel_width, panel_height) = self.hud.panel_image(&document.sections);
                let compose_seconds = compose_start.elapsed().as_secs_f64();
                let render_start = Instant::now();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.render_map_and_panel_in(
                        cpu.rgba,
                        map_side,
                        map_side,
                        panel,
                        panel_width,
                        panel_height,
                        frame_rects.map_viewport(),
                        frame_rects
                            .panel_viewport()
                            .expect("native bitmap panel has a visible viewport"),
                        CLEAR_COLOR,
                    );
                }
                (compose_seconds, render_start.elapsed().as_secs_f64())
            }
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
    /// through [`Renderer::render_pov`]. The complete shared panel document is
    /// built once and mounted as the POV information rail; M8 only changes how
    /// the already-shared passes are planned.
    fn frame_pov(&mut self, output: &TickOutput, update_seconds: f64) {
        let mut stats = output.stats;
        let camera = self.controller.pov_camera();
        let fog_end = f64::from(pov::pov_fog_end(self.pov_radius) as f32);
        // Ordering contract for native hover: the controller tick has already
        // applied primary-drag look and the single world update. Synchronize
        // the geometry those values select next, then pick, then build the
        // panel and draw. Hover alone never contributes a look delta.
        // The frame-side POV work (scheduling + amortized integration) fills
        // the Mesh pass, following the Flush precedent (plan §8.1); the
        // worker-side mesh milliseconds ride the manager's counters instead.
        let mesh_start = Instant::now();
        let (uploads, removes) = self.pov_chunks.sync(
            self.controller.world().map(),
            (camera.pos.x, camera.pos.y),
            self.pov_radius,
            self.executor.as_ref(),
        );
        let organisms_changed = self.pov_organisms.sync(
            self.controller.world().map(),
            &self.pov_chunks,
            (camera.pos.x, camera.pos.y),
            fog_end,
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

        let Some(frame_rects) = self.pov_frame_layout() else {
            return;
        };
        let pov_viewport = frame_rects.map_viewport();
        let panel_viewport = frame_rects
            .panel_viewport()
            .expect("resolved native POV panel has a visible viewport");
        // The water-wobble clock (3d-phase-3-plan.md §7.1): wrapped at the
        // shader's period so f32 never loses phase precision. This exact value
        // also drives animated-organism picking.
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
            pov_viewport.width as f32 / pov_viewport.height.max(1) as f32,
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
            Some(frame_rects.map),
            f64::from(params.fog_end),
            params.time,
        );
        let hover = self.pov_hover.hover().clone();

        let mut panel_tick = output.clone();
        panel_tick.stats = stats;
        self.last_tick_output = Some(panel_tick.clone());
        let surface_format = self
            .renderer
            .as_ref()
            .map(renderer::Renderer::surface_format_name);
        let surface_losses = self
            .renderer
            .as_ref()
            .map_or(0, renderer::Renderer::surface_losses);
        let renderer_info =
            self.panel_state
                .renderer_for_pov(output.map.backend, surface_format, surface_losses);
        let performance = self.panel_performance();
        let streaming = self.streaming_supplement(self.composer.pinned_violations);
        let persistence = self.persistence_info();
        let capture = self.controller.capture_preferences();
        let split_ratio = self.controller.layout().split_ratio;
        let document = self.panel_state.document(
            &panel_tick,
            self.controller.world(),
            hover,
            performance,
            streaming,
            persistence,
            renderer_info,
            capture,
            split_ratio,
        );

        let render_start = Instant::now();
        let mut organism_buffer_stats = None;
        if let Some(renderer) = self.renderer.as_mut() {
            let (panel_rgba, panel_width, panel_height, _) = self
                .hud
                .panel_image_for(document.revision, &document.sections);
            let panel_changed = self.pov_panel_revision != Some(document.revision);
            // Keep the complete shared information surface mounted beside a
            // projection-correct POV pane. Unchanged panel pixels retain the
            // renderer texture and incur no upload.
            let information = Some(PovInformationSurface {
                upload: panel_changed.then_some(PovInformationUpload {
                    rgba: panel_rgba,
                    width: panel_width,
                    height: panel_height,
                }),
                viewport: panel_viewport,
            });
            let rendered = renderer.render_pov(
                &params,
                &uploads,
                &removes,
                organism_upload,
                CLEAR_COLOR,
                pov_viewport,
                information,
                output.pov.render_scale,
            );
            upload_bytes += commit_pov_panel_upload(
                &mut self.pov_panel_revision,
                document.revision,
                (panel_width, panel_height),
                rendered,
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
                let focused = self.input_context().focused;
                let event = self.winit_input.cursor_moved(position, focused);
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::CursorLeft { .. } => {
                let event = self.winit_input.cursor_left();
                self.handle_input_event(event, event_loop);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let view = self.pointer_gesture_view();
                if let Some(event) = self.winit_input.mouse_input(state, button, view) {
                    self.handle_input_event(event, event_loop);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let view = self.pointer_gesture_view();
                let event = self.winit_input.wheel(delta, view);
                self.handle_input_event(event, event_loop);
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

/// Headless screenshot: settle the streaming window at `pos` and write the
/// composed false-color map as a binary PPM (P6). No window, no GPU — the map
/// is CPU-composed, which is exactly what makes it inspectable in tests and
/// from the command line.
fn run_screenshot(path: &str, channel: Channel, pos: (f64, f64), zoom: u32) -> Result<(), String> {
    let cfg = StreamConfig::default();
    let mut world = ExplorationWorld::with_runtime(cfg, Budget::unlimited(), ResourceTier::Low);
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

        let event = adapter.cursor_moved(PhysicalPosition::new(100.0, 100.0), ViewKind::Pov);
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let unheld = mapper.take_frame().look_delta != [0.0, 0.0];
        writeln!(&mut actual, "pov move-unheld look={unheld}").unwrap();
        let event = adapter
            .mouse_input(ElementState::Pressed, MouseButton::Left, ViewKind::Pov)
            .expect("cursor position arms the drag");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(112.0, 92.0), ViewKind::Pov);
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
            .mouse_input(ElementState::Released, MouseButton::Left, ViewKind::Pov)
            .expect("release retains the cursor position");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(140.0, 140.0), ViewKind::Pov);
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
            .mouse_input(ElementState::Pressed, MouseButton::Left, ViewKind::Pov)
            .expect("second press retains the cursor position");
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_left();
        assert!(mapper.handle_event(event, context(PresentationMode::Pov)));
        let event = adapter.cursor_moved(PhysicalPosition::new(150.0, 150.0), ViewKind::Pov);
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
