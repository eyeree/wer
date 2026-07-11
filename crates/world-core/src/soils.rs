//! Soils: depth and fertility from terrain, geology, climate, and hydrology
//! (phase-2-plan.md §7.5, milestone M5).
//!
//! Pure per-sample arithmetic over four input layers, with **no direct
//! possibility reads** — all sensitivity is inherited through inputs, which
//! makes soils the best test of transitive invalidation (phase-2-plan.md
//! §12.3). Ecological plausibility over science: fixed formulas, no soil
//! chemistry, no simulation loops (section 9).

use crate::climate::Climate;
use crate::geology::Geology;
use crate::hydrology::Hydrology;
use crate::terrain::SEA_LEVEL;

/// Per-sample soil state, both in `[0, 1]`. Presentation math, never identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Soils {
    /// Normalized soil depth (0 = bare rock, 1 = deep valley fill).
    pub depth: f32,
    /// Fertility (0 = sterile, 1 = rich).
    pub fertility: f32,
}

/// Slope (rise/run) at which soil can no longer accumulate at all.
const BARE_SLOPE: f32 = 0.4;

/// Temperature (°C) of peak biological soil activity.
const FERTILE_TEMPERATURE: f32 = 15.0;

/// Distance from the optimum (°C) at which fertility reaches zero.
const FERTILE_TOLERANCE: f32 = 25.0;

/// Soils at a sample: `depth = f(slope↓, hardness↓, wetness↑ deposition)`,
/// `fertility = f(depth, moisture, temperature window, lithology bias)`.
#[must_use]
pub fn soils(elevation: f32, slope: f32, g: &Geology, c: &Climate, h: &Hydrology) -> Soils {
    if elevation < SEA_LEVEL {
        return Soils {
            depth: 0.0,
            fertility: 0.0,
        };
    }
    // Flat, soft ground accumulates; wet ground receives deposition.
    let flatness = 1.0 - (slope / BARE_SLOPE).clamp(0.0, 1.0);
    let softness = 1.0 - 0.7 * g.hardness;
    let depth = (flatness * softness + 0.25 * h.wetness).clamp(0.0, 1.0);

    // Biological activity needs warmth (a smooth window) and water; the
    // lithology class biases the outcome (some rock weathers into richer soil).
    let t = (c.temperature - FERTILE_TEMPERATURE) / FERTILE_TOLERANCE;
    let warmth = (1.0 - t * t).max(0.0);
    let lithology_bias = 0.85 + 0.30 * f32::from(g.lithology) / 7.0;
    let fertility =
        (depth.sqrt() * (0.3 + 0.7 * c.moisture) * warmth * lithology_bias).clamp(0.0, 1.0);

    Soils { depth, fertility }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::climate;
    use crate::hydrology::hydrology;
    use crate::possibility::PossibilityVector;

    fn inputs(elevation: f32, slope: f32, hardness: f32) -> (Geology, Climate, Hydrology) {
        let p = PossibilityVector::neutral();
        let g = Geology {
            lithology: 3,
            hardness,
        };
        let c = climate(elevation, &p);
        let h = hydrology(elevation, slope, 0.0, &c, 0.5, 0.5);
        (g, c, h)
    }

    #[test]
    fn underwater_is_bare() {
        let (g, c, h) = inputs(-10.0, 0.0, 0.5);
        assert_eq!(
            soils(-10.0, 0.0, &g, &c, &h),
            Soils {
                depth: 0.0,
                fertility: 0.0
            }
        );
    }

    #[test]
    fn slopes_and_hard_rock_thin_the_soil() {
        let (g, c, h) = inputs(100.0, 0.0, 0.3);
        let flat_soft = soils(100.0, 0.0, &g, &c, &h);
        let steep = soils(100.0, 0.5, &g, &c, &h);
        let (hard_g, ..) = inputs(100.0, 0.0, 0.9);
        let hard = soils(100.0, 0.0, &hard_g, &c, &h);
        assert!(steep.depth < flat_soft.depth);
        assert!(hard.depth < flat_soft.depth);
    }

    #[test]
    fn fertility_needs_depth_and_warmth() {
        let (g, c, h) = inputs(100.0, 0.0, 0.4);
        let temperate = soils(100.0, 0.0, &g, &c, &h);
        // High-altitude cold: fertility collapses even where soil exists.
        let (g2, c2, h2) = inputs(4000.0, 0.0, 0.4);
        let alpine = soils(4000.0, 0.0, &g2, &c2, &h2);
        assert!(temperate.fertility > alpine.fertility);
        assert!((0.0..=1.0).contains(&temperate.depth));
        assert!((0.0..=1.0).contains(&temperate.fertility));
    }
}
