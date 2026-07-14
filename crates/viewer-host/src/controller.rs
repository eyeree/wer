//! Ordered controller boundary values. Reducer and single-world-tick behavior
//! land in Milestone 3.

use crate::action::ViewerEffect;
use crate::input::InputFrame;
use crate::layout::{PresentationMode, ViewKind};
use crate::panel::PlatformTelemetry;

/// Inputs sampled for one logical viewer tick after service responses and
/// discrete actions have been ordered.
#[derive(Debug, Clone, PartialEq)]
pub struct TickInput {
    /// Elapsed frame time in seconds.
    pub dt_seconds: f64,
    /// Continuous input intent.
    pub input: InputFrame,
    /// Measurements injected by the platform shell.
    pub platform: PlatformTelemetry,
}

/// Value-only result returned to a thin platform shell.
#[derive(Debug, Clone, PartialEq)]
pub struct TickOutput {
    /// Presentation selected after reduction.
    pub mode: PresentationMode,
    /// Pane receiving view-scoped input.
    pub focused: ViewKind,
    /// Effects requiring platform capabilities, in reducer order.
    pub effects: Vec<ViewerEffect>,
}
