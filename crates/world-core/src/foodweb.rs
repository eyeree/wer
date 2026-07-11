//! Food webs and their plausibility constraints (phase-3-plan.md §4.4, §7.4).
//!
//! [`food_web`] is the first real embodiment of section 8's stance —
//! **rule-based constraints and one deterministic relaxation pass, not machine
//! learning**. It takes a [`SpeciesRoster`] and the habitat's primary
//! productivity (aggregate vegetation density is the producer base) and
//! projects a trophic graph through fixed ecological rules:
//!
//! - herbivore biomass is capped at a fraction of producer biomass,
//! - carnivore biomass at a fraction of herbivore biomass (the classic ~10%
//!   trophic-transfer pyramid),
//! - maximum sustainable body size is a function of productivity,
//! - predator→prey edges are drawn only where genome size and diet admit the
//!   prey,
//! - species that end with no sustainable biomass are **pruned** (an animal
//!   predator with no admissible prey, or one too large for the habitat to
//!   feed).
//!
//! The output is *coherent by construction*: its post-conditions are exactly
//! the coherence invariants the §12.3 harness asserts, so the harness checks
//! the algorithm, not a re-derivation.

use crate::genome::size_class_units;
use crate::hash::mix;
use crate::species::{Species, SpeciesRoster, Trophic, TROPHIC_TIERS};

/// Herbivore biomass as a fraction of producer biomass (the ~10% trophic
/// transfer rule). Part of the coherence contract.
pub const HERBIVORE_EFFICIENCY: f32 = 0.1;

/// Carnivore biomass as a fraction of herbivore biomass.
pub const CARNIVORE_EFFICIENCY: f32 = 0.1;

/// Decomposer biomass as a fraction of producer biomass (dead matter recycled).
pub const DECOMPOSER_FRACTION: f32 = 0.15;

/// A predator can take prey up to this multiple of its own body size (ambush /
/// pack hunting admits slightly-larger prey; anything bigger is off the menu).
const PREY_SIZE_RATIO: f32 = 1.2;

/// Smallest max-body-size a habitat allows (world units), even at zero
/// productivity.
const MAX_BODY_SIZE_FLOOR: f32 = 0.5;

/// Additional max-body-size a fully productive habitat allows, on top of the
/// floor — so the size ceiling spans roughly `[0.5, 12.8]` across productivity.
const MAX_BODY_SIZE_SPAN: f32 = 12.3;

/// The maximum sustainable body size for a habitat of given primary
/// productivity: barren habitats support only small organisms, rich ones the
/// largest (phase-3-plan.md §7.4). Near-field realization clamps expressed body
/// size to this bound (the §12.3 body-size invariant).
#[inline]
#[must_use]
pub fn max_body_size(primary_productivity: f32) -> f32 {
    MAX_BODY_SIZE_FLOOR + primary_productivity.clamp(0.0, 1.0) * MAX_BODY_SIZE_SPAN
}

/// A trophic graph over a roster: predator→prey edges, sustainable per-tier
/// biomass shares, and the pruned (unsustainable) species — all constrained by
/// the section 8 plausibility rules.
#[derive(Debug, Clone, PartialEq)]
pub struct FoodWeb {
    /// `(predator index, prey index)` within the roster; predator eats prey.
    pub edges: Vec<(u32, u32)>,
    /// Sustainable biomass share per trophic tier ([`Trophic::tier`]), summing
    /// to ~1 — the aggregate L8 samples to fill its pressure channels. Ratios
    /// are the coherence contract: `tier[1] ≤ α·tier[0]`, `tier[2] ≤ β·tier[1]`.
    pub tier_biomass: [f32; TROPHIC_TIERS],
    /// Maximum sustainable body size for the habitat (world units).
    pub max_body_size: f32,
    /// Roster indices pruned as unsustainable (orphan predators, oversized
    /// animals). Ascending, deduplicated.
    pub pruned: Vec<u32>,
}

impl FoodWeb {
    /// Whether a roster index survived the plausibility projection.
    #[inline]
    #[must_use]
    pub fn survives(&self, index: u32) -> bool {
        self.pruned.binary_search(&index).is_err()
    }

    /// A stable 64-bit fingerprint of the tier biomass — the parity surface
    /// exported to wasm and pinned to a golden (phase-3-plan.md §12.5). Tier
    /// biomass is portable `f32` (pure IEEE arithmetic over integer-derived
    /// inputs), so this is a cross-platform identity.
    #[must_use]
    pub fn tier_biomass_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xF00D_3EB0_0BEE_0002;
        for b in self.tier_biomass {
            h = mix(h, u64::from(b.to_bits()));
        }
        h
    }
}

/// Whether a species can be an animal predator whose body size is capped by
/// habitat productivity.
#[inline]
fn size_capped(species: &Species) -> bool {
    species.trophic.is_predator()
}

/// Whether `species` is sustainable in a habitat with this `max_size` — animal
/// predators larger than the habitat can feed are not.
#[inline]
fn sustainable(species: &Species, max_size: f32) -> bool {
    !size_capped(species) || species.genome.base_size() <= max_size
}

/// Build (and constrain) the food web for a roster and its primary
/// productivity.
///
/// A single deterministic relaxation pass (no iteration to convergence needed
/// at Phase 3 fidelity, phase-2-plan.md §7.4): draw admissible predator→prey
/// edges, prune orphan and oversized predators, and allocate a sustainable
/// biomass budget down the tiers by fixed fractions. Pure and portable.
#[must_use]
pub fn food_web(roster: &SpeciesRoster, primary_productivity: f32) -> FoodWeb {
    let species = &roster.species;
    let pp = primary_productivity.clamp(0.0, 1.0);
    let max_size = max_body_size(pp);

    let mut edges = Vec::new();
    let mut pruned = Vec::new();

    for (pi, pred) in species.iter().enumerate() {
        let (eats_producers, eats_animals) = match pred.trophic {
            Trophic::Herbivore => (true, false),
            Trophic::Omnivore => (true, true),
            Trophic::Carnivore => (false, true),
            // Producers and decomposers have no outgoing predation edges.
            Trophic::Producer | Trophic::Decomposer => continue,
        };
        // Oversized animal predators cannot be fed by this habitat.
        if !sustainable(pred, max_size) {
            pruned.push(pi as u32);
            continue;
        }
        // Diet breadth (a niche gene) caps how many prey a predator draws.
        let max_prey = 1 + usize::from(pred.genome.niche.diet_breadth) / 64;
        let pred_size = pred.genome.base_size();
        let mut prey = Vec::new();
        for (qi, cand) in species.iter().enumerate() {
            if qi == pi || !sustainable(cand, max_size) {
                continue;
            }
            let admissible = match cand.trophic {
                Trophic::Producer => eats_producers,
                Trophic::Herbivore | Trophic::Omnivore | Trophic::Carnivore => {
                    eats_animals && cand.genome.base_size() <= pred_size * PREY_SIZE_RATIO
                }
                Trophic::Decomposer => false,
            };
            if admissible {
                prey.push(qi as u32);
                if prey.len() >= max_prey {
                    break;
                }
            }
        }
        // A carnivore with no admissible animal prey is an orphan tier: pruned
        // (omnivores always retain producer prey, so they never orphan here).
        if pred.trophic == Trophic::Carnivore && prey.is_empty() {
            pruned.push(pi as u32);
            continue;
        }
        for q in prey {
            edges.push((pi as u32, q));
        }
    }

    pruned.sort_unstable();
    pruned.dedup();

    // Which biomass tiers retain at least one surviving species.
    let mut tier_present = [false; TROPHIC_TIERS];
    for (i, s) in species.iter().enumerate() {
        if pruned.binary_search(&(i as u32)).is_err() {
            tier_present[s.trophic.tier()] = true;
        }
    }

    // A sustainable pyramid: producer base, then fixed-fraction transfers. The
    // ratios (and so the coherence invariants) are preserved by normalization.
    let producer = if tier_present[0] { pp.max(1.0e-3) } else { 0.0 };
    let herbivore = if tier_present[1] {
        HERBIVORE_EFFICIENCY * producer
    } else {
        0.0
    };
    let carnivore = if tier_present[2] {
        CARNIVORE_EFFICIENCY * herbivore
    } else {
        0.0
    };
    let decomposer = if tier_present[3] {
        DECOMPOSER_FRACTION * producer
    } else {
        0.0
    };
    let raw = [producer, herbivore, carnivore, decomposer];
    let sum: f32 = raw.iter().sum();
    let tier_biomass = if sum > 0.0 {
        [raw[0] / sum, raw[1] / sum, raw[2] / sum, raw[3] / sum]
    } else {
        [0.0; TROPHIC_TIERS]
    };

    FoodWeb {
        edges,
        tier_biomass,
        max_body_size: max_size,
        pruned,
    }
}

/// Per-species biomass estimate for a surviving roster index: its tier's share
/// split evenly across the surviving species in that tier. Used by
/// [`crate::population`] to pick the dominant species and weight diversity.
#[must_use]
pub fn species_biomass(roster: &SpeciesRoster, web: &FoodWeb, index: usize) -> f32 {
    let species = &roster.species;
    if index >= species.len() || !web.survives(index as u32) {
        return 0.0;
    }
    let tier = species[index].trophic.tier();
    let count = species
        .iter()
        .enumerate()
        .filter(|(i, s)| s.trophic.tier() == tier && web.survives(*i as u32))
        .count();
    if count == 0 {
        return 0.0;
    }
    web.tier_biomass[tier] / count as f32
}

/// The body size a size class expresses, re-exported for the realization clamp.
#[inline]
#[must_use]
pub fn body_size(size_class: u8) -> f32 {
    size_class_units(size_class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::Biome;
    use crate::habitat::HabitatSignature;
    use crate::species::species_roster;

    fn sig(biome: Biome, t: u8, m: u8, f: u8) -> HabitatSignature {
        HabitatSignature {
            biome: biome.id(),
            temperature_band: t,
            moisture_band: m,
            fertility_band: f,
        }
    }

    #[test]
    fn web_is_pure_and_deterministic() {
        let roster = species_roster(sig(Biome::Rainforest, 5, 4, 3));
        assert_eq!(food_web(&roster, 0.8), food_web(&roster, 0.8));
    }

    #[test]
    fn coherence_invariants_hold_over_the_signature_space() {
        // The §12.3 coherence invariants, asserted as food-web post-conditions.
        for biome in crate::biome::BIOMES {
            for t in 0..crate::habitat::TEMPERATURE_BANDS {
                for m in 0..crate::habitat::MOISTURE_BANDS {
                    for f in 0..crate::habitat::FERTILITY_BANDS {
                        let s = sig(biome, t, m, f);
                        let roster = species_roster(s);
                        for pp in [0.0f32, 0.25, 0.6, 1.0] {
                            let web = food_web(&roster, pp);
                            let [prod, herb, carn, _dec] = web.tier_biomass;

                            // Productivity bound: herbivore ≤ α · producer.
                            assert!(
                                herb <= HERBIVORE_EFFICIENCY * prod + 1e-6,
                                "herbivore bound violated {biome:?} {t}/{m}/{f} pp={pp}"
                            );
                            // Trophic bound: carnivore ≤ β · herbivore.
                            assert!(
                                carn <= CARNIVORE_EFFICIENCY * herb + 1e-6,
                                "carnivore bound violated {biome:?} {t}/{m}/{f} pp={pp}"
                            );
                            // No orphan tiers: every surviving carnivore has a
                            // prey edge; carnivore biomass implies herbivores.
                            if carn > 0.0 {
                                assert!(herb > 0.0, "carnivore tier without herbivores");
                            }
                            for (i, sp) in roster.species.iter().enumerate() {
                                if sp.trophic == Trophic::Carnivore && web.survives(i as u32) {
                                    assert!(
                                        web.edges.iter().any(|&(p, _)| p == i as u32),
                                        "surviving carnivore {i} has no prey edge"
                                    );
                                }
                            }
                            // Body-size bound: no surviving animal predator
                            // exceeds the habitat's max body size.
                            for (i, sp) in roster.species.iter().enumerate() {
                                if sp.trophic.is_predator() && web.survives(i as u32) {
                                    assert!(
                                        sp.genome.base_size() <= web.max_body_size,
                                        "oversized predator survived"
                                    );
                                }
                            }
                            // Biomass shares are non-negative and sum to ~1 (or 0).
                            let total: f32 = web.tier_biomass.iter().sum();
                            assert!(total <= 1.0 + 1e-5);
                            assert!(web.tier_biomass.iter().all(|&b| b >= 0.0));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn edges_point_predator_to_smaller_or_equal_prey() {
        let roster = species_roster(sig(Biome::Rainforest, 5, 4, 3));
        let web = food_web(&roster, 0.9);
        for &(p, q) in &web.edges {
            let pred = &roster.species[p as usize];
            let prey = &roster.species[q as usize];
            // Animal prey must be within the size ratio; producer prey is grazed.
            if prey.trophic != Trophic::Producer {
                assert!(
                    prey.genome.base_size() <= pred.genome.base_size() * PREY_SIZE_RATIO + 1e-6
                );
            }
        }
    }

    #[test]
    fn low_productivity_supports_only_small_organisms() {
        assert!(max_body_size(0.0) < max_body_size(1.0));
        let roster = species_roster(sig(Biome::Rainforest, 5, 4, 3));
        let barren = food_web(&roster, 0.0);
        // In a barren habitat, large animal predators are pruned.
        for (i, sp) in roster.species.iter().enumerate() {
            if sp.trophic.is_predator() && sp.genome.base_size() > barren.max_body_size {
                assert!(!barren.survives(i as u32), "oversized predator not pruned");
            }
        }
    }
}
