//! Shared control-surface DTOs (ADR 0028, wry-overlay plan M2).
//!
//! The browser toolbar and the native overlay toolbar render the same shared
//! UI code, so the JSON they consume must have one authority. This module
//! owns the per-frame presentation DTO (`presentation_dto`) and the
//! action/map descriptor serializations that were previously private to the
//! web facade; `platform-web` and the native overlay both delegate here, so
//! the two shells cannot drift.
//!
//! The DTO deliberately excludes the information panel: that document is
//! [`crate::panel::PanelDocument`], cached and revisioned separately, so the
//! panel is never rebuilt or serialized on ordinary animation frames.

use crate::action::{ActionScope, ACTION_DESCRIPTORS};
use crate::controller::{MapPreferences, PovStateSnapshot};
use crate::input::BINDING_DESCRIPTORS;
use crate::layout::{PresentationMode, ViewKind};
use crate::map::{Channel, MapBackend, Overlays};
use crate::{CHANNEL_DESCRIPTORS, MAP_OVERLAY_DESCRIPTORS};

/// Immediate presentation state for toolbar/control chrome. Rebuilt every
/// frame from the tick output plus the platform capability strings; cheap by
/// design (a handful of scalars and `&'static str`s).
#[derive(Debug, serde::Serialize)]
pub struct PresentationDto {
    pub view: ViewPresentation,
    pub map: MapPresentation,
    pub tier: TierPresentation,
    pub executor: ExecutorPresentation,
    pub storage: StoragePresentation,
    pub renderer: RendererPresentation,
    pub decor_status: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct ViewPresentation {
    pub mode: PresentationMode,
    pub focused: ViewKind,
    pub split_ratio: f32,
    pub pov_supported: bool,
    pub pov: PovPresentation,
}

#[derive(Debug, serde::Serialize)]
pub struct PovPresentation {
    pub motion: &'static str,
    pub shadow_ao: bool,
    pub detail_normals: bool,
    pub water: bool,
    pub render_scale: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct MapPresentation {
    pub backend: MapBackend,
    pub channel: Channel,
    pub zoom: u32,
    pub refinement: bool,
    pub overlays: Overlays,
}

#[derive(Debug, serde::Serialize)]
pub struct TierPresentation {
    pub runtime: &'static str,
    pub benchmark_ms: f32,
}

#[derive(Debug, serde::Serialize)]
pub struct ExecutorPresentation {
    pub mode: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct StoragePresentation {
    pub mode: &'static str,
}

#[derive(Debug, serde::Serialize)]
pub struct RendererPresentation {
    pub mode: &'static str,
    pub device_losses: u32,
}

/// The capability facts only a shell knows: executor/storage backends, the
/// live renderer mode, and the startup benchmark. Everything else in the DTO
/// derives from shared controller state.
#[derive(Debug, Clone, Copy)]
pub struct PresentationPlatform {
    pub worker_mode: &'static str,
    pub storage: &'static str,
    pub renderer: &'static str,
    pub device_losses: u32,
    pub benchmark_ms: f32,
    pub decor_status: &'static str,
}

/// Assemble the per-frame presentation DTO. Field-for-field this is the
/// contract `ui/toolbar.js` (`syncControls`) renders on both shells.
#[must_use]
pub fn presentation_dto(
    mode: PresentationMode,
    focused: ViewKind,
    split_ratio: f32,
    pov: &PovStateSnapshot,
    map: MapPreferences,
    tier_name: &'static str,
    platform: PresentationPlatform,
) -> PresentationDto {
    PresentationDto {
        view: ViewPresentation {
            mode,
            focused,
            split_ratio,
            pov_supported: pov.supported,
            pov: PovPresentation {
                motion: if pov.walk { "walk" } else { "fly" },
                shadow_ao: pov.shadow_ao,
                detail_normals: pov.detail_normals,
                water: pov.water,
                render_scale: pov.render_scale,
            },
        },
        map: MapPresentation {
            backend: map.backend,
            channel: map.channel,
            zoom: map.zoom,
            refinement: map.refinement,
            overlays: map.overlays,
        },
        tier: TierPresentation {
            runtime: tier_name,
            benchmark_ms: platform.benchmark_ms,
        },
        executor: ExecutorPresentation {
            mode: platform.worker_mode,
        },
        storage: StoragePresentation {
            mode: platform.storage,
        },
        renderer: RendererPresentation {
            mode: platform.renderer,
            device_losses: platform.device_losses,
        },
        decor_status: platform.decor_status,
    }
}

/// Serialize the shared action registry (with each action's default binding
/// help) for toolbar/help construction. One authority for both shells and the
/// help route.
#[must_use]
pub fn action_descriptors_json() -> String {
    let descriptors = ACTION_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            let scope = match descriptor.scope {
                ActionScope::Global => "global",
                ActionScope::FocusedView => "focused-view",
                ActionScope::Map => "map",
                ActionScope::Pov => "pov",
            };
            let bindings = descriptor
                .default_binding_ids
                .iter()
                .map(|id| {
                    let binding = BINDING_DESCRIPTORS
                        .iter()
                        .find(|binding| binding.id == *id)
                        .expect("action descriptor names a registered binding");
                    serde_json::json!({
                        "id": binding.id,
                        "help": binding.help,
                    })
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": descriptor.id.as_str(),
                "label": descriptor.label,
                "help": descriptor.help,
                "scope": scope,
                "bindings": bindings,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&descriptors).expect("action descriptors are serializable")
}

/// Serialize the shared map channel/overlay descriptor registry for
/// `ui/toolbar.js` `installMapControls` on both shells.
#[must_use]
pub fn map_descriptors_json() -> String {
    let channels = CHANNEL_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            serde_json::json!({
                "id": descriptor.id,
                "label": descriptor.label,
                "group": descriptor.group.id(),
                "group_label": descriptor.group.label(),
                "order": descriptor.order,
            })
        })
        .collect::<Vec<_>>();
    let overlays = MAP_OVERLAY_DESCRIPTORS
        .iter()
        .map(|descriptor| {
            serde_json::json!({
                "id": descriptor.id,
                "label": descriptor.label,
                "group": descriptor.group.id(),
                "group_label": descriptor.group.label(),
                "order": descriptor.order,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&serde_json::json!({
        "channels": channels,
        "overlays": overlays,
    }))
    .expect("map descriptors are serializable")
}
