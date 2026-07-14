//! Shared, platform-neutral viewer contracts.
//!
//! Native and browser shells translate environment events and services at
//! this boundary. This crate owns presentation/controller values, but never
//! accesses a window, DOM, filesystem, socket, or platform thread API (ADR
//! 0028 and `native-web-alignment.md` section 4.2).

pub mod action;
pub mod atlas;
pub mod controller;
pub mod input;
pub mod inspect;
pub mod layout;
pub mod map;
pub mod panel;
pub mod world;

pub use action::{ViewerAction, ViewerEffect};
pub use inspect::{CellInfo, CursorInfo, EcologyInfo, HoverInfo, OrganismInfo};
pub use layout::{PixelRect, PresentationMode, ViewKind, ViewLayout};
pub use map::{Channel, MapDecor, MapOverlay, Overlays};
pub use panel::{InfoPanelModel, PanelFieldId, PlatformTelemetry};
