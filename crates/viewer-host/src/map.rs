//! Shared top-down map presentation values (`native-web-alignment.md`
//! section 5.5). Composition behavior moves here in Milestone 4.

use std::collections::BTreeSet;

use world_core::RegionCoord;

/// Which scalar or categorical field the map paints.
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
        match self {
            Self::Composite => "composite",
            Self::Elevation => "elevation",
            Self::Geology => "geology",
            Self::Temperature => "temperature",
            Self::Moisture => "moisture",
            Self::River => "river",
            Self::Wetness => "wetness",
            Self::Soil => "soil",
            Self::Biome => "biome",
            Self::Vegetation => "vegetation",
            Self::Herbivore => "herbivore",
            Self::Predator => "predator",
            Self::Diversity => "diversity",
            Self::DominantSpecies => "dominant",
            Self::Influence => "influence",
            Self::Stability => "stability",
            Self::Revision => "revision",
            Self::Residual => "residual",
        }
    }

    /// Parse an exact stable channel id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|channel| channel.id() == id)
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

impl MapOverlay {
    /// Stable overlay order.
    pub const ALL: [Self; 5] = [
        Self::Grid,
        Self::Rings,
        Self::PinnedFlash,
        Self::Organisms,
        Self::Discovered,
    ];
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

/// Map rendering path selected by the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MapBackend {
    /// Canonical CPU raster.
    Cpu,
    /// Derived WebGPU atlas composition.
    GpuAtlas,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_ids_round_trip_and_cycle_once() {
        for (index, channel) in Channel::ALL.into_iter().enumerate() {
            assert_eq!(Channel::from_id(channel.id()), Some(channel));
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
        for overlay in MapOverlay::ALL {
            assert!(overlays.enabled(overlay));
            overlays.set(overlay, false);
            assert!(!overlays.enabled(overlay));
        }
    }
}
