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
