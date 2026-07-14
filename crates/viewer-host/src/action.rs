//! Typed semantic actions, their stable metadata, and platform effects
//! (`native-web-alignment.md` sections 5.2 and 5.3).

use std::fmt;

use world_core::{AnchorKind, PossibilityDomain};
use world_runtime::ResourceTier;

use crate::layout::{PresentationMode, ViewKind};
use crate::map::{Channel, MapBackend, MapOverlay};
use crate::panel::ViewerWarning;

/// Direction of a possibility-domain nudge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NudgeDirection {
    /// Increase the domain bias.
    Up,
    /// Decrease the domain bias.
    Down,
}

/// Browser/native worker strategy selected by a typed control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerBackend {
    /// Run tasks inline.
    Inline,
    /// Use a worker pool without shared memory.
    Workers,
    /// Use a shared-memory worker pool.
    SharedWorkers,
}

/// Stable identifier for one semantic action variant.
///
/// Payload-bearing actions keep one id and decode their payload separately at
/// a platform boundary. For example, every exact map channel uses
/// `set-map-channel`; channel names never become a second command language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ActionId(&'static str);

impl ActionId {
    /// Declare an action id. IDs shipped in [`ACTION_DESCRIPTORS`] are the
    /// supported public surface.
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// Exact string used by platform controls and generated help.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Where an action is meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionScope {
    /// Independent of the currently focused pane.
    Global,
    /// Routed through whichever visible pane has focus.
    FocusedView,
    /// Top-down map only.
    Map,
    /// First-person view only.
    Pov,
}

/// Payload shape used to build exact controls at a platform boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionValueKind {
    /// A one-shot action with no payload.
    Pulse,
    /// An explicit boolean payload.
    Boolean,
    /// A bounded floating-point value.
    Scalar,
    /// One member of a documented enum.
    Choice,
    /// A typed payload containing more than one field.
    Structured,
}

/// Optional environment service or renderer feature required by an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewerCapability {
    /// A functioning POV renderer.
    Pov,
    /// GPU atlas map composition.
    GpuMap,
    /// Session persistence.
    SessionStorage,
    /// Discovery/preserve/route vault operations.
    Vault,
    /// Configurable worker execution.
    WorkerControl,
    /// Atlas bundle import/export.
    AtlasTransfer,
    /// Resource-tier benchmark support.
    Benchmark,
    /// Platform diagnostic capture.
    DebugCapture,
    /// Host application exit.
    Exit,
}

/// One source-of-truth description consumed by controls and help renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDescriptor {
    /// Stable action-variant id.
    pub id: ActionId,
    /// Short user-facing label.
    pub label: &'static str,
    /// View routing scope.
    pub scope: ActionScope,
    /// Expected payload shape.
    pub value_kind: ActionValueKind,
    /// Concise generated-help text.
    pub help: &'static str,
    /// Capabilities that a platform must expose before showing the action.
    pub capabilities: &'static [ViewerCapability],
    /// Stable ids in the default input binding registry.
    pub default_binding_ids: &'static [&'static str],
}

const NONE: &[ViewerCapability] = &[];
const POV: &[ViewerCapability] = &[ViewerCapability::Pov];
const GPU_MAP: &[ViewerCapability] = &[ViewerCapability::GpuMap];
const SESSION: &[ViewerCapability] = &[ViewerCapability::SessionStorage];
const VAULT: &[ViewerCapability] = &[ViewerCapability::Vault];
const WORKERS: &[ViewerCapability] = &[ViewerCapability::WorkerControl];
const ATLAS: &[ViewerCapability] = &[ViewerCapability::AtlasTransfer];
const BENCHMARK: &[ViewerCapability] = &[ViewerCapability::Benchmark];
const DEBUG_CAPTURE: &[ViewerCapability] = &[ViewerCapability::DebugCapture];
const EXIT: &[ViewerCapability] = &[ViewerCapability::Exit];

const NUDGE_BINDINGS: &[&str] = &[
    "map-digit1-up",
    "map-digit1-down",
    "map-digit2-up",
    "map-digit2-down",
    "map-digit3-up",
    "map-digit3-down",
    "map-digit4-up",
    "map-digit4-down",
    "map-digit5-up",
    "map-digit5-down",
    "map-digit6-up",
    "map-digit6-down",
    "map-digit7-up",
    "map-digit7-down",
    "map-digit8-up",
    "map-digit8-down",
];
const DROP_ANCHOR_BINDINGS: &[&str] = &["map-key-e", "map-key-q"];
const OVERLAY_BINDINGS: &[&str] = &[
    "map-key-f",
    "map-key-g",
    "map-key-n",
    "map-key-x",
    "map-key-m",
];
const ZOOM_IN_BINDINGS: &[&str] = &["map-wheel-positive"];
const ZOOM_OUT_BINDINGS: &[&str] = &["map-wheel-negative"];
const TOGGLE_VIEW_BINDINGS: &[&str] = &["single-tab"];
const FOCUS_VIEW_BINDINGS: &[&str] = &["split-tab", "split-primary-press"];

macro_rules! descriptor {
    ($id:literal, $label:literal, $scope:ident, $kind:ident, $help:literal, $caps:expr, $bindings:expr) => {
        ActionDescriptor {
            id: ActionId::new($id),
            label: $label,
            scope: ActionScope::$scope,
            value_kind: ActionValueKind::$kind,
            help: $help,
            capabilities: $caps,
            default_binding_ids: $bindings,
        }
    };
}

/// Stable descriptor registry for every [`ViewerAction`] variant.
pub const ACTION_DESCRIPTORS: &[ActionDescriptor] = &[
    descriptor!(
        "set-presentation",
        "Presentation",
        Global,
        Choice,
        "Show Map, POV, or Split.",
        NONE,
        &[]
    ),
    descriptor!(
        "toggle-primary-view",
        "Toggle view",
        FocusedView,
        Pulse,
        "Toggle Map/POV, or move Split focus.",
        NONE,
        TOGGLE_VIEW_BINDINGS
    ),
    descriptor!(
        "focus-view",
        "Focus view",
        FocusedView,
        Choice,
        "Route view-scoped input to a pane.",
        NONE,
        FOCUS_VIEW_BINDINGS
    ),
    descriptor!(
        "set-split-ratio",
        "Split ratio",
        Global,
        Scalar,
        "Set the Map share of Split presentation.",
        NONE,
        &[]
    ),
    descriptor!(
        "nudge-possibility",
        "Nudge possibility",
        Map,
        Structured,
        "Nudge one possibility domain up or down.",
        NONE,
        NUDGE_BINDINGS
    ),
    descriptor!(
        "reset-possibility-bias",
        "Reset possibility",
        Map,
        Pulse,
        "Reset manual possibility bias.",
        NONE,
        &["map-key-z"]
    ),
    descriptor!(
        "drop-anchor",
        "Drop anchor",
        Map,
        Choice,
        "Drop an Emphasize or Suppress manual anchor.",
        NONE,
        DROP_ANCHOR_BINDINGS
    ),
    descriptor!(
        "capture-anchor",
        "Capture anchor",
        Map,
        Pulse,
        "Capture traits under the traveler.",
        NONE,
        &["map-key-k"]
    ),
    descriptor!(
        "cycle-capture-category",
        "Capture category",
        Map,
        Pulse,
        "Cycle the captured trait category.",
        NONE,
        &["map-key-t"]
    ),
    descriptor!(
        "toggle-capture-polarity",
        "Capture polarity",
        Map,
        Pulse,
        "Toggle capture Emphasize/Suppress polarity.",
        NONE,
        &["map-key-y"]
    ),
    descriptor!(
        "clear-anchors",
        "Clear anchors",
        Map,
        Pulse,
        "Remove active anchors.",
        NONE,
        &["map-key-c"]
    ),
    descriptor!(
        "toggle-transition-mode",
        "Transition mode",
        Map,
        Pulse,
        "Toggle deliberate transition movement.",
        NONE,
        &["map-key-r"]
    ),
    descriptor!(
        "save-session",
        "Save session",
        Map,
        Pulse,
        "Persist the current session.",
        SESSION,
        &["map-key-o"]
    ),
    descriptor!(
        "load-session",
        "Load session",
        Map,
        Pulse,
        "Load the persisted session.",
        SESSION,
        &["map-key-l"]
    ),
    descriptor!(
        "record-last-anchor",
        "Record discovery",
        Map,
        Pulse,
        "Persist the last anchor as a discovery.",
        VAULT,
        &["map-key-b"]
    ),
    descriptor!(
        "summon-discoveries",
        "Summon discoveries",
        Map,
        Pulse,
        "Summon retained discoveries as anchors.",
        VAULT,
        &["map-key-i"]
    ),
    descriptor!(
        "toggle-preserve",
        "Toggle preserve",
        Map,
        Pulse,
        "Toggle a preserve at the traveler.",
        VAULT,
        &["map-key-p"]
    ),
    descriptor!(
        "toggle-path-tracking",
        "Path tracking",
        Map,
        Pulse,
        "Toggle path display and tracking.",
        VAULT,
        &["map-key-h"]
    ),
    descriptor!(
        "toggle-route-recording",
        "Route recording",
        Map,
        Pulse,
        "Toggle route recording.",
        VAULT,
        &["map-key-j"]
    ),
    descriptor!(
        "toggle-route-attraction",
        "Route attraction",
        Map,
        Pulse,
        "Toggle route attraction.",
        VAULT,
        &["map-key-u"]
    ),
    descriptor!(
        "clear-routes",
        "Clear routes",
        Map,
        Pulse,
        "Clear retained routes.",
        VAULT,
        &["map-delete"]
    ),
    descriptor!(
        "cycle-map-channel",
        "Map channel",
        Map,
        Pulse,
        "Cycle the visible map channel.",
        NONE,
        &["map-key-v"]
    ),
    descriptor!(
        "set-map-channel",
        "Set map channel",
        Map,
        Choice,
        "Select an exact map channel.",
        NONE,
        &[]
    ),
    descriptor!(
        "toggle-overlay",
        "Map overlay",
        Map,
        Structured,
        "Toggle one map overlay.",
        NONE,
        OVERLAY_BINDINGS
    ),
    descriptor!(
        "zoom-in",
        "Zoom in",
        Map,
        Pulse,
        "Increase map magnification.",
        NONE,
        ZOOM_IN_BINDINGS
    ),
    descriptor!(
        "zoom-out",
        "Zoom out",
        Map,
        Pulse,
        "Decrease map magnification.",
        NONE,
        ZOOM_OUT_BINDINGS
    ),
    descriptor!(
        "toggle-gpu-compose",
        "GPU compose",
        Map,
        Pulse,
        "Toggle GPU atlas map composition.",
        GPU_MAP,
        &["map-comma"]
    ),
    descriptor!(
        "toggle-refinement",
        "Map refinement",
        Map,
        Pulse,
        "Toggle presentation-only map refinement.",
        GPU_MAP,
        &["map-period"]
    ),
    descriptor!(
        "toggle-walk",
        "Walk/fly",
        Pov,
        Pulse,
        "Toggle POV walk/fly movement.",
        POV,
        &["pov-key-f"]
    ),
    descriptor!(
        "toggle-pov-shadow-ao",
        "POV shadow/AO",
        Pov,
        Pulse,
        "Toggle POV shadows and ambient occlusion.",
        POV,
        &["pov-key-b"]
    ),
    descriptor!(
        "toggle-pov-detail-normals",
        "POV detail normals",
        Pov,
        Pulse,
        "Toggle POV detail normals.",
        POV,
        &["pov-key-n"]
    ),
    descriptor!(
        "toggle-pov-water",
        "POV water",
        Pov,
        Pulse,
        "Toggle POV water passes.",
        POV,
        &["pov-key-v"]
    ),
    descriptor!(
        "set-pov-render-scale",
        "POV render scale",
        Pov,
        Scalar,
        "Set the internal POV render scale.",
        POV,
        &[]
    ),
    descriptor!(
        "set-resource-tier",
        "Resource tier",
        Global,
        Choice,
        "Select a resource tier.",
        NONE,
        &[]
    ),
    descriptor!(
        "request-tier-benchmark",
        "Tier benchmark",
        Global,
        Pulse,
        "Run the resource-tier benchmark.",
        BENCHMARK,
        &[]
    ),
    descriptor!(
        "set-worker-backend",
        "Worker backend",
        Global,
        Choice,
        "Select task execution strategy.",
        WORKERS,
        &[]
    ),
    descriptor!(
        "cancel-superseded-jobs",
        "Cancel stale jobs",
        Global,
        Pulse,
        "Cancel work superseded by newer requests.",
        WORKERS,
        &[]
    ),
    descriptor!(
        "set-map-backend",
        "Map backend",
        Map,
        Choice,
        "Select CPU or GPU atlas map rendering.",
        NONE,
        &[]
    ),
    descriptor!(
        "set-storage-enabled",
        "Storage",
        Global,
        Boolean,
        "Enable or disable platform storage.",
        SESSION,
        &[]
    ),
    descriptor!(
        "reset-local-vault",
        "Reset vault",
        Global,
        Pulse,
        "Reset platform-local vault data.",
        VAULT,
        &[]
    ),
    descriptor!(
        "request-atlas-import",
        "Import atlas",
        Global,
        Pulse,
        "Open a platform atlas importer.",
        ATLAS,
        &[]
    ),
    descriptor!(
        "request-atlas-export",
        "Export atlas",
        Global,
        Pulse,
        "Export an atlas bundle.",
        ATLAS,
        &[]
    ),
    descriptor!(
        "request-debug-dump",
        "Debug dump",
        Global,
        Pulse,
        "Capture presentation diagnostics.",
        DEBUG_CAPTURE,
        &["any-f12"]
    ),
    descriptor!(
        "request-exit",
        "Exit",
        Global,
        Pulse,
        "Exit where the host supports it.",
        EXIT,
        &["any-escape"]
    ),
];

/// Find a descriptor by an exact stable id.
#[must_use]
pub fn action_descriptor(id: &str) -> Option<&'static ActionDescriptor> {
    ACTION_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.id.as_str() == id)
}

/// Why an action id/payload pair was rejected at a platform boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionDecodeError {
    /// The id is not present in [`ACTION_DESCRIPTORS`].
    UnknownId,
    /// A payload-bearing action did not provide its payload.
    MissingValue,
    /// A pulse action was given a payload.
    UnexpectedValue,
    /// The payload was not an exact supported value.
    InvalidValue,
}

impl fmt::Display for ActionDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownId => "unknown action id",
            Self::MissingValue => "action payload is required",
            Self::UnexpectedValue => "action does not accept a payload",
            Self::InvalidValue => "invalid action payload",
        })
    }
}

impl std::error::Error for ActionDecodeError {}

/// A semantic viewer command. Platform adapters enqueue these values and do
/// not mutate viewer state directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewerAction {
    /// Select visible panes.
    SetPresentation(PresentationMode),
    /// Toggle Map/POV in a single-view presentation.
    TogglePrimaryView,
    /// Route view-scoped input to a pane.
    FocusView(ViewKind),
    /// Set the map share in Split mode.
    SetSplitRatio(f32),
    /// Nudge one possibility domain.
    NudgePossibility {
        /// Domain to nudge.
        domain: PossibilityDomain,
        /// Nudge polarity.
        direction: NudgeDirection,
    },
    /// Clear manual possibility bias.
    ResetPossibilityBias,
    /// Drop a manual anchor.
    DropAnchor(AnchorKind),
    /// Capture an anchor under the traveler.
    CaptureAnchor,
    /// Advance the capture trait category.
    CycleCaptureCategory,
    /// Toggle capture emphasize/suppress polarity.
    ToggleCapturePolarity,
    /// Remove active anchors.
    ClearAnchors,
    /// Toggle deliberate transition movement.
    ToggleTransitionMode,
    /// Persist the session.
    SaveSession,
    /// Load the session.
    LoadSession,
    /// Persist the last anchor as a discovery.
    RecordLastAnchor,
    /// Summon retained discoveries.
    SummonDiscoveries,
    /// Toggle a preserve at the traveler.
    TogglePreserve,
    /// Toggle path display/tracking.
    TogglePathTracking,
    /// Toggle route recording.
    ToggleRouteRecording,
    /// Toggle route attraction.
    ToggleRouteAttraction,
    /// Clear routes.
    ClearRoutes,
    /// Advance the map channel.
    CycleMapChannel,
    /// Select an exact map channel.
    SetMapChannel(Channel),
    /// Toggle a map overlay.
    ToggleOverlay(MapOverlay),
    /// Increase map magnification.
    ZoomIn,
    /// Decrease map magnification.
    ZoomOut,
    /// Toggle GPU map composition.
    ToggleGpuCompose,
    /// Toggle GPU refinement.
    ToggleRefinement,
    /// Toggle POV walk/fly mode.
    ToggleWalk,
    /// Toggle POV shadows and ambient occlusion.
    TogglePovShadowAo,
    /// Toggle POV detail normals.
    TogglePovDetailNormals,
    /// Toggle POV water passes.
    TogglePovWater,
    /// Set the POV render scale.
    SetPovRenderScale(f32),
    /// Select a resource tier.
    SetResourceTier(ResourceTier),
    /// Request the tier benchmark.
    RequestTierBenchmark,
    /// Select task execution strategy.
    SetWorkerBackend(WorkerBackend),
    /// Cancel work superseded by newer requests.
    CancelSupersededJobs,
    /// Select the map renderer.
    SetMapBackend(MapBackend),
    /// Enable or disable platform storage.
    SetStorageEnabled(bool),
    /// Reset local vault data.
    ResetLocalVault,
    /// Open atlas import.
    RequestAtlasImport,
    /// Export an atlas bundle.
    RequestAtlasExport,
    /// Capture diagnostics.
    RequestDebugDump,
    /// Exit where supported.
    RequestExit,
}

impl ViewerAction {
    /// Decode one exact public action id and its optional payload.
    ///
    /// This is the sole string boundary for DOM controls and other declarative
    /// platform UI. It intentionally performs no case folding, prefix matching,
    /// substring matching, or implicit fallback.
    pub fn decode_exact(id: &str, value: Option<&str>) -> Result<Self, ActionDecodeError> {
        use ActionDecodeError::{InvalidValue, MissingValue, UnexpectedValue};

        fn pulse(
            value: Option<&str>,
            action: ViewerAction,
        ) -> Result<ViewerAction, ActionDecodeError> {
            if value.is_none() {
                Ok(action)
            } else {
                Err(UnexpectedValue)
            }
        }

        let required = || value.ok_or(MissingValue);
        match id {
            "set-presentation" => PresentationMode::parse(required()?)
                .map(Self::SetPresentation)
                .ok_or(InvalidValue),
            "toggle-primary-view" => pulse(value, Self::TogglePrimaryView),
            "focus-view" => match required()? {
                "map" => Ok(Self::FocusView(ViewKind::Map)),
                "pov" => Ok(Self::FocusView(ViewKind::Pov)),
                _ => Err(InvalidValue),
            },
            "set-split-ratio" => {
                let ratio = required()?.parse::<f32>().map_err(|_| InvalidValue)?;
                if ratio.is_finite() && (0.1..=0.9).contains(&ratio) {
                    Ok(Self::SetSplitRatio(ratio))
                } else {
                    Err(InvalidValue)
                }
            }
            "nudge-possibility" => {
                let (domain, direction) = required()?.split_once(':').ok_or(InvalidValue)?;
                let domain = match domain {
                    "planetary" => PossibilityDomain::Planetary,
                    "climate" => PossibilityDomain::Climate,
                    "geology" => PossibilityDomain::Geology,
                    "hydrology" => PossibilityDomain::Hydrology,
                    "ecology" => PossibilityDomain::Ecology,
                    "morphology" => PossibilityDomain::Morphology,
                    "behavior" => PossibilityDomain::Behavior,
                    "aesthetics" => PossibilityDomain::Aesthetics,
                    _ => return Err(InvalidValue),
                };
                let direction = match direction {
                    "up" => NudgeDirection::Up,
                    "down" => NudgeDirection::Down,
                    _ => return Err(InvalidValue),
                };
                Ok(Self::NudgePossibility { domain, direction })
            }
            "reset-possibility-bias" => pulse(value, Self::ResetPossibilityBias),
            "drop-anchor" => match required()? {
                "emphasize" => Ok(Self::DropAnchor(AnchorKind::Emphasize)),
                "suppress" => Ok(Self::DropAnchor(AnchorKind::Suppress)),
                _ => Err(InvalidValue),
            },
            "capture-anchor" => pulse(value, Self::CaptureAnchor),
            "cycle-capture-category" => pulse(value, Self::CycleCaptureCategory),
            "toggle-capture-polarity" => pulse(value, Self::ToggleCapturePolarity),
            "clear-anchors" => pulse(value, Self::ClearAnchors),
            "toggle-transition-mode" => pulse(value, Self::ToggleTransitionMode),
            "save-session" => pulse(value, Self::SaveSession),
            "load-session" => pulse(value, Self::LoadSession),
            "record-last-anchor" => pulse(value, Self::RecordLastAnchor),
            "summon-discoveries" => pulse(value, Self::SummonDiscoveries),
            "toggle-preserve" => pulse(value, Self::TogglePreserve),
            "toggle-path-tracking" => pulse(value, Self::TogglePathTracking),
            "toggle-route-recording" => pulse(value, Self::ToggleRouteRecording),
            "toggle-route-attraction" => pulse(value, Self::ToggleRouteAttraction),
            "clear-routes" => pulse(value, Self::ClearRoutes),
            "cycle-map-channel" => pulse(value, Self::CycleMapChannel),
            "set-map-channel" => Channel::from_id(required()?)
                .map(Self::SetMapChannel)
                .ok_or(InvalidValue),
            "toggle-overlay" => match required()? {
                "grid" => Ok(Self::ToggleOverlay(MapOverlay::Grid)),
                "rings" => Ok(Self::ToggleOverlay(MapOverlay::Rings)),
                "pinned-flash" => Ok(Self::ToggleOverlay(MapOverlay::PinnedFlash)),
                "organisms" => Ok(Self::ToggleOverlay(MapOverlay::Organisms)),
                "discovered" => Ok(Self::ToggleOverlay(MapOverlay::Discovered)),
                _ => Err(InvalidValue),
            },
            "zoom-in" => pulse(value, Self::ZoomIn),
            "zoom-out" => pulse(value, Self::ZoomOut),
            "toggle-gpu-compose" => pulse(value, Self::ToggleGpuCompose),
            "toggle-refinement" => pulse(value, Self::ToggleRefinement),
            "toggle-walk" => pulse(value, Self::ToggleWalk),
            "toggle-pov-shadow-ao" => pulse(value, Self::TogglePovShadowAo),
            "toggle-pov-detail-normals" => pulse(value, Self::TogglePovDetailNormals),
            "toggle-pov-water" => pulse(value, Self::TogglePovWater),
            "set-pov-render-scale" => match required()? {
                "1" => Ok(Self::SetPovRenderScale(1.0)),
                "0.5" => Ok(Self::SetPovRenderScale(0.5)),
                "0.25" => Ok(Self::SetPovRenderScale(0.25)),
                _ => Err(InvalidValue),
            },
            "set-resource-tier" => match required()? {
                "low" => Ok(Self::SetResourceTier(ResourceTier::Low)),
                "mid" => Ok(Self::SetResourceTier(ResourceTier::Mid)),
                "high" => Ok(Self::SetResourceTier(ResourceTier::High)),
                _ => Err(InvalidValue),
            },
            "request-tier-benchmark" => pulse(value, Self::RequestTierBenchmark),
            "set-worker-backend" => match required()? {
                "inline" => Ok(Self::SetWorkerBackend(WorkerBackend::Inline)),
                "workers" => Ok(Self::SetWorkerBackend(WorkerBackend::Workers)),
                "shared-workers" => Ok(Self::SetWorkerBackend(WorkerBackend::SharedWorkers)),
                _ => Err(InvalidValue),
            },
            "cancel-superseded-jobs" => pulse(value, Self::CancelSupersededJobs),
            "set-map-backend" => match required()? {
                "cpu" => Ok(Self::SetMapBackend(MapBackend::Cpu)),
                "gpu-atlas" => Ok(Self::SetMapBackend(MapBackend::GpuAtlas)),
                _ => Err(InvalidValue),
            },
            "set-storage-enabled" => match required()? {
                "true" => Ok(Self::SetStorageEnabled(true)),
                "false" => Ok(Self::SetStorageEnabled(false)),
                _ => Err(InvalidValue),
            },
            "reset-local-vault" => pulse(value, Self::ResetLocalVault),
            "request-atlas-import" => pulse(value, Self::RequestAtlasImport),
            "request-atlas-export" => pulse(value, Self::RequestAtlasExport),
            "request-debug-dump" => pulse(value, Self::RequestDebugDump),
            "request-exit" => pulse(value, Self::RequestExit),
            _ => Err(ActionDecodeError::UnknownId),
        }
    }

    /// Stable variant id. Payload values never change the id.
    #[must_use]
    pub const fn id(self) -> ActionId {
        let id = match self {
            Self::SetPresentation(_) => "set-presentation",
            Self::TogglePrimaryView => "toggle-primary-view",
            Self::FocusView(_) => "focus-view",
            Self::SetSplitRatio(_) => "set-split-ratio",
            Self::NudgePossibility { .. } => "nudge-possibility",
            Self::ResetPossibilityBias => "reset-possibility-bias",
            Self::DropAnchor(_) => "drop-anchor",
            Self::CaptureAnchor => "capture-anchor",
            Self::CycleCaptureCategory => "cycle-capture-category",
            Self::ToggleCapturePolarity => "toggle-capture-polarity",
            Self::ClearAnchors => "clear-anchors",
            Self::ToggleTransitionMode => "toggle-transition-mode",
            Self::SaveSession => "save-session",
            Self::LoadSession => "load-session",
            Self::RecordLastAnchor => "record-last-anchor",
            Self::SummonDiscoveries => "summon-discoveries",
            Self::TogglePreserve => "toggle-preserve",
            Self::TogglePathTracking => "toggle-path-tracking",
            Self::ToggleRouteRecording => "toggle-route-recording",
            Self::ToggleRouteAttraction => "toggle-route-attraction",
            Self::ClearRoutes => "clear-routes",
            Self::CycleMapChannel => "cycle-map-channel",
            Self::SetMapChannel(_) => "set-map-channel",
            Self::ToggleOverlay(_) => "toggle-overlay",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::ToggleGpuCompose => "toggle-gpu-compose",
            Self::ToggleRefinement => "toggle-refinement",
            Self::ToggleWalk => "toggle-walk",
            Self::TogglePovShadowAo => "toggle-pov-shadow-ao",
            Self::TogglePovDetailNormals => "toggle-pov-detail-normals",
            Self::TogglePovWater => "toggle-pov-water",
            Self::SetPovRenderScale(_) => "set-pov-render-scale",
            Self::SetResourceTier(_) => "set-resource-tier",
            Self::RequestTierBenchmark => "request-tier-benchmark",
            Self::SetWorkerBackend(_) => "set-worker-backend",
            Self::CancelSupersededJobs => "cancel-superseded-jobs",
            Self::SetMapBackend(_) => "set-map-backend",
            Self::SetStorageEnabled(_) => "set-storage-enabled",
            Self::ResetLocalVault => "reset-local-vault",
            Self::RequestAtlasImport => "request-atlas-import",
            Self::RequestAtlasExport => "request-atlas-export",
            Self::RequestDebugDump => "request-debug-dump",
            Self::RequestExit => "request-exit",
        };
        ActionId::new(id)
    }

    /// Descriptor for this exact typed action.
    #[must_use]
    pub fn descriptor(self) -> &'static ActionDescriptor {
        action_descriptor(self.id().as_str()).expect("every ViewerAction id is registered")
    }
}

/// Monotonic identifier attached to an asynchronous platform request.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServiceRequestId(pub u64);

/// Request to capture a presentation and its semantic state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugCaptureRequest {
    /// Correlates the eventual service response.
    pub request_id: ServiceRequestId,
    /// Presentation visible at request time.
    pub mode: PresentationMode,
    /// Focused pane at request time.
    pub focused: ViewKind,
}

/// Effects requiring a capability owned by a platform shell.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewerEffect {
    /// Exit the host application.
    Exit,
    /// Write a file-bound diagnostic capture.
    WriteDebugCapture(DebugCaptureRequest),
    /// Persist the current shared session snapshot.
    PersistSession(ServiceRequestId),
    /// Load a session through platform storage.
    LoadSession(ServiceRequestId),
    /// Open a platform atlas importer.
    OpenAtlasImport(ServiceRequestId),
    /// Download/write an atlas bundle.
    DownloadAtlasBundle(ServiceRequestId),
    /// Configure task execution.
    ConfigureWorkerBackend(WorkerBackend),
    /// Cancel superseded jobs.
    CancelSupersededJobs,
    /// Configure platform storage.
    ConfigureStorage {
        /// Whether storage should be active.
        enabled: bool,
    },
    /// Clear platform-local vault data.
    ResetLocalVault,
    /// Select the map rendering path.
    SelectMapBackend(MapBackend),
    /// Run resource-tier benchmarking.
    RunTierBenchmark,
    /// Surface a non-fatal warning through the common panel.
    ReportWarning(ViewerWarning),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn effects_keep_request_identity_and_typed_payloads() {
        let id = ServiceRequestId(41);
        assert_eq!(ViewerEffect::LoadSession(id), ViewerEffect::LoadSession(id));
        assert_ne!(
            ViewerEffect::SelectMapBackend(MapBackend::Cpu),
            ViewerEffect::SelectMapBackend(MapBackend::GpuAtlas)
        );
    }

    #[test]
    fn stable_action_ids_are_unique_and_exact() {
        let mut ids = BTreeSet::new();
        for descriptor in ACTION_DESCRIPTORS {
            assert!(ids.insert(descriptor.id.as_str()));
            assert_eq!(action_descriptor(descriptor.id.as_str()), Some(descriptor));
            assert_eq!(
                action_descriptor(&descriptor.id.as_str().to_ascii_uppercase()),
                None
            );
        }
    }

    #[test]
    fn payloads_share_their_variant_descriptor() {
        assert_eq!(
            ViewerAction::SetPresentation(PresentationMode::Split).id(),
            ViewerAction::SetPresentation(PresentationMode::Map).id()
        );
        assert_eq!(
            ViewerAction::NudgePossibility {
                domain: PossibilityDomain::Aesthetics,
                direction: NudgeDirection::Down,
            }
            .descriptor()
            .scope,
            ActionScope::Map
        );
        assert_eq!(
            ViewerAction::SetStorageEnabled(true)
                .descriptor()
                .value_kind,
            ActionValueKind::Boolean
        );
    }

    #[test]
    fn exact_decoder_accepts_typed_payloads_without_a_fallback_language() {
        assert_eq!(
            ViewerAction::decode_exact("set-presentation", Some("split")),
            Ok(ViewerAction::SetPresentation(PresentationMode::Split))
        );
        assert_eq!(
            ViewerAction::decode_exact("nudge-possibility", Some("aesthetics:down")),
            Ok(ViewerAction::NudgePossibility {
                domain: PossibilityDomain::Aesthetics,
                direction: NudgeDirection::Down,
            })
        );
        assert_eq!(
            ViewerAction::decode_exact("set-pov-render-scale", Some("0.25")),
            Ok(ViewerAction::SetPovRenderScale(0.25))
        );
        assert_eq!(
            ViewerAction::decode_exact("toggle-refinement", None),
            Ok(ViewerAction::ToggleRefinement)
        );

        assert_eq!(
            ViewerAction::decode_exact("please-toggle-refinement-now", None),
            Err(ActionDecodeError::UnknownId)
        );
        assert_eq!(
            ViewerAction::decode_exact("Toggle-Refinement", None),
            Err(ActionDecodeError::UnknownId)
        );
        assert_eq!(
            ViewerAction::decode_exact("toggle-refinement", Some("ignored")),
            Err(ActionDecodeError::UnexpectedValue)
        );
        assert_eq!(
            ViewerAction::decode_exact("set-map-channel", None),
            Err(ActionDecodeError::MissingValue)
        );
        assert_eq!(
            ViewerAction::decode_exact("set-map-channel", Some("Composite")),
            Err(ActionDecodeError::InvalidValue)
        );
        for invalid in ["NaN", "inf", "0.09", "0.91"] {
            assert_eq!(
                ViewerAction::decode_exact("set-split-ratio", Some(invalid)),
                Err(ActionDecodeError::InvalidValue)
            );
        }
    }
}
