//! SIMD ↔ scalar differential tests (phase-6-plan.md §11.2, ADR 0016):
//! seeded randomized inputs plus edge sweeps, asserting **bit equality** of
//! the row kernels against their scalar twins on this platform. These are CI
//! gates: a single differing bit means a kernel stopped being a
//! transcription and became an algorithm change, which is out of scope for
//! Phase 6 by definition.

use world_core::simd::{
    climate_row, climate_row_scalar, elevation_row, elevation_row_scalar, fbm_row, fbm_row_scalar,
    hydrology_row, hydrology_row_scalar, soils_row, soils_row_scalar, vegetation_row,
    vegetation_row_scalar,
};
use world_core::{PossibilityDomain, PossibilityVector, Rng, BIOME_COUNT};

const ROW: usize = 37; // deliberately not a multiple of the lane width

fn assert_rows_bit_equal(simd: &[f32], scalar: &[f32], label: &str) {
    for (i, (a, b)) in simd.iter().zip(scalar).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "{label}: lane {i} differs (simd {a}, scalar {b})"
        );
    }
}

fn possibility(rng: &mut Rng) -> PossibilityVector {
    let mut p = PossibilityVector::neutral();
    for domain in PossibilityDomain::ALL {
        p.set(domain, rng.next_f32());
    }
    p
}

#[test]
fn fbm_and_elevation_rows_are_bit_identical() {
    let mut rng = Rng::new(0x51D_0016);
    for case in 0..64 {
        let base_x = f64::from(rng.next_u32()) * 16.0 - f64::from(u32::MAX) * 8.0;
        let y = f64::from(rng.next_u32()) * 16.0 - f64::from(u32::MAX) * 8.0;
        // Tile-shaped rows (contiguous cells) and scattered rows both count.
        let step = if case % 2 == 0 {
            8.0
        } else {
            f64::from(rng.next_f32()) * 500.0 + 0.25
        };
        let xs: Vec<f64> = (0..ROW).map(|i| base_x + i as f64 * step).collect();
        let mut simd = vec![0f32; ROW];
        let mut scalar = vec![0f32; ROW];
        fbm_row(&xs, y, &mut simd);
        fbm_row_scalar(&xs, y, &mut scalar);
        assert_rows_bit_equal(&simd, &scalar, "fbm_row");

        let p = possibility(&mut rng);
        elevation_row(&xs, y, &p, &mut simd);
        elevation_row_scalar(&xs, y, &p, &mut scalar);
        assert_rows_bit_equal(&simd, &scalar, "elevation_row");
    }
}

#[test]
fn climate_row_is_bit_identical() {
    let mut rng = Rng::new(0xC11_0016);
    for _ in 0..64 {
        let p = possibility(&mut rng);
        // Elevations across the full plausible range, plus exact-boundary and
        // signed-zero edge cases (the sea-level branch).
        let mut elevation: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 2400.0 - 800.0).collect();
        elevation[0] = 0.0;
        elevation[1] = -0.0;
        elevation[2] = f32::MIN_POSITIVE; // denormal boundary
        elevation[3] = -f32::MIN_POSITIVE;
        let (mut ts, mut ms) = (vec![0f32; ROW], vec![0f32; ROW]);
        let (mut tv, mut mv) = (vec![0f32; ROW], vec![0f32; ROW]);
        climate_row_scalar(&elevation, &p, &mut ts, &mut ms);
        climate_row(&elevation, &p, &mut tv, &mut mv);
        assert_rows_bit_equal(&tv, &ts, "climate_row temperature");
        assert_rows_bit_equal(&mv, &ms, "climate_row moisture");
    }
}

#[test]
fn soils_row_is_bit_identical() {
    let mut rng = Rng::new(0x5011_0016);
    for _ in 0..64 {
        let elevation: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 1600.0 - 400.0).collect();
        let slope: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 0.8).collect();
        let hardness: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let lithology: Vec<u8> = (0..ROW).map(|_| (rng.next_below(8)) as u8).collect();
        let temperature: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 60.0 - 20.0).collect();
        let moisture: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let wetness: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let (mut ds, mut fs) = (vec![0f32; ROW], vec![0f32; ROW]);
        let (mut dv, mut fv) = (vec![0f32; ROW], vec![0f32; ROW]);
        soils_row_scalar(
            &elevation,
            &slope,
            &hardness,
            &lithology,
            &temperature,
            &moisture,
            &wetness,
            &mut ds,
            &mut fs,
        );
        soils_row(
            &elevation,
            &slope,
            &hardness,
            &lithology,
            &temperature,
            &moisture,
            &wetness,
            &mut dv,
            &mut fv,
        );
        assert_rows_bit_equal(&dv, &ds, "soils_row depth");
        assert_rows_bit_equal(&fv, &fs, "soils_row fertility");
    }
}

#[test]
fn hydrology_row_is_bit_identical() {
    let mut rng = Rng::new(0x44D_0016);
    for _ in 0..64 {
        let elevation: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 1600.0 - 400.0).collect();
        let slope: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 0.5).collect();
        // Accumulation sweep includes the source threshold (exact tie), the
        // saturation knee, and far beyond.
        let mut accum: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 800.0).collect();
        accum[0] = 0.0;
        accum[1] = 5.0; // == RIVER_SOURCE_ACCUM: the branch boundary
        accum[2] = 400.0; // == RIVER_SATURATION_ACCUM
        accum[3] = 1.0e6;
        let temperature: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 50.0 - 10.0).collect();
        let moisture: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let ph = rng.next_f32();
        let pp = rng.next_f32();
        let (mut rs, mut ws) = (vec![0f32; ROW], vec![0f32; ROW]);
        let (mut rv, mut wv) = (vec![0f32; ROW], vec![0f32; ROW]);
        hydrology_row_scalar(
            &elevation,
            &slope,
            &accum,
            &temperature,
            &moisture,
            ph,
            pp,
            &mut rs,
            &mut ws,
        );
        hydrology_row(
            &elevation,
            &slope,
            &accum,
            &temperature,
            &moisture,
            ph,
            pp,
            &mut rv,
            &mut wv,
        );
        assert_rows_bit_equal(&rv, &rs, "hydrology_row river");
        assert_rows_bit_equal(&wv, &ws, "hydrology_row wetness");
    }
}

#[test]
fn vegetation_row_is_bit_identical() {
    let mut rng = Rng::new(0x0EC0_0016);
    for _ in 0..64 {
        let mut biome: Vec<u8> = (0..ROW)
            .map(|_| rng.next_below(BIOME_COUNT as u32) as u8)
            .collect();
        for id in 0..BIOME_COUNT as u8 {
            biome[usize::from(id)] = id;
        }
        let temperature: Vec<f32> = (0..ROW).map(|_| rng.next_f32() * 60.0 - 20.0).collect();
        let moisture: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let depth: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let fertility: Vec<f32> = (0..ROW).map(|_| rng.next_f32()).collect();
        let pe = rng.next_f32();
        let (mut ds, mut cs) = (vec![0f32; ROW], vec![0f32; ROW]);
        let (mut dv, mut cv) = (vec![0f32; ROW], vec![0f32; ROW]);
        vegetation_row_scalar(
            &biome,
            &temperature,
            &moisture,
            &depth,
            &fertility,
            pe,
            &mut ds,
            &mut cs,
        );
        vegetation_row(
            &biome,
            &temperature,
            &moisture,
            &depth,
            &fertility,
            pe,
            &mut dv,
            &mut cv,
        );
        assert_rows_bit_equal(&dv, &ds, "vegetation_row density");
        assert_rows_bit_equal(&cv, &cs, "vegetation_row canopy");
    }
}
