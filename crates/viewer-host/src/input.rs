//! Normalized, platform-neutral input values. Binding and held-state behavior
//! lands in Milestone 2.

use crate::layout::{PresentationMode, ViewKind};

/// Press/release phase for keys and buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonPhase {
    /// Control became held.
    Pressed,
    /// Control stopped being held.
    Released,
}

/// Keyboard modifier snapshot accompanying an event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// Control is held.
    pub control: bool,
    /// Alt/Option is held.
    pub alt: bool,
    /// Super/Command is held.
    pub super_key: bool,
}

/// Pointer button after platform translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Primary/left button.
    Primary,
    /// Auxiliary/middle button.
    Auxiliary,
    /// Secondary/right button.
    Secondary,
    /// Another physical button.
    Other(u16),
}

/// Raw wheel quantity with its adapter-normalized unit retained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// Logical line notches.
    Lines(f64),
    /// Physical pixels.
    Pixels(f64),
}

/// Controller axis admitted by the normalized contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerAxis {
    /// Navigation horizontal axis.
    MoveX,
    /// Navigation forward axis.
    MoveY,
    /// Fly vertical axis.
    MoveZ,
    /// Look horizontal axis.
    LookX,
    /// Look vertical axis.
    LookY,
}

/// Current view routing context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputContext {
    /// Visible presentation.
    pub mode: PresentationMode,
    /// Pane receiving scoped input.
    pub focused: ViewKind,
    /// Whether a view surface, rather than a toolbar control, owns keyboard focus.
    pub surface_focused: bool,
}

/// Continuous intent sampled once per logical viewer frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct InputFrame {
    /// Map right/forward axes.
    pub map_axis: [f32; 2],
    /// POV strafe/forward/vertical axes.
    pub pov_axis: [f32; 3],
    /// Sprint modifier.
    pub sprint: bool,
    /// Accumulated pointer-look delta in physical pixels.
    pub look_delta: [f64; 2],
    /// Whole wheel notches after fractional accumulation.
    pub wheel_steps: i32,
    /// Whether the primary POV-look gesture remains held.
    pub primary_drag: bool,
}
