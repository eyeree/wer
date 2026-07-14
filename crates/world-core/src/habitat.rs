//! The habitat signature: the environmental class a species roster is a
//! function of (phase-3-plan.md §4.1, ADR 0010).
//!
//! A [`HabitatSignature`] is a compact, coarse classification of a cell's
//! environment — biome plus coarsely-banded temperature, moisture, and
//! fertility. Rosters and food webs are memoized by signature (phase-3-plan.md
//! §6.3), so the coarse banding is what makes the world read as ecologically
//! *zoned* (an entire biome at similar climate draws from one roster) and keeps
//! the roster cache bounded (`≤ Biome × band³` entries).
//!
//! Like biome ids (phase-2-plan.md §7.6), the signature a cell *derives* is
//! **presentation-grade**: [`HabitatSignature::of`] reads `f32` climate/soil
//! tiles, so knife-edge cells may band differently across platforms. The
//! portable surface Phase 3 guarantees is everything *downstream of a
//! signature* — [`HabitatSignature::seed`], the genome, `species_seed` — which
//! are pure integer functions of the (integer) signature (§9.3). Phase 5 makes
//! the classification itself portable by quantizing the inputs before hashing
//! (ADR 0010).

use crate::biome::Biome;
use crate::climate::Climate;
use crate::hash::mix;
use crate::soils::Soils;
use crate::WORLD_ALGORITHM_VERSION;

/// Fixed basis separating habitat hashing from every other hash domain.
const HABITAT_BASIS: u64 = 0x48A1_7B0C_5E39_D264;

/// Number of temperature bands the signature quantizes into.
pub const TEMPERATURE_BANDS: u8 = 6;
/// Number of moisture bands the signature quantizes into.
pub const MOISTURE_BANDS: u8 = 5;
/// Number of fertility bands the signature quantizes into.
pub const FERTILITY_BANDS: u8 = 4;

/// Lowest temperature (°C) the banding resolves; anything colder saturates to
/// band 0.
const TEMPERATURE_MIN: f32 = -20.0;
/// Highest temperature (°C) the banding resolves; anything warmer saturates to
/// the top band.
const TEMPERATURE_MAX: f32 = 40.0;

/// The environmental class a species roster is a function of. Coarse on
/// purpose (phase-3-plan.md §4.1): nearby cells share a signature so rosters
/// are shared and visible zonation emerges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct HabitatSignature {
    /// [`Biome::id`] of the cell.
    pub biome: u8,
    /// Quantized climate temperature, in `[0, TEMPERATURE_BANDS)`.
    pub temperature_band: u8,
    /// Quantized climate moisture, in `[0, MOISTURE_BANDS)`.
    pub moisture_band: u8,
    /// Quantized soil fertility, in `[0, FERTILITY_BANDS)`.
    pub fertility_band: u8,
}

/// Quantize a value in `[lo, hi]` into `[0, bands)`, saturating outside the
/// range. Boundary values land in the upper band (floor semantics), matching
/// the possibility quantizer (phase-2-plan.md §4.2).
#[inline]
#[must_use]
fn band(value: f32, lo: f32, hi: f32, bands: u8) -> u8 {
    let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    ((t * f32::from(bands)) as u8).min(bands - 1)
}

impl HabitatSignature {
    /// The temperature band for a temperature in °C.
    #[inline]
    #[must_use]
    pub fn temperature_band(temperature: f32) -> u8 {
        band(
            temperature,
            TEMPERATURE_MIN,
            TEMPERATURE_MAX,
            TEMPERATURE_BANDS,
        )
    }

    /// The moisture band for a moisture in `[0, 1]`.
    #[inline]
    #[must_use]
    pub fn moisture_band(moisture: f32) -> u8 {
        band(moisture, 0.0, 1.0, MOISTURE_BANDS)
    }

    /// The fertility band for a fertility in `[0, 1]`.
    #[inline]
    #[must_use]
    pub fn fertility_band(fertility: f32) -> u8 {
        band(fertility, 0.0, 1.0, FERTILITY_BANDS)
    }

    /// Classify a cell's environment into a signature.
    ///
    /// Presentation-grade: reads `f32` climate/soil tiles, so a cell exactly on
    /// a band boundary may classify differently across platforms — the same
    /// residual biome classification already has (ADR 0010).
    #[must_use]
    pub fn of(biome: Biome, c: &Climate, s: &Soils) -> Self {
        Self {
            biome: biome.id(),
            temperature_band: Self::temperature_band(c.temperature),
            moisture_band: Self::moisture_band(c.moisture),
            fertility_band: Self::fertility_band(s.fertility),
        }
    }

    /// The biome this signature classifies.
    #[inline]
    #[must_use]
    pub const fn biome(&self) -> Biome {
        Biome::from_id(self.biome)
    }

    /// The 64-bit seed a roster for this habitat derives from.
    ///
    /// Portable given a signature — the fold is pure integer hashing under a
    /// fixed basis, cross-platform golden-fixtured (§9.3). The *signature
    /// itself* is presentation-grade ([`Self::of`]); the seed derivation from
    /// it is not.
    #[inline]
    #[must_use]
    pub const fn seed(&self) -> u64 {
        let mut h = HABITAT_BASIS;
        h = mix(h, WORLD_ALGORITHM_VERSION as u64);
        h = mix(h, self.biome as u64);
        h = mix(h, self.temperature_band as u64);
        h = mix(h, self.moisture_band as u64);
        h = mix(h, self.fertility_band as u64);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banding_saturates_and_covers_the_range() {
        assert_eq!(HabitatSignature::temperature_band(-100.0), 0);
        assert_eq!(
            HabitatSignature::temperature_band(100.0),
            TEMPERATURE_BANDS - 1
        );
        assert_eq!(HabitatSignature::moisture_band(0.0), 0);
        assert_eq!(HabitatSignature::moisture_band(1.0), MOISTURE_BANDS - 1);
        assert_eq!(HabitatSignature::fertility_band(0.0), 0);
        assert_eq!(HabitatSignature::fertility_band(1.0), FERTILITY_BANDS - 1);
    }

    #[test]
    fn banding_boundaries_are_monotonic() {
        let mut last = 0u8;
        for i in 0..=100 {
            let m = i as f32 / 100.0;
            let b = HabitatSignature::moisture_band(m);
            assert!(b >= last, "moisture band not monotonic at {m}");
            assert!(b < MOISTURE_BANDS);
            last = b;
        }
    }

    #[test]
    fn seed_is_pure_and_separates_every_field() {
        let base = HabitatSignature {
            biome: Biome::TemperateForest.id(),
            temperature_band: 3,
            moisture_band: 2,
            fertility_band: 1,
        };
        assert_eq!(base.seed(), base.seed());
        let variants = [
            HabitatSignature {
                biome: Biome::Rainforest.id(),
                ..base
            },
            HabitatSignature {
                temperature_band: 4,
                ..base
            },
            HabitatSignature {
                moisture_band: 3,
                ..base
            },
            HabitatSignature {
                fertility_band: 2,
                ..base
            },
        ];
        for v in variants {
            assert_ne!(v.seed(), base.seed(), "field change did not move the seed");
        }
    }

    #[test]
    fn of_reads_biome_and_bands() {
        let c = Climate {
            temperature: 16.0,
            moisture: 0.6,
        };
        let s = Soils {
            depth: 0.7,
            fertility: 0.55,
        };
        let sig = HabitatSignature::of(Biome::TemperateForest, &c, &s);
        assert_eq!(sig.biome(), Biome::TemperateForest);
        assert_eq!(
            sig.temperature_band,
            HabitatSignature::temperature_band(16.0)
        );
        assert_eq!(sig.moisture_band, HabitatSignature::moisture_band(0.6));
        assert_eq!(sig.fertility_band, HabitatSignature::fertility_band(0.55));
    }
}
