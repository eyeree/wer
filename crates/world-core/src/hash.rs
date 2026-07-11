//! Deterministic, portable integer hashing and pseudo-random number generation.
//!
//! Every permanent identity in the world (feature ids, organism genomes, spawn
//! decisions) is derived here from stable integer inputs, never from a sequential
//! random stream or platform-dependent floating point. The same inputs must
//! produce the same bits on native x86_64, native ARM, and `wasm32` (section 6.2
//! and 23.5 of `implementation-plan.md`).

use crate::coord::RegionCoord;

/// The [`splitmix64`] increment (fractional bits of the golden ratio).
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// `splitmix64` finalizer — a well-known, portable, high-quality integer mix.
///
/// Used as the primitive from which [`mix`], [`feature_hash`], and [`Rng`] are
/// built. It is `const` so hashes can be computed in const contexts.
#[inline]
#[must_use]
pub const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(GOLDEN_GAMMA);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fold `value` into a running hash `seed`. Order-dependent by design so that
/// hashing a fixed sequence of fields yields a stable identity.
#[inline]
#[must_use]
pub const fn mix(seed: u64, value: u64) -> u64 {
    splitmix64(seed ^ value.wrapping_mul(GOLDEN_GAMMA))
}

/// The stable inputs that identify a single generated feature.
///
/// This mirrors the reproducibility recipe in section 6.2 of the plan:
/// `world version + region coordinate + generator layer + feature index +
/// possibility state revision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureKey {
    /// [`crate::WORLD_ALGORITHM_VERSION`] at generation time.
    pub world_version: u32,
    /// Which region the feature belongs to.
    pub region: RegionCoord,
    /// Which generator layer produced it (climate, hydrology, ...).
    pub layer: u16,
    /// Index of the feature within its region+layer.
    pub feature_index: u32,
    /// Revision of the realized possibility state the feature was generated from.
    pub possibility_revision: u32,
}

impl FeatureKey {
    /// Deterministic 64-bit identity for this feature.
    #[inline]
    #[must_use]
    pub const fn hash(&self) -> u64 {
        feature_hash(self)
    }
}

/// Deterministic 64-bit identity for a [`FeatureKey`].
///
/// The field fold order is part of the stable contract — changing it changes
/// every id in every world, and so would require a [`crate::WORLD_ALGORITHM_VERSION`]
/// bump.
#[inline]
#[must_use]
pub const fn feature_hash(key: &FeatureKey) -> u64 {
    let mut h: u64 = 0xA5A5_5A5A_C3C3_3C3C; // arbitrary fixed basis
    h = mix(h, key.world_version as u64);
    // Region coordinates are folded as unsigned bit-patterns for portability.
    h = mix(h, key.region.x as u32 as u64);
    h = mix(h, key.region.y as u32 as u64);
    h = mix(h, key.region.level as u64);
    h = mix(h, key.layer as u64);
    h = mix(h, key.feature_index as u64);
    h = mix(h, key.possibility_revision as u64);
    h
}

/// A small, portable, deterministic PRNG seeded from a 64-bit state.
///
/// This is a `splitmix64` stream. It is intended for approximate sampling
/// (jitter, distributions) seeded *from* a stable hash such as
/// [`feature_hash`]; it is not itself a source of permanent identity, so its
/// float outputs are acceptable.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seed the generator. Any 64-bit value is valid.
    #[inline]
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Seed the generator from a [`FeatureKey`]'s deterministic hash.
    #[inline]
    #[must_use]
    pub const fn from_key(key: &FeatureKey) -> Self {
        Self::new(feature_hash(key))
    }

    /// Next 64-bit output, advancing the stream.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN_GAMMA);
        splitmix64(self.state)
    }

    /// Next 32-bit output.
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform `f32` in `[0, 1)` built from 24 mantissa bits.
    ///
    /// The integer-to-float step uses only exact, IEEE-754-portable operations,
    /// so it is stable across platforms (though it should still not be used for
    /// permanent identities).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        // 24 high bits -> [0, 2^24) -> divide by 2^24.
        let bits = (self.next_u64() >> 40) as u32; // top 24 bits
        (bits as f32) * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform integer in `[0, bound)` using Lemire's biased-but-fast reduction.
    /// Returns 0 when `bound == 0`.
    #[inline]
    pub fn next_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        let product = (self.next_u32() as u64) * (bound as u64);
        (product >> 32) as u32
    }
}
