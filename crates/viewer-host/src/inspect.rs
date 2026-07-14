//! Semantic inspection values shared by map and CPU-side POV picking.

use world_core::{LocalPos, RegionCoord};
use world_runtime::GenerationStatus;

/// Streaming/generation state reported for an inspected cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellStatus {
    /// The region is outside the resident set.
    NotResident,
    /// Authoritative state exists without an admitted field working set.
    Unloaded,
    /// One or more field layers are pending.
    Generating,
    /// Required field layers are current.
    Ready,
}

impl From<Option<GenerationStatus>> for CellStatus {
    fn from(status: Option<GenerationStatus>) -> Self {
        match status {
            None => Self::NotResident,
            Some(GenerationStatus::Unloaded) => Self::Unloaded,
            Some(GenerationStatus::Generating) => Self::Generating,
            Some(GenerationStatus::Ready) => Self::Ready,
        }
    }
}

/// Aggregate ecology facts at an inspected cell.
#[derive(Debug, Clone, PartialEq)]
pub struct EcologyInfo {
    /// Species in the habitat roster.
    pub roster_size: usize,
    /// Dominant species id.
    pub dominant_id: u64,
    /// Producer/herbivore/omnivore/carnivore/decomposer counts.
    pub trophic_counts: [usize; 5],
    /// Aggregate herbivore pressure.
    pub herbivore: f32,
    /// Aggregate predator pressure.
    pub predator: f32,
    /// Aggregate species diversity.
    pub diversity: f32,
}

/// Native cell-sampling contract retained during the staged extraction.
/// Milestone 6 replaces this display-oriented shape with [`CellInfo`] after
/// both shells consume the same sampler and owned low-rate model.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorInfo {
    /// Continuous world position sampled.
    pub world: (f64, f64),
    /// Region coordinate containing the sample.
    pub region: (i32, i32),
    /// Streaming stability.
    pub stability: f32,
    /// Realized-state revision.
    pub revision: u32,
    /// Existing native generation status label.
    pub status: &'static str,
    /// Elevation.
    pub elevation: Option<f32>,
    /// Temperature.
    pub temperature: Option<f32>,
    /// Moisture.
    pub moisture: Option<f32>,
    /// Rock hardness.
    pub hardness: Option<f32>,
    /// River expression.
    pub river: Option<f32>,
    /// Surface wetness.
    pub wetness: Option<f32>,
    /// Soil depth.
    pub soil_depth: Option<f32>,
    /// Soil fertility.
    pub fertility: Option<f32>,
    /// Vegetation density.
    pub vegetation: Option<f32>,
    /// Canopy height.
    pub canopy: Option<f32>,
    /// Existing native biome display name.
    pub biome: Option<&'static str>,
    /// Aggregate ecology, when generated.
    pub ecology: Option<EcologyInfo>,
}

/// Terrain, climate, soil, biome, and ecology data for one cell.
#[derive(Debug, Clone, PartialEq)]
pub struct CellInfo {
    /// Continuous world position sampled.
    pub world: (f64, f64),
    /// Region containing the sample.
    pub region: RegionCoord,
    /// Quantized cell within the region.
    pub cell: LocalPos,
    /// Region pipeline state.
    pub status: CellStatus,
    /// Streaming stability.
    pub stability: f32,
    /// Realized-state revision.
    pub revision: u32,
    /// Elevation.
    pub elevation: Option<f32>,
    /// Temperature.
    pub temperature: Option<f32>,
    /// Moisture.
    pub moisture: Option<f32>,
    /// Rock hardness.
    pub hardness: Option<f32>,
    /// River expression.
    pub river: Option<f32>,
    /// Surface wetness.
    pub wetness: Option<f32>,
    /// Soil depth.
    pub soil_depth: Option<f32>,
    /// Soil fertility.
    pub fertility: Option<f32>,
    /// Vegetation density.
    pub vegetation: Option<f32>,
    /// Canopy height.
    pub canopy: Option<f32>,
    /// Stable biome display name, when generated.
    pub biome: Option<&'static str>,
    /// Aggregate ecology, when generated.
    pub ecology: Option<EcologyInfo>,
}

/// A realized presentation organism under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrganismInfo {
    /// Stable instance identity.
    pub id: u64,
    /// Species identity.
    pub species: u64,
    /// Existing native trophic-role label.
    pub trophic: &'static str,
    /// Jittered XY world position.
    pub world: (f64, f64),
    /// Expressed hue.
    pub hue: f32,
    /// Expressed bioluminance.
    pub luminance: f32,
    /// Expressed body size.
    pub size: f32,
    /// Expressed activity.
    pub activity: f32,
    /// Expressed aggression.
    pub aggression: f32,
}

/// Nearest visible semantic object under a pointer.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverInfo {
    /// Pointer is outside a pane or over sky/missing geometry.
    None,
    /// Terrain/cell information.
    Terrain(CellInfo),
    /// Realized organism information.
    Organism(OrganismInfo),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_generation_states_map_without_string_matching() {
        assert_eq!(CellStatus::from(None), CellStatus::NotResident);
        assert_eq!(
            CellStatus::from(Some(GenerationStatus::Generating)),
            CellStatus::Generating
        );
        assert_eq!(
            CellStatus::from(Some(GenerationStatus::Ready)),
            CellStatus::Ready
        );
    }
}
