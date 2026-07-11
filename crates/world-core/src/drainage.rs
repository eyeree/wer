//! Macro drainage: stable river-network topology from quantized elevation
//! (phase-2-plan.md §7.3, milestone M4, ADR 0009).
//!
//! River networks are *topology* and must be integer-derived (section 6.2),
//! yet flow is inherently non-local. Drainage is therefore computed per
//! **macro region** at [`MACRO_LEVEL`] — one cell per level-0 region plus a
//! [`MACRO_APRON`]-region apron — on an elevation grid quantized to integer
//! centimeters. All routing decisions happen on integers: float elevation
//! never decides topology.
//!
//! Routing consumes **no runtime possibility state**: each cell's elevation is
//! sampled at the quantized anchor-free possibility-field base of its region.
//! That makes a macro tile a pure function of its coordinate (plus the world
//! algorithm version), so networks are permanent — rivers do not walk under
//! any drift, fast or slow, and a macro tile spanning the pinned zone can
//! never rewrite the ground under the player. Possibility expresses through
//! the hydrology layer instead (river width, wetness — phase-2-plan.md §7.4).
//! The realized-terrain-vs-routing-elevation skew in strongly steered worlds
//! is a declared plausibility approximation (ADR 0009).
//!
//! Flow directions are **window-independent**: a cell's direction depends only
//! on its own 3×3 quantized neighborhood, so adjacent macro tiles can never
//! disagree about shared cells. Accumulation is computed within the aproned
//! window only; truncated catchments are a declared approximation — the
//! logarithmic width mapping in hydrology makes the truncation read as "big
//! river" rather than a seam, and the continuity replay bounds the residual
//! width step across macro boundaries (phase-2-plan.md §12.2).

use crate::anchor::project_plausible;
use crate::coord::{RegionCoord, REGION_SIZE};
use crate::hash::mix;
use crate::possibility_field::PossibilityField;
use crate::terrain::elevation;
use crate::WORLD_ALGORITHM_VERSION;

/// Hierarchy level of macro drainage tiles: level 4 ⇒ 16×16 level-0 regions
/// (4096 world units — one terrain [`crate::terrain::BASE_WAVELENGTH`]).
pub const MACRO_LEVEL: u16 = 4;

/// Level-0 regions per macro-tile edge.
pub const MACRO_REGIONS: i32 = 1 << MACRO_LEVEL;

/// Apron, in regions, sampled beyond the macro tile on every side so
/// accumulation sees upstream context and bilinear reads never leave the tile.
pub const MACRO_APRON: i32 = 16;

/// Cells per macro-tile grid edge (core + both aprons): 48.
pub const MACRO_GRID: usize = (MACRO_REGIONS + 2 * MACRO_APRON) as usize;

/// Flow-direction value for a cell with no lower neighbor (a quantized local
/// minimum — a lake/wetland seed rather than a carved channel).
pub const FLOW_NONE: u8 = 8;

/// The eight neighbor offsets, in the fixed order flow directions index.
/// Even indices are cardinal, odd are diagonal. The order is part of the
/// stable routing contract.
pub const FLOW_DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// Fixed basis separating drainage tie-break hashing from every other hash
/// domain. Part of the stable contract (changing it re-routes tied cells).
const DRAINAGE_BASIS: u64 = 0x4B83_F60D_2E97_A1C4;

/// Deterministic tie-break hash for the drainage cell of region
/// `(region_x, region_y)` — a permanent identity (it decides topology),
/// golden-fixtured and wasm-parity-tested (phase-2-plan.md §12.5).
#[inline]
#[must_use]
pub const fn tiebreak_hash(region_x: i64, region_y: i64) -> u64 {
    let mut h = DRAINAGE_BASIS;
    h = mix(h, WORLD_ALGORITHM_VERSION as u64);
    h = mix(h, region_x as u64);
    h = mix(h, region_y as u64);
    h
}

/// Quantize an elevation to integer centimeters — the only form routing sees.
#[inline]
#[must_use]
pub fn quantize_elevation_cm(elevation: f32) -> i32 {
    (elevation * 100.0).round() as i32
}

/// Routing elevation of one drainage cell (the level-0 region at
/// `(region_x, region_y)`): the terrain heightfield at the region center,
/// under the region's quantized anchor-free field base, in centimeters.
#[must_use]
pub fn routing_elevation_cm(field: &PossibilityField, region_x: i32, region_y: i32) -> i32 {
    let p = project_plausible(field.sample(RegionCoord::new(region_x, region_y))).requantized();
    let cx = (f64::from(region_x) + 0.5) * REGION_SIZE;
    let cy = (f64::from(region_y) + 0.5) * REGION_SIZE;
    quantize_elevation_cm(elevation(cx, cy, &p))
}

/// Flow direction of one cell from its own 3×3 quantized neighborhood —
/// window-independent by construction. `neighborhood[dy+1][dx+1]` is the
/// elevation of the cell at offset `(dx, dy)`; steepest integer descent with
/// diagonal distance weighting, ties broken by [`tiebreak_hash`].
#[must_use]
pub fn flow_direction(neighborhood: &[[i32; 3]; 3], region_x: i64, region_y: i64) -> u8 {
    let here = neighborhood[1][1];
    let mut best_score: i64 = 0;
    let mut candidates: [u8; 8] = [0; 8];
    let mut count = 0usize;
    for (dir, (dx, dy)) in FLOW_DIRS.iter().enumerate() {
        let there = neighborhood[(dy + 1) as usize][(dx + 1) as usize];
        let drop = i64::from(here) - i64::from(there);
        if drop <= 0 {
            continue; // only strictly lower neighbors: descent is acyclic
        }
        // Distance-weighted steepness kept integral: ×10 cardinal, ×7 diagonal
        // (7/10 ≈ 1/√2).
        let weight = if dir % 2 == 0 { 10 } else { 7 };
        let score = drop * weight;
        match score.cmp(&best_score) {
            core::cmp::Ordering::Greater => {
                best_score = score;
                candidates[0] = dir as u8;
                count = 1;
            }
            core::cmp::Ordering::Equal => {
                candidates[count] = dir as u8;
                count += 1;
            }
            core::cmp::Ordering::Less => {}
        }
    }
    if count == 0 {
        FLOW_NONE
    } else {
        candidates[(tiebreak_hash(region_x, region_y) % count as u64) as usize]
    }
}

/// One macro tile of drainage topology: per-cell flow direction and flow
/// accumulation over the aproned [`MACRO_GRID`]² grid, tagged with the
/// dependency hash it was generated for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainageTile {
    coord: RegionCoord,
    /// The [`crate::dephash::drainage_dep_hash`] this tile was generated for.
    pub dep_hash: u64,
    flow_dir: Vec<u8>,
    accum: Vec<u32>,
}

impl DrainageTile {
    /// The macro coordinate (level [`MACRO_LEVEL`]) this tile covers.
    #[inline]
    #[must_use]
    pub const fn coord(&self) -> RegionCoord {
        self.coord
    }

    /// Region coordinate of grid cell `(0, 0)` (south-west apron corner).
    #[inline]
    #[must_use]
    pub const fn origin_region(&self) -> (i32, i32) {
        (
            self.coord.x * MACRO_REGIONS - MACRO_APRON,
            self.coord.y * MACRO_REGIONS - MACRO_APRON,
        )
    }

    /// Flow direction of grid cell `(gx, gy)` (index into [`FLOW_DIRS`], or
    /// [`FLOW_NONE`]).
    #[inline]
    #[must_use]
    pub fn flow_dir_at(&self, gx: usize, gy: usize) -> u8 {
        self.flow_dir[gy * MACRO_GRID + gx]
    }

    /// Flow accumulation (cells draining through, including itself) of grid
    /// cell `(gx, gy)`.
    #[inline]
    #[must_use]
    pub fn accum_at(&self, gx: usize, gy: usize) -> u32 {
        self.accum[gy * MACRO_GRID + gx]
    }

    /// Flow accumulation of the cell for level-0 region `(region_x, region_y)`,
    /// if it lies within this tile's grid.
    #[must_use]
    pub fn accum_at_region(&self, region_x: i32, region_y: i32) -> Option<u32> {
        let (ox, oy) = self.origin_region();
        let gx = region_x.checked_sub(ox)?;
        let gy = region_y.checked_sub(oy)?;
        if (0..MACRO_GRID as i32).contains(&gx) && (0..MACRO_GRID as i32).contains(&gy) {
            Some(self.accum_at(gx as usize, gy as usize))
        } else {
            None
        }
    }

    /// Bilinear flow accumulation under a continuous world position. Cell
    /// centers sit at region centers; positions over the tile's core (and well
    /// into the apron) always have four in-grid neighbors.
    #[must_use]
    pub fn accum_bilinear(&self, world_x: f64, world_y: f64) -> f32 {
        let (ox, oy) = self.origin_region();
        // Grid-space position: cell (gx, gy) center is at grid coord (gx, gy).
        let u = world_x / REGION_SIZE - 0.5 - f64::from(ox);
        let v = world_y / REGION_SIZE - 0.5 - f64::from(oy);
        let max = (MACRO_GRID - 2) as f64;
        let u = u.clamp(0.0, max);
        let v = v.clamp(0.0, max);
        let x0 = u.floor();
        let y0 = v.floor();
        let fx = (u - x0) as f32;
        let fy = (v - y0) as f32;
        let gx = x0 as usize;
        let gy = y0 as usize;
        let a = |x: usize, y: usize| self.accum_at(x, y) as f32;
        let top = a(gx, gy) + (a(gx + 1, gy) - a(gx, gy)) * fx;
        let bottom = a(gx, gy + 1) + (a(gx + 1, gy + 1) - a(gx, gy + 1)) * fx;
        top + (bottom - top) * fy
    }

    /// Order-stable hash of the tile's contents (replay determinism check).
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xD8A1_4A6E_5EED_1234;
        h = mix(h, self.dep_hash);
        h = mix(h, self.coord.x as u32 as u64);
        h = mix(h, self.coord.y as u32 as u64);
        for &d in &self.flow_dir {
            h = mix(h, d as u64);
        }
        for &a in &self.accum {
            h = mix(h, a as u64);
        }
        h
    }

    /// Heap bytes held by the tile (cache telemetry, phase-2-plan.md §13).
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.flow_dir.len() + self.accum.len() * core::mem::size_of::<u32>()
    }
}

/// The macro coordinate covering a level-0 region.
#[inline]
#[must_use]
pub const fn macro_coord_for(region: RegionCoord) -> RegionCoord {
    // Arithmetic shift floors toward negative infinity, matching parent().
    RegionCoord::at_level(
        region.x >> MACRO_LEVEL,
        region.y >> MACRO_LEVEL,
        MACRO_LEVEL,
    )
}

/// Generate the drainage tile for one macro coordinate.
///
/// Pure and allocation-bounded: a `(MACRO_GRID + 2)²` quantized elevation
/// grid, one flow-direction pass, and one descending-elevation accumulation
/// pass. `dep_hash` is the [`crate::dephash::drainage_dep_hash`] the caller
/// computed for its cache; the generator just stamps it on the tile.
#[must_use]
pub fn drainage(macro_coord: RegionCoord, field: &PossibilityField, dep_hash: u64) -> DrainageTile {
    debug_assert_eq!(macro_coord.level, MACRO_LEVEL);
    let ox = macro_coord.x * MACRO_REGIONS - MACRO_APRON;
    let oy = macro_coord.y * MACRO_REGIONS - MACRO_APRON;

    // Elevations for the grid plus a one-cell rim, so every grid cell has a
    // full 3×3 neighborhood.
    const EDGE: usize = MACRO_GRID + 2;
    let mut elev = vec![0i32; EDGE * EDGE];
    for gy in 0..EDGE {
        for gx in 0..EDGE {
            let rx = ox + gx as i32 - 1;
            let ry = oy + gy as i32 - 1;
            elev[gy * EDGE + gx] = routing_elevation_cm(field, rx, ry);
        }
    }

    let mut flow_dir = vec![FLOW_NONE; MACRO_GRID * MACRO_GRID];
    for gy in 0..MACRO_GRID {
        for gx in 0..MACRO_GRID {
            let mut neighborhood = [[0i32; 3]; 3];
            for (dy, row) in neighborhood.iter_mut().enumerate() {
                for (dx, cell) in row.iter_mut().enumerate() {
                    *cell = elev[(gy + dy) * EDGE + (gx + dx)];
                }
            }
            flow_dir[gy * MACRO_GRID + gx] = flow_direction(
                &neighborhood,
                i64::from(ox) + gx as i64,
                i64::from(oy) + gy as i64,
            );
        }
    }

    // Accumulation: process cells from highest to lowest. Flow always goes to
    // a strictly lower cell, so every contributor is finished before its
    // target; ties in elevation cannot flow to each other, making the
    // deterministic (elevation, coord) order also order-*insensitive*.
    let mut order: Vec<u32> = (0..(MACRO_GRID * MACRO_GRID) as u32).collect();
    order.sort_unstable_by_key(|&i| {
        let gx = i as usize % MACRO_GRID;
        let gy = i as usize / MACRO_GRID;
        (core::cmp::Reverse(elev[(gy + 1) * EDGE + (gx + 1)]), i)
    });
    let mut accum = vec![1u32; MACRO_GRID * MACRO_GRID];
    for &i in &order {
        let gx = i as usize % MACRO_GRID;
        let gy = i as usize / MACRO_GRID;
        let dir = flow_dir[gy * MACRO_GRID + gx];
        if dir == FLOW_NONE {
            continue;
        }
        let (dx, dy) = FLOW_DIRS[dir as usize];
        let tx = gx as i32 + dx;
        let ty = gy as i32 + dy;
        if (0..MACRO_GRID as i32).contains(&tx) && (0..MACRO_GRID as i32).contains(&ty) {
            accum[ty as usize * MACRO_GRID + tx as usize] += accum[gy * MACRO_GRID + gx];
        }
    }

    DrainageTile {
        coord: macro_coord,
        dep_hash,
        flow_dir,
        accum,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_pure() {
        let field = PossibilityField::default();
        let mc = RegionCoord::at_level(0, 0, MACRO_LEVEL);
        let a = drainage(mc, &field, 1);
        let b = drainage(mc, &field, 1);
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn flow_directions_are_window_independent() {
        // Adjacent macro tiles overlap by 2×MACRO_APRON columns of cells; the
        // shared cells must route identically in both windows (phase-2-plan.md
        // §7.3 point 2).
        let field = PossibilityField::default();
        let a = drainage(RegionCoord::at_level(0, 0, MACRO_LEVEL), &field, 0);
        let b = drainage(RegionCoord::at_level(1, 0, MACRO_LEVEL), &field, 0);
        let (aox, aoy) = a.origin_region();
        let (box_, boy) = b.origin_region();
        assert_eq!(aoy, boy);
        let overlap = MACRO_GRID as i32 - (box_ - aox);
        assert!(overlap > 0);
        for gy in 0..MACRO_GRID {
            for k in 0..overlap as usize {
                let ax = (box_ - aox) as usize + k;
                assert_eq!(
                    a.flow_dir_at(ax, gy),
                    b.flow_dir_at(k, gy),
                    "shared cell ({k}, {gy}) routes differently across windows"
                );
            }
        }
    }

    #[test]
    fn flow_goes_strictly_downhill_and_accum_is_sane() {
        let field = PossibilityField::default();
        let mc = RegionCoord::at_level(-1, 2, MACRO_LEVEL);
        let tile = drainage(mc, &field, 0);
        let (ox, oy) = tile.origin_region();
        let mut max_accum = 0;
        for gy in 0..MACRO_GRID {
            for gx in 0..MACRO_GRID {
                let accum = tile.accum_at(gx, gy);
                assert!(accum >= 1);
                max_accum = max_accum.max(accum);
                let dir = tile.flow_dir_at(gx, gy);
                if dir == FLOW_NONE {
                    continue;
                }
                let (dx, dy) = FLOW_DIRS[dir as usize];
                let here = routing_elevation_cm(&field, ox + gx as i32, oy + gy as i32);
                let there = routing_elevation_cm(&field, ox + gx as i32 + dx, oy + gy as i32 + dy);
                assert!(there < here, "flow must be strictly downhill");
            }
        }
        // Water collects: some cell must gather a real catchment.
        assert!(
            max_accum > 20,
            "expected a river trunk, max accum {max_accum}"
        );
    }

    #[test]
    fn tie_break_is_a_pure_function_of_the_cell() {
        let n = [[10, 5, 10], [5, 8, 10], [10, 10, 10]];
        // Two equal steepest descents (W and S at drop 3 ×10, N at 3×10 too):
        // whatever wins must win again.
        assert_eq!(flow_direction(&n, 7, -3), flow_direction(&n, 7, -3));
        // And a different cell coordinate may legitimately pick differently,
        // but always among the tied candidates (indices 2, 4, 6 are N, W, S).
        let picked = flow_direction(&n, 12345, 678);
        assert!([2u8, 4, 6].contains(&picked));
    }

    #[test]
    fn accum_bilinear_interpolates_within_the_grid() {
        let field = PossibilityField::default();
        let mc = RegionCoord::at_level(0, 0, MACRO_LEVEL);
        let tile = drainage(mc, &field, 0);
        // At an exact cell center the bilinear read returns the cell value.
        let (ox, oy) = tile.origin_region();
        let rx = ox + MACRO_APRON + 3;
        let ry = oy + MACRO_APRON + 5;
        let world = (
            (f64::from(rx) + 0.5) * REGION_SIZE,
            (f64::from(ry) + 0.5) * REGION_SIZE,
        );
        let exact = tile.accum_at_region(rx, ry).unwrap() as f32;
        assert_eq!(tile.accum_bilinear(world.0, world.1), exact);
    }

    #[test]
    fn macro_coord_floors_negatives() {
        assert_eq!(
            macro_coord_for(RegionCoord::new(-1, -17)),
            RegionCoord::at_level(-1, -2, MACRO_LEVEL)
        );
        assert_eq!(
            macro_coord_for(RegionCoord::new(15, 16)),
            RegionCoord::at_level(0, 1, MACRO_LEVEL)
        );
    }
}
