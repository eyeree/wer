//! Biome classification (phase-2-plan.md §7.6, milestone M6).
//!
//! A Whittaker-style temperature × moisture lookup with ordered priority
//! overrides. Biome ids are **derived presentation** in Phase 2: the
//! thresholds compare `f32`s, so knife-edge cells may classify differently
//! across platforms. If Phase 3 wants identity-grade biomes (for species
//! hashing), classification inputs get quantized first — noted now so it is a
//! decision, not an accident (phase-2-plan.md §7.6).

use crate::climate::Climate;
use crate::hydrology::Hydrology;
use crate::soils::Soils;
use crate::terrain::SEA_LEVEL;

/// Biome classes. The discriminants are the `u8` ids stored in biome field
/// tiles; the ordering is part of the (presentation-grade) palette contract.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Biome {
    /// Below sea level.
    Ocean = 0,
    /// A strong drainage channel.
    River = 1,
    /// Saturated ground.
    Wetland = 2,
    /// Hot or cold drylands.
    Desert = 3,
    /// Open grass plains.
    Grassland = 4,
    /// Dry scrub or shallow-soil demoted forest.
    Shrubland = 5,
    /// Broadleaf/mixed forest.
    TemperateForest = 6,
    /// Hot, wet forest.
    Rainforest = 7,
    /// Cold conifer forest.
    Taiga = 8,
    /// Cold, treeless.
    Tundra = 9,
    /// Rock above the vegetation line.
    Bare = 10,
    /// Permanent ice.
    Ice = 11,
}

/// Number of biome classes.
pub const BIOME_COUNT: usize = 12;

/// All biomes, indexable by id.
pub const BIOMES: [Biome; BIOME_COUNT] = [
    Biome::Ocean,
    Biome::River,
    Biome::Wetland,
    Biome::Desert,
    Biome::Grassland,
    Biome::Shrubland,
    Biome::TemperateForest,
    Biome::Rainforest,
    Biome::Taiga,
    Biome::Tundra,
    Biome::Bare,
    Biome::Ice,
];

impl Biome {
    /// The tile-stored id of this biome.
    #[inline]
    #[must_use]
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// Biome for a tile-stored id (ids outside the table fall back to
    /// [`Biome::Bare`] — defensive, since tiles are trusted run-local state).
    #[inline]
    #[must_use]
    pub const fn from_id(id: u8) -> Self {
        if (id as usize) < BIOME_COUNT {
            BIOMES[id as usize]
        } else {
            Biome::Bare
        }
    }

    /// Display name for tools and the debug panel.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "ocean",
            Biome::River => "river",
            Biome::Wetland => "wetland",
            Biome::Desert => "desert",
            Biome::Grassland => "grassland",
            Biome::Shrubland => "shrubland",
            Biome::TemperateForest => "temperate forest",
            Biome::Rainforest => "rainforest",
            Biome::Taiga => "taiga",
            Biome::Tundra => "tundra",
            Biome::Bare => "bare rock",
            Biome::Ice => "ice",
        }
    }
}

/// River strength at or above which a cell classifies as [`Biome::River`].
pub const RIVER_BIOME_THRESHOLD: f32 = 0.5;

/// Wetness at or above which land classifies as [`Biome::Wetland`] (hydrology
/// already folds slope into wetness, so this single threshold covers the
/// "wetness + low slope" rule).
pub const WETLAND_THRESHOLD: f32 = 0.78;

/// Elevation above which land is bare rock regardless of climate.
pub const BARE_ELEVATION: f32 = 850.0;

/// Soil depth below which forest demotes to shrubland.
pub const FOREST_SOIL_FLOOR: f32 = 0.2;

/// Classify one sample. Override order (each rule yields to the ones above):
/// water → ice → river → wetland → tundra → altitude bare → Whittaker lookup,
/// with shallow soil demoting forest to shrub (phase-2-plan.md §7.6). The
/// rule order is part of the deterministic contract (golden-fixtured).
#[must_use]
pub fn classify(elevation: f32, c: &Climate, h: &Hydrology, s: &Soils) -> Biome {
    if elevation < SEA_LEVEL {
        return Biome::Ocean;
    }
    if c.temperature < -10.0 {
        return Biome::Ice;
    }
    if h.river >= RIVER_BIOME_THRESHOLD {
        return Biome::River;
    }
    if h.wetness >= WETLAND_THRESHOLD {
        return Biome::Wetland;
    }
    if c.temperature < -2.0 {
        return Biome::Tundra;
    }
    if elevation > BARE_ELEVATION {
        return Biome::Bare;
    }
    // Whittaker-style temperature × moisture body.
    let base = if c.moisture < 0.18 {
        Biome::Desert
    } else if c.temperature < 5.0 {
        Biome::Taiga
    } else if c.moisture > 0.75 && c.temperature > 18.0 {
        Biome::Rainforest
    } else if c.moisture > 0.45 {
        Biome::TemperateForest
    } else if c.moisture > 0.28 {
        Biome::Shrubland
    } else {
        Biome::Grassland
    };
    // Shallow soil cannot carry forest (section 8 plausibility rules).
    match base {
        Biome::Rainforest | Biome::TemperateForest | Biome::Taiga
            if s.depth < FOREST_SOIL_FLOOR =>
        {
            Biome::Shrubland
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(temperature: f32, moisture: f32) -> (Climate, Hydrology, Soils) {
        (
            Climate {
                temperature,
                moisture,
            },
            Hydrology {
                river: 0.0,
                wetness: 0.3,
            },
            Soils {
                depth: 0.6,
                fertility: 0.5,
            },
        )
    }

    #[test]
    fn ids_round_trip() {
        for b in BIOMES {
            assert_eq!(Biome::from_id(b.id()), b);
        }
        assert_eq!(Biome::from_id(200), Biome::Bare);
    }

    #[test]
    fn override_order_holds() {
        let (c, h, s) = sample(20.0, 0.5);
        // Water beats everything.
        assert_eq!(classify(-5.0, &c, &h, &s), Biome::Ocean);
        // Deep cold beats river.
        let cold = Climate {
            temperature: -15.0,
            moisture: 0.5,
        };
        let river = Hydrology {
            river: 0.9,
            wetness: 0.9,
        };
        assert_eq!(classify(100.0, &cold, &river, &s), Biome::Ice);
        // River beats wetland.
        assert_eq!(classify(100.0, &c, &river, &s), Biome::River);
        // Wetland beats the Whittaker body.
        let marsh = Hydrology {
            river: 0.1,
            wetness: 0.9,
        };
        assert_eq!(classify(100.0, &c, &marsh, &s), Biome::Wetland);
    }

    #[test]
    fn whittaker_body_covers_the_plane() {
        let (_, h, s) = sample(0.0, 0.0);
        let class = |t: f32, m: f32| {
            classify(
                100.0,
                &Climate {
                    temperature: t,
                    moisture: m,
                },
                &h,
                &s,
            )
        };
        assert_eq!(class(25.0, 0.1), Biome::Desert);
        assert_eq!(class(0.0, 0.5), Biome::Taiga);
        assert_eq!(class(25.0, 0.85), Biome::Rainforest);
        assert_eq!(class(12.0, 0.6), Biome::TemperateForest);
        assert_eq!(class(12.0, 0.35), Biome::Shrubland);
        assert_eq!(class(12.0, 0.22), Biome::Grassland);
        assert_eq!(class(-5.0, 0.5), Biome::Tundra);
    }

    #[test]
    fn shallow_soil_demotes_forest() {
        let (c, h, _) = sample(12.0, 0.6);
        let thin = Soils {
            depth: 0.1,
            fertility: 0.2,
        };
        assert_eq!(classify(100.0, &c, &h, &thin), Biome::Shrubland);
    }

    #[test]
    fn altitude_bares_the_summit() {
        let (c, h, s) = sample(10.0, 0.5);
        assert_eq!(classify(900.0, &c, &h, &s), Biome::Bare);
    }
}
