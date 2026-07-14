//! Canonical CPU composition of the top-down false-color map
//! (`native-web-alignment.md` Milestone 4; phase-2-plan.md §11).
//!
//! Composing on the CPU keeps the GPU surface area minimal (the renderer just
//! presents one texture) and makes every overlay trivial to draw. A continuous
//! field renders as smooth gradients; a chunk-replacement bug renders as a
//! visible seam or a flickering tile. Rivers are the Phase 2
//! popping-detector-in-chief: a drainage discontinuity is instantly visible as
//! a broken river line across a macro boundary.

use std::collections::{BTreeMap, BTreeSet};

use renderer::{GpuMapParams, MapTileUpload};
use world_core::{mix, Anchor, Biome, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{
    RegionMap, CHANNEL_DIVERSITY, CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS,
    CHANNEL_HERBIVORE, CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER, CHANNEL_SOIL_DEPTH,
    CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

use crate::atlas::{gpu_channel, refinement_octaves, AtlasManager, RefinementRequest};

// The per-cell color ramps live in `world_runtime::mapcolor`; the canonical
// composer layers shared overlays, zoom, inverse picking, and pinned-revision
// detection on top so native and browser fallbacks consume identical bytes.
use world_runtime::mapcolor::{
    biome_color, diversity_color, elevation_color, geology_color, herbivore_color, lerp_rgb,
    missing_color, moisture_color, predator_color, river_color, soil_color, species_color,
    temperature_color, vegetation_color, wetness_color,
};
pub use world_runtime::mapcolor::{composite_cell_color, expressed_color};

/// Which scalar or categorical field the map paints.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Composite terrain, biome, and water color.
    Composite,
    /// Terrain elevation.
    Elevation,
    /// Lithology and hardness.
    Geology,
    /// Air temperature.
    Temperature,
    /// Surface moisture.
    Moisture,
    /// River expression.
    River,
    /// Surface wetness.
    Wetness,
    /// Soil depth and fertility.
    Soil,
    /// Biome classification.
    Biome,
    /// Vegetation density.
    Vegetation,
    /// Herbivore pressure.
    Herbivore,
    /// Predator pressure.
    Predator,
    /// Species diversity.
    Diversity,
    /// Dominant species.
    DominantSpecies,
    /// Anchor influence.
    Influence,
    /// Streaming stability.
    Stability,
    /// Realized-state revision.
    Revision,
    /// Realized-to-target residual.
    Residual,
}

/// Stable grouping for map controls and presentation layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapDescriptorGroup {
    /// Terrain, geology, soil, and biome surfaces.
    Surface,
    /// Climate and water fields.
    ClimateWater,
    /// Vegetation, trophic pressure, and species fields.
    Ecology,
    /// Streaming and steering diagnostics.
    Diagnostics,
    /// View aids such as the grid, rings, and player marker.
    Presentation,
    /// Durable exploration records and discovery state.
    Exploration,
}

impl MapDescriptorGroup {
    /// Stable machine id used by platform control grouping.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::ClimateWater => "climate-water",
            Self::Ecology => "ecology",
            Self::Diagnostics => "diagnostics",
            Self::Presentation => "presentation",
            Self::Exploration => "exploration",
        }
    }

    /// User-facing group label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Surface => "Surface",
            Self::ClimateWater => "Climate & Water",
            Self::Ecology => "Ecology",
            Self::Diagnostics => "Diagnostics",
            Self::Presentation => "Presentation",
            Self::Exploration => "Exploration",
        }
    }

    /// Stable group order for platform renderers.
    #[must_use]
    pub const fn order(self) -> u8 {
        match self {
            Self::Surface => 0,
            Self::ClimateWater => 1,
            Self::Ecology => 2,
            Self::Diagnostics => 3,
            Self::Presentation => 4,
            Self::Exploration => 5,
        }
    }
}

/// One shared map-channel control descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelDescriptor {
    /// Typed channel selected by the control.
    pub channel: Channel,
    /// Stable machine id.
    pub id: &'static str,
    /// User-facing label.
    pub label: &'static str,
    /// Toolbar/control group.
    pub group: MapDescriptorGroup,
    /// Stable channel cycle and display order.
    pub order: u8,
}

/// Complete channel registry. Shells render controls from this table instead
/// of maintaining native and browser option lists independently.
pub const CHANNEL_DESCRIPTORS: [ChannelDescriptor; 18] = [
    ChannelDescriptor {
        channel: Channel::Composite,
        id: "composite",
        label: "Composite",
        group: MapDescriptorGroup::Surface,
        order: 0,
    },
    ChannelDescriptor {
        channel: Channel::Elevation,
        id: "elevation",
        label: "Elevation",
        group: MapDescriptorGroup::Surface,
        order: 1,
    },
    ChannelDescriptor {
        channel: Channel::Geology,
        id: "geology",
        label: "Geology",
        group: MapDescriptorGroup::Surface,
        order: 2,
    },
    ChannelDescriptor {
        channel: Channel::Temperature,
        id: "temperature",
        label: "Temperature",
        group: MapDescriptorGroup::ClimateWater,
        order: 3,
    },
    ChannelDescriptor {
        channel: Channel::Moisture,
        id: "moisture",
        label: "Moisture",
        group: MapDescriptorGroup::ClimateWater,
        order: 4,
    },
    ChannelDescriptor {
        channel: Channel::River,
        id: "river",
        label: "River",
        group: MapDescriptorGroup::ClimateWater,
        order: 5,
    },
    ChannelDescriptor {
        channel: Channel::Wetness,
        id: "wetness",
        label: "Wetness",
        group: MapDescriptorGroup::ClimateWater,
        order: 6,
    },
    ChannelDescriptor {
        channel: Channel::Soil,
        id: "soil",
        label: "Soil",
        group: MapDescriptorGroup::Surface,
        order: 7,
    },
    ChannelDescriptor {
        channel: Channel::Biome,
        id: "biome",
        label: "Biome",
        group: MapDescriptorGroup::Surface,
        order: 8,
    },
    ChannelDescriptor {
        channel: Channel::Vegetation,
        id: "vegetation",
        label: "Vegetation",
        group: MapDescriptorGroup::Ecology,
        order: 9,
    },
    ChannelDescriptor {
        channel: Channel::Herbivore,
        id: "herbivore",
        label: "Herbivore",
        group: MapDescriptorGroup::Ecology,
        order: 10,
    },
    ChannelDescriptor {
        channel: Channel::Predator,
        id: "predator",
        label: "Predator",
        group: MapDescriptorGroup::Ecology,
        order: 11,
    },
    ChannelDescriptor {
        channel: Channel::Diversity,
        id: "diversity",
        label: "Diversity",
        group: MapDescriptorGroup::Ecology,
        order: 12,
    },
    ChannelDescriptor {
        channel: Channel::DominantSpecies,
        id: "dominant",
        label: "Dominant Species",
        group: MapDescriptorGroup::Ecology,
        order: 13,
    },
    ChannelDescriptor {
        channel: Channel::Influence,
        id: "influence",
        label: "Anchor Influence",
        group: MapDescriptorGroup::Diagnostics,
        order: 14,
    },
    ChannelDescriptor {
        channel: Channel::Stability,
        id: "stability",
        label: "Stability",
        group: MapDescriptorGroup::Diagnostics,
        order: 15,
    },
    ChannelDescriptor {
        channel: Channel::Revision,
        id: "revision",
        label: "Revision",
        group: MapDescriptorGroup::Diagnostics,
        order: 16,
    },
    ChannelDescriptor {
        channel: Channel::Residual,
        id: "residual",
        label: "Residual",
        group: MapDescriptorGroup::Diagnostics,
        order: 17,
    },
];

impl Channel {
    /// Stable cycle order shared by controls and help.
    pub const ALL: [Self; 18] = [
        Self::Composite,
        Self::Elevation,
        Self::Geology,
        Self::Temperature,
        Self::Moisture,
        Self::River,
        Self::Wetness,
        Self::Soil,
        Self::Biome,
        Self::Vegetation,
        Self::Herbivore,
        Self::Predator,
        Self::Diversity,
        Self::DominantSpecies,
        Self::Influence,
        Self::Stability,
        Self::Revision,
        Self::Residual,
    ];

    /// Stable id used at platform boundaries.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.descriptor().id
    }

    /// Parse an exact stable channel id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        CHANNEL_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.id == id)
            .map(|descriptor| descriptor.channel)
    }

    /// Shared descriptor for this channel.
    #[must_use]
    pub const fn descriptor(self) -> &'static ChannelDescriptor {
        &CHANNEL_DESCRIPTORS[self as usize]
    }

    /// Compatibility name for the native presenter during extraction.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.id()
    }

    /// Compatibility parser for the native presenter during extraction.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::from_id(name)
    }

    /// Next channel in the stable cycle.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Composite => Self::Elevation,
            Self::Elevation => Self::Geology,
            Self::Geology => Self::Temperature,
            Self::Temperature => Self::Moisture,
            Self::Moisture => Self::River,
            Self::River => Self::Wetness,
            Self::Wetness => Self::Soil,
            Self::Soil => Self::Biome,
            Self::Biome => Self::Vegetation,
            Self::Vegetation => Self::Herbivore,
            Self::Herbivore => Self::Predator,
            Self::Predator => Self::Diversity,
            Self::Diversity => Self::DominantSpecies,
            Self::DominantSpecies => Self::Influence,
            Self::Influence => Self::Stability,
            Self::Stability => Self::Revision,
            Self::Revision => Self::Residual,
            Self::Residual => Self::Composite,
        }
    }
}

/// An independently switchable map overlay.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapOverlay {
    /// Region grid.
    Grid,
    /// Near/far stability rings.
    Rings,
    /// Changed-while-pinned flash.
    PinnedFlash,
    /// Realized organisms.
    Organisms,
    /// Undiscovered-region dimming.
    Discovered,
}

/// One shared toggleable-overlay control descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapOverlayDescriptor {
    /// Typed overlay toggled by the control.
    pub overlay: MapOverlay,
    /// Stable machine id.
    pub id: &'static str,
    /// User-facing label.
    pub label: &'static str,
    /// Toolbar/control group.
    pub group: MapDescriptorGroup,
    /// Stable control order.
    pub order: u8,
}

/// Complete toggleable-overlay registry.
pub const MAP_OVERLAY_DESCRIPTORS: [MapOverlayDescriptor; 5] = [
    MapOverlayDescriptor {
        overlay: MapOverlay::Grid,
        id: "grid",
        label: "Region Grid",
        group: MapDescriptorGroup::Presentation,
        order: 0,
    },
    MapOverlayDescriptor {
        overlay: MapOverlay::Rings,
        id: "rings",
        label: "Streaming Rings",
        group: MapDescriptorGroup::Presentation,
        order: 1,
    },
    MapOverlayDescriptor {
        overlay: MapOverlay::PinnedFlash,
        id: "pinned-flash",
        label: "Pinned-Change Flash",
        group: MapDescriptorGroup::Diagnostics,
        order: 2,
    },
    MapOverlayDescriptor {
        overlay: MapOverlay::Organisms,
        id: "organisms",
        label: "Organisms",
        group: MapDescriptorGroup::Ecology,
        order: 3,
    },
    MapOverlayDescriptor {
        overlay: MapOverlay::Discovered,
        id: "discovered",
        label: "Discovered Regions",
        group: MapDescriptorGroup::Exploration,
        order: 4,
    },
];

impl MapOverlay {
    /// Stable overlay order.
    pub const ALL: [Self; 5] = [
        Self::Grid,
        Self::Rings,
        Self::PinnedFlash,
        Self::Organisms,
        Self::Discovered,
    ];

    /// Stable machine id.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.descriptor().id
    }

    /// Shared control descriptor for this overlay.
    #[must_use]
    pub const fn descriptor(self) -> &'static MapOverlayDescriptor {
        &MAP_OVERLAY_DESCRIPTORS[self as usize]
    }

    /// Parse an exact stable overlay id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        MAP_OVERLAY_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.id == id)
            .map(|descriptor| descriptor.overlay)
    }
}

/// A stage in the canonical map layer stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapLayer {
    /// Selected channel's base field.
    Base,
    /// Changed-while-pinned diagnostic tint.
    PinnedFlash,
    /// Region boundary grid.
    Grid,
    /// Undiscovered-region dimming.
    Discovered,
    /// Durable expedition routes.
    Routes,
    /// Durable preserve outlines.
    Preserves,
    /// Realized organism markers.
    Organisms,
    /// Near/far streaming rings.
    Rings,
    /// Traveler marker, always topmost.
    Player,
}

/// One visual-layer descriptor. `overlay` is `None` for always-present layers
/// whose empty data source simply draws nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapLayerDescriptor {
    /// Typed visual layer.
    pub layer: MapLayer,
    /// Toggle controlling this layer, if any.
    pub overlay: Option<MapOverlay>,
    /// Stable machine id.
    pub id: &'static str,
    /// User-facing label.
    pub label: &'static str,
    /// Semantic group.
    pub group: MapDescriptorGroup,
    /// Bottom-to-top compositing order.
    pub order: u8,
}

/// Bottom-to-top map layer stack. Both the canonical CPU composer and sparse
/// GPU overlay preparation exhaustively consume this constant.
pub const MAP_LAYER_SEQUENCE: [MapLayer; 9] = [
    MapLayer::Base,
    MapLayer::PinnedFlash,
    MapLayer::Grid,
    MapLayer::Discovered,
    MapLayer::Routes,
    MapLayer::Preserves,
    MapLayer::Organisms,
    MapLayer::Rings,
    MapLayer::Player,
];

/// Descriptors in the same bottom-to-top order as [`MAP_LAYER_SEQUENCE`].
pub const MAP_LAYER_DESCRIPTORS: [MapLayerDescriptor; 9] = [
    MapLayerDescriptor {
        layer: MapLayer::Base,
        overlay: None,
        id: "base",
        label: "Selected Channel",
        group: MapDescriptorGroup::Presentation,
        order: 0,
    },
    MapLayerDescriptor {
        layer: MapLayer::PinnedFlash,
        overlay: Some(MapOverlay::PinnedFlash),
        id: "pinned-flash",
        label: "Pinned-Change Flash",
        group: MapDescriptorGroup::Diagnostics,
        order: 1,
    },
    MapLayerDescriptor {
        layer: MapLayer::Grid,
        overlay: Some(MapOverlay::Grid),
        id: "grid",
        label: "Region Grid",
        group: MapDescriptorGroup::Presentation,
        order: 2,
    },
    MapLayerDescriptor {
        layer: MapLayer::Discovered,
        overlay: Some(MapOverlay::Discovered),
        id: "discovered",
        label: "Discovered Regions",
        group: MapDescriptorGroup::Exploration,
        order: 3,
    },
    MapLayerDescriptor {
        layer: MapLayer::Routes,
        overlay: None,
        id: "routes",
        label: "Routes",
        group: MapDescriptorGroup::Exploration,
        order: 4,
    },
    MapLayerDescriptor {
        layer: MapLayer::Preserves,
        overlay: None,
        id: "preserves",
        label: "Preserves",
        group: MapDescriptorGroup::Exploration,
        order: 5,
    },
    MapLayerDescriptor {
        layer: MapLayer::Organisms,
        overlay: Some(MapOverlay::Organisms),
        id: "organisms",
        label: "Organisms",
        group: MapDescriptorGroup::Ecology,
        order: 6,
    },
    MapLayerDescriptor {
        layer: MapLayer::Rings,
        overlay: Some(MapOverlay::Rings),
        id: "rings",
        label: "Streaming Rings",
        group: MapDescriptorGroup::Presentation,
        order: 7,
    },
    MapLayerDescriptor {
        layer: MapLayer::Player,
        overlay: None,
        id: "player",
        label: "Traveler",
        group: MapDescriptorGroup::Presentation,
        order: 8,
    },
];

impl MapLayer {
    /// Descriptor for this visual layer.
    #[must_use]
    pub const fn descriptor(self) -> &'static MapLayerDescriptor {
        match self {
            Self::Base => &MAP_LAYER_DESCRIPTORS[0],
            Self::PinnedFlash => &MAP_LAYER_DESCRIPTORS[1],
            Self::Grid => &MAP_LAYER_DESCRIPTORS[2],
            Self::Discovered => &MAP_LAYER_DESCRIPTORS[3],
            Self::Routes => &MAP_LAYER_DESCRIPTORS[4],
            Self::Preserves => &MAP_LAYER_DESCRIPTORS[5],
            Self::Organisms => &MAP_LAYER_DESCRIPTORS[6],
            Self::Rings => &MAP_LAYER_DESCRIPTORS[7],
            Self::Player => &MAP_LAYER_DESCRIPTORS[8],
        }
    }

    /// Whether the layer is enabled for these preferences. Data-backed layers
    /// remain enabled even when their current data source is empty.
    #[must_use]
    pub const fn enabled(self, overlays: Overlays) -> bool {
        match self.descriptor().overlay {
            Some(overlay) => overlays.enabled(overlay),
            None => true,
        }
    }
}

/// Map overlay toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlays {
    /// Region grid lines.
    pub grid: bool,
    /// Stability rings.
    pub rings: bool,
    /// Changed-while-pinned flashes.
    pub pinned_flash: bool,
    /// Realized organism markers.
    pub organisms: bool,
    /// Discovery dimming.
    pub discovered: bool,
}

impl Overlays {
    /// Read a toggle through its typed id.
    #[must_use]
    pub const fn enabled(self, overlay: MapOverlay) -> bool {
        match overlay {
            MapOverlay::Grid => self.grid,
            MapOverlay::Rings => self.rings,
            MapOverlay::PinnedFlash => self.pinned_flash,
            MapOverlay::Organisms => self.organisms,
            MapOverlay::Discovered => self.discovered,
        }
    }

    /// Set a toggle through its typed id.
    pub fn set(&mut self, overlay: MapOverlay, enabled: bool) {
        match overlay {
            MapOverlay::Grid => self.grid = enabled,
            MapOverlay::Rings => self.rings = enabled,
            MapOverlay::PinnedFlash => self.pinned_flash = enabled,
            MapOverlay::Organisms => self.organisms = enabled,
            MapOverlay::Discovered => self.discovered = enabled,
        }
    }
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            grid: true,
            rings: true,
            pinned_flash: true,
            organisms: true,
            discovered: true,
        }
    }
}

/// Vault-derived map decorations: discovered regions, preserve outlines, and
/// route polylines. The shared composer consumes these values in Milestone 4;
/// shells may build them from their platform-owned storage services.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MapDecor {
    /// Discovered regions within the view. `None` means discovery dimming is
    /// unavailable because no vault is open.
    pub seen: Option<BTreeSet<RegionCoord>>,
    /// Preserved regions.
    pub preserves: BTreeSet<RegionCoord>,
    /// Route node positions in travel order and the route usage count.
    pub routes: Vec<(Vec<(f64, f64)>, u32)>,
}

/// Minimum map magnification at which one-cell organism markers are eligible
/// for hover inspection.
pub const ORGANISM_PICK_ZOOM: u32 = 4;

/// Nearest realized organism whose one-cell marker covers `world` at an
/// inspectable zoom. This is presentation picking only; it never feeds world
/// identity or simulation.
#[must_use]
pub fn pick_organism(
    map: &RegionMap,
    world: (f64, f64),
    zoom: u32,
) -> Option<&world_runtime::Organism> {
    if zoom < ORGANISM_PICK_ZOOM {
        return None;
    }
    let cell = REGION_SIZE / f64::from(map.config().field_resolution);
    map.organisms()
        .filter_map(|organism| {
            let distance = f64::hypot(
                organism.world_pos.0 - world.0,
                organism.world_pos.1 - world.1,
            );
            (distance <= cell).then_some((distance, organism))
        })
        .min_by(|(left, _), (right, _)| left.total_cmp(right))
        .map(|(_, organism)| organism)
}

/// Map rendering path selected by the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapBackend {
    /// Canonical CPU raster.
    Cpu,
    /// Derived WebGPU atlas composition.
    GpuAtlas,
}

/// Why a requested GPU-atlas draw was prepared through the canonical CPU
/// path instead. The actual path remains available as [`MapRenderPacket::backend`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapBackendFallback {
    /// The platform has no initialized map-capable GPU renderer.
    GpuUnavailable,
    /// This channel depends on CPU-only presentation inputs.
    UnsupportedChannel(Channel),
}

/// Backend-independent source projection metadata for one map draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapProjection {
    /// Square source raster edge in field cells.
    pub side: u32,
    /// Integer magnification about the map center.
    pub zoom: u32,
    /// Source-cell coverage required for a visible region boundary.
    pub grid_thickness_cells: f32,
}

/// Input to canonical map render preparation. Platform shells supply
/// capability and durable decor, but do not duplicate backend selection,
/// atlas packing, refinement construction, or CPU composition.
#[derive(Debug, Clone, Copy)]
pub struct MapRenderRequest<'a> {
    /// CPU-authoritative streamed world state.
    pub map: &'a RegionMap,
    /// Shared traveler position and map center.
    pub player: (f64, f64),
    /// Selected map channel.
    pub channel: Channel,
    /// Overlay preferences.
    pub overlays: Overlays,
    /// Active steering anchors used by the Influence channel.
    pub anchors: &'a [Anchor],
    /// Vault-derived map decorations.
    pub decor: &'a MapDecor,
    /// Backend requested by shared viewer preferences.
    pub requested_backend: MapBackend,
    /// Whether the platform currently has an initialized GPU map renderer.
    pub gpu_available: bool,
    /// Presentation-only refinement request.
    pub refinement: RefinementRequest,
    /// Controller/host dirty key carried through for redraw suppression.
    pub dirty_key: u64,
}

/// Canonical RGBA8 map image ready for a CPU texture/canvas upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparedCpuMap<'a> {
    /// Complete, layer-composited RGBA8 source image.
    pub rgba: &'a [u8],
}

/// Atlas work and sparse overlay ready for a GPU map submission.
#[derive(Debug)]
pub struct PreparedGpuMap<'a> {
    /// Renderer-ready parameters, including zoom, grid width, and refinement.
    pub params: GpuMapParams,
    /// Visible-window region-to-atlas-slot lookup.
    pub slots: Vec<i32>,
    /// Dependency-keyed changed region uploads; empty at steady state.
    pub uploads: Vec<MapTileUpload>,
    /// Canonically ordered sparse layers folded into one straight-alpha image.
    pub overlay_rgba: &'a [u8],
    /// Stable hash of [`Self::overlay_rgba`] for upload suppression.
    pub overlay_hash: u64,
}

/// Prepared source for exactly one actual map backend.
#[derive(Debug)]
pub enum PreparedMapSource<'a> {
    /// Canonical CPU image.
    Cpu(PreparedCpuMap<'a>),
    /// Derived GPU atlas submission.
    GpuAtlas(PreparedGpuMap<'a>),
}

/// Shared prepared-render packet. It borrows the composer's reusable CPU or
/// overlay buffer and owns only derived atlas lookup/upload work, so platform
/// code only uploads and submits the selected path.
#[derive(Debug)]
pub struct MapRenderPacket<'a> {
    /// Shared source projection.
    pub projection: MapProjection,
    /// Selected map channel.
    pub channel: Channel,
    /// Overlay preferences.
    pub overlays: Overlays,
    /// Backend requested by viewer preferences.
    pub requested_backend: MapBackend,
    /// Actual backend selected for this draw.
    pub backend: MapBackend,
    /// Reason a requested GPU path fell back, if any.
    pub fallback: Option<MapBackendFallback>,
    /// Host/controller-provided key for redraw suppression.
    pub dirty_key: u64,
    /// Hash of the packet's CPU image or GPU folded-overlay pixels.
    pub pixel_hash: u64,
    /// Prepared work for the actual backend.
    pub source: PreparedMapSource<'a>,
}

impl MapRenderPacket<'_> {
    /// Canonical CPU pixels when [`Self::backend`] is [`MapBackend::Cpu`].
    #[must_use]
    pub fn cpu_rgba(&self) -> Option<&[u8]> {
        match &self.source {
            PreparedMapSource::Cpu(cpu) => Some(cpu.rgba),
            PreparedMapSource::GpuAtlas(_) => None,
        }
    }

    /// Prepared atlas work when [`Self::backend`] is [`MapBackend::GpuAtlas`].
    #[must_use]
    pub const fn gpu(&self) -> Option<&PreparedGpuMap<'_>> {
        match &self.source {
            PreparedMapSource::Cpu(_) => None,
            PreparedMapSource::GpuAtlas(gpu) => Some(gpu),
        }
    }
}

/// Order-stable presentation-pixel hash shared by native and browser upload
/// suppression. It is derived presentation only and must never feed world
/// identity, generation, or persistence.
#[must_use]
pub fn map_pixel_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0x0DDB_1A5E_D0F0_0006;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        hash = mix(
            hash,
            u64::from_le_bytes(chunk.try_into().expect("eight-byte map chunk")),
        );
    }
    for &byte in chunks.remainder() {
        hash = mix(hash, u64::from(byte));
    }
    hash
}

/// Result of advancing transient map presentation state for a logical tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapPresenterUpdate {
    /// Tick serial accepted by the presenter.
    pub tick: u64,
    /// `false` for a duplicate or stale serial, which cannot age state twice.
    pub advanced: bool,
    /// Continuity violations first observed during this tick.
    pub new_pinned_violations: u64,
    /// Total continuity violations observed by this presenter.
    pub total_pinned_violations: u64,
    /// Regions whose transient flash remains active after this tick.
    pub flashing_regions: usize,
    /// Whether visible transient presentation changed on this tick. This is
    /// true when a flash begins or whenever an active flash expires, so a
    /// sleeping map redraws every clearing frame.
    pub presentation_changed: bool,
}

/// Distinct tints per possibility domain, indexed like
/// [`world_core::PossibilityDomain::ALL`] — used to colour the anchor-influence
/// channel by which trait an anchor steers (phase-4-plan.md §11).
const DOMAIN_TINTS: [[u8; 3]; POSSIBILITY_DIMS] = [
    [90, 150, 230],  // Planetary
    [230, 120, 90],  // Climate
    [170, 140, 110], // Geology
    [80, 170, 220],  // Hydrology
    [110, 200, 90],  // Ecology
    [210, 150, 220], // Morphology
    [230, 200, 90],  // Behavior
    [240, 120, 180], // Aesthetics
];

/// Summed anchor influence at a cell, coloured by the dominant steered domain
/// and brightened by total influence over a dark base (phase-4-plan.md §11).
fn influence_color(anchors: &[Anchor], world: (f64, f64)) -> [u8; 3] {
    let mut per_domain = [0.0f32; POSSIBILITY_DIMS];
    let mut total = 0.0f32;
    for anchor in anchors {
        let inf = anchor.influence(world);
        if inf <= 0.0 {
            continue;
        }
        total += inf;
        for (i, slot) in per_domain.iter_mut().enumerate() {
            if anchor.mask & (1 << i as u8) != 0 {
                *slot += inf;
            }
        }
    }
    if total <= 0.0 {
        return [18, 18, 22];
    }
    let dominant = per_domain
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map_or(0, |(i, _)| i);
    lerp_rgb([18, 18, 22], DOMAIN_TINTS[dominant], total.clamp(0.0, 1.0))
}

/// Composes the active window into an RGBA8 image, one pixel per field cell,
/// and tracks pinned-region revisions for the changed-while-pinned detector.
#[derive(Debug)]
pub struct MapComposer {
    /// Half-extent of the view in regions (view is `2 * half + 1` square).
    half_regions: i32,
    /// Field cells per region edge (matches the stream config resolution).
    resolution: u16,
    pixels: Vec<u8>,
    /// Integer view magnification about the image center (scroll wheel).
    /// Presentation only: the base image is composed as usual and then the
    /// center block is blown up nearest-neighbor, so zooming reveals no data
    /// beyond the field resolution — it makes single-pixel markers (organisms)
    /// readable and pickable. [`Self::pixel_to_world`] inverts it.
    zoom: u32,
    /// Source-cell grid coverage shared with the GPU projection.
    grid_thickness_cells: f32,
    /// Scratch buffer the magnify step writes into (swapped with `pixels`).
    zoom_scratch: Vec<u8>,
    /// Last logical tick that advanced transient presentation state.
    last_presenter_tick: Option<u64>,
    /// Last revision seen per pinned region.
    pinned_revisions: BTreeMap<RegionCoord, u32>,
    /// Frames of highlight left per offending region.
    flash: BTreeMap<RegionCoord, u8>,
    /// Total changed-while-pinned events observed (a continuity-bug counter).
    pub pinned_violations: u64,
}

const FLASH_FRAMES: u8 = 45;

/// Realized-state revisions mapped to white on the [`Channel::Revision`] ramp.
/// A region that has changed its realized state this many times (or more) reads
/// as fully churned; steady/pinned regions stay near black.
const REVISION_WHITE: u32 = 256;

/// Mean per-domain `|current - target|` mapped to white on the
/// [`Channel::Residual`] ramp. Both vectors live in `[0, 1]`, so the raw mean
/// gap is bounded by 1, but real convergence residuals are small — a modest cap
/// keeps in-flight churn visible instead of washed out near black.
const RESIDUAL_WHITE: f32 = 0.25;

/// Preserve outline tint (phase-5-plan.md §11).
const PRESERVE_OUTLINE: [u8; 3] = [120, 255, 170];
/// Route polyline base tint; brightness saturates with usage.
const ROUTE_TINT: [u8; 3] = [255, 220, 120];

impl MapComposer {
    /// A composer viewing `half_regions` in every direction at `resolution`
    /// cells per region.
    #[must_use]
    pub fn new(half_regions: i32, resolution: u16) -> Self {
        let side = Self::side_for(half_regions, resolution);
        Self {
            half_regions,
            resolution,
            pixels: vec![0; side * side * 4],
            zoom: 1,
            grid_thickness_cells: 1.0,
            zoom_scratch: vec![0; side * side * 4],
            last_presenter_tick: None,
            pinned_revisions: BTreeMap::new(),
            flash: BTreeMap::new(),
            pinned_violations: 0,
        }
    }

    /// Set the view magnification (clamped to at least 1). CPU composition
    /// applies it directly; GPU packets carry the same projection so the
    /// shader transforms the base field and overlays together.
    pub fn set_zoom(&mut self, zoom: u32) {
        self.zoom = zoom.max(1);
    }

    /// Set source-cell grid coverage. Fractional values are preserved for the
    /// GPU projection; the CPU raster covers their ceiling, at least one cell.
    pub fn set_grid_thickness_cells(&mut self, thickness: f32) {
        self.grid_thickness_cells = if thickness.is_finite() {
            thickness.max(1.0)
        } else {
            1.0
        };
    }

    fn side_for(half_regions: i32, resolution: u16) -> usize {
        (2 * half_regions + 1) as usize * resolution as usize
    }

    /// Image edge length in pixels.
    #[must_use]
    pub fn side(&self) -> u32 {
        Self::side_for(self.half_regions, self.resolution) as u32
    }

    /// Regions viewed in every direction from the center (the view is a
    /// `2·half+1` square) — the bound the shell uses to gather per-region
    /// decor (discovered set, preserves) for exactly the visible window.
    #[must_use]
    pub const fn half_regions(&self) -> i32 {
        self.half_regions
    }

    /// Current backend-independent projection metadata.
    #[must_use]
    pub fn projection(&self) -> MapProjection {
        MapProjection {
            side: self.side(),
            zoom: self.zoom,
            grid_thickness_cells: self.grid_thickness_cells,
        }
    }

    /// Prepare all CPU or GPU-atlas work for one map draw. GPU requests fall
    /// back to canonical CPU composition when the platform renderer is absent
    /// or the selected channel has no derived GPU implementation.
    pub fn prepare_render<'a>(
        &'a mut self,
        atlas: &mut AtlasManager,
        request: MapRenderRequest<'_>,
    ) -> MapRenderPacket<'a> {
        let projection = self.projection();
        let gpu_selection = if request.requested_backend == MapBackend::GpuAtlas {
            if request.gpu_available {
                match gpu_channel(request.channel) {
                    Some(channel) => Ok(channel),
                    None => Err(MapBackendFallback::UnsupportedChannel(request.channel)),
                }
            } else {
                Err(MapBackendFallback::GpuUnavailable)
            }
        } else {
            Err(MapBackendFallback::GpuUnavailable)
        };

        if let Ok(channel) = gpu_selection {
            let center = RegionCoord::from_world(request.player.0, request.player.1);
            let resolution = request.map.config().field_resolution;
            debug_assert_eq!(resolution, self.resolution);
            let (slots, uploads) = atlas.sync(request.map, center, self.half_regions, resolution);
            let (west, north) = self.view_origin(request.player);
            let refinement_count = if request.refinement.enabled {
                u32::from(request.refinement.octave_count)
            } else {
                0
            };
            let (refine, refine_count) =
                refinement_octaves(west, north, resolution, refinement_count);
            let params = GpuMapParams {
                half_regions: self.half_regions,
                resolution: u32::from(resolution),
                channel,
                zoom: projection.zoom,
                grid_thickness_cells: projection.grid_thickness_cells,
                refine,
                refine_count,
            };
            let overlay_rgba =
                self.compose_overlays(request.map, request.player, request.overlays, request.decor);
            let overlay_hash = map_pixel_hash(overlay_rgba);
            return MapRenderPacket {
                projection,
                channel: request.channel,
                overlays: request.overlays,
                requested_backend: request.requested_backend,
                backend: MapBackend::GpuAtlas,
                fallback: None,
                dirty_key: request.dirty_key,
                pixel_hash: overlay_hash,
                source: PreparedMapSource::GpuAtlas(PreparedGpuMap {
                    params,
                    slots,
                    uploads,
                    overlay_rgba,
                    overlay_hash,
                }),
            };
        }

        let fallback = (request.requested_backend == MapBackend::GpuAtlas)
            .then(|| gpu_selection.expect_err("GPU selection did not succeed"));
        let rgba = self.compose(
            request.map,
            request.player,
            request.channel,
            request.overlays,
            request.anchors,
            request.decor,
        );
        let pixel_hash = map_pixel_hash(rgba);
        MapRenderPacket {
            projection,
            channel: request.channel,
            overlays: request.overlays,
            requested_backend: request.requested_backend,
            backend: MapBackend::Cpu,
            fallback,
            dirty_key: request.dirty_key,
            pixel_hash,
            source: PreparedMapSource::Cpu(PreparedCpuMap { rgba }),
        }
    }

    /// Advance pinned-revision detection and flash aging exactly once for a
    /// monotonically increasing logical viewer tick. Calling this from POV-only
    /// ticks keeps diagnostics truthful; duplicate live/dump compositions with
    /// the same serial cannot age or count state again.
    pub fn update_for_tick(&mut self, tick: u64, map: &RegionMap) -> MapPresenterUpdate {
        if self.last_presenter_tick.is_some_and(|last| tick <= last) {
            return MapPresenterUpdate {
                tick,
                advanced: false,
                new_pinned_violations: 0,
                total_pinned_violations: self.pinned_violations,
                flashing_regions: self.flash.len(),
                presentation_changed: false,
            };
        }
        self.last_presenter_tick = Some(tick);
        let before = self.pinned_violations;
        let flashing_before = self.flash.len();
        self.detect_pinned_changes(map);
        let new_pinned_violations = self.pinned_violations - before;
        let flashing_regions = self.flash.len();
        MapPresenterUpdate {
            tick,
            advanced: true,
            new_pinned_violations,
            total_pinned_violations: self.pinned_violations,
            flashing_regions,
            presentation_changed: new_pinned_violations > 0 || flashing_before != flashing_regions,
        }
    }

    /// Compose the map for this frame and return the RGBA buffer
    /// (row 0 = north edge, as the renderer expects).
    pub fn compose(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        channel: Channel,
        overlays: Overlays,
        anchors: &[Anchor],
        decor: &MapDecor,
    ) -> &[u8] {
        let center = RegionCoord::from_world(player.0, player.1);
        for layer in MAP_LAYER_SEQUENCE {
            if !layer.enabled(overlays) {
                continue;
            }
            match layer {
                MapLayer::Base => {
                    for row_region in 0..=(2 * self.half_regions) {
                        // Row 0 is the northernmost (max y) region.
                        let ry = center.y + self.half_regions - row_region;
                        for col_region in 0..=(2 * self.half_regions) {
                            let rx = center.x - self.half_regions + col_region;
                            self.paint_region(
                                map,
                                RegionCoord::new(rx, ry),
                                channel,
                                row_region,
                                col_region,
                                anchors,
                            );
                        }
                    }
                }
                MapLayer::PinnedFlash => self.tint_flashing_regions(center),
                MapLayer::Grid => self.draw_grid(),
                MapLayer::Discovered => {
                    if let Some(seen) = &decor.seen {
                        self.dim_undiscovered(center, seen);
                    }
                }
                MapLayer::Routes => {
                    for (path, usage) in &decor.routes {
                        self.draw_route(player, path, *usage);
                    }
                }
                MapLayer::Preserves => {
                    for &coord in &decor.preserves {
                        self.outline_region(center, coord, PRESERVE_OUTLINE);
                    }
                }
                MapLayer::Organisms => self.draw_organisms(map, player),
                MapLayer::Rings => self.draw_rings(map, player),
                MapLayer::Player => self.draw_player_marker(player),
            }
        }
        self.magnify();
        &self.pixels
    }

    /// Compose only the sparse overlay content into a transparent RGBA
    /// buffer for the GPU-composed map (phase-6-plan.md §6.5): pinned-flash
    /// fills, undiscovered dimming, routes, preserve outlines, organisms,
    /// rings, grid, and the player marker. Only base field painting stays in
    /// the atlas shader; folding the grid here preserves its exact declared
    /// order relative to translucent overlays. Transient presenter state is
    /// read-only here and must be advanced once through
    /// [`Self::update_for_tick`] (ADR 0017).
    pub fn compose_overlays(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        overlays: Overlays,
        decor: &MapDecor,
    ) -> &[u8] {
        self.pixels.fill(0);
        let center = RegionCoord::from_world(player.0, player.1);
        for layer in MAP_LAYER_SEQUENCE {
            if !layer.enabled(overlays) {
                continue;
            }
            match layer {
                // The selected base channel is prepared from the GPU atlas.
                // Grid stays in the sparse overlay so it composes after the
                // pinned flash and before every later layer, exactly matching
                // the canonical stack.
                MapLayer::Base => {}
                MapLayer::PinnedFlash => self.fold_flashing_regions(center),
                MapLayer::Grid => self.draw_overlay_grid(),
                MapLayer::Discovered => {
                    if let Some(seen) = &decor.seen {
                        for row in 0..=(2 * self.half_regions) {
                            let ry = center.y + self.half_regions - row;
                            for col in 0..=(2 * self.half_regions) {
                                let rx = center.x - self.half_regions + col;
                                let coord = RegionCoord::new(rx, ry);
                                if !seen.contains(&coord) {
                                    // alpha 113/255 ≈ keep 5/9, the CPU dim.
                                    self.fill_region(center, coord, [0, 0, 0], 113);
                                }
                            }
                        }
                    }
                }
                MapLayer::Routes => {
                    for (path, usage) in &decor.routes {
                        self.draw_route(player, path, *usage);
                    }
                }
                MapLayer::Preserves => {
                    for &coord in &decor.preserves {
                        self.outline_region(center, coord, PRESERVE_OUTLINE);
                    }
                }
                MapLayer::Organisms => self.draw_organisms(map, player),
                MapLayer::Rings => self.draw_rings(map, player),
                MapLayer::Player => self.draw_player_marker(player),
            }
        }
        &self.pixels
    }

    /// Apply the pinned-change tint as its declared visual layer. Keeping this
    /// out of base-cell painting makes the ordering shared with GPU overlays.
    fn tint_flashing_regions(&mut self, center: RegionCoord) {
        let res = self.resolution as usize;
        let side = self.side() as usize;
        let span = 2 * self.half_regions;
        let pixels = &mut self.pixels;
        for &coord in self.flash.keys() {
            let row_region = center.y + self.half_regions - coord.y;
            let col_region = coord.x - center.x + self.half_regions;
            if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
                continue;
            }
            for py in row_region as usize * res..(row_region as usize + 1) * res {
                let row = py * side;
                for px in col_region as usize * res..(col_region as usize + 1) * res {
                    let offset = (row + px) * 4;
                    let rgb = lerp_rgb(
                        [pixels[offset], pixels[offset + 1], pixels[offset + 2]],
                        [255, 30, 30],
                        0.6,
                    );
                    pixels[offset..offset + 3].copy_from_slice(&rgb);
                }
            }
        }
    }

    /// Fold all active pinned flashes into the reusable sparse overlay without
    /// allocating a temporary coordinate list.
    fn fold_flashing_regions(&mut self, center: RegionCoord) {
        let res = self.resolution as usize;
        let side = self.side() as usize;
        let span = 2 * self.half_regions;
        let pixels = &mut self.pixels;
        for &coord in self.flash.keys() {
            let row_region = center.y + self.half_regions - coord.y;
            let col_region = coord.x - center.x + self.half_regions;
            if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
                continue;
            }
            for py in row_region as usize * res..(row_region as usize + 1) * res {
                let row = py * side;
                for px in col_region as usize * res..(col_region as usize + 1) * res {
                    Self::blend_overlay_pixel(pixels, (row + px) * 4, [255, 30, 30], 153);
                }
            }
        }
    }

    /// Darken the same south/west region-boundary cells as the original CPU
    /// composer, widening to the shared source-cell thickness when requested.
    fn draw_grid(&mut self) {
        let res = self.resolution as usize;
        let cells = self.grid_thickness_cells.ceil() as usize;
        let cells = cells.clamp(1, res);
        let side = self.side() as usize;
        for py in 0..side {
            let local_y = py % res;
            for px in 0..side {
                let local_x = px % res;
                if local_x >= cells && local_y < res - cells {
                    continue;
                }
                let offset = (py * side + px) * 4;
                let rgb = lerp_rgb(
                    [
                        self.pixels[offset],
                        self.pixels[offset + 1],
                        self.pixels[offset + 2],
                    ],
                    [0, 0, 0],
                    0.35,
                );
                self.pixels[offset..offset + 3].copy_from_slice(&rgb);
            }
        }
    }

    /// Encode the grid as an affine sparse-overlay operation. Folding it into
    /// the one RGBA overlay preserves Base → PinnedFlash → Grid ordering even
    /// though the GPU submits the folded overlay in one final texture blend.
    fn draw_overlay_grid(&mut self) {
        let res = self.resolution as usize;
        let cells = (self.grid_thickness_cells.ceil() as usize).clamp(1, res);
        let side = self.side() as usize;
        for py in 0..side {
            let local_y = py % res;
            for px in 0..side {
                let local_x = px % res;
                if local_x >= cells && local_y < res - cells {
                    continue;
                }
                Self::blend_overlay_pixel(&mut self.pixels, (py * side + px) * 4, [0, 0, 0], 89);
            }
        }
    }

    /// Fold one straight-alpha layer over the transform already encoded at a
    /// sparse-overlay pixel. The result remains a single straight-alpha RGBA
    /// operation that can be mixed over any atlas base color.
    fn blend_overlay_pixel(pixels: &mut [u8], offset: usize, rgb: [u8; 3], alpha: u8) {
        let old_alpha = u32::from(pixels[offset + 3]);
        let new_alpha = u32::from(alpha);
        let out_alpha = new_alpha * 255 + old_alpha * (255 - new_alpha);
        if out_alpha == 0 {
            return;
        }
        for (component, &new_component) in rgb.iter().enumerate() {
            let old = u32::from(pixels[offset + component]);
            let premultiplied =
                u32::from(new_component) * new_alpha * 255 + old * old_alpha * (255 - new_alpha);
            pixels[offset + component] = ((premultiplied + out_alpha / 2) / out_alpha) as u8;
        }
        pixels[offset + 3] = ((out_alpha + 127) / 255) as u8;
    }

    /// Blend `rgb` at `alpha` over one region's pixel block (overlay mode).
    fn fill_region(&mut self, center: RegionCoord, coord: RegionCoord, rgb: [u8; 3], alpha: u8) {
        let row_region = center.y + self.half_regions - coord.y;
        let col_region = coord.x - center.x + self.half_regions;
        let span = 2 * self.half_regions;
        if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
            return;
        }
        let res = self.resolution as usize;
        let side = self.side() as usize;
        for py in row_region as usize * res..(row_region as usize + 1) * res {
            let row = py * side;
            for px in col_region as usize * res..(col_region as usize + 1) * res {
                let offset = (row + px) * 4;
                Self::blend_overlay_pixel(&mut self.pixels, offset, rgb, alpha);
            }
        }
    }

    /// Dim every view region the explorer has never visited (phase-5-plan.md
    /// §11): the discovered world reads bright, the unknown reads dark — the
    /// first appearance of the atlas map.
    fn dim_undiscovered(
        &mut self,
        center: RegionCoord,
        seen: &std::collections::BTreeSet<RegionCoord>,
    ) {
        let res = self.resolution as usize;
        let side = self.side() as usize;
        for row_region in 0..=(2 * self.half_regions) as usize {
            let ry = center.y + self.half_regions - row_region as i32;
            for col_region in 0..=(2 * self.half_regions) as usize {
                let rx = center.x - self.half_regions + col_region as i32;
                if seen.contains(&RegionCoord::new(rx, ry)) {
                    continue;
                }
                for py in row_region * res..(row_region + 1) * res {
                    let row = py * side;
                    for px in col_region * res..(col_region + 1) * res {
                        let offset = (row + px) * 4;
                        for c in &mut self.pixels[offset..offset + 3] {
                            *c = (u16::from(*c) * 5 / 9) as u8;
                        }
                    }
                }
            }
        }
    }

    /// Outline one region's pixel block (preserves, phase-5-plan.md §11).
    fn outline_region(&mut self, center: RegionCoord, coord: RegionCoord, rgb: [u8; 3]) {
        let row_region = center.y + self.half_regions - coord.y;
        let col_region = coord.x - center.x + self.half_regions;
        let span = 2 * self.half_regions;
        if !(0..=span).contains(&row_region) || !(0..=span).contains(&col_region) {
            return;
        }
        let res = self.resolution as i64;
        let x0 = i64::from(col_region) * res;
        let y0 = i64::from(row_region) * res;
        for k in 0..res {
            self.plot(x0 + k, y0, rgb);
            self.plot(x0 + k, y0 + res - 1, rgb);
            self.plot(x0, y0 + k, rgb);
            self.plot(x0 + res - 1, y0 + k, rgb);
        }
    }

    /// A recorded route as a polyline through its node positions, brightness
    /// saturating with usage ("frequently used routes become easier to
    /// follow", Overview; phase-5-plan.md §11).
    fn draw_route(&mut self, player: (f64, f64), path: &[(f64, f64)], usage: u32) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let brightness = 0.45 + 0.55 * (usage as f32 / (usage as f32 + 4.0));
        let rgb = [
            (ROUTE_TINT[0] as f32 * brightness) as u8,
            (ROUTE_TINT[1] as f32 * brightness) as u8,
            (ROUTE_TINT[2] as f32 * brightness) as u8,
        ];
        let to_px = |(wx, wy): (f64, f64)| ((wx - west) / cell, (north - wy) / cell);
        for pair in path.windows(2) {
            let (x0, y0) = to_px(pair[0]);
            let (x1, y1) = to_px(pair[1]);
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                self.plot(
                    (x0 + (x1 - x0) * t) as i64,
                    (y0 + (y1 - y0) * t) as i64,
                    rgb,
                );
            }
        }
    }

    /// Near-field organism markers, coloured by expressed appearance. A marker
    /// that contradicts its cell's aggregate tint is instantly visible — the
    /// Tier-B popping/coherence detector (phase-3-plan.md §11).
    fn draw_organisms(&mut self, map: &RegionMap, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        for organism in map.organisms() {
            let (wx, wy) = organism.world_pos;
            let px = ((wx - west) / cell) as i64;
            let py = ((north - wy) / cell) as i64;
            let rgb = expressed_color(&organism.expressed);
            self.plot(px, py, rgb);
        }
    }

    /// The most recently composed RGBA buffer (valid after [`Self::compose`]).
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Update the pinned-revision ledger; regions whose revision advanced
    /// while pinned are continuity bugs by definition (phase-1-plan.md §10).
    fn detect_pinned_changes(&mut self, map: &RegionMap) {
        let mut next = BTreeMap::new();
        for region in map.iter_active() {
            if region.stability >= 1.0 {
                if let Some(&prev) = self.pinned_revisions.get(&region.coord) {
                    if region.revision != prev {
                        self.flash.insert(region.coord, FLASH_FRAMES);
                        self.pinned_violations += 1;
                        log::warn!(
                            "continuity: region ({}, {}) revision {} -> {} while pinned",
                            region.coord.x,
                            region.coord.y,
                            prev,
                            region.revision
                        );
                    }
                }
                next.insert(region.coord, region.revision);
            }
        }
        self.pinned_revisions = next;
        self.flash.retain(|_, frames| {
            *frames = frames.saturating_sub(1);
            *frames > 0
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_region(
        &mut self,
        map: &RegionMap,
        coord: RegionCoord,
        channel: Channel,
        row_region: i32,
        col_region: i32,
        anchors: &[Anchor],
    ) {
        let res = self.resolution;
        let side = self.side() as usize;
        let state = map.get(coord);
        let tiles = map.cache().get(coord);

        let tile = |channel_index: usize| tiles.and_then(|t| t.channels[channel_index].as_deref());
        let elevation = tile(CHANNEL_ELEVATION);
        let hardness = tile(CHANNEL_HARDNESS);
        let temperature = tile(CHANNEL_TEMPERATURE);
        let moisture = tile(CHANNEL_MOISTURE);
        let river = tile(CHANNEL_RIVER);
        let wetness = tile(CHANNEL_WETNESS);
        let soil_depth = tile(CHANNEL_SOIL_DEPTH);
        let fertility = tile(CHANNEL_FERTILITY);
        let vegetation = tile(CHANNEL_VEGETATION);
        let herbivore = tile(CHANNEL_HERBIVORE);
        let predator = tile(CHANNEL_PREDATOR);
        let diversity = tile(CHANNEL_DIVERSITY);
        let biome = tiles.and_then(|t| t.biome.as_deref());

        let (origin_x, origin_y) = coord.origin();
        let cell = REGION_SIZE / f64::from(res);

        for cy in 0..res {
            for cx in 0..res {
                let scalar = |t: Option<&world_core::FieldTile<f32>>,
                              paint: &dyn Fn(f32) -> [u8; 3]| {
                    t.map(|t| paint(t.get(cx, cy)))
                        .unwrap_or_else(|| missing_color(cx, cy))
                };
                let rgb = match channel {
                    Channel::Elevation => scalar(elevation, &elevation_color),
                    Channel::Temperature => scalar(temperature, &temperature_color),
                    Channel::Moisture => scalar(moisture, &moisture_color),
                    Channel::River => scalar(river, &river_color),
                    Channel::Wetness => scalar(wetness, &wetness_color),
                    Channel::Vegetation => scalar(vegetation, &vegetation_color),
                    Channel::Herbivore => scalar(herbivore, &herbivore_color),
                    Channel::Predator => scalar(predator, &predator_color),
                    Channel::Diversity => scalar(diversity, &diversity_color),
                    Channel::DominantSpecies => match map.dominant_species_id(coord, cx, cy) {
                        Some(id) => species_color(id),
                        None => missing_color(cx, cy),
                    },
                    Channel::Influence => {
                        let wx = origin_x + (f64::from(cx) + 0.5) * cell;
                        let wy = origin_y + (f64::from(cy) + 0.5) * cell;
                        influence_color(anchors, (wx, wy))
                    }
                    Channel::Geology => match hardness {
                        Some(h) => {
                            let wx = origin_x + (f64::from(cx) + 0.5) * cell;
                            let wy = origin_y + (f64::from(cy) + 0.5) * cell;
                            geology_color(wx, wy, h.get(cx, cy))
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Soil => match (soil_depth, fertility) {
                        (Some(d), Some(f)) => soil_color(d.get(cx, cy), f.get(cx, cy)),
                        _ => missing_color(cx, cy),
                    },
                    Channel::Biome => match biome {
                        Some(b) => biome_color(Biome::from_id(b.get(cx, cy))),
                        None => missing_color(cx, cy),
                    },
                    Channel::Stability => match state {
                        Some(r) => {
                            let s = (r.stability * 255.0) as u8;
                            [s, s, s]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Revision => match state {
                        Some(r) => {
                            let g = (r.revision.min(REVISION_WHITE) as f32 / REVISION_WHITE as f32
                                * 255.0) as u8;
                            [g, g, g]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Residual => match state {
                        Some(r) => {
                            let mut sum = 0.0f32;
                            for i in 0..POSSIBILITY_DIMS {
                                sum += (r.current.dims[i] - r.target.dims[i]).abs();
                            }
                            let mean = sum / POSSIBILITY_DIMS as f32;
                            let g = ((mean / RESIDUAL_WHITE).clamp(0.0, 1.0) * 255.0) as u8;
                            [g, g, g]
                        }
                        None => missing_color(cx, cy),
                    },
                    Channel::Composite => match (elevation, biome, river, wetness) {
                        (Some(e), Some(b), Some(r), Some(w)) => composite_cell_color(
                            e.get(cx, cy),
                            Biome::from_id(b.get(cx, cy)),
                            r.get(cx, cy),
                            w.get(cx, cy),
                            map.dominant_species_id(coord, cx, cy),
                        ),
                        _ => missing_color(cx, cy),
                    },
                };

                // Cell (cx, cy) has cy growing north; image rows grow south.
                let px = col_region as usize * res as usize + cx as usize;
                let py_region = row_region as usize * res as usize;
                let py = py_region + (res - 1 - cy) as usize;
                let offset = (py * side + px) * 4;
                self.pixels[offset] = rgb[0];
                self.pixels[offset + 1] = rgb[1];
                self.pixels[offset + 2] = rgb[2];
                self.pixels[offset + 3] = 255;
            }
        }
    }

    /// Blow the center block of the composed image up by the zoom factor,
    /// nearest-neighbor, in place (a swap with the scratch buffer). Everything
    /// already drawn — field cells and overlay markers alike — magnifies
    /// together, so the zoomed view stays a faithful crop of the base map.
    fn magnify(&mut self) {
        if self.zoom <= 1 {
            return;
        }
        let side = self.side() as usize;
        let zoom = f64::from(self.zoom);
        let center = side as f64 / 2.0;
        debug_assert_eq!(self.zoom_scratch.len(), self.pixels.len());
        // Compute indices in place instead of allocating a per-frame lookup.
        // This is the continuous zoom-about-center mapping, floored to a base
        // pixel (and mirrors `pixel_to_world`).
        let source_index = |i: usize| {
            let source = (i as f64 + 0.5 - center) / zoom + center;
            (source.max(0.0) as usize).min(side - 1)
        };
        for oy in 0..side {
            let sy = source_index(oy);
            let src_row = sy * side;
            let dst_row = oy * side;
            for ox in 0..side {
                let sx = source_index(ox);
                let s = (src_row + sx) * 4;
                let d = (dst_row + ox) * 4;
                self.zoom_scratch[d..d + 4].copy_from_slice(&self.pixels[s..s + 4]);
            }
        }
        std::mem::swap(&mut self.pixels, &mut self.zoom_scratch);
    }

    /// World position at the center of image pixel `(px, py)` — the inverse of
    /// the compose mapping (including the zoom magnification), for mouse
    /// picking. Returns `None` outside the map.
    #[must_use]
    pub fn pixel_to_world(&self, player: (f64, f64), px: f64, py: f64) -> Option<(f64, f64)> {
        let side = f64::from(self.side());
        if px < 0.0 || py < 0.0 || px >= side || py >= side {
            return None;
        }
        // Undo the magnify-about-center step first (identity at zoom 1).
        let zoom = f64::from(self.zoom);
        let center = side / 2.0;
        let px = (px - center) / zoom + center;
        let py = (py - center) / zoom + center;
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        Some((west + (px + 0.5) * cell, north - (py + 0.5) * cell))
    }

    /// World position of the view's north-west pixel corner.
    fn view_origin(&self, player: (f64, f64)) -> (f64, f64) {
        let center = RegionCoord::from_world(player.0, player.1);
        let west = (center.x - self.half_regions) as f64 * REGION_SIZE;
        let north = (center.y + self.half_regions + 1) as f64 * REGION_SIZE;
        (west, north)
    }

    fn plot(&mut self, px: i64, py: i64, rgb: [u8; 3]) {
        let side = self.side() as i64;
        if px < 0 || py < 0 || px >= side || py >= side {
            return;
        }
        let offset = ((py * side + px) * 4) as usize;
        self.pixels[offset] = rgb[0];
        self.pixels[offset + 1] = rgb[1];
        self.pixels[offset + 2] = rgb[2];
        self.pixels[offset + 3] = 255;
    }

    /// Near (white) and far (orange) stability rings around the player.
    fn draw_rings(&mut self, map: &RegionMap, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let rings = [
            (map.config().near_radius, [255u8, 255, 255]),
            (map.config().far_radius, [255, 160, 40]),
        ];
        for (radius, rgb) in rings {
            // Enough angular steps that adjacent plotted pixels touch.
            let steps = ((radius * core::f64::consts::TAU / cell) as usize).max(64);
            for i in 0..steps {
                let a = i as f64 / steps as f64 * core::f64::consts::TAU;
                let wx = player.0 + radius * a.cos();
                let wy = player.1 + radius * a.sin();
                let px = ((wx - west) / cell) as i64;
                let py = ((north - wy) / cell) as i64;
                self.plot(px, py, rgb);
            }
        }
    }

    /// Small cross marking the player's exact world position.
    fn draw_player_marker(&mut self, player: (f64, f64)) {
        let cell = REGION_SIZE / f64::from(self.resolution);
        let (west, north) = self.view_origin(player);
        let px = ((player.0 - west) / cell) as i64;
        let py = ((north - player.1) / cell) as i64;
        for d in -3i64..=3 {
            self.plot(px + d, py, [255, 255, 255]);
            self.plot(px, py + d, [255, 255, 255]);
        }
        self.plot(px, py, [255, 40, 40]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    use world_core::{
        bound_target, domain_mask, AnchorKind, AnchorSource, PossibilityDomain, PossibilityField,
        POSSIBILITY_DIMS, SEA_LEVEL,
    };
    use world_runtime::mapcolor::composite_color;
    use world_runtime::{Budget, InlineExecutor, StreamConfig};

    #[test]
    fn channel_ids_round_trip_and_cycle_once() {
        for (index, channel) in Channel::ALL.into_iter().enumerate() {
            assert_eq!(Channel::from_id(channel.id()), Some(channel));
            assert_eq!(channel.descriptor().channel, channel);
            assert_eq!(channel.descriptor().id, channel.id());
            assert_eq!(usize::from(channel.descriptor().order), index);
            assert_eq!(
                channel.next(),
                Channel::ALL[(index + 1) % Channel::ALL.len()]
            );
        }
        assert_eq!(Channel::from_id("Dominant"), None);
    }

    #[test]
    fn every_overlay_has_one_typed_toggle() {
        let mut overlays = Overlays::default();
        for (index, overlay) in MapOverlay::ALL.into_iter().enumerate() {
            assert_eq!(MapOverlay::from_id(overlay.id()), Some(overlay));
            assert_eq!(overlay.descriptor().overlay, overlay);
            assert_eq!(usize::from(overlay.descriptor().order), index);
            assert!(overlays.enabled(overlay));
            overlays.set(overlay, false);
            assert!(!overlays.enabled(overlay));
        }
        assert_eq!(MapOverlay::from_id("Grid"), None);
    }

    #[test]
    fn layer_descriptors_exhaustively_pin_shared_visual_order() {
        assert_eq!(
            MAP_LAYER_SEQUENCE,
            [
                MapLayer::Base,
                MapLayer::PinnedFlash,
                MapLayer::Grid,
                MapLayer::Discovered,
                MapLayer::Routes,
                MapLayer::Preserves,
                MapLayer::Organisms,
                MapLayer::Rings,
                MapLayer::Player,
            ]
        );
        for (index, layer) in MAP_LAYER_SEQUENCE.into_iter().enumerate() {
            let descriptor = layer.descriptor();
            assert_eq!(descriptor.layer, layer);
            assert_eq!(usize::from(descriptor.order), index);
            assert!(!descriptor.id.is_empty());
            assert!(!descriptor.label.is_empty());
            assert!(!descriptor.group.id().is_empty());
            assert!(!descriptor.group.label().is_empty());
        }
        let always_present: Vec<MapLayer> = MAP_LAYER_SEQUENCE
            .into_iter()
            .filter(|layer| layer.descriptor().overlay.is_none())
            .collect();
        assert_eq!(
            always_present,
            [
                MapLayer::Base,
                MapLayer::Routes,
                MapLayer::Preserves,
                MapLayer::Player,
            ]
        );
    }

    /// A small fully-settled window (the `gpumap.rs` test fixture).
    fn settled_map() -> RegionMap {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 8,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let mut map = RegionMap::new(cfg);
        for _ in 0..6 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        map
    }

    fn no_overlays() -> Overlays {
        Overlays {
            grid: false,
            rings: false,
            pinned_flash: false,
            organisms: false,
            discovered: false,
        }
    }

    fn fixture_anchor() -> Anchor {
        let mask = domain_mask(&[
            PossibilityDomain::Climate,
            PossibilityDomain::Hydrology,
            PossibilityDomain::Ecology,
        ]);
        Anchor {
            world_pos: (256.0, -128.0),
            target: bound_target(mask, 0.82),
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.7,
            falloff_radius: 2_400.0,
            source: AnchorSource::Manual,
        }
    }

    fn force_pinned_revision_change(map: &mut RegionMap, domain: PossibilityDomain) -> RegionCoord {
        let center = RegionCoord::new(0, 0);
        let before = map.get(center).expect("settled center");
        assert_eq!(before.stability, 1.0);
        let mut signature = world_core::PossibilitySignature::of(before.current);
        let bucket = &mut signature.buckets[domain.index()];
        *bucket = if *bucket == 0 {
            world_core::POSSIBILITY_QUANT - 1
        } else {
            0
        };
        let revision = before.revision;
        map.apply_preserve_contribution(0xA11E_0000_0000_0001, center, signature);
        assert_eq!(
            map.get(center).expect("preserved center").revision,
            revision + 1
        );
        center
    }

    #[test]
    fn presenter_state_advances_once_per_tick_independent_of_composition() {
        let mut map = settled_map();
        let mut composer = MapComposer::new(1, 8);
        assert!(composer.update_for_tick(10, &map).advanced);
        let center = force_pinned_revision_change(&mut map, PossibilityDomain::Aesthetics);

        let changed = composer.update_for_tick(11, &map);
        assert!(changed.advanced);
        assert_eq!(changed.new_pinned_violations, 1);
        assert!(changed.presentation_changed);
        assert_eq!(changed.flashing_regions, 1);
        let frames_after_update = composer.flash[&center];

        let first = composer
            .compose(
                &map,
                (0.0, 0.0),
                Channel::Composite,
                Overlays::default(),
                &[],
                &MapDecor::default(),
            )
            .to_vec();
        let second = composer
            .compose(
                &map,
                (0.0, 0.0),
                Channel::Composite,
                Overlays::default(),
                &[],
                &MapDecor::default(),
            )
            .to_vec();
        assert_eq!(first, second, "duplicate draws must be presentation-pure");
        assert_eq!(composer.flash[&center], frames_after_update);
        assert_eq!(composer.pinned_violations, 1);

        let duplicate = composer.update_for_tick(11, &map);
        assert!(!duplicate.advanced);
        assert_eq!(composer.flash[&center], frames_after_update);

        // No map composition occurs between these calls: a POV-only logical
        // tick still ages the transient diagnostic exactly once.
        let pov_tick = composer.update_for_tick(12, &map);
        assert!(pov_tick.advanced);
        assert_eq!(composer.flash[&center], frames_after_update - 1);
        let stale = composer.update_for_tick(9, &map);
        assert!(!stale.advanced);
        assert_eq!(composer.flash[&center], frames_after_update - 1);

        let mut tick = 13;
        loop {
            let update = composer.update_for_tick(tick, &map);
            if update.flashing_regions == 0 {
                assert!(
                    update.presentation_changed,
                    "expiry must clear cached pixels"
                );
                break;
            }
            assert!(
                !update.presentation_changed,
                "the flash tint is constant mid-run"
            );
            tick += 1;
        }
        assert!(
            !composer
                .update_for_tick(tick + 1, &map)
                .presentation_changed
        );
    }

    #[test]
    fn each_staggered_flash_expiry_marks_the_presenter_dirty() {
        let map = settled_map();
        let mut composer = MapComposer::new(1, 8);
        composer.update_for_tick(1, &map);
        composer.flash.insert(RegionCoord::new(0, 0), 1);
        composer.flash.insert(RegionCoord::new(1, 0), 2);

        let first_expiry = composer.update_for_tick(2, &map);
        assert_eq!(first_expiry.flashing_regions, 1);
        assert!(first_expiry.presentation_changed);

        let second_expiry = composer.update_for_tick(3, &map);
        assert_eq!(second_expiry.flashing_regions, 0);
        assert!(second_expiry.presentation_changed);
    }

    #[test]
    fn folded_gpu_overlay_preserves_translucent_layer_order() {
        let mut map = settled_map();
        let mut composer = MapComposer::new(1, 8);
        composer.update_for_tick(1, &map);
        force_pinned_revision_change(&mut map, PossibilityDomain::Aesthetics);
        composer.update_for_tick(2, &map);

        let overlays = Overlays {
            grid: true,
            rings: false,
            pinned_flash: true,
            organisms: false,
            discovered: true,
        };
        let decor = MapDecor {
            seen: Some(BTreeSet::new()),
            ..MapDecor::default()
        };
        let player = (0.0, 0.0);
        let cpu = composer
            .compose(&map, player, Channel::Composite, overlays, &[], &decor)
            .to_vec();
        let base = composer
            .compose(
                &map,
                player,
                Channel::Composite,
                no_overlays(),
                &[],
                &MapDecor::default(),
            )
            .to_vec();
        let overlay = composer
            .compose_overlays(&map, player, overlays, &decor)
            .to_vec();

        let offset = (8 * 24 + 8) * 4;
        assert_eq!(&overlay[offset..offset + 4], &[65, 8, 8, 218]);
        let alpha = f32::from(overlay[offset + 3]) / 255.0;
        for component in 0..3 {
            let folded = (f32::from(base[offset + component]) * (1.0 - alpha)
                + f32::from(overlay[offset + component]) * alpha)
                .round() as i16;
            let canonical = i16::from(cpu[offset + component]);
            assert!(
                (folded - canonical).abs() <= 2,
                "component {component}: folded={folded}, canonical={canonical}"
            );
        }
    }

    #[test]
    fn organism_marker_color_position_and_pick_threshold_are_shared() {
        let map = settled_map();
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(1, 8);
        let cell = REGION_SIZE / 8.0;
        let (west, north) = composer.view_origin(player);
        let organism = map
            .organisms()
            .find(|organism| {
                let px = ((organism.world_pos.0 - west) / cell) as i64;
                let py = ((north - organism.world_pos.1) / cell) as i64;
                (0..24).contains(&px)
                    && (0..24).contains(&py)
                    && (px - 12).abs() > 4
                    && (py - 12).abs() > 4
            })
            .expect("settled fixture has a visible marker away from the player");
        let overlays = Overlays {
            organisms: true,
            ..no_overlays()
        };
        let pixels = composer
            .compose(
                &map,
                player,
                Channel::Composite,
                overlays,
                &[],
                &MapDecor::default(),
            )
            .to_vec();
        let px = ((organism.world_pos.0 - west) / cell) as usize;
        let py = ((north - organism.world_pos.1) / cell) as usize;
        let offset = (py * 24 + px) * 4;
        assert_eq!(
            &pixels[offset..offset + 3],
            &expressed_color(&organism.expressed)
        );
        assert!(pick_organism(&map, organism.world_pos, ORGANISM_PICK_ZOOM - 1).is_none());
        assert_eq!(
            pick_organism(&map, organism.world_pos, ORGANISM_PICK_ZOOM).map(|picked| picked.id),
            Some(organism.id)
        );
    }

    #[test]
    fn prepared_packet_selects_cpu_when_requested_or_gpu_unavailable() {
        let map = settled_map();
        let decor = MapDecor::default();
        let mut composer = MapComposer::new(1, 8);
        composer.set_zoom(4);
        composer.set_grid_thickness_cells(1.25);
        let mut atlas = AtlasManager::default();
        let base = MapRenderRequest {
            map: &map,
            player: (0.0, 0.0),
            channel: Channel::Composite,
            overlays: Overlays::default(),
            anchors: &[],
            decor: &decor,
            requested_backend: MapBackend::Cpu,
            gpu_available: true,
            refinement: RefinementRequest::default(),
            dirty_key: 0xA71A_5000_0000_0004,
        };
        {
            let packet = composer.prepare_render(&mut atlas, base);
            assert_eq!(packet.projection.side, 24);
            assert_eq!(packet.projection.zoom, 4);
            assert_eq!(packet.projection.grid_thickness_cells, 1.25);
            assert_eq!(packet.channel, Channel::Composite);
            assert_eq!(packet.requested_backend, MapBackend::Cpu);
            assert_eq!(packet.backend, MapBackend::Cpu);
            assert_eq!(packet.fallback, None);
            assert_eq!(packet.dirty_key, 0xA71A_5000_0000_0004);
            let rgba = packet.cpu_rgba().expect("CPU request has CPU pixels");
            assert_eq!(rgba.len(), 24 * 24 * 4);
            assert_eq!(packet.pixel_hash, map_pixel_hash(rgba));
            assert!(packet.gpu().is_none());
        }

        let packet = composer.prepare_render(
            &mut atlas,
            MapRenderRequest {
                requested_backend: MapBackend::GpuAtlas,
                gpu_available: false,
                ..base
            },
        );
        assert_eq!(packet.backend, MapBackend::Cpu);
        assert_eq!(packet.fallback, Some(MapBackendFallback::GpuUnavailable));
        assert!(packet.cpu_rgba().is_some());
    }

    #[test]
    fn prepared_gpu_packet_reuses_slots_uploads_and_pixel_backing() {
        let map = settled_map();
        let decor = MapDecor::default();
        let mut composer = MapComposer::new(2, 8);
        composer.set_zoom(2);
        let mut atlas = AtlasManager::default();
        let request = MapRenderRequest {
            map: &map,
            player: (0.0, 0.0),
            channel: Channel::Composite,
            overlays: Overlays::default(),
            anchors: &[],
            decor: &decor,
            requested_backend: MapBackend::GpuAtlas,
            gpu_available: true,
            refinement: RefinementRequest {
                enabled: true,
                octave_count: 3,
            },
            dirty_key: 7,
        };

        let (first_slots, first_overlay_hash, first_overlay_ptr) = {
            let packet = composer.prepare_render(&mut atlas, request);
            assert_eq!(packet.backend, MapBackend::GpuAtlas);
            assert_eq!(packet.fallback, None);
            assert!(packet.cpu_rgba().is_none());
            let gpu = packet.gpu().expect("supported channel prepares atlas work");
            assert_eq!(gpu.params.channel, 0);
            assert_eq!(gpu.params.zoom, 2);
            assert_eq!(gpu.params.refine_count, 3);
            assert!(!gpu.uploads.is_empty(), "first atlas sync uploads tiles");
            assert!(gpu.slots.iter().any(|slot| *slot >= 0));
            assert_eq!(gpu.overlay_hash, map_pixel_hash(gpu.overlay_rgba));
            assert_eq!(packet.pixel_hash, gpu.overlay_hash);
            (
                gpu.slots.clone(),
                gpu.overlay_hash,
                gpu.overlay_rgba.as_ptr(),
            )
        };

        let packet = composer.prepare_render(&mut atlas, request);
        let gpu = packet.gpu().expect("steady frame remains on GPU");
        assert_eq!(gpu.slots, first_slots);
        assert!(gpu.uploads.is_empty(), "steady atlas has zero uploads");
        assert_eq!(gpu.overlay_hash, first_overlay_hash);
        assert_eq!(gpu.overlay_rgba.as_ptr(), first_overlay_ptr);
    }

    #[test]
    fn unsupported_gpu_channel_falls_back_without_consuming_atlas_deltas() {
        let map = settled_map();
        let decor = MapDecor::default();
        let mut composer = MapComposer::new(1, 8);
        let mut atlas = AtlasManager::default();
        let request = MapRenderRequest {
            map: &map,
            player: (0.0, 0.0),
            channel: Channel::Geology,
            overlays: no_overlays(),
            anchors: &[],
            decor: &decor,
            requested_backend: MapBackend::GpuAtlas,
            gpu_available: true,
            refinement: RefinementRequest::default(),
            dirty_key: 11,
        };
        {
            let packet = composer.prepare_render(&mut atlas, request);
            assert_eq!(packet.backend, MapBackend::Cpu);
            assert_eq!(
                packet.fallback,
                Some(MapBackendFallback::UnsupportedChannel(Channel::Geology))
            );
            assert!(packet.cpu_rgba().is_some());
        }

        let packet = composer.prepare_render(
            &mut atlas,
            MapRenderRequest {
                channel: Channel::Elevation,
                ..request
            },
        );
        assert!(
            !packet
                .gpu()
                .expect("elevation is GPU-capable")
                .uploads
                .is_empty(),
            "CPU fallback must not consume the atlas's first delta upload"
        );
    }

    #[test]
    fn presentation_pixel_hash_and_cpu_backing_are_stable() {
        assert_eq!(map_pixel_hash(&[]), 0x0DDB_1A5E_D0F0_0006);
        assert_eq!(map_pixel_hash(b"map-pixels-v1"), 0xCCE1_E271_3BDB_F6F6);

        let map = settled_map();
        let decor = MapDecor::default();
        let mut composer = MapComposer::new(1, 8);
        let mut atlas = AtlasManager::default();
        let request = MapRenderRequest {
            map: &map,
            player: (0.0, 0.0),
            channel: Channel::Composite,
            overlays: no_overlays(),
            anchors: &[],
            decor: &decor,
            requested_backend: MapBackend::Cpu,
            gpu_available: false,
            refinement: RefinementRequest::default(),
            dirty_key: 13,
        };
        let (first_hash, first_ptr) = {
            let packet = composer.prepare_render(&mut atlas, request);
            (packet.pixel_hash, packet.cpu_rgba().unwrap().as_ptr())
        };
        let packet = composer.prepare_render(&mut atlas, request);
        assert_eq!(packet.pixel_hash, first_hash);
        assert_eq!(packet.cpu_rgba().unwrap().as_ptr(), first_ptr);

        composer.set_grid_thickness_cells(f32::NAN);
        assert_eq!(composer.projection().grid_thickness_cells, 1.0);
    }

    fn rgba_digest(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
        }
        hex
    }

    fn compose_digest(map: &RegionMap, overlays: Overlays, decor: &MapDecor) -> String {
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(1, 8);
        rgba_digest(composer.compose(
            map,
            player,
            Channel::Composite,
            overlays,
            &[fixture_anchor()],
            decor,
        ))
    }

    fn compose_channel_digest(map: &RegionMap, channel: Channel) -> String {
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(1, 8);
        rgba_digest(composer.compose(
            map,
            player,
            channel,
            no_overlays(),
            &[fixture_anchor()],
            &MapDecor::default(),
        ))
    }

    fn compose_after_detected_pinned_change(
        map: &mut RegionMap,
        overlays: Overlays,
        decor: &MapDecor,
    ) -> String {
        let player = (0.0, 0.0);
        let center = RegionCoord::from_world(player.0, player.1);
        let mut composer = MapComposer::new(1, 8);
        let baseline = composer.update_for_tick(1, map);
        assert!(baseline.advanced);
        composer.compose(
            map,
            player,
            Channel::Composite,
            no_overlays(),
            &[fixture_anchor()],
            &MapDecor::default(),
        );
        assert_eq!(composer.pinned_violations, 0);

        let before = map.get(center).expect("settled center");
        assert_eq!(before.stability, 1.0);
        let before_revision = before.revision;
        let mut signature = world_core::PossibilitySignature::of(before.current);
        let bucket = &mut signature.buckets[PossibilityDomain::Aesthetics.index()];
        *bucket = if *bucket == 0 {
            world_core::POSSIBILITY_QUANT - 1
        } else {
            0
        };
        map.apply_preserve_contribution(0xA11E_0000_0000_0001, center, signature);
        let after = map.get(center).expect("preserved center stays resident");
        assert_eq!(after.stability, 1.0);
        assert_eq!(after.revision, before_revision + 1);

        let changed = composer.update_for_tick(2, map);
        assert_eq!(changed.new_pinned_violations, 1);

        let digest = rgba_digest(composer.compose(
            map,
            player,
            Channel::Composite,
            overlays,
            &[fixture_anchor()],
            decor,
        ));
        assert_eq!(composer.pinned_violations, 1);
        let unchanged = rgba_digest(composer.compose(
            map,
            player,
            Channel::Composite,
            overlays,
            &[fixture_anchor()],
            decor,
        ));
        assert_eq!(composer.pinned_violations, 1);
        assert_eq!(
            digest, unchanged,
            "an unchanged frame must not double-count"
        );
        digest
    }

    /// Milestone 0 extraction guard for the complete native CPU presentation.
    ///
    /// GPU pixels are deliberately absent (ADR 0017). The fixture pins SHA-256
    /// digests of the complete RGBA byte streams for all native channels and
    /// for each overlay/decor layer, including an organism-bearing settled
    /// window. Later moves to `viewer-host` must reproduce these bytes unless a
    /// separately identified layout/grid change intentionally supersedes one.
    #[test]
    fn native_cpu_map_characterization() {
        let map = settled_map();
        assert!(
            map.organism_count() > 0,
            "the characterization window must exercise realized organisms"
        );
        let player = (0.0, 0.0);
        let anchors = [fixture_anchor()];
        let mut actual = String::from("native-cpu-map-characterization-v1\n");
        writeln!(&mut actual, "rgba8 24 24 2304").unwrap();
        writeln!(&mut actual, "organisms {}", map.organism_count()).unwrap();

        for channel in Channel::ALL {
            writeln!(
                &mut actual,
                "channel {:<12} {}",
                channel.name(),
                compose_channel_digest(&map, channel)
            )
            .unwrap();
        }

        // A fully settled default field legitimately makes both diagnostic
        // channels black. Record active examples too, so their extraction is
        // protected by visible bytes rather than only by an enum dispatch.
        let mut pinned = no_overlays();
        pinned.pinned_flash = true;
        let mut revision_map = settled_map();
        let pinned_digest =
            compose_after_detected_pinned_change(&mut revision_map, pinned, &MapDecor::default());
        let center = RegionCoord::from_world(player.0, player.1);
        let before = revision_map.get(center).expect("preserved center");
        let mut second_signature = world_core::PossibilitySignature::of(before.current);
        let bucket = &mut second_signature.buckets[PossibilityDomain::Behavior.index()];
        *bucket = if *bucket == 0 {
            world_core::POSSIBILITY_QUANT - 1
        } else {
            0
        };
        let before_revision = before.revision;
        revision_map.apply_preserve_contribution(0xA11E_0000_0000_0001, center, second_signature);
        assert_eq!(
            revision_map.get(center).expect("preserved center").revision,
            before_revision + 1
        );
        let active_revision = compose_channel_digest(&revision_map, Channel::Revision);
        assert_ne!(
            active_revision,
            compose_channel_digest(&map, Channel::Revision),
            "active revision fixture must contain visible churn"
        );
        writeln!(&mut actual, "channel revision+    {active_revision}").unwrap();

        let mut residual_map = settled_map();
        residual_map.update(
            player,
            0.0,
            &PossibilityField::default(),
            &anchors,
            &[0.0; POSSIBILITY_DIMS],
            &Budget::unlimited(),
            &InlineExecutor,
            false,
        );
        let residual_state = residual_map.get(center).expect("retargeted center");
        assert!(
            residual_state
                .current
                .dims
                .iter()
                .zip(residual_state.target.dims)
                .any(|(current, target)| current.to_bits() != target.to_bits()),
            "active residual fixture must not already be converged"
        );
        let active_residual = compose_channel_digest(&residual_map, Channel::Residual);
        assert_ne!(
            active_residual,
            compose_channel_digest(&map, Channel::Residual),
            "active residual fixture must contain visible error"
        );
        writeln!(&mut actual, "channel residual+    {active_residual}").unwrap();

        let mut player_composer = MapComposer::new(1, 8);
        let player_pixels = player_composer
            .compose(
                &map,
                player,
                Channel::Composite,
                no_overlays(),
                &anchors,
                &MapDecor::default(),
            )
            .to_vec();
        let base = rgba_digest(&player_pixels);
        writeln!(&mut actual, "overlay player       {base}").unwrap();
        let player_px = (16 * 24 + 8) * 4;
        assert_eq!(&player_pixels[player_px..player_px + 3], &[255, 40, 40]);
        assert_eq!(
            &player_pixels[player_px + 4..player_px + 7],
            &[255, 255, 255]
        );

        let mut grid = no_overlays();
        grid.grid = true;
        let grid_digest = compose_digest(&map, grid, &MapDecor::default());
        assert_ne!(grid_digest, base, "grid fixture must alter visible pixels");
        writeln!(&mut actual, "overlay grid         {}", grid_digest).unwrap();

        let mut rings = no_overlays();
        rings.rings = true;
        let rings_digest = compose_digest(&map, rings, &MapDecor::default());
        assert_ne!(rings_digest, base, "ring fixture must alter visible pixels");
        writeln!(&mut actual, "overlay rings        {}", rings_digest).unwrap();

        assert_ne!(
            pinned_digest, base,
            "pinned flash must alter visible pixels"
        );
        writeln!(&mut actual, "overlay pinned-flash {}", pinned_digest).unwrap();

        let mut organisms = no_overlays();
        organisms.organisms = true;
        let organism_digest = compose_digest(&map, organisms, &MapDecor::default());
        assert_ne!(
            organism_digest, base,
            "organism markers must alter the fixture"
        );
        writeln!(&mut actual, "overlay organisms    {organism_digest}").unwrap();

        let center = RegionCoord::from_world(player.0, player.1);
        let discovered_decor = MapDecor {
            seen: Some([center].into_iter().collect()),
            ..MapDecor::default()
        };
        let mut discovered = no_overlays();
        discovered.discovered = true;
        let discovered_digest = compose_digest(&map, discovered, &discovered_decor);
        assert_ne!(
            discovered_digest, base,
            "discovery dimming fixture must alter visible pixels"
        );
        writeln!(&mut actual, "overlay discovered   {}", discovered_digest).unwrap();

        let route_decor = MapDecor {
            routes: vec![(vec![(-700.0, 700.0), (300.0, 850.0), (900.0, -500.0)], 7)],
            ..MapDecor::default()
        };
        let route_digest = compose_digest(&map, no_overlays(), &route_decor);
        assert_ne!(
            route_digest, base,
            "route fixture must alter visible pixels"
        );
        writeln!(&mut actual, "overlay route        {}", route_digest).unwrap();

        let preserve_decor = MapDecor {
            preserves: [RegionCoord::new(-1, 1)].into_iter().collect(),
            ..MapDecor::default()
        };
        let preserve_digest = compose_digest(&map, no_overlays(), &preserve_decor);
        assert_ne!(
            preserve_digest, base,
            "preserve fixture must alter visible pixels"
        );
        writeln!(&mut actual, "overlay preserve     {}", preserve_digest).unwrap();

        let combined_decor = MapDecor {
            seen: discovered_decor.seen,
            preserves: preserve_decor.preserves,
            routes: route_decor.routes,
        };
        let mut combined_map = settled_map();
        let combined_digest = compose_after_detected_pinned_change(
            &mut combined_map,
            Overlays::default(),
            &combined_decor,
        );
        assert_ne!(combined_digest, base);
        writeln!(&mut actual, "overlay combined     {}", combined_digest).unwrap();

        assert_eq!(
            actual.trim_end(),
            include_str!("../tests/fixtures/native_map_characterization.txt").trim_end()
        );
    }

    #[test]
    fn composite_paint_matches_the_pre_hoist_logic() {
        // Pins the `composite_cell_color` hoist (3d-phase-1-plan.md §6.4):
        // the refactored `paint_region` must be byte-identical to the old
        // inline logic, replicated here verbatim, for a settled window.
        let map = settled_map();
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(1, 8);
        let overlays = Overlays {
            grid: false,
            rings: false,
            pinned_flash: false,
            organisms: false,
            discovered: false,
        };
        composer.compose(
            &map,
            player,
            Channel::Composite,
            overlays,
            &[],
            &MapDecor::default(),
        );
        // The player cross is drawn on top; skip its pixels.
        let side = composer.side() as usize;
        let cell = REGION_SIZE / 8.0;
        let (west, north) = composer.view_origin(player);
        let ppx = ((player.0 - west) / cell) as i64;
        let ppy = ((north - player.1) / cell) as i64;
        let on_marker = |px: i64, py: i64| {
            (py == ppy && (px - ppx).abs() <= 3) || (px == ppx && (py - ppy).abs() <= 3)
        };

        let center = RegionCoord::from_world(player.0, player.1);
        let res = 8u16;
        let mut checked = 0u32;
        for row_region in 0..3i32 {
            let ry = center.y + 1 - row_region;
            for col_region in 0..3i32 {
                let rx = center.x - 1 + col_region;
                let coord = RegionCoord::new(rx, ry);
                let tiles = map.cache().get(coord).expect("settled window");
                let elevation = tiles.channels[CHANNEL_ELEVATION].as_ref().expect("tile");
                let river = tiles.channels[CHANNEL_RIVER].as_ref().expect("tile");
                let wetness = tiles.channels[CHANNEL_WETNESS].as_ref().expect("tile");
                let biome = tiles.biome.as_ref().expect("tile");
                for cy in 0..res {
                    for cx in 0..res {
                        // The pre-hoist Composite arm, verbatim.
                        let elev = elevation.get(cx, cy);
                        let mut expected = composite_color(
                            elev,
                            Biome::from_id(biome.get(cx, cy)),
                            river.get(cx, cy),
                            wetness.get(cx, cy),
                        );
                        if elev >= SEA_LEVEL {
                            if let Some(id) = map.dominant_species_id(coord, cx, cy) {
                                expected = lerp_rgb(expected, species_color(id), 0.18);
                            }
                        }
                        let px = col_region as usize * 8 + cx as usize;
                        let py = row_region as usize * 8 + (7 - cy) as usize;
                        if on_marker(px as i64, py as i64) {
                            continue;
                        }
                        let offset = (py * side + px) * 4;
                        assert_eq!(
                            &composer.pixels[offset..offset + 3],
                            &expected,
                            "pixel ({px}, {py}) diverged from the pre-hoist logic"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 500, "the sweep must cover the window");
    }

    #[test]
    fn zoom_preserves_the_view_center() {
        // The magnification is about the image center, so the world position
        // under the center pixel must not move as the zoom changes.
        let player = (300.0, -10.0);
        let mut composer = MapComposer::new(3, 16);
        let center = f64::from(composer.side()) / 2.0;
        let base = composer
            .pixel_to_world(player, center, center)
            .expect("center is inside the map");
        for zoom in [2, 4, 8, 16] {
            composer.set_zoom(zoom);
            let zoomed = composer
                .pixel_to_world(player, center, center)
                .expect("center is inside the map");
            assert!(
                (zoomed.0 - base.0).abs() < 1e-9 && (zoomed.1 - base.1).abs() < 1e-9,
                "center moved at zoom x{zoom}: {base:?} -> {zoomed:?}"
            );
        }
    }

    #[test]
    fn zoomed_picking_shrinks_the_world_span() {
        // Pixel-to-world across the full image must cover exactly 1/zoom of
        // the base world extent (the inverse of the magnify step).
        let player = (0.0, 0.0);
        let mut composer = MapComposer::new(3, 16);
        let side = f64::from(composer.side());
        let span = |c: &MapComposer| {
            let w0 = c.pixel_to_world(player, 0.0, 0.0).unwrap();
            let w1 = c.pixel_to_world(player, side - 1.0, 0.0).unwrap();
            w1.0 - w0.0
        };
        let base = span(&composer);
        composer.set_zoom(4);
        let zoomed = span(&composer);
        assert!(
            (zoomed - base / 4.0).abs() < 1e-6,
            "zoom x4 span {zoomed} != base {base} / 4"
        );
    }

    #[test]
    fn magnify_blows_up_the_center_block() {
        // Paint a distinct color per base pixel, magnify, and check output
        // pixels sample the base pixel the pixel_to_world inverse names.
        let mut composer = MapComposer::new(1, 4);
        let side = composer.side() as usize;
        for i in 0..side * side {
            let v = (i % 251) as u8;
            composer.pixels[i * 4..i * 4 + 4].copy_from_slice(&[v, v.wrapping_add(1), 0, 255]);
        }
        let before = composer.pixels.clone();
        composer.set_zoom(2);
        composer.magnify();
        let zoom = 2.0;
        let center = side as f64 / 2.0;
        for oy in 0..side {
            for ox in 0..side {
                let src = |i: usize| {
                    let s = (i as f64 + 0.5 - center) / zoom + center;
                    (s.max(0.0) as usize).min(side - 1)
                };
                let (sx, sy) = (src(ox), src(oy));
                assert_eq!(
                    composer.pixels[(oy * side + ox) * 4..(oy * side + ox) * 4 + 4],
                    before[(sy * side + sx) * 4..(sy * side + sx) * 4 + 4],
                    "output ({ox},{oy}) should show base ({sx},{sy})"
                );
            }
        }
    }
}
