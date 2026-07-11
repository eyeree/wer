//! Ecology: a single aggregate vegetation scalar (phase-1-plan.md section 6.2,
//! milestone M3).
//!
//! Phase 1 collapses all of ecology into one density in `[0, 1]` — the product
//! of climate suitability and the Ecology possibility dimension, clamped by a
//! rainfall/vegetation plausibility rule (implementation-plan.md section 8).
//! Canopy, biomass, and species structure arrive in Phase 3.

use crate::climate::Climate;
use crate::possibility::{PossibilityDomain, PossibilityVector};
use crate::terrain::SEA_LEVEL;

/// Temperature (°C) at which vegetation thrives.
const OPTIMAL_TEMPERATURE: f32 = 18.0;

/// Distance from optimum (°C) at which suitability reaches zero.
const TEMPERATURE_TOLERANCE: f32 = 30.0;

/// Aggregate vegetation density in `[0, 1]` for a sample.
///
/// Zero on open water. On land: the Ecology possibility dimension drives
/// density, scaled by a smooth temperature-suitability curve and by moisture,
/// then capped so vegetation never materially exceeds available rainfall (the
/// plausibility rule the projection in [`crate::anchor::project_plausible`]
/// also enforces at the possibility level).
#[must_use]
pub fn vegetation_density(elevation: f32, c: &Climate, p: &PossibilityVector) -> f32 {
    if elevation < SEA_LEVEL {
        return 0.0;
    }
    let t = (c.temperature - OPTIMAL_TEMPERATURE) / TEMPERATURE_TOLERANCE;
    let suitability = (1.0 - t * t).max(0.0);
    let drive = p.get(PossibilityDomain::Ecology);
    let raw = drive * suitability * c.moisture.sqrt();
    raw.min(c.moisture + 0.1).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::climate;

    #[test]
    fn water_has_no_vegetation() {
        let p = PossibilityVector::neutral();
        let c = climate(-10.0, &p);
        assert_eq!(vegetation_density(-10.0, &c, &p), 0.0);
    }

    #[test]
    fn density_is_bounded_and_tracks_the_ecology_dim() {
        let mut lush = PossibilityVector::neutral();
        lush.set(PossibilityDomain::Ecology, 1.0);
        let mut barren = PossibilityVector::neutral();
        barren.set(PossibilityDomain::Ecology, 0.0);
        let c = climate(50.0, &lush);
        let hi = vegetation_density(50.0, &c, &lush);
        let lo = vegetation_density(50.0, &climate(50.0, &barren), &barren);
        assert!((0.0..=1.0).contains(&hi));
        assert_eq!(lo, 0.0);
        assert!(hi > lo);
    }

    #[test]
    fn vegetation_never_far_exceeds_moisture() {
        let mut p = PossibilityVector::neutral();
        p.set(PossibilityDomain::Ecology, 1.0);
        p.set(PossibilityDomain::Hydrology, 0.0);
        p.set(PossibilityDomain::Planetary, 0.0);
        let c = climate(400.0, &p);
        assert!(vegetation_density(400.0, &c, &p) <= c.moisture + 0.1);
    }
}
