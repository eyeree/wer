//! Geology expression: lithology provinces and rock hardness
//! (phase-2-plan.md §7.2, milestone M3).
//!
//! The world is partitioned into lithology cells on a coarse jittered integer
//! lattice — a Voronoi diagram of hash-jittered cell centers, so rock
//! boundaries never read as grid lines. Each cell's lithology id and base
//! hardness derive from [`lithology_seed`]: pure integer hashing under a fixed
//! basis, the same discipline as terrain gradients (ADR 0003/0004). The seed
//! is a permanent identity, golden-fixtured and wasm-parity-tested; the float
//! Voronoi distance test and the hardness scalar are presentation math.
//!
//! Hardness is modulated smoothly by the slow Geology dimension (harder, more
//! exposed rock in tectonically active worlds). No fast domain touches
//! geology — rock does not change under a climate anchor (section 9).

use crate::coord::REGION_SIZE;
use crate::hash::mix;
use crate::WORLD_ALGORITHM_VERSION;

/// Fixed basis separating lithology hashing from every other hash domain.
/// Part of the stable contract (changing it moves every rock province).
const GEOLOGY_BASIS: u64 = 0x9C0F_31E7_D48A_5B26;

/// Edge length, in world units, of one lithology lattice cell — six regions,
/// inside the plan's "≈ 4–8 regions across" band, so provinces span multiple
/// regions without dwarfing the streaming window.
pub const LITHOLOGY_CELL_SIZE: f64 = 6.0 * REGION_SIZE;

/// Number of distinct lithology classes (ids are `0..LITHOLOGY_TYPES`).
pub const LITHOLOGY_TYPES: u8 = 8;

/// Per-sample geology state. Presentation math (`f32` hardness), never
/// identity — the identity lives in [`lithology_seed`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geology {
    /// Lithology class id in `[0, LITHOLOGY_TYPES)`.
    pub lithology: u8,
    /// Rock hardness in `[0, 1]` (soft sediment ↔ hard basement).
    pub hardness: f32,
}

/// Deterministic 64-bit seed for the lithology cell at integer lattice
/// coordinate `(cell_x, cell_y)`.
///
/// A permanent *identity* (it decides which rock is where), so it is pure
/// integer hashing, covered by golden fixtures and the native↔wasm parity
/// test (phase-2-plan.md §12.5). Fold order is part of the stable contract.
#[inline]
#[must_use]
pub const fn lithology_seed(cell_x: i64, cell_y: i64) -> u64 {
    let mut h = GEOLOGY_BASIS;
    h = mix(h, WORLD_ALGORITHM_VERSION as u64);
    // Signed coordinates fold as their unsigned bit patterns for portability.
    h = mix(h, cell_x as u64);
    h = mix(h, cell_y as u64);
    h
}

/// Jittered world-space center of a lithology cell: the cell center displaced
/// by up to ±¼ cell in each axis, from exact integer-derived fractions.
fn cell_center(cell_x: i64, cell_y: i64) -> (f64, f64) {
    let seed = lithology_seed(cell_x, cell_y);
    // Top bits → [0, 1) fractions; exact in f64 (20-bit numerators).
    let jx = ((seed >> 44) as f64) / (1u64 << 20) as f64 - 0.5;
    let jy = (((seed >> 24) & 0xF_FFFF) as f64) / (1u64 << 20) as f64 - 0.5;
    (
        (cell_x as f64 + 0.5 + jx * 0.5) * LITHOLOGY_CELL_SIZE,
        (cell_y as f64 + 0.5 + jy * 0.5) * LITHOLOGY_CELL_SIZE,
    )
}

/// The lithology cell whose jittered center is nearest to a world position —
/// the Voronoi lookup behind [`lithology_id`]. With jitter bounded to ±¼ cell,
/// the nearest center is always within the 3×3 cell neighborhood.
#[must_use]
pub fn lithology_cell(world_x: f64, world_y: f64) -> (i64, i64) {
    let gx = (world_x / LITHOLOGY_CELL_SIZE).floor() as i64;
    let gy = (world_y / LITHOLOGY_CELL_SIZE).floor() as i64;
    let mut best = (gx, gy);
    let mut best_d2 = f64::INFINITY;
    // Fixed iteration order makes the (measure-zero) tie case deterministic.
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cell = (gx + dx, gy + dy);
            let (cx, cy) = cell_center(cell.0, cell.1);
            let d2 = (world_x - cx).powi(2) + (world_y - cy).powi(2);
            if d2 < best_d2 {
                best_d2 = d2;
                best = cell;
            }
        }
    }
    best
}

/// Lithology class id at a world position — independent of every possibility
/// dimension, so soils can read it through this pure function rather than a
/// cached channel (phase-2-plan.md §6.1).
#[must_use]
pub fn lithology_id(world_x: f64, world_y: f64) -> u8 {
    let (cx, cy) = lithology_cell(world_x, world_y);
    (lithology_seed(cx, cy) & (LITHOLOGY_TYPES as u64 - 1)) as u8
}

/// Geology at a world position, given the dequantized slow Geology dimension.
///
/// Base hardness is drawn per cell from the seed; the Geology dimension scales
/// it smoothly (`0..1` → `×0.75..×1.25`) so tectonically active worlds read as
/// harder, more exposed rock without ever moving a province boundary.
#[must_use]
pub fn geology(world_x: f64, world_y: f64, p_geology: f32) -> Geology {
    let (cx, cy) = lithology_cell(world_x, world_y);
    let seed = lithology_seed(cx, cy);
    let lithology = (seed & (LITHOLOGY_TYPES as u64 - 1)) as u8;
    // Bits 8..24 → base hardness in [0.2, 0.9].
    let base = 0.2 + 0.7 * ((seed >> 8) & 0xFFFF) as f32 / 65536.0;
    let hardness = (base * (0.75 + 0.5 * p_geology)).clamp(0.0, 1.0);
    Geology {
        lithology,
        hardness,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lithology_is_constant_within_a_cell_interior() {
        // Two nearby samples deep inside the same Voronoi cell agree.
        let (cx, cy) = cell_center(3, -2);
        assert_eq!(lithology_id(cx, cy), lithology_id(cx + 10.0, cy - 10.0));
    }

    #[test]
    fn geology_ignores_fast_dimensions_by_construction() {
        // The signature admits only the slow Geology scalar; identical inputs
        // must produce identical outputs.
        let a = geology(1234.5, -678.9, 0.5);
        let b = geology(1234.5, -678.9, 0.5);
        assert_eq!(a, b);
        assert!(a.lithology < LITHOLOGY_TYPES);
        assert!((0.0..=1.0).contains(&a.hardness));
    }

    #[test]
    fn tectonic_activity_hardens_rock_without_moving_it() {
        let quiet = geology(500.0, 500.0, 0.0);
        let active = geology(500.0, 500.0, 1.0);
        assert_eq!(quiet.lithology, active.lithology, "provinces must not move");
        assert!(active.hardness > quiet.hardness);
    }

    #[test]
    fn provinces_have_multiple_classes() {
        // Sweep a wide area: more than one lithology class must appear.
        let mut seen = [false; LITHOLOGY_TYPES as usize];
        for i in -8..8 {
            for j in -8..8 {
                let x = f64::from(i) * LITHOLOGY_CELL_SIZE;
                let y = f64::from(j) * LITHOLOGY_CELL_SIZE;
                seen[lithology_id(x, y) as usize] = true;
            }
        }
        assert!(seen.iter().filter(|&&s| s).count() >= 4);
    }
}
