//! Anchor capture: turning a live discovery into a steering target
//! (phase-4-plan.md §4.2, §4.4, §7.1).
//!
//! Capture is split into a **pure core** (here) and a **runtime gatherer**
//! (`world-runtime::stream::capture_at`). The pure core, given a habitat
//! baseline possibility vector and a bounded per-domain trait *deviation*,
//! builds the captured [`Anchor`](crate::anchor::Anchor) target — the habitat's
//! signature nudged toward what makes *this* discovery distinctive. That is the
//! honest heart of "carry forward the characteristics of what you remember":
//! the anchor targets neither the raw discovery nor a fixed bound, but *the
//! world that would make this discovery typical*, pushed a bounded step toward
//! what made it stand out.
//!
//! Everything here is pure and float-deterministic. The captured target is
//! **presentation-grade** (ADR 0010 lineage, ADR 0011): `baseline` and
//! `deviation` derive from `f32` tiles and organism expression, so which world
//! a capture yields is per-run, per-platform. The portable surface Phase 4 adds
//! is the pure `steer`/`project_plausible`/`capture_target` *math* — identical
//! on native and wasm for the *same inputs* (the `steer_sample` parity export).

use crate::genome::{Expressed, Genome};
use crate::possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS};

/// A bounded per-domain deviation describing what makes a discovery distinctive
/// relative to its habitat baseline — e.g. an unusually large, luminous organism
/// yields positive Morphology and Aesthetics deviations. Values in `[-1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraitDeviation {
    /// One signed deviation per [`PossibilityDomain`], indexed by its
    /// `index()`. Unmasked/irrelevant domains stay `0`.
    pub dims: [f32; POSSIBILITY_DIMS],
}

impl TraitDeviation {
    /// The zero deviation (a discovery exactly typical of its habitat).
    #[inline]
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            dims: [0.0; POSSIBILITY_DIMS],
        }
    }

    /// Read one domain's signed deviation.
    #[inline]
    #[must_use]
    pub fn get(&self, domain: PossibilityDomain) -> f32 {
        self.dims[domain.index()]
    }

    /// Set one domain's deviation, clamped to `[-1, 1]`.
    #[inline]
    pub fn set(&mut self, domain: PossibilityDomain, value: f32) {
        self.dims[domain.index()] = value.clamp(-1.0, 1.0);
    }
}

impl Default for TraitDeviation {
    fn default() -> Self {
        Self::zero()
    }
}

/// The in-fiction trait categories the Overview lists, mapped onto the eight
/// possibility domains (phase-4-plan.md §4.4). Several categories collapse onto
/// one scalar domain in Phase 4 — a recorded limitation that resolves without
/// touching the anchor algebra when the possibility vector grows (the mask
/// simply widens).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TraitCategory {
    /// Coloration (hue/luminance) → Aesthetics.
    Coloration,
    /// Morphology, scale, branching patterns → Morphology.
    Morphology,
    /// Behavior → Behavior.
    Behavior,
    /// Ecological traits (drives vegetation + L8 pressure) → Ecology.
    Ecological,
    /// Climate affinity → Climate.
    ClimateAffinity,
    /// Rock / landscape character → Geology.
    Landscape,
    /// River / wetness → Hydrology.
    Waterways,
    /// Atmosphere / ocean → Planetary.
    Atmosphere,
}

impl TraitCategory {
    /// All categories in stable order (the debug shell cycles through these).
    pub const ALL: [TraitCategory; 8] = [
        TraitCategory::Coloration,
        TraitCategory::Morphology,
        TraitCategory::Behavior,
        TraitCategory::Ecological,
        TraitCategory::ClimateAffinity,
        TraitCategory::Landscape,
        TraitCategory::Waterways,
        TraitCategory::Atmosphere,
    ];

    /// The possibility domain this category maps onto in Phase 4.
    #[inline]
    #[must_use]
    pub const fn domain(self) -> PossibilityDomain {
        match self {
            TraitCategory::Coloration => PossibilityDomain::Aesthetics,
            TraitCategory::Morphology => PossibilityDomain::Morphology,
            TraitCategory::Behavior => PossibilityDomain::Behavior,
            TraitCategory::Ecological => PossibilityDomain::Ecology,
            TraitCategory::ClimateAffinity => PossibilityDomain::Climate,
            TraitCategory::Landscape => PossibilityDomain::Geology,
            TraitCategory::Waterways => PossibilityDomain::Hydrology,
            TraitCategory::Atmosphere => PossibilityDomain::Planetary,
        }
    }

    /// The single-domain anchor mask bit for this category.
    #[inline]
    #[must_use]
    pub const fn mask_bit(self) -> u8 {
        1 << self.domain().index() as u8
    }

    /// Short display name for the debug panel and inspector.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            TraitCategory::Coloration => "coloration",
            TraitCategory::Morphology => "morphology",
            TraitCategory::Behavior => "behavior",
            TraitCategory::Ecological => "ecological",
            TraitCategory::ClimateAffinity => "climate",
            TraitCategory::Landscape => "landscape",
            TraitCategory::Waterways => "waterways",
            TraitCategory::Atmosphere => "atmosphere",
        }
    }
}

/// The anchor mask covering a set of trait categories, in the fiction's terms.
#[must_use]
pub fn category_mask(categories: &[TraitCategory]) -> u8 {
    let mut mask = 0u8;
    for c in categories {
        mask |= c.mask_bit();
    }
    mask
}

/// Build a captured anchor target: the habitat's possibility signature nudged by
/// the discovery's deviation, on the masked dimensions only (phase-4-plan.md
/// §7.1). `gain` bounds how far a capture can pull past its habitat baseline
/// (the "distinctiveness" strength). Unmasked dimensions are left at neutral —
/// [`steer`](crate::anchor::steer) never reads them.
///
/// A neutral deviation reproduces the baseline exactly on the mask, so capture
/// is a modulation of what the world already is, not a snapshot of the discovery.
/// Pure and float-deterministic; presentation-grade because `baseline` and
/// `deviation` derive from `f32` tiles/organisms (ADR 0010).
#[must_use]
pub fn capture_target(
    baseline: PossibilityVector,
    deviation: TraitDeviation,
    mask: u8,
    gain: f32,
) -> PossibilityVector {
    let mut v = PossibilityVector::neutral();
    for i in 0..POSSIBILITY_DIMS {
        if mask & (1 << i as u8) != 0 {
            v.dims[i] = (baseline.dims[i] + gain * deviation.dims[i]).clamp(0.0, 1.0);
        }
    }
    v
}

/// The trait deviation of an organism relative to its habitat's baseline
/// expression (phase-4-plan.md §7.1): a distinctively large creature yields a
/// positive Morphology deviation, a luminous one a positive Aesthetics
/// deviation, an active/aggressive one a positive Behavior deviation, and a
/// higher-tier predator a positive Ecology deviation. Each is the organism's
/// expressed trait level minus the habitat baseline in that domain, bounded to
/// `[-1, 1]`, so a typical organism in a neutral habitat deviates near zero.
///
/// Presentation-grade throughout (reads `f32` expression).
#[must_use]
pub fn organism_trait_deviation(
    expressed: Expressed,
    genome: Genome,
    habitat_baseline: PossibilityVector,
) -> TraitDeviation {
    let mut dev = TraitDeviation::zero();
    // Morphology: body size relative to the size-class ladder (0..7). Bigger
    // than the habitat's baseline body scale reads as distinctive morphology.
    let size_norm = f32::from(genome.appearance.size_class) / 7.0;
    dev.set(
        PossibilityDomain::Morphology,
        size_norm - habitat_baseline.get(PossibilityDomain::Morphology),
    );
    // Aesthetics: expressed luminance departure from the habitat baseline.
    dev.set(
        PossibilityDomain::Aesthetics,
        expressed.luminance - habitat_baseline.get(PossibilityDomain::Aesthetics),
    );
    // Behavior: mean of expressed activity and aggression vs the baseline.
    let behaviour = 0.5 * (expressed.activity + expressed.aggression);
    dev.set(
        PossibilityDomain::Behavior,
        behaviour - habitat_baseline.get(PossibilityDomain::Behavior),
    );
    // Ecology: trophic tendency (producer↔predator) vs the baseline pressure.
    let trophic = f32::from(genome.niche.trophic_tendency) / 255.0;
    dev.set(
        PossibilityDomain::Ecology,
        trophic - habitat_baseline.get(PossibilityDomain::Ecology),
    );
    dev
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genome::GenomeBias;

    #[test]
    fn categories_map_to_distinct_domains() {
        let mut seen = 0u8;
        for c in TraitCategory::ALL {
            let bit = c.mask_bit();
            assert_eq!(seen & bit, 0, "two categories share a domain in Phase 4");
            seen |= bit;
        }
        // Every domain is covered exactly once (8 categories → 8 domains).
        assert_eq!(seen, 0xFF);
    }

    #[test]
    fn neutral_deviation_reproduces_baseline_on_mask() {
        let mut baseline = PossibilityVector::neutral();
        baseline.set(PossibilityDomain::Morphology, 0.7);
        baseline.set(PossibilityDomain::Aesthetics, 0.3);
        let mask = category_mask(&[TraitCategory::Morphology, TraitCategory::Coloration]);
        let target = capture_target(baseline, TraitDeviation::zero(), mask, 0.5);
        assert!((target.get(PossibilityDomain::Morphology) - 0.7).abs() < 1e-6);
        assert!((target.get(PossibilityDomain::Aesthetics) - 0.3).abs() < 1e-6);
        // Unmasked domains stay neutral.
        assert_eq!(target.get(PossibilityDomain::Ecology), 0.5);
    }

    #[test]
    fn capture_target_nudges_toward_deviation_within_bounds() {
        let baseline = PossibilityVector::neutral();
        let mut dev = TraitDeviation::zero();
        dev.set(PossibilityDomain::Morphology, 1.0);
        let mask = TraitCategory::Morphology.mask_bit();
        let target = capture_target(baseline, dev, mask, 0.4);
        // Pulled up from 0.5 toward the deviation, and clamped to [0, 1].
        assert!(target.get(PossibilityDomain::Morphology) > 0.5);
        assert!(target.get(PossibilityDomain::Morphology) <= 1.0);
    }

    #[test]
    fn organism_deviation_is_bounded() {
        let genome = Genome::from_seed(0xDEAD_BEEF);
        let expressed = genome.express(GenomeBias::neutral());
        let dev = organism_trait_deviation(expressed, genome, PossibilityVector::neutral());
        for d in dev.dims {
            assert!((-1.0..=1.0).contains(&d), "deviation {d} out of range");
        }
    }

    #[test]
    fn a_large_luminous_organism_deviates_positively() {
        // A genome with a large size class and high luminance should read as a
        // positive Morphology and Aesthetics deviation against a neutral habitat.
        let mut genome = Genome::from_seed(1);
        genome.appearance.size_class = 7;
        genome.appearance.luminance = 255;
        let expressed = genome.express(GenomeBias::neutral());
        let dev = organism_trait_deviation(expressed, genome, PossibilityVector::neutral());
        assert!(dev.get(PossibilityDomain::Morphology) > 0.0);
        assert!(dev.get(PossibilityDomain::Aesthetics) > 0.0);
    }
}
