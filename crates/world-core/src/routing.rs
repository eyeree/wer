//! Identity-grade fixed-point elevation for macro drainage (ADR 0027).
//!
//! This module deliberately contains no floating-point types, conversions,
//! comparisons, or rounding. Q30 values are stored in `i64`; every product,
//! weighted sum, and unit conversion uses `i128` and the one documented signed
//! rounding rule: nearest, with exact ties away from zero.

use crate::possibility::PossibilityDomain;
use crate::terrain::{gradient_seed, OCTAVES};
use crate::{PossibilityField, RegionCoord};

const Q: u32 = 30;
const ONE: i64 = 1_i64 << Q;
const HALF: i64 = ONE / 2;
const SQRT_2: i64 = 1_518_500_250;
const FRAC_1_SQRT_2: i64 = 759_250_125;

#[inline]
fn round_div_signed(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    if numerator == 0 {
        return 0;
    }
    let negative = numerator < 0;
    let magnitude = numerator.unsigned_abs();
    let denominator = denominator as u128;
    let quotient = magnitude / denominator;
    let remainder = magnitude % denominator;
    let rounded = quotient + u128::from(remainder >= denominator - remainder);
    if negative {
        -(rounded as i128)
    } else {
        rounded as i128
    }
}

#[inline]
fn narrow_q30(value: i128) -> i64 {
    i64::try_from(value).expect("bounded routing Q30 intermediate")
}

#[inline]
fn mul_q30(a: i64, b: i64) -> i64 {
    narrow_q30(round_div_signed(
        i128::from(a) * i128::from(b),
        i128::from(ONE),
    ))
}

#[inline]
fn lerp_q30(a: i64, b: i64, t: i64) -> i64 {
    a + mul_q30(b - a, t)
}

#[inline]
fn fade_q30(t: i64) -> i64 {
    let t2 = mul_q30(t, t);
    let t3 = mul_q30(t2, t);
    let inner = mul_q30(t, t.saturating_mul(6) - ONE.saturating_mul(15)) + ONE.saturating_mul(10);
    mul_q30(t3, inner)
}

#[inline]
fn gradient(octave: u32, ix: i64, iy: i64) -> (i64, i64) {
    match gradient_seed(ix, iy, octave) & 7 {
        0 => (ONE, 0),
        1 => (-ONE, 0),
        2 => (0, ONE),
        3 => (0, -ONE),
        4 => (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        5 => (-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
        6 => (FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
        _ => (-FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    }
}

#[inline]
fn octave_offset_q30(octave: u32) -> (i64, i64) {
    let hx = gradient_seed(i64::MIN, 0, octave) >> 44;
    let hy = gradient_seed(0, i64::MIN, octave) >> 44;
    ((hx << 16) as i64, (hy << 16) as i64)
}

fn gradient_noise_q30(x: i64, y: i64, octave: u32) -> i64 {
    let ix = x.div_euclid(ONE);
    let iy = y.div_euclid(ONE);
    let fx = x.rem_euclid(ONE);
    let fy = y.rem_euclid(ONE);
    let dot = |gx: i64, gy: i64, dx: i64, dy: i64| {
        let (gradient_x, gradient_y) = gradient(octave, gx, gy);
        narrow_q30(round_div_signed(
            i128::from(gradient_x) * i128::from(dx) + i128::from(gradient_y) * i128::from(dy),
            i128::from(ONE),
        ))
    };
    let n00 = dot(ix, iy, fx, fy);
    let n10 = dot(ix + 1, iy, fx - ONE, fy);
    let n01 = dot(ix, iy + 1, fx, fy - ONE);
    let n11 = dot(ix + 1, iy + 1, fx - ONE, fy - ONE);
    let nx0 = lerp_q30(n00, n10, fade_q30(fx));
    let nx1 = lerp_q30(n01, n11, fade_q30(fx));
    mul_q30(lerp_q30(nx0, nx1, fade_q30(fy)), SQRT_2)
}

fn relief_q30(region_x: i32, region_y: i32) -> i64 {
    let twice_x = i64::from(region_x) * 2 + 1;
    let twice_y = i64::from(region_y) * 2 + 1;
    let mut weighted = 0_i128;
    for octave in 0..OCTAVES {
        // Region-center lattice coordinate is (2r+1)*2^octave/32. In Q30
        // that is an exact shift by octave+25.
        let shift = octave + 25;
        let (offset_x, offset_y) = octave_offset_q30(octave);
        let x = twice_x
            .checked_mul(1_i64 << shift)
            .and_then(|value| value.checked_add(offset_x))
            .expect("routing coordinate remains in Q30 range");
        let y = twice_y
            .checked_mul(1_i64 << shift)
            .and_then(|value| value.checked_add(offset_y))
            .expect("routing coordinate remains in Q30 range");
        let weight = 1_i128 << (4 - octave);
        weighted += i128::from(gradient_noise_q30(x, y, octave)) * weight;
    }
    narrow_q30(round_div_signed(weighted, 31))
}

#[inline]
fn bucket_center_q30(bucket: u16) -> i64 {
    i64::from(2 * u32::from(bucket) + 1) << 17
}

/// Integer-only routing elevation of one level-0 region center, in
/// centimeters. This is the sole elevation source for drainage direction and
/// accumulation ordering.
#[must_use]
pub fn routing_elevation_cm(field: &PossibilityField, region_x: i32, region_y: i32) -> i32 {
    let region = RegionCoord::new(region_x, region_y);
    let planetary = bucket_center_q30(field.routing_bucket(region, PossibilityDomain::Planetary));
    let geology = bucket_center_q30(field.routing_bucket(region, PossibilityDomain::Geology));
    let relief = relief_q30(region_x, region_y);
    let tectonic = HALF + geology;
    let relief_scaled = mul_q30(relief, tectonic);
    let elevation_q30 = i128::from(relief_scaled) * 600 - i128::from(planetary - HALF) * 120;
    let centimeters = round_div_signed(elevation_q30 * 100, i128::from(ONE));
    i32::try_from(centimeters).expect("routing elevation fits i32 centimeters")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_rounding_is_nearest_ties_away_from_zero() {
        assert_eq!(round_div_signed(14, 10), 1);
        assert_eq!(round_div_signed(15, 10), 2);
        assert_eq!(round_div_signed(-14, 10), -1);
        assert_eq!(round_div_signed(-15, 10), -2);
    }

    #[test]
    fn fixed_constants_are_pinned() {
        assert_eq!(SQRT_2, 1_518_500_250);
        assert_eq!(FRAC_1_SQRT_2, 759_250_125);
        assert_eq!(mul_q30(SQRT_2, FRAC_1_SQRT_2), ONE);
    }

    #[test]
    fn routing_elevation_covers_negative_and_custom_fields() {
        let cases = [
            (
                PossibilityField::default(),
                0,
                0,
                778,
                564,
                39_293_125,
                5_120,
            ),
            (
                PossibilityField::default(),
                -1,
                -1,
                857,
                904,
                48_591_714,
                5_445,
            ),
            (
                PossibilityField::new(7),
                14,
                -15,
                182,
                3_449,
                -8_993_522,
                4_791,
            ),
            (
                PossibilityField::new(1),
                -127,
                255,
                419,
                2_143,
                -78_486_388,
                283,
            ),
            (
                PossibilityField::new(1),
                i32::MAX - 1,
                i32::MIN + 1,
                2_372,
                3_941,
                -203_534_703,
                -17_582,
            ),
            (
                PossibilityField::new(u32::MAX),
                i32::MIN + 1,
                i32::MAX - 1,
                1_429,
                1_950,
                -13_102_647,
                1_097,
            ),
        ];
        for (field, x, y, planetary, geology, relief, centimeters) in cases {
            assert_eq!(
                field.routing_bucket(RegionCoord::new(x, y), PossibilityDomain::Planetary),
                planetary,
            );
            assert_eq!(
                field.routing_bucket(RegionCoord::new(x, y), PossibilityDomain::Geology),
                geology,
            );
            assert_eq!(relief_q30(x, y), relief);
            assert_eq!(routing_elevation_cm(&field, x, y), centimeters);
        }
    }
}
