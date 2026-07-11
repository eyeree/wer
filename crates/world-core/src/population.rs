//! Aggregate population sampling from a roster and its food web
//! (phase-3-plan.md §7.5).
//!
//! This is the per-cell arithmetic the L8 Ecology layer runs (section 10's
//! aggregate ecology, and section 9's food-web-structure / species-distribution
//! steps collapsed into cached fields). Given the memoized `(roster, food web)`
//! for a cell's habitat, the cell's local primary productivity (aggregate
//! vegetation density), and the dequantized Ecology bucket, it emits the four
//! aggregate values L8 caches: the dominant species index, herbivore and
//! predator pressure, and species diversity.
//!
//! Everything here is presentation `f32` over the (portable) roster/web — never
//! an identity. The dominant index is an index *into the cell's roster*, so the
//! L8 tile stays compact (`--species` reconstructs the full identity, §6.1).

use crate::foodweb::{species_biomass, FoodWeb};
use crate::habitat::{HabitatSignature, FERTILITY_BANDS, MOISTURE_BANDS};
use crate::species::SpeciesRoster;
use crate::vegetation::biome_base;

/// A representative primary productivity for a habitat *signature* — the value
/// the memoized food web is built from (phase-3-plan.md §6.3). The web's
/// structure (which species survive, the max body size) is a function of the
/// signature; per-cell productivity variation is applied later at the L8 field
/// level by [`population`]. Mirrors [`crate::vegetation::vegetation`]'s density
/// shape from the biome base and the banded fertility/moisture.
#[must_use]
pub fn signature_productivity(signature: HabitatSignature) -> f32 {
    let (base_density, _) = biome_base(signature.biome());
    let fertility = (f32::from(signature.fertility_band) + 0.5) / f32::from(FERTILITY_BANDS);
    let moisture = (f32::from(signature.moisture_band) + 0.5) / f32::from(MOISTURE_BANDS);
    (base_density * (0.4 + 0.6 * fertility))
        .min(moisture + 0.1)
        .clamp(0.0, 1.0)
}

/// The aggregate ecology values for one cell (the L8 channels, §6.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopulationSample {
    /// Index of the highest-biomass species in the roster (dominant-species
    /// `u16` tile). `0` for an empty roster.
    pub dominant: u16,
    /// Herbivore pressure in `[0, 1]` — herbivore-tier biomass scaled by local
    /// primary productivity and the Ecology bucket.
    pub herbivore: f32,
    /// Predator pressure in `[0, 1]` — carnivore-tier biomass, likewise scaled.
    pub predator: f32,
    /// Species diversity in `[0, 1]` — normalized biomass-weighted entropy of
    /// the roster (Ecology-independent; a property of the roster and web).
    pub diversity: f32,
}

/// Aggregate population values for a cell (phase-3-plan.md §7.5).
///
/// `primary_productivity` is the cell's aggregate vegetation density; `ecology`
/// is the dequantized Ecology bucket, both in `[0, 1]`. Herbivore and predator
/// pressures scale by `productivity · ecology`, so they never exceed the
/// food-web tier biomass — keeping the coherence invariants (herbivore ≤
/// α·productivity, predator ≤ β·herbivore) true at the field level (§12.3).
#[must_use]
pub fn population(
    roster: &SpeciesRoster,
    web: &FoodWeb,
    primary_productivity: f32,
    ecology: f32,
) -> PopulationSample {
    let pp = primary_productivity.clamp(0.0, 1.0);
    let eco = ecology.clamp(0.0, 1.0);
    let scale = pp * eco;

    // Dominant: the highest-biomass surviving species (producers dominate; ties
    // resolve to the lowest roster index, which is stable and roster-sorted).
    let mut dominant = 0u16;
    let mut best = -1.0f32;
    for i in 0..roster.species.len() {
        let bio = species_biomass(roster, web, i);
        if bio > best {
            best = bio;
            dominant = i as u16;
        }
    }

    let herbivore = (web.tier_biomass[1] * scale).clamp(0.0, 1.0);
    let predator = (web.tier_biomass[2] * scale).clamp(0.0, 1.0);
    let diversity = diversity_of(roster, web);

    PopulationSample {
        dominant,
        herbivore,
        predator,
        diversity,
    }
}

/// Normalized Shannon entropy of the surviving roster weighted by per-species
/// biomass, in `[0, 1]` (1 = maximally even, 0 = a monoculture or empty).
#[must_use]
pub fn diversity_of(roster: &SpeciesRoster, web: &FoodWeb) -> f32 {
    let mut shares = Vec::with_capacity(roster.species.len());
    let mut total = 0.0f32;
    for i in 0..roster.species.len() {
        let bio = species_biomass(roster, web, i);
        if bio > 0.0 {
            shares.push(bio);
            total += bio;
        }
    }
    let n = shares.len();
    if n <= 1 || total <= 0.0 {
        return 0.0;
    }
    let mut entropy = 0.0f32;
    for bio in shares {
        let p = bio / total;
        entropy -= p * p.ln();
    }
    // Normalize by ln(n): the entropy of a perfectly even distribution.
    (entropy / (n as f32).ln()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::Biome;
    use crate::foodweb::food_web;
    use crate::habitat::HabitatSignature;
    use crate::species::species_roster;

    fn habitat(biome: Biome, t: u8, m: u8, f: u8) -> (SpeciesRoster, FoodWeb) {
        let sig = HabitatSignature {
            biome: biome.id(),
            temperature_band: t,
            moisture_band: m,
            fertility_band: f,
        };
        let roster = species_roster(sig);
        let web = food_web(&roster, 0.7);
        (roster, web)
    }

    #[test]
    fn pressures_respect_the_coherence_bounds() {
        let (roster, web) = habitat(Biome::Rainforest, 5, 4, 3);
        let s = population(&roster, &web, 0.8, 0.6);
        // Herbivore pressure never exceeds α · primary productivity.
        assert!(s.herbivore <= crate::foodweb::HERBIVORE_EFFICIENCY * 0.8 + 1e-6);
        // Predator pressure never exceeds β · herbivore pressure (same scale).
        assert!(s.predator <= crate::foodweb::CARNIVORE_EFFICIENCY * s.herbivore + 1e-6);
        assert!((0.0..=1.0).contains(&s.diversity));
        assert!((s.dominant as usize) < roster.species.len());
    }

    #[test]
    fn ecology_bucket_scales_pressure_not_diversity() {
        let (roster, web) = habitat(Biome::TemperateForest, 3, 3, 2);
        let lush = population(&roster, &web, 0.7, 1.0);
        let sparse = population(&roster, &web, 0.7, 0.0);
        assert!(lush.herbivore >= sparse.herbivore);
        assert_eq!(sparse.herbivore, 0.0);
        // Diversity is a roster property, independent of the Ecology bucket.
        assert_eq!(lush.diversity, sparse.diversity);
    }

    #[test]
    fn richer_habitats_are_more_diverse() {
        let (rf, rf_web) = habitat(Biome::Rainforest, 5, 4, 3);
        let (ice, ice_web) = habitat(Biome::Ice, 0, 0, 0);
        let rf_div = diversity_of(&rf, &rf_web);
        let ice_div = diversity_of(&ice, &ice_web);
        assert!(rf_div > ice_div);
    }

    #[test]
    fn dominant_is_the_highest_biomass_species() {
        let (roster, web) = habitat(Biome::Rainforest, 5, 4, 3);
        let s = population(&roster, &web, 0.8, 0.6);
        let dominant_bio = species_biomass(&roster, &web, s.dominant as usize);
        for i in 0..roster.species.len() {
            assert!(species_biomass(&roster, &web, i) <= dominant_bio + 1e-6);
        }
    }
}
