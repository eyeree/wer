//! Climate: temperature and moisture from possibility + elevation
//! (phase-1-plan.md section 6.2; re-plumbed through the layer graph by
//! phase-2-plan.md §7.1).
//!
//! The Phase 1 math survives unchanged; what changed is plumbing — the layer
//! now reads *dequantized* possibility buckets and the terrain input tile
//! (its declared inputs, see [`crate::layer`]) instead of raw `current` floats
//! and recomputed elevation. Deliberately cheap — pure per-sample arithmetic —
//! so a whole region tile can be recomputed every time a bucket flips.

use crate::possibility::{PossibilityDomain, PossibilityVector};
use crate::terrain::SEA_LEVEL;

/// Per-sample climate state. Presentation math (`f32`), never identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Climate {
    /// Air temperature in °C.
    pub temperature: f32,
    /// Surface moisture / rainfall proxy in `[0, 1]`.
    pub moisture: f32,
}

/// Temperature drop per world unit of elevation above sea level (a standard
/// atmospheric lapse rate, treating world units as meters).
pub const LAPSE_RATE: f32 = 0.0065;

/// Coldest and warmest sea-level base temperatures the Climate dimension can
/// reach, in °C.
pub const TEMPERATURE_RANGE: (f32, f32) = (-5.0, 30.0);

/// Moisture lost per world unit of elevation above sea level (high ground
/// drains and dries).
const MOISTURE_LAPSE: f32 = 8.0e-4;

/// Climate at a sample, given its elevation and the region's realized
/// possibility state.
///
/// Temperature: the Climate dimension sets the sea-level base, cooled by
/// elevation at [`LAPSE_RATE`]. Moisture: fed by the Hydrology (surface
/// wetness) and Planetary (ocean fraction) dimensions, drying with altitude;
/// open water saturates.
#[must_use]
pub fn climate(elevation: f32, p: &PossibilityVector) -> Climate {
    let (t_min, t_max) = TEMPERATURE_RANGE;
    let base = t_min + (t_max - t_min) * p.get(PossibilityDomain::Climate);
    let above_sea = (elevation - SEA_LEVEL).max(0.0);
    let temperature = base - LAPSE_RATE * above_sea;

    let moisture = if elevation < SEA_LEVEL {
        1.0
    } else {
        let supply = 0.15
            + 0.55 * p.get(PossibilityDomain::Hydrology)
            + 0.30 * p.get(PossibilityDomain::Planetary);
        (supply - MOISTURE_LAPSE * above_sea).clamp(0.0, 1.0)
    };

    Climate {
        temperature,
        moisture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_falls_with_elevation() {
        let p = PossibilityVector::neutral();
        let low = climate(10.0, &p);
        let high = climate(800.0, &p);
        assert!(high.temperature < low.temperature);
    }

    #[test]
    fn water_is_saturated_and_moisture_is_bounded() {
        let p = PossibilityVector::neutral();
        assert_eq!(climate(-50.0, &p).moisture, 1.0);
        for elev in [0.0, 200.0, 900.0] {
            let c = climate(elev, &p);
            assert!((0.0..=1.0).contains(&c.moisture));
        }
    }

    #[test]
    fn hydrology_raises_moisture() {
        let mut wet = PossibilityVector::neutral();
        wet.set(PossibilityDomain::Hydrology, 1.0);
        let mut dry = PossibilityVector::neutral();
        dry.set(PossibilityDomain::Hydrology, 0.0);
        assert!(climate(100.0, &wet).moisture > climate(100.0, &dry).moisture);
    }
}
