//! Portable SIMD row kernels and their scalar twins (phase-6-plan.md §6.1,
//! ADR 0016).
//!
//! **The bit-identity contract (ADR 0016):** every SIMD kernel here is a
//! *lane-parallel transcription* of the corresponding scalar kernel — the
//! same operations, in the same order, per cell. No FMA contraction (`wide`
//! never fuses and nothing here calls `mul_add`), no reassociation, no
//! fast-math, no cross-lane operations in `f32` paths, and transcendentals
//! (`ln` in the river-width curve) run *scalar per lane* through the very
//! same function the scalar kernel calls. Same-platform, same-input outputs
//! are **bit-equal** to the scalar twin — asserted by the differential tests
//! in `tests/simd_differential.rs`, and by every golden fixture, which pass
//! unchanged by definition of the rule. An optimization that cannot meet
//! bit-identity is an algorithm change and belongs to a versioned
//! `algorithm_revision` bump in a later phase, not here.
//!
//! The scalar twin is never deleted: it is the spec, the tail path for rows
//! not divisible by the lane width, and the wasm fallback (`wide` compiles
//! to scalar code without `simd128`, so the browser build is clean and
//! correct without any feature flag).
//!
//! Clamp/select points are transcribed with explicit compare + blend, which
//! mirrors the scalar branch semantics exactly (including `-0.0`, which
//! `max`-based clamping would silently normalize).

use wide::{f32x8, CmpGt, CmpLt};

use crate::biome::Biome;
use crate::climate::{LAPSE_RATE, TEMPERATURE_RANGE};
use crate::hydrology::river_intensity;
use crate::possibility::{PossibilityDomain, PossibilityVector};
use crate::terrain::{fade, fbm, gradient, octave_offset, BASE_WAVELENGTH, OCTAVES, SEA_LEVEL};
use crate::vegetation::biome_base;

/// f32 lanes per vector step.
const LANES: usize = 8;

/// Load 8 lanes from a slice (wide converts from arrays only).
#[inline]
#[must_use]
fn load8(s: &[f32]) -> f32x8 {
    let mut a = [0f32; LANES];
    a.copy_from_slice(&s[..LANES]);
    f32x8::from(a)
}

/// Transcription of `f32::clamp(v, lo, hi)`'s branch semantics:
/// `if v < lo { lo } else if v > hi { hi } else { v }`.
#[inline]
#[must_use]
fn clamp_v(v: f32x8, lo: f32x8, hi: f32x8) -> f32x8 {
    v.cmp_lt(lo).blend(lo, v.cmp_gt(hi).blend(hi, v))
}

/// Transcription of `.max(0.0)` as used by the scalar kernels on finite
/// inputs: `if v > 0.0 { v } else { 0.0 }` — for finite non-zero values this
/// is exactly `f32::max(v, 0.0)`; at `±0.0` it returns `+0.0`, matching the
/// x86 `maxss(v, 0.0)` lowering of the scalar call.
#[inline]
#[must_use]
fn max_zero(v: f32x8) -> f32x8 {
    let zero = f32x8::splat(0.0);
    v.cmp_gt(zero).blend(v, zero)
}

// ---------------------------------------------------------------------------
// Terrain fBm (the top kernel: 5 octaves × 4 hashed corner gradients/cell).
// ---------------------------------------------------------------------------

/// Scalar twin of [`fbm_row`]: the per-cell kernel, verbatim.
pub fn fbm_row_scalar(xs: &[f64], world_y: f64, out: &mut [f32]) {
    for (o, &x) in out.iter_mut().zip(xs) {
        *o = fbm(x, world_y);
    }
}

/// [`crate::terrain::fbm`] across a row of cells at constant `world_y`
/// (phase-6-plan.md §6.1 kernel 1).
///
/// Vectorizes the per-cell `f32` interpolation math four cells at a time and
/// memoizes the integer-hashed lattice gradients per row (identical values,
/// fetched through the identical hash — a same-math cache, not an
/// approximation): at tile scale a whole row shares one or two lattice
/// columns per octave, so the 4-hashes-per-cell-per-octave cost collapses to
/// a handful per row. The `f64` lattice-position math stays scalar per lane
/// — the same expressions the scalar kernel evaluates.
pub fn fbm_row(xs: &[f64], world_y: f64, out: &mut [f32]) {
    use wide::f32x4;
    debug_assert_eq!(xs.len(), out.len());
    let n4 = xs.len() / 4 * 4;
    let mut xi = 0usize;
    while xi < n4 {
        let lanes = &xs[xi..xi + 4];
        let mut sum = f32x4::splat(0.0);
        let mut norm = 0.0f32;
        let mut amplitude = 1.0f32;
        let mut wavelength = BASE_WAVELENGTH;
        for octave in 0..OCTAVES {
            let (ox, oy) = octave_offset(octave);
            // The y side is shared by the whole row; computing it once per
            // octave yields the same bits the scalar kernel computes per cell.
            let v_pos = world_y / wavelength + oy;
            let y0 = v_pos.floor();
            let iy = y0 as i64;
            let fy = (v_pos - y0) as f32;

            let mut fx = [0f32; 4];
            let mut g00x = [0f32; 4];
            let mut g00y = [0f32; 4];
            let mut g10x = [0f32; 4];
            let mut g10y = [0f32; 4];
            let mut g01x = [0f32; 4];
            let mut g01y = [0f32; 4];
            let mut g11x = [0f32; 4];
            let mut g11y = [0f32; 4];
            let mut cached_ix = i64::MIN;
            let mut cg = [(0f32, 0f32); 4];
            for (lane, &x) in lanes.iter().enumerate() {
                // Identical to the scalar kernel's per-cell lattice math.
                let u_pos = x / wavelength + ox;
                let x0 = u_pos.floor();
                let ix = x0 as i64;
                fx[lane] = (u_pos - x0) as f32;
                if ix != cached_ix {
                    cached_ix = ix;
                    cg = [
                        gradient(ix, iy, octave),
                        gradient(ix + 1, iy, octave),
                        gradient(ix, iy + 1, octave),
                        gradient(ix + 1, iy + 1, octave),
                    ];
                }
                g00x[lane] = cg[0].0;
                g00y[lane] = cg[0].1;
                g10x[lane] = cg[1].0;
                g10y[lane] = cg[1].1;
                g01x[lane] = cg[2].0;
                g01y[lane] = cg[2].1;
                g11x[lane] = cg[3].0;
                g11y[lane] = cg[3].1;
            }

            let fxv = f32x4::from(fx);
            let fyv = f32x4::splat(fy);
            let one = f32x4::splat(1.0);
            // Same operation sequence as the scalar `dot` calls.
            let n00 = f32x4::from(g00x) * fxv + f32x4::from(g00y) * fyv;
            let n10 = f32x4::from(g10x) * (fxv - one) + f32x4::from(g10y) * fyv;
            let n01 = f32x4::from(g01x) * fxv + f32x4::from(g01y) * (fyv - one);
            let n11 = f32x4::from(g11x) * (fxv - one) + f32x4::from(g11y) * (fyv - one);

            // fade(t) = t*t*t*(t*(t*6-15)+10), transcribed op for op.
            let u = {
                let t = fxv;
                t * t * t * (t * (t * f32x4::splat(6.0) - f32x4::splat(15.0)) + f32x4::splat(10.0))
            };
            let v = fade(fy);

            let nx0 = n00 + (n10 - n00) * u;
            let nx1 = n01 + (n11 - n01) * u;
            let n = nx0 + (nx1 - nx0) * f32x4::splat(v);
            let n = n * f32x4::splat(core::f32::consts::SQRT_2);

            sum += f32x4::splat(amplitude) * n;
            norm += amplitude;
            amplitude *= 0.5;
            wavelength *= 0.5;
        }
        let result = (sum / f32x4::splat(norm)).to_array();
        out[xi..xi + 4].copy_from_slice(&result);
        xi += 4;
    }
    // Tail cells run the scalar twin.
    fbm_row_scalar(&xs[n4..], world_y, &mut out[n4..]);
}

/// Scalar twin of [`elevation_row`].
pub fn elevation_row_scalar(xs: &[f64], world_y: f64, p: &PossibilityVector, out: &mut [f32]) {
    for (o, &x) in out.iter_mut().zip(xs) {
        *o = crate::terrain::elevation(x, world_y, p);
    }
}

/// [`crate::terrain::elevation`] across a row: [`fbm_row`] plus the scalar
/// possibility scaling, in the scalar kernel's exact expression order.
pub fn elevation_row(xs: &[f64], world_y: f64, p: &PossibilityVector, out: &mut [f32]) {
    fbm_row(xs, world_y, out);
    for o in out.iter_mut() {
        *o = crate::terrain::elevation_from_relief(*o, p);
    }
}

// ---------------------------------------------------------------------------
// Climate (kernel 2: pure per-cell arithmetic).
// ---------------------------------------------------------------------------

/// Scalar twin of [`climate_row`].
pub fn climate_row_scalar(
    elevation: &[f32],
    p: &PossibilityVector,
    temperature: &mut [f32],
    moisture: &mut [f32],
) {
    for ((t, m), &e) in temperature
        .iter_mut()
        .zip(moisture.iter_mut())
        .zip(elevation)
    {
        let c = crate::climate::climate(e, p);
        *t = c.temperature;
        *m = c.moisture;
    }
}

/// [`crate::climate::climate`] across a row.
pub fn climate_row(
    elevation: &[f32],
    p: &PossibilityVector,
    temperature: &mut [f32],
    moisture: &mut [f32],
) {
    let (t_min, t_max) = TEMPERATURE_RANGE;
    let base = t_min + (t_max - t_min) * p.get(PossibilityDomain::Climate);
    let supply = 0.15
        + 0.55 * p.get(PossibilityDomain::Hydrology)
        + 0.30 * p.get(PossibilityDomain::Planetary);
    const MOISTURE_LAPSE: f32 = 8.0e-4;

    let n8 = elevation.len() / LANES * LANES;
    let mut i = 0usize;
    while i < n8 {
        let e = load8(&elevation[i..i + LANES]);
        let sea = f32x8::splat(SEA_LEVEL);
        let above = max_zero(e - sea);
        let t = f32x8::splat(base) - f32x8::splat(LAPSE_RATE) * above;
        let land = clamp_v(
            f32x8::splat(supply) - f32x8::splat(MOISTURE_LAPSE) * above,
            f32x8::splat(0.0),
            f32x8::splat(1.0),
        );
        let m = e.cmp_lt(sea).blend(f32x8::splat(1.0), land);
        temperature[i..i + LANES].copy_from_slice(&t.to_array());
        moisture[i..i + LANES].copy_from_slice(&m.to_array());
        i += LANES;
    }
    climate_row_scalar(
        &elevation[n8..],
        p,
        &mut temperature[n8..],
        &mut moisture[n8..],
    );
}

// ---------------------------------------------------------------------------
// Soils (kernel 2 family).
// ---------------------------------------------------------------------------

/// Scalar twin of [`soils_row`].
#[allow(clippy::too_many_arguments)]
pub fn soils_row_scalar(
    elevation: &[f32],
    slope: &[f32],
    hardness: &[f32],
    lithology: &[u8],
    temperature: &[f32],
    moisture: &[f32],
    wetness: &[f32],
    depth_out: &mut [f32],
    fertility_out: &mut [f32],
) {
    for i in 0..elevation.len() {
        let g = crate::geology::Geology {
            lithology: lithology[i],
            hardness: hardness[i],
        };
        let c = crate::climate::Climate {
            temperature: temperature[i],
            moisture: moisture[i],
        };
        let h = crate::hydrology::Hydrology {
            river: 0.0,
            wetness: wetness[i],
        };
        let s = crate::soils::soils(elevation[i], slope[i], &g, &c, &h);
        depth_out[i] = s.depth;
        fertility_out[i] = s.fertility;
    }
}

/// [`crate::soils::soils`] across a row (the river input is unread by the
/// kernel, so only wetness rides in).
#[allow(clippy::too_many_arguments)]
pub fn soils_row(
    elevation: &[f32],
    slope: &[f32],
    hardness: &[f32],
    lithology: &[u8],
    temperature: &[f32],
    moisture: &[f32],
    wetness: &[f32],
    depth_out: &mut [f32],
    fertility_out: &mut [f32],
) {
    const BARE_SLOPE: f32 = 0.4;
    const FERTILE_TEMPERATURE: f32 = 15.0;
    const FERTILE_TOLERANCE: f32 = 25.0;
    let zero = f32x8::splat(0.0);
    let one = f32x8::splat(1.0);

    let n8 = elevation.len() / LANES * LANES;
    let mut i = 0usize;
    while i < n8 {
        let e = load8(&elevation[i..i + LANES]);
        let sl = load8(&slope[i..i + LANES]);
        let hard = load8(&hardness[i..i + LANES]);
        let tc = load8(&temperature[i..i + LANES]);
        let m = load8(&moisture[i..i + LANES]);
        let wet = load8(&wetness[i..i + LANES]);
        let mut lith = [0f32; LANES];
        for (l, &id) in lith.iter_mut().zip(&lithology[i..i + LANES]) {
            *l = f32::from(id);
        }
        let lith_bias =
            f32x8::splat(0.85) + f32x8::splat(0.30) * f32x8::from(lith) / f32x8::splat(7.0);

        let flatness = one - clamp_v(sl / f32x8::splat(BARE_SLOPE), zero, one);
        let softness = one - f32x8::splat(0.7) * hard;
        let depth = clamp_v(flatness * softness + f32x8::splat(0.25) * wet, zero, one);

        let t = (tc - f32x8::splat(FERTILE_TEMPERATURE)) / f32x8::splat(FERTILE_TOLERANCE);
        let warmth = max_zero(one - t * t);
        let fertility = clamp_v(
            depth.sqrt() * (f32x8::splat(0.3) + f32x8::splat(0.7) * m) * warmth * lith_bias,
            zero,
            one,
        );

        // Underwater is bare (the scalar early return).
        let sea_mask = e.cmp_lt(f32x8::splat(SEA_LEVEL));
        let depth = sea_mask.blend(zero, depth);
        let fertility = sea_mask.blend(zero, fertility);
        depth_out[i..i + LANES].copy_from_slice(&depth.to_array());
        fertility_out[i..i + LANES].copy_from_slice(&fertility.to_array());
        i += LANES;
    }
    soils_row_scalar(
        &elevation[n8..],
        &slope[n8..],
        &hardness[n8..],
        &lithology[n8..],
        &temperature[n8..],
        &moisture[n8..],
        &wetness[n8..],
        &mut depth_out[n8..],
        &mut fertility_out[n8..],
    );
}

// ---------------------------------------------------------------------------
// Hydrology (kernel 3: arithmetic vectorized, `ln`/`sqrt` of the width curve
// scalar per lane through the same function).
// ---------------------------------------------------------------------------

/// Scalar twin of [`hydrology_row`].
#[allow(clippy::too_many_arguments)]
pub fn hydrology_row_scalar(
    elevation: &[f32],
    slope: &[f32],
    accum: &[f32],
    temperature: &[f32],
    moisture: &[f32],
    p_hydrology: f32,
    p_planetary: f32,
    river_out: &mut [f32],
    wetness_out: &mut [f32],
) {
    for i in 0..elevation.len() {
        let c = crate::climate::Climate {
            temperature: temperature[i],
            moisture: moisture[i],
        };
        let h = crate::hydrology::hydrology(
            elevation[i],
            slope[i],
            accum[i],
            &c,
            p_hydrology,
            p_planetary,
        );
        river_out[i] = h.river;
        wetness_out[i] = h.wetness;
    }
}

/// [`crate::hydrology::hydrology`] across a row. The logarithmic width curve
/// runs scalar per lane through [`river_intensity`] itself — slower than a
/// vector-math approximation, but bit-identical, which is the rule
/// (ADR 0016).
#[allow(clippy::too_many_arguments)]
pub fn hydrology_row(
    elevation: &[f32],
    slope: &[f32],
    accum: &[f32],
    temperature: &[f32],
    moisture: &[f32],
    p_hydrology: f32,
    p_planetary: f32,
    river_out: &mut [f32],
    wetness_out: &mut [f32],
) {
    const PONDING_SLOPE: f32 = 0.05;
    let zero = f32x8::splat(0.0);
    let one = f32x8::splat(1.0);

    let n8 = elevation.len() / LANES * LANES;
    let mut i = 0usize;
    while i < n8 {
        let e = load8(&elevation[i..i + LANES]);
        let sl = load8(&slope[i..i + LANES]);
        let m = load8(&moisture[i..i + LANES]);
        let mut width = [0f32; LANES];
        for (w, &a) in width.iter_mut().zip(&accum[i..i + LANES]) {
            *w = river_intensity(a);
        }
        let width = f32x8::from(width);

        let river = clamp_v(
            width
                * (f32x8::splat(0.55) + f32x8::splat(0.45) * m)
                * (f32x8::splat(0.6) + f32x8::splat(0.8) * f32x8::splat(p_hydrology)),
            zero,
            one,
        );
        let ponding = clamp_v(one - sl / f32x8::splat(PONDING_SLOPE), zero, one);
        let wetness = clamp_v(
            f32x8::splat(0.40) * m
                + f32x8::splat(0.30) * river
                + f32x8::splat(0.20) * ponding * (f32x8::splat(0.3) + f32x8::splat(0.7) * m)
                + f32x8::splat(0.15) * f32x8::splat(p_hydrology)
                + f32x8::splat(0.05) * f32x8::splat(p_planetary),
            zero,
            one,
        );

        // Open water saturates (the scalar early return).
        let sea_mask = e.cmp_lt(f32x8::splat(SEA_LEVEL));
        let river = sea_mask.blend(zero, river);
        let wetness = sea_mask.blend(one, wetness);
        river_out[i..i + LANES].copy_from_slice(&river.to_array());
        wetness_out[i..i + LANES].copy_from_slice(&wetness.to_array());
        i += LANES;
    }
    hydrology_row_scalar(
        &elevation[n8..],
        &slope[n8..],
        &accum[n8..],
        &temperature[n8..],
        &moisture[n8..],
        p_hydrology,
        p_planetary,
        &mut river_out[n8..],
        &mut wetness_out[n8..],
    );
}

// ---------------------------------------------------------------------------
// Vegetation (kernel 2 family; biome base looked up scalar per lane).
// ---------------------------------------------------------------------------

/// Scalar twin of [`vegetation_row`].
#[allow(clippy::too_many_arguments)]
pub fn vegetation_row_scalar(
    biome: &[u8],
    temperature: &[f32],
    moisture: &[f32],
    depth: &[f32],
    fertility: &[f32],
    p_ecology: f32,
    density_out: &mut [f32],
    canopy_out: &mut [f32],
) {
    for i in 0..biome.len() {
        let c = crate::climate::Climate {
            temperature: temperature[i],
            moisture: moisture[i],
        };
        let s = crate::soils::Soils {
            depth: depth[i],
            fertility: fertility[i],
        };
        let v = crate::vegetation::vegetation(Biome::from_id(biome[i]), &c, &s, p_ecology);
        density_out[i] = v.density;
        canopy_out[i] = v.canopy_height;
    }
}

/// [`crate::vegetation::vegetation`] across a row.
#[allow(clippy::too_many_arguments)]
pub fn vegetation_row(
    biome: &[u8],
    temperature: &[f32],
    moisture: &[f32],
    depth: &[f32],
    fertility: &[f32],
    p_ecology: f32,
    density_out: &mut [f32],
    canopy_out: &mut [f32],
) {
    const CANOPY_TEMPERATURE: f32 = 16.0;
    const CANOPY_TOLERANCE: f32 = 28.0;
    const CANOPY_FULL_SOIL: f32 = 0.5;
    let zero = f32x8::splat(0.0);
    let one = f32x8::splat(1.0);

    let n8 = biome.len() / LANES * LANES;
    let mut i = 0usize;
    while i < n8 {
        let tc = load8(&temperature[i..i + LANES]);
        let m = load8(&moisture[i..i + LANES]);
        let d = load8(&depth[i..i + LANES]);
        let f = load8(&fertility[i..i + LANES]);
        let mut base_d = [0f32; LANES];
        let mut base_c = [0f32; LANES];
        for (lane, &id) in biome[i..i + LANES].iter().enumerate() {
            let (bd, bc) = biome_base(Biome::from_id(id));
            base_d[lane] = bd;
            base_c[lane] = bc;
        }
        let base_d = f32x8::from(base_d);
        let base_c = f32x8::from(base_c);

        let density = clamp_v(
            (base_d
                * (f32x8::splat(0.4) + f32x8::splat(0.6) * f)
                * (f32x8::splat(0.5) + f32x8::splat(p_ecology)))
            .min(m + f32x8::splat(0.1)),
            zero,
            one,
        );

        let t = (tc - f32x8::splat(CANOPY_TEMPERATURE)) / f32x8::splat(CANOPY_TOLERANCE);
        let warmth = max_zero(one - t * t);
        let rooting = clamp_v(d / f32x8::splat(CANOPY_FULL_SOIL), zero, one);
        let canopy = base_c * rooting * warmth * (f32x8::splat(0.5) + f32x8::splat(0.5) * density);

        density_out[i..i + LANES].copy_from_slice(&density.to_array());
        canopy_out[i..i + LANES].copy_from_slice(&canopy.to_array());
        i += LANES;
    }
    vegetation_row_scalar(
        &biome[n8..],
        &temperature[n8..],
        &moisture[n8..],
        &depth[n8..],
        &fertility[n8..],
        p_ecology,
        &mut density_out[n8..],
        &mut canopy_out[n8..],
    );
}
