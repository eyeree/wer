//! Canonical, topology-independent State Packets for World Loom Stage 0A.
//!
//! This is the bounded kernel described by
//! `docs/new-world-model-option-4-implemenation.md`: integers and canonical
//! bytes are authoritative, while hosts own clocks, storage, and execution.

use core::fmt;
use sha2::{Digest, Sha256};

/// Fractional bits in the Stage 0A nonnegative mass format.
pub const MASS_FRACTIONAL_BITS: u32 = 24;
/// One whole unit in Q24.
pub const MASS_ONE: u32 = 1 << MASS_FRACTIONAL_BITS;
/// Maximum atom count in one Stage 0A motif space.
pub const MAX_ATOMS: u8 = 64;
/// Maximum active levels considered by the Stage 0A solver.
pub const MAX_ACTIVE_LEVELS: u8 = 4;
/// Architectural packet-entry ceiling.
pub const MAX_PACKET_ENTRIES: usize = 4_096;
/// Architectural canonical packet byte ceiling.
pub const MAX_PACKET_BYTES: usize = 64 * 1024;
/// Canonical packet codec version.
pub const PACKET_FORMAT_VERSION: u16 = 1;
/// Stage 0A two-law grammar version.
pub const PROGRAM_VERSION: u16 = 1;

/// A checked nonnegative Q24 quantity.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Mass(u32);

impl Mass {
    /// Zero mass.
    pub const ZERO: Self = Self(0);
    /// One Q24 unit.
    pub const ONE: Self = Self(MASS_ONE);

    /// Construct a representable Stage 0A fraction.
    pub const fn new(raw: u32) -> Result<Self, PacketError> {
        if raw <= MASS_ONE {
            Ok(Self(raw))
        } else {
            Err(PacketError::MassOutOfRange)
        }
    }

    /// Return the canonical integer representation.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// The two typed measure laws supported by Stage 0A.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum MeasureKind {
    /// Exactly conserved material inventory.
    Material = 0,
    /// Trait capacity with licensed creation and destruction.
    Trait = 1,
}

impl TryFrom<u8> for MeasureKind {
    type Error = PacketError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Material),
            1 => Ok(Self::Trait),
            _ => Err(PacketError::UnknownMeasureKind),
        }
    }
}

/// One possibly duplicated input entry; normalization creates canonical atoms.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AtomEntry {
    /// Typed law owning the atom.
    pub kind: MeasureKind,
    /// Active law level.
    pub level: u8,
    /// Motif atom index.
    pub atom: u8,
    /// Absolute nonnegative allocation.
    pub mass: Mass,
}

/// Stable identity of a normalized packet.
pub type StateRoot = [u8; 32];

/// A validated canonical Stage 0A State Packet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatePacket {
    entries: Vec<AtomEntry>,
    material_total: Mass,
    trait_capacity: Mass,
    trait_rewrite_active: bool,
    bytes: Vec<u8>,
    root: StateRoot,
}

impl StatePacket {
    /// Normalize possibly unordered/duplicated entries into one packet.
    pub fn normalize(
        mut entries: Vec<AtomEntry>,
        material_total: Mass,
        trait_capacity: Mass,
    ) -> Result<Self, PacketError> {
        if entries.len() > MAX_PACKET_ENTRIES {
            return Err(PacketError::TooManyEntries);
        }
        for entry in &entries {
            if entry.level >= MAX_ACTIVE_LEVELS {
                return Err(PacketError::LevelOutOfRange);
            }
            if entry.atom >= MAX_ATOMS {
                return Err(PacketError::AtomOutOfRange);
            }
        }
        entries.sort_by_key(|entry| (entry.kind, entry.level, entry.atom));
        let mut folded: Vec<AtomEntry> = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.mass == Mass::ZERO {
                continue;
            }
            if let Some(previous) = folded.last_mut().filter(|previous| {
                (previous.kind, previous.level, previous.atom)
                    == (entry.kind, entry.level, entry.atom)
            }) {
                let raw = previous
                    .mass
                    .raw()
                    .checked_add(entry.mass.raw())
                    .ok_or(PacketError::ArithmeticOverflow)?;
                previous.mass = Mass::new(raw)?;
            } else {
                folded.push(entry);
            }
        }
        let material_sum = checked_sum(&folded, MeasureKind::Material)?;
        let trait_sum = checked_sum(&folded, MeasureKind::Trait)?;
        if material_sum != material_total.raw() {
            return Err(PacketError::MaterialInventoryMismatch);
        }
        if trait_sum > trait_capacity.raw() {
            return Err(PacketError::TraitCapacityExceeded);
        }
        let trait_rewrite_active = trait_sum != 0;
        let bytes = encode_fields(
            &folded,
            material_total,
            trait_capacity,
            trait_rewrite_active,
        )?;
        let root = Sha256::digest(&bytes).into();
        Ok(Self {
            entries: folded,
            material_total,
            trait_capacity,
            trait_rewrite_active,
            bytes,
            root,
        })
    }

    /// Decode canonical bytes, rejecting alternative encodings.
    pub fn decode(bytes: &[u8]) -> Result<Self, PacketError> {
        if bytes.len() > MAX_PACKET_BYTES {
            return Err(PacketError::PacketTooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        if cursor.take(4)? != b"LOOM" {
            return Err(PacketError::BadMagic);
        }
        if cursor.u16()? != PACKET_FORMAT_VERSION {
            return Err(PacketError::UnsupportedPacketVersion);
        }
        if cursor.u16()? != PROGRAM_VERSION {
            return Err(PacketError::UnsupportedProgramVersion);
        }
        let flags = cursor.u8()?;
        if flags & !1 != 0 {
            return Err(PacketError::NonCanonicalEncoding);
        }
        let material_total = Mass::new(cursor.u32()?)?;
        let trait_capacity = Mass::new(cursor.u32()?)?;
        let count = usize::from(cursor.u16()?);
        if count > MAX_PACKET_ENTRIES {
            return Err(PacketError::TooManyEntries);
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(AtomEntry {
                kind: MeasureKind::try_from(cursor.u8()?)?,
                level: cursor.u8()?,
                atom: cursor.u8()?,
                mass: Mass::new(cursor.u32()?)?,
            });
        }
        if !cursor.finished() {
            return Err(PacketError::TrailingBytes);
        }
        let packet = Self::normalize(entries, material_total, trait_capacity)?;
        if packet.trait_rewrite_active != (flags == 1) || packet.bytes != bytes {
            return Err(PacketError::NonCanonicalEncoding);
        }
        Ok(packet)
    }

    /// Canonical packet bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA-256 of the canonical payload.
    pub const fn root(&self) -> StateRoot {
        self.root
    }

    /// Canonical sparse entries.
    pub fn entries(&self) -> &[AtomEntry] {
        &self.entries
    }

    /// Exact material inventory declared by this packet.
    pub const fn material_total(&self) -> Mass {
        self.material_total
    }

    /// Maximum licensed trait allocation.
    pub const fn trait_capacity(&self) -> Mass {
        self.trait_capacity
    }

    /// Whether the optional trait law survives normalization.
    pub const fn trait_rewrite_active(&self) -> bool {
        self.trait_rewrite_active
    }

    /// Read one atom, returning zero for omitted sparse entries.
    pub fn mass(&self, kind: MeasureKind, level: u8, atom: u8) -> Mass {
        self.entries
            .binary_search_by_key(&(kind, level, atom), |entry| {
                (entry.kind, entry.level, entry.atom)
            })
            .map_or(Mass::ZERO, |index| self.entries[index].mass)
    }
}

fn checked_sum(entries: &[AtomEntry], kind: MeasureKind) -> Result<u32, PacketError> {
    entries
        .iter()
        .filter(|entry| entry.kind == kind)
        .try_fold(0u32, |sum, entry| {
            sum.checked_add(entry.mass.raw())
                .ok_or(PacketError::ArithmeticOverflow)
        })
}

fn encode_fields(
    entries: &[AtomEntry],
    material_total: Mass,
    trait_capacity: Mass,
    rewrite: bool,
) -> Result<Vec<u8>, PacketError> {
    let count = u16::try_from(entries.len()).map_err(|_| PacketError::TooManyEntries)?;
    let mut bytes = Vec::with_capacity(19 + entries.len() * 7);
    bytes.extend_from_slice(b"LOOM");
    bytes.extend_from_slice(&PACKET_FORMAT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&PROGRAM_VERSION.to_be_bytes());
    bytes.push(u8::from(rewrite));
    bytes.extend_from_slice(&material_total.raw().to_be_bytes());
    bytes.extend_from_slice(&trait_capacity.raw().to_be_bytes());
    bytes.extend_from_slice(&count.to_be_bytes());
    for entry in entries {
        bytes.push(entry.kind as u8);
        bytes.push(entry.level);
        bytes.push(entry.atom);
        bytes.extend_from_slice(&entry.mass.raw().to_be_bytes());
    }
    if bytes.len() > MAX_PACKET_BYTES {
        return Err(PacketError::PacketTooLarge);
    }
    Ok(bytes)
}

/// One raw weighted request term. Equal ids must carry equal content.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IntentTerm {
    /// Caller-stable term identity.
    pub id: u64,
    /// Target law.
    pub kind: MeasureKind,
    /// Active level.
    pub level: u8,
    /// Target motif atom.
    pub atom: u8,
    /// Signed Q24 desired change before weight.
    pub delta: i32,
    /// Nonnegative integer importance.
    pub weight: u16,
}

/// Canonical order-independent Stage 0A request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedIntent {
    terms: Vec<IntentTerm>,
    bytes: Vec<u8>,
    digest: [u8; 32],
}

impl NormalizedIntent {
    /// Sort, validate, and deduplicate an intent multiset.
    pub fn normalize(mut terms: Vec<IntentTerm>) -> Result<Self, IntentError> {
        for term in &terms {
            if term.level >= MAX_ACTIVE_LEVELS || term.atom >= MAX_ATOMS {
                return Err(IntentError::AddressOutOfRange);
            }
        }
        terms.sort();
        let mut canonical: Vec<IntentTerm> = Vec::with_capacity(terms.len());
        for term in terms {
            if let Some(previous) = canonical.last() {
                if previous.id == term.id && previous != &term {
                    return Err(IntentError::ConflictingId);
                }
                if previous == &term {
                    continue;
                }
            }
            canonical.push(term);
        }
        let count = u16::try_from(canonical.len()).map_err(|_| IntentError::TooManyTerms)?;
        let mut bytes = Vec::with_capacity(2 + canonical.len() * 18);
        bytes.extend_from_slice(&count.to_be_bytes());
        for term in &canonical {
            bytes.extend_from_slice(&term.id.to_be_bytes());
            bytes.push(term.kind as u8);
            bytes.push(term.level);
            bytes.push(term.atom);
            bytes.extend_from_slice(&term.delta.to_be_bytes());
            bytes.extend_from_slice(&term.weight.to_be_bytes());
        }
        let digest = Sha256::digest(&bytes).into();
        Ok(Self {
            terms: canonical,
            bytes,
            digest,
        })
    }

    /// Canonical terms.
    pub fn terms(&self) -> &[IntentTerm] {
        &self.terms
    }

    /// Canonical bytes.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Domain-local request digest.
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// Packet construction/codec failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError {
    /// Q24 value exceeded one.
    MassOutOfRange,
    /// Checked integer arithmetic overflowed.
    ArithmeticOverflow,
    /// More than 4,096 entries were supplied.
    TooManyEntries,
    /// Canonical bytes exceeded 64 KiB.
    PacketTooLarge,
    /// Atom was outside the 64-atom space.
    AtomOutOfRange,
    /// Level was outside the Stage 0A active range.
    LevelOutOfRange,
    /// Conserved material did not equal its declared total.
    MaterialInventoryMismatch,
    /// Trait allocation exceeded its capacity.
    TraitCapacityExceeded,
    /// Codec magic was invalid.
    BadMagic,
    /// Packet codec version is unsupported.
    UnsupportedPacketVersion,
    /// Program normal-form version is unsupported.
    UnsupportedProgramVersion,
    /// Measure tag is unknown.
    UnknownMeasureKind,
    /// Input ended before the declared packet did.
    Truncated,
    /// Bytes remained after the packet.
    TrailingBytes,
    /// Bytes decoded but were not the unique normalized encoding.
    NonCanonicalEncoding,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PacketError {}

/// Intent normalization failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentError {
    /// Atom or level lies outside Stage 0A.
    AddressOutOfRange,
    /// One stable id was reused for different content.
    ConflictingId,
    /// Term count cannot be represented by the codec.
    TooManyTerms,
}

impl fmt::Display for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for IntentError {}

#[derive(Debug)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], PacketError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(PacketError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PacketError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, PacketError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PacketError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte cursor slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, PacketError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte cursor slice"),
        ))
    }

    const fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mass(raw: u32) -> Mass {
        Mass::new(raw).unwrap()
    }

    #[test]
    fn packet_normalization_is_unique_and_round_trips() {
        let packet = StatePacket::normalize(
            vec![
                AtomEntry {
                    kind: MeasureKind::Material,
                    level: 0,
                    atom: 2,
                    mass: mass(3),
                },
                AtomEntry {
                    kind: MeasureKind::Material,
                    level: 0,
                    atom: 1,
                    mass: mass(4),
                },
                AtomEntry {
                    kind: MeasureKind::Material,
                    level: 0,
                    atom: 2,
                    mass: mass(5),
                },
                AtomEntry {
                    kind: MeasureKind::Trait,
                    level: 0,
                    atom: 7,
                    mass: Mass::ZERO,
                },
            ],
            mass(12),
            Mass::ONE,
        )
        .unwrap();
        assert_eq!(packet.entries().len(), 2);
        assert!(!packet.trait_rewrite_active());
        assert_eq!(
            StatePacket::decode(packet.canonical_bytes()).unwrap(),
            packet
        );
    }

    #[test]
    fn decoder_rejects_noncanonical_order() {
        let packet = StatePacket::normalize(
            vec![AtomEntry {
                kind: MeasureKind::Material,
                level: 0,
                atom: 0,
                mass: Mass::ONE,
            }],
            Mass::ONE,
            Mass::ONE,
        )
        .unwrap();
        let mut bytes = packet.canonical_bytes().to_vec();
        bytes[8] = 1;
        assert!(StatePacket::decode(&bytes).is_err());
    }

    #[test]
    fn intent_is_permutation_and_duplicate_invariant() {
        let a = IntentTerm {
            id: 2,
            kind: MeasureKind::Trait,
            level: 0,
            atom: 3,
            delta: 5,
            weight: 1,
        };
        let b = IntentTerm {
            id: 1,
            kind: MeasureKind::Material,
            level: 0,
            atom: 2,
            delta: -2,
            weight: 3,
        };
        let left = NormalizedIntent::normalize(vec![a, b, a]).unwrap();
        let right = NormalizedIntent::normalize(vec![b, a]).unwrap();
        assert_eq!(left, right);
    }
}
