//! Typed semantic actions and platform effects (`native-web-alignment.md`
//! sections 5.2 and 5.3).

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
}
