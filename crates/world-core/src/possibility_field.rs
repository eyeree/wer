//! Sparse possibility field: a coarse control-point lattice, bilinearly
//! interpolated (phase-1-plan.md section 6.3, milestone M2).
//!
//! The infinite world cannot store a possibility vector per region, so the
//! field is defined by control points on a coarse integer lattice — one every
//! [`PossibilityField::cell_regions`] regions. Each control point's base
//! vector is derived deterministically from its integer coordinate; a region's
//! base target is the bilinear blend of the four surrounding control points,
//! yielding a smoothly varying field everywhere. (An adaptive quadtree per
//! implementation-plan.md section 7 is a later refinement.)

use crate::coord::RegionCoord;
use crate::hash::{mix, Rng};
use crate::possibility::{PossibilityVector, POSSIBILITY_DIMS};
use crate::WORLD_ALGORITHM_VERSION;

/// Fixed basis separating possibility-field seeding from other hash domains.
const FIELD_BASIS: u64 = 0x51AB_93D0_4E2C_88F5;

/// A deterministic, smoothly varying possibility field over the infinite
/// region grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PossibilityField {
    /// Lattice spacing: one control point every `cell_regions` regions.
    /// Must be at least 1.
    pub cell_regions: u32,
}

impl PossibilityField {
    /// Default lattice spacing — possibility varies over ~8 regions (2 km at
    /// `REGION_SIZE = 256`), slow enough that adjacent regions differ gently.
    pub const DEFAULT_CELL_REGIONS: u32 = 8;

    /// A field with the given lattice spacing (clamped to at least 1).
    #[inline]
    #[must_use]
    pub const fn new(cell_regions: u32) -> Self {
        Self {
            cell_regions: if cell_regions == 0 { 1 } else { cell_regions },
        }
    }

    /// Deterministic seed for the control point at lattice coordinate
    /// `(cx, cy)`.
    ///
    /// A permanent integer identity — golden-fixtured and parity-tested across
    /// native and wasm (phase-1-plan.md section 11.2). Fold order is part of
    /// the stable contract.
    #[inline]
    #[must_use]
    pub const fn control_point_seed(&self, cx: i32, cy: i32) -> u64 {
        let mut h = FIELD_BASIS;
        h = mix(h, WORLD_ALGORITHM_VERSION as u64);
        h = mix(h, self.cell_regions as u64);
        h = mix(h, cx as u32 as u64);
        h = mix(h, cy as u32 as u64);
        h
    }

    /// The base possibility vector at a control point: each dimension drawn
    /// uniformly from the seeded portable [`Rng`]. Float outputs are
    /// presentation state; the identity is the integer seed.
    #[must_use]
    pub fn control_point(&self, cx: i32, cy: i32) -> PossibilityVector {
        let mut rng = Rng::new(self.control_point_seed(cx, cy));
        let mut v = PossibilityVector::neutral();
        for dim in v.dims.iter_mut() {
            *dim = rng.next_f32();
        }
        v
    }

    /// Sample the field at a region: bilinear interpolation of the four
    /// surrounding control points.
    ///
    /// Continuous across the whole grid — adjacent regions differ per
    /// dimension by at most `1 / cell_regions`, which is the seam bound the
    /// continuity replay asserts (phase-1-plan.md section 11.3).
    #[must_use]
    pub fn sample(&self, region: RegionCoord) -> PossibilityVector {
        let cell = self.cell_regions.max(1) as i32;
        // Euclidean division keeps cell index + fraction consistent for
        // negative coordinates (no reflection at the origin).
        let cx0 = region.x.div_euclid(cell);
        let cy0 = region.y.div_euclid(cell);
        let fx = region.x.rem_euclid(cell) as f32 / cell as f32;
        let fy = region.y.rem_euclid(cell) as f32 / cell as f32;

        let c00 = self.control_point(cx0, cy0);
        let c10 = self.control_point(cx0 + 1, cy0);
        let c01 = self.control_point(cx0, cy0 + 1);
        let c11 = self.control_point(cx0 + 1, cy0 + 1);

        let mut out = PossibilityVector::neutral();
        for i in 0..POSSIBILITY_DIMS {
            let x0 = c00.dims[i] + (c10.dims[i] - c00.dims[i]) * fx;
            let x1 = c01.dims[i] + (c11.dims[i] - c01.dims[i]) * fx;
            out.dims[i] = x0 + (x1 - x0) * fy;
        }
        out
    }
}

impl Default for PossibilityField {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CELL_REGIONS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_at_control_point_equals_control_point() {
        let f = PossibilityField::new(8);
        let cp = f.control_point(3, -2);
        let sampled = f.sample(RegionCoord::new(3 * 8, -2 * 8));
        assert_eq!(cp, sampled);
    }

    #[test]
    fn adjacent_regions_differ_by_bounded_gradient() {
        let f = PossibilityField::default();
        let bound = 1.0 / f.cell_regions as f32 + 1e-6;
        for x in -30..30 {
            for y in -30..30 {
                let a = f.sample(RegionCoord::new(x, y));
                let b = f.sample(RegionCoord::new(x + 1, y));
                let c = f.sample(RegionCoord::new(x, y + 1));
                for i in 0..POSSIBILITY_DIMS {
                    assert!((a.dims[i] - b.dims[i]).abs() <= bound);
                    assert!((a.dims[i] - c.dims[i]).abs() <= bound);
                }
            }
        }
    }

    #[test]
    fn sample_is_in_unit_range() {
        let f = PossibilityField::default();
        for x in -50..50 {
            for y in -50..50 {
                let v = f.sample(RegionCoord::new(x * 3, y * 3));
                for d in v.dims {
                    assert!((0.0..=1.0).contains(&d));
                }
            }
        }
    }
}
