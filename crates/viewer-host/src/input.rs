//! Normalized, platform-neutral input, canonical bindings, and held-state
//! mapping (`native-web-alignment.md` sections 5.1 and 5.2).

use std::collections::{HashSet, VecDeque};

use world_core::{AnchorKind, PossibilityDomain};

use crate::action::{NudgeDirection, ViewerAction};
use crate::layout::{PresentationMode, ViewKind};
use crate::map::MapOverlay;

/// Native touchpad/CSS physical pixels treated as one wheel notch.
pub const WHEEL_PIXELS_PER_NOTCH: f64 = 40.0;

/// A locale-independent physical keyboard position.
///
/// Names deliberately mirror DOM `KeyboardEvent.code`; platform adapters
/// perform an exact conversion and never use locale/case-sensitive text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PhysicalKey {
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Space,
    ShiftLeft,
    ShiftRight,
    Tab,
    Escape,
    Delete,
    F12,
    Comma,
    Period,
}

impl PhysicalKey {
    /// Convert an exact DOM physical-code string.
    #[must_use]
    pub fn from_dom_code(code: &str) -> Option<Self> {
        Some(match code {
            "KeyA" => Self::KeyA,
            "KeyB" => Self::KeyB,
            "KeyC" => Self::KeyC,
            "KeyD" => Self::KeyD,
            "KeyE" => Self::KeyE,
            "KeyF" => Self::KeyF,
            "KeyG" => Self::KeyG,
            "KeyH" => Self::KeyH,
            "KeyI" => Self::KeyI,
            "KeyJ" => Self::KeyJ,
            "KeyK" => Self::KeyK,
            "KeyL" => Self::KeyL,
            "KeyM" => Self::KeyM,
            "KeyN" => Self::KeyN,
            "KeyO" => Self::KeyO,
            "KeyP" => Self::KeyP,
            "KeyQ" => Self::KeyQ,
            "KeyR" => Self::KeyR,
            "KeyS" => Self::KeyS,
            "KeyT" => Self::KeyT,
            "KeyU" => Self::KeyU,
            "KeyV" => Self::KeyV,
            "KeyW" => Self::KeyW,
            "KeyX" => Self::KeyX,
            "KeyY" => Self::KeyY,
            "KeyZ" => Self::KeyZ,
            "Digit1" => Self::Digit1,
            "Digit2" => Self::Digit2,
            "Digit3" => Self::Digit3,
            "Digit4" => Self::Digit4,
            "Digit5" => Self::Digit5,
            "Digit6" => Self::Digit6,
            "Digit7" => Self::Digit7,
            "Digit8" => Self::Digit8,
            "ArrowUp" => Self::ArrowUp,
            "ArrowDown" => Self::ArrowDown,
            "ArrowLeft" => Self::ArrowLeft,
            "ArrowRight" => Self::ArrowRight,
            "Space" => Self::Space,
            "ShiftLeft" => Self::ShiftLeft,
            "ShiftRight" => Self::ShiftRight,
            "Tab" => Self::Tab,
            "Escape" => Self::Escape,
            "Delete" => Self::Delete,
            "F12" => Self::F12,
            "Comma" => Self::Comma,
            "Period" => Self::Period,
            _ => return None,
        })
    }

    /// Exact DOM physical-code spelling.
    #[must_use]
    pub const fn dom_code(self) -> &'static str {
        match self {
            Self::KeyA => "KeyA",
            Self::KeyB => "KeyB",
            Self::KeyC => "KeyC",
            Self::KeyD => "KeyD",
            Self::KeyE => "KeyE",
            Self::KeyF => "KeyF",
            Self::KeyG => "KeyG",
            Self::KeyH => "KeyH",
            Self::KeyI => "KeyI",
            Self::KeyJ => "KeyJ",
            Self::KeyK => "KeyK",
            Self::KeyL => "KeyL",
            Self::KeyM => "KeyM",
            Self::KeyN => "KeyN",
            Self::KeyO => "KeyO",
            Self::KeyP => "KeyP",
            Self::KeyQ => "KeyQ",
            Self::KeyR => "KeyR",
            Self::KeyS => "KeyS",
            Self::KeyT => "KeyT",
            Self::KeyU => "KeyU",
            Self::KeyV => "KeyV",
            Self::KeyW => "KeyW",
            Self::KeyX => "KeyX",
            Self::KeyY => "KeyY",
            Self::KeyZ => "KeyZ",
            Self::Digit1 => "Digit1",
            Self::Digit2 => "Digit2",
            Self::Digit3 => "Digit3",
            Self::Digit4 => "Digit4",
            Self::Digit5 => "Digit5",
            Self::Digit6 => "Digit6",
            Self::Digit7 => "Digit7",
            Self::Digit8 => "Digit8",
            Self::ArrowUp => "ArrowUp",
            Self::ArrowDown => "ArrowDown",
            Self::ArrowLeft => "ArrowLeft",
            Self::ArrowRight => "ArrowRight",
            Self::Space => "Space",
            Self::ShiftLeft => "ShiftLeft",
            Self::ShiftRight => "ShiftRight",
            Self::Tab => "Tab",
            Self::Escape => "Escape",
            Self::Delete => "Delete",
            Self::F12 => "F12",
            Self::Comma => "Comma",
            Self::Period => "Period",
        }
    }
}

/// Press/release phase for keys and buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonPhase {
    Pressed,
    Released,
}

/// Keyboard modifier snapshot accompanying an event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// Pointer button after platform translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary,
    Auxiliary,
    Secondary,
    Back,
    Forward,
    Other(u16),
}

/// Raw wheel quantity with its adapter-normalized unit retained.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    Lines(f64),
    Pixels(f64),
}

/// Controller axis admitted by the normalized contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerAxis {
    MoveX,
    MoveY,
    MoveZ,
    LookX,
    LookY,
}

impl ControllerAxis {
    const fn index(self) -> usize {
        match self {
            Self::MoveX => 0,
            Self::MoveY => 1,
            Self::MoveZ => 2,
            Self::LookX => 3,
            Self::LookY => 4,
        }
    }
}

/// Current view routing context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputContext {
    pub mode: PresentationMode,
    pub focused: ViewKind,
    pub surface_focused: bool,
}

impl Default for InputContext {
    fn default() -> Self {
        Self {
            mode: PresentationMode::Map,
            focused: ViewKind::Map,
            surface_focused: true,
        }
    }
}

/// Platform-neutral event produced by a thin raw-environment adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NormalizedInputEvent {
    Key {
        key: PhysicalKey,
        phase: ButtonPhase,
        repeat: bool,
        modifiers: Modifiers,
    },
    ModifiersChanged(Modifiers),
    PointerMoved {
        pointer: u64,
        position: [f64; 2],
        view: ViewKind,
    },
    PointerButton {
        pointer: u64,
        button: PointerButton,
        phase: ButtonPhase,
        position: [f64; 2],
        view: ViewKind,
    },
    PointerCancelled {
        pointer: u64,
    },
    Wheel {
        delta: WheelDelta,
        view: ViewKind,
    },
    Axis {
        axis: ControllerAxis,
        value: f32,
    },
    FocusChanged {
        focused: bool,
    },
}

/// Context in which a default binding is eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingContext {
    Global,
    SingleView,
    Split,
    Map,
    Pov,
    FocusedView,
}

/// Shift matching for a one-shot binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftBinding {
    Any,
    Required,
    Forbidden,
}

/// Physical source shown by generated help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingInput {
    Key(PhysicalKey),
    WheelPositive,
    WheelNegative,
    PrimaryPress,
    PrimaryDrag,
    NavigationKeys,
}

/// Semantic result of a binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BindingOutput {
    Action(ViewerAction),
    FocusOtherView,
    HeldNavigation,
    PointerLook,
    PovSpeed,
}

/// One canonical input binding record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BindingDescriptor {
    pub id: &'static str,
    pub context: BindingContext,
    pub input: BindingInput,
    pub shift: ShiftBinding,
    pub output: BindingOutput,
    pub help: &'static str,
}

macro_rules! key_binding {
    ($id:literal, $context:ident, $key:ident, $shift:ident, $action:expr, $help:literal) => {
        BindingDescriptor {
            id: $id,
            context: BindingContext::$context,
            input: BindingInput::Key(PhysicalKey::$key),
            shift: ShiftBinding::$shift,
            output: BindingOutput::Action($action),
            help: $help,
        }
    };
}

/// Single source of truth for key, wheel, and pointer bindings.
pub const BINDING_DESCRIPTORS: &[BindingDescriptor] = &[
    BindingDescriptor {
        id: "focused-navigation",
        context: BindingContext::FocusedView,
        input: BindingInput::NavigationKeys,
        shift: ShiftBinding::Any,
        output: BindingOutput::HeldNavigation,
        help: "WASD / arrows move the focused view.",
    },
    key_binding!(
        "map-digit1-up",
        Map,
        Digit1,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Planetary,
            direction: NudgeDirection::Up
        },
        "1 nudges Planetary up."
    ),
    key_binding!(
        "map-digit1-down",
        Map,
        Digit1,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Planetary,
            direction: NudgeDirection::Down
        },
        "Shift+1 nudges Planetary down."
    ),
    key_binding!(
        "map-digit2-up",
        Map,
        Digit2,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Climate,
            direction: NudgeDirection::Up
        },
        "2 nudges Climate up."
    ),
    key_binding!(
        "map-digit2-down",
        Map,
        Digit2,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Climate,
            direction: NudgeDirection::Down
        },
        "Shift+2 nudges Climate down."
    ),
    key_binding!(
        "map-digit3-up",
        Map,
        Digit3,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Geology,
            direction: NudgeDirection::Up
        },
        "3 nudges Geology up."
    ),
    key_binding!(
        "map-digit3-down",
        Map,
        Digit3,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Geology,
            direction: NudgeDirection::Down
        },
        "Shift+3 nudges Geology down."
    ),
    key_binding!(
        "map-digit4-up",
        Map,
        Digit4,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Hydrology,
            direction: NudgeDirection::Up
        },
        "4 nudges Hydrology up."
    ),
    key_binding!(
        "map-digit4-down",
        Map,
        Digit4,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Hydrology,
            direction: NudgeDirection::Down
        },
        "Shift+4 nudges Hydrology down."
    ),
    key_binding!(
        "map-digit5-up",
        Map,
        Digit5,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Ecology,
            direction: NudgeDirection::Up
        },
        "5 nudges Ecology up."
    ),
    key_binding!(
        "map-digit5-down",
        Map,
        Digit5,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Ecology,
            direction: NudgeDirection::Down
        },
        "Shift+5 nudges Ecology down."
    ),
    key_binding!(
        "map-digit6-up",
        Map,
        Digit6,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Morphology,
            direction: NudgeDirection::Up
        },
        "6 nudges Morphology up."
    ),
    key_binding!(
        "map-digit6-down",
        Map,
        Digit6,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Morphology,
            direction: NudgeDirection::Down
        },
        "Shift+6 nudges Morphology down."
    ),
    key_binding!(
        "map-digit7-up",
        Map,
        Digit7,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Behavior,
            direction: NudgeDirection::Up
        },
        "7 nudges Behavior up."
    ),
    key_binding!(
        "map-digit7-down",
        Map,
        Digit7,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Behavior,
            direction: NudgeDirection::Down
        },
        "Shift+7 nudges Behavior down."
    ),
    key_binding!(
        "map-digit8-up",
        Map,
        Digit8,
        Forbidden,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Aesthetics,
            direction: NudgeDirection::Up
        },
        "8 nudges Aesthetics up."
    ),
    key_binding!(
        "map-digit8-down",
        Map,
        Digit8,
        Required,
        ViewerAction::NudgePossibility {
            domain: PossibilityDomain::Aesthetics,
            direction: NudgeDirection::Down
        },
        "Shift+8 nudges Aesthetics down."
    ),
    key_binding!(
        "map-key-z",
        Map,
        KeyZ,
        Any,
        ViewerAction::ResetPossibilityBias,
        "Z resets possibility bias."
    ),
    key_binding!(
        "map-key-e",
        Map,
        KeyE,
        Any,
        ViewerAction::DropAnchor(AnchorKind::Emphasize),
        "E drops an Emphasize anchor."
    ),
    key_binding!(
        "map-key-q",
        Map,
        KeyQ,
        Any,
        ViewerAction::DropAnchor(AnchorKind::Suppress),
        "Q drops a Suppress anchor."
    ),
    key_binding!(
        "map-key-k",
        Map,
        KeyK,
        Any,
        ViewerAction::CaptureAnchor,
        "K captures under the traveler."
    ),
    key_binding!(
        "map-key-t",
        Map,
        KeyT,
        Any,
        ViewerAction::CycleCaptureCategory,
        "T cycles capture category."
    ),
    key_binding!(
        "map-key-y",
        Map,
        KeyY,
        Any,
        ViewerAction::ToggleCapturePolarity,
        "Y toggles capture polarity."
    ),
    key_binding!(
        "map-key-r",
        Map,
        KeyR,
        Any,
        ViewerAction::ToggleTransitionMode,
        "R toggles transition movement."
    ),
    key_binding!(
        "map-key-c",
        Map,
        KeyC,
        Any,
        ViewerAction::ClearAnchors,
        "C clears anchors."
    ),
    key_binding!(
        "map-key-o",
        Map,
        KeyO,
        Any,
        ViewerAction::SaveSession,
        "O saves the session."
    ),
    key_binding!(
        "map-key-l",
        Map,
        KeyL,
        Any,
        ViewerAction::LoadSession,
        "L loads the session."
    ),
    key_binding!(
        "map-key-b",
        Map,
        KeyB,
        Any,
        ViewerAction::RecordLastAnchor,
        "B records the last anchor."
    ),
    key_binding!(
        "map-key-i",
        Map,
        KeyI,
        Any,
        ViewerAction::SummonDiscoveries,
        "I summons discoveries."
    ),
    key_binding!(
        "map-key-p",
        Map,
        KeyP,
        Any,
        ViewerAction::TogglePreserve,
        "P toggles a preserve."
    ),
    key_binding!(
        "map-key-h",
        Map,
        KeyH,
        Any,
        ViewerAction::TogglePathTracking,
        "H toggles path tracking."
    ),
    key_binding!(
        "map-key-j",
        Map,
        KeyJ,
        Any,
        ViewerAction::ToggleRouteRecording,
        "J toggles route recording."
    ),
    key_binding!(
        "map-key-u",
        Map,
        KeyU,
        Any,
        ViewerAction::ToggleRouteAttraction,
        "U toggles route attraction."
    ),
    key_binding!(
        "map-delete",
        Map,
        Delete,
        Any,
        ViewerAction::ClearRoutes,
        "Delete clears routes."
    ),
    key_binding!(
        "map-key-v",
        Map,
        KeyV,
        Any,
        ViewerAction::CycleMapChannel,
        "V cycles the map channel."
    ),
    key_binding!(
        "map-key-f",
        Map,
        KeyF,
        Any,
        ViewerAction::ToggleOverlay(MapOverlay::Discovered),
        "F toggles discovery dimming."
    ),
    key_binding!(
        "map-key-g",
        Map,
        KeyG,
        Any,
        ViewerAction::ToggleOverlay(MapOverlay::Grid),
        "G toggles the grid."
    ),
    key_binding!(
        "map-key-n",
        Map,
        KeyN,
        Any,
        ViewerAction::ToggleOverlay(MapOverlay::Rings),
        "N toggles stability rings."
    ),
    key_binding!(
        "map-key-x",
        Map,
        KeyX,
        Any,
        ViewerAction::ToggleOverlay(MapOverlay::PinnedFlash),
        "X toggles pinned-change flashes."
    ),
    key_binding!(
        "map-key-m",
        Map,
        KeyM,
        Any,
        ViewerAction::ToggleOverlay(MapOverlay::Organisms),
        "M toggles organisms."
    ),
    key_binding!(
        "map-comma",
        Map,
        Comma,
        Any,
        ViewerAction::ToggleGpuCompose,
        ", toggles GPU composition."
    ),
    key_binding!(
        "map-period",
        Map,
        Period,
        Any,
        ViewerAction::ToggleRefinement,
        ". toggles map refinement."
    ),
    key_binding!(
        "pov-key-f",
        Pov,
        KeyF,
        Any,
        ViewerAction::ToggleWalk,
        "F toggles walk/fly."
    ),
    key_binding!(
        "pov-key-b",
        Pov,
        KeyB,
        Any,
        ViewerAction::TogglePovShadowAo,
        "B toggles POV shadows and AO."
    ),
    key_binding!(
        "pov-key-n",
        Pov,
        KeyN,
        Any,
        ViewerAction::TogglePovDetailNormals,
        "N toggles POV detail normals."
    ),
    key_binding!(
        "pov-key-v",
        Pov,
        KeyV,
        Any,
        ViewerAction::TogglePovWater,
        "V toggles POV water."
    ),
    key_binding!(
        "any-f12",
        Global,
        F12,
        Any,
        ViewerAction::RequestDebugDump,
        "F12 captures diagnostics."
    ),
    key_binding!(
        "any-escape",
        Global,
        Escape,
        Any,
        ViewerAction::RequestExit,
        "Escape exits."
    ),
    key_binding!(
        "single-tab",
        SingleView,
        Tab,
        Any,
        ViewerAction::TogglePrimaryView,
        "Tab toggles Map and POV."
    ),
    BindingDescriptor {
        id: "split-tab",
        context: BindingContext::Split,
        input: BindingInput::Key(PhysicalKey::Tab),
        shift: ShiftBinding::Any,
        output: BindingOutput::FocusOtherView,
        help: "Tab focuses the other Split pane.",
    },
    BindingDescriptor {
        id: "map-wheel-positive",
        context: BindingContext::Map,
        input: BindingInput::WheelPositive,
        shift: ShiftBinding::Any,
        output: BindingOutput::Action(ViewerAction::ZoomIn),
        help: "Wheel up zooms in.",
    },
    BindingDescriptor {
        id: "map-wheel-negative",
        context: BindingContext::Map,
        input: BindingInput::WheelNegative,
        shift: ShiftBinding::Any,
        output: BindingOutput::Action(ViewerAction::ZoomOut),
        help: "Wheel down zooms out.",
    },
    BindingDescriptor {
        id: "pov-wheel",
        context: BindingContext::Pov,
        input: BindingInput::WheelPositive,
        shift: ShiftBinding::Any,
        output: BindingOutput::PovSpeed,
        help: "Wheel adjusts POV speed.",
    },
    BindingDescriptor {
        id: "split-primary-press",
        context: BindingContext::Split,
        input: BindingInput::PrimaryPress,
        shift: ShiftBinding::Any,
        output: BindingOutput::FocusOtherView,
        help: "Click focuses a Split pane.",
    },
    BindingDescriptor {
        id: "pov-primary-drag",
        context: BindingContext::Pov,
        input: BindingInput::PrimaryDrag,
        shift: ShiftBinding::Any,
        output: BindingOutput::PointerLook,
        help: "Hold primary and drag to look.",
    },
];

/// Continuous intent sampled once per logical viewer frame. Axes retain raw
/// signed components; world-space normalization happens after camera-basis
/// application, and map movement preserves the pre-alignment f64 operation
/// order exactly.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct InputFrame {
    pub map_axis: [i8; 2],
    pub pov_axis: [i8; 3],
    pub controller_axes: [f32; 5],
    pub sprint: bool,
    pub look_delta: [f64; 2],
    pub wheel_steps: i32,
    pub primary_drag: bool,
    pub map_pointer: Option<[f64; 2]>,
    pub pov_pointer: Option<[f64; 2]>,
}

impl InputFrame {
    /// Bit-compatible native map delta: `speed * sprint * dt / hypot(x,y)`.
    #[must_use]
    pub fn map_movement_delta(self, speed: f64, dt: f64) -> Option<(f64, f64)> {
        let dx = f64::from(self.map_axis[0]);
        let dy = f64::from(self.map_axis[1]);
        let len = f64::hypot(dx, dy);
        if len == 0.0 {
            return None;
        }
        let sprint = if self.sprint { 4.0 } else { 1.0 };
        let step = speed * sprint * dt / len;
        Some((dx * step, dy * step))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DragState {
    pointer: u64,
    last: [f64; 2],
}

/// Canonical held-state machine and ordered semantic-action queue.
#[derive(Debug)]
pub struct InputMapper {
    held: HashSet<PhysicalKey>,
    modifiers: Modifiers,
    context: InputContext,
    actions: VecDeque<ViewerAction>,
    look_delta: [f64; 2],
    pov_wheel_steps: i32,
    wheel_residual: [f64; 2],
    drag: Option<DragState>,
    pointer_positions: [Option<(u64, [f64; 2])>; 2],
    controller_axes: [f32; 5],
}

impl Default for InputMapper {
    fn default() -> Self {
        Self {
            held: HashSet::new(),
            modifiers: Modifiers::default(),
            context: InputContext::default(),
            actions: VecDeque::new(),
            look_delta: [0.0; 2],
            pov_wheel_steps: 0,
            wheel_residual: [0.0; 2],
            drag: None,
            pointer_positions: [None; 2],
            controller_axes: [0.0; 5],
        }
    }
}

impl InputMapper {
    /// Update routing after a semantic presentation/focus action.
    pub fn set_context(&mut self, context: InputContext) {
        self.context = context;
        if !context.surface_focused {
            self.drag = None;
        }
    }

    /// Normalize one event into held intent and zero or more ordered actions.
    pub fn handle_event(&mut self, event: NormalizedInputEvent, context: InputContext) -> bool {
        self.set_context(context);
        match event {
            NormalizedInputEvent::Key {
                key,
                phase,
                repeat,
                modifiers,
            } => self.handle_key(key, phase, repeat, modifiers),
            NormalizedInputEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                true
            }
            NormalizedInputEvent::PointerMoved {
                pointer,
                position,
                view,
            } => self.handle_pointer_moved(pointer, position, view),
            NormalizedInputEvent::PointerButton {
                pointer,
                button,
                phase,
                position,
                view,
            } => self.handle_pointer_button(pointer, button, phase, position, view),
            NormalizedInputEvent::PointerCancelled { pointer } => {
                if self.drag.is_some_and(|drag| drag.pointer == pointer) {
                    self.drag = None;
                }
                for position in &mut self.pointer_positions {
                    if position.is_some_and(|(position_pointer, _)| position_pointer == pointer) {
                        *position = None;
                    }
                }
                true
            }
            NormalizedInputEvent::Wheel { delta, view } => self.handle_wheel(delta, view),
            NormalizedInputEvent::Axis { axis, value } => {
                self.controller_axes[axis.index()] = value.clamp(-1.0, 1.0);
                true
            }
            NormalizedInputEvent::FocusChanged { focused } => {
                if !focused {
                    self.clear_held_state();
                }
                self.context.surface_focused = focused;
                true
            }
        }
    }

    /// Enqueue a toolbar/controller action into the same ordered consumer.
    pub fn enqueue_action(&mut self, action: ViewerAction) {
        self.actions.push_back(action);
    }

    /// Drain semantic actions in raw-event enqueue order.
    pub fn drain_actions(&mut self) -> impl Iterator<Item = ViewerAction> + '_ {
        self.actions.drain(..)
    }

    /// Whether another animation frame can consume input without a new raw
    /// platform event. Browser adapters use this to schedule held movement
    /// without keeping a second key-state authority in JavaScript.
    #[must_use]
    pub fn has_continuous_input(&self) -> bool {
        self.held.iter().copied().any(is_navigation_key)
            || self.drag.is_some()
            || self.look_delta != [0.0; 2]
            || self.pov_wheel_steps != 0
            || self
                .controller_axes
                .iter()
                .any(|value| value.abs() > f32::EPSILON)
    }

    /// Sample held intent and consume accumulated look/wheel quantities.
    pub fn take_frame(&mut self) -> InputFrame {
        let active = active_view(self.context);
        let surface = self.context.surface_focused;
        let key_axis = |positive: &[PhysicalKey], negative: &[PhysicalKey]| -> i8 {
            i8::from(positive.iter().any(|key| self.held.contains(key)))
                - i8::from(negative.iter().any(|key| self.held.contains(key)))
        };
        let horizontal = key_axis(
            &[PhysicalKey::KeyD, PhysicalKey::ArrowRight],
            &[PhysicalKey::KeyA, PhysicalKey::ArrowLeft],
        );
        let forward = key_axis(
            &[PhysicalKey::KeyW, PhysicalKey::ArrowUp],
            &[PhysicalKey::KeyS, PhysicalKey::ArrowDown],
        );
        let vertical = key_axis(&[PhysicalKey::Space], &[PhysicalKey::ShiftLeft]);
        let controller_sign = |index: usize| -> i8 {
            let value = self.controller_axes[index];
            i8::from(value > f32::EPSILON) - i8::from(value < -f32::EPSILON)
        };

        let mut frame = InputFrame {
            controller_axes: self.controller_axes,
            map_pointer: self.pointer_positions[view_index(ViewKind::Map)].map(|(_, point)| point),
            pov_pointer: self.pointer_positions[view_index(ViewKind::Pov)].map(|(_, point)| point),
            ..InputFrame::default()
        };
        if surface {
            match active {
                ViewKind::Map => {
                    frame.map_axis = [
                        (horizontal + controller_sign(0)).clamp(-1, 1),
                        (forward + controller_sign(1)).clamp(-1, 1),
                    ];
                    frame.sprint = self.modifiers.shift
                        || self.held.contains(&PhysicalKey::ShiftLeft)
                        || self.held.contains(&PhysicalKey::ShiftRight);
                }
                ViewKind::Pov => {
                    frame.pov_axis = [
                        (horizontal + controller_sign(0)).clamp(-1, 1),
                        (forward + controller_sign(1)).clamp(-1, 1),
                        (vertical + controller_sign(2)).clamp(-1, 1),
                    ];
                    frame.look_delta = self.look_delta;
                    frame.wheel_steps = self.pov_wheel_steps;
                    frame.primary_drag = self.drag.is_some();
                }
            }
        }
        self.look_delta = [0.0; 2];
        self.pov_wheel_steps = 0;
        frame
    }

    /// Clear every transient after focus loss. The action queue is retained:
    /// actions already ordered before blur still execute exactly once.
    pub fn clear_held_state(&mut self) {
        self.held.clear();
        self.modifiers = Modifiers::default();
        self.look_delta = [0.0; 2];
        self.pov_wheel_steps = 0;
        self.wheel_residual = [0.0; 2];
        self.drag = None;
        self.pointer_positions = [None; 2];
        self.controller_axes = [0.0; 5];
    }

    fn handle_key(
        &mut self,
        key: PhysicalKey,
        phase: ButtonPhase,
        repeat: bool,
        modifiers: Modifiers,
    ) -> bool {
        self.modifiers = modifiers;
        if phase == ButtonPhase::Released {
            return self.held.remove(&key);
        }
        if !self.context.surface_focused {
            return false;
        }
        let first_press = self.held.insert(key);
        if repeat || !first_press || modifiers.control || modifiers.alt || modifiers.super_key {
            return is_known_key(key);
        }
        let mut consumed = is_navigation_key(key);
        for binding in BINDING_DESCRIPTORS {
            if binding.input != BindingInput::Key(key)
                || !binding_context_matches(binding.context, self.context)
                || !shift_matches(binding.shift, modifiers.shift)
            {
                continue;
            }
            match binding.output {
                BindingOutput::Action(action) => self.actions.push_back(action),
                BindingOutput::FocusOtherView => {
                    self.actions
                        .push_back(ViewerAction::FocusView(match self.context.focused {
                            ViewKind::Map => ViewKind::Pov,
                            ViewKind::Pov => ViewKind::Map,
                        }))
                }
                BindingOutput::HeldNavigation
                | BindingOutput::PointerLook
                | BindingOutput::PovSpeed => {}
            }
            consumed = true;
        }
        consumed
    }

    fn handle_pointer_moved(&mut self, pointer: u64, position: [f64; 2], view: ViewKind) -> bool {
        if !presentation_contains(self.context.mode, view) {
            return false;
        }
        self.pointer_positions[view_index(view)] = Some((pointer, position));
        let Some(drag) = self.drag.as_mut() else {
            return true;
        };
        if drag.pointer != pointer || view != ViewKind::Pov {
            return false;
        }
        self.look_delta[0] += position[0] - drag.last[0];
        self.look_delta[1] += position[1] - drag.last[1];
        drag.last = position;
        true
    }

    fn handle_pointer_button(
        &mut self,
        pointer: u64,
        button: PointerButton,
        phase: ButtonPhase,
        position: [f64; 2],
        view: ViewKind,
    ) -> bool {
        if button != PointerButton::Primary {
            return false;
        }
        self.pointer_positions[view_index(view)] = Some((pointer, position));
        match phase {
            ButtonPhase::Pressed => {
                if self.context.mode == PresentationMode::Split && self.context.focused != view {
                    self.actions.push_back(ViewerAction::FocusView(view));
                }
                if view == ViewKind::Pov && presentation_contains(self.context.mode, view) {
                    self.drag = Some(DragState {
                        pointer,
                        last: position,
                    });
                }
            }
            ButtonPhase::Released => {
                if self.drag.is_some_and(|drag| drag.pointer == pointer) {
                    self.drag = None;
                }
            }
        }
        true
    }

    fn handle_wheel(&mut self, delta: WheelDelta, view: ViewKind) -> bool {
        if !self.context.surface_focused
            || !presentation_contains(self.context.mode, view)
            || (self.context.mode == PresentationMode::Split && self.context.focused != view)
        {
            return false;
        }
        let index = match view {
            ViewKind::Map => 0,
            ViewKind::Pov => 1,
        };
        let delta = match delta {
            WheelDelta::Lines(lines) => lines,
            WheelDelta::Pixels(pixels) => pixels / WHEEL_PIXELS_PER_NOTCH,
        };
        self.wheel_residual[index] += delta;
        let steps = take_whole_steps(&mut self.wheel_residual[index]);
        match view {
            ViewKind::Map => {
                for _ in 0..steps.max(0) {
                    self.actions.push_back(ViewerAction::ZoomIn);
                }
                for _ in 0..(-steps).max(0) {
                    self.actions.push_back(ViewerAction::ZoomOut);
                }
            }
            ViewKind::Pov => self.pov_wheel_steps += steps,
        }
        true
    }
}

fn take_whole_steps(residual: &mut f64) -> i32 {
    let mut steps = 0;
    while *residual >= 1.0 {
        *residual -= 1.0;
        steps += 1;
    }
    while *residual <= -1.0 {
        *residual += 1.0;
        steps -= 1;
    }
    steps
}

fn active_view(context: InputContext) -> ViewKind {
    match context.mode {
        PresentationMode::Map => ViewKind::Map,
        PresentationMode::Pov => ViewKind::Pov,
        PresentationMode::Split => context.focused,
    }
}

fn presentation_contains(mode: PresentationMode, view: ViewKind) -> bool {
    match mode {
        PresentationMode::Map => view == ViewKind::Map,
        PresentationMode::Pov => view == ViewKind::Pov,
        PresentationMode::Split => true,
    }
}

const fn view_index(view: ViewKind) -> usize {
    match view {
        ViewKind::Map => 0,
        ViewKind::Pov => 1,
    }
}

fn binding_context_matches(binding: BindingContext, context: InputContext) -> bool {
    match binding {
        BindingContext::Global | BindingContext::FocusedView => true,
        BindingContext::SingleView => context.mode != PresentationMode::Split,
        BindingContext::Split => context.mode == PresentationMode::Split,
        BindingContext::Map => active_view(context) == ViewKind::Map,
        BindingContext::Pov => active_view(context) == ViewKind::Pov,
    }
}

fn shift_matches(binding: ShiftBinding, shift: bool) -> bool {
    match binding {
        ShiftBinding::Any => true,
        ShiftBinding::Required => shift,
        ShiftBinding::Forbidden => !shift,
    }
}

fn is_navigation_key(key: PhysicalKey) -> bool {
    matches!(
        key,
        PhysicalKey::KeyW
            | PhysicalKey::KeyA
            | PhysicalKey::KeyS
            | PhysicalKey::KeyD
            | PhysicalKey::ArrowUp
            | PhysicalKey::ArrowDown
            | PhysicalKey::ArrowLeft
            | PhysicalKey::ArrowRight
            | PhysicalKey::Space
            | PhysicalKey::ShiftLeft
            | PhysicalKey::ShiftRight
    )
}

const fn is_known_key(_key: PhysicalKey) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::action::ACTION_DESCRIPTORS;

    use super::*;

    fn context(mode: PresentationMode, focused: ViewKind) -> InputContext {
        InputContext {
            mode,
            focused,
            surface_focused: true,
        }
    }

    fn key(
        key: PhysicalKey,
        phase: ButtonPhase,
        repeat: bool,
        shift: bool,
    ) -> NormalizedInputEvent {
        NormalizedInputEvent::Key {
            key,
            phase,
            repeat,
            modifiers: Modifiers {
                shift,
                ..Modifiers::default()
            },
        }
    }

    #[test]
    fn dom_codes_round_trip_exactly() {
        for descriptor in BINDING_DESCRIPTORS {
            if let BindingInput::Key(key) = descriptor.input {
                assert_eq!(PhysicalKey::from_dom_code(key.dom_code()), Some(key));
            }
        }
        assert_eq!(PhysicalKey::from_dom_code("v"), None);
        assert_eq!(PhysicalKey::from_dom_code("KeyAA"), None);
    }

    #[test]
    fn binding_ids_are_unique_and_action_metadata_references_them() {
        let mut ids = BTreeSet::new();
        for binding in BINDING_DESCRIPTORS {
            assert!(ids.insert(binding.id), "duplicate binding {}", binding.id);
        }
        for action in ACTION_DESCRIPTORS {
            for binding in action.default_binding_ids {
                assert!(
                    ids.contains(binding),
                    "{} references missing {binding}",
                    action.id.as_str()
                );
            }
        }
    }

    #[test]
    fn context_collisions_resolve_to_one_action() {
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, false, false),
            context(PresentationMode::Map, ViewKind::Map),
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::CycleMapChannel]
        );
        mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Released, false, false),
            context(PresentationMode::Map, ViewKind::Map),
        );
        mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, false, false),
            context(PresentationMode::Pov, ViewKind::Pov),
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::TogglePovWater]
        );
    }

    #[test]
    fn shift_selects_nudge_direction_and_browser_chords_are_rejected() {
        let ctx = context(PresentationMode::Map, ViewKind::Map);
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            key(PhysicalKey::Digit1, ButtonPhase::Pressed, false, true),
            ctx,
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::NudgePossibility {
                domain: PossibilityDomain::Planetary,
                direction: NudgeDirection::Down,
            }]
        );
        mapper.handle_event(
            key(PhysicalKey::Digit1, ButtonPhase::Released, false, true),
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::Key {
                key: PhysicalKey::KeyV,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers {
                    control: true,
                    ..Modifiers::default()
                },
            },
            ctx,
        );
        assert!(mapper.drain_actions().next().is_none());
    }

    #[test]
    fn repeats_and_duplicate_presses_do_not_repeat_one_shots() {
        let ctx = context(PresentationMode::Map, ViewKind::Map);
        let mut mapper = InputMapper::default();
        assert!(mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, false, false),
            ctx
        ));
        assert!(mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, true, false),
            ctx
        ));
        assert!(mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, false, false),
            ctx
        ));
        assert_eq!(mapper.drain_actions().count(), 1);
        mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Released, false, false),
            ctx,
        );
        mapper.handle_event(
            key(PhysicalKey::KeyV, ButtonPhase::Pressed, false, false),
            ctx,
        );
        assert_eq!(mapper.drain_actions().count(), 1);
    }

    #[test]
    fn opposite_axes_cancel_and_diagonal_delta_keeps_legacy_bits() {
        let ctx = context(PresentationMode::Map, ViewKind::Map);
        let mut mapper = InputMapper::default();
        for key_code in [PhysicalKey::KeyW, PhysicalKey::KeyS, PhysicalKey::KeyD] {
            mapper.handle_event(key(key_code, ButtonPhase::Pressed, false, false), ctx);
        }
        assert_eq!(mapper.take_frame().map_axis, [1, 0]);
        mapper.handle_event(
            key(PhysicalKey::KeyS, ButtonPhase::Released, false, false),
            ctx,
        );
        let frame = mapper.take_frame();
        assert_eq!(frame.map_axis, [1, 1]);
        let actual = frame.map_movement_delta(500.0, 0.1).unwrap();
        let step = 500.0f64 * 0.1 / f64::hypot(1.0, 1.0);
        assert_eq!(
            (actual.0.to_bits(), actual.1.to_bits()),
            (step.to_bits(), step.to_bits())
        );
    }

    #[test]
    fn focus_loss_clears_keys_pointer_wheels_and_residuals() {
        let ctx = context(PresentationMode::Pov, ViewKind::Pov);
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            key(PhysicalKey::KeyW, ButtonPhase::Pressed, false, false),
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(25.0),
                view: ViewKind::Pov,
            },
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::PointerButton {
                pointer: 7,
                button: PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position: [4.0, 5.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        mapper.handle_event(NormalizedInputEvent::FocusChanged { focused: false }, ctx);
        mapper.set_context(InputContext {
            surface_focused: true,
            ..ctx
        });
        let frame = mapper.take_frame();
        assert_eq!(frame, InputFrame::default());
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(20.0),
                view: ViewKind::Pov,
            },
            ctx,
        );
        assert_eq!(
            mapper.take_frame().wheel_steps,
            0,
            "wheel residual was cleared"
        );
    }

    #[test]
    fn line_and_pixel_wheels_share_notches_and_keep_per_view_residuals() {
        let map = context(PresentationMode::Map, ViewKind::Map);
        let pov = context(PresentationMode::Pov, ViewKind::Pov);
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(15.0),
                view: ViewKind::Map,
            },
            map,
        );
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(30.0),
                view: ViewKind::Map,
            },
            map,
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::ZoomIn]
        );
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Lines(0.5),
                view: ViewKind::Pov,
            },
            pov,
        );
        mapper.handle_event(
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(20.0),
                view: ViewKind::Pov,
            },
            pov,
        );
        assert_eq!(mapper.take_frame().wheel_steps, 1);
    }

    #[test]
    fn pointer_look_requires_matching_primary_hold_and_cancels() {
        let ctx = context(PresentationMode::Pov, ViewKind::Pov);
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            NormalizedInputEvent::PointerMoved {
                pointer: 2,
                position: [100.0, 100.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        assert_eq!(mapper.take_frame().look_delta, [0.0, 0.0]);
        mapper.handle_event(
            NormalizedInputEvent::PointerButton {
                pointer: 2,
                button: PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position: [100.0, 100.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::PointerMoved {
                pointer: 2,
                position: [112.0, 92.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        let frame = mapper.take_frame();
        assert_eq!(frame.look_delta, [12.0, -8.0]);
        assert!(frame.primary_drag);
        mapper.handle_event(NormalizedInputEvent::PointerCancelled { pointer: 2 }, ctx);
        mapper.handle_event(
            NormalizedInputEvent::PointerMoved {
                pointer: 2,
                position: [130.0, 130.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        assert_eq!(mapper.take_frame().look_delta, [0.0, 0.0]);
    }

    #[test]
    fn pointer_positions_and_modifier_only_changes_are_frame_state() {
        let ctx = context(PresentationMode::Split, ViewKind::Map);
        let mut mapper = InputMapper::default();
        mapper.handle_event(
            NormalizedInputEvent::PointerMoved {
                pointer: 3,
                position: [11.0, 12.0],
                view: ViewKind::Map,
            },
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::PointerMoved {
                pointer: 4,
                position: [21.0, 22.0],
                view: ViewKind::Pov,
            },
            ctx,
        );
        mapper.handle_event(
            NormalizedInputEvent::ModifiersChanged(Modifiers {
                shift: true,
                ..Modifiers::default()
            }),
            ctx,
        );
        let frame = mapper.take_frame();
        assert_eq!(frame.map_pointer, Some([11.0, 12.0]));
        assert_eq!(frame.pov_pointer, Some([21.0, 22.0]));
        assert!(frame.sprint);

        mapper.handle_event(NormalizedInputEvent::PointerCancelled { pointer: 3 }, ctx);
        assert_eq!(mapper.take_frame().map_pointer, None);
    }

    #[test]
    fn continuous_input_reports_only_work_a_future_frame_can_consume() {
        let ctx = context(PresentationMode::Map, ViewKind::Map);
        let mut mapper = InputMapper::default();
        assert!(!mapper.has_continuous_input());
        mapper.handle_event(
            key(PhysicalKey::KeyW, ButtonPhase::Pressed, false, false),
            ctx,
        );
        assert!(mapper.has_continuous_input());
        mapper.handle_event(
            key(PhysicalKey::KeyW, ButtonPhase::Released, false, false),
            ctx,
        );
        assert!(!mapper.has_continuous_input());
    }

    #[test]
    fn tab_is_surface_scoped_and_split_moves_focus() {
        let mut mapper = InputMapper::default();
        let toolbar = InputContext {
            mode: PresentationMode::Map,
            focused: ViewKind::Map,
            surface_focused: false,
        };
        assert!(!mapper.handle_event(
            key(PhysicalKey::Tab, ButtonPhase::Pressed, false, false),
            toolbar
        ));
        assert!(mapper.drain_actions().next().is_none());
        mapper.clear_held_state();
        let split = context(PresentationMode::Split, ViewKind::Map);
        mapper.handle_event(
            key(PhysicalKey::Tab, ButtonPhase::Pressed, false, false),
            split,
        );
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::FocusView(ViewKind::Pov)]
        );
    }
}
