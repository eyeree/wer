//! Ordered semantic reducer and the one-traveler/one-update viewer tick
//! (`native-web-alignment.md` sections 4.3 and 5.3).

use std::collections::{BTreeMap, VecDeque};

use pov_host::{PovCamera, PovToggles, EYE_HEIGHT};
use world_core::{
    bound_target, domain_mask, Anchor, AnchorKind, AnchorSnapshot, AnchorSource, PossibilityDomain,
    PossibilitySignature, RegionCoord, RegionSnapshotRecord, SessionSnapshot, TraitCategory,
    POSSIBILITY_DIMS, REGION_SIZE,
};
use world_runtime::{
    apply_session_regions, session_runtime_record, FrameStats, RegionMap,
    SessionSnapshotOwnedInput, StreamConfig, TaskExecutor,
};

use crate::action::{
    DebugCaptureRequest, DiscoveryWriteRequest, NudgeDirection, PreserveMutation, PreserveRequest,
    RouteWriteRequest, ServiceRequestId, SessionWriteRequest, ViewerAction, ViewerEffect,
};
use crate::input::{InputContext, InputFrame};
use crate::layout::{PresentationMode, ViewKind, ViewLayout};
use crate::map::{Channel, MapBackend, Overlays};
use crate::panel::{PlatformTelemetry, Severity, ViewerWarning};
use crate::world::{
    ExplorationWorld, WorldTickHook, WorldTickOutput, MAP_MOVEMENT_SPEED, MAX_TICK_SECONDS,
};

/// Bias delta retained from the pre-alignment native and browser reducers.
pub const POSSIBILITY_NUDGE_STEP: f32 = 0.05;

/// Strength of a manual or captured anchor.
pub const ANCHOR_STRENGTH: f32 = 0.8;

/// World-space falloff radius of a manual or captured anchor.
pub const ANCHOR_RADIUS: f64 = 2048.0;

/// Largest power-of-two map magnification.
pub const MAX_MAP_ZOOM: u32 = 16;

/// Ground result supplied by a platform-neutral POV presentation service.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundSample {
    /// Ground elevation under the camera.
    pub height: f64,
    /// Whether the result came from resident rendered mesh rather than the
    /// deterministic analytic frontier fallback.
    pub mesh_resident: bool,
}

/// Read-only POV grounding seam. A native/web shell normally delegates to
/// `pov_host::walk_ground` with its presentation chunk manager.
pub trait PovGroundSampler {
    fn sample_ground(&self, map: &RegionMap, position: (f64, f64)) -> GroundSample;
}

impl<F> PovGroundSampler for F
where
    F: Fn(&RegionMap, (f64, f64)) -> GroundSample,
{
    fn sample_ground(&self, map: &RegionMap, position: (f64, f64)) -> GroundSample {
        self(map, position)
    }
}

/// Analytic entry/fallback sampler useful before a POV chunk is resident.
#[derive(Debug, Default, Clone, Copy)]
pub struct AnalyticGroundSampler;

impl PovGroundSampler for AnalyticGroundSampler {
    fn sample_ground(&self, map: &RegionMap, position: (f64, f64)) -> GroundSample {
        GroundSample {
            height: pov_host::entry_ground(map, position),
            mesh_resident: false,
        }
    }
}

/// Shared top-down presentation preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPreferences {
    pub channel: Channel,
    pub overlays: Overlays,
    pub zoom: u32,
    pub backend: MapBackend,
    pub refinement: bool,
}

impl Default for MapPreferences {
    fn default() -> Self {
        Self {
            channel: Channel::Composite,
            overlays: Overlays::default(),
            zoom: 1,
            // Capability is platform-reported. CPU is the truthful fallback
            // until a shell confirms a working GPU atlas path.
            backend: MapBackend::Cpu,
            refinement: false,
        }
    }
}

/// Capture category/polarity shared by both shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePreferences {
    pub category: TraitCategory,
    pub polarity: AnchorKind,
}

impl Default for CapturePreferences {
    fn default() -> Self {
        Self {
            category: TraitCategory::Morphology,
            polarity: AnchorKind::Emphasize,
        }
    }
}

/// Copyable POV state exposed to render/panel code without giving a shell
/// mutation authority over the camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PovStateSnapshot {
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fly_speed: f64,
    pub walk: bool,
    pub walk_speed: f64,
    pub shadow_ao: bool,
    pub detail_normals: bool,
    pub water: bool,
    pub render_scale: f32,
    pub initialized: bool,
    pub supported: bool,
}

#[derive(Debug)]
struct PovState {
    camera: PovCamera,
    toggles: PovToggles,
    render_scale: f32,
    initialized: bool,
    supported: bool,
    snap_walk_ground: bool,
}

impl Default for PovState {
    fn default() -> Self {
        Self {
            camera: PovCamera::new(),
            toggles: PovToggles::default(),
            render_scale: 1.0,
            initialized: false,
            // A shell opts in only after renderer/device initialization.
            supported: false,
            snap_walk_ground: false,
        }
    }
}

impl PovState {
    fn snapshot(&self) -> PovStateSnapshot {
        PovStateSnapshot {
            position: [self.camera.pos.x, self.camera.pos.y, self.camera.pos.z],
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            fly_speed: self.camera.speed,
            walk: self.camera.walk,
            walk_speed: self.camera.walk_speed,
            shadow_ao: self.toggles.shadow_ao,
            detail_normals: self.toggles.detail_normals,
            water: self.toggles.water,
            render_scale: self.render_scale,
            initialized: self.initialized,
            supported: self.supported,
        }
    }
}

/// Monotonic adapter sequence attached to every asynchronous service response.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceResponseSequence(pub u64);

/// Decoded session plus the compatibility decisions made by the platform
/// storage adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSession {
    pub snapshot: Box<SessionSnapshot>,
    /// Exact-compatible sessions may restore their recorded stream config;
    /// `None` retains the current platform configuration.
    pub stream_config: Option<StreamConfig>,
    /// Incompatible sessions retain world values but discard route leg state.
    pub restore_route_state: bool,
    /// Canonically ordered durable preserve contributions reapplied after the
    /// replacement map is restored and before its first update.
    pub preserve_contributions: Vec<(u64, RegionCoord, PossibilitySignature)>,
}

/// Typed asynchronous service result. No result mutates shared state until it
/// is drained at the beginning of a controller tick.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceResponseResult {
    /// Successful operation with no returned viewer data.
    Completed,
    /// Decoded session ready for shared-state restoration.
    SessionLoaded(LoadedSession),
    /// Retained discoveries converted to their exact live anchor form.
    DiscoveriesLoaded(Vec<Anchor>),
    /// Content id assigned to a newly persisted discovery.
    DiscoveryWritten { id: u64 },
    /// New preserve content id and the exact contributions now owned by it.
    PreserveCreated {
        id: u64,
        regions: Vec<(RegionCoord, PossibilitySignature)>,
    },
    /// Removed preserve id and regions whose contribution must be withdrawn.
    PreserveRemoved { id: u64, regions: Vec<RegionCoord> },
    /// Durable route-clear outcome. A partial failure returns the ids that
    /// remain plus a warning; shared tracker state is retained for exactly
    /// those records.
    RoutesCleared {
        remaining_route_ids: Vec<u64>,
        warning: Option<ViewerWarning>,
    },
    /// Service failure; the warning is surfaced in reducer order.
    Failed(ViewerWarning),
}

/// One correlated asynchronous service response.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceResponse {
    pub sequence: ServiceResponseSequence,
    pub request_id: ServiceRequestId,
    pub result: ServiceResponseResult,
}

/// Typed asynchronous platform notification that is not correlated with a
/// viewer action request. Notifications share the service-response sequence
/// and queue so capability changes cannot reorder around completed storage
/// work or mutate presentation between controller ticks.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceNotification {
    /// Result of actual POV renderer/device initialization, or a later loss.
    PovAvailability {
        sequence: ServiceResponseSequence,
        supported: bool,
        reason: Option<ViewerWarning>,
    },
}

impl ServiceNotification {
    const fn sequence(&self) -> ServiceResponseSequence {
        match self {
            Self::PovAvailability { sequence, .. } => *sequence,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ServiceInput {
    Response(ServiceResponse),
    Notification(ServiceNotification),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingRequest {
    SaveSession,
    LoadSession,
    WriteDiscovery,
    LoadDiscoveries,
    MutatePreserve,
    WriteRoute,
    ClearRoutes,
    ConfigurePathTracking,
    AtlasImport,
    AtlasExport,
    DebugCapture,
}

/// Inputs sampled for one logical viewer tick after platform events have been
/// normalized. Service responses and queued actions are already held by the
/// controller and drain before this continuous intent.
#[derive(Debug, Clone, PartialEq)]
pub struct TickInput {
    /// Elapsed frame time in seconds (finite values clamp to 0..=0.1).
    pub dt_seconds: f64,
    /// Continuous held input intent.
    pub input: InputFrame,
    /// Measurements injected by the platform shell exactly once.
    pub platform: PlatformTelemetry,
}

impl Default for TickInput {
    fn default() -> Self {
        Self {
            dt_seconds: 0.0,
            input: InputFrame::default(),
            platform: PlatformTelemetry::default(),
        }
    }
}

/// Which presentation products changed during a tick.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PresentationDirty {
    pub map: bool,
    pub pov: bool,
    pub panel: bool,
}

impl PresentationDirty {
    #[must_use]
    pub const fn any(self) -> bool {
        self.map || self.pov || self.panel
    }
}

/// Value-only result returned to a thin platform shell.
#[derive(Debug, Clone, PartialEq)]
pub struct TickOutput {
    /// Monotonic logical viewer frame.
    pub frame: u64,
    /// Monotonic single-world-update serial.
    pub update_serial: u64,
    /// Presentation selected after reduction.
    pub mode: PresentationMode,
    /// Pane receiving view-scoped input.
    pub focused: ViewKind,
    /// Shared traveler/map/streaming center.
    pub traveler: (f64, f64),
    /// Travel supplied to convergence once.
    pub travel: f64,
    /// Counters from the sole RegionMap update.
    pub stats: FrameStats,
    /// Map presentation preferences.
    pub map: MapPreferences,
    /// POV camera and toggles after movement.
    pub pov: PovStateSnapshot,
    /// Effects requiring platform capabilities, in reducer order.
    pub effects: Vec<ViewerEffect>,
    /// Platform measurements consumed by this frame.
    pub platform: PlatformTelemetry,
    /// Presentation invalidation summary.
    pub dirty: PresentationDirty,
    /// Shared scheduler hint. Idle Map may sleep when false.
    pub needs_frame: bool,
}

/// Shared semantic action consumer, camera/traveler synchronizer, and sole
/// logical viewer-tick authority.
#[derive(Debug)]
pub struct ViewerController {
    world: ExplorationWorld,
    layout: ViewLayout,
    map: MapPreferences,
    capture: CapturePreferences,
    pov: PovState,
    actions: VecDeque<ViewerAction>,
    service_inputs: VecDeque<ServiceInput>,
    effects: Vec<ViewerEffect>,
    pending: BTreeMap<ServiceRequestId, PendingRequest>,
    next_request_id: u64,
    last_response_sequence: Option<ServiceResponseSequence>,
    last_completed_request: Option<ServiceRequestId>,
    frame: u64,
    dirty: PresentationDirty,
}

impl ViewerController {
    /// Wrap an exploration world with default Map presentation preferences.
    #[must_use]
    pub fn new(world: ExplorationWorld) -> Self {
        let refinement = world.tier().refinement();
        Self {
            world,
            layout: ViewLayout::default(),
            map: MapPreferences {
                refinement,
                ..MapPreferences::default()
            },
            capture: CapturePreferences::default(),
            pov: PovState::default(),
            actions: VecDeque::new(),
            service_inputs: VecDeque::new(),
            effects: Vec::new(),
            pending: BTreeMap::new(),
            next_request_id: 1,
            last_response_sequence: None,
            last_completed_request: None,
            frame: 0,
            dirty: PresentationDirty {
                map: true,
                pov: true,
                panel: true,
            },
        }
    }

    /// Authoritative exploration state for presentation and typed services.
    #[must_use]
    pub const fn world(&self) -> &ExplorationWorld {
        &self.world
    }

    /// Current visibility/focus state.
    #[must_use]
    pub const fn layout(&self) -> ViewLayout {
        self.layout
    }

    /// Input-routing context after every already-enqueued service capability
    /// and layout/focus action is previewed in the controller's tick order.
    /// Adapters call this before decoding each raw event, so two events that
    /// arrive before one frame still observe intervening `Tab`/focus actions
    /// without maintaining a second layout reducer.
    #[must_use]
    pub fn input_context(&self, surface_focused: bool) -> InputContext {
        let (layout, _) = self.preview_layout();
        InputContext {
            mode: layout.mode,
            focused: layout.focused,
            surface_focused,
        }
    }

    /// Current map preferences.
    #[must_use]
    pub const fn map_preferences(&self) -> MapPreferences {
        self.map
    }

    /// Current capture preferences.
    #[must_use]
    pub const fn capture_preferences(&self) -> CapturePreferences {
        self.capture
    }

    /// Shared POV camera used by `pov-host` and renderer packet builders.
    #[must_use]
    pub const fn pov_camera(&self) -> &PovCamera {
        &self.pov.camera
    }

    /// Copy the shared POV renderer switches.
    #[must_use]
    pub fn pov_toggles(&self) -> PovToggles {
        PovToggles {
            shadow_ao: self.pov.toggles.shadow_ao,
            detail_normals: self.pov.toggles.detail_normals,
            water: self.pov.toggles.water,
        }
    }

    /// Current copyable POV state.
    #[must_use]
    pub fn pov_state(&self) -> PovStateSnapshot {
        self.pov.snapshot()
    }

    /// Configure an initial render scale from a platform/resource-tier choice.
    pub fn set_pov_render_scale(&mut self, scale: f32) {
        if scale.is_finite() {
            self.pov.render_scale = scale.clamp(0.25, 1.0);
            self.dirty.pov = true;
            self.dirty.panel = true;
        }
    }

    /// Atomically degrade unsupported/lost POV to Map without changing world
    /// state. Re-enabling support does not switch presentation automatically.
    fn set_pov_supported(&mut self, supported: bool, reason: Option<ViewerWarning>) {
        self.pov.supported = supported;
        if !supported && self.layout.mode != PresentationMode::Map {
            self.layout.mode = PresentationMode::Map;
            self.layout.focused = ViewKind::Map;
        }
        if let Some(warning) = reason {
            self.effects.push(ViewerEffect::ReportWarning(warning));
        }
        self.dirty = PresentationDirty {
            map: true,
            pov: true,
            panel: true,
        };
    }

    /// Enqueue keyboard/pointer/button actions in one total order.
    pub fn enqueue_action(&mut self, action: ViewerAction) {
        self.actions.push_back(action);
    }

    /// Enqueue a typed asynchronous response. It is not applied until the next
    /// tick, before any discrete action queued for that tick.
    pub fn enqueue_service_response(&mut self, response: ServiceResponse) {
        self.service_inputs
            .push_back(ServiceInput::Response(response));
    }

    /// Enqueue a sequenced asynchronous capability notification. Like service
    /// responses, it is reduced only at the beginning of the next tick.
    pub fn enqueue_service_notification(&mut self, notification: ServiceNotification) {
        self.service_inputs
            .push_back(ServiceInput::Notification(notification));
    }

    /// Whether a correlated service operation has not produced a response.
    #[must_use]
    pub fn request_pending(&self, request_id: ServiceRequestId) -> bool {
        self.pending.contains_key(&request_id)
    }

    /// Most recent request whose valid success response was reduced.
    #[must_use]
    pub const fn last_completed_request(&self) -> Option<ServiceRequestId> {
        self.last_completed_request
    }

    /// Apply service responses, actions, continuous input, and exactly one
    /// world update in the contract order from section 5.3.
    pub fn tick(
        &mut self,
        tick: TickInput,
        executor: &dyn TaskExecutor,
        hook: &mut dyn WorldTickHook,
        ground: &dyn PovGroundSampler,
    ) -> TickOutput {
        self.drain_service_inputs();
        while let Some(action) = self.actions.pop_front() {
            self.apply_action(action);
        }
        self.prepare_pov_camera(ground);

        let dt = clamped_dt(tick.dt_seconds);
        self.apply_continuous_input(tick.input, dt, ground);
        let world_output = self.world.update(executor, hook);
        self.frame = self.frame.saturating_add(1);

        if stats_changed_presentation(world_output.stats) || world_output.travel != 0.0 {
            self.dirty.map = true;
            self.dirty.pov = true;
            self.dirty.panel = true;
        }
        let dirty = self.dirty;
        let continuous = has_continuous_intent(tick.input);
        let pov_visible = self.layout.mode != PresentationMode::Map;
        let needs_frame = pov_visible
            || continuous
            || self.world.map().jobs_in_flight() > 0
            || stats_has_deferred_work(world_output.stats)
            || dirty.any();
        let effects = std::mem::take(&mut self.effects);
        self.dirty = PresentationDirty::default();

        self.tick_output(world_output, tick.platform, effects, dirty, needs_frame)
    }

    fn tick_output(
        &self,
        world: WorldTickOutput,
        platform: PlatformTelemetry,
        effects: Vec<ViewerEffect>,
        dirty: PresentationDirty,
        needs_frame: bool,
    ) -> TickOutput {
        TickOutput {
            frame: self.frame,
            update_serial: world.update_serial,
            mode: self.layout.mode,
            focused: self.layout.focused,
            traveler: world.traveler,
            travel: world.travel,
            stats: world.stats,
            map: self.map,
            pov: self.pov.snapshot(),
            effects,
            platform,
            dirty,
            needs_frame,
        }
    }

    /// The sole reducer for discrete semantic viewer actions.
    pub fn apply_action(&mut self, action: ViewerAction) {
        self.dirty.panel = true;
        if let Some(reduction) = reduce_layout_action(&mut self.layout, self.pov.supported, action)
        {
            if reduction.unsupported_pov {
                self.report_warning(
                    "pov-unsupported",
                    "POV is unavailable; presentation remains Map.",
                    Severity::Warning,
                );
            }
            if reduction.sets_presentation
                && self.layout.mode != PresentationMode::Map
                && self.pov.initialized
                && self.pov.camera.walk
            {
                self.pov.snap_walk_ground = true;
            }
            self.dirty.map = true;
            self.dirty.pov = true;
            return;
        }
        match action {
            ViewerAction::SetPresentation(_)
            | ViewerAction::TogglePrimaryView
            | ViewerAction::FocusView(_)
            | ViewerAction::SetSplitRatio(_) => unreachable!("layout actions returned above"),
            ViewerAction::NudgePossibility { domain, direction } => {
                let step = match direction {
                    NudgeDirection::Up => POSSIBILITY_NUDGE_STEP,
                    NudgeDirection::Down => -POSSIBILITY_NUDGE_STEP,
                };
                let value = &mut self.world.bias_mut()[domain.index()];
                *value = (*value + step).clamp(-1.0, 1.0);
                self.dirty.map = true;
            }
            ViewerAction::ResetPossibilityBias => {
                *self.world.bias_mut() = [0.0; POSSIBILITY_DIMS];
                self.dirty.map = true;
            }
            ViewerAction::DropAnchor(kind) => {
                let mask = domain_mask(&[
                    PossibilityDomain::Climate,
                    PossibilityDomain::Hydrology,
                    PossibilityDomain::Ecology,
                ]);
                let position = self.world.traveler().position;
                self.world.anchors_mut().push(Anchor {
                    world_pos: position,
                    target: bound_target(mask, 1.0),
                    mask,
                    kind,
                    strength: ANCHOR_STRENGTH,
                    falloff_radius: ANCHOR_RADIUS,
                    source: AnchorSource::Manual,
                });
                self.dirty.map = true;
            }
            ViewerAction::CaptureAnchor => {
                let position = self.world.traveler().position;
                if let Some(anchor) = self.world.map().capture_at(
                    position,
                    self.capture.category.mask_bit(),
                    self.capture.polarity,
                    ANCHOR_STRENGTH,
                    ANCHOR_RADIUS,
                ) {
                    self.world.anchors_mut().push(anchor);
                    self.dirty.map = true;
                } else {
                    self.report_warning(
                        "capture-unavailable",
                        "Nothing is capturable under the traveler yet.",
                        Severity::Info,
                    );
                }
            }
            ViewerAction::CycleCaptureCategory => {
                let index = TraitCategory::ALL
                    .iter()
                    .position(|category| *category == self.capture.category)
                    .unwrap_or(0);
                self.capture.category = TraitCategory::ALL[(index + 1) % TraitCategory::ALL.len()];
            }
            ViewerAction::ToggleCapturePolarity => {
                self.capture.polarity = match self.capture.polarity {
                    AnchorKind::Emphasize => AnchorKind::Suppress,
                    AnchorKind::Suppress => AnchorKind::Emphasize,
                };
            }
            ViewerAction::ClearAnchors => {
                self.world.anchors_mut().clear();
                self.dirty.map = true;
            }
            ViewerAction::ToggleTransitionMode => {
                self.world
                    .set_transition_mode(!self.world.transition_mode());
            }
            ViewerAction::SaveSession => {
                let request = self.session_write_request();
                self.effects
                    .push(ViewerEffect::PersistSession(Box::new(request)));
            }
            ViewerAction::LoadSession => {
                let id = self.begin_request(PendingRequest::LoadSession);
                self.effects.push(ViewerEffect::LoadSession(id));
            }
            ViewerAction::RecordLastAnchor => self.request_discovery_write(),
            ViewerAction::SummonDiscoveries => {
                let id = self.begin_request(PendingRequest::LoadDiscoveries);
                self.effects.push(ViewerEffect::LoadDiscoveries(id));
            }
            ViewerAction::TogglePreserve => self.request_preserve_mutation(),
            ViewerAction::TogglePathTracking => {
                let enabled = !self.world.path_tracking();
                self.world.set_path_tracking(enabled);
                if !enabled {
                    self.world.discard_route_recording();
                }
                let request_id = self.begin_request(PendingRequest::ConfigurePathTracking);
                self.effects.push(ViewerEffect::ConfigurePathTracking {
                    request_id,
                    enabled,
                });
                self.dirty.map = true;
            }
            ViewerAction::ToggleRouteRecording => self.toggle_route_recording(),
            ViewerAction::ToggleRouteAttraction => {
                self.world
                    .set_route_attraction(!self.world.route_attraction());
            }
            ViewerAction::ClearRoutes => {
                // An unfinished expedition is transient and discarded now,
                // but tracker legs stay until the durable response reports
                // exactly which route records remain.
                self.world.discard_route_recording();
                let id = self.begin_request(PendingRequest::ClearRoutes);
                self.effects.push(ViewerEffect::ClearRoutes(id));
                self.dirty.map = true;
            }
            ViewerAction::CycleMapChannel => {
                self.map.channel = self.map.channel.next();
                self.dirty.map = true;
            }
            ViewerAction::SetMapChannel(channel) => {
                self.map.channel = channel;
                self.dirty.map = true;
            }
            ViewerAction::ToggleOverlay(overlay) => {
                let enabled = !self.map.overlays.enabled(overlay);
                self.map.overlays.set(overlay, enabled);
                self.dirty.map = true;
            }
            ViewerAction::ZoomIn => {
                self.map.zoom = (self.map.zoom.saturating_mul(2)).min(MAX_MAP_ZOOM);
                self.dirty.map = true;
            }
            ViewerAction::ZoomOut => {
                self.map.zoom = (self.map.zoom / 2).max(1);
                self.dirty.map = true;
            }
            ViewerAction::ToggleGpuCompose => {
                let backend = match self.map.backend {
                    MapBackend::Cpu => MapBackend::GpuAtlas,
                    MapBackend::GpuAtlas => MapBackend::Cpu,
                };
                self.select_map_backend(backend);
            }
            ViewerAction::ToggleRefinement => {
                self.map.refinement = !self.map.refinement;
                self.dirty.map = true;
            }
            ViewerAction::ToggleWalk => {
                let walk = !self.pov.camera.walk;
                self.pov.camera.walk = walk;
                self.pov.snap_walk_ground = walk;
                self.dirty.pov = true;
            }
            ViewerAction::TogglePovShadowAo => {
                self.pov.toggles.shadow_ao = !self.pov.toggles.shadow_ao;
                self.dirty.pov = true;
            }
            ViewerAction::TogglePovDetailNormals => {
                self.pov.toggles.detail_normals = !self.pov.toggles.detail_normals;
                self.dirty.pov = true;
            }
            ViewerAction::TogglePovWater => {
                self.pov.toggles.water = !self.pov.toggles.water;
                self.dirty.pov = true;
            }
            ViewerAction::SetPovRenderScale(scale) => self.set_pov_render_scale(scale),
            ViewerAction::SetResourceTier(tier) => {
                self.effects.push(ViewerEffect::ConfigureResourceTier(tier));
            }
            ViewerAction::RequestTierBenchmark => {
                self.effects.push(ViewerEffect::RunTierBenchmark);
            }
            ViewerAction::SetWorkerBackend(backend) => {
                self.effects
                    .push(ViewerEffect::ConfigureWorkerBackend(backend));
            }
            ViewerAction::CancelSupersededJobs => {
                self.effects.push(ViewerEffect::CancelSupersededJobs);
            }
            ViewerAction::SetMapBackend(backend) => self.select_map_backend(backend),
            ViewerAction::SetStorageEnabled(enabled) => {
                self.effects
                    .push(ViewerEffect::ConfigureStorage { enabled });
            }
            ViewerAction::ResetLocalVault => {
                self.effects.push(ViewerEffect::ResetLocalVault);
            }
            ViewerAction::RequestAtlasImport => {
                let id = self.begin_request(PendingRequest::AtlasImport);
                self.effects.push(ViewerEffect::OpenAtlasImport(id));
            }
            ViewerAction::RequestAtlasExport => {
                let id = self.begin_request(PendingRequest::AtlasExport);
                self.effects.push(ViewerEffect::DownloadAtlasBundle(id));
            }
            ViewerAction::RequestDebugDump => {
                let id = self.begin_request(PendingRequest::DebugCapture);
                self.effects
                    .push(ViewerEffect::WriteDebugCapture(DebugCaptureRequest {
                        request_id: id,
                        mode: self.layout.mode,
                        focused: self.layout.focused,
                    }));
            }
            ViewerAction::RequestExit => self.effects.push(ViewerEffect::Exit),
        }
    }

    fn preview_layout(&self) -> (ViewLayout, bool) {
        let mut layout = self.layout;
        let mut pov_supported = self.pov.supported;
        let mut sequence = self.last_response_sequence;
        for input in &self.service_inputs {
            let next = match input {
                ServiceInput::Response(response) => response.sequence,
                ServiceInput::Notification(notification) => notification.sequence(),
            };
            if sequence.is_some_and(|last| next <= last) {
                continue;
            }
            sequence = Some(next);
            if let ServiceInput::Notification(ServiceNotification::PovAvailability {
                supported,
                ..
            }) = input
            {
                pov_supported = *supported;
                if !*supported {
                    layout.mode = PresentationMode::Map;
                    layout.focused = ViewKind::Map;
                }
            }
        }
        for &action in &self.actions {
            let _ = reduce_layout_action(&mut layout, pov_supported, action);
        }
        (layout, pov_supported)
    }

    fn select_map_backend(&mut self, backend: MapBackend) {
        self.map.backend = backend;
        self.effects.push(ViewerEffect::SelectMapBackend(backend));
        self.dirty.map = true;
    }

    fn session_write_request(&mut self) -> SessionWriteRequest {
        let request_id = self.begin_request(PendingRequest::SaveSession);
        let traveler = self.world.traveler();
        let runtime = session_runtime_record(
            self.world.map().config(),
            self.world.budget(),
            Some(self.world.tier()),
            self.world.path_tracking(),
            self.world.route_attraction(),
        );
        let regions = self
            .world
            .map()
            .iter_active()
            .map(|region| RegionSnapshotRecord {
                coord: region.coord,
                current: region.current.dims,
                target: region.target.dims,
                stability: region.stability,
                revision: region.revision,
            })
            .collect();
        let tracker = if self.world.path_tracking() {
            self.world.tracker_snapshot()
        } else {
            Default::default()
        };
        SessionWriteRequest {
            request_id,
            snapshot: SessionSnapshotOwnedInput {
                runtime,
                player: traveler.position,
                last_player: traveler.previous_position,
                bias: *self.world.bias(),
                transition_mode: self.world.transition_mode(),
                anchors: self
                    .world
                    .anchors()
                    .iter()
                    .map(AnchorSnapshot::from_anchor)
                    .collect(),
                regions,
                recorder: self.world.recorder_snapshot(),
                tracker,
            },
        }
    }

    fn request_discovery_write(&mut self) {
        let Some(anchor) = self.world.anchors().last().copied() else {
            self.report_warning(
                "discovery-no-anchor",
                "Capture or drop an anchor before recording a discovery.",
                Severity::Info,
            );
            return;
        };
        let coord = RegionCoord::from_world(anchor.world_pos.0, anchor.world_pos.1);
        let resolution = self.world.map().config().field_resolution;
        let (origin_x, origin_y) = coord.origin();
        let cell = REGION_SIZE / f64::from(resolution);
        let cell_x = (((anchor.world_pos.0 - origin_x) / cell) as u16).min(resolution - 1);
        let cell_y = (((anchor.world_pos.1 - origin_y) / cell) as u16).min(resolution - 1);
        let signature_seed = self
            .world
            .map()
            .cell_signature(coord, cell_x, cell_y)
            .map_or(0, |signature| signature.seed());
        let request_id = self.begin_request(PendingRequest::WriteDiscovery);
        self.effects
            .push(ViewerEffect::WriteDiscovery(DiscoveryWriteRequest {
                request_id,
                anchor,
                signature_seed,
            }));
    }

    fn request_preserve_mutation(&mut self) {
        let traveler = self.world.traveler().position;
        let coord = RegionCoord::from_world(traveler.0, traveler.1);
        let mutation = if let Some((id, _)) = self.world.map().effective_preserve(coord) {
            PreserveMutation::Remove { id }
        } else {
            let regions: Vec<_> = self
                .world
                .map()
                .iter_active()
                .filter(|region| {
                    region.stability >= 1.0 && !self.world.map().is_overridden(region.coord)
                })
                .map(|region| (region.coord, PossibilitySignature::of(region.current)))
                .collect();
            if regions.is_empty() {
                self.report_warning(
                    "preserve-no-pinned-regions",
                    "No settled pinned regions are available to preserve.",
                    Severity::Info,
                );
                return;
            }
            PreserveMutation::Create { regions }
        };
        let request_id = self.begin_request(PendingRequest::MutatePreserve);
        self.effects
            .push(ViewerEffect::MutatePreserve(PreserveRequest {
                request_id,
                mutation,
            }));
    }

    fn toggle_route_recording(&mut self) {
        if !self.world.path_tracking() {
            self.report_warning(
                "route-path-tracking-disabled",
                "Enable path tracking before recording a route.",
                Severity::Info,
            );
            return;
        }
        if !self.world.route_recording() {
            self.world.start_route_recording();
            return;
        }
        let Some((nodes, discoveries)) = self.world.finish_route_recording() else {
            return;
        };
        if nodes.len() < 2 {
            self.report_warning(
                "route-too-short",
                "The recorded route is too short and was discarded.",
                Severity::Info,
            );
            return;
        }
        let request_id = self.begin_request(PendingRequest::WriteRoute);
        self.effects
            .push(ViewerEffect::WriteRoute(RouteWriteRequest {
                request_id,
                nodes,
                discoveries,
            }));
    }

    fn prepare_pov_camera(&mut self, ground: &dyn PovGroundSampler) {
        if self.layout.mode != PresentationMode::Map && !self.pov.initialized {
            let traveler = self.world.traveler().position;
            let sample = ground.sample_ground(self.world.map(), traveler);
            self.pov.camera.enter_at(traveler, sample.height);
            self.pov.initialized = true;
            if self.pov.camera.walk {
                self.pov.camera.snap_to_ground(sample.height);
            }
            self.dirty.pov = true;
        }
        if self.pov.snap_walk_ground && self.pov.initialized && self.pov.camera.walk {
            let position = (self.pov.camera.pos.x, self.pov.camera.pos.y);
            let sample = ground.sample_ground(self.world.map(), position);
            self.pov.camera.snap_to_ground(sample.height);
            self.pov.snap_walk_ground = false;
            self.dirty.pov = true;
        } else if !self.pov.camera.walk {
            self.pov.snap_walk_ground = false;
        }
    }

    fn apply_continuous_input(
        &mut self,
        input: InputFrame,
        dt: f64,
        ground: &dyn PovGroundSampler,
    ) {
        match self.layout.focused {
            ViewKind::Map => {
                if let Some((dx, dy)) = input.map_movement_delta(MAP_MOVEMENT_SPEED, dt) {
                    let position = self.world.traveler().position;
                    self.world
                        .set_traveler_position((position.0 + dx, position.1 + dy));
                    if self.pov.initialized {
                        self.pov.camera.pos.x += dx;
                        self.pov.camera.pos.y += dy;
                        if self.pov.camera.walk {
                            let position = (self.pov.camera.pos.x, self.pov.camera.pos.y);
                            let sample = ground.sample_ground(self.world.map(), position);
                            self.pov
                                .camera
                                .follow_ground(sample.height + EYE_HEIGHT, dt);
                        }
                    }
                    self.dirty.map = true;
                    self.dirty.pov = true;
                }
            }
            ViewKind::Pov => {
                if !self.pov.initialized {
                    return;
                }
                self.pov
                    .camera
                    .look(input.look_delta[0], input.look_delta[1]);
                for _ in 0..input.wheel_steps.max(0) {
                    self.pov.camera.scroll_speed(true);
                }
                for _ in 0..input.wheel_steps.saturating_neg().max(0) {
                    self.pov.camera.scroll_speed(false);
                }

                let strafe = f64::from(input.pov_axis[0]);
                let forward = f64::from(input.pov_axis[1]);
                let vertical = if self.pov.camera.walk {
                    0.0
                } else {
                    f64::from(input.pov_axis[2])
                };
                let mut movement = if self.pov.camera.walk {
                    self.pov.camera.walk_forward() * forward
                } else {
                    self.pov.camera.forward() * forward
                };
                movement += self.pov.camera.right() * strafe;
                movement.z += vertical;
                if movement.length_squared() > 0.0 {
                    let speed = if self.pov.camera.walk {
                        self.pov.camera.walk_speed
                    } else {
                        self.pov.camera.speed
                    };
                    self.pov.camera.pos += movement.normalize() * (speed * dt);
                }
                if self.pov.camera.walk {
                    let position = (self.pov.camera.pos.x, self.pov.camera.pos.y);
                    let sample = ground.sample_ground(self.world.map(), position);
                    self.pov
                        .camera
                        .follow_ground(sample.height + EYE_HEIGHT, dt);
                }
                self.world
                    .set_traveler_position((self.pov.camera.pos.x, self.pov.camera.pos.y));
                if input.look_delta != [0.0; 2]
                    || input.wheel_steps != 0
                    || input.pov_axis != [0; 3]
                {
                    self.dirty.pov = true;
                    self.dirty.map = true;
                }
            }
        }
    }

    fn begin_request(&mut self, kind: PendingRequest) -> ServiceRequestId {
        let id = ServiceRequestId(self.next_request_id);
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.pending.insert(id, kind);
        id
    }

    fn drain_service_inputs(&mut self) {
        while let Some(input) = self.service_inputs.pop_front() {
            let sequence = match &input {
                ServiceInput::Response(response) => response.sequence,
                ServiceInput::Notification(notification) => notification.sequence(),
            };
            if self
                .last_response_sequence
                .is_some_and(|last| sequence <= last)
            {
                self.report_warning(
                    "service-response-sequence",
                    "A stale or repeated service input was ignored.",
                    Severity::Warning,
                );
                continue;
            }
            self.last_response_sequence = Some(sequence);
            match input {
                ServiceInput::Response(response) => self.apply_service_response(response),
                ServiceInput::Notification(notification) => {
                    self.apply_service_notification(notification);
                }
            }
        }
    }

    fn apply_service_response(&mut self, response: ServiceResponse) {
        let Some(&kind) = self.pending.get(&response.request_id) else {
            self.report_warning(
                "service-response-request",
                "A service response referenced no pending request.",
                Severity::Warning,
            );
            return;
        };
        if !service_result_matches(kind, &response.result) {
            self.report_warning(
                "service-response-type",
                "A service response did not match its pending request.",
                Severity::Warning,
            );
            return;
        }
        self.pending.remove(&response.request_id);
        if self.apply_service_result(kind, response.result) {
            self.last_completed_request = Some(response.request_id);
        }
    }

    fn apply_service_notification(&mut self, notification: ServiceNotification) {
        match notification {
            ServiceNotification::PovAvailability {
                supported, reason, ..
            } => self.set_pov_supported(supported, reason),
        }
    }

    fn apply_service_result(
        &mut self,
        kind: PendingRequest,
        result: ServiceResponseResult,
    ) -> bool {
        match result {
            ServiceResponseResult::Failed(warning) => {
                self.effects.push(ViewerEffect::ReportWarning(warning));
                false
            }
            ServiceResponseResult::Completed
                if matches!(
                    kind,
                    PendingRequest::SaveSession
                        | PendingRequest::WriteRoute
                        | PendingRequest::ConfigurePathTracking
                        | PendingRequest::AtlasImport
                        | PendingRequest::AtlasExport
                        | PendingRequest::DebugCapture
                ) =>
            {
                true
            }
            ServiceResponseResult::SessionLoaded(loaded) if kind == PendingRequest::LoadSession => {
                self.restore_session(loaded);
                true
            }
            ServiceResponseResult::DiscoveriesLoaded(anchors)
                if kind == PendingRequest::LoadDiscoveries =>
            {
                self.world.anchors_mut().extend(anchors);
                self.dirty.map = true;
                true
            }
            ServiceResponseResult::DiscoveryWritten { id }
                if kind == PendingRequest::WriteDiscovery =>
            {
                self.world.attach_recorded_discovery(id);
                true
            }
            ServiceResponseResult::PreserveCreated { id, regions }
                if kind == PendingRequest::MutatePreserve =>
            {
                self.world.map_mut().apply_preserve_contributions(
                    regions
                        .into_iter()
                        .map(|(coord, signature)| (id, coord, signature)),
                );
                self.dirty.map = true;
                true
            }
            ServiceResponseResult::PreserveRemoved { id, regions }
                if kind == PendingRequest::MutatePreserve =>
            {
                for coord in regions {
                    self.world.map_mut().remove_preserve_contribution(id, coord);
                }
                self.dirty.map = true;
                true
            }
            ServiceResponseResult::RoutesCleared {
                remaining_route_ids,
                warning,
            } if kind == PendingRequest::ClearRoutes => {
                self.world.retain_route_tracking(&remaining_route_ids);
                if let Some(warning) = warning {
                    self.effects.push(ViewerEffect::ReportWarning(warning));
                }
                self.dirty.map = true;
                true
            }
            _ => {
                self.report_warning(
                    "service-response-type",
                    "A service response did not match its pending request.",
                    Severity::Warning,
                );
                false
            }
        }
    }

    fn restore_session(&mut self, loaded: LoadedSession) {
        let preserve_contributions = loaded.preserve_contributions;
        let snapshot = loaded.snapshot;
        let config = loaded.stream_config.unwrap_or(*self.world.map().config());
        *self.world.map_mut() = RegionMap::new(config);
        apply_session_regions(self.world.map_mut(), &snapshot);
        self.world
            .map_mut()
            .apply_preserve_contributions(preserve_contributions);
        self.world
            .restore_traveler(snapshot.player, snapshot.player);
        *self.world.bias_mut() = snapshot.bias;
        self.world.set_transition_mode(snapshot.transition_mode);
        *self.world.anchors_mut() = snapshot
            .anchors
            .iter()
            .map(world_core::AnchorSnapshot::to_anchor)
            .collect();
        if loaded.restore_route_state {
            self.world.set_path_tracking(snapshot.runtime.path_tracking);
            self.world
                .set_route_attraction(snapshot.runtime.route_attraction);
            self.world
                .restore_route_state(snapshot.recorder, snapshot.tracker);
        } else {
            self.world.clear_route_state();
        }
        if self.pov.initialized {
            self.pov.camera.pos.x = snapshot.player.0;
            self.pov.camera.pos.y = snapshot.player.1;
            self.pov.snap_walk_ground = self.pov.camera.walk;
        }
        self.dirty = PresentationDirty {
            map: true,
            pov: true,
            panel: true,
        };
    }

    fn report_warning(&mut self, id: &'static str, message: &str, severity: Severity) {
        self.effects
            .push(ViewerEffect::ReportWarning(ViewerWarning {
                id,
                message: String::from(message),
                severity,
            }));
    }
}

impl Default for ViewerController {
    fn default() -> Self {
        Self::new(ExplorationWorld::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LayoutReduction {
    unsupported_pov: bool,
    sets_presentation: bool,
}

fn reduce_layout_action(
    layout: &mut ViewLayout,
    pov_supported: bool,
    action: ViewerAction,
) -> Option<LayoutReduction> {
    let mut reduction = LayoutReduction {
        unsupported_pov: false,
        sets_presentation: false,
    };
    let requested = match action {
        ViewerAction::SetPresentation(mode) => {
            reduction.sets_presentation = true;
            mode
        }
        ViewerAction::TogglePrimaryView => match layout.mode {
            PresentationMode::Map => {
                reduction.sets_presentation = true;
                PresentationMode::Pov
            }
            PresentationMode::Pov => {
                reduction.sets_presentation = true;
                PresentationMode::Map
            }
            PresentationMode::Split => {
                layout.focused = other_view(layout.focused);
                return Some(reduction);
            }
        },
        ViewerAction::FocusView(view) => {
            if view_is_visible(layout.mode, view) {
                layout.focused = view;
            }
            return Some(reduction);
        }
        ViewerAction::SetSplitRatio(ratio) => {
            if ratio.is_finite() {
                layout.split_ratio = ratio.clamp(0.1, 0.9);
            }
            return Some(reduction);
        }
        _ => return None,
    };

    let mode = if requested != PresentationMode::Map && !pov_supported {
        reduction.unsupported_pov = true;
        PresentationMode::Map
    } else {
        requested
    };
    layout.mode = mode;
    match mode {
        PresentationMode::Map => layout.focused = ViewKind::Map,
        PresentationMode::Pov => layout.focused = ViewKind::Pov,
        PresentationMode::Split => {}
    }
    Some(reduction)
}

fn clamped_dt(dt: f64) -> f64 {
    if dt.is_finite() {
        dt.clamp(0.0, MAX_TICK_SECONDS)
    } else {
        0.0
    }
}

fn stats_changed_presentation(stats: FrameStats) -> bool {
    stats.loaded != 0
        || stats.evicted != 0
        || stats.converged != 0
        || stats.layers_regenerated != 0
        || stats.macro_jobs != 0
        || stats.rosters_built != 0
        || stats.authoritative_organisms_realized != 0
        || stats.organisms_realized != 0
        || stats.jobs_cancelled != 0
        || stats.results_dropped != 0
        || stats.jobs_failed != 0
        || stats.evicted_for_capacity != 0
}

fn stats_has_deferred_work(stats: FrameStats) -> bool {
    stats.deferred_loads != 0
        || stats.deferred_converges != 0
        || stats.deferred_regens != 0
        || stats.retarget_deferred != 0
}

fn service_result_matches(kind: PendingRequest, result: &ServiceResponseResult) -> bool {
    match result {
        ServiceResponseResult::Failed(_) => true,
        ServiceResponseResult::Completed => matches!(
            kind,
            PendingRequest::SaveSession
                | PendingRequest::WriteRoute
                | PendingRequest::ConfigurePathTracking
                | PendingRequest::AtlasImport
                | PendingRequest::AtlasExport
                | PendingRequest::DebugCapture
        ),
        ServiceResponseResult::SessionLoaded(_) => kind == PendingRequest::LoadSession,
        ServiceResponseResult::DiscoveriesLoaded(_) => kind == PendingRequest::LoadDiscoveries,
        ServiceResponseResult::DiscoveryWritten { .. } => kind == PendingRequest::WriteDiscovery,
        ServiceResponseResult::PreserveCreated { .. }
        | ServiceResponseResult::PreserveRemoved { .. } => kind == PendingRequest::MutatePreserve,
        ServiceResponseResult::RoutesCleared { .. } => kind == PendingRequest::ClearRoutes,
    }
}

const fn other_view(view: ViewKind) -> ViewKind {
    match view {
        ViewKind::Map => ViewKind::Pov,
        ViewKind::Pov => ViewKind::Map,
    }
}

const fn view_is_visible(mode: PresentationMode, view: ViewKind) -> bool {
    matches!(mode, PresentationMode::Split)
        || matches!((mode, view), (PresentationMode::Map, ViewKind::Map))
        || matches!((mode, view), (PresentationMode::Pov, ViewKind::Pov))
}

fn has_continuous_intent(input: InputFrame) -> bool {
    input.map_axis != [0; 2]
        || input.pov_axis != [0; 3]
        || input.look_delta != [0.0; 2]
        || input.wheel_steps != 0
        || input.primary_drag
        || input
            .controller_axes
            .iter()
            .any(|value| value.abs() > f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use world_core::{
        LegacyTargetPolicy, PossibilityVector, RegionSnapshotRecord, RouteRecorderSnapshot,
        RouteTrackerLegSnapshot, RouteTrackerSnapshot, SessionRuntimeRecord, SessionTierRecord,
    };
    use world_runtime::{Budget, InlineExecutor, ResourceTier, StreamConfig};

    use super::*;
    use crate::input::{ButtonPhase, Modifiers, NormalizedInputEvent, PhysicalKey};
    use crate::world::NoopWorldTickHook;

    #[derive(Debug, Default)]
    struct CountingHook {
        before: usize,
        after: usize,
        positions: Vec<(f64, f64)>,
    }

    impl WorldTickHook for CountingHook {
        fn before_world_update(
            &mut self,
            input: crate::world::WorldPreUpdate<'_>,
        ) -> crate::world::WorldServiceInput {
            self.before += 1;
            self.positions.push(input.traveler);
            crate::world::WorldServiceInput::default()
        }

        fn after_world_update(&mut self, _output: crate::world::WorldPostUpdate<'_>) {
            self.after += 1;
        }
    }

    fn controller_without_pov() -> ViewerController {
        let config = StreamConfig {
            near_radius: 0.0,
            far_radius: 0.0,
            load_radius: 0.0,
            unload_radius: 1.0,
            ..StreamConfig::default()
        };
        ViewerController::new(ExplorationWorld::with_runtime(
            config,
            Budget::unlimited(),
            ResourceTier::Low,
        ))
    }

    fn controller() -> ViewerController {
        let mut controller = controller_without_pov();
        controller.set_pov_supported(true, None);
        controller
    }

    fn flat_ground(_map: &RegionMap, _position: (f64, f64)) -> GroundSample {
        GroundSample {
            height: 10.0,
            mesh_resident: true,
        }
    }

    fn sloped_ground(_map: &RegionMap, position: (f64, f64)) -> GroundSample {
        GroundSample {
            height: 10.0 + position.0 * 0.01 + position.1 * 0.02,
            mesh_resident: true,
        }
    }

    fn tracker_snapshot(ids: &[u64]) -> RouteTrackerSnapshot {
        RouteTrackerSnapshot {
            legs: ids
                .iter()
                .map(|&route_id| RouteTrackerLegSnapshot {
                    route_id,
                    visited_nodes: vec![0],
                })
                .collect(),
        }
    }

    fn tick(controller: &mut ViewerController, input: InputFrame, dt: f64) -> TickOutput {
        controller.tick(
            TickInput {
                dt_seconds: dt,
                input,
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut NoopWorldTickHook,
            &flat_ground,
        )
    }

    #[test]
    fn map_pov_and_split_mode_focus_reduction_follows_one_transition_table() {
        let cases = [
            (
                ViewLayout::default(),
                ViewerAction::SetPresentation(PresentationMode::Split),
                PresentationMode::Split,
                ViewKind::Map,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Pov,
                    focused: ViewKind::Pov,
                    split_ratio: 0.5,
                },
                ViewerAction::SetPresentation(PresentationMode::Split),
                PresentationMode::Split,
                ViewKind::Pov,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Map,
                    split_ratio: 0.5,
                },
                ViewerAction::TogglePrimaryView,
                PresentationMode::Split,
                ViewKind::Pov,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Pov,
                    split_ratio: 0.5,
                },
                ViewerAction::TogglePrimaryView,
                PresentationMode::Split,
                ViewKind::Map,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Map,
                    split_ratio: 0.5,
                },
                ViewerAction::FocusView(ViewKind::Pov),
                PresentationMode::Split,
                ViewKind::Pov,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Pov,
                    split_ratio: 0.5,
                },
                ViewerAction::SetPresentation(PresentationMode::Map),
                PresentationMode::Map,
                ViewKind::Map,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Split,
                    focused: ViewKind::Map,
                    split_ratio: 0.5,
                },
                ViewerAction::SetPresentation(PresentationMode::Pov),
                PresentationMode::Pov,
                ViewKind::Pov,
            ),
            (
                ViewLayout::default(),
                ViewerAction::FocusView(ViewKind::Pov),
                PresentationMode::Map,
                ViewKind::Map,
            ),
            (
                ViewLayout::default(),
                ViewerAction::TogglePrimaryView,
                PresentationMode::Pov,
                ViewKind::Pov,
            ),
            (
                ViewLayout {
                    mode: PresentationMode::Pov,
                    focused: ViewKind::Pov,
                    split_ratio: 0.5,
                },
                ViewerAction::TogglePrimaryView,
                PresentationMode::Map,
                ViewKind::Map,
            ),
        ];

        for (mut layout, action, mode, focused) in cases {
            let ratio = layout.split_ratio;
            let reduction = reduce_layout_action(&mut layout, true, action)
                .expect("table contains only layout actions");
            assert!(!reduction.unsupported_pov);
            assert_eq!((layout.mode, layout.focused), (mode, focused));
            assert_eq!(layout.split_ratio, ratio);
        }
    }

    #[test]
    fn diagonal_map_movement_preserves_bits_and_clamps_dt() {
        let mut controller = controller();
        let output = tick(
            &mut controller,
            InputFrame {
                map_axis: [1, 1],
                ..InputFrame::default()
            },
            1.0,
        );
        let expected = MAP_MOVEMENT_SPEED * MAX_TICK_SECONDS / f64::sqrt(2.0);
        assert_eq!(output.traveler.0.to_bits(), expected.to_bits());
        assert_eq!(output.traveler.1.to_bits(), expected.to_bits());
        assert_eq!(
            output.travel.to_bits(),
            f64::hypot(expected, expected).to_bits()
        );
    }

    #[test]
    fn map_sprint_preserves_the_four_x_speed_contract() {
        let mut controller = controller();
        let output = tick(
            &mut controller,
            InputFrame {
                map_axis: [1, 0],
                sprint: true,
                ..InputFrame::default()
            },
            0.1,
        );
        assert_eq!(output.traveler, (200.0, 0.0));
        assert_eq!(output.travel, 200.0);
    }

    #[test]
    fn pov_fly_vertical_wheel_speed_and_dt_clamp_preserve_contract() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        let initial = tick(&mut controller, InputFrame::default(), 0.0);
        let output = tick(
            &mut controller,
            InputFrame {
                pov_axis: [0, 0, 1],
                wheel_steps: 1,
                ..InputFrame::default()
            },
            5.0,
        );
        assert_eq!(output.pov.fly_speed, 60.0);
        assert_eq!(output.pov.position[2] - initial.pov.position[2], 6.0);
        assert_eq!(output.traveler, (0.0, 0.0));
        assert_eq!(output.travel, 0.0);
    }

    #[test]
    fn pov_camera_traveler_and_hook_share_one_new_center_and_update() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        let mut hook = CountingHook::default();
        let output = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input: InputFrame {
                    pov_axis: [0, 1, 0],
                    ..InputFrame::default()
                },
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &flat_ground,
        );

        assert_eq!(hook.before, 1);
        assert_eq!(hook.after, 1);
        assert_eq!(output.update_serial, 1);
        assert_eq!(output.traveler, hook.positions[0]);
        assert_eq!(
            output.traveler,
            (output.pov.position[0], output.pov.position[1])
        );
        assert!((output.traveler.1 - 4.0).abs() < 1.0e-5);
        assert_eq!(controller.world().map().jobs_in_flight(), 0);
    }

    #[test]
    fn pov_focused_split_has_one_travel_value_and_one_world_update() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        controller.enqueue_action(ViewerAction::FocusView(ViewKind::Pov));
        let mut hook = CountingHook::default();
        let output = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input: InputFrame {
                    pov_axis: [0, 1, 0],
                    ..InputFrame::default()
                },
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &flat_ground,
        );

        assert_eq!(
            (output.mode, output.focused),
            (PresentationMode::Split, ViewKind::Pov)
        );
        assert_eq!((hook.before, hook.after), (1, 1));
        assert_eq!(output.update_serial, 1);
        assert_eq!(hook.positions, vec![output.traveler]);
        assert_eq!(
            output.traveler,
            (output.pov.position[0], output.pov.position[1])
        );
        assert!((output.travel - 4.0).abs() < 1.0e-5);
        assert!((output.traveler.1 - 4.0).abs() < 1.0e-5);
    }

    #[test]
    fn map_focused_split_translates_fly_camera_and_updates_world_once() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        let initialized = tick(&mut controller, InputFrame::default(), 0.0);
        controller.pov.camera.yaw = 0.37;
        controller.pov.camera.pitch = -0.21;
        let z = initialized.pov.position[2];

        let mut hook = CountingHook::default();
        let moved = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input: InputFrame {
                    map_axis: [1, 0],
                    ..InputFrame::default()
                },
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut hook,
            &flat_ground,
        );
        assert_eq!((hook.before, hook.after), (1, 1));
        assert_eq!(hook.positions, vec![moved.traveler]);
        assert_eq!(moved.traveler, (50.0, 0.0));
        assert_eq!(moved.travel, 50.0);
        assert_eq!(moved.pov.position[0], 50.0);
        assert_eq!(moved.pov.position[1], 0.0);
        assert_eq!(moved.pov.position[2], z);
        assert_eq!(moved.pov.yaw, 0.37);
        assert_eq!(moved.pov.pitch, -0.21);
    }

    #[test]
    fn map_focused_split_regrounds_walk_camera_after_translation() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        controller.enqueue_action(ViewerAction::ToggleWalk);
        let initial = controller.tick(
            TickInput {
                dt_seconds: 0.0,
                input: InputFrame::default(),
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut NoopWorldTickHook,
            &sloped_ground,
        );
        controller.pov.camera.yaw = 0.41;
        controller.pov.camera.pitch = -0.17;

        let moved = controller.tick(
            TickInput {
                dt_seconds: 0.1,
                input: InputFrame {
                    map_axis: [1, 0],
                    ..InputFrame::default()
                },
                platform: PlatformTelemetry::default(),
            },
            &InlineExecutor,
            &mut NoopWorldTickHook,
            &sloped_ground,
        );
        let target = sloped_ground(controller.world().map(), moved.traveler).height + EYE_HEIGHT;
        assert!(initial.pov.walk);
        assert_eq!(moved.traveler, (50.0, 0.0));
        assert_eq!(moved.pov.position[0], moved.traveler.0);
        assert_eq!(moved.pov.position[1], moved.traveler.1);
        assert!((moved.pov.position[2] - target).abs() < 1.0e-9);
        assert_eq!(moved.pov.yaw, 0.41);
        assert_eq!(moved.pov.pitch, -0.17);
    }

    #[test]
    fn pending_view_action_previews_context_for_same_frame_conflicts() {
        let mut controller = controller();
        let mut mapper = crate::input::InputMapper::default();
        controller.enqueue_action(ViewerAction::TogglePrimaryView);
        let preview = controller.input_context(true);
        assert_eq!(preview.mode, PresentationMode::Pov);
        assert_eq!(preview.focused, ViewKind::Pov);

        mapper.handle_event(
            NormalizedInputEvent::Key {
                key: PhysicalKey::KeyB,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            preview,
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::TogglePovShadowAo]
        );
        mapper.handle_event(
            NormalizedInputEvent::Key {
                key: PhysicalKey::KeyW,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            preview,
        );
        let frame = mapper.take_frame();
        assert_eq!(frame.map_axis, [0, 0]);
        assert_eq!(frame.pov_axis, [0, 1, 0]);
    }

    #[test]
    fn split_pointer_focus_is_previewed_before_a_same_batch_collision_key() {
        let mut controller = controller();
        let mut mapper = crate::input::InputMapper::default();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        let map_context = controller.input_context(true);
        assert_eq!(
            (map_context.mode, map_context.focused),
            (PresentationMode::Split, ViewKind::Map)
        );

        mapper.handle_event(
            NormalizedInputEvent::PointerButton {
                pointer: 1,
                button: crate::input::PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position: [75.0, 20.0],
                view: ViewKind::Pov,
            },
            map_context,
        );
        for action in mapper.drain_actions() {
            controller.enqueue_action(action);
        }
        let pov_context = controller.input_context(true);
        assert_eq!(
            (pov_context.mode, pov_context.focused),
            (PresentationMode::Split, ViewKind::Pov)
        );

        mapper.handle_event(
            NormalizedInputEvent::Key {
                key: PhysicalKey::KeyV,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers::default(),
            },
            pov_context,
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::TogglePovWater]
        );
    }

    #[test]
    fn walk_ignores_vertical_input_and_follows_ground() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        controller.enqueue_action(ViewerAction::ToggleWalk);
        let output = tick(
            &mut controller,
            InputFrame {
                pov_axis: [0, 1, 1],
                ..InputFrame::default()
            },
            0.1,
        );
        assert!(output.pov.walk);
        assert!((output.traveler.1 - 0.6).abs() < 1.0e-5);
        assert_eq!(output.pov.position[2], 10.0 + EYE_HEIGHT);
    }

    #[test]
    fn walk_wheel_adjusts_only_walk_speed() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        controller.enqueue_action(ViewerAction::ToggleWalk);
        let output = tick(
            &mut controller,
            InputFrame {
                wheel_steps: 1,
                ..InputFrame::default()
            },
            0.0,
        );
        assert_eq!(output.pov.walk_speed, 9.0);
        assert_eq!(output.pov.fly_speed, 40.0);
    }

    #[test]
    fn service_response_precedes_newer_action_and_load_has_zero_travel() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::LoadSession);
        let request_output = tick(&mut controller, InputFrame::default(), 0.0);
        let request_id = match request_output.effects.as_slice() {
            [ViewerEffect::LoadSession(id)] => *id,
            effects => panic!("unexpected effects: {effects:?}"),
        };
        assert!(controller.request_pending(request_id));
        assert_eq!(controller.last_completed_request(), None);

        let mut bias = [0.0; POSSIBILITY_DIMS];
        bias[PossibilityDomain::Climate.index()] = 0.8;
        let runtime = SessionRuntimeRecord {
            stream: world_runtime::stream_config_record(controller.world().map().config()),
            budget: world_runtime::budget_record(controller.world().budget()),
            tier: SessionTierRecord::Low,
            path_tracking: false,
            route_attraction: true,
            legacy_target_policy: LegacyTargetPolicy::ExactTargetStored,
        };
        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id,
            result: ServiceResponseResult::SessionLoaded(LoadedSession {
                snapshot: Box::new(SessionSnapshot {
                    runtime,
                    player: (20.0, -5.0),
                    last_player: (10.0, -5.0),
                    bias,
                    transition_mode: false,
                    anchors: Vec::new(),
                    regions: Vec::new(),
                    recorder: None,
                    tracker: RouteTrackerSnapshot::default(),
                    sequence: 1,
                }),
                stream_config: None,
                restore_route_state: true,
                preserve_contributions: Vec::new(),
            }),
        });
        controller.enqueue_action(ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Climate,
            direction: NudgeDirection::Up,
        });
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(
            controller.world().bias()[PossibilityDomain::Climate.index()],
            0.85
        );
        assert_eq!(output.traveler, (20.0, -5.0));
        assert_eq!(output.travel, 0.0);
        assert!(!controller.request_pending(request_id));
        assert_eq!(controller.last_completed_request(), Some(request_id));
    }

    #[test]
    fn request_does_not_claim_success_before_matching_ack() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SaveSession);
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        let id = match output.effects.as_slice() {
            [ViewerEffect::PersistSession(request)] => request.request_id,
            effects => panic!("unexpected effects: {effects:?}"),
        };
        assert!(controller.request_pending(id));
        assert_eq!(controller.last_completed_request(), None);

        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id: id,
            result: ServiceResponseResult::Completed,
        });
        let _ = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(!controller.request_pending(id));
        assert_eq!(controller.last_completed_request(), Some(id));
    }

    #[test]
    fn session_effect_captures_state_at_its_ordered_action_position() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SaveSession);
        controller.enqueue_action(ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Climate,
            direction: NudgeDirection::Up,
        });
        let output = tick(
            &mut controller,
            InputFrame {
                map_axis: [1, 0],
                ..InputFrame::default()
            },
            0.1,
        );
        let request = match output.effects.as_slice() {
            [ViewerEffect::PersistSession(request)] => request,
            effects => panic!("unexpected effects: {effects:?}"),
        };

        assert_eq!(
            request.snapshot.bias[PossibilityDomain::Climate.index()],
            0.0
        );
        assert_eq!(request.snapshot.player, (0.0, 0.0));
        assert_eq!(
            controller.world().bias()[PossibilityDomain::Climate.index()],
            POSSIBILITY_NUDGE_STEP
        );
        assert_eq!(output.traveler, (50.0, 0.0));
    }

    #[test]
    fn payloadless_ack_cannot_complete_a_payload_bearing_request() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SummonDiscoveries);
        let requested = tick(&mut controller, InputFrame::default(), 0.0);
        let request_id = match requested.effects.as_slice() {
            [ViewerEffect::LoadDiscoveries(id)] => *id,
            effects => panic!("unexpected effects: {effects:?}"),
        };

        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id,
            result: ServiceResponseResult::Completed,
        });
        let rejected = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(controller.request_pending(request_id));
        assert_eq!(controller.last_completed_request(), None);
        assert!(rejected.effects.iter().any(|effect| matches!(
            effect,
            ViewerEffect::ReportWarning(ViewerWarning {
                id: "service-response-type",
                ..
            })
        )));

        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(2),
            request_id,
            result: ServiceResponseResult::DiscoveriesLoaded(Vec::new()),
        });
        let _ = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(!controller.request_pending(request_id));
        assert_eq!(controller.last_completed_request(), Some(request_id));
    }

    #[test]
    fn path_disable_discards_only_recorder_and_keeps_tracker_legs() {
        let mut controller = controller();
        let tracker = tracker_snapshot(&[7]);
        controller.world.set_path_tracking(true);
        controller.world.restore_route_state(
            Some(RouteRecorderSnapshot {
                accumulated: 0.0,
                last_observed: None,
                nodes: Vec::new(),
                discoveries: Vec::new(),
            }),
            tracker.clone(),
        );
        controller.enqueue_action(ViewerAction::TogglePathTracking);
        let _ = tick(&mut controller, InputFrame::default(), 0.0);

        assert!(!controller.world().path_tracking());
        assert!(!controller.world().route_recording());
        assert_eq!(controller.world().tracker_snapshot(), tracker);
    }

    #[test]
    fn failed_route_clear_preserves_tracker_and_partial_clear_retains_remaining_ids() {
        let mut controller = controller();
        let tracker = tracker_snapshot(&[4, 9]);
        controller.world.restore_route_state(None, tracker.clone());
        controller.enqueue_action(ViewerAction::ClearRoutes);
        let request = tick(&mut controller, InputFrame::default(), 0.0);
        let request_id = match request.effects.as_slice() {
            [ViewerEffect::ClearRoutes(id)] => *id,
            effects => panic!("unexpected effects: {effects:?}"),
        };
        assert_eq!(controller.world().tracker_snapshot(), tracker);

        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id,
            result: ServiceResponseResult::Failed(ViewerWarning {
                id: "route-clear-failed",
                message: String::from("durable removal failed"),
                severity: Severity::Warning,
            }),
        });
        let _ = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(controller.world().tracker_snapshot(), tracker);

        controller.enqueue_action(ViewerAction::ClearRoutes);
        let request = tick(&mut controller, InputFrame::default(), 0.0);
        let request_id = match request.effects.as_slice() {
            [ViewerEffect::ClearRoutes(id)] => *id,
            effects => panic!("unexpected effects: {effects:?}"),
        };
        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(2),
            request_id,
            result: ServiceResponseResult::RoutesCleared {
                remaining_route_ids: vec![9],
                warning: Some(ViewerWarning {
                    id: "route-clear-partial",
                    message: String::from("one route remains"),
                    severity: Severity::Warning,
                }),
            },
        });
        let response = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(
            controller.world().tracker_snapshot(),
            tracker_snapshot(&[9])
        );
        assert!(response.effects.iter().any(|effect| matches!(
            effect,
            ViewerEffect::ReportWarning(ViewerWarning {
                id: "route-clear-partial",
                ..
            })
        )));
    }

    #[test]
    fn session_restore_applies_preserves_before_its_single_update() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::LoadSession);
        let request = tick(&mut controller, InputFrame::default(), 0.0);
        let request_id = match request.effects.as_slice() {
            [ViewerEffect::LoadSession(id)] => *id,
            effects => panic!("unexpected effects: {effects:?}"),
        };
        let coord = RegionCoord::new(0, 0);
        let current = PossibilityVector::neutral();
        let signature = PossibilitySignature::of(current);
        let runtime = SessionRuntimeRecord {
            stream: world_runtime::stream_config_record(controller.world().map().config()),
            budget: world_runtime::budget_record(controller.world().budget()),
            tier: SessionTierRecord::Low,
            path_tracking: false,
            route_attraction: true,
            legacy_target_policy: LegacyTargetPolicy::ExactTargetStored,
        };
        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id,
            result: ServiceResponseResult::SessionLoaded(LoadedSession {
                snapshot: Box::new(SessionSnapshot {
                    runtime,
                    player: (128.0, 128.0),
                    last_player: (0.0, 0.0),
                    bias: [0.0; POSSIBILITY_DIMS],
                    transition_mode: false,
                    anchors: Vec::new(),
                    regions: vec![RegionSnapshotRecord {
                        coord,
                        current: current.dims,
                        target: current.dims,
                        stability: 1.0,
                        revision: 3,
                    }],
                    recorder: None,
                    tracker: RouteTrackerSnapshot::default(),
                    sequence: 1,
                }),
                stream_config: None,
                restore_route_state: true,
                preserve_contributions: vec![(23, coord, signature)],
            }),
        });
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(output.travel, 0.0);
        assert_eq!(
            controller.world().map().effective_preserve(coord),
            Some((23, signature))
        );
    }

    #[test]
    fn stale_response_sequence_is_ignored() {
        let mut controller = controller();
        for _ in 0..2 {
            controller.enqueue_action(ViewerAction::SaveSession);
        }
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        let ids: Vec<_> = output
            .effects
            .iter()
            .filter_map(|effect| match effect {
                ViewerEffect::PersistSession(request) => Some(request.request_id),
                _ => None,
            })
            .collect();
        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(2),
            request_id: ids[0],
            result: ServiceResponseResult::Completed,
        });
        controller.enqueue_service_response(ServiceResponse {
            sequence: ServiceResponseSequence(1),
            request_id: ids[1],
            result: ServiceResponseResult::Completed,
        });
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(controller.last_completed_request(), Some(ids[0]));
        assert!(controller.request_pending(ids[1]));
        assert!(output.effects.iter().any(|effect| matches!(
            effect,
            ViewerEffect::ReportWarning(ViewerWarning {
                id: "service-response-sequence",
                ..
            })
        )));
    }

    #[test]
    fn unsupported_pov_and_split_fall_back_without_moving_world() {
        for requested in [PresentationMode::Pov, PresentationMode::Split] {
            let mut controller = controller_without_pov();
            controller.enqueue_action(ViewerAction::SetPresentation(requested));
            let output = tick(&mut controller, InputFrame::default(), 0.1);
            assert_eq!(output.mode, PresentationMode::Map);
            assert_eq!(output.focused, ViewKind::Map);
            assert_eq!(output.traveler, (0.0, 0.0));
            assert!(output.effects.iter().any(|effect| matches!(
                effect,
                ViewerEffect::ReportWarning(ViewerWarning {
                    id: "pov-unsupported",
                    ..
                })
            )));
        }
    }

    #[test]
    fn capability_loss_from_split_preserves_world_and_camera_then_focuses_map() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        controller.enqueue_action(ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Climate,
            direction: NudgeDirection::Up,
        });
        let before = tick(
            &mut controller,
            InputFrame {
                map_axis: [1, 0],
                ..InputFrame::default()
            },
            0.1,
        );
        let bias = *controller.world().bias();
        let camera = before.pov;

        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(1),
            supported: false,
            reason: Some(ViewerWarning {
                id: "renderer-device-loss",
                message: String::from("test device loss"),
                severity: Severity::Warning,
            }),
        });
        let preview = controller.input_context(true);
        assert_eq!(
            (preview.mode, preview.focused),
            (PresentationMode::Map, ViewKind::Map)
        );
        let output = tick(&mut controller, InputFrame::default(), 0.0);

        assert_eq!(
            (output.mode, output.focused),
            (PresentationMode::Map, ViewKind::Map)
        );
        assert!(!output.pov.supported);
        assert_eq!(output.traveler, before.traveler);
        assert_eq!(output.pov.position, camera.position);
        assert_eq!(
            (output.pov.yaw, output.pov.pitch),
            (camera.yaw, camera.pitch)
        );
        assert_eq!(controller.world().bias(), &bias);
        assert!(output.effects.iter().any(|effect| matches!(
            effect,
            ViewerEffect::ReportWarning(ViewerWarning {
                id: "renderer-device-loss",
                ..
            })
        )));
    }

    #[test]
    fn loss_tick_rejects_queued_split_before_later_capability_recovery() {
        let mut controller = controller();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        let entered = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(entered.mode, PresentationMode::Split);

        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(1),
            supported: false,
            reason: None,
        });
        // Models a presentation action already queued when the platform
        // observes device loss. Service inputs run first, so this loss tick
        // must reject it and produce Map/Map.
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        let lost = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(
            (lost.mode, lost.focused),
            (PresentationMode::Map, ViewKind::Map)
        );
        assert!(!lost.pov.supported);

        // Native advertises a successfully rebuilt device only after the
        // fallback tick. Capability recovery alone never changes presentation.
        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(2),
            supported: true,
            reason: None,
        });
        let recovered = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(
            (recovered.mode, recovered.focused),
            (PresentationMode::Map, ViewKind::Map)
        );
        assert!(recovered.pov.supported);

        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Split));
        let reentered = tick(&mut controller, InputFrame::default(), 0.0);
        assert_eq!(reentered.mode, PresentationMode::Split);
    }

    #[test]
    fn asynchronous_pov_capability_changes_only_at_tick_before_actions() {
        let mut controller = controller_without_pov();
        controller.enqueue_action(ViewerAction::SetPresentation(PresentationMode::Pov));
        controller.enqueue_service_notification(ServiceNotification::PovAvailability {
            sequence: ServiceResponseSequence(1),
            supported: true,
            reason: None,
        });

        assert!(!controller.pov_state().supported);
        assert_eq!(controller.layout().mode, PresentationMode::Map);
        let output = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(output.pov.supported);
        assert_eq!(output.mode, PresentationMode::Pov);
    }

    #[test]
    fn conservative_capabilities_and_idle_map_scheduler_are_truthful() {
        let config = StreamConfig {
            near_radius: 0.0,
            far_radius: 0.0,
            load_radius: 0.0,
            unload_radius: 1.0,
            ..StreamConfig::default()
        };
        let mut controller = ViewerController::new(ExplorationWorld::with_runtime(
            config,
            Budget::unlimited(),
            ResourceTier::Low,
        ));
        assert!(!controller.pov_state().supported);
        assert_eq!(controller.map_preferences().backend, MapBackend::Cpu);

        let first = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(first.needs_frame, "initial presentation must draw once");
        let settled = tick(&mut controller, InputFrame::default(), 0.0);
        assert!(!settled.dirty.any());
        assert!(!settled.needs_frame, "steady idle Map may sleep");

        let deferred = FrameStats {
            retarget_deferred: 1,
            ..FrameStats::default()
        };
        assert!(stats_has_deferred_work(deferred));
        assert!(!stats_changed_presentation(deferred));
    }
}
