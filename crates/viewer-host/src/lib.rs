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

pub use action::{
    DiscoveryWriteRequest, PreserveMutation, PreserveRequest, RouteWriteRequest, ServiceRequestId,
    SessionWriteRequest, ViewerAction, ViewerEffect,
};
pub use controller::{
    AnalyticGroundSampler, CapturePreferences, GroundSample, LoadedSession, MapPreferences,
    PovGroundSampler, PovStateSnapshot, PresentationDirty, ServiceNotification, ServiceResponse,
    ServiceResponseResult, ServiceResponseSequence, TickInput, TickOutput, ViewerController,
};
pub use inspect::{CellInfo, CursorInfo, EcologyInfo, HoverInfo, OrganismInfo};
pub use layout::{PixelRect, PresentationMode, ViewKind, ViewLayout};
pub use map::{
    map_pixel_hash, pick_organism, Channel, ChannelDescriptor, MapBackend, MapBackendFallback,
    MapDecor, MapDescriptorGroup, MapLayer, MapLayerDescriptor, MapOverlay, MapOverlayDescriptor,
    MapPresenterUpdate, MapProjection, MapRenderPacket, MapRenderRequest, Overlays, PreparedCpuMap,
    PreparedGpuMap, PreparedMapSource, CHANNEL_DESCRIPTORS, MAP_LAYER_DESCRIPTORS,
    MAP_LAYER_SEQUENCE, MAP_OVERLAY_DESCRIPTORS, ORGANISM_PICK_ZOOM,
};
pub use panel::{InfoPanelModel, PanelFieldId, PlatformTelemetry};
pub use world::{
    ExplorationWorld, NoopWorldTickHook, TravelerState, WorldPostUpdate, WorldPreUpdate,
    WorldServiceInput, WorldTickHook, WorldTickOutput,
};
