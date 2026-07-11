//! Deterministic infinite heightfield (phase-1-plan.md section 6.1, milestone M1).
//!
//! Multi-octave gradient noise (fBm). Interpolation and octave summation are
//! `f32` presentation math, but every lattice-corner gradient — the thing that
//! decides *where* mountains are — is selected by integer hashing through
//! [`crate::hash::mix`], so major topology is exactly reproducible for a given
//! [`crate::WORLD_ALGORITHM_VERSION`] (ADR 0003, ADR 0004).
//!
//! Possibility coupling is deliberately weak: elevation reads only the slow
//! Geology and Planetary dimensions, through smooth scale/offset functions.
//! Possibility *drift* therefore moves climate and ecology, not the mountains —
//! the single most important choice for avoiding landmark contradiction
//! (implementation-plan.md section 9).

use crate::hash::mix;
use crate::possibility::{PossibilityDomain, PossibilityVector};
use crate::WORLD_ALGORITHM_VERSION;

/// Fixed basis separating terrain-gradient hashing from every other hash
/// domain. Part of the stable contract (changing it moves every mountain).
const TERRAIN_BASIS: u64 = 0x7E11_AD5C_0FFE_E712;

/// Octaves of fBm summed into the heightfield.
pub const OCTAVES: u32 = 5;

/// Wavelength, in world units, of the lowest-frequency octave. At
/// `REGION_SIZE = 256` this puts continental undulation across ~16 regions.
pub const BASE_WAVELENGTH: f64 = 4096.0;

/// Peak-to-mean amplitude, in world units (meters, informally), of the summed
/// noise before possibility scaling.
pub const BASE_AMPLITUDE: f32 = 600.0;

/// Elevation of the sea surface. Terrain below this is open water.
pub const SEA_LEVEL: f32 = 0.0;

/// How far the Planetary (ocean fraction) dimension can shift the effective
/// land height relative to sea level, in world units total swing.
const SEA_SHIFT_RANGE: f32 = 120.0;

/// Deterministic 64-bit seed for the gradient at integer lattice corner
/// `(ix, iy)` of `octave`.
///
/// This is a permanent *identity* (it decides topology), so it is pure integer
/// hashing and is covered by golden fixtures and the native↔wasm parity test
/// (phase-1-plan.md section 11.2). The fold order is part of the stable
/// contract; changing it requires a [`WORLD_ALGORITHM_VERSION`] bump.
#[inline]
#[must_use]
pub const fn gradient_seed(ix: i64, iy: i64, octave: u32) -> u64 {
    let mut h = TERRAIN_BASIS;
    h = mix(h, WORLD_ALGORITHM_VERSION as u64);
    h = mix(h, octave as u64);
    // Signed coordinates fold as their unsigned bit patterns for portability.
    h = mix(h, ix as u64);
    h = mix(h, iy as u64);
    h
}

/// The eight unit gradients. Selection by `seed & 7` keeps the choice a pure
/// integer operation; the constants themselves are exact `f32` literals so the
/// dot products are portable presentation math.
const SQRT_HALF: f32 = core::f32::consts::FRAC_1_SQRT_2;
const GRADIENTS: [(f32, f32); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (SQRT_HALF, SQRT_HALF),
    (-SQRT_HALF, SQRT_HALF),
    (SQRT_HALF, -SQRT_HALF),
    (-SQRT_HALF, -SQRT_HALF),
];

/// Gradient vector at a lattice corner, selected by integer hash.
#[inline]
#[must_use]
fn gradient(ix: i64, iy: i64, octave: u32) -> (f32, f32) {
    GRADIENTS[(gradient_seed(ix, iy, octave) & 7) as usize]
}

/// Perlin's quintic fade: C2-continuous across lattice cell boundaries, which
/// is what keeps the heightfield free of visible grid creases.
#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Single octave of gradient noise at lattice-space position `(x, y)`,
/// roughly in `[-1, 1]`.
fn gradient_noise(x: f64, y: f64, octave: u32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let ix = x0 as i64;
    let iy = y0 as i64;
    // Fractional offsets are small (< 1), so the f64→f32 narrowing is benign.
    let fx = (x - x0) as f32;
    let fy = (y - y0) as f32;

    let dot = |gx: i64, gy: i64, dx: f32, dy: f32| -> f32 {
        let (gxv, gyv) = gradient(gx, gy, octave);
        gxv * dx + gyv * dy
    };
    let n00 = dot(ix, iy, fx, fy);
    let n10 = dot(ix + 1, iy, fx - 1.0, fy);
    let n01 = dot(ix, iy + 1, fx, fy - 1.0);
    let n11 = dot(ix + 1, iy + 1, fx - 1.0, fy - 1.0);

    let u = fade(fx);
    let v = fade(fy);
    let nx0 = n00 + (n10 - n00) * u;
    let nx1 = n01 + (n11 - n01) * u;
    let n = nx0 + (nx1 - nx0) * v;
    // Scale the theoretical ±√½ 2-D Perlin range up to roughly ±1.
    n * core::f32::consts::SQRT_2
}

/// Deterministic lattice offset for an octave, in lattice units.
///
/// Gradient noise is exactly zero at its own lattice corners; without offsets
/// every octave's lattice aligns at multiples of [`BASE_WAVELENGTH`], stamping
/// a regular grid of forced sea-level points across the world. The offsets are
/// derived from the same integer-hash domain as the gradients, so they are
/// part of the reproducible topology.
#[inline]
fn octave_offset(octave: u32) -> (f64, f64) {
    // A distinct corner of the gradient-seed space reserved for offsets.
    let hx = gradient_seed(i64::MIN, 0, octave);
    let hy = gradient_seed(0, i64::MIN, octave);
    // Top 20 bits → [0, 64) lattice units; exact in f64.
    let ox = (hx >> 44) as f64 * (64.0 / (1u64 << 20) as f64);
    let oy = (hy >> 44) as f64 * (64.0 / (1u64 << 20) as f64);
    (ox, oy)
}

/// Fractal Brownian motion: [`OCTAVES`] octaves of gradient noise, halving
/// wavelength and amplitude per octave, normalized to roughly `[-1, 1]`.
#[must_use]
pub fn fbm(world_x: f64, world_y: f64) -> f32 {
    let mut sum = 0.0f32;
    let mut norm = 0.0f32;
    let mut amplitude = 1.0f32;
    let mut wavelength = BASE_WAVELENGTH;
    for octave in 0..OCTAVES {
        let (ox, oy) = octave_offset(octave);
        sum += amplitude
            * gradient_noise(world_x / wavelength + ox, world_y / wavelength + oy, octave);
        norm += amplitude;
        amplitude *= 0.5;
        wavelength *= 0.5;
    }
    sum / norm
}

/// Elevation, in world units relative to [`SEA_LEVEL`], at a continuous world
/// position.
///
/// Presentation state (`f32`) — never an identity. The only possibility inputs
/// are the slow dimensions: Geology scales relief amplitude (tectonic
/// activity), Planetary shifts the land/sea balance (ocean fraction). Both act
/// through smooth linear maps so a drifting vector deforms terrain gently
/// rather than rearranging it.
#[must_use]
pub fn elevation(world_x: f64, world_y: f64, p: &PossibilityVector) -> f32 {
    let relief = fbm(world_x, world_y);
    // Geology 0..1 → amplitude scale 0.5..1.5 (quiet plains ↔ young mountains).
    let tectonic = 0.5 + p.get(PossibilityDomain::Geology);
    // Planetary 0..1 → sea shift −60..+60 (more ocean ↔ more land).
    let sea_shift = (p.get(PossibilityDomain::Planetary) - 0.5) * SEA_SHIFT_RANGE;
    relief * BASE_AMPLITUDE * tectonic - sea_shift
}

/// Whether an elevation is open water.
#[inline]
#[must_use]
pub fn is_water(elevation: f32) -> bool {
    elevation < SEA_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fbm_is_bounded() {
        let p = PossibilityVector::neutral();
        for i in -20..20 {
            for j in -20..20 {
                let x = f64::from(i) * 197.0;
                let y = f64::from(j) * 311.0;
                let n = fbm(x, y);
                assert!(
                    (-1.5..=1.5).contains(&n),
                    "fbm out of range at ({x},{y}): {n}"
                );
                let e = elevation(x, y, &p);
                assert!(e.is_finite());
            }
        }
    }

    #[test]
    fn elevation_couples_weakly_to_fast_dimensions() {
        // Fast dimensions (Climate, Ecology, ...) must not move terrain at all.
        let base = PossibilityVector::neutral();
        let mut fast = base;
        fast.set(PossibilityDomain::Climate, 1.0);
        fast.set(PossibilityDomain::Ecology, 0.0);
        fast.set(PossibilityDomain::Hydrology, 1.0);
        assert_eq!(
            elevation(1234.5, -678.9, &base),
            elevation(1234.5, -678.9, &fast)
        );
    }
}
