//! Aggregate vegetation: density and canopy height (phase-2-plan.md §7.7,
//! milestone M6). Replaces Phase 1's single-scalar `ecology` module.
//!
//! Each biome contributes base density and canopy ranges; density scales with
//! fertility, moisture, and the (dequantized) Ecology bucket; canopy needs
//! soil depth and shelters below the temperature window. The section 8
//! plausibility rules — canopy vs soil depth, vegetation vs rainfall — are
//! kept as code here, mirroring what [`crate::anchor::project_plausible`]
//! enforces at the possibility level. Organisms, species, and food webs are
//! Phase 3; these stay small scalar fields.

use crate::biome::Biome;
use crate::climate::Climate;
use crate::soils::Soils;

/// Per-sample aggregate vegetation. Presentation math, never identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vegetation {
    /// Aggregate density in `[0, 1]`.
    pub density: f32,
    /// Canopy height in world units (meters, informally); 0 = no canopy.
    pub canopy_height: f32,
}

/// Temperature (°C) at which canopy growth peaks.
const CANOPY_TEMPERATURE: f32 = 16.0;

/// Distance from the optimum (°C) at which canopy growth reaches zero.
const CANOPY_TOLERANCE: f32 = 28.0;

/// Soil depth at which canopy reaches its full biome-base height.
const CANOPY_FULL_SOIL: f32 = 0.5;

/// Base density and maximum canopy height per biome.
#[must_use]
pub const fn biome_base(biome: Biome) -> (f32, f32) {
    match biome {
        Biome::Ocean | Biome::Ice => (0.0, 0.0),
        Biome::River => (0.1, 0.0),
        Biome::Wetland => (0.55, 6.0),
        Biome::Desert => (0.05, 1.0),
        Biome::Grassland => (0.35, 1.5),
        Biome::Shrubland => (0.30, 3.0),
        Biome::TemperateForest => (0.75, 25.0),
        Biome::Rainforest => (0.95, 35.0),
        Biome::Taiga => (0.60, 15.0),
        Biome::Tundra => (0.15, 0.5),
        Biome::Bare => (0.02, 0.3),
    }
}

/// Aggregate vegetation at a sample.
///
/// `p_ecology` is the dequantized Ecology bucket — the one domain this layer
/// reads directly. Density is capped so vegetation never materially exceeds
/// available rainfall; canopy is capped by soil depth (the plausibility rules
/// of implementation-plan.md section 8).
#[must_use]
pub fn vegetation(biome: Biome, c: &Climate, s: &Soils, p_ecology: f32) -> Vegetation {
    let (base_density, base_canopy) = biome_base(biome);
    let density = (base_density * (0.4 + 0.6 * s.fertility) * (0.5 + p_ecology))
        .min(c.moisture + 0.1)
        .clamp(0.0, 1.0);

    // Canopy needs soil to root in and a survivable temperature window.
    let t = (c.temperature - CANOPY_TEMPERATURE) / CANOPY_TOLERANCE;
    let warmth = (1.0 - t * t).max(0.0);
    let rooting = (s.depth / CANOPY_FULL_SOIL).clamp(0.0, 1.0);
    let canopy_height = base_canopy * rooting * warmth * (0.5 + 0.5 * density);

    Vegetation {
        density,
        canopy_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temperate() -> (Climate, Soils) {
        (
            Climate {
                temperature: 15.0,
                moisture: 0.6,
            },
            Soils {
                depth: 0.7,
                fertility: 0.6,
            },
        )
    }

    #[test]
    fn water_and_ice_are_barren() {
        let (c, s) = temperate();
        for biome in [Biome::Ocean, Biome::Ice] {
            let v = vegetation(biome, &c, &s, 1.0);
            assert_eq!(v.density, 0.0);
            assert_eq!(v.canopy_height, 0.0);
        }
    }

    #[test]
    fn ecology_bucket_drives_density() {
        let (c, s) = temperate();
        let lush = vegetation(Biome::TemperateForest, &c, &s, 1.0);
        let sparse = vegetation(Biome::TemperateForest, &c, &s, 0.0);
        assert!(lush.density > sparse.density);
        assert!((0.0..=1.0).contains(&lush.density));
    }

    #[test]
    fn canopy_respects_the_soil_depth_rule() {
        // The milestone M6 exit criterion (phase-2-plan.md §16).
        let (c, _) = temperate();
        let deep = Soils {
            depth: 0.8,
            fertility: 0.6,
        };
        let thin = Soils {
            depth: 0.1,
            fertility: 0.6,
        };
        let none = Soils {
            depth: 0.0,
            fertility: 0.6,
        };
        let tall = vegetation(Biome::TemperateForest, &c, &deep, 0.5);
        let short = vegetation(Biome::TemperateForest, &c, &thin, 0.5);
        let bare = vegetation(Biome::TemperateForest, &c, &none, 0.5);
        assert!(tall.canopy_height > short.canopy_height);
        assert_eq!(bare.canopy_height, 0.0);
    }

    #[test]
    fn vegetation_never_far_exceeds_moisture() {
        let dry = Climate {
            temperature: 20.0,
            moisture: 0.1,
        };
        let s = Soils {
            depth: 1.0,
            fertility: 1.0,
        };
        let v = vegetation(Biome::Rainforest, &dry, &s, 1.0);
        assert!(v.density <= dry.moisture + 0.1);
    }
}
