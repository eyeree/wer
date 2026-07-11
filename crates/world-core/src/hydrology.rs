//! Hydrology expression: river width and surface wetness — the drifting half
//! of the water story (phase-2-plan.md §7.4, milestone M5).
//!
//! The river *network* is pinned by the macro drainage topology (ADR 0009);
//! this layer is where section 9's "possibility drift should more commonly
//! modify river width, surface wetness, marsh extent" lands. Per level-0
//! sample: the bilinear macro flow accumulation maps through a logarithmic
//! width curve into `river ∈ [0, 1]`; climate moisture, river proximity,
//! low-slope ponding, and the Hydrology/Planetary buckets combine into
//! `wetness ∈ [0, 1]`. Pure per-sample presentation math — never identity.

use crate::climate::Climate;
use crate::terrain::SEA_LEVEL;

/// Per-sample hydrology expression. Presentation math, never identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hydrology {
    /// River presence/width in `[0, 1]` (0 = dry land, 1 = major channel).
    pub river: f32,
    /// Surface wetness in `[0, 1]` (dust ↔ standing water).
    pub wetness: f32,
}

/// Catchment (in macro cells ≈ regions) below which no channel expresses.
pub const RIVER_SOURCE_ACCUM: f32 = 5.0;

/// Catchment at which the logarithmic width curve saturates. Truncated
/// catchments (phase-2-plan.md §7.3) clamp here, so the window cut reads as
/// "big river" rather than a seam.
pub const RIVER_SATURATION_ACCUM: f32 = 400.0;

/// Slope (rise/run) above which ponding is impossible.
const PONDING_SLOPE: f32 = 0.05;

/// The log-shaped channel-width curve: 0 at [`RIVER_SOURCE_ACCUM`], 1 at
/// [`RIVER_SATURATION_ACCUM`] and beyond. The square root lifts low-order
/// streams into visibility without moving the endpoints — headwaters should
/// read as thin lines, not vanish (phase-2-plan.md §11: rivers are the
/// popping detector, so they must be *legible*).
#[must_use]
pub fn river_intensity(accum: f32) -> f32 {
    if accum <= RIVER_SOURCE_ACCUM {
        return 0.0;
    }
    let t = (accum / RIVER_SOURCE_ACCUM).ln() / (RIVER_SATURATION_ACCUM / RIVER_SOURCE_ACCUM).ln();
    t.clamp(0.0, 1.0).sqrt()
}

/// Hydrology expression at a sample.
///
/// `accum` is the bilinear macro flow accumulation under the sample; `slope`
/// is the local terrain gradient (rise/run); `p_hydrology` / `p_planetary`
/// are the *dequantized* Hydrology and Planetary buckets (phase-2-plan.md
/// §4.2 — generators never see raw floats). Open water saturates.
#[must_use]
pub fn hydrology(
    elevation: f32,
    slope: f32,
    accum: f32,
    c: &Climate,
    p_hydrology: f32,
    p_planetary: f32,
) -> Hydrology {
    if elevation < SEA_LEVEL {
        return Hydrology {
            river: 0.0,
            wetness: 1.0,
        };
    }
    // Width breathes with possibility and climate; the channel *location*
    // (where accum is high) is fixed by drainage.
    let width = river_intensity(accum);
    let river = (width * (0.55 + 0.45 * c.moisture) * (0.6 + 0.8 * p_hydrology)).clamp(0.0, 1.0);

    // Flat ground holds water; steeper than PONDING_SLOPE sheds it.
    let ponding = (1.0 - slope / PONDING_SLOPE).clamp(0.0, 1.0);
    let wetness = (0.40 * c.moisture
        + 0.30 * river
        + 0.20 * ponding * (0.3 + 0.7 * c.moisture)
        + 0.15 * p_hydrology
        + 0.05 * p_planetary)
        .clamp(0.0, 1.0);

    Hydrology { river, wetness }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::climate;
    use crate::possibility::PossibilityVector;

    #[test]
    fn open_water_saturates() {
        let c = climate(-20.0, &PossibilityVector::neutral());
        let h = hydrology(-20.0, 0.0, 500.0, &c, 0.5, 0.5);
        assert_eq!(h.river, 0.0);
        assert_eq!(h.wetness, 1.0);
    }

    #[test]
    fn river_needs_a_catchment_and_grows_logarithmically() {
        assert_eq!(river_intensity(0.0), 0.0);
        assert_eq!(river_intensity(RIVER_SOURCE_ACCUM), 0.0);
        let small = river_intensity(40.0);
        let big = river_intensity(400.0);
        assert!(small > 0.0 && big > small);
        assert_eq!(river_intensity(RIVER_SATURATION_ACCUM * 10.0), 1.0);
    }

    #[test]
    fn width_breathes_with_the_hydrology_bucket() {
        // The section 9 commitment: steering widens rivers without moving them.
        let c = climate(50.0, &PossibilityVector::neutral());
        let dry = hydrology(50.0, 0.02, 200.0, &c, 0.0, 0.5);
        let wet = hydrology(50.0, 0.02, 200.0, &c, 1.0, 0.5);
        assert!(wet.river > dry.river);
        assert!(wet.wetness > dry.wetness);
    }

    #[test]
    fn flat_land_ponds_and_slopes_shed() {
        let c = climate(80.0, &PossibilityVector::neutral());
        let flat = hydrology(80.0, 0.0, 0.0, &c, 0.5, 0.5);
        let steep = hydrology(80.0, 0.4, 0.0, &c, 0.5, 0.5);
        assert!(flat.wetness > steep.wetness);
    }

    #[test]
    fn outputs_are_bounded() {
        let c = climate(10.0, &PossibilityVector::neutral());
        for accum in [0.0, 15.0, 1e6] {
            for p in [0.0, 1.0] {
                let h = hydrology(10.0, 0.1, accum, &c, p, p);
                assert!((0.0..=1.0).contains(&h.river));
                assert!((0.0..=1.0).contains(&h.wetness));
            }
        }
    }
}
