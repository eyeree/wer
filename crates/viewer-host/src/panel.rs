//! Shared semantic information model and section document.
//!
//! Sampling lives in [`crate::inspect`]. This module performs the remaining
//! platform-neutral information processing exactly once and supplies stable,
//! formatted fields to both the native bitmap HUD and browser DOM
//! (`native-web-alignment.md` section 5.7). Platform renderers choose pixels or
//! elements; they do not reinterpret world values.

use std::borrow::Cow;

use world_core::{Anchor, AnchorKind, AnchorSource, RegionCoord, LAYER_COUNT, POSSIBILITY_DIMS};
use world_runtime::{FrameStats, PASS_COUNT};

use crate::action::WorkerBackend;
use crate::controller::{CapturePreferences, PovStateSnapshot, TickOutput};
use crate::inspect::{CellInfo, CellStatus, HoverInfo, OrganismInfo};
use crate::layout::{PresentationMode, ViewKind};
use crate::map::{Channel, MapBackend, MapBackendFallback, Overlays};
use crate::world::ExplorationWorld;

/// Stable field identity used by native and accessible DOM panel renderers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(transparent)]
pub struct PanelFieldId(Cow<'static, str>);

impl PanelFieldId {
    /// Declare a static stable field id.
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Construct a stable id whose suffix comes from typed runtime metadata.
    #[must_use]
    pub fn owned(id: String) -> Self {
        Self(Cow::Owned(id))
    }

    /// Exact id used at the platform boundary.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Common ids used directly by adapters and tests.
pub mod field_ids {
    use super::PanelFieldId;

    pub const FRAME_FPS: PanelFieldId = PanelFieldId::new("frame.fps");
    pub const VIEW_MODE: PanelFieldId = PanelFieldId::new("view.mode");
    pub const VIEW_FOCUS: PanelFieldId = PanelFieldId::new("view.focus");
    pub const MAP_CHANNEL: PanelFieldId = PanelFieldId::new("map.channel");
    pub const TRAVELER_X: PanelFieldId = PanelFieldId::new("traveler.x");
    pub const TRAVELER_Y: PanelFieldId = PanelFieldId::new("traveler.y");
}

/// Severity for a warning or panel field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A typed warning shown by either panel renderer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ViewerWarning {
    pub id: &'static str,
    pub message: String,
    pub severity: Severity,
}

/// Stable-id warning storage shared by thin platform adapters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WarningRegistry {
    warnings: Vec<ViewerWarning>,
    revision: u64,
}

impl WarningRegistry {
    /// Add or replace a warning by stable id. Returns whether visible state
    /// changed, allowing a shell to invalidate only the panel.
    pub fn upsert(&mut self, warning: ViewerWarning) -> bool {
        if let Some(existing) = self
            .warnings
            .iter_mut()
            .find(|entry| entry.id == warning.id)
        {
            if *existing == warning {
                return false;
            }
            *existing = warning;
        } else {
            self.warnings.push(warning);
            self.warnings.sort_by_key(|entry| entry.id);
        }
        self.revision = self.revision.saturating_add(1);
        true
    }

    /// Remove one warning after recovery.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.warnings.len();
        self.warnings.retain(|warning| warning.id != id);
        if self.warnings.len() == before {
            return false;
        }
        self.revision = self.revision.saturating_add(1);
        true
    }

    #[must_use]
    pub fn warnings(&self) -> &[ViewerWarning] {
        &self.warnings
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// Measurements gathered by a platform shell and injected into shared viewer
/// state. `viewer-host` never queries a window, DOM, executor, or storage API.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PlatformTelemetry {
    pub present_ms: f64,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub dom_updates: u64,
    pub surface_format: Option<String>,
    pub executor_backend: WorkerBackend,
    pub workers: usize,
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

/// Monotonic logical state represented by a panel document.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct FrameInfo {
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub sequence: u64,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub update_serial: u64,
    pub traveler: (f64, f64),
}

/// Full shared POV state used by either information renderer.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CameraInfo {
    pub position: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fly_speed: f64,
    pub walk: bool,
    pub walk_speed: f64,
    pub shadow_ao: bool,
    pub detail_normals: bool,
    pub water: bool,
    pub render_scale: f32,
    pub initialized: bool,
    pub supported: bool,
}

impl From<PovStateSnapshot> for CameraInfo {
    fn from(pov: PovStateSnapshot) -> Self {
        Self {
            position: pov.position,
            yaw: pov.yaw,
            pitch: pov.pitch,
            fly_speed: pov.fly_speed,
            walk: pov.walk,
            walk_speed: pov.walk_speed,
            shadow_ao: pov.shadow_ao,
            detail_normals: pov.detail_normals,
            water: pov.water,
            render_scale: pov.render_scale,
            initialized: pov.initialized,
            supported: pov.supported,
        }
    }
}

/// Renderer truth supplied by the shell after map preparation/device state.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RendererInfo {
    pub requested_map_backend: MapBackend,
    pub effective_map_backend: MapBackend,
    pub map_fallback: Option<MapBackendFallback>,
    pub surface_format: Option<String>,
    pub device_losses: u32,
    pub surface_losses: u32,
}

impl Default for RendererInfo {
    fn default() -> Self {
        Self {
            requested_map_backend: MapBackend::Cpu,
            effective_map_backend: MapBackend::Cpu,
            map_fallback: None,
            surface_format: None,
            device_losses: 0,
            surface_losses: 0,
        }
    }
}

/// View-specific portion of the shared information model.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ViewInfo {
    pub mode: PresentationMode,
    pub focused: ViewKind,
    pub map_channel: Channel,
    pub map_zoom: u32,
    pub map_backend: MapBackend,
    pub map_overlays: Overlays,
    pub map_refinement: bool,
    pub split_ratio: f32,
    pub camera: CameraInfo,
    pub renderer: RendererInfo,
}

/// Capped performance sample. Adapters update this only at their telemetry
/// cadence; a renderer never measures or formats it itself.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PerformanceInfo {
    pub fps: u32,
    pub update_ms: f64,
    pub compose_ms: f64,
    pub present_ms: f64,
    pub upload_kib_per_frame: f64,
    pub pass_ms: [f32; PASS_COUNT],
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub dom_updates: u64,
}

impl Default for PerformanceInfo {
    fn default() -> Self {
        Self {
            fps: 0,
            update_ms: 0.0,
            compose_ms: 0.0,
            present_ms: 0.0,
            upload_kib_per_frame: 0.0,
            pass_ms: [0.0; PASS_COUNT],
            dom_updates: 0,
        }
    }
}

/// Values accumulated outside the one runtime update but still derived from
/// platform-neutral world state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingSupplement {
    pub regen_totals: [u64; LAYER_COUNT as usize],
    pub macro_tiles: usize,
    pub rosters: usize,
    pub organisms: usize,
    pub jobs_in_flight: usize,
    pub pinned_violations: u64,
}

impl Default for StreamingSupplement {
    fn default() -> Self {
        Self {
            regen_totals: [0; LAYER_COUNT as usize],
            macro_tiles: 0,
            rosters: 0,
            organisms: 0,
            jobs_in_flight: 0,
            pinned_violations: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LayerRegenInfo {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub total: u64,
}

/// Runtime/cache/ecology counters for the latest shared world update.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StreamingInfo {
    pub tier: &'static str,
    pub cache_ceiling_bytes: usize,
    pub stats: FrameStats,
    pub regen_by_layer: Vec<LayerRegenInfo>,
    pub macro_tiles: usize,
    pub rosters: usize,
    pub organisms: usize,
    pub jobs_in_flight: usize,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub pinned_violations: u64,
    pub executor_backend: WorkerBackend,
    pub workers: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnchorInfo {
    pub kind: &'static str,
    pub source: String,
    pub world: (f64, f64),
    pub strength: f32,
}

/// Possibility steering and capture selection.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SteeringInfo {
    pub current: Option<[f32; POSSIBILITY_DIMS]>,
    pub target: Option<[f32; POSSIBILITY_DIMS]>,
    pub bias: [f32; POSSIBILITY_DIMS],
    pub anchors: Vec<AnchorInfo>,
    pub capture_category: &'static str,
    pub capture_polarity: &'static str,
    pub transition_mode: bool,
}

/// Durable record counters, moved from the native bitmap renderer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct VaultInfo {
    pub records: usize,
    pub dirty: usize,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub seen: u64,
    pub issues: usize,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub suppressed_issues: u64,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub persistence_retries: u64,
}

/// Platform storage capability plus shared route state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PersistenceInfo {
    pub mode: String,
    pub available: bool,
    pub vault: Option<VaultInfo>,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub pending_writes: u64,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub failures: u64,
    pub path_tracking: bool,
    pub route_recording: bool,
    pub route_attraction: bool,
}

impl Default for PersistenceInfo {
    fn default() -> Self {
        Self {
            mode: String::from("unavailable"),
            available: false,
            vault: None,
            pending_writes: 0,
            failures: 0,
            path_tracking: false,
            route_recording: false,
            route_attraction: false,
        }
    }
}

/// Cross-platform semantic information model.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InfoPanelModel {
    pub frame: FrameInfo,
    pub view: ViewInfo,
    pub performance: PerformanceInfo,
    pub streaming: StreamingInfo,
    pub steering: SteeringInfo,
    pub persistence: PersistenceInfo,
    pub hover: HoverInfo,
    pub warnings: Vec<ViewerWarning>,
}

/// One of the desktop dock's three stable columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelColumn {
    Explorer,
    World,
    System,
}

impl PanelColumn {
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Explorer => 0,
            Self::World => 1,
            Self::System => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSpan {
    Single,
    Wide,
}

/// Final shared field. `value` is the only display spelling either renderer
/// may show, including units, precision, missing markers, and exact hex ids.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PanelField {
    pub id: PanelFieldId,
    pub label: &'static str,
    pub value: String,
    pub severity: Severity,
    pub span: PanelSpan,
    pub visible: bool,
}

impl PanelField {
    fn info(id: &'static str, label: &'static str, value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            id: PanelFieldId::new(id),
            label,
            value: ascii_display(&value),
            severity: Severity::Info,
            span: PanelSpan::Single,
            visible: true,
        }
    }

    fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    fn wide(mut self) -> Self {
        self.span = PanelSpan::Wide;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PanelSection {
    pub id: &'static str,
    pub title: &'static str,
    pub column: PanelColumn,
    pub span: PanelSpan,
    pub fields: Vec<PanelField>,
}

/// Atomically built model plus its one canonical display projection.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PanelDocument {
    pub schema_version: u32,
    #[serde(serialize_with = "serialize_decimal_u64")]
    pub revision: u64,
    pub model: InfoPanelModel,
    pub sections: Vec<PanelSection>,
}

/// Raw shared/platform inputs consumed by the single model builder.
#[derive(Debug)]
pub struct PanelBuildInput<'a> {
    pub tick: &'a TickOutput,
    pub world: &'a ExplorationWorld,
    pub hover: HoverInfo,
    pub performance: PerformanceInfo,
    pub streaming: StreamingSupplement,
    pub persistence: PersistenceInfo,
    pub renderer: RendererInfo,
    pub capture: CapturePreferences,
    pub warnings: &'a [ViewerWarning],
    /// Current controller-owned Map share in Split mode.
    pub split_ratio: f32,
    /// Semantic panel revision maintained by the adapter. It changes for
    /// actual state/hover/warning updates, not merely because an RAF ran.
    pub revision: u64,
}

/// Build the semantic model and its section strings once.
#[must_use]
pub fn build_panel_document(input: PanelBuildInput<'_>) -> PanelDocument {
    let tick = input.tick;
    let world = input.world;
    let map = world.map();
    let region = RegionCoord::from_world(tick.traveler.0, tick.traveler.1);
    let (current, target) = map.get(region).map_or((None, None), |state| {
        (Some(state.current.dims), Some(state.target.dims))
    });
    let regen_by_layer = world_core::layer::LAYERS
        .iter()
        .zip(LAYER_FIELD_IDS)
        .zip(input.streaming.regen_totals)
        .map(|((layer, id), total)| LayerRegenInfo {
            id,
            name: layer.name,
            total,
        })
        .collect();
    let anchors = world.anchors().iter().map(anchor_info).collect();
    let mut warnings = input.warnings.to_vec();
    append_renderer_warnings(&mut warnings, &input.renderer);
    warnings.sort_by_key(|warning| warning.id);

    let model = InfoPanelModel {
        frame: FrameInfo {
            sequence: tick.frame,
            update_serial: tick.update_serial,
            traveler: tick.traveler,
        },
        view: ViewInfo {
            mode: tick.mode,
            focused: tick.focused,
            map_channel: tick.map.channel,
            map_zoom: tick.map.zoom,
            map_backend: input.renderer.effective_map_backend,
            map_overlays: tick.map.overlays,
            map_refinement: tick.map.refinement,
            split_ratio: input.split_ratio,
            camera: tick.pov.into(),
            renderer: input.renderer,
        },
        performance: input.performance,
        streaming: StreamingInfo {
            tier: world.tier().name(),
            cache_ceiling_bytes: map.config().max_field_cache_bytes,
            stats: tick.stats,
            regen_by_layer,
            macro_tiles: input.streaming.macro_tiles,
            rosters: input.streaming.rosters,
            organisms: input.streaming.organisms,
            jobs_in_flight: input.streaming.jobs_in_flight,
            pinned_violations: input.streaming.pinned_violations,
            executor_backend: tick.platform.executor_backend,
            workers: tick.platform.workers,
        },
        steering: SteeringInfo {
            current,
            target,
            bias: *world.bias(),
            anchors,
            capture_category: input.capture.category.name(),
            capture_polarity: match input.capture.polarity {
                AnchorKind::Emphasize => "emphasize",
                AnchorKind::Suppress => "suppress",
            },
            transition_mode: world.transition_mode(),
        },
        persistence: PersistenceInfo {
            path_tracking: world.path_tracking(),
            route_recording: world.route_recording(),
            route_attraction: world.route_attraction(),
            ..input.persistence
        },
        hover: input.hover,
        warnings,
    };
    let sections = build_panel_sections(&model);
    PanelDocument {
        schema_version: 1,
        revision: input.revision,
        model,
        sections,
    }
}

fn append_renderer_warnings(warnings: &mut Vec<ViewerWarning>, renderer: &RendererInfo) {
    if renderer.device_losses > 0 {
        ensure_warning(
            warnings,
            ViewerWarning {
                id: "renderer-device-loss",
                message: format!(
                    "Renderer device lost {} time(s); fallback remains available",
                    renderer.device_losses
                ),
                severity: Severity::Warning,
            },
        );
    }
    if let Some(fallback) = renderer.map_fallback {
        let message = match fallback {
            MapBackendFallback::GpuUnavailable => {
                String::from("GPU map unavailable; CPU fallback active")
            }
            MapBackendFallback::UnsupportedChannel(channel) => {
                format!("{} is CPU-only; CPU fallback active", channel.name())
            }
        };
        ensure_warning(
            warnings,
            ViewerWarning {
                id: "renderer-map-fallback",
                message,
                severity: Severity::Warning,
            },
        );
    }
    if renderer.surface_losses > 0 {
        ensure_warning(
            warnings,
            ViewerWarning {
                id: "renderer-surface-loss",
                message: format!(
                    "Renderer surface lost {} time(s); recovery was attempted",
                    renderer.surface_losses
                ),
                severity: Severity::Warning,
            },
        );
    }
}

fn ensure_warning(warnings: &mut Vec<ViewerWarning>, warning: ViewerWarning) {
    if !warnings.iter().any(|existing| existing.id == warning.id) {
        warnings.push(warning);
    }
}

fn anchor_info(anchor: &Anchor) -> AnchorInfo {
    AnchorInfo {
        kind: match anchor.kind {
            AnchorKind::Emphasize => "emphasize",
            AnchorKind::Suppress => "suppress",
        },
        source: match anchor.source {
            AnchorSource::Organism { species } => format!("organism {species:016x}"),
            AnchorSource::Landform => String::from("landform"),
            AnchorSource::River => String::from("river"),
            AnchorSource::Atmosphere => String::from("atmosphere"),
            AnchorSource::Manual => String::from("manual"),
        },
        world: anchor.world_pos,
        strength: anchor.strength,
    }
}

const LAYER_FIELD_IDS: [&str; LAYER_COUNT as usize] = [
    "terrain",
    "geology",
    "drainage",
    "climate",
    "hydrology",
    "soils",
    "biome",
    "vegetation",
    "ecology",
];

const DOMAIN_FIELD_IDS: [&str; POSSIBILITY_DIMS] = [
    "planetary",
    "climate",
    "geology",
    "hydrology",
    "ecology",
    "morphology",
    "behavior",
    "aesthetics",
];

const DOMAIN_LABELS: [&str; POSSIBILITY_DIMS] = [
    "Planetary",
    "Climate",
    "Geology",
    "Hydrology",
    "Ecology",
    "Morphology",
    "Behavior",
    "Aesthetics",
];

/// Deterministic, fixed-superset panel projection. Hover variants toggle
/// visibility instead of changing the field schema, so DOM nodes stay mounted.
#[must_use]
pub fn build_panel_sections(model: &InfoPanelModel) -> Vec<PanelSection> {
    vec![
        summary_section(model),
        view_section(model),
        hover_section(model),
        streaming_section(model),
        ecology_section(model),
        steering_section(model),
        regen_section(model),
        performance_section(model),
        runtime_section(model),
        persistence_section(model),
        warnings_section(model),
    ]
}

fn section(
    id: &'static str,
    title: &'static str,
    column: PanelColumn,
    fields: Vec<PanelField>,
) -> PanelSection {
    PanelSection {
        id,
        title,
        column,
        span: PanelSpan::Single,
        fields,
    }
}

fn summary_section(model: &InfoPanelModel) -> PanelSection {
    section(
        "summary",
        "Explorer",
        PanelColumn::Explorer,
        vec![
            PanelField::info("frame.fps", "FPS", model.performance.fps.to_string()),
            PanelField::info("frame.sequence", "Frame", model.frame.sequence.to_string()),
            PanelField::info(
                "traveler.position",
                "Traveler",
                format!(
                    "{:.0}, {:.0}",
                    model.frame.traveler.0, model.frame.traveler.1
                ),
            ),
            PanelField::info(
                "traveler.x",
                "Traveler X",
                format!("{:.3}", model.frame.traveler.0),
            )
            .visible(false),
            PanelField::info(
                "traveler.y",
                "Traveler Y",
                format!("{:.3}", model.frame.traveler.1),
            )
            .visible(false),
        ],
    )
}

fn view_section(model: &InfoPanelModel) -> PanelSection {
    let view = &model.view;
    let camera = view.camera;
    section(
        "view",
        "View",
        PanelColumn::Explorer,
        vec![
            PanelField::info("view.mode", "Mode", mode_name(view.mode)),
            PanelField::info("view.focus", "Focus", view_name(view.focused)),
            PanelField::info("map.channel", "Map channel", view.map_channel.name()),
            PanelField::info("map.zoom", "Map zoom", format!("x{}", view.map_zoom)),
            PanelField::info("map.backend", "Map backend", backend_name(view.map_backend)),
            PanelField::info(
                "view.split-ratio",
                "Split",
                format!("{:.0}%", view.split_ratio * 100.0),
            ),
            PanelField::info(
                "pov.position",
                "Camera",
                format!(
                    "{:.1}, {:.1}, {:.1}",
                    camera.position[0], camera.position[1], camera.position[2]
                ),
            ),
            PanelField::info(
                "pov.orientation",
                "Yaw / pitch",
                format!(
                    "{:.1} deg / {:.1} deg",
                    camera.yaw.to_degrees(),
                    camera.pitch.to_degrees()
                ),
            ),
            PanelField::info(
                "pov.motion",
                "Motion",
                if camera.walk { "walk" } else { "fly" },
            ),
            PanelField::info(
                "pov.speed",
                "Speed",
                format!(
                    "{:.1} ({})",
                    if camera.walk {
                        camera.walk_speed
                    } else {
                        camera.fly_speed
                    },
                    if camera.walk { "walk" } else { "fly" }
                ),
            ),
            PanelField::info(
                "pov.features",
                "POV features",
                format!(
                    "shadow {} | detail {} | water {} | scale {:.2}",
                    on_off(camera.shadow_ao),
                    on_off(camera.detail_normals),
                    on_off(camera.water),
                    camera.render_scale
                ),
            )
            .wide(),
        ],
    )
}

fn hover_section(model: &InfoPanelModel) -> PanelSection {
    let terrain = match &model.hover {
        HoverInfo::Terrain(cell) => Some(cell),
        HoverInfo::None | HoverInfo::Organism(_) => None,
    };
    let organism = match &model.hover {
        HoverInfo::Organism(organism) => Some(organism),
        HoverInfo::None | HoverInfo::Terrain(_) => None,
    };
    let kind = match model.hover {
        HoverInfo::None => "none",
        HoverInfo::Terrain(_) => "terrain",
        HoverInfo::Organism(_) => "organism",
    };
    let terrain_visible = terrain.is_some();
    let organism_visible = organism.is_some();
    let mut fields = vec![PanelField::info("hover.kind", "Hover", kind)];
    fields.extend(terrain_fields(terrain, terrain_visible));
    fields.extend(organism_fields(organism, organism_visible));
    section("hover", "Inspection", PanelColumn::Explorer, fields)
}

fn terrain_fields(cell: Option<&CellInfo>, visible: bool) -> Vec<PanelField> {
    let world = cell.map_or((0.0, 0.0), |value| value.world);
    let region = cell.map_or(RegionCoord::new(0, 0), |value| value.region);
    let local = cell.map_or((0, 0), |value| (value.cell.cx, value.cell.cy));
    let status = cell.map_or(CellStatus::NotResident, |value| value.status);
    let stability = cell.map_or(0.0, |value| value.stability);
    let revision = cell.map_or(0, |value| value.revision);
    let value = |sample: Option<f32>, suffix: &str, digits: usize| {
        sample.map_or_else(
            || String::from("-"),
            |number| format_float(number, digits, suffix),
        )
    };
    vec![
        PanelField::info(
            "hover.terrain.world",
            "World",
            format!("{:.0}, {:.0}", world.0, world.1),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.region",
            "Region / cell",
            format!("{}, {} / {}, {}", region.x, region.y, local.0, local.1),
        )
        .visible(visible),
        PanelField::info("hover.terrain.status", "Status", cell_status_name(status))
            .visible(visible),
        PanelField::info(
            "hover.terrain.stability",
            "Stability / rev",
            format!("{stability:.2} / {revision}"),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.elevation",
            "Elevation",
            value(cell.and_then(|entry| entry.elevation), "", 0),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.temperature",
            "Temperature",
            value(cell.and_then(|entry| entry.temperature), " deg C", 1),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.moisture",
            "Moisture",
            value(cell.and_then(|entry| entry.moisture), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.hardness",
            "Rock hardness",
            value(cell.and_then(|entry| entry.hardness), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.river",
            "River",
            value(cell.and_then(|entry| entry.river), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.wetness",
            "Wetness",
            value(cell.and_then(|entry| entry.wetness), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.soil-depth",
            "Soil depth",
            value(cell.and_then(|entry| entry.soil_depth), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.fertility",
            "Fertility",
            value(cell.and_then(|entry| entry.fertility), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.vegetation",
            "Vegetation",
            value(cell.and_then(|entry| entry.vegetation), "", 2),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.canopy",
            "Canopy",
            value(cell.and_then(|entry| entry.canopy), "m", 1),
        )
        .visible(visible),
        PanelField::info(
            "hover.terrain.biome",
            "Biome",
            cell.and_then(|entry| entry.biome).unwrap_or("-"),
        )
        .visible(visible),
    ]
}

fn organism_fields(organism: Option<&OrganismInfo>, visible: bool) -> Vec<PanelField> {
    let id = organism.map_or(0, |value| value.id);
    let species = organism.map_or(0, |value| value.species);
    let slot = organism.map_or(0, |value| value.slot);
    let cell = organism.map_or((0, 0), |value| (value.cell.cx, value.cell.cy));
    let world = organism.map_or((0.0, 0.0), |value| value.world);
    vec![
        PanelField::info("hover.organism.id", "Organism id", format!("{id:016x}")).visible(visible),
        PanelField::info("hover.organism.slot", "Slot", slot.to_string()).visible(visible),
        PanelField::info(
            "hover.organism.species",
            "Species",
            format!("{species:016x}"),
        )
        .visible(visible),
        PanelField::info(
            "hover.organism.trophic",
            "Trophic role",
            organism.map_or("-", |value| value.trophic.name()),
        )
        .visible(visible),
        PanelField::info(
            "hover.organism.world",
            "World / cell",
            format!("{:.0}, {:.0} / {}, {}", world.0, world.1, cell.0, cell.1),
        )
        .visible(visible),
        PanelField::info(
            "hover.organism.form",
            "Form",
            organism.map_or_else(|| String::from("-"), |value| value.form.to_string()),
        )
        .visible(visible),
        PanelField::info(
            "hover.organism.expression",
            "Size / hue / luminance",
            organism.map_or_else(
                || String::from("-"),
                |value| {
                    format!(
                        "{:.2} / {:.2} / {:.2}",
                        value.size, value.hue, value.luminance
                    )
                },
            ),
        )
        .visible(visible),
        PanelField::info(
            "hover.organism.behavior",
            "Activity / aggression",
            organism.map_or_else(
                || String::from("-"),
                |value| format!("{:.2} / {:.2}", value.activity, value.aggression),
            ),
        )
        .visible(visible),
    ]
}

fn streaming_section(model: &InfoPanelModel) -> PanelSection {
    let streaming = &model.streaming;
    let stats = streaming.stats;
    section(
        "streaming",
        "Streaming",
        PanelColumn::World,
        vec![
            PanelField::info(
                "streaming.regions",
                "Regions",
                stats.active_regions.to_string(),
            ),
            PanelField::info(
                "streaming.cache",
                "Cache",
                format!(
                    "{} / {}",
                    mib(stats.cache_bytes.saturating_add(stats.macro_cache_bytes)),
                    mib(streaming.cache_ceiling_bytes)
                ),
            ),
            PanelField::info(
                "streaming.jobs",
                "Jobs",
                format!(
                    "{} running | {} cancelled",
                    streaming.jobs_in_flight, stats.jobs_cancelled
                ),
            ),
            PanelField::info(
                "streaming.macro-tiles",
                "Macro tiles",
                streaming.macro_tiles.to_string(),
            ),
            PanelField::info(
                "streaming.rosters",
                "Rosters",
                streaming.rosters.to_string(),
            ),
            PanelField::info(
                "streaming.organisms",
                "Organisms",
                streaming.organisms.to_string(),
            ),
            PanelField::info(
                "streaming.realized",
                "Realized auth / visual",
                format!(
                    "{} / {}",
                    stats.authoritative_organisms_realized, stats.organisms_realized
                ),
            ),
            PanelField::info(
                "streaming.resonance",
                "Resonance / nodes",
                format!(
                    "{:.2} / {}",
                    stats.resonance_strength, stats.resonance_nodes
                ),
            ),
            PanelField::info(
                "streaming.deferred",
                "Deferred",
                stats.deferred_regens.to_string(),
            ),
            PanelField::info(
                "streaming.converged",
                "Converged",
                stats.converged.to_string(),
            ),
            PanelField::info(
                "streaming.cost",
                "Regen cost",
                stats.regen_cost_spent.to_string(),
            ),
            PanelField::info(
                "streaming.pool",
                "Tile pool",
                format!(
                    "{}h / {}m | {}",
                    stats.pool_hits,
                    stats.pool_misses,
                    mib(stats.pool_bytes)
                ),
            ),
            PanelField::info(
                "streaming.failures",
                "Dropped / failed",
                format!("{} / {}", stats.results_dropped, stats.jobs_failed),
            )
            .severity(if stats.jobs_failed > 0 {
                Severity::Warning
            } else {
                Severity::Info
            }),
            PanelField::info(
                "streaming.pinned-violations",
                "Pinned violations",
                streaming.pinned_violations.to_string(),
            )
            .severity(if streaming.pinned_violations > 0 {
                Severity::Error
            } else {
                Severity::Info
            }),
        ],
    )
}

fn ecology_section(model: &InfoPanelModel) -> PanelSection {
    let ecology = match &model.hover {
        HoverInfo::Terrain(cell) => cell.ecology.as_ref(),
        HoverInfo::None | HoverInfo::Organism(_) => None,
    };
    let visible = ecology.is_some();
    let opt = |value: Option<f32>, digits: usize| {
        value.map_or_else(
            || String::from("-"),
            |number| format_float(number, digits, ""),
        )
    };
    section(
        "ecology",
        "Ecology",
        PanelColumn::World,
        vec![
            PanelField::info(
                "ecology.roster-size",
                "Roster species",
                ecology.map_or(0, |entry| entry.roster_size).to_string(),
            )
            .visible(visible),
            PanelField::info(
                "ecology.dominant-id",
                "Dominant species",
                format!("{:016x}", ecology.map_or(0, |entry| entry.dominant_id)),
            )
            .visible(visible),
            PanelField::info(
                "ecology.trophic-counts",
                "P / H / O / C / D",
                ecology.map_or_else(
                    || String::from("0 / 0 / 0 / 0 / 0"),
                    |entry| {
                        let c = entry.trophic_counts;
                        format!("{} / {} / {} / {} / {}", c[0], c[1], c[2], c[3], c[4])
                    },
                ),
            )
            .visible(visible),
            PanelField::info(
                "ecology.pressure",
                "Herbivore / predator",
                ecology.map_or_else(
                    || String::from("- / -"),
                    |entry| format!("{} / {}", opt(entry.herbivore, 3), opt(entry.predator, 3)),
                ),
            )
            .visible(visible),
            PanelField::info(
                "ecology.diversity",
                "Diversity",
                ecology.map_or_else(|| String::from("-"), |entry| opt(entry.diversity, 2)),
            )
            .visible(visible),
        ],
    )
}

fn steering_section(model: &InfoPanelModel) -> PanelSection {
    let steering = &model.steering;
    let mut fields = vec![
        PanelField::info(
            "steering.mode",
            "Transition",
            if steering.transition_mode {
                "active"
            } else {
                "free"
            },
        ),
        PanelField::info(
            "steering.capture",
            "Capture",
            format!(
                "{} {}",
                steering.capture_polarity, steering.capture_category
            ),
        ),
        PanelField::info(
            "steering.anchor-count",
            "Anchors",
            steering.anchors.len().to_string(),
        ),
    ];
    for index in 0..2 {
        let anchor = steering.anchors.get(index);
        fields.push(PanelField {
            id: PanelFieldId::owned(format!("steering.anchor.{index}")),
            label: "Anchor",
            value: anchor.map_or_else(
                || String::from("-"),
                |entry| {
                    format!(
                        "{} {} @ {:.0}, {:.0} | {:.2}",
                        entry.kind, entry.source, entry.world.0, entry.world.1, entry.strength
                    )
                },
            ),
            severity: Severity::Info,
            span: PanelSpan::Wide,
            visible: anchor.is_some(),
        });
    }
    for (index, value) in steering.bias.into_iter().enumerate() {
        fields.push(PanelField {
            id: PanelFieldId::owned(format!("steering.bias.{}", DOMAIN_FIELD_IDS[index])),
            label: DOMAIN_LABELS[index],
            value: format!("{value:+.2}"),
            severity: if value.abs() > f32::EPSILON {
                Severity::Warning
            } else {
                Severity::Info
            },
            span: PanelSpan::Single,
            visible: true,
        });
    }
    section("steering", "Steering", PanelColumn::World, fields)
}

fn regen_section(model: &InfoPanelModel) -> PanelSection {
    let fields = model
        .streaming
        .regen_by_layer
        .iter()
        .map(|layer| PanelField {
            id: PanelFieldId::owned(format!("regen.{}", layer.id)),
            label: layer.name,
            value: layer.total.to_string(),
            severity: Severity::Info,
            span: PanelSpan::Single,
            visible: true,
        })
        .collect();
    section("regen", "Regen by layer", PanelColumn::World, fields)
}

fn performance_section(model: &InfoPanelModel) -> PanelSection {
    let performance = model.performance;
    let passes = world_runtime::Pass::ALL
        .iter()
        .map(|pass| format!("{} {:.2}", pass.name(), performance.pass_ms[pass.index()]))
        .collect::<Vec<_>>()
        .join(" | ");
    section(
        "performance",
        "Performance",
        PanelColumn::System,
        vec![
            PanelField::info("performance.update", "Update", ms(performance.update_ms)),
            PanelField::info("performance.compose", "Compose", ms(performance.compose_ms)),
            PanelField::info("performance.present", "Present", ms(performance.present_ms)),
            PanelField::info(
                "performance.upload",
                "Upload",
                format!("{:.0} KiB/f", performance.upload_kib_per_frame),
            ),
            PanelField::info("performance.passes", "Pass timings", passes).wide(),
            PanelField::info(
                "performance.dom-updates",
                "DOM updates",
                performance.dom_updates.to_string(),
            ),
        ],
    )
}

fn runtime_section(model: &InfoPanelModel) -> PanelSection {
    let streaming = &model.streaming;
    let renderer = &model.view.renderer;
    let fallback = renderer.map_fallback.map_or_else(
        || String::from("none"),
        |reason| match reason {
            MapBackendFallback::GpuUnavailable => String::from("GPU unavailable"),
            MapBackendFallback::UnsupportedChannel(channel) => {
                format!("{} is CPU-only", channel.name())
            }
        },
    );
    section(
        "runtime",
        "Runtime",
        PanelColumn::System,
        vec![
            PanelField::info("runtime.tier", "Tier", streaming.tier),
            PanelField::info(
                "runtime.executor",
                "Executor",
                format!(
                    "{} / {}",
                    worker_name(streaming.executor_backend),
                    streaming.workers
                ),
            ),
            PanelField::info(
                "runtime.renderer",
                "Renderer",
                format!(
                    "{} requested | {} active",
                    backend_name(renderer.requested_map_backend),
                    backend_name(renderer.effective_map_backend)
                ),
            ),
            PanelField::info("runtime.fallback", "Fallback", fallback.clone()).severity(
                if renderer.map_fallback.is_some() {
                    Severity::Warning
                } else {
                    Severity::Info
                },
            ),
            PanelField::info(
                "runtime.surface-format",
                "Surface",
                renderer.surface_format.as_deref().unwrap_or("unreported"),
            ),
            PanelField::info(
                "runtime.device-losses",
                "Device losses",
                renderer.device_losses.to_string(),
            )
            .severity(if renderer.device_losses > 0 {
                Severity::Warning
            } else {
                Severity::Info
            }),
            PanelField::info(
                "runtime.surface-losses",
                "Surface losses",
                renderer.surface_losses.to_string(),
            )
            .severity(if renderer.surface_losses > 0 {
                Severity::Warning
            } else {
                Severity::Info
            }),
            PanelField::info(
                "runtime.pov-capability",
                "POV",
                if model.view.camera.supported {
                    "available"
                } else {
                    "unavailable"
                },
            )
            .severity(if model.view.camera.supported {
                Severity::Info
            } else {
                Severity::Warning
            }),
        ],
    )
}

fn persistence_section(model: &InfoPanelModel) -> PanelSection {
    let persistence = &model.persistence;
    let vault = persistence.vault.unwrap_or_default();
    let warning = persistence.failures > 0
        || vault.issues > 0
        || vault.suppressed_issues > 0
        || vault.persistence_retries > 0;
    section(
        "persistence",
        "Persistence",
        PanelColumn::System,
        vec![
            PanelField::info("persistence.mode", "Storage", persistence.mode.clone()).severity(
                if persistence.available {
                    Severity::Info
                } else {
                    Severity::Warning
                },
            ),
            PanelField::info("persistence.records", "Records", vault.records.to_string()),
            PanelField::info(
                "persistence.pending",
                "Pending writes",
                persistence
                    .pending_writes
                    .saturating_add(vault.dirty as u64)
                    .to_string(),
            ),
            PanelField::info("persistence.seen", "Seen regions", vault.seen.to_string()),
            PanelField::info(
                "persistence.issues",
                "Issues",
                format!(
                    "{} +{} suppressed | {} retries | {} failures",
                    vault.issues,
                    vault.suppressed_issues,
                    vault.persistence_retries,
                    persistence.failures
                ),
            )
            .severity(if warning {
                Severity::Warning
            } else {
                Severity::Info
            }),
            PanelField::info(
                "persistence.routes",
                "Routes",
                format!(
                    "tracking {} | recording {} | attraction {}",
                    on_off(persistence.path_tracking),
                    on_off(persistence.route_recording),
                    on_off(persistence.route_attraction)
                ),
            )
            .wide(),
        ],
    )
}

fn warnings_section(model: &InfoPanelModel) -> PanelSection {
    let mut fields = vec![PanelField::info(
        "warnings.status",
        "Status",
        if model.warnings.is_empty() {
            "No active warnings"
        } else {
            "Attention required"
        },
    )
    .severity(if model.warnings.is_empty() {
        Severity::Info
    } else {
        model
            .warnings
            .iter()
            .map(|warning| warning.severity)
            .max_by_key(|severity| severity_rank(*severity))
            .unwrap_or(Severity::Info)
    })];
    fields.extend(model.warnings.iter().map(|warning| PanelField {
        id: PanelFieldId::owned(format!("warnings.{}", warning.id)),
        label: "Warning",
        value: ascii_display(&warning.message),
        severity: warning.severity,
        span: PanelSpan::Wide,
        visible: true,
    }));
    section("warnings", "Warnings", PanelColumn::System, fields)
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn ascii_display(value: &str) -> String {
    if value.is_ascii() {
        return value.to_owned();
    }
    let mut display = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii() {
            display.push(character);
        } else {
            display.extend(character.escape_unicode());
        }
    }
    display
}

fn mode_name(mode: PresentationMode) -> &'static str {
    mode.as_str()
}

fn view_name(view: ViewKind) -> &'static str {
    match view {
        ViewKind::Map => "map",
        ViewKind::Pov => "pov",
    }
}

fn backend_name(backend: MapBackend) -> &'static str {
    match backend {
        MapBackend::Cpu => "cpu",
        MapBackend::GpuAtlas => "gpu-atlas",
    }
}

fn worker_name(backend: WorkerBackend) -> &'static str {
    match backend {
        WorkerBackend::Inline => "inline",
        WorkerBackend::Workers => "workers",
        WorkerBackend::SharedWorkers => "shared-workers",
    }
}

fn cell_status_name(status: CellStatus) -> &'static str {
    match status {
        CellStatus::NotResident => "not resident",
        CellStatus::Unloaded => "unloaded",
        CellStatus::Generating => "generating",
        CellStatus::Ready => "ready",
    }
}

fn on_off(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

fn ms(value: f64) -> String {
    format!("{value:.2} ms")
}

fn mib(bytes: usize) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn format_float(value: f32, digits: usize, suffix: &str) -> String {
    format!("{value:.digits$}{suffix}")
}

fn serialize_decimal_u64<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Cache key split by semantic sources. Adapters increment only the component
/// whose source changed; an unchanged RAF cannot rebuild the document.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelDocumentKey {
    pub state: u64,
    pub hover: u64,
    pub telemetry: u64,
    pub platform: u64,
}

/// One cached document shared by every presenter in a shell.
#[derive(Debug, Default)]
pub struct PanelDocumentCache {
    key: Option<PanelDocumentKey>,
    document: Option<PanelDocument>,
    builds: u64,
}

impl PanelDocumentCache {
    /// Return the existing document or invoke `build` exactly once for a new
    /// semantic key. The boolean reports whether a build occurred.
    pub fn get_or_build<F>(&mut self, key: PanelDocumentKey, build: F) -> (&PanelDocument, bool)
    where
        F: FnOnce() -> PanelDocument,
    {
        let changed = self.key != Some(key) || self.document.is_none();
        if changed {
            self.document = Some(build());
            self.key = Some(key);
            self.builds = self.builds.saturating_add(1);
        }
        (
            self.document.as_ref().expect("document is initialized"),
            changed,
        )
    }

    #[must_use]
    pub fn document(&self) -> Option<&PanelDocument> {
        self.document.as_ref()
    }

    #[must_use]
    pub const fn builds(&self) -> u64 {
        self.builds
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use world_core::{AnchorSource, LocalPos, PossibilityDomain, Trophic};

    use super::*;

    fn fixture_model(hover: HoverInfo) -> InfoPanelModel {
        InfoPanelModel {
            frame: FrameInfo {
                sequence: 7,
                update_serial: 6,
                traveler: (12.5, -8.25),
            },
            view: ViewInfo {
                mode: PresentationMode::Split,
                focused: ViewKind::Pov,
                map_channel: Channel::Composite,
                map_zoom: 4,
                map_backend: MapBackend::Cpu,
                map_overlays: Overlays::default(),
                map_refinement: false,
                split_ratio: 0.5,
                camera: CameraInfo {
                    position: [12.5, -8.25, 30.0],
                    yaw: 0.5,
                    pitch: -0.25,
                    fly_speed: 64.0,
                    walk: true,
                    walk_speed: 12.0,
                    shadow_ao: true,
                    detail_normals: true,
                    water: true,
                    render_scale: 0.5,
                    initialized: true,
                    supported: true,
                },
                renderer: RendererInfo::default(),
            },
            performance: PerformanceInfo {
                fps: 60,
                ..PerformanceInfo::default()
            },
            streaming: StreamingInfo {
                tier: "low",
                cache_ceiling_bytes: 48 * 1024 * 1024,
                stats: FrameStats::default(),
                regen_by_layer: world_core::layer::LAYERS
                    .iter()
                    .zip(LAYER_FIELD_IDS)
                    .map(|(layer, id)| LayerRegenInfo {
                        id,
                        name: layer.name,
                        total: 0,
                    })
                    .collect(),
                macro_tiles: 0,
                rosters: 0,
                organisms: 0,
                jobs_in_flight: 0,
                pinned_violations: 0,
                executor_backend: WorkerBackend::Inline,
                workers: 1,
            },
            steering: SteeringInfo {
                current: None,
                target: None,
                bias: [0.0; POSSIBILITY_DIMS],
                anchors: vec![anchor_info(&Anchor {
                    world_pos: (1.0, 2.0),
                    target: world_core::PossibilityVector::neutral(),
                    mask: 1,
                    kind: AnchorKind::Emphasize,
                    strength: 1.0,
                    falloff_radius: 10.0,
                    source: AnchorSource::Manual,
                })],
                capture_category: "Morphology",
                capture_polarity: "emphasize",
                transition_mode: false,
            },
            persistence: PersistenceInfo::default(),
            hover,
            warnings: Vec::new(),
        }
    }

    fn terrain(status: CellStatus) -> CellInfo {
        CellInfo {
            world: (10.0, -1.0),
            region: RegionCoord::new(0, -1),
            cell: LocalPos::new(2, 3),
            status,
            stability: 0.75,
            revision: 4,
            elevation: Some(123.25),
            temperature: Some(18.5),
            moisture: Some(0.4),
            hardness: Some(0.5),
            river: Some(0.0),
            wetness: Some(0.2),
            soil_depth: Some(0.8),
            fertility: Some(0.6),
            vegetation: Some(0.7),
            canopy: Some(9.5),
            biome: Some("taiga"),
            ecology: Some(crate::inspect::EcologyInfo {
                signature: world_core::HabitatSignature {
                    biome: 1,
                    temperature_band: 2,
                    moisture_band: 3,
                    fertility_band: 4,
                },
                roster_size: 9,
                dominant_index: 2,
                dominant_id: 0xfedc_ba98_7654_3210,
                trophic_counts: [3, 2, 1, 2, 1],
                herbivore: Some(0.25),
                predator: Some(0.125),
                diversity: Some(0.75),
            }),
        }
    }

    #[test]
    fn panel_ids_are_stable_hierarchical_and_unique() {
        assert_eq!(field_ids::FRAME_FPS.as_str(), "frame.fps");
        assert_eq!(field_ids::VIEW_MODE.as_str(), "view.mode");
        let sections = build_panel_sections(&fixture_model(HoverInfo::Terrain(terrain(
            CellStatus::Ready,
        ))));
        let ids: Vec<_> = sections
            .iter()
            .flat_map(|section| section.fields.iter().map(|field| field.id.as_str()))
            .collect();
        assert_eq!(
            ids.len(),
            ids.iter().copied().collect::<BTreeSet<_>>().len()
        );
        assert_eq!(
            sections
                .iter()
                .map(|section| section.column)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        assert!(ids.iter().all(|id| id.contains('.')));
    }

    #[test]
    fn hover_schema_is_fixed_across_none_terrain_and_organism() {
        let organism = OrganismInfo {
            id: 0xfedc_ba98_7654_3210,
            slot: 3,
            species: 0xeffe_dcab_8967_4523,
            trophic: Trophic::Herbivore,
            cell: LocalPos::new(2, 3),
            world: (10.0, -1.0),
            form: 12,
            hue: 0.2,
            luminance: 0.3,
            size: 1.4,
            activity: 0.5,
            aggression: 0.6,
        };
        let documents = [
            fixture_model(HoverInfo::None),
            fixture_model(HoverInfo::Terrain(terrain(CellStatus::Ready))),
            fixture_model(HoverInfo::Organism(organism)),
        ];
        let schemas: Vec<Vec<String>> = documents
            .iter()
            .map(|model| {
                build_panel_sections(model)
                    .into_iter()
                    .flat_map(|section| {
                        section
                            .fields
                            .into_iter()
                            .map(|field| field.id.0.into_owned())
                    })
                    .collect()
            })
            .collect();
        assert_eq!(schemas[0], schemas[1]);
        assert_eq!(schemas[1], schemas[2]);

        let fields = build_panel_sections(&documents[2]);
        let id = fields
            .iter()
            .flat_map(|section| &section.fields)
            .find(|field| field.id.as_str() == "hover.organism.id")
            .expect("organism id field");
        assert_eq!(id.value, "fedcba9876543210");
    }

    #[test]
    fn serialized_document_never_rounds_identity_or_revision_u64s() {
        let organism = OrganismInfo {
            id: 0xfedc_ba98_7654_3210,
            slot: 3,
            species: 0xeffe_dcab_8967_4523,
            trophic: Trophic::Herbivore,
            cell: LocalPos::new(2, 3),
            world: (10.0, -1.0),
            form: 12,
            hue: 0.2,
            luminance: 0.3,
            size: 1.4,
            activity: 0.5,
            aggression: 0.6,
        };
        let model = fixture_model(HoverInfo::Organism(organism));
        let document = PanelDocument {
            schema_version: 1,
            revision: u64::MAX,
            sections: build_panel_sections(&model),
            model,
        };
        let json = serde_json::to_value(document).expect("serialize panel document");
        assert_eq!(json["revision"], u64::MAX.to_string());
        assert_eq!(json["model"]["hover"]["value"]["id"], "fedcba9876543210");
        assert_eq!(
            json["model"]["hover"]["value"]["species"],
            "effedcab89674523"
        );
    }

    #[test]
    fn terrain_states_and_full_ecology_are_formatted_once() {
        for (status, expected) in [
            (CellStatus::NotResident, "not resident"),
            (CellStatus::Unloaded, "unloaded"),
            (CellStatus::Generating, "generating"),
            (CellStatus::Ready, "ready"),
        ] {
            let sections =
                build_panel_sections(&fixture_model(HoverInfo::Terrain(terrain(status))));
            let field = |id: &str| {
                sections
                    .iter()
                    .flat_map(|section| &section.fields)
                    .find(|field| field.id.as_str() == id)
                    .expect("stable field")
            };
            assert_eq!(field("hover.terrain.status").value, expected);
            assert_eq!(field("ecology.trophic-counts").value, "3 / 2 / 1 / 2 / 1");
            assert_eq!(field("ecology.dominant-id").value, "fedcba9876543210");
        }
    }

    #[test]
    fn legacy_streaming_diagnostics_reach_both_renderers() {
        let mut model = fixture_model(HoverInfo::None);
        model.streaming.macro_tiles = 11;
        model.streaming.rosters = 12;
        model.streaming.organisms = 13;
        model.streaming.pinned_violations = 2;
        model.streaming.stats.authoritative_organisms_realized = 14;
        model.streaming.stats.organisms_realized = 15;
        model.streaming.stats.resonance_strength = 0.625;
        model.streaming.stats.resonance_nodes = 16;
        let sections = build_panel_sections(&model);
        let field = |id: &str| {
            sections
                .iter()
                .flat_map(|section| &section.fields)
                .find(|field| field.id.as_str() == id)
                .expect("legacy diagnostic field")
        };
        assert_eq!(field("streaming.macro-tiles").value, "11");
        assert_eq!(field("streaming.rosters").value, "12");
        assert_eq!(field("streaming.organisms").value, "13");
        assert_eq!(field("streaming.realized").value, "14 / 15");
        assert_eq!(field("streaming.resonance").value, "0.62 / 16");
        assert_eq!(field("streaming.pinned-violations").value, "2");
        assert_eq!(
            field("streaming.pinned-violations").severity,
            Severity::Error
        );
    }

    #[test]
    fn display_projection_is_ascii_lossless_for_the_bitmap_font() {
        let mut model = fixture_model(HoverInfo::None);
        model.warnings.push(ViewerWarning {
            id: "unicode-fixture",
            message: String::from("surface café — retry"),
            severity: Severity::Warning,
        });
        let sections = build_panel_sections(&model);
        assert!(sections.iter().all(|section| {
            section.title.is_ascii()
                && section.fields.iter().all(|field| {
                    field.label.is_ascii() && field.id.as_str().is_ascii() && field.value.is_ascii()
                })
        }));
        let warning = sections
            .iter()
            .flat_map(|section| &section.fields)
            .find(|field| field.id.as_str() == "warnings.unicode-fixture")
            .expect("escaped warning field");
        assert_eq!(warning.value, "surface caf\\u{e9} \\u{2014} retry");
    }

    #[test]
    fn renderer_and_persistence_failures_share_warning_severity() {
        let mut model = fixture_model(HoverInfo::None);
        model.view.renderer.map_fallback = Some(MapBackendFallback::GpuUnavailable);
        model.persistence = PersistenceInfo {
            mode: String::from("indexeddb-failed"),
            failures: 2,
            ..PersistenceInfo::default()
        };
        model.warnings.push(ViewerWarning {
            id: "webgpu-device-lost",
            message: String::from("WebGPU device lost; CPU map fallback active"),
            severity: Severity::Warning,
        });
        let sections = build_panel_sections(&model);
        for id in [
            "runtime.fallback",
            "persistence.issues",
            "warnings.webgpu-device-lost",
        ] {
            let field = sections
                .iter()
                .flat_map(|section| &section.fields)
                .find(|field| field.id.as_str() == id)
                .expect("warning field");
            assert_eq!(field.severity, Severity::Warning);
        }
    }

    #[test]
    fn surface_loss_warning_does_not_claim_recovery_succeeded() {
        let mut warnings = Vec::new();
        append_renderer_warnings(
            &mut warnings,
            &RendererInfo {
                surface_losses: 2,
                ..RendererInfo::default()
            },
        );

        assert_eq!(
            warnings,
            vec![ViewerWarning {
                id: "renderer-surface-loss",
                message: String::from("Renderer surface lost 2 time(s); recovery was attempted"),
                severity: Severity::Warning,
            }]
        );
    }

    #[test]
    fn device_loss_does_not_hide_the_active_map_fallback_warning() {
        let mut warnings = Vec::new();
        append_renderer_warnings(
            &mut warnings,
            &RendererInfo {
                map_fallback: Some(MapBackendFallback::GpuUnavailable),
                device_losses: 1,
                ..RendererInfo::default()
            },
        );

        assert_eq!(warnings.len(), 2);
        assert!(warnings
            .iter()
            .any(|warning| warning.id == "renderer-device-loss"));
        assert!(warnings
            .iter()
            .any(|warning| warning.id == "renderer-map-fallback"));
    }

    #[test]
    fn cache_rebuilds_only_for_a_changed_semantic_key() {
        let mut cache = PanelDocumentCache::default();
        let model = fixture_model(HoverInfo::None);
        let build = || PanelDocument {
            schema_version: 1,
            revision: 1,
            sections: build_panel_sections(&model),
            model: model.clone(),
        };
        let key = PanelDocumentKey {
            state: 1,
            hover: 2,
            telemetry: 3,
            platform: 4,
        };
        assert!(cache.get_or_build(key, build).1);
        assert!(
            !cache
                .get_or_build(key, || panic!("unchanged key rebuilt"))
                .1
        );
        assert_eq!(cache.builds(), 1);
        assert!(
            cache
                .get_or_build(
                    PanelDocumentKey {
                        telemetry: 4,
                        ..key
                    },
                    build
                )
                .1
        );
        assert_eq!(cache.builds(), 2);
    }

    #[test]
    fn warning_registry_is_idempotent_and_revisioned() {
        let mut registry = WarningRegistry::default();
        let warning = ViewerWarning {
            id: "storage",
            message: String::from("storage unavailable"),
            severity: Severity::Warning,
        };
        assert!(registry.upsert(warning.clone()));
        assert!(!registry.upsert(warning));
        assert_eq!(registry.revision(), 1);
        assert!(registry.remove("storage"));
        assert_eq!(registry.revision(), 2);
    }

    #[test]
    fn section_output_contains_no_stale_phase_or_hardcoded_help() {
        let model = fixture_model(HoverInfo::None);
        let text = format!("{:?}", build_panel_sections(&model));
        assert!(!text.contains("PHASE 4"));
        assert!(!text.contains("KEYS"));
    }

    #[test]
    fn possibility_domains_remain_in_declared_order() {
        for (index, domain) in PossibilityDomain::ALL.into_iter().enumerate() {
            assert_eq!(domain.index(), index);
            assert!(!DOMAIN_FIELD_IDS[index].is_empty());
        }
    }
}
