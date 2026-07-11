//! The possibility-space representation (sections 7 and 6.4 of the plan).
//!
//! Each realized location is generated from a continuous, hierarchical
//! *possibility state* rather than from a discrete world seed. This module
//! defines a small, fixed possibility vector for the Phase 1 continuity
//! prototype; it is deliberately compact and will grow into the full domain set
//! (planetary, climate, geology, hydrology, ecology, morphology, behavior,
//! aesthetics) in later phases.

/// Domains that group possibility dimensions (section 7). The Phase 1 prototype
/// uses one representative scalar per domain; later phases expand each into a
/// richer sub-vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PossibilityDomain {
    /// Large-scale planetary tendencies (e.g. ocean fraction).
    Planetary,
    /// Temperature, precipitation, seasonality.
    Climate,
    /// Tectonic activity, erosion strength.
    Geology,
    /// Surface wetness, drainage tendency.
    Hydrology,
    /// Vegetation density, ecological aggression.
    Ecology,
    /// Morphological tendencies of organisms.
    Morphology,
    /// Behavioral tendencies of organisms.
    Behavior,
    /// Color and bioluminescence tendencies.
    Aesthetics,
}

impl PossibilityDomain {
    /// All domains in stable order. The order is part of the serialization
    /// contract for [`PossibilityVector`].
    pub const ALL: [PossibilityDomain; 8] = [
        PossibilityDomain::Planetary,
        PossibilityDomain::Climate,
        PossibilityDomain::Geology,
        PossibilityDomain::Hydrology,
        PossibilityDomain::Ecology,
        PossibilityDomain::Morphology,
        PossibilityDomain::Behavior,
        PossibilityDomain::Aesthetics,
    ];

    /// Index of this domain within [`PossibilityVector`].
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Number of scalar dimensions in the Phase 1 possibility vector.
pub const POSSIBILITY_DIMS: usize = PossibilityDomain::ALL.len();

/// Quantization steps per possibility dimension (phase-2-plan.md §4.2).
/// 4096 steps ⇒ a one-bucket change moves any generated sample by well under
/// the continuity replay's per-frame epsilon, and drift smaller than a bucket
/// costs zero regeneration.
pub const POSSIBILITY_QUANT: u16 = 4096;

/// A point in possibility space: one normalized scalar per domain.
///
/// Values are conventionally in `[0, 1]`. This is presentation/simulation state,
/// so `f32` is acceptable here; permanent identities are hashed separately from
/// the *revision* number a region records for its realized state (section 6.4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PossibilityVector {
    /// One scalar per [`PossibilityDomain`], indexed by [`PossibilityDomain::index`].
    pub dims: [f32; POSSIBILITY_DIMS],
}

impl PossibilityVector {
    /// The neutral midpoint of possibility space (all dimensions at 0.5).
    #[inline]
    #[must_use]
    pub const fn neutral() -> Self {
        Self {
            dims: [0.5; POSSIBILITY_DIMS],
        }
    }

    /// Read one domain's scalar.
    #[inline]
    #[must_use]
    pub fn get(&self, domain: PossibilityDomain) -> f32 {
        self.dims[domain.index()]
    }

    /// Quantize one domain to an integer bucket in `[0, POSSIBILITY_QUANT)`.
    ///
    /// Generators consume the *dequantized* value ([`Self::dequantize`]), never
    /// the raw float, so a tile's content is a pure function of its integer
    /// dependency key (phase-2-plan.md §4.2, ADR 0008). Buckets are run-local
    /// cache keys, not cross-platform identities: `current` is runtime float
    /// state, so bucket boundaries may land differently across platforms.
    #[inline]
    #[must_use]
    pub fn quantized(&self, domain: PossibilityDomain) -> u16 {
        quantize(self.dims[domain.index()])
    }

    /// The exact `f32` a generator must consume for a bucket: the bucket
    /// center, so `quantize(dequantize(b)) == b` for every bucket.
    #[inline]
    #[must_use]
    pub fn dequantize(bucket: u16) -> f32 {
        (f32::from(bucket) + 0.5) / f32::from(POSSIBILITY_QUANT)
    }

    /// A vector carrying dequantized values for the domains set in `mask`
    /// (bit = domain index; `buckets` in [`PossibilityDomain::ALL`] order for
    /// the set bits) and neutral values elsewhere. This is the vector a layer
    /// generator sees: exactly its declared domains, nothing else — reading an
    /// undeclared domain yields the constant neutral and so cannot leak an
    /// undeclared dependency into tile content.
    #[must_use]
    pub fn from_quantized(mask: u8, buckets: &[u16]) -> Self {
        let mut v = Self::neutral();
        let mut next = 0;
        for (i, dim) in v.dims.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *dim = Self::dequantize(buckets[next]);
                next += 1;
            }
        }
        debug_assert_eq!(next, buckets.len(), "bucket count must match mask");
        v
    }

    /// The quantized buckets of the domains set in `mask`, in
    /// [`PossibilityDomain::ALL`] order — the possibility half of a layer's
    /// dependency key (phase-2-plan.md §4.3).
    #[must_use]
    pub fn quantized_domains(&self, mask: u8) -> Vec<u16> {
        let mut out = Vec::with_capacity(mask.count_ones() as usize);
        for (i, &dim) in self.dims.iter().enumerate() {
            if mask & (1 << i) != 0 {
                out.push(quantize(dim));
            }
        }
        out
    }

    /// Requantize every dimension to its bucket center. Used where a float
    /// vector (e.g. the anchor-free field base) must be collapsed onto the
    /// same value grid the generators consume.
    #[must_use]
    pub fn requantized(&self) -> Self {
        let mut v = *self;
        for dim in v.dims.iter_mut() {
            *dim = Self::dequantize(quantize(*dim));
        }
        v
    }

    /// Set one domain's scalar, clamped to `[0, 1]`.
    #[inline]
    pub fn set(&mut self, domain: PossibilityDomain, value: f32) {
        self.dims[domain.index()] = value.clamp(0.0, 1.0);
    }

    /// Linear interpolation toward `target` by `t` in `[0, 1]`.
    ///
    /// This is the primitive behind continuous convergence: distant regions step
    /// their realized state toward their target state a little each update
    /// (section 6.4), rather than snapping.
    #[inline]
    #[must_use]
    pub fn lerp(&self, target: &PossibilityVector, t: f32) -> PossibilityVector {
        let t = t.clamp(0.0, 1.0);
        let mut out = PossibilityVector::neutral();
        let mut i = 0;
        while i < POSSIBILITY_DIMS {
            out.dims[i] = self.dims[i] + (target.dims[i] - self.dims[i]) * t;
            i += 1;
        }
        out
    }
}

impl Default for PossibilityVector {
    fn default() -> Self {
        Self::neutral()
    }
}

/// Quantize a `[0, 1]` scalar to its bucket in `[0, POSSIBILITY_QUANT)`.
#[inline]
#[must_use]
fn quantize(value: f32) -> u16 {
    let v = value.clamp(0.0, 1.0);
    ((v * f32::from(POSSIBILITY_QUANT)) as u16).min(POSSIBILITY_QUANT - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_round_trips_every_bucket_edge() {
        // phase-2-plan.md §12.1: dequantize must land inside its own bucket.
        for bucket in [0u16, 1, 2047, 2048, 4094, POSSIBILITY_QUANT - 1] {
            let v = PossibilityVector::dequantize(bucket);
            let mut p = PossibilityVector::neutral();
            p.set(PossibilityDomain::Climate, v);
            assert_eq!(p.quantized(PossibilityDomain::Climate), bucket);
        }
    }

    #[test]
    fn quantize_clamps_and_covers_the_unit_interval() {
        let mut p = PossibilityVector::neutral();
        p.dims[0] = 0.0;
        assert_eq!(p.quantized(PossibilityDomain::Planetary), 0);
        p.dims[0] = 1.0;
        assert_eq!(
            p.quantized(PossibilityDomain::Planetary),
            POSSIBILITY_QUANT - 1
        );
    }

    #[test]
    fn from_quantized_reads_buckets_in_domain_order() {
        let mask =
            (1 << PossibilityDomain::Climate.index()) | (1 << PossibilityDomain::Ecology.index());
        let v = PossibilityVector::from_quantized(mask, &[100, 4000]);
        assert_eq!(
            v.get(PossibilityDomain::Climate),
            PossibilityVector::dequantize(100)
        );
        assert_eq!(
            v.get(PossibilityDomain::Ecology),
            PossibilityVector::dequantize(4000)
        );
        // Undeclared domains stay neutral.
        assert_eq!(v.get(PossibilityDomain::Geology), 0.5);
    }

    #[test]
    fn quantized_domains_matches_per_domain_quantize() {
        let mut p = PossibilityVector::neutral();
        p.set(PossibilityDomain::Planetary, 0.2);
        p.set(PossibilityDomain::Geology, 0.9);
        let mask =
            (1 << PossibilityDomain::Planetary.index()) | (1 << PossibilityDomain::Geology.index());
        assert_eq!(
            p.quantized_domains(mask),
            vec![
                p.quantized(PossibilityDomain::Planetary),
                p.quantized(PossibilityDomain::Geology)
            ]
        );
    }
}
