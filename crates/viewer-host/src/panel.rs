//! Shared semantic panel values. Native bitmap and browser DOM renderers
//! remain platform-specific (`native-web-alignment.md` section 5.7).

use crate::action::WorkerBackend;
use crate::inspect::HoverInfo;
use crate::layout::{PresentationMode, ViewKind};
use crate::map::{Channel, MapBackend};

/// Stable field identity used by native and accessible DOM panel renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PanelFieldId(&'static str);

impl PanelFieldId {
    /// Declare a stable field id.
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// Exact id used at the platform boundary.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Common field ids introduced before moving panel construction.
pub mod field_ids {
    use super::PanelFieldId;

    /// Frames per second.
    pub const FRAME_FPS: PanelFieldId = PanelFieldId::new("frame.fps");
    /// Visible presentation mode.
    pub const VIEW_MODE: PanelFieldId = PanelFieldId::new("view.mode");
    /// Focused pane.
    pub const VIEW_FOCUS: PanelFieldId = PanelFieldId::new("view.focus");
    /// Map channel.
    pub const MAP_CHANNEL: PanelFieldId = PanelFieldId::new("map.channel");
    /// Traveler X coordinate.
    pub const TRAVELER_X: PanelFieldId = PanelFieldId::new("traveler.x");
    /// Traveler Y coordinate.
    pub const TRAVELER_Y: PanelFieldId = PanelFieldId::new("traveler.y");
}

/// Severity for a warning or panel field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Normal informational content.
    Info,
    /// Degraded capability or retryable failure.
    Warning,
    /// The requested presentation/action cannot proceed.
    Error,
}

/// A typed warning shown by either panel renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerWarning {
    /// Stable machine-readable warning id.
    pub id: &'static str,
    /// User-facing explanation.
    pub message: String,
    /// Display severity.
    pub severity: Severity,
}

/// Measurements gathered by a platform shell and injected into shared panel
/// construction. Viewer code never queries platform APIs for these values.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformTelemetry {
    /// Mean surface present time in milliseconds.
    pub present_ms: f64,
    /// Number of DOM updates (zero on native).
    pub dom_updates: u64,
    /// Surface format label reported by the renderer.
    pub surface_format: Option<String>,
    /// Active task execution strategy.
    pub executor_backend: WorkerBackend,
    /// Executor parallelism.
    pub workers: usize,
    /// Whether persistent storage is available.
    pub storage_available: bool,
}

impl Default for PlatformTelemetry {
    fn default() -> Self {
        Self {
            present_ms: 0.0,
            dom_updates: 0,
            surface_format: None,
            executor_backend: WorkerBackend::Inline,
            workers: 1,
            storage_available: false,
        }
    }
}

/// View-specific portion of the shared information model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewInfo {
    /// Visible panes.
    pub mode: PresentationMode,
    /// Pane receiving scoped input.
    pub focused: ViewKind,
    /// Active map field.
    pub map_channel: Channel,
    /// Map magnification.
    pub map_zoom: u32,
    /// Map presentation backend.
    pub map_backend: MapBackend,
    /// Map share in Split mode.
    pub split_ratio: f32,
    /// Whether POV is currently supported.
    pub pov_supported: bool,
}

/// Cross-platform semantic information model. Detailed streaming, steering,
/// and persistence sections are added as their current builders migrate.
#[derive(Debug, Clone, PartialEq)]
pub struct InfoPanelModel {
    /// Logical frame sequence.
    pub frame: u64,
    /// Frames presented during the latest reporting window.
    pub fps: u32,
    /// Current view state.
    pub view: ViewInfo,
    /// Current traveler XY.
    pub traveler: (f64, f64),
    /// Nearest inspected cell or organism.
    pub hover: HoverInfo,
    /// Typed platform measurements.
    pub platform: PlatformTelemetry,
    /// Active warnings.
    pub warnings: Vec<ViewerWarning>,
}

#[cfg(test)]
mod tests {
    use super::field_ids::*;

    #[test]
    fn panel_ids_are_stable_and_hierarchical() {
        assert_eq!(FRAME_FPS.as_str(), "frame.fps");
        assert_eq!(VIEW_MODE.as_str(), "view.mode");
        assert_ne!(VIEW_FOCUS, MAP_CHANNEL);
        assert_ne!(TRAVELER_X, TRAVELER_Y);
    }
}
