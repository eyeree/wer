//! Species archetypes and deterministic rosters (phase-3-plan.md §4.3).
//!
//! A [`Species`] is a stable identity ([`species_seed`]) plus its [`Genome`]
//! and an assigned [`Trophic`] role. A [`SpeciesRoster`] is the deterministic,
//! bounded, trophic-sorted set of species a habitat carries — a pure function
//! of its [`HabitatSignature`], memoized by the runtime (§6.3) so identical
//! habitats across the world share species and the world reads as ecologically
//! zoned.
//!
//! Roster size and trophic composition follow from the habitat: barren biomes
//! yield tiny producer-only rosters; rich, warm, wet biomes yield the largest
//! and most trophically complete. Everything here is pure integer-derived and
//! capped at [`ROSTER_MAX`], so the aggregate tiles' dominant-species index
//! fits a small type and the cache stays bounded.

use crate::genome::Genome;
use crate::habitat::HabitatSignature;
use crate::hash::mix;

/// Fixed basis separating species-identity hashing from every other domain.
const SPECIES_BASIS: u64 = 0x5EED_C0DE_A11C_E5E5;

/// Maximum species in one roster. Small enough that a dominant-species index
/// fits a `u16` tile with room to spare and the roster cache stays bounded
/// (phase-3-plan.md §4.3, §6.3).
pub const ROSTER_MAX: usize = 12;

/// Number of biomass trophic tiers the food web tracks (phase-3-plan.md §4.4).
/// [`Trophic`] roles map onto these via [`Trophic::tier`].
pub const TROPHIC_TIERS: usize = 4;

/// A species' trophic role in its ecosystem.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Trophic {
    /// Primary producer (plants, phytoplankton).
    Producer = 0,
    /// Eats producers.
    Herbivore = 1,
    /// Eats both producers and animals.
    Omnivore = 2,
    /// Eats animals.
    Carnivore = 3,
    /// Breaks down dead matter.
    Decomposer = 4,
}

impl Trophic {
    /// The biomass tier this role contributes to, in `[0, TROPHIC_TIERS)`:
    /// producers form the base, herbivores/omnivores the consumer band,
    /// carnivores the predator band, decomposers their own tier.
    #[inline]
    #[must_use]
    pub const fn tier(self) -> usize {
        match self {
            Trophic::Producer => 0,
            Trophic::Herbivore | Trophic::Omnivore => 1,
            Trophic::Carnivore => 2,
            Trophic::Decomposer => 3,
        }
    }

    /// Whether this role consumes animal prey (a predator edge may originate
    /// here). Decomposers and producers never do.
    #[inline]
    #[must_use]
    pub const fn is_predator(self) -> bool {
        matches!(self, Trophic::Carnivore | Trophic::Omnivore)
    }

    /// Display name for tools and the debug panel.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Trophic::Producer => "producer",
            Trophic::Herbivore => "herbivore",
            Trophic::Omnivore => "omnivore",
            Trophic::Carnivore => "carnivore",
            Trophic::Decomposer => "decomposer",
        }
    }
}

/// A species: a stable identity plus its genome and assigned niche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Species {
    /// The stable identity: [`species_seed`] of `(signature, index)`.
    pub id: u64,
    /// Procedural genome, derived from `id` (§9.3).
    pub genome: Genome,
    /// Assigned trophic role (roster-sorted by [`Trophic`] rank).
    pub trophic: Trophic,
}

/// The deterministic species roster for a habitat: a small, ordered set.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeciesRoster {
    /// The habitat this roster is a function of.
    pub signature: HabitatSignature,
    /// Bounded ([`ROSTER_MAX`]), trophic-sorted species.
    pub species: Vec<Species>,
}

impl SpeciesRoster {
    /// Number of species.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.species.len()
    }

    /// Whether the roster is empty (a fully barren habitat).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.species.is_empty()
    }

    /// Count of species in a given trophic role.
    #[must_use]
    pub fn count_of(&self, trophic: Trophic) -> usize {
        self.species.iter().filter(|s| s.trophic == trophic).count()
    }
}

/// The stable per-species identity for the `index`th species of a habitat.
///
/// Pure integer hashing over the (integer) habitat seed and the index —
/// cross-platform golden-fixtured (§9.3). The *signature* a cell derives is
/// presentation-grade, but this function of a fixed signature is portable.
#[inline]
#[must_use]
pub const fn species_seed(signature: HabitatSignature, index: u32) -> u64 {
    let mut h = SPECIES_BASIS;
    h = mix(h, signature.seed());
    h = mix(h, index as u64);
    h
}

/// Derive the full roster for a habitat (pure; memoized by the runtime, §6.3).
#[must_use]
pub fn species_roster(signature: HabitatSignature) -> SpeciesRoster {
    let count = roster_size(signature);
    let layout = trophic_layout(signature, count);
    let species = layout
        .into_iter()
        .enumerate()
        .map(|(index, trophic)| {
            let id = species_seed(signature, index as u32);
            Species {
                id,
                genome: Genome::from_seed(id),
                trophic,
            }
        })
        .collect();
    SpeciesRoster { signature, species }
}

/// Base roster richness per biome: how many species a fully-realized habitat of
/// this biome carries before band adjustment. Barren biomes (Ocean surface,
/// Ice, Bare) are producer-thin; rainforest is the richest
/// (phase-3-plan.md §4.3).
#[must_use]
const fn biome_base_richness(biome: crate::biome::Biome) -> usize {
    use crate::biome::Biome;
    match biome {
        Biome::Ocean => 2,
        Biome::Ice => 1,
        Biome::Bare => 1,
        Biome::Desert => 3,
        Biome::Tundra => 3,
        Biome::River => 4,
        Biome::Grassland => 6,
        Biome::Shrubland => 5,
        Biome::Wetland => 8,
        Biome::Taiga => 8,
        Biome::TemperateForest => 9,
        Biome::Rainforest => ROSTER_MAX,
    }
}

/// The roster size for a habitat: the biome base nudged by fertility, so cells
/// of the same biome at different fertility carry visibly different diversity.
/// Clamped to `1..=ROSTER_MAX`.
#[must_use]
fn roster_size(signature: HabitatSignature) -> usize {
    let base = biome_base_richness(signature.biome()) as i32;
    let nudge = i32::from(signature.fertility_band) - 1;
    (base + nudge).clamp(1, ROSTER_MAX as i32) as usize
}

/// Build the trophic composition of a roster of `count` species: a plausible
/// pyramid (producers most numerous, predators fewest), trophic-sorted.
///
/// Cold or unproductive habitats are producer-only. Where consumers are
/// supported, the split guarantees `carnivores ≤ herbivores` and never places
/// carnivores without herbivores to eat — the roster-level precursor to the
/// food-web plausibility constraints (phase-3-plan.md §4.4, §7.3).
#[must_use]
fn trophic_layout(signature: HabitatSignature, count: usize) -> Vec<Trophic> {
    let mut out = Vec::with_capacity(count);
    if count == 0 {
        return out;
    }
    // Consumers need warmth and some productivity, and a roster big enough to
    // form a pyramid.
    let warm = signature.temperature_band >= 1;
    let productive =
        usize::from(signature.moisture_band) + usize::from(signature.fertility_band) >= 2;
    if !(warm && productive && count >= 3) {
        for _ in 0..count {
            out.push(Trophic::Producer);
        }
        return out;
    }

    let producers = (count * 2 / 5).max(1);
    let herbivores = (((count - producers) * 3 / 5).max(1)).min(producers);
    let after_herb = count - producers - herbivores;
    // Carnivores only where herbivores exist (no orphan predator tier).
    let carnivores = (after_herb / 2).min(herbivores);
    let after_carn = after_herb - carnivores;
    // Wetter habitats support omnivores; decomposers absorb the remainder.
    let omnivores = if signature.moisture_band >= 3 {
        after_carn / 2
    } else {
        0
    };
    let decomposers = after_carn - omnivores;

    let mut push = |trophic: Trophic, n: usize| {
        for _ in 0..n {
            out.push(trophic);
        }
    };
    push(Trophic::Producer, producers);
    push(Trophic::Herbivore, herbivores);
    push(Trophic::Omnivore, omnivores);
    push(Trophic::Carnivore, carnivores);
    push(Trophic::Decomposer, decomposers);
    debug_assert_eq!(out.len(), count);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::Biome;

    fn sig(biome: Biome, t: u8, m: u8, f: u8) -> HabitatSignature {
        HabitatSignature {
            biome: biome.id(),
            temperature_band: t,
            moisture_band: m,
            fertility_band: f,
        }
    }

    #[test]
    fn species_seed_is_pure_and_separates_index() {
        let s = sig(Biome::Grassland, 3, 2, 1);
        assert_eq!(species_seed(s, 0), species_seed(s, 0));
        assert_ne!(species_seed(s, 0), species_seed(s, 1));
    }

    #[test]
    fn roster_is_deterministic_and_capped() {
        let s = sig(Biome::Rainforest, 5, 4, 3);
        let a = species_roster(s);
        let b = species_roster(s);
        assert_eq!(a, b);
        assert!(a.len() <= ROSTER_MAX);
        assert!(!a.is_empty());
    }

    #[test]
    fn roster_is_trophic_sorted() {
        let s = sig(Biome::Rainforest, 5, 4, 3);
        let roster = species_roster(s);
        let mut last = 0u8;
        for sp in &roster.species {
            let rank = sp.trophic as u8;
            assert!(rank >= last, "roster not trophic-sorted");
            last = rank;
        }
    }

    #[test]
    fn barren_habitats_are_small_and_producer_only() {
        for biome in [Biome::Ice, Biome::Bare] {
            let roster = species_roster(sig(biome, 0, 0, 0));
            assert!(roster.len() <= 2);
            assert!(roster
                .species
                .iter()
                .all(|s| s.trophic == Trophic::Producer));
        }
    }

    #[test]
    fn rich_habitats_carry_multiple_trophic_tiers() {
        let roster = species_roster(sig(Biome::Rainforest, 5, 4, 3));
        assert!(roster.count_of(Trophic::Producer) >= 1);
        assert!(roster.count_of(Trophic::Herbivore) >= 1);
        // Carnivores never exceed the herbivores that sustain them.
        assert!(roster.count_of(Trophic::Carnivore) <= roster.count_of(Trophic::Herbivore));
    }

    #[test]
    fn no_carnivores_without_herbivores() {
        // Sweep the whole signature space: the orphan-tier precursor holds.
        for biome in crate::biome::BIOMES {
            for t in 0..super::super::habitat::TEMPERATURE_BANDS {
                for m in 0..super::super::habitat::MOISTURE_BANDS {
                    for f in 0..super::super::habitat::FERTILITY_BANDS {
                        let roster = species_roster(sig(biome, t, m, f));
                        if roster.count_of(Trophic::Carnivore) > 0 {
                            assert!(
                                roster.count_of(Trophic::Herbivore) > 0,
                                "carnivores without herbivores in {biome:?} {t}/{m}/{f}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn neighbouring_habitats_differ() {
        // Two distinct biomes at the same climate carry distinct rosters.
        let forest = species_roster(sig(Biome::TemperateForest, 3, 3, 2));
        let desert = species_roster(sig(Biome::Desert, 3, 1, 1));
        assert_ne!(forest.species, desert.species);
    }
}
