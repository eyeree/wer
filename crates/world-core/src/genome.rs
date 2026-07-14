//! Procedural genomes (phase-3-plan.md §4.2, section 11).
//!
//! A [`Genome`] is a stable procedural identity split into three *independent*
//! sub-genomes — appearance, behavior, ecological niche. The raw trait words
//! are integer and portable (derived from a species seed by pure hashing,
//! cross-platform golden-fixtured, §9.3); *expression* into `f32` happens on
//! read, biased by the Morphology / Behavior / Aesthetics possibility domains.
//!
//! Bias is a bounded modulation of the base genes, never a re-identification:
//! the genome id is fixed, and a neutral possibility vector reproduces the
//! unbiased genome exactly — mirroring how Phase 2 possibility drift changes a
//! tile's *content* but never a feature's identity (phase-2-plan.md §4.2).

use crate::hash::mix;

/// Independent salts so the three sub-genomes fold from the same species seed
/// without correlating (section 11: "three independent domains"). Part of the
/// stable contract — changing one re-rolls that sub-genome for every species.
const APPEARANCE_SALT: u64 = 0x00A9_9EA2_4C13_7F01;
const BEHAVIOR_SALT: u64 = 0x00B3_1D57_9028_AE44;
const NICHE_SALT: u64 = 0x00C7_4420_6BF1_5D99;

/// How far Aesthetics can rotate a species' base hue around the colour wheel,
/// as a fraction of the wheel (`±HUE_SHIFT/2`). A neutral Aesthetics bucket
/// (0.5) applies no rotation.
const HUE_SHIFT: f32 = 0.35;

/// Appearance sub-genome (Aesthetics domain): colour, luminance, size, form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AppearanceGenes {
    /// Base hue, `0..=255` around the colour wheel.
    pub hue: u8,
    /// Base bioluminance, `0..=255`.
    pub luminance: u8,
    /// Size class, `0..=7` (exponential body-size ladder, [`size_class_units`]).
    pub size_class: u8,
    /// Body-form archetype, `0..=15`.
    pub form: u8,
}

impl AppearanceGenes {
    #[inline]
    #[must_use]
    const fn from_word(w: u64) -> Self {
        Self {
            hue: (w & 0xFF) as u8,
            luminance: ((w >> 8) & 0xFF) as u8,
            size_class: ((w >> 16) & 0x7) as u8,
            form: ((w >> 19) & 0xF) as u8,
        }
    }
}

/// Behavior sub-genome (Behavior domain): activity, aggression, sociality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BehaviorGenes {
    /// Activity level (diurnal ↔ nocturnal / sluggish ↔ restless), `0..=255`.
    pub activity: u8,
    /// Aggression, `0..=255`.
    pub aggression: u8,
    /// Sociality (solitary ↔ herd/flock), `0..=255`.
    pub sociality: u8,
}

impl BehaviorGenes {
    #[inline]
    #[must_use]
    const fn from_word(w: u64) -> Self {
        Self {
            activity: (w & 0xFF) as u8,
            aggression: ((w >> 8) & 0xFF) as u8,
            sociality: ((w >> 16) & 0xFF) as u8,
        }
    }
}

/// Niche sub-genome (Morphology domain drives its expressed size): trophic
/// tendency, diet breadth, and environmental tolerances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NicheGenes {
    /// Trophic tendency (producer ↔ predator), `0..=255`.
    pub trophic_tendency: u8,
    /// Diet breadth (specialist ↔ generalist), `0..=255`.
    pub diet_breadth: u8,
    /// Temperature tolerance width, `0..=255`.
    pub temperature_tolerance: u8,
    /// Moisture tolerance width, `0..=255`.
    pub moisture_tolerance: u8,
}

impl NicheGenes {
    #[inline]
    #[must_use]
    const fn from_word(w: u64) -> Self {
        Self {
            trophic_tendency: (w & 0xFF) as u8,
            diet_breadth: ((w >> 8) & 0xFF) as u8,
            temperature_tolerance: ((w >> 16) & 0xFF) as u8,
            moisture_tolerance: ((w >> 24) & 0xFF) as u8,
        }
    }
}

/// A stable procedural genome: three independent sub-genomes (section 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Genome {
    /// Colour / luminance / size / form.
    pub appearance: AppearanceGenes,
    /// Activity / aggression / sociality.
    pub behavior: BehaviorGenes,
    /// Trophic tendency / diet breadth / tolerances.
    pub niche: NicheGenes,
}

impl Genome {
    /// Derive a genome purely from a species seed.
    ///
    /// Cross-platform: pure integer hashing (§9.3), golden-fixtured and
    /// wasm-parity-tested. Each sub-genome folds the seed under an independent
    /// salt so appearance, behavior, and niche do not correlate.
    #[inline]
    #[must_use]
    pub const fn from_seed(seed: u64) -> Self {
        Self {
            appearance: AppearanceGenes::from_word(mix(seed, APPEARANCE_SALT)),
            behavior: BehaviorGenes::from_word(mix(seed, BEHAVIOR_SALT)),
            niche: NicheGenes::from_word(mix(seed, NICHE_SALT)),
        }
    }

    /// A stable 64-bit fingerprint of every trait word — the parity surface
    /// exported to wasm and pinned to a golden (phase-3-plan.md §12.5). Fold
    /// order is part of the stable contract.
    #[must_use]
    pub const fn fingerprint(&self) -> u64 {
        let a = &self.appearance;
        let b = &self.behavior;
        let n = &self.niche;
        let mut h: u64 = 0x6E00_0A11_C0DE_0001;
        h = mix(h, a.hue as u64);
        h = mix(h, a.luminance as u64);
        h = mix(h, a.size_class as u64);
        h = mix(h, a.form as u64);
        h = mix(h, b.activity as u64);
        h = mix(h, b.aggression as u64);
        h = mix(h, b.sociality as u64);
        h = mix(h, n.trophic_tendency as u64);
        h = mix(h, n.diet_breadth as u64);
        h = mix(h, n.temperature_tolerance as u64);
        h = mix(h, n.moisture_tolerance as u64);
        h
    }

    /// Base body size in world units (unbiased), the [`size_class_units`] of the
    /// appearance genes.
    #[inline]
    #[must_use]
    pub fn base_size(&self) -> f32 {
        size_class_units(self.appearance.size_class)
    }

    /// Express this genome under a possibility bias into presentation `f32`.
    ///
    /// Colour/luminance shift with Aesthetics, body size with Morphology,
    /// activity/aggression with Behavior. Each modulation is a bounded
    /// `base × (0.5 + bucket)` (so a neutral 0.5 bucket reproduces the base
    /// gene exactly — expression is a modulation, never a re-identification).
    #[must_use]
    pub fn express(&self, bias: GenomeBias) -> Expressed {
        let aes = 0.5 + bias.aesthetics;
        let morph = 0.5 + bias.morphology;
        let beh = 0.5 + bias.behavior;

        let base_hue = f32::from(self.appearance.hue) / 255.0;
        // A neutral (0.5) Aesthetics bucket rotates the hue by zero.
        let hue = (base_hue + (bias.aesthetics - 0.5) * HUE_SHIFT).rem_euclid(1.0);
        let luminance = (f32::from(self.appearance.luminance) / 255.0 * aes).clamp(0.0, 1.0);
        let size = (self.base_size() * morph).max(0.0);
        let activity = (f32::from(self.behavior.activity) / 255.0 * beh).clamp(0.0, 1.0);
        let aggression = (f32::from(self.behavior.aggression) / 255.0 * beh).clamp(0.0, 1.0);

        Expressed {
            hue,
            luminance,
            size,
            activity,
            aggression,
            form: self.appearance.form,
        }
    }
}

/// Body size (world units) of an appearance size class: an exponential ladder
/// from ~0.1 to ~12.8 units, so the food-web body-size bound has real dynamic
/// range (phase-3-plan.md §7.4).
#[inline]
#[must_use]
pub fn size_class_units(size_class: u8) -> f32 {
    0.1 * f32::from(1u16 << size_class.min(7))
}

/// The possibility bias applied when expressing a genome — the dequantized
/// Morphology / Behavior / Aesthetics buckets L8 reads (phase-3-plan.md §4.2).
/// A neutral vector ([`GenomeBias::neutral`]) reproduces the unbiased genome.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenomeBias {
    /// Morphology bucket (body size), `[0, 1]`.
    pub morphology: f32,
    /// Behavior bucket (activity/aggression), `[0, 1]`.
    pub behavior: f32,
    /// Aesthetics bucket (colour/luminance), `[0, 1]`.
    pub aesthetics: f32,
}

impl GenomeBias {
    /// The neutral bias (all buckets at 0.5): expression reproduces the base
    /// genome exactly.
    #[inline]
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            morphology: 0.5,
            behavior: 0.5,
            aesthetics: 0.5,
        }
    }
}

impl Default for GenomeBias {
    fn default() -> Self {
        Self::neutral()
    }
}

/// A genome expressed into presentation `f32` under a [`GenomeBias`]. This is
/// what near-field organisms carry (phase-3-plan.md §7.6) and what the viz
/// paints; it is presentation state, never identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Expressed {
    /// Expressed hue in `[0, 1)` around the colour wheel.
    pub hue: f32,
    /// Expressed bioluminance in `[0, 1]`.
    pub luminance: f32,
    /// Expressed body size in world units (`> 0`).
    pub size: f32,
    /// Expressed activity in `[0, 1]`.
    pub activity: f32,
    /// Expressed aggression in `[0, 1]`.
    pub aggression: f32,
    /// Morphology archetype copied from [`AppearanceGenes::form`] (`0..=15`).
    /// Presentation only; never identity or persistence.
    pub form: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_seed_is_pure_and_deterministic() {
        let g = Genome::from_seed(0x1234_5678_9ABC_DEF0);
        assert_eq!(g, Genome::from_seed(0x1234_5678_9ABC_DEF0));
    }

    #[test]
    fn sub_genomes_are_independent() {
        // Two seeds differing by one bit should not move all three sub-genomes
        // in lock-step (the salts decorrelate them).
        let a = Genome::from_seed(0);
        let b = Genome::from_seed(1);
        assert_ne!(a, b);
    }

    #[test]
    fn neutral_bias_reproduces_the_base_genome() {
        // The core "expression is modulation, not re-identification" property.
        for seed in [0u64, 1, 42, 0xDEAD_BEEF_CAFE_F00D] {
            let g = Genome::from_seed(seed);
            let e = g.express(GenomeBias::neutral());
            assert!((e.hue - f32::from(g.appearance.hue) / 255.0).abs() < 1e-6);
            assert!((e.luminance - f32::from(g.appearance.luminance) / 255.0).abs() < 1e-6);
            assert!((e.size - g.base_size()).abs() < 1e-6);
            assert!((e.activity - f32::from(g.behavior.activity) / 255.0).abs() < 1e-6);
            assert!((e.aggression - f32::from(g.behavior.aggression) / 255.0).abs() < 1e-6);
            assert_eq!(e.form, g.appearance.form);
        }
    }

    /// The five original expression fields, evaluated with the formula that
    /// predates the presentation-only `form` passthrough. Comparing bits makes
    /// this a guard against accidentally perturbing existing presentation
    /// values while extending [`Expressed`].
    fn legacy_expression_bits(genome: &Genome, bias: GenomeBias) -> [u32; 5] {
        let aes = 0.5 + bias.aesthetics;
        let morph = 0.5 + bias.morphology;
        let beh = 0.5 + bias.behavior;
        let base_hue = f32::from(genome.appearance.hue) / 255.0;

        [
            (base_hue + (bias.aesthetics - 0.5) * HUE_SHIFT)
                .rem_euclid(1.0)
                .to_bits(),
            (f32::from(genome.appearance.luminance) / 255.0 * aes)
                .clamp(0.0, 1.0)
                .to_bits(),
            (genome.base_size() * morph).max(0.0).to_bits(),
            (f32::from(genome.behavior.activity) / 255.0 * beh)
                .clamp(0.0, 1.0)
                .to_bits(),
            (f32::from(genome.behavior.aggression) / 255.0 * beh)
                .clamp(0.0, 1.0)
                .to_bits(),
        ]
    }

    #[test]
    fn form_passthrough_preserves_every_legacy_field_for_bias_range() {
        const BUCKETS: [f32; 3] = [0.0, 0.5, 1.0];

        for form in 0..=15 {
            let mut genome = Genome::from_seed(0xA11C_E55E_D15C_0A57);
            genome.appearance.form = form;
            for morphology in BUCKETS {
                for behavior in BUCKETS {
                    for aesthetics in BUCKETS {
                        let bias = GenomeBias {
                            morphology,
                            behavior,
                            aesthetics,
                        };
                        let expected = legacy_expression_bits(&genome, bias);
                        let expressed = genome.express(bias);

                        assert_eq!(expressed.form, form, "bias {bias:?}");
                        assert_eq!(
                            [
                                expressed.hue.to_bits(),
                                expressed.luminance.to_bits(),
                                expressed.size.to_bits(),
                                expressed.activity.to_bits(),
                                expressed.aggression.to_bits(),
                            ],
                            expected,
                            "form {form}, bias {bias:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn generated_forms_cover_exact_declared_range() {
        let mut seen = 0u16;
        for seed in 0..=u16::MAX {
            let form = Genome::from_seed(u64::from(seed)).appearance.form;
            assert!(form <= 15, "seed {seed} generated out-of-range form {form}");
            seen |= 1 << form;
        }
        assert_eq!(seen, u16::MAX, "the seed sweep should exercise all forms");
    }

    #[test]
    fn bias_modulates_within_bounds() {
        let g = Genome::from_seed(7);
        let bright = g.express(GenomeBias {
            aesthetics: 1.0,
            ..GenomeBias::neutral()
        });
        let dim = g.express(GenomeBias {
            aesthetics: 0.0,
            ..GenomeBias::neutral()
        });
        // Luminance responds to Aesthetics and stays bounded.
        assert!(bright.luminance >= dim.luminance);
        assert!((0.0..=1.0).contains(&bright.luminance));
        // Morphology modulates size.
        let big = g.express(GenomeBias {
            morphology: 1.0,
            ..GenomeBias::neutral()
        });
        assert!(big.size > g.base_size());
    }

    #[test]
    fn size_class_ladder_is_monotonic() {
        for sc in 0..7u8 {
            assert!(size_class_units(sc) < size_class_units(sc + 1));
        }
    }

    #[test]
    fn fingerprint_tracks_every_field() {
        let g = Genome::from_seed(100);
        let mut other = g;
        other.appearance.hue ^= 1;
        assert_ne!(g.fingerprint(), other.fingerprint());

        // `form` was already folded into the stable identity fingerprint;
        // copying it to `Expressed` must not add or reorder any hash input.
        let mut other = g;
        other.appearance.form ^= 1;
        assert_ne!(g.fingerprint(), other.fingerprint());
    }
}
