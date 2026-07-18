//! Bounded fixed-point Egress for the World Loom Stage 0A experiment.
//!
//! The solver intentionally implements only the frozen two-law fragment. Every
//! complete result is canonical, bounded, and independently certificate-checked.

use core::fmt;
use loom_core::{
    AtomEntry, IntentTerm, Mass, MeasureKind, NormalizedIntent, PacketError, StatePacket,
    StateRoot, MASS_ONE, MAX_ACTIVE_LEVELS, MAX_ATOMS,
};
use sha2::{Digest, Sha256};

/// Fixed Stage 0A relaxation work cap.
pub const SCALING_ITERATIONS: u8 = 24;
/// Maximum returned path modes.
pub const MAX_MODES: usize = 8;
/// Maximum alternatives in addition to the default.
pub const MAX_ALTERNATIVES: usize = 2;
/// Canonical solver/checker revision.
pub const SOLVER_REVISION: u16 = 1;
/// Positive cost of activating the optional trait law.
pub const REWRITE_LENGTH: u64 = 1_000;
const BIRTH_DEATH_COST: u64 = 8;

/// One normalized path signature. Different sweeps are structurally distinct
/// certified modes while converging to the same unique endpoint in Stage 0A.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum PathSignature {
    /// Lexicographic atom sweep.
    Direct = 0,
    /// Reverse atom sweep; useful as a stable alternate route.
    Reverse = 1,
    /// Activate the optional trait law, then sweep lexicographically.
    RewriteDirect = 2,
    /// Activate the optional trait law, then sweep in reverse.
    RewriteReverse = 3,
}

impl PathSignature {
    const fn uses_rewrite(self) -> bool {
        matches!(self, Self::RewriteDirect | Self::RewriteReverse)
    }

    const fn reverse(self) -> bool {
        matches!(self, Self::Reverse | Self::RewriteReverse)
    }
}

/// Why a bounded Stage 0A request could not yield a canonical complete prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnresolvedReason {
    /// Request exceeds the declared atom/level/mode fragment.
    RequestCapExceeded,
    /// Checked fixed-point arithmetic overflowed.
    ArithmeticOverflow,
    /// No typed feasible compromise exists.
    Infeasible,
    /// Supplied length limit cannot reach a nonzero certified step.
    LengthLimit,
    /// Packet normalization rejected the generated endpoint.
    InvalidEndpoint,
}

/// A compact independently replayable Stage 0A segment witness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentCertificate {
    /// Source packet root.
    pub source_root: StateRoot,
    /// Certified endpoint root.
    pub destination_root: StateRoot,
    /// Normalized intent digest.
    pub request_digest: [u8; 32],
    /// Mode-specific path signature.
    pub signature: PathSignature,
    /// Quantized directed path length.
    pub path_length: u64,
    /// Request length limit.
    pub length_limit: u64,
    /// Exact material residual (always zero when accepted).
    pub material_residual: i64,
    /// Lower objective bound.
    pub objective_lower: u64,
    /// Upper objective bound.
    pub objective_upper: u64,
    /// Solver/checker revision.
    pub solver_revision: u16,
}

impl SegmentCertificate {
    /// Canonical certificate bytes; certificate identity is not State identity.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(132);
        bytes.extend_from_slice(b"LCRT");
        bytes.extend_from_slice(&self.solver_revision.to_be_bytes());
        bytes.extend_from_slice(&self.source_root);
        bytes.extend_from_slice(&self.destination_root);
        bytes.extend_from_slice(&self.request_digest);
        bytes.push(self.signature as u8);
        bytes.extend_from_slice(&self.path_length.to_be_bytes());
        bytes.extend_from_slice(&self.length_limit.to_be_bytes());
        bytes.extend_from_slice(&self.material_residual.to_be_bytes());
        bytes.extend_from_slice(&self.objective_lower.to_be_bytes());
        bytes.extend_from_slice(&self.objective_upper.to_be_bytes());
        bytes
    }
}

/// One certified candidate route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EgressMode {
    /// Stable id derived from canonical inputs, never discovery order.
    pub mode_id: [u8; 32],
    /// Normalized route signature.
    pub signature: PathSignature,
    /// Canonical endpoint packet.
    pub endpoint: StatePacket,
    /// Quantized directed length.
    pub path_length: u64,
    /// Weighted request miss after the step.
    pub yearning_cost: u64,
    /// Replayable witness.
    pub certificate: SegmentCertificate,
}

/// Complete Stage 0A top-three mode prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompleteProbe {
    /// Globally least certified lexicographic mode id.
    pub default_mode_id: [u8; 32],
    /// Default followed by at most two structurally distinct alternatives.
    pub modes: Vec<EgressMode>,
}

/// Bounded probe outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeOutcome {
    /// The complete selectable prefix is certified.
    Complete(CompleteProbe),
    /// No canonical answer is claimed.
    Unresolved(UnresolvedReason),
}

/// Probe one path-constrained minimizing-movement step.
#[must_use]
pub fn probe(source: &StatePacket, intent: &NormalizedIntent, length_limit: u64) -> ProbeOutcome {
    if intent
        .terms()
        .iter()
        .any(|term| term.atom >= MAX_ATOMS || term.level >= MAX_ACTIVE_LEVELS)
    {
        return ProbeOutcome::Unresolved(UnresolvedReason::RequestCapExceeded);
    }
    let needs_rewrite = !source.trait_rewrite_active()
        && intent
            .terms()
            .iter()
            .any(|term| term.kind == MeasureKind::Trait && term.delta > 0 && term.weight > 0);
    let signatures: &[PathSignature] = if needs_rewrite {
        &[PathSignature::RewriteDirect, PathSignature::RewriteReverse]
    } else {
        &[PathSignature::Direct, PathSignature::Reverse]
    };
    let mut modes = Vec::with_capacity(signatures.len());
    for &signature in signatures.iter().take(MAX_MODES) {
        match solve_mode(source, intent, length_limit, signature) {
            Ok(mode) => modes.push(mode),
            Err(UnresolvedReason::LengthLimit) => {}
            Err(reason) => return ProbeOutcome::Unresolved(reason),
        }
    }
    if modes.is_empty() {
        return ProbeOutcome::Unresolved(UnresolvedReason::LengthLimit);
    }
    modes.sort_by_key(|mode| {
        (
            mode.path_length,
            mode.yearning_cost,
            mode.signature,
            mode.mode_id,
        )
    });
    modes.truncate(1 + MAX_ALTERNATIVES);
    let default_mode_id = modes[0].mode_id;
    ProbeOutcome::Complete(CompleteProbe {
        default_mode_id,
        modes,
    })
}

fn solve_mode(
    source: &StatePacket,
    intent: &NormalizedIntent,
    length_limit: u64,
    signature: PathSignature,
) -> Result<EgressMode, UnresolvedReason> {
    let mut masses = [[[0u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize]; 2];
    for entry in source.entries() {
        masses[entry.kind as usize][entry.level as usize][entry.atom as usize] = entry.mass.raw();
    }
    let mut desired = masses;
    for term in intent.terms() {
        if term.weight == 0 || term.delta == 0 {
            continue;
        }
        let weighted = i64::from(term.delta)
            .checked_mul(i64::from(term.weight))
            .ok_or(UnresolvedReason::ArithmeticOverflow)?;
        let slot = &mut desired[term.kind as usize][term.level as usize][term.atom as usize];
        let next = i64::from(*slot)
            .checked_add(weighted)
            .ok_or(UnresolvedReason::ArithmeticOverflow)?;
        *slot = u32::try_from(next.clamp(0, i64::from(MASS_ONE)))
            .map_err(|_| UnresolvedReason::ArithmeticOverflow)?;
    }

    // Fixed-count deterministic relaxation. Each pass restores the material
    // inventory and trait capacity; later passes are stable fixed points.
    for _ in 0..SCALING_ITERATIONS {
        restore_total(
            &mut desired[MeasureKind::Material as usize],
            source.material_total().raw(),
            signature.reverse(),
        )?;
        cap_total(
            &mut desired[MeasureKind::Trait as usize],
            source.trait_capacity().raw(),
            signature.reverse(),
        );
    }

    let mut entries = Vec::new();
    for kind in [MeasureKind::Material, MeasureKind::Trait] {
        for level in 0..MAX_ACTIVE_LEVELS {
            for atom in 0..MAX_ATOMS {
                let raw = desired[kind as usize][level as usize][atom as usize];
                if raw != 0 {
                    entries.push(AtomEntry {
                        kind,
                        level,
                        atom,
                        mass: Mass::new(raw).map_err(|_| UnresolvedReason::InvalidEndpoint)?,
                    });
                }
            }
        }
    }
    let endpoint =
        StatePacket::normalize(entries, source.material_total(), source.trait_capacity())
            .map_err(|_| UnresolvedReason::InvalidEndpoint)?;
    let transport = transport_cost(&masses, &desired)?;
    let rewrite = if signature.uses_rewrite() && endpoint.trait_rewrite_active() {
        REWRITE_LENGTH
    } else {
        0
    };
    let path_length = transport
        .checked_add(rewrite)
        .ok_or(UnresolvedReason::ArithmeticOverflow)?;
    if path_length > length_limit {
        return Err(UnresolvedReason::LengthLimit);
    }
    let yearning_cost = yearning_cost(&desired, intent)?;
    let certificate = SegmentCertificate {
        source_root: source.root(),
        destination_root: endpoint.root(),
        request_digest: intent.digest(),
        signature,
        path_length,
        length_limit,
        material_residual: 0,
        objective_lower: transport,
        objective_upper: transport,
        solver_revision: SOLVER_REVISION,
    };
    let mode_id = mode_id(source, intent, &endpoint, signature);
    Ok(EgressMode {
        mode_id,
        signature,
        endpoint,
        path_length,
        yearning_cost,
        certificate,
    })
}

fn restore_total(
    values: &mut [[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize],
    required: u32,
    reverse: bool,
) -> Result<(), UnresolvedReason> {
    let sum = values.iter().flatten().try_fold(0u32, |sum, &value| {
        sum.checked_add(value)
            .ok_or(UnresolvedReason::ArithmeticOverflow)
    })?;
    if sum < required {
        adjust(values, required - sum, true, reverse);
    } else if sum > required {
        adjust(values, sum - required, false, reverse);
    }
    Ok(())
}

fn cap_total(
    values: &mut [[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize],
    capacity: u32,
    reverse: bool,
) {
    let sum = values
        .iter()
        .flatten()
        .fold(0u64, |sum, &value| sum + u64::from(value));
    if sum > u64::from(capacity) {
        adjust(values, (sum - u64::from(capacity)) as u32, false, reverse);
    }
}

fn adjust(
    values: &mut [[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize],
    mut amount: u32,
    add: bool,
    reverse: bool,
) {
    for index in 0..usize::from(MAX_ACTIVE_LEVELS) * usize::from(MAX_ATOMS) {
        let flat = if reverse {
            usize::from(MAX_ACTIVE_LEVELS) * usize::from(MAX_ATOMS) - 1 - index
        } else {
            index
        };
        let slot = &mut values[flat / usize::from(MAX_ATOMS)][flat % usize::from(MAX_ATOMS)];
        let room = if add { MASS_ONE - *slot } else { *slot };
        let change = room.min(amount);
        if add {
            *slot += change;
        } else {
            *slot -= change;
        }
        amount -= change;
        if amount == 0 {
            break;
        }
    }
}

fn transport_cost(
    source: &[[[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize]; 2],
    target: &[[[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize]; 2],
) -> Result<u64, UnresolvedReason> {
    let mut cost = 0u64;
    for kind in [MeasureKind::Material, MeasureKind::Trait] {
        for level in 0..MAX_ACTIVE_LEVELS as usize {
            let mut imbalance = 0i64;
            for atom in 0..MAX_ATOMS as usize {
                imbalance += i64::from(source[kind as usize][level][atom])
                    - i64::from(target[kind as usize][level][atom]);
                cost = cost
                    .checked_add(imbalance.unsigned_abs())
                    .ok_or(UnresolvedReason::ArithmeticOverflow)?;
            }
            if kind == MeasureKind::Trait {
                cost = cost
                    .checked_add(imbalance.unsigned_abs().saturating_mul(BIRTH_DEATH_COST))
                    .ok_or(UnresolvedReason::ArithmeticOverflow)?;
            } else if imbalance != 0 {
                return Err(UnresolvedReason::Infeasible);
            }
        }
    }
    Ok(cost)
}

fn yearning_cost(
    target: &[[[u32; MAX_ATOMS as usize]; MAX_ACTIVE_LEVELS as usize]; 2],
    intent: &NormalizedIntent,
) -> Result<u64, UnresolvedReason> {
    intent.terms().iter().try_fold(0u64, |cost, term| {
        let actual = i64::from(target[term.kind as usize][term.level as usize][term.atom as usize]);
        let miss = actual
            .saturating_sub(i64::from(term.delta).max(0))
            .unsigned_abs();
        cost.checked_add(miss.saturating_mul(u64::from(term.weight)))
            .ok_or(UnresolvedReason::ArithmeticOverflow)
    })
}

fn mode_id(
    source: &StatePacket,
    intent: &NormalizedIntent,
    endpoint: &StatePacket,
    signature: PathSignature,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"loom-mode-v1");
    digest.update(source.root());
    digest.update(intent.digest());
    digest.update([signature as u8]);
    digest.update(endpoint.root());
    digest.update(SOLVER_REVISION.to_be_bytes());
    digest.finalize().into()
}

/// Verify a certificate without trusting its producer.
pub fn verify_certificate(
    source: &StatePacket,
    intent: &NormalizedIntent,
    mode: &EgressMode,
) -> Result<(), CertificateError> {
    let certificate = &mode.certificate;
    if certificate.solver_revision != SOLVER_REVISION
        || certificate.source_root != source.root()
        || certificate.destination_root != mode.endpoint.root()
        || certificate.request_digest != intent.digest()
        || certificate.signature != mode.signature
        || certificate.path_length != mode.path_length
        || certificate.path_length > certificate.length_limit
        || certificate.material_residual != 0
        || certificate.objective_lower > certificate.objective_upper
        || mode.mode_id != mode_id(source, intent, &mode.endpoint, mode.signature)
    {
        return Err(CertificateError::Mismatch);
    }
    let replay = probe(source, intent, certificate.length_limit);
    let ProbeOutcome::Complete(replay) = replay else {
        return Err(CertificateError::ReplayUnresolved);
    };
    let Some(replayed) = replay
        .modes
        .iter()
        .find(|candidate| candidate.mode_id == mode.mode_id)
    else {
        return Err(CertificateError::ModeMissing);
    };
    if replayed != mode {
        return Err(CertificateError::Mismatch);
    }
    Ok(())
}

/// Certificate validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CertificateError {
    /// A committed field does not match canonical inputs.
    Mismatch,
    /// Replaying the bounded solver did not resolve.
    ReplayUnresolved,
    /// The certified mode was absent from the complete prefix.
    ModeMissing,
}

impl fmt::Display for CertificateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CertificateError {}

/// Purely select one returned mode.
pub fn select<'a>(probe: &'a CompleteProbe, mode_id: &[u8; 32]) -> Option<&'a EgressMode> {
    probe.modes.iter().find(|mode| &mode.mode_id == mode_id)
}

/// Commit a selected segment only when cumulative credit reaches its complete
/// quantized length. Stage 0A has one checkpoint, so a smaller or zero budget
/// remains exactly at the source and is retained as unused credit.
#[must_use]
pub fn advance(
    source: &StatePacket,
    selected: &EgressMode,
    cumulative_credit: u64,
) -> (StatePacket, u64) {
    if cumulative_credit < selected.path_length {
        (source.clone(), cumulative_credit)
    } else {
        (
            selected.endpoint.clone(),
            cumulative_credit - selected.path_length,
        )
    }
}

/// Convenience fixture shared by native and wasm parity tests.
pub fn parity_fixture() -> Result<(StatePacket, NormalizedIntent, CompleteProbe), FixtureError> {
    let source = StatePacket::normalize(
        vec![
            AtomEntry {
                kind: MeasureKind::Material,
                level: 0,
                atom: 0,
                mass: Mass::new(MASS_ONE / 2)?,
            },
            AtomEntry {
                kind: MeasureKind::Material,
                level: 0,
                atom: 2,
                mass: Mass::new(MASS_ONE / 2)?,
            },
        ],
        Mass::ONE,
        Mass::ONE,
    )?;
    let intent = NormalizedIntent::normalize(vec![
        IntentTerm {
            id: 9,
            kind: MeasureKind::Trait,
            level: 0,
            atom: 3,
            delta: 1 << 20,
            weight: 1,
        },
        IntentTerm {
            id: 4,
            kind: MeasureKind::Material,
            level: 0,
            atom: 1,
            delta: 1 << 19,
            weight: 2,
        },
    ])?;
    let ProbeOutcome::Complete(probe) = probe(&source, &intent, u64::MAX) else {
        return Err(FixtureError::Unresolved);
    };
    Ok((source, intent, probe))
}

/// Check the frozen Stage 0A cross-platform byte vector.
#[must_use]
pub fn frozen_parity_vector_matches() -> bool {
    let Ok((source, intent, probe)) = parity_fixture() else {
        return false;
    };
    let expected_source = [
        0x84, 0x76, 0x32, 0x7a, 0x4b, 0xb6, 0x79, 0xa9, 0xe0, 0x54, 0x31, 0x32, 0x07, 0x61, 0x56,
        0xa7, 0x33, 0x9c, 0x3c, 0x63, 0x0c, 0xf4, 0xe3, 0x02, 0xb2, 0x26, 0x04, 0xd7, 0x5f, 0x8d,
        0xab, 0xfc,
    ];
    let expected_intent = [
        0x7a, 0x19, 0x7c, 0x64, 0x30, 0xd5, 0x2e, 0xa7, 0x68, 0xbb, 0x8f, 0xa9, 0x63, 0x80, 0x14,
        0xed, 0x28, 0x49, 0x79, 0x50, 0x12, 0x3d, 0xac, 0x08, 0xb7, 0x94, 0x40, 0x48, 0x64, 0xa7,
        0x32, 0x93,
    ];
    let expected_modes = [
        [
            0x9f, 0x41, 0x2d, 0xda, 0xf2, 0x7d, 0x39, 0xac, 0x13, 0x0d, 0xbf, 0x7a, 0xee, 0x62,
            0x04, 0x15, 0x0e, 0x5b, 0x35, 0x07, 0xa6, 0x13, 0xee, 0x1d, 0xe5, 0xfe, 0xf4, 0x42,
            0x80, 0x2f, 0x89, 0xef,
        ],
        [
            0x31, 0x22, 0xc1, 0x53, 0x5f, 0xad, 0xd5, 0x1e, 0x90, 0x10, 0xe1, 0x5f, 0x03, 0xf6,
            0xd1, 0x5e, 0x21, 0x96, 0xac, 0x06, 0xb6, 0xca, 0xd5, 0x15, 0x3f, 0xb5, 0x2f, 0x4f,
            0x7c, 0xc1, 0x6a, 0xbf,
        ],
    ];
    let expected_endpoints = [
        [
            0xbb, 0x54, 0xf4, 0xee, 0x8f, 0x75, 0xcd, 0x49, 0x68, 0x75, 0xde, 0x0e, 0x70, 0xfe,
            0x6e, 0xea, 0x99, 0x85, 0x8c, 0x0c, 0x23, 0xf9, 0xa7, 0xe6, 0xff, 0x0b, 0x5d, 0xb4,
            0x69, 0xac, 0xe2, 0x2e,
        ],
        [
            0x4b, 0x59, 0xb1, 0x7b, 0x27, 0xac, 0xe9, 0xa9, 0x01, 0xa5, 0x02, 0xad, 0x0a, 0x94,
            0xc1, 0x3c, 0xf0, 0x26, 0x9c, 0x81, 0x64, 0x04, 0xbb, 0x32, 0x6d, 0x8b, 0xb1, 0x73,
            0xde, 0x69, 0xa2, 0x40,
        ],
    ];
    let expected_certificates = [
        [
            0x46, 0x3b, 0x4e, 0x3b, 0xde, 0xf7, 0x91, 0x7c, 0x0f, 0x87, 0x76, 0xeb, 0x1f, 0x4b,
            0x58, 0x19, 0xb3, 0xdf, 0xa9, 0x6e, 0xe1, 0xc7, 0x73, 0xda, 0x5f, 0xaa, 0xcb, 0xda,
            0x5e, 0x00, 0xf6, 0x2e,
        ],
        [
            0xd0, 0xc9, 0xc9, 0xaf, 0xf3, 0x44, 0x31, 0x17, 0xb4, 0x59, 0x4c, 0x01, 0xd6, 0x16,
            0x3b, 0x69, 0xec, 0x9c, 0x67, 0x0a, 0x3a, 0x35, 0x33, 0x0b, 0x23, 0xca, 0x78, 0xa5,
            0x05, 0x4e, 0x74, 0x52,
        ],
    ];
    source.root() == expected_source
        && intent.digest() == expected_intent
        && probe.modes.len() == 2
        && probe
            .modes
            .iter()
            .zip(expected_modes)
            .all(|(mode, expected)| mode.mode_id == expected)
        && probe
            .modes
            .iter()
            .zip(expected_endpoints)
            .all(|(mode, expected)| mode.endpoint.root() == expected)
        && probe
            .modes
            .iter()
            .zip(expected_certificates)
            .all(|(mode, expected)| {
                <[u8; 32]>::from(Sha256::digest(mode.certificate.canonical_bytes())) == expected
            })
}

/// Frozen-fixture construction failure.
#[derive(Debug)]
pub enum FixtureError {
    /// Packet validation failed.
    Packet(PacketError),
    /// Intent normalization failed.
    Intent(loom_core::IntentError),
    /// Solver unexpectedly failed to resolve.
    Unresolved,
}

impl From<PacketError> for FixtureError {
    fn from(value: PacketError) -> Self {
        Self::Packet(value)
    }
}

impl From<loom_core::IntentError> for FixtureError {
    fn from(value: loom_core::IntentError) -> Self {
        Self::Intent(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_complete_and_certificates_replay() {
        let (source, intent, probe) = parity_fixture().unwrap();
        assert_eq!(probe.modes.len(), 2);
        for mode in &probe.modes {
            verify_certificate(&source, &intent, mode).unwrap();
        }
    }

    #[test]
    fn frozen_native_vector_matches() {
        assert!(frozen_parity_vector_matches());
    }

    #[test]
    fn zero_and_split_credit_are_cadence_independent() {
        let (source, _, probe) = parity_fixture().unwrap();
        let selected = select(&probe, &probe.default_mode_id).unwrap();
        assert_eq!(advance(&source, selected, 0).0, source);
        let total = selected.path_length;
        let one = advance(&source, selected, total);
        let staged_credit = total / 3 + total - total / 3;
        let staged = advance(&source, selected, staged_credit);
        assert_eq!(one, staged);
    }

    #[test]
    fn insufficient_length_is_explicitly_unresolved() {
        let (source, intent, _) = parity_fixture().unwrap();
        assert_eq!(
            probe(&source, &intent, 0),
            ProbeOutcome::Unresolved(UnresolvedReason::LengthLimit)
        );
    }
}
