//! Thin winit-to-`viewer_host::input` adapter (alignment plan Milestone 2).
//!
//! This module owns only winit spellings and the transient modifier/cursor
//! values needed to assemble normalized events. Bindings, held keys, repeat
//! suppression, wheel fractions, and POV drag state all live in the shared
//! [`viewer_host::input::InputMapper`].

use viewer_host::input::{
    ButtonPhase, Modifiers, NormalizedInputEvent, PhysicalKey as ViewerPhysicalKey, PointerButton,
    WheelDelta,
};
use viewer_host::ViewKind;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::keyboard::{KeyCode, ModifiersState};

/// Stable pointer identity for winit's single mouse cursor.
pub(crate) const MOUSE_POINTER_ID: u64 = 0;

/// Platform transport state that is not a viewer behavior authority.
#[derive(Debug)]
pub(crate) struct WinitInputAdapter {
    modifiers: Modifiers,
    cursor: Option<[f64; 2]>,
    surface_focused: bool,
    /// Native has one mouse pointer. Once a primary press starts in POV,
    /// keep transporting that gesture as POV until release/cancel even if
    /// the cursor crosses the Split seam. The shared mapper still owns the
    /// actual drag/look gate.
    primary_pov_drag: bool,
}

impl Default for WinitInputAdapter {
    fn default() -> Self {
        Self {
            modifiers: Modifiers::default(),
            cursor: None,
            surface_focused: true,
            primary_pov_drag: false,
        }
    }
}

impl WinitInputAdapter {
    pub(crate) fn surface_focused(&self) -> bool {
        self.surface_focused
    }

    /// Last physical surface position supplied by winit, used only to route
    /// button/wheel gestures through the shared visible pane rectangle.
    pub(crate) const fn cursor_position(&self) -> Option<[f64; 2]> {
        self.cursor
    }

    pub(crate) fn modifiers_changed(&mut self, state: ModifiersState) -> NormalizedInputEvent {
        self.modifiers = modifiers(state);
        NormalizedInputEvent::ModifiersChanged(self.modifiers)
    }

    pub(crate) fn key_event(
        &self,
        code: KeyCode,
        state: ElementState,
        repeat: bool,
    ) -> Option<NormalizedInputEvent> {
        Some(NormalizedInputEvent::Key {
            key: physical_key(code)?,
            phase: button_phase(state),
            repeat,
            modifiers: self.modifiers,
        })
    }

    pub(crate) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        hit_view: Option<ViewKind>,
    ) -> NormalizedInputEvent {
        let position = [position.x, position.y];
        self.cursor = Some(position);
        if let Some(view) = self.primary_pov_drag.then_some(ViewKind::Pov).or(hit_view) {
            NormalizedInputEvent::PointerMoved {
                pointer: MOUSE_POINTER_ID,
                position,
                view,
            }
        } else {
            NormalizedInputEvent::PointerCancelled {
                pointer: MOUSE_POINTER_ID,
            }
        }
    }

    pub(crate) fn cursor_left(&mut self) -> NormalizedInputEvent {
        self.cursor = None;
        self.primary_pov_drag = false;
        NormalizedInputEvent::PointerCancelled {
            pointer: MOUSE_POINTER_ID,
        }
    }

    /// Cancel adapter-only pointer capture after the renderer loses every
    /// presentation pane. The shared mapper clears the semantic drag and held
    /// inputs separately; this keeps later Map motion from being transported
    /// through the old POV press view while waiting for a physical release.
    pub(crate) fn cancel_pointer_gesture(&mut self) {
        self.primary_pov_drag = false;
    }

    pub(crate) fn mouse_input(
        &mut self,
        state: ElementState,
        button: MouseButton,
        hit_view: Option<ViewKind>,
    ) -> Option<NormalizedInputEvent> {
        let position = self.cursor?;
        let button = pointer_button(button);
        let phase = button_phase(state);
        let view = match (button, phase) {
            (PointerButton::Primary, ButtonPhase::Pressed) => {
                let view = hit_view?;
                self.primary_pov_drag = view == ViewKind::Pov;
                view
            }
            (PointerButton::Primary, ButtonPhase::Released) => {
                // The captured view owns moves, but release should leave the
                // pointer in the pane physically under it. Falling back to
                // POV only keeps an outside-pane release able to end a drag.
                let view = hit_view.or(self.primary_pov_drag.then_some(ViewKind::Pov))?;
                self.primary_pov_drag = false;
                view
            }
            (_, _) => hit_view?,
        };
        Some(NormalizedInputEvent::PointerButton {
            pointer: MOUSE_POINTER_ID,
            button,
            phase,
            // Match the previous native gesture: a press received before any
            // cursor position does not arm a drag from a fabricated origin.
            position,
            view,
        })
    }

    pub(crate) fn wheel(&self, delta: MouseScrollDelta, view: ViewKind) -> NormalizedInputEvent {
        let delta = match delta {
            MouseScrollDelta::LineDelta(_, y) => WheelDelta::Lines(f64::from(y)),
            MouseScrollDelta::PixelDelta(position) => WheelDelta::Pixels(position.y),
        };
        NormalizedInputEvent::Wheel { delta, view }
    }

    pub(crate) fn focus_changed(&mut self, focused: bool) -> NormalizedInputEvent {
        self.surface_focused = focused;
        if !focused {
            self.modifiers = Modifiers::default();
            self.cursor = None;
            self.primary_pov_drag = false;
        }
        NormalizedInputEvent::FocusChanged { focused }
    }
}

fn button_phase(state: ElementState) -> ButtonPhase {
    match state {
        ElementState::Pressed => ButtonPhase::Pressed,
        ElementState::Released => ButtonPhase::Released,
    }
}

fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        super_key: state.super_key(),
    }
}

fn pointer_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Middle => PointerButton::Auxiliary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Back => PointerButton::Back,
        MouseButton::Forward => PointerButton::Forward,
        MouseButton::Other(button) => PointerButton::Other(button),
    }
}

fn physical_key(code: KeyCode) -> Option<ViewerPhysicalKey> {
    Some(match code {
        KeyCode::KeyA => ViewerPhysicalKey::KeyA,
        KeyCode::KeyB => ViewerPhysicalKey::KeyB,
        KeyCode::KeyC => ViewerPhysicalKey::KeyC,
        KeyCode::KeyD => ViewerPhysicalKey::KeyD,
        KeyCode::KeyE => ViewerPhysicalKey::KeyE,
        KeyCode::KeyF => ViewerPhysicalKey::KeyF,
        KeyCode::KeyG => ViewerPhysicalKey::KeyG,
        KeyCode::KeyH => ViewerPhysicalKey::KeyH,
        KeyCode::KeyI => ViewerPhysicalKey::KeyI,
        KeyCode::KeyJ => ViewerPhysicalKey::KeyJ,
        KeyCode::KeyK => ViewerPhysicalKey::KeyK,
        KeyCode::KeyL => ViewerPhysicalKey::KeyL,
        KeyCode::KeyM => ViewerPhysicalKey::KeyM,
        KeyCode::KeyN => ViewerPhysicalKey::KeyN,
        KeyCode::KeyO => ViewerPhysicalKey::KeyO,
        KeyCode::KeyP => ViewerPhysicalKey::KeyP,
        KeyCode::KeyQ => ViewerPhysicalKey::KeyQ,
        KeyCode::KeyR => ViewerPhysicalKey::KeyR,
        KeyCode::KeyS => ViewerPhysicalKey::KeyS,
        KeyCode::KeyT => ViewerPhysicalKey::KeyT,
        KeyCode::KeyU => ViewerPhysicalKey::KeyU,
        KeyCode::KeyV => ViewerPhysicalKey::KeyV,
        KeyCode::KeyW => ViewerPhysicalKey::KeyW,
        KeyCode::KeyX => ViewerPhysicalKey::KeyX,
        KeyCode::KeyY => ViewerPhysicalKey::KeyY,
        KeyCode::KeyZ => ViewerPhysicalKey::KeyZ,
        KeyCode::Digit1 => ViewerPhysicalKey::Digit1,
        KeyCode::Digit2 => ViewerPhysicalKey::Digit2,
        KeyCode::Digit3 => ViewerPhysicalKey::Digit3,
        KeyCode::Digit4 => ViewerPhysicalKey::Digit4,
        KeyCode::Digit5 => ViewerPhysicalKey::Digit5,
        KeyCode::Digit6 => ViewerPhysicalKey::Digit6,
        KeyCode::Digit7 => ViewerPhysicalKey::Digit7,
        KeyCode::Digit8 => ViewerPhysicalKey::Digit8,
        KeyCode::ArrowUp => ViewerPhysicalKey::ArrowUp,
        KeyCode::ArrowDown => ViewerPhysicalKey::ArrowDown,
        KeyCode::ArrowLeft => ViewerPhysicalKey::ArrowLeft,
        KeyCode::ArrowRight => ViewerPhysicalKey::ArrowRight,
        KeyCode::Space => ViewerPhysicalKey::Space,
        KeyCode::ShiftLeft => ViewerPhysicalKey::ShiftLeft,
        KeyCode::ShiftRight => ViewerPhysicalKey::ShiftRight,
        KeyCode::Tab => ViewerPhysicalKey::Tab,
        KeyCode::Escape => ViewerPhysicalKey::Escape,
        KeyCode::Delete => ViewerPhysicalKey::Delete,
        KeyCode::F12 => ViewerPhysicalKey::F12,
        KeyCode::Comma => ViewerPhysicalKey::Comma,
        KeyCode::Period => ViewerPhysicalKey::Period,
        _ => return None,
    })
}

/// One compact native HUD help row plus the canonical bindings it summarizes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeHelpRow {
    pub(crate) keys: &'static str,
    pub(crate) action: &'static str,
    pub(crate) binding_ids: &'static [&'static str],
}

/// The bitmap HUD consumes these rows; the test below validates their ids
/// against the shared registry so native help cannot silently invent keys.
pub(crate) const NATIVE_HELP_ROWS: &[NativeHelpRow] = &[
    NativeHelpRow {
        keys: "WASD 1-8 Z",
        action: "move, bias, reset",
        binding_ids: &[
            "focused-navigation",
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
            "map-key-z",
        ],
    },
    NativeHelpRow {
        keys: "E / Q / C",
        action: "anchors, clear",
        binding_ids: &["map-key-e", "map-key-q", "map-key-c"],
    },
    NativeHelpRow {
        keys: "T Y K",
        action: "categ,polar,capture",
        binding_ids: &["map-key-t", "map-key-y", "map-key-k"],
    },
    NativeHelpRow {
        keys: "R",
        action: "transition mode",
        binding_ids: &["map-key-r"],
    },
    NativeHelpRow {
        keys: "H J U Del",
        action: "paths: on,rec,attr,clr",
        binding_ids: &["map-key-h", "map-key-j", "map-key-u", "map-delete"],
    },
    NativeHelpRow {
        keys: "V F G N X M",
        action: "channel,overlays",
        binding_ids: &[
            "map-key-v",
            "map-key-f",
            "map-key-g",
            "map-key-n",
            "map-key-x",
            "map-key-m",
        ],
    },
    NativeHelpRow {
        keys: "scroll",
        action: "zoom (organism info)",
        binding_ids: &["map-wheel-positive", "map-wheel-negative"],
    },
];

#[cfg(test)]
mod tests {
    use viewer_host::input::{BindingInput, InputContext, InputMapper, BINDING_DESCRIPTORS};
    use viewer_host::{PresentationMode, ViewerAction};

    use super::*;

    #[test]
    fn every_supported_winit_key_maps_to_the_same_physical_code() {
        let cases = [
            (KeyCode::KeyA, ViewerPhysicalKey::KeyA),
            (KeyCode::KeyB, ViewerPhysicalKey::KeyB),
            (KeyCode::KeyC, ViewerPhysicalKey::KeyC),
            (KeyCode::KeyD, ViewerPhysicalKey::KeyD),
            (KeyCode::KeyE, ViewerPhysicalKey::KeyE),
            (KeyCode::KeyF, ViewerPhysicalKey::KeyF),
            (KeyCode::KeyG, ViewerPhysicalKey::KeyG),
            (KeyCode::KeyH, ViewerPhysicalKey::KeyH),
            (KeyCode::KeyI, ViewerPhysicalKey::KeyI),
            (KeyCode::KeyJ, ViewerPhysicalKey::KeyJ),
            (KeyCode::KeyK, ViewerPhysicalKey::KeyK),
            (KeyCode::KeyL, ViewerPhysicalKey::KeyL),
            (KeyCode::KeyM, ViewerPhysicalKey::KeyM),
            (KeyCode::KeyN, ViewerPhysicalKey::KeyN),
            (KeyCode::KeyO, ViewerPhysicalKey::KeyO),
            (KeyCode::KeyP, ViewerPhysicalKey::KeyP),
            (KeyCode::KeyQ, ViewerPhysicalKey::KeyQ),
            (KeyCode::KeyR, ViewerPhysicalKey::KeyR),
            (KeyCode::KeyS, ViewerPhysicalKey::KeyS),
            (KeyCode::KeyT, ViewerPhysicalKey::KeyT),
            (KeyCode::KeyU, ViewerPhysicalKey::KeyU),
            (KeyCode::KeyV, ViewerPhysicalKey::KeyV),
            (KeyCode::KeyW, ViewerPhysicalKey::KeyW),
            (KeyCode::KeyX, ViewerPhysicalKey::KeyX),
            (KeyCode::KeyY, ViewerPhysicalKey::KeyY),
            (KeyCode::KeyZ, ViewerPhysicalKey::KeyZ),
            (KeyCode::Digit1, ViewerPhysicalKey::Digit1),
            (KeyCode::Digit2, ViewerPhysicalKey::Digit2),
            (KeyCode::Digit3, ViewerPhysicalKey::Digit3),
            (KeyCode::Digit4, ViewerPhysicalKey::Digit4),
            (KeyCode::Digit5, ViewerPhysicalKey::Digit5),
            (KeyCode::Digit6, ViewerPhysicalKey::Digit6),
            (KeyCode::Digit7, ViewerPhysicalKey::Digit7),
            (KeyCode::Digit8, ViewerPhysicalKey::Digit8),
            (KeyCode::ArrowUp, ViewerPhysicalKey::ArrowUp),
            (KeyCode::ArrowDown, ViewerPhysicalKey::ArrowDown),
            (KeyCode::ArrowLeft, ViewerPhysicalKey::ArrowLeft),
            (KeyCode::ArrowRight, ViewerPhysicalKey::ArrowRight),
            (KeyCode::Space, ViewerPhysicalKey::Space),
            (KeyCode::ShiftLeft, ViewerPhysicalKey::ShiftLeft),
            (KeyCode::ShiftRight, ViewerPhysicalKey::ShiftRight),
            (KeyCode::Tab, ViewerPhysicalKey::Tab),
            (KeyCode::Escape, ViewerPhysicalKey::Escape),
            (KeyCode::Delete, ViewerPhysicalKey::Delete),
            (KeyCode::F12, ViewerPhysicalKey::F12),
            (KeyCode::Comma, ViewerPhysicalKey::Comma),
            (KeyCode::Period, ViewerPhysicalKey::Period),
        ];
        for (winit, shared) in cases {
            assert_eq!(physical_key(winit), Some(shared));
            assert_eq!(
                ViewerPhysicalKey::from_dom_code(shared.dom_code()),
                Some(shared)
            );
        }
        assert_eq!(physical_key(KeyCode::F11), None);
    }

    #[test]
    fn adapter_preserves_units_pointer_positions_and_modifier_snapshots() {
        let mut adapter = WinitInputAdapter::default();
        assert_eq!(
            adapter.modifiers_changed(ModifiersState::SHIFT | ModifiersState::CONTROL),
            NormalizedInputEvent::ModifiersChanged(Modifiers {
                shift: true,
                control: true,
                alt: false,
                super_key: false,
            })
        );
        assert_eq!(
            adapter.key_event(KeyCode::Digit1, ElementState::Pressed, false),
            Some(NormalizedInputEvent::Key {
                key: ViewerPhysicalKey::Digit1,
                phase: ButtonPhase::Pressed,
                repeat: false,
                modifiers: Modifiers {
                    shift: true,
                    control: true,
                    alt: false,
                    super_key: false
                },
            })
        );
        assert_eq!(
            adapter.wheel(
                MouseScrollDelta::PixelDelta(PhysicalPosition::new(3.0, 15.0)),
                ViewKind::Map
            ),
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Pixels(15.0),
                view: ViewKind::Map
            }
        );
        assert_eq!(
            adapter.wheel(MouseScrollDelta::LineDelta(2.0, -0.5), ViewKind::Pov),
            NormalizedInputEvent::Wheel {
                delta: WheelDelta::Lines(-0.5),
                view: ViewKind::Pov
            }
        );
        let moved = adapter.cursor_moved(PhysicalPosition::new(12.0, 9.0), Some(ViewKind::Pov));
        assert_eq!(
            moved,
            NormalizedInputEvent::PointerMoved {
                pointer: MOUSE_POINTER_ID,
                position: [12.0, 9.0],
                view: ViewKind::Pov
            }
        );
        assert_eq!(
            adapter.mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov)
            ),
            Some(NormalizedInputEvent::PointerButton {
                pointer: MOUSE_POINTER_ID,
                button: PointerButton::Primary,
                phase: ButtonPhase::Pressed,
                position: [12.0, 9.0],
                view: ViewKind::Pov
            })
        );
        assert_eq!(pointer_button(MouseButton::Back), PointerButton::Back);
        assert_eq!(pointer_button(MouseButton::Forward), PointerButton::Forward);
        assert_eq!(
            adapter.cursor_left(),
            NormalizedInputEvent::PointerCancelled {
                pointer: MOUSE_POINTER_ID
            }
        );
    }

    #[test]
    fn primary_pov_drag_keeps_its_press_view_until_release() {
        let mut adapter = WinitInputAdapter::default();
        let _ = adapter.cursor_moved(PhysicalPosition::new(20.0, 30.0), Some(ViewKind::Pov));
        let pressed = adapter
            .mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .unwrap();
        assert!(matches!(
            pressed,
            NormalizedInputEvent::PointerButton {
                view: ViewKind::Pov,
                phase: ButtonPhase::Pressed,
                ..
            }
        ));

        let crossed = adapter.cursor_moved(PhysicalPosition::new(220.0, 30.0), Some(ViewKind::Map));
        assert!(matches!(
            crossed,
            NormalizedInputEvent::PointerMoved {
                view: ViewKind::Pov,
                ..
            }
        ));
        let released = adapter
            .mouse_input(ElementState::Released, MouseButton::Left, None)
            .unwrap();
        assert!(matches!(
            released,
            NormalizedInputEvent::PointerButton {
                view: ViewKind::Pov,
                phase: ButtonPhase::Released,
                ..
            }
        ));
        let after = adapter.cursor_moved(PhysicalPosition::new(220.0, 30.0), None);
        assert!(matches!(
            after,
            NormalizedInputEvent::PointerCancelled { .. }
        ));
    }

    #[test]
    fn primary_pov_release_prefers_the_current_hit_pane() {
        let mut adapter = WinitInputAdapter::default();
        let _ = adapter.cursor_moved(PhysicalPosition::new(20.0, 30.0), Some(ViewKind::Pov));
        let _ = adapter.mouse_input(
            ElementState::Pressed,
            MouseButton::Left,
            Some(ViewKind::Pov),
        );
        let _ = adapter.cursor_moved(PhysicalPosition::new(220.0, 30.0), Some(ViewKind::Map));
        let released = adapter
            .mouse_input(
                ElementState::Released,
                MouseButton::Left,
                Some(ViewKind::Map),
            )
            .unwrap();
        assert!(matches!(
            released,
            NormalizedInputEvent::PointerButton {
                view: ViewKind::Map,
                phase: ButtonPhase::Released,
                ..
            }
        ));
    }

    #[test]
    fn split_pov_press_focuses_before_followup_input_and_panel_press_is_inert() {
        let mut adapter = WinitInputAdapter::default();
        let mut mapper = InputMapper::default();
        let map_focus = InputContext {
            mode: PresentationMode::Split,
            focused: ViewKind::Map,
            surface_focused: true,
        };
        let moved = adapter.cursor_moved(PhysicalPosition::new(20.0, 30.0), Some(ViewKind::Pov));
        assert!(mapper.handle_event(moved, map_focus));
        let pressed = adapter
            .mouse_input(
                ElementState::Pressed,
                MouseButton::Left,
                Some(ViewKind::Pov),
            )
            .unwrap();
        assert!(mapper.handle_event(pressed, map_focus));
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::FocusView(ViewKind::Pov)]
        );
        mapper.set_context(InputContext {
            focused: ViewKind::Pov,
            ..map_focus
        });
        assert!(mapper.take_frame().primary_drag);

        let released = adapter
            .mouse_input(ElementState::Released, MouseButton::Left, None)
            .unwrap();
        assert!(mapper.handle_event(
            released,
            InputContext {
                focused: ViewKind::Pov,
                ..map_focus
            }
        ));
        let _ = adapter.cursor_moved(PhysicalPosition::new(900.0, 30.0), None);
        assert_eq!(
            adapter.mouse_input(ElementState::Pressed, MouseButton::Left, None),
            None,
            "the information panel is not a focusable view pane"
        );
    }

    #[test]
    fn native_key_and_direct_action_share_the_ordered_consumer() {
        let mut adapter = WinitInputAdapter::default();
        let context = InputContext {
            mode: PresentationMode::Map,
            focused: ViewKind::Map,
            surface_focused: true,
        };
        let mut mapper = InputMapper::default();
        let key = adapter
            .key_event(KeyCode::KeyV, ElementState::Pressed, false)
            .unwrap();
        assert!(mapper.handle_event(key, context));
        mapper.enqueue_action(ViewerAction::CycleMapChannel);
        assert_eq!(
            mapper.drain_actions().collect::<Vec<_>>(),
            vec![ViewerAction::CycleMapChannel, ViewerAction::CycleMapChannel]
        );

        let focus = adapter.focus_changed(false);
        assert!(mapper.handle_event(focus, context));
        assert_eq!(
            mapper.take_frame(),
            viewer_host::input::InputFrame::default()
        );
    }

    #[test]
    fn bitmap_help_rows_reference_only_canonical_bindings() {
        for row in NATIVE_HELP_ROWS {
            assert!(!row.keys.is_empty());
            assert!(!row.action.is_empty());
            for id in row.binding_ids {
                assert!(
                    BINDING_DESCRIPTORS.iter().any(|binding| binding.id == *id),
                    "missing binding {id}"
                );
            }
        }
        // Native help's movement row is tied to the held-input descriptor,
        // not to a hand-authored second movement binding table.
        assert!(BINDING_DESCRIPTORS.iter().any(|binding| {
            binding.id == "focused-navigation" && binding.input == BindingInput::NavigationKeys
        }));
    }
}
