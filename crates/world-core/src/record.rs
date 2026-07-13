//! The Phase 5 record schema and codec — the persistence boundary
//! (implementation-plan.md sections 12.4, 13, 18; phase-5-plan.md §4, §7.1–7.2).
//!
//! Everything that outlives a process crosses through this module, and the
//! module enforces the two-tier policy of ADR 0013:
//!
//! - **Shareable records** ([`DiscoveryRecord`], [`RouteRecord`],
//!   [`PreserveRecord`], [`SeenRecord`], [`AtlasBundle`]) carry **only integers
//!   and strings**. `f32` values are quantized on write (possibility vectors
//!   through the [`POSSIBILITY_QUANT`] grid via [`PossibilitySignature`];
//!   strengths through the same unit grid; positions to integer world units),
//!   so a record means the same thing on every platform. Their ids are
//!   **content-derived** from the immutable integer fields (ADR 0014): the same
//!   discovery yields the same id in every store, which is what makes merge
//!   union-by-id conflict-free by construction.
//! - **The session tier** ([`SessionSnapshot`]) is run-local: it carries
//!   bit-exact floats (the wire format round-trips IEEE bit patterns exactly)
//!   so save→load is *state-hash exact*, and it is never shared or merged.
//!
//! Every persisted value is wrapped in an [`Envelope`] carrying
//! [`RECORD_FORMAT_VERSION`] — a version axis independent of
//! [`WORLD_ALGORITHM_VERSION`]; the format can evolve without touching world
//! identity and vice versa. Readers refuse records from a *newer* format and
//! migrate older ones forward (none exist yet at v1). The wire format is
//! `postcard` (a stable, specified, `no_std` encoding); the exact bytes, the
//! serde field/variant orders, and every content-id fold order are part of the
//! deterministic format contract and are golden-fixtured in
//! `tests/determinism.rs` — changing any of them is a format change and MUST
//! bump [`RECORD_FORMAT_VERSION`] with a migration (phase-5-plan.md §9.4).

use std::any::{Any, TypeId};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::anchor::{Anchor, AnchorKind, AnchorSource};
use crate::coord::RegionCoord;
use crate::hash::mix;
use crate::possibility::{PossibilityVector, POSSIBILITY_DIMS, POSSIBILITY_QUANT};
use crate::WORLD_ALGORITHM_VERSION;

/// Version of the on-disk/wire record encoding. Bump on any schema change and
/// add a migration in [`decode_record`] (phase-5-plan.md §9.4).
pub const RECORD_FORMAT_VERSION: u16 = 2;

/// Fixed basis separating possibility-signature seeds from every other hash
/// domain.
const SIGNATURE_BASIS: u64 = 0x5165_A7C2_90D3_4E8B;
/// Fixed bases separating each record kind's content-id fold (ADR 0014).
const DISCOVERY_ID_BASIS: u64 = 0xD15C_08E2_4F17_A6C3;
const ROUTE_ID_BASIS: u64 = 0x2073_E4D1_8B5F_60C9;
const ROUTE_V2_NODE_TAG: u64 = 0xA12A_0002_7A67_3E11;
const PRESERVE_ID_BASIS: u64 = 0x94E5_1D3B_C82A_7F04;
/// Fixed basis for the mutable-field merge tiebreak fold (§7.6).
const MUTABLE_RANK_BASIS: u64 = 0x3A9C_60B7_E512_D8F4;

/// A checked merge refused to combine two records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordMergeError {
    /// The records do not address the same candidate id.
    IdMismatch {
        /// Existing/local id.
        left: u64,
        /// Incoming/remote id.
        right: u64,
    },
    /// The ids match, but the immutable bodies do not.
    ImmutableConflict {
        /// The colliding id.
        id: u64,
    },
}

impl core::fmt::Display for RecordMergeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IdMismatch { left, right } => {
                write!(f, "record id mismatch: {left:#018x} != {right:#018x}")
            }
            Self::ImmutableConflict { id } => {
                write!(f, "same-id immutable content conflict for {id:#018x}")
            }
        }
    }
}

impl core::error::Error for RecordMergeError {}

/// Preserve region set canonicalization failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreserveRegionError {
    /// One coordinate appears more than once with the same signature.
    DuplicateRegion {
        /// The duplicate coordinate.
        coord: RegionCoord,
    },
    /// Entries are not in canonical coordinate order.
    NonCanonicalOrder,
    /// One coordinate was supplied with more than one signature.
    ConflictingDuplicateRegion {
        /// The duplicate coordinate.
        coord: RegionCoord,
    },
}

impl core::fmt::Display for PreserveRegionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateRegion { coord } => {
                write!(f, "preserve region {:?} appears more than once", coord)
            }
            Self::NonCanonicalOrder => {
                f.write_str("preserve regions are not in canonical coordinate order")
            }
            Self::ConflictingDuplicateRegion { coord } => write!(
                f,
                "preserve region {:?} appears with conflicting signatures",
                coord
            ),
        }
    }
}

impl core::error::Error for PreserveRegionError {}

/// A shareable record body is not in canonical set form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordCanonicalError {
    /// Stored id does not equal the recomputed content id.
    ContentIdMismatch {
        /// The record kind.
        kind: RecordKind,
        /// Stored id.
        id: u64,
    },
    /// Route discovery refs are not sorted and unique.
    RouteDiscoveryRefs {
        /// Route id.
        id: u64,
    },
    /// Preserve regions are not sorted and unique.
    PreserveRegions {
        /// Preserve id.
        id: u64,
        /// Detail.
        source: PreserveRegionError,
    },
}

impl core::fmt::Display for RecordCanonicalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ContentIdMismatch { kind, id } => {
                write!(f, "{kind:?} {id:#018x}: content id mismatch")
            }
            Self::RouteDiscoveryRefs { id } => {
                write!(f, "route {id:#018x}: discovery refs are not sorted unique")
            }
            Self::PreserveRegions { id, source } => {
                write!(f, "preserve {id:#018x}: {source}")
            }
        }
    }
}

impl core::error::Error for RecordCanonicalError {}

/// Bundle set canonicalization failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleCanonicalError {
    /// A record body is malformed or non-canonical.
    Record(RecordCanonicalError),
    /// Duplicate ids could not be collapsed because immutable bodies differ.
    Merge {
        /// The record kind.
        kind: RecordKind,
        /// Merge error.
        source: RecordMergeError,
    },
}

impl core::fmt::Display for BundleCanonicalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Record(e) => e.fmt(f),
            Self::Merge { kind, source } => write!(f, "{kind:?}: {source}"),
        }
    }
}

impl core::error::Error for BundleCanonicalError {}

impl From<RecordCanonicalError> for BundleCanonicalError {
    fn from(value: RecordCanonicalError) -> Self {
        Self::Record(value)
    }
}

/// SHA-256 digest of canonical public atlas content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AtlasDigest(pub [u8; 32]);

impl AtlasDigest {
    /// Lowercase hexadecimal encoding.
    #[must_use]
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            use core::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    }
}

/// What kind of value an [`Envelope`] wraps. The discriminant order is part of
/// the record format contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordKind {
    /// Store-level metadata (the vault's `meta/store` header).
    Meta,
    /// The run-local [`SessionSnapshot`].
    Session,
    /// A [`DiscoveryRecord`].
    Discovery,
    /// A [`RouteRecord`].
    Route,
    /// A [`PreserveRecord`].
    Preserve,
    /// A [`SeenRecord`] (discovered-region chunk).
    Seen,
    /// An [`AtlasBundle`] (export/import container).
    Bundle,
}

/// Every persisted value is prefixed by this envelope. `world_version` records
/// which algorithm generation the value was created under; a mismatch does not
/// block decoding (records are quantized intents, not generated output) but is
/// surfaced by callers because the same bucket realizes a different world under
/// a different algorithm version — an honest label, not a lie of equivalence
/// (phase-5-plan.md §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// [`RECORD_FORMAT_VERSION`] at write time.
    pub format_version: u16,
    /// [`WORLD_ALGORITHM_VERSION`] at write time.
    pub world_version: u32,
    /// What the body is.
    pub kind: RecordKind,
}

/// Errors the record codec may return. Decoding never panics and never guesses:
/// a future format or a mangled body is refused, not repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The record was written by a newer format than this reader understands.
    UnsupportedFormat(u16),
    /// The envelope wraps a different kind than the caller asked for.
    WrongKind {
        /// The kind the caller expected.
        expected: RecordKind,
        /// The kind the envelope carried.
        found: RecordKind,
    },
    /// The bytes do not decode as a well-formed envelope + body.
    Corrupt,
}

impl core::fmt::Display for RecordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RecordError::UnsupportedFormat(v) => {
                write!(
                    f,
                    "record format v{v} is newer than v{RECORD_FORMAT_VERSION}"
                )
            }
            RecordError::WrongKind { expected, found } => {
                write!(f, "expected a {expected:?} record, found {found:?}")
            }
            RecordError::Corrupt => write!(f, "corrupt record bytes"),
        }
    }
}

impl core::error::Error for RecordError {}

/// Encode a record body under a versioned [`Envelope`]: `envelope || body`,
/// both `postcard`. Infallible for the in-memory record types (they contain no
/// unserializable values).
#[must_use]
pub fn encode_record<T: Serialize>(kind: RecordKind, body: &T) -> Vec<u8> {
    let envelope = Envelope {
        format_version: RECORD_FORMAT_VERSION,
        world_version: WORLD_ALGORITHM_VERSION,
        kind,
    };
    let mut out = postcard::to_allocvec(&envelope).expect("envelope encoding cannot fail");
    let body = postcard::to_allocvec(body).expect("record encoding cannot fail");
    out.extend_from_slice(&body);
    out
}

/// Decode the envelope alone — for listing/validating a store without knowing
/// each record's type up front.
pub fn peek_envelope(bytes: &[u8]) -> Result<Envelope, RecordError> {
    let (envelope, _rest): (Envelope, &[u8]) =
        postcard::take_from_bytes(bytes).map_err(|_| RecordError::Corrupt)?;
    if envelope.format_version > RECORD_FORMAT_VERSION {
        return Err(RecordError::UnsupportedFormat(envelope.format_version));
    }
    Ok(envelope)
}

/// Decode `envelope || body`, checking the format version and expected kind.
/// A body with trailing garbage is corrupt, not silently accepted. Older
/// format versions migrate forward here (none exist yet at v1); newer ones are
/// refused (§7.2).
pub fn decode_record<T: DeserializeOwned + 'static>(
    bytes: &[u8],
    expected: RecordKind,
) -> Result<(Envelope, T), RecordError> {
    let (envelope, rest): (Envelope, &[u8]) =
        postcard::take_from_bytes(bytes).map_err(|_| RecordError::Corrupt)?;
    if envelope.format_version > RECORD_FORMAT_VERSION {
        return Err(RecordError::UnsupportedFormat(envelope.format_version));
    }
    if envelope.kind != expected {
        return Err(RecordError::WrongKind {
            expected,
            found: envelope.kind,
        });
    }
    if envelope.format_version == 1 {
        let body = decode_v1_body::<T>(rest, expected)?;
        return Ok((envelope, body));
    }
    let (body, trailing): (T, &[u8]) =
        postcard::take_from_bytes(rest).map_err(|_| RecordError::Corrupt)?;
    if !trailing.is_empty() {
        return Err(RecordError::Corrupt);
    }
    Ok((envelope, body))
}

fn decode_v1_body<T: DeserializeOwned + 'static>(
    bytes: &[u8],
    expected: RecordKind,
) -> Result<T, RecordError> {
    match expected {
        RecordKind::Route if TypeId::of::<T>() == TypeId::of::<RouteRecord>() => {
            let (body, trailing): (v1::RouteRecordV1, &[u8]) =
                postcard::take_from_bytes(bytes).map_err(|_| RecordError::Corrupt)?;
            if !trailing.is_empty() {
                return Err(RecordError::Corrupt);
            }
            downcast_migration(body.migrate())
        }
        RecordKind::Session if TypeId::of::<T>() == TypeId::of::<SessionSnapshot>() => {
            let (body, trailing): (v1::SessionSnapshotV1, &[u8]) =
                postcard::take_from_bytes(bytes).map_err(|_| RecordError::Corrupt)?;
            if !trailing.is_empty() {
                return Err(RecordError::Corrupt);
            }
            downcast_migration(body.migrate())
        }
        _ => {
            let (body, trailing): (T, &[u8]) =
                postcard::take_from_bytes(bytes).map_err(|_| RecordError::Corrupt)?;
            if !trailing.is_empty() {
                return Err(RecordError::Corrupt);
            }
            Ok(body)
        }
    }
}

fn downcast_migration<T: 'static, U: Any>(value: U) -> Result<T, RecordError> {
    let boxed: Box<dyn Any> = Box::new(value);
    boxed
        .downcast::<T>()
        .map(|boxed| *boxed)
        .map_err(|_| RecordError::Corrupt)
}

/// Quantize a `[0, 1]` scalar onto the shared record grid (the possibility
/// bucket grid, so one epsilon rules every quantized unit field).
#[inline]
#[must_use]
pub fn quantize_unit(value: f32) -> u16 {
    let v = value.clamp(0.0, 1.0);
    ((v * f32::from(POSSIBILITY_QUANT)) as u16).min(POSSIBILITY_QUANT - 1)
}

/// The exact `f32` a reader reconstructs for a unit bucket (its center), so
/// `quantize_unit(dequantize_unit(q)) == q` for every bucket.
#[inline]
#[must_use]
pub fn dequantize_unit(bucket: u16) -> f32 {
    (f32::from(bucket) + 0.5) / f32::from(POSSIBILITY_QUANT)
}

/// A possibility vector quantized for persistence and sharing: one bucket per
/// domain (ADR 0013). Dequantizing yields bucket centers — identical on every
/// platform — so any math downstream of a signature (`steer`, projection,
/// dependency hashes) is cross-platform by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PossibilitySignature {
    /// One bucket per [`crate::PossibilityDomain`], in stable domain order.
    pub buckets: [u16; POSSIBILITY_DIMS],
}

impl PossibilitySignature {
    /// Quantize a live vector onto the record grid (the write boundary).
    #[must_use]
    pub fn of(v: PossibilityVector) -> Self {
        let mut buckets = [0u16; POSSIBILITY_DIMS];
        for (bucket, dim) in buckets.iter_mut().zip(v.dims) {
            *bucket = quantize_unit(dim);
        }
        Self { buckets }
    }

    /// The exact vector a reader reconstructs: every domain at its bucket
    /// center (the read boundary). `Self::of(sig.dequantize()) == sig` for
    /// every signature.
    #[must_use]
    pub fn dequantize(self) -> PossibilityVector {
        let mut v = PossibilityVector::neutral();
        for (dim, bucket) in v.dims.iter_mut().zip(self.buckets) {
            *dim = PossibilityVector::dequantize(bucket);
        }
        v
    }

    /// The 64-bit seed of this point in possibility space — the key the route
    /// graph indexes possibility space by (phase-5-plan.md §7.4), the
    /// possibility-space analogue of `HabitatSignature::seed`. Portable: a pure
    /// integer fold under a fixed basis.
    #[inline]
    #[must_use]
    pub const fn seed(&self) -> u64 {
        let mut h = SIGNATURE_BASIS;
        h = mix(h, WORLD_ALGORITHM_VERSION as u64);
        let mut i = 0;
        while i < POSSIBILITY_DIMS {
            h = mix(h, self.buckets[i] as u64);
            i += 1;
        }
        h
    }
}

/// Fold an [`AnchorSource`] into a content id (tag, then payload).
const fn fold_source(h: u64, source: AnchorSource) -> u64 {
    match source {
        AnchorSource::Organism { species } => mix(mix(h, 0), species),
        AnchorSource::Landform => mix(h, 1),
        AnchorSource::River => mix(h, 2),
        AnchorSource::Atmosphere => mix(h, 3),
        AnchorSource::Manual => mix(h, 4),
    }
}

/// Fold an [`AnchorKind`] into a content id.
const fn fold_kind(h: u64, kind: AnchorKind) -> u64 {
    match kind {
        AnchorKind::Emphasize => mix(h, 0),
        AnchorKind::Suppress => mix(h, 1),
    }
}

/// Fold a string's bytes into a hash (merge tiebreaks only — never identity).
fn fold_str(mut h: u64, s: &str) -> u64 {
    h = mix(h, s.len() as u64);
    for b in s.bytes() {
        h = mix(h, u64::from(b));
    }
    h
}

/// The deterministic, commutative rank by which two stores' *mutable*
/// presentation fields (name, journal) resolve on merge: higher `sequence`
/// wins, with a content-hash tiebreak so the pick is total and symmetric even
/// across stores whose sequences collide (phase-5-plan.md §7.6).
#[must_use]
fn mutable_rank(sequence: u64, name: &str, journal: &str) -> (u64, u64) {
    let mut h = MUTABLE_RANK_BASIS;
    h = fold_str(h, name);
    h = fold_str(h, journal);
    (sequence, h)
}

/// A named, shareable discovery — the persistent form of a Phase 4 capture
/// (phase-5-plan.md §4.3). Identity fields are integers (portable); `name`,
/// `journal`, and `sequence` are mutable presentation metadata excluded from
/// [`Self::content_id`], so renaming never changes identity and the same
/// capture collides to the same id in every store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryRecord {
    /// Content-derived id: [`Self::content_id`] of the immutable fields below.
    pub id: u64,
    /// What was captured (an organism's stable species id, a landform, …).
    pub source: AnchorSource,
    /// The habitat signature seed at the capture cell (`HabitatSignature::seed`),
    /// or 0 if the habitat had not settled — the portable ADR 0010 identity core.
    pub signature_seed: u64,
    /// The captured trait target, quantized.
    pub target: PossibilitySignature,
    /// Bitmask of affected possibility domains (bit = `domain.index()`).
    pub mask: u8,
    /// Emphasize (pull toward `target`) or Suppress (push away).
    pub kind: AnchorKind,
    /// Anchor strength quantized onto the unit grid.
    pub strength_q: u16,
    /// Falloff radius rounded to integer world units.
    pub falloff_q: u32,
    /// Capture position rounded to integer world units.
    pub pos_q: (i64, i64),
    /// Store-local monotonic sequence; merge tiebreak for the mutable fields.
    pub sequence: u64,
    /// Player-given name (mutable; excluded from the id).
    pub name: String,
    /// Journal text (mutable; excluded from the id).
    pub journal: String,
}

impl DiscoveryRecord {
    /// Quantize a live anchor into a record (the write boundary, ADR 0013).
    /// `signature_seed` is the capture cell's habitat identity (0 if unknown).
    #[must_use]
    pub fn from_anchor(a: &Anchor, signature_seed: u64, sequence: u64, name: String) -> Self {
        let mut record = Self {
            id: 0,
            source: a.source,
            signature_seed,
            target: PossibilitySignature::of(a.target),
            mask: a.mask,
            kind: a.kind,
            strength_q: quantize_unit(a.strength),
            falloff_q: a.falloff_radius.round().clamp(0.0, f64::from(u32::MAX)) as u32,
            pos_q: (round_world(a.world_pos.0), round_world(a.world_pos.1)),
            sequence,
            name,
            journal: String::new(),
        };
        record.id = record.content_id();
        record
    }

    /// Reconstruct a steering anchor (the read boundary). Pure and portable:
    /// dequantized integers in, so the anchor — and everything `steer` and
    /// `project_plausible` derive from it — is identical on every platform.
    #[must_use]
    pub fn to_anchor(&self) -> Anchor {
        Anchor {
            world_pos: (self.pos_q.0 as f64, self.pos_q.1 as f64),
            target: self.target.dequantize(),
            mask: self.mask,
            kind: self.kind,
            strength: dequantize_unit(self.strength_q),
            falloff_radius: f64::from(self.falloff_q),
            source: self.source,
        }
    }

    /// The content id: a fold of the immutable integer fields in a fixed,
    /// golden-fixtured order (ADR 0014). Excludes `name`, `journal`, and
    /// `sequence`.
    #[must_use]
    pub fn content_id(&self) -> u64 {
        let mut h = DISCOVERY_ID_BASIS;
        h = fold_source(h, self.source);
        h = mix(h, self.signature_seed);
        for b in self.target.buckets {
            h = mix(h, u64::from(b));
        }
        h = mix(h, u64::from(self.mask));
        h = fold_kind(h, self.kind);
        h = mix(h, u64::from(self.strength_q));
        h = mix(h, u64::from(self.falloff_q));
        h = mix(h, self.pos_q.0 as u64);
        h = mix(h, self.pos_q.1 as u64);
        h
    }

    /// Whether the immutable body fields that define [`Self::content_id`] are
    /// equal. Mutable presentation fields are deliberately excluded.
    #[must_use]
    pub fn immutable_eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.signature_seed == other.signature_seed
            && self.target == other.target
            && self.mask == other.mask
            && self.kind == other.kind
            && self.strength_q == other.strength_q
            && self.falloff_q == other.falloff_q
            && self.pos_q == other.pos_q
    }

    /// Validate that the stored id is honest.
    pub fn validate_canonical(&self) -> Result<(), RecordCanonicalError> {
        if self.id != self.content_id() {
            return Err(RecordCanonicalError::ContentIdMismatch {
                kind: RecordKind::Discovery,
                id: self.id,
            });
        }
        Ok(())
    }

    /// Merge another store's copy of the same record (ADR 0014): immutable
    /// fields must compare equal before mutable fields resolve by
    /// [`mutable_rank`]. Returns whether `self` changed. Commutative,
    /// associative, idempotent for equal immutable bodies — machine-checked.
    pub fn merge_from(&mut self, other: &Self) -> Result<bool, RecordMergeError> {
        if self.id != other.id {
            return Err(RecordMergeError::IdMismatch {
                left: self.id,
                right: other.id,
            });
        }
        if !self.immutable_eq(other) {
            return Err(RecordMergeError::ImmutableConflict { id: self.id });
        }
        if mutable_rank(other.sequence, &other.name, &other.journal)
            > mutable_rank(self.sequence, &self.name, &self.journal)
        {
            self.sequence = other.sequence;
            self.name.clone_from(&other.name);
            self.journal.clone_from(&other.journal);
            return Ok(true);
        }
        Ok(false)
    }
}

/// One sample along an expedition — the section 13 node shape, quantized
/// (phase-5-plan.md §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteNode {
    /// Physical position, integer world units.
    pub pos_q: (i64, i64),
    /// The possibility-space target: the covering region's steered target,
    /// quantized. Kept as `signature` for the v1 API surface; new code should
    /// read it as the target signature, not the visible current state.
    pub signature: PossibilitySignature,
    /// The covering region's visible realized current, quantized. `None`
    /// identifies a migrated v1 node whose current-world truth was not stored.
    pub current_signature: Option<PossibilitySignature>,
    /// Transition cost, banded to `[0, 255]` from `1 − resonance` at record
    /// time — route difficulty falls out of the world model, not a knob.
    pub cost_q: u8,
    /// Region stability at record time, banded to `[0, 255]`.
    pub stability_q: u8,
    /// Order-independent signature of the anchor set active at record time.
    pub anchor_sig: u64,
    /// Rounded world distance represented by this sample since the previous
    /// node. The first node and migrated v1 nodes use zero.
    pub distance_q: u32,
}

impl RouteNode {
    /// Whether this node has exactly the v1 identity shape.
    #[must_use]
    pub const fn is_legacy_identity(self) -> bool {
        self.current_signature.is_none() && self.distance_q == 0
    }

    /// Fold this node into a v1 route content id.
    #[must_use]
    const fn fold_v1(self, mut h: u64) -> u64 {
        h = mix(h, self.pos_q.0 as u64);
        h = mix(h, self.pos_q.1 as u64);
        h = mix(h, self.signature.seed());
        h = mix(h, self.cost_q as u64);
        h = mix(h, self.stability_q as u64);
        h = mix(h, self.anchor_sig);
        h
    }

    /// Fold this node into a v2 route content id.
    #[must_use]
    const fn fold_v2(self, mut h: u64) -> u64 {
        h = mix(h, ROUTE_V2_NODE_TAG);
        h = self.fold_v1(h);
        match self.current_signature {
            Some(sig) => {
                h = mix(h, 1);
                h = mix(h, sig.seed());
            }
            None => {
                h = mix(h, 0);
            }
        }
        h = mix(h, self.distance_q as u64);
        h
    }
}

/// A persisted expedition: an ordered node path plus its social metadata
/// (phase-5-plan.md §4.4). The node path and discovery refs are immutable
/// identity; `usage`, `name`, and `journal` are mutable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    /// Content-derived id: [`Self::content_id`] of the node path + discoveries.
    pub id: u64,
    /// The recorded path, in travel order.
    pub nodes: Vec<RouteNode>,
    /// Traversal count (mutable; merges by `max`, so re-importing a bundle
    /// never double-counts).
    pub usage: u32,
    /// [`DiscoveryRecord::id`]s made along the way (the expedition's journal
    /// entries reference these).
    pub discoveries: Vec<u64>,
    /// Store-local monotonic sequence; merge tiebreak for the mutable fields.
    pub sequence: u64,
    /// Player-given name (mutable; excluded from the id).
    pub name: String,
    /// The expedition journal (mutable; excluded from the id).
    pub journal: String,
}

impl RouteRecord {
    /// Close a recorded path into a record.
    #[must_use]
    pub fn new(
        nodes: Vec<RouteNode>,
        mut discoveries: Vec<u64>,
        sequence: u64,
        name: String,
    ) -> Self {
        canonicalize_discovery_refs(&mut discoveries);
        let mut record = Self {
            id: 0,
            nodes,
            usage: 0,
            discoveries,
            sequence,
            name,
            journal: String::new(),
        };
        record.id = record.content_id();
        record
    }

    /// The content id: a fold of the node path and discovery refs in a fixed,
    /// golden-fixtured order. Excludes `usage`, `name`, `journal`, `sequence`.
    #[must_use]
    pub fn content_id(&self) -> u64 {
        let mut h = ROUTE_ID_BASIS;
        h = mix(h, self.nodes.len() as u64);
        let legacy_identity = self.nodes.iter().all(|node| node.is_legacy_identity());
        for node in &self.nodes {
            h = if legacy_identity {
                node.fold_v1(h)
            } else {
                node.fold_v2(h)
            };
        }
        h = mix(h, self.discoveries.len() as u64);
        for &d in &self.discoveries {
            h = mix(h, d);
        }
        h
    }

    /// Whether the immutable body fields that define [`Self::content_id`] are
    /// equal.
    #[must_use]
    pub fn immutable_eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes && self.discoveries == other.discoveries
    }

    /// Validate that the body is canonical and its stored id is honest.
    pub fn validate_canonical(&self) -> Result<(), RecordCanonicalError> {
        if self.id != self.content_id() {
            return Err(RecordCanonicalError::ContentIdMismatch {
                kind: RecordKind::Route,
                id: self.id,
            });
        }
        if !discovery_refs_are_canonical(&self.discoveries) {
            return Err(RecordCanonicalError::RouteDiscoveryRefs { id: self.id });
        }
        Ok(())
    }

    /// Merge another store's copy: `usage` by `max` (idempotent), the mutable
    /// presentation fields by [`mutable_rank`]. Returns whether `self` changed.
    pub fn merge_from(&mut self, other: &Self) -> Result<bool, RecordMergeError> {
        if self.id != other.id {
            return Err(RecordMergeError::IdMismatch {
                left: self.id,
                right: other.id,
            });
        }
        if !self.immutable_eq(other) {
            return Err(RecordMergeError::ImmutableConflict { id: self.id });
        }
        let mut changed = false;
        if other.usage > self.usage {
            self.usage = other.usage;
            changed = true;
        }
        if mutable_rank(other.sequence, &other.name, &other.journal)
            > mutable_rank(self.sequence, &self.name, &self.journal)
        {
            self.sequence = other.sequence;
            self.name.clone_from(&other.name);
            self.journal.clone_from(&other.journal);
            changed = true;
        }
        Ok(changed)
    }
}

/// A preserved window: pinned regions restored from quantized buckets alone
/// (phase-5-plan.md §4.5, §7.5). No tiles, no organisms — deterministic
/// generation reproduces the landscape from `regions` (ADR 0008), which is the
/// success criterion in one struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreserveRecord {
    /// Content-derived id: [`Self::content_id`] of the region set.
    pub id: u64,
    /// Each preserved region's coordinate and its quantized possibility state,
    /// in deterministic coordinate order.
    pub regions: Vec<(RegionCoord, PossibilitySignature)>,
    /// Store-local monotonic sequence; merge tiebreak for the mutable fields.
    pub sequence: u64,
    /// Player-given name (mutable; excluded from the id).
    pub name: String,
    /// Journal text (mutable; excluded from the id).
    pub journal: String,
}

impl PreserveRecord {
    /// Snapshot a set of regions' possibility states into a preserve. The
    /// entries are sorted into deterministic coordinate order so the content id
    /// is a function of the *set*.
    pub fn try_new(
        mut regions: Vec<(RegionCoord, PossibilitySignature)>,
        sequence: u64,
        name: String,
    ) -> Result<Self, PreserveRegionError> {
        canonicalize_preserve_regions(&mut regions)?;
        let mut record = Self {
            id: 0,
            regions,
            sequence,
            name,
            journal: String::new(),
        };
        record.id = record.content_id();
        Ok(record)
    }

    /// Snapshot a trusted set of regions. Panics only if a caller supplies the
    /// same coordinate with conflicting signatures.
    #[must_use]
    pub fn new(
        regions: Vec<(RegionCoord, PossibilitySignature)>,
        sequence: u64,
        name: String,
    ) -> Self {
        Self::try_new(regions, sequence, name)
            .expect("preserve regions must be a coordinate-keyed set")
    }

    /// The content id: a fold of the sorted region set in a fixed,
    /// golden-fixtured order. Excludes `name`, `journal`, `sequence`.
    #[must_use]
    pub fn content_id(&self) -> u64 {
        let mut h = PRESERVE_ID_BASIS;
        h = mix(h, self.regions.len() as u64);
        for (coord, sig) in &self.regions {
            h = mix(h, coord.x as u32 as u64);
            h = mix(h, coord.y as u32 as u64);
            h = mix(h, u64::from(coord.level));
            h = mix(h, sig.seed());
        }
        h
    }

    /// Whether the immutable body fields that define [`Self::content_id`] are
    /// equal.
    #[must_use]
    pub fn immutable_eq(&self, other: &Self) -> bool {
        self.regions == other.regions
    }

    /// Validate that the body is canonical and its stored id is honest.
    pub fn validate_canonical(&self) -> Result<(), RecordCanonicalError> {
        if self.id != self.content_id() {
            return Err(RecordCanonicalError::ContentIdMismatch {
                kind: RecordKind::Preserve,
                id: self.id,
            });
        }
        let mut canonical = self.regions.clone();
        canonicalize_preserve_regions(&mut canonical).map_err(|source| {
            RecordCanonicalError::PreserveRegions {
                id: self.id,
                source,
            }
        })?;
        if canonical != self.regions {
            let Some(coord) = self
                .regions
                .windows(2)
                .find(|pair| pair[0].0 == pair[1].0)
                .map(|pair| pair[0].0)
            else {
                return Err(RecordCanonicalError::PreserveRegions {
                    id: self.id,
                    source: PreserveRegionError::NonCanonicalOrder,
                });
            };
            return Err(RecordCanonicalError::PreserveRegions {
                id: self.id,
                source: PreserveRegionError::DuplicateRegion { coord },
            });
        }
        Ok(())
    }

    /// Merge another store's copy: mutable presentation fields by
    /// [`mutable_rank`]. Returns whether `self` changed.
    pub fn merge_from(&mut self, other: &Self) -> Result<bool, RecordMergeError> {
        if self.id != other.id {
            return Err(RecordMergeError::IdMismatch {
                left: self.id,
                right: other.id,
            });
        }
        if !self.immutable_eq(other) {
            return Err(RecordMergeError::ImmutableConflict { id: self.id });
        }
        if mutable_rank(other.sequence, &other.name, &other.journal)
            > mutable_rank(self.sequence, &self.name, &self.journal)
        {
            self.sequence = other.sequence;
            self.name.clone_from(&other.name);
            self.journal.clone_from(&other.journal);
            return Ok(true);
        }
        Ok(false)
    }
}

fn canonicalize_discovery_refs(discoveries: &mut Vec<u64>) {
    discoveries.sort_unstable();
    discoveries.dedup();
}

fn discovery_refs_are_canonical(discoveries: &[u64]) -> bool {
    discoveries.windows(2).all(|pair| pair[0] < pair[1])
}

fn canonicalize_preserve_regions(
    regions: &mut Vec<(RegionCoord, PossibilitySignature)>,
) -> Result<(), PreserveRegionError> {
    regions.sort_unstable_by_key(|(coord, _)| *coord);
    let mut write = 0;
    for read in 0..regions.len() {
        if write > 0 && regions[write - 1].0 == regions[read].0 {
            if regions[write - 1].1 != regions[read].1 {
                return Err(PreserveRegionError::ConflictingDuplicateRegion {
                    coord: regions[read].0,
                });
            }
            continue;
        }
        if write != read {
            regions[write] = regions[read];
        }
        write += 1;
    }
    regions.truncate(write);
    Ok(())
}

/// Store-level metadata — the vault's `meta/store` header. The format and
/// world versions live in the [`Envelope`]; the body carries only the
/// store-local monotonic sequence counter. Readers heal a stale counter (a
/// crash between record and meta writes) by taking the max of this and every
/// loaded record's sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StoreMeta {
    /// The last sequence number handed out.
    pub sequence: u64,
}

/// Hierarchy level of a discovered-region chunk: one [`SeenRecord`] covers a
/// `2^SEEN_CHUNK_LEVEL × 2^SEEN_CHUNK_LEVEL` block of level-0 regions.
pub const SEEN_CHUNK_LEVEL: u16 = 4;
/// Level-0 regions per chunk edge.
pub const SEEN_CHUNK_EDGE: i32 = 1 << SEEN_CHUNK_LEVEL;
/// `u64` words in a chunk's bitmap (`edge² / 64`).
pub const SEEN_CHUNK_WORDS: usize = ((SEEN_CHUNK_EDGE * SEEN_CHUNK_EDGE) / 64) as usize;

/// The discovered-region set for one chunk of the world — a fixed bitmap, a
/// few bytes per region ever visited, partial-loading cleanly (only chunks near
/// the player load). Merges by union (phase-5-plan.md §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenRecord {
    /// The chunk coordinate (level = [`SEEN_CHUNK_LEVEL`]).
    pub chunk: RegionCoord,
    /// One bit per level-0 region, row-major within the chunk.
    pub bits: [u64; SEEN_CHUNK_WORDS],
}

impl SeenRecord {
    /// The chunk covering a level-0 region (arithmetic shift floors toward
    /// negative infinity, matching [`RegionCoord::parent`]).
    #[inline]
    #[must_use]
    pub const fn chunk_of(region: RegionCoord) -> RegionCoord {
        RegionCoord::at_level(
            region.x >> SEEN_CHUNK_LEVEL,
            region.y >> SEEN_CHUNK_LEVEL,
            SEEN_CHUNK_LEVEL,
        )
    }

    /// An empty chunk record.
    #[inline]
    #[must_use]
    pub const fn empty(chunk: RegionCoord) -> Self {
        Self {
            chunk,
            bits: [0; SEEN_CHUNK_WORDS],
        }
    }

    /// The bit index of a level-0 region within its chunk, or `None` if the
    /// region belongs to a different chunk.
    #[inline]
    #[must_use]
    fn bit_index(&self, region: RegionCoord) -> Option<usize> {
        if Self::chunk_of(region) != self.chunk || region.level != 0 {
            return None;
        }
        let lx = region.x - (self.chunk.x << SEEN_CHUNK_LEVEL);
        let ly = region.y - (self.chunk.y << SEEN_CHUNK_LEVEL);
        Some((ly * SEEN_CHUNK_EDGE + lx) as usize)
    }

    /// Mark a level-0 region discovered. Returns whether the bit was new.
    pub fn mark(&mut self, region: RegionCoord) -> bool {
        let Some(i) = self.bit_index(region) else {
            return false;
        };
        let word = &mut self.bits[i / 64];
        let bit = 1u64 << (i % 64);
        let new = *word & bit == 0;
        *word |= bit;
        new
    }

    /// Whether a level-0 region is marked discovered.
    #[must_use]
    pub fn contains(&self, region: RegionCoord) -> bool {
        self.bit_index(region)
            .is_some_and(|i| self.bits[i / 64] & (1 << (i % 64)) != 0)
    }

    /// Number of discovered regions in this chunk.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }

    /// Merge another store's copy of the same chunk: bitwise union. Returns
    /// whether `self` changed.
    pub fn merge_from(&mut self, other: &Self) -> bool {
        debug_assert_eq!(self.chunk, other.chunk, "merge_from requires one chunk");
        let mut changed = false;
        for (a, b) in self.bits.iter_mut().zip(other.bits) {
            if *a | b != *a {
                *a |= b;
                changed = true;
            }
        }
        changed
    }
}

/// A bit-exact snapshot of one live anchor — the session tier (ADR 0013): raw
/// floats, run-local, never shared or merged. The wire format round-trips IEEE
/// bit patterns exactly, so `to_anchor(from_anchor(a)) == a` bit-for-bit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchorSnapshot {
    /// [`Anchor::world_pos`], bit-exact.
    pub world_pos: (f64, f64),
    /// [`Anchor::target`] dims, bit-exact.
    pub target: [f32; POSSIBILITY_DIMS],
    /// [`Anchor::mask`].
    pub mask: u8,
    /// [`Anchor::kind`].
    pub kind: AnchorKind,
    /// [`Anchor::strength`], bit-exact.
    pub strength: f32,
    /// [`Anchor::falloff_radius`], bit-exact.
    pub falloff_radius: f64,
    /// [`Anchor::source`].
    pub source: AnchorSource,
}

impl AnchorSnapshot {
    /// Snapshot a live anchor exactly.
    #[must_use]
    pub fn from_anchor(a: &Anchor) -> Self {
        Self {
            world_pos: a.world_pos,
            target: a.target.dims,
            mask: a.mask,
            kind: a.kind,
            strength: a.strength,
            falloff_radius: a.falloff_radius,
            source: a.source,
        }
    }

    /// Restore the live anchor exactly.
    #[must_use]
    pub fn to_anchor(&self) -> Anchor {
        Anchor {
            world_pos: self.world_pos,
            target: PossibilityVector { dims: self.target },
            mask: self.mask,
            kind: self.kind,
            strength: self.strength,
            falloff_radius: self.falloff_radius,
            source: self.source,
        }
    }
}

/// A bit-exact snapshot of one resident region's authoritative runtime state —
/// the session tier. Caches, rosters, and organisms are deliberately absent:
/// they re-derive deterministically from `current` (ADR 0008), so persisting
/// them would store geometry (§1.2's sparsity bound).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegionSnapshotRecord {
    /// The region.
    pub coord: RegionCoord,
    /// Realized possibility state, bit-exact.
    pub current: [f32; POSSIBILITY_DIMS],
    /// Steered target possibility state, bit-exact.
    pub target: [f32; POSSIBILITY_DIMS],
    /// Distance-ramp stability at save time, bit-exact.
    pub stability: f32,
    /// The region's monotonic revision counter.
    pub revision: u32,
}

/// Effective streaming configuration at session-save time. Primitive widths
/// are fixed for the record format; runtime converts to platform `usize`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StreamConfigRecord {
    pub near_radius: f64,
    pub far_radius: f64,
    pub load_radius: f64,
    pub unload_radius: f64,
    pub converge_per_unit: f32,
    pub converge_rate_cap: f32,
    pub field_resolution: u16,
    pub max_field_cache_bytes: u64,
    pub max_macro_cache_bytes: u64,
    pub max_roster_cache_bytes: u64,
    pub organisms_per_cell: u16,
}

/// Effective frame budget at session-save time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetRecord {
    pub max_loads: u64,
    pub max_converge_regions: u64,
    pub max_regen_cost: u32,
    pub max_realize_organisms: u64,
    pub max_persist_ops: u64,
    pub max_route_attraction_nodes: u64,
    pub max_retarget_regions: u64,
}

/// Resource tier label, if the saving platform knew one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionTierRecord {
    Unknown,
    Low,
    Mid,
    High,
}

/// Policy note for old sessions that did not encode region targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegacyTargetPolicy {
    ExactTargetStored,
    TargetEqualsCurrent,
}

/// Run-local metadata needed to decide whether exact continuation is being
/// attempted under the same runtime contract.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SessionRuntimeRecord {
    pub stream: StreamConfigRecord,
    pub budget: BudgetRecord,
    pub tier: SessionTierRecord,
    pub path_tracking: bool,
    pub route_attraction: bool,
    pub legacy_target_policy: LegacyTargetPolicy,
}

impl Default for SessionRuntimeRecord {
    fn default() -> Self {
        Self {
            stream: StreamConfigRecord {
                near_radius: 0.0,
                far_radius: 0.0,
                load_radius: 0.0,
                unload_radius: 0.0,
                converge_per_unit: 0.0,
                converge_rate_cap: 0.0,
                field_resolution: 0,
                max_field_cache_bytes: 0,
                max_macro_cache_bytes: 0,
                max_roster_cache_bytes: 0,
                organisms_per_cell: 0,
            },
            budget: BudgetRecord {
                max_loads: 0,
                max_converge_regions: 0,
                max_regen_cost: 0,
                max_realize_organisms: 0,
                max_persist_ops: 0,
                max_route_attraction_nodes: 0,
                max_retarget_regions: 0,
            },
            tier: SessionTierRecord::Unknown,
            path_tracking: false,
            route_attraction: false,
            legacy_target_policy: LegacyTargetPolicy::TargetEqualsCurrent,
        }
    }
}

/// Active route-recorder state in the run-local session tier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteRecorderSnapshot {
    pub accumulated: f64,
    pub last_observed: Option<(f64, f64)>,
    pub nodes: Vec<RouteNode>,
    pub discoveries: Vec<u64>,
}

/// One active route-tracker leg.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteTrackerLegSnapshot {
    pub route_id: u64,
    pub visited_nodes: Vec<u32>,
}

/// Route-tracker state in the run-local session tier.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RouteTrackerSnapshot {
    pub legs: Vec<RouteTrackerLegSnapshot>,
}

/// The run-local session tier (phase-5-plan.md §4.5, §6.3): everything needed
/// to make save→load *state-hash exact* on the platform that wrote it. Never
/// shared, never merged, excluded from [`AtlasBundle`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Runtime contract metadata for exactness checks.
    pub runtime: SessionRuntimeRecord,
    /// Player position, bit-exact.
    pub player: (f64, f64),
    /// Previous-frame player position (travel derivation), bit-exact.
    pub last_player: (f64, f64),
    /// The player's direct possibility bias, bit-exact.
    pub bias: [f32; POSSIBILITY_DIMS],
    /// Whether transition mode was active.
    pub transition_mode: bool,
    /// Every live anchor, bit-exact.
    pub anchors: Vec<AnchorSnapshot>,
    /// Every resident region's authoritative state, in deterministic
    /// coordinate order.
    pub regions: Vec<RegionSnapshotRecord>,
    /// Active route recorder, if one was running at save time.
    pub recorder: Option<RouteRecorderSnapshot>,
    /// Active route-tracker leg state.
    pub tracker: RouteTrackerSnapshot,
    /// The store sequence at snapshot time.
    pub sequence: u64,
}

/// The export/import container for the shareable tier (phase-5-plan.md §4.5,
/// Overview "Community Atlas"): discoveries, routes, and preserves — never the
/// session tier. Records inside a bundle are sorted by id, so a bundle is a
/// canonical function of its record *set*.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AtlasBundle {
    /// Shared discoveries.
    pub discoveries: Vec<DiscoveryRecord>,
    /// Shared expeditions.
    pub routes: Vec<RouteRecord>,
    /// Shared preserves.
    pub preserves: Vec<PreserveRecord>,
}

impl AtlasBundle {
    /// Sort every record list by id, collapsing equal-id records only after
    /// checked immutable equality.
    pub fn canonicalize_checked(&mut self) -> Result<(), BundleCanonicalError> {
        canonicalize_records(
            &mut self.discoveries,
            RecordKind::Discovery,
            DiscoveryRecord::validate_canonical,
            DiscoveryRecord::merge_from,
        )?;
        canonicalize_records(
            &mut self.routes,
            RecordKind::Route,
            RouteRecord::validate_canonical,
            RouteRecord::merge_from,
        )?;
        canonicalize_records(
            &mut self.preserves,
            RecordKind::Preserve,
            PreserveRecord::validate_canonical,
            PreserveRecord::merge_from,
        )?;
        Ok(())
    }

    /// Return a checked canonical copy.
    pub fn canonicalized(mut self) -> Result<Self, BundleCanonicalError> {
        self.canonicalize_checked()?;
        Ok(self)
    }

    /// Sort every record list by id (canonical form). Panics if duplicate ids
    /// cannot be lawfully collapsed; use [`Self::canonicalize_checked`] for
    /// untrusted bundles.
    pub fn canonicalize(&mut self) {
        self.canonicalize_checked()
            .expect("atlas bundle must be canonicalizable");
    }

    /// Total records in the bundle.
    #[must_use]
    pub fn len(&self) -> usize {
        self.discoveries.len() + self.routes.len() + self.preserves.len()
    }

    /// Whether the bundle holds no records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether every record's stored id matches its recomputed content id — a
    /// record whose id mismatches is corrupt or tampered and must be rejected,
    /// never partially applied (phase-5-plan.md §7.6).
    #[must_use]
    pub fn ids_valid(&self) -> bool {
        self.discoveries.iter().all(|r| r.id == r.content_id())
            && self.routes.iter().all(|r| r.id == r.content_id())
            && self.preserves.iter().all(|r| r.id == r.content_id())
    }

    /// SHA-256 over the canonical encoded bundle record.
    pub fn digest(&self) -> Result<AtlasDigest, BundleCanonicalError> {
        let canonical = self.clone().canonicalized()?;
        let bytes = encode_record(RecordKind::Bundle, &canonical);
        let digest = Sha256::digest(&bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        Ok(AtlasDigest(out))
    }
}

fn canonicalize_records<T>(
    records: &mut Vec<T>,
    kind: RecordKind,
    validate: fn(&T) -> Result<(), RecordCanonicalError>,
    merge: fn(&mut T, &T) -> Result<bool, RecordMergeError>,
) -> Result<(), BundleCanonicalError>
where
    T: Clone,
    T: HasRecordId,
{
    for record in records.iter() {
        validate(record)?;
    }
    records.sort_unstable_by_key(HasRecordId::record_id);
    let mut out: Vec<T> = Vec::with_capacity(records.len());
    for record in records.drain(..) {
        if let Some(existing) = out.last_mut() {
            if existing.record_id() == record.record_id() {
                merge(existing, &record)
                    .map_err(|source| BundleCanonicalError::Merge { kind, source })?;
                continue;
            }
        }
        out.push(record);
    }
    *records = out;
    Ok(())
}

trait HasRecordId {
    fn record_id(&self) -> u64;
}

impl HasRecordId for DiscoveryRecord {
    fn record_id(&self) -> u64 {
        self.id
    }
}

impl HasRecordId for RouteRecord {
    fn record_id(&self) -> u64 {
        self.id
    }
}

impl HasRecordId for PreserveRecord {
    fn record_id(&self) -> u64 {
        self.id
    }
}

/// Round a continuous world coordinate to integer world units for a shareable
/// record.
#[inline]
fn round_world(v: f64) -> i64 {
    v.round() as i64
}

mod v1 {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub(super) struct RouteNodeV1 {
        pub pos_q: (i64, i64),
        pub signature: PossibilitySignature,
        pub cost_q: u8,
        pub stability_q: u8,
        pub anchor_sig: u64,
    }

    impl RouteNodeV1 {
        pub(super) fn migrate(self) -> RouteNode {
            RouteNode {
                pos_q: self.pos_q,
                signature: self.signature,
                current_signature: None,
                cost_q: self.cost_q,
                stability_q: self.stability_q,
                anchor_sig: self.anchor_sig,
                distance_q: 0,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(super) struct RouteRecordV1 {
        pub id: u64,
        pub nodes: Vec<RouteNodeV1>,
        pub usage: u32,
        pub discoveries: Vec<u64>,
        pub sequence: u64,
        pub name: String,
        pub journal: String,
    }

    impl RouteRecordV1 {
        pub(super) fn migrate(self) -> RouteRecord {
            RouteRecord {
                id: self.id,
                nodes: self.nodes.into_iter().map(RouteNodeV1::migrate).collect(),
                usage: self.usage,
                discoveries: self.discoveries,
                sequence: self.sequence,
                name: self.name,
                journal: self.journal,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub(super) struct RegionSnapshotRecordV1 {
        pub coord: RegionCoord,
        pub current: [f32; POSSIBILITY_DIMS],
        pub stability: f32,
        pub revision: u32,
    }

    impl RegionSnapshotRecordV1 {
        pub(super) fn migrate(self) -> RegionSnapshotRecord {
            RegionSnapshotRecord {
                coord: self.coord,
                current: self.current,
                target: self.current,
                stability: self.stability,
                revision: self.revision,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub(super) struct SessionSnapshotV1 {
        pub player: (f64, f64),
        pub last_player: (f64, f64),
        pub bias: [f32; POSSIBILITY_DIMS],
        pub transition_mode: bool,
        pub anchors: Vec<AnchorSnapshot>,
        pub regions: Vec<RegionSnapshotRecordV1>,
        pub sequence: u64,
    }

    impl SessionSnapshotV1 {
        pub(super) fn migrate(self) -> SessionSnapshot {
            SessionSnapshot {
                runtime: SessionRuntimeRecord::default(),
                player: self.player,
                last_player: self.last_player,
                bias: self.bias,
                transition_mode: self.transition_mode,
                anchors: self.anchors,
                regions: self
                    .regions
                    .into_iter()
                    .map(RegionSnapshotRecordV1::migrate)
                    .collect(),
                recorder: None,
                tracker: RouteTrackerSnapshot::default(),
                sequence: self.sequence,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::bound_target;
    use crate::possibility::PossibilityDomain;

    fn sample_anchor() -> Anchor {
        let mask = 0b1010_0000; // Behavior | Aesthetics... (bits 5..7 region)
        Anchor {
            world_pos: (300.4, -10.6),
            target: bound_target(mask, 0.9),
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.8,
            falloff_radius: 1500.0,
            source: AnchorSource::Organism {
                species: 0x2340_6061_75CD_D2D2,
            },
        }
    }

    fn sample_discovery() -> DiscoveryRecord {
        DiscoveryRecord::from_anchor(
            &sample_anchor(),
            0x4204_1386_32E9_C315,
            7,
            String::from("glowfin"),
        )
    }

    #[test]
    fn discovery_round_trips_through_the_codec() {
        let record = sample_discovery();
        let bytes = encode_record(RecordKind::Discovery, &record);
        let (envelope, decoded): (Envelope, DiscoveryRecord) =
            decode_record(&bytes, RecordKind::Discovery).expect("round trip");
        assert_eq!(envelope.format_version, RECORD_FORMAT_VERSION);
        assert_eq!(envelope.world_version, WORLD_ALGORITHM_VERSION);
        assert_eq!(envelope.kind, RecordKind::Discovery);
        assert_eq!(decoded, record);
    }

    #[test]
    fn every_record_kind_round_trips() {
        let discovery = sample_discovery();
        let node = RouteNode {
            pos_q: (300, -11),
            signature: PossibilitySignature::of(PossibilityVector::neutral()),
            current_signature: Some(PossibilitySignature::of(PossibilityVector::neutral())),
            cost_q: 40,
            stability_q: 255,
            anchor_sig: 0x1234,
            distance_q: 192,
        };
        let route = RouteRecord::new(vec![node, node], vec![discovery.id], 3, "trek".into());
        let preserve = PreserveRecord::new(
            vec![(
                RegionCoord::new(-3, 7),
                PossibilitySignature::of(PossibilityVector::neutral()),
            )],
            4,
            "glade".into(),
        );
        let mut seen = SeenRecord::empty(SeenRecord::chunk_of(RegionCoord::new(-3, 7)));
        seen.mark(RegionCoord::new(-3, 7));
        let session = SessionSnapshot {
            runtime: SessionRuntimeRecord {
                stream: StreamConfigRecord {
                    near_radius: 10.0,
                    far_radius: 20.0,
                    load_radius: 30.0,
                    unload_radius: 40.0,
                    converge_per_unit: 0.01,
                    converge_rate_cap: 0.2,
                    field_resolution: 16,
                    max_field_cache_bytes: 1024,
                    max_macro_cache_bytes: 2048,
                    max_roster_cache_bytes: 4096,
                    organisms_per_cell: 1,
                },
                budget: BudgetRecord {
                    max_loads: 1,
                    max_converge_regions: 2,
                    max_regen_cost: 3,
                    max_realize_organisms: 4,
                    max_persist_ops: 5,
                    max_route_attraction_nodes: 6,
                    max_retarget_regions: 7,
                },
                tier: SessionTierRecord::Low,
                path_tracking: true,
                route_attraction: true,
                legacy_target_policy: LegacyTargetPolicy::ExactTargetStored,
            },
            player: (300.4, -10.6),
            last_player: (299.0, -10.0),
            bias: [0.1; POSSIBILITY_DIMS],
            transition_mode: true,
            anchors: vec![AnchorSnapshot::from_anchor(&sample_anchor())],
            regions: vec![RegionSnapshotRecord {
                coord: RegionCoord::new(1, -1),
                current: [0.25; POSSIBILITY_DIMS],
                target: [0.75; POSSIBILITY_DIMS],
                stability: 1.0,
                revision: 9,
            }],
            recorder: Some(RouteRecorderSnapshot {
                accumulated: 12.5,
                last_observed: Some((299.0, -10.0)),
                nodes: vec![node],
                discoveries: vec![discovery.id],
            }),
            tracker: RouteTrackerSnapshot {
                legs: vec![RouteTrackerLegSnapshot {
                    route_id: route.id,
                    visited_nodes: vec![0, 1],
                }],
            },
            sequence: 11,
        };
        let mut bundle = AtlasBundle {
            discoveries: vec![discovery.clone()],
            routes: vec![route.clone()],
            preserves: vec![preserve.clone()],
        };
        bundle.canonicalize();

        macro_rules! round_trip {
            ($kind:expr, $value:expr, $ty:ty) => {{
                let bytes = encode_record($kind, &$value);
                let (_, decoded): (Envelope, $ty) =
                    decode_record(&bytes, $kind).expect("round trip");
                assert_eq!(decoded, $value);
            }};
        }
        round_trip!(RecordKind::Discovery, discovery, DiscoveryRecord);
        round_trip!(RecordKind::Route, route, RouteRecord);
        round_trip!(RecordKind::Preserve, preserve, PreserveRecord);
        round_trip!(RecordKind::Seen, seen, SeenRecord);
        round_trip!(RecordKind::Session, session, SessionSnapshot);
        round_trip!(RecordKind::Bundle, bundle, AtlasBundle);
    }

    #[test]
    fn session_snapshot_is_bit_exact() {
        // The session tier's whole point (ADR 0013): floats round-trip
        // bit-for-bit, including awkward values.
        let anchor = Anchor {
            strength: f32::from_bits(0x3F7F_FFFF), // just below 1.0
            world_pos: (1.0e-300, -0.0),
            ..sample_anchor()
        };
        let snap = AnchorSnapshot::from_anchor(&anchor);
        let bytes = encode_record(RecordKind::Session, &snap);
        let (_, decoded): (Envelope, AnchorSnapshot) =
            decode_record(&bytes, RecordKind::Session).expect("round trip");
        let restored = decoded.to_anchor();
        assert_eq!(restored.strength.to_bits(), anchor.strength.to_bits());
        assert_eq!(restored.world_pos.0.to_bits(), anchor.world_pos.0.to_bits());
        assert_eq!(restored.world_pos.1.to_bits(), anchor.world_pos.1.to_bits());
        for (a, b) in restored.target.dims.iter().zip(anchor.target.dims) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn future_format_is_refused_not_guessed() {
        let record = sample_discovery();
        let envelope = Envelope {
            format_version: RECORD_FORMAT_VERSION + 1,
            world_version: WORLD_ALGORITHM_VERSION,
            kind: RecordKind::Discovery,
        };
        let mut bytes = postcard::to_allocvec(&envelope).unwrap();
        bytes.extend_from_slice(&postcard::to_allocvec(&record).unwrap());
        let err = decode_record::<DiscoveryRecord>(&bytes, RecordKind::Discovery).unwrap_err();
        assert_eq!(
            err,
            RecordError::UnsupportedFormat(RECORD_FORMAT_VERSION + 1)
        );
    }

    #[test]
    fn wrong_kind_and_corruption_are_refused() {
        let record = sample_discovery();
        let bytes = encode_record(RecordKind::Discovery, &record);
        let err = decode_record::<DiscoveryRecord>(&bytes, RecordKind::Route).unwrap_err();
        assert_eq!(
            err,
            RecordError::WrongKind {
                expected: RecordKind::Route,
                found: RecordKind::Discovery,
            }
        );
        // Truncated body.
        let err =
            decode_record::<DiscoveryRecord>(&bytes[..bytes.len() - 3], RecordKind::Discovery)
                .unwrap_err();
        assert_eq!(err, RecordError::Corrupt);
        // Trailing garbage.
        let mut noisy = bytes.clone();
        noisy.push(0xFF);
        let err = decode_record::<DiscoveryRecord>(&noisy, RecordKind::Discovery).unwrap_err();
        assert_eq!(err, RecordError::Corrupt);
    }

    #[test]
    fn content_id_excludes_mutable_fields_and_covers_immutable_ones() {
        let base = sample_discovery();
        // Rename ⇒ same id.
        let mut renamed = base.clone();
        renamed.name = String::from("dawn glowfin");
        renamed.journal = String::from("found at the river mouth");
        renamed.sequence = 99;
        assert_eq!(renamed.content_id(), base.id);
        // Any immutable change ⇒ new id.
        let mut variants = Vec::new();
        for f in [
            |r: &mut DiscoveryRecord| r.source = AnchorSource::Landform,
            |r: &mut DiscoveryRecord| r.signature_seed ^= 1,
            |r: &mut DiscoveryRecord| r.target.buckets[0] ^= 1,
            |r: &mut DiscoveryRecord| r.mask ^= 1,
            |r: &mut DiscoveryRecord| r.kind = AnchorKind::Suppress,
            |r: &mut DiscoveryRecord| r.strength_q ^= 1,
            |r: &mut DiscoveryRecord| r.falloff_q ^= 1,
            |r: &mut DiscoveryRecord| r.pos_q.0 ^= 1,
            |r: &mut DiscoveryRecord| r.pos_q.1 ^= 1,
        ] {
            let mut v = base.clone();
            f(&mut v);
            variants.push(v.content_id());
        }
        for id in &variants {
            assert_ne!(*id, base.id, "immutable field change did not move the id");
        }
    }

    #[test]
    fn record_anchor_round_trip_is_within_quantization_epsilon() {
        let anchor = sample_anchor();
        let record = sample_discovery();
        let restored = record.to_anchor();
        // Target dims within half a bucket.
        let eps = 0.5 / f32::from(POSSIBILITY_QUANT) + 1e-6;
        for (a, b) in restored.target.dims.iter().zip(anchor.target.dims) {
            assert!((a - b).abs() <= eps, "target moved {}", (a - b).abs());
        }
        assert!((restored.strength - anchor.strength).abs() <= eps);
        // Position within half a world unit; falloff within half a unit.
        assert!((restored.world_pos.0 - anchor.world_pos.0).abs() <= 0.5);
        assert!((restored.world_pos.1 - anchor.world_pos.1).abs() <= 0.5);
        assert!((restored.falloff_radius - anchor.falloff_radius).abs() <= 0.5);
        // Everything integer is exact.
        assert_eq!(restored.mask, anchor.mask);
        assert_eq!(restored.kind, anchor.kind);
        assert_eq!(restored.source, anchor.source);
        // And a re-record of the restored anchor is a fixed point (id-stable).
        let again =
            DiscoveryRecord::from_anchor(&restored, record.signature_seed, 0, String::new());
        assert_eq!(again.id, record.id);
    }

    #[test]
    fn possibility_signature_round_trips_and_seed_separates() {
        let mut v = PossibilityVector::neutral();
        v.set(PossibilityDomain::Ecology, 0.73);
        v.set(PossibilityDomain::Climate, 0.11);
        let sig = PossibilitySignature::of(v);
        assert_eq!(PossibilitySignature::of(sig.dequantize()), sig);
        let mut other = sig;
        other.buckets[PossibilityDomain::Ecology.index()] ^= 1;
        assert_ne!(other.seed(), sig.seed());
    }

    #[test]
    fn seen_record_marks_and_merges_by_union() {
        let region = RegionCoord::new(-3, 7);
        let chunk = SeenRecord::chunk_of(region);
        assert_eq!(chunk.level, SEEN_CHUNK_LEVEL);
        let mut a = SeenRecord::empty(chunk);
        assert!(!a.contains(region));
        assert!(a.mark(region));
        assert!(!a.mark(region), "re-mark is not new");
        assert!(a.contains(region));
        assert_eq!(a.count(), 1);
        // A region from a different chunk is refused, not silently misfiled.
        assert!(!a.mark(RegionCoord::new(100, 100)));
        // Union merge.
        let other_region = RegionCoord::new(-1, 1);
        assert_eq!(SeenRecord::chunk_of(other_region), chunk);
        let mut b = SeenRecord::empty(chunk);
        b.mark(other_region);
        assert!(a.merge_from(&b));
        assert!(a.contains(other_region) && a.contains(region));
        assert!(!a.merge_from(&b), "idempotent");
    }

    #[test]
    fn seen_chunking_floors_toward_negative_infinity() {
        assert_eq!(
            SeenRecord::chunk_of(RegionCoord::new(-1, -1)),
            RegionCoord::at_level(-1, -1, SEEN_CHUNK_LEVEL)
        );
        assert_eq!(
            SeenRecord::chunk_of(RegionCoord::new(0, 15)),
            RegionCoord::at_level(0, 0, SEEN_CHUNK_LEVEL)
        );
        assert_eq!(
            SeenRecord::chunk_of(RegionCoord::new(-16, 16)),
            RegionCoord::at_level(-1, 1, SEEN_CHUNK_LEVEL)
        );
    }

    #[test]
    fn merge_is_commutative_and_idempotent_on_mutable_fields() {
        let base = sample_discovery();
        let mut a = base.clone();
        a.sequence = 10;
        a.name = String::from("a-name");
        let mut b = base.clone();
        b.sequence = 12;
        b.name = String::from("b-name");
        // a←b and b←a converge to the same record.
        let mut ab = a.clone();
        ab.merge_from(&b).unwrap();
        let mut ba = b.clone();
        ba.merge_from(&a).unwrap();
        assert_eq!(ab, ba);
        // Idempotent.
        let before = ab.clone();
        assert!(!ab.merge_from(&before.clone()).unwrap());
        assert_eq!(ab, before);
        // Equal sequences resolve by the deterministic content tiebreak,
        // still commutatively.
        let mut c = base.clone();
        c.sequence = 12;
        c.name = String::from("c-name");
        let mut bc = b.clone();
        bc.merge_from(&c).unwrap();
        let mut cb = c.clone();
        cb.merge_from(&b).unwrap();
        assert_eq!(bc, cb);
    }

    #[test]
    fn route_usage_merges_by_max() {
        let node = RouteNode {
            pos_q: (0, 0),
            signature: PossibilitySignature::of(PossibilityVector::neutral()),
            current_signature: None,
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
            distance_q: 0,
        };
        let base = RouteRecord::new(vec![node], vec![], 1, "r".into());
        let mut a = base.clone();
        a.usage = 5;
        let mut b = base.clone();
        b.usage = 3;
        assert!(!a.clone().merge_from(&b).unwrap() || a.usage == 5);
        let mut merged = a.clone();
        merged.merge_from(&b).unwrap();
        assert_eq!(
            merged.usage, 5,
            "max, not sum: re-import never double-counts"
        );
        let mut merged2 = b.clone();
        merged2.merge_from(&a).unwrap();
        assert_eq!(merged2.usage, 5);
    }

    #[test]
    fn route_content_id_covers_current_signature_and_distance_for_v2_nodes() {
        let mut node = RouteNode {
            pos_q: (0, 0),
            signature: PossibilitySignature::of(PossibilityVector::neutral()),
            current_signature: Some(PossibilitySignature::of(PossibilityVector::neutral())),
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
            distance_q: 192,
        };
        let base = RouteRecord::new(vec![node], vec![], 1, "r".into());
        node.current_signature = Some(PossibilitySignature {
            buckets: [1; POSSIBILITY_DIMS],
        });
        assert_ne!(
            RouteRecord::new(vec![node], vec![], 1, "r".into()).id,
            base.id
        );
        node.current_signature = base.nodes[0].current_signature;
        node.distance_q = 193;
        assert_ne!(
            RouteRecord::new(vec![node], vec![], 1, "r".into()).id,
            base.id
        );
    }

    #[test]
    fn v1_route_decodes_with_legacy_id_preserved() {
        let sig = PossibilitySignature::of(PossibilityVector::neutral());
        let old_node = v1::RouteNodeV1 {
            pos_q: (12, 34),
            signature: sig,
            cost_q: 9,
            stability_q: 8,
            anchor_sig: 7,
        };
        let migrated_node = old_node.migrate();
        let legacy = RouteRecord::new(vec![migrated_node], vec![3, 1], 5, "old".into());
        let old = v1::RouteRecordV1 {
            id: legacy.id,
            nodes: vec![old_node],
            usage: 2,
            discoveries: vec![1, 3],
            sequence: 5,
            name: "old".into(),
            journal: "notes".into(),
        };
        let envelope = Envelope {
            format_version: 1,
            world_version: WORLD_ALGORITHM_VERSION,
            kind: RecordKind::Route,
        };
        let mut bytes = postcard::to_allocvec(&envelope).unwrap();
        bytes.extend_from_slice(&postcard::to_allocvec(&old).unwrap());

        let (_, decoded): (Envelope, RouteRecord) =
            decode_record(&bytes, RecordKind::Route).expect("v1 route migrates");
        assert_eq!(decoded.id, legacy.id);
        assert_eq!(decoded.content_id(), legacy.id);
        assert_eq!(decoded.nodes[0].current_signature, None);
        assert_eq!(decoded.nodes[0].distance_q, 0);
    }

    #[test]
    fn v1_session_decodes_with_target_equals_current_policy() {
        let current = [0.25; POSSIBILITY_DIMS];
        let old = v1::SessionSnapshotV1 {
            player: (1.0, 2.0),
            last_player: (0.0, 2.0),
            bias: [0.0; POSSIBILITY_DIMS],
            transition_mode: false,
            anchors: Vec::new(),
            regions: vec![v1::RegionSnapshotRecordV1 {
                coord: RegionCoord::new(0, 0),
                current,
                stability: 0.5,
                revision: 4,
            }],
            sequence: 9,
        };
        let envelope = Envelope {
            format_version: 1,
            world_version: WORLD_ALGORITHM_VERSION,
            kind: RecordKind::Session,
        };
        let mut bytes = postcard::to_allocvec(&envelope).unwrap();
        bytes.extend_from_slice(&postcard::to_allocvec(&old).unwrap());

        let (_, decoded): (Envelope, SessionSnapshot) =
            decode_record(&bytes, RecordKind::Session).expect("v1 session migrates");
        assert_eq!(
            decoded.runtime.legacy_target_policy,
            LegacyTargetPolicy::TargetEqualsCurrent
        );
        assert_eq!(decoded.regions[0].target, current);
        assert!(decoded.recorder.is_none());
        assert!(decoded.tracker.legs.is_empty());
    }

    #[test]
    fn merge_rejects_same_id_immutable_conflict() {
        let mut base = sample_discovery();
        let before = base.clone();
        let mut tampered = base.clone();
        tampered.strength_q ^= 1;
        tampered.id = base.id;

        assert_eq!(
            base.merge_from(&tampered),
            Err(RecordMergeError::ImmutableConflict { id: before.id })
        );
        assert_eq!(base, before);
    }

    #[test]
    fn merge_allows_same_immutable_body_mutable_update() {
        let mut base = sample_discovery();
        let mut renamed = base.clone();
        renamed.sequence = base.sequence + 1;
        renamed.name = "renamed".into();
        renamed.journal = "later".into();

        assert_eq!(base.merge_from(&renamed), Ok(true));
        assert_eq!(base.name, "renamed");
        assert_eq!(base.journal, "later");
    }

    #[test]
    fn preserve_id_is_a_function_of_the_region_set() {
        let sig = PossibilitySignature::of(PossibilityVector::neutral());
        let regions = vec![
            (RegionCoord::new(1, 2), sig),
            (RegionCoord::new(-1, 0), sig),
        ];
        let mut reversed = regions.clone();
        reversed.reverse();
        let a = PreserveRecord::new(regions, 0, String::new());
        let b = PreserveRecord::new(reversed, 5, String::from("named"));
        assert_eq!(a.id, b.id, "entry order and mutable fields must not matter");
    }

    #[test]
    fn preserve_constructor_deduplicates_exact_regions() {
        let sig = PossibilitySignature::of(PossibilityVector::neutral());
        let coord = RegionCoord::new(1, 2);
        let one = PreserveRecord::new(vec![(coord, sig)], 0, String::new());
        let duplicate = PreserveRecord::new(vec![(coord, sig), (coord, sig)], 0, String::new());

        assert_eq!(duplicate.regions, one.regions);
        assert_eq!(duplicate.id, one.id);
    }

    #[test]
    fn preserve_constructor_rejects_conflicting_duplicate_region() {
        let coord = RegionCoord::new(1, 2);
        let first = PossibilitySignature::of(PossibilityVector::neutral());
        let mut second = first;
        second.buckets[PossibilityDomain::Ecology.index()] ^= 1;

        assert_eq!(
            PreserveRecord::try_new(vec![(coord, first), (coord, second)], 0, String::new()),
            Err(PreserveRegionError::ConflictingDuplicateRegion { coord })
        );
    }

    #[test]
    fn route_discovery_refs_are_canonical() {
        let node = RouteNode {
            pos_q: (0, 0),
            signature: PossibilitySignature::of(PossibilityVector::neutral()),
            current_signature: None,
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
            distance_q: 0,
        };
        let route = RouteRecord::new(vec![node], vec![9, 3, 9, 1, 3], 0, String::new());

        assert_eq!(route.discoveries, vec![1, 3, 9]);
        assert_eq!(route.id, route.content_id());
    }

    #[test]
    fn bundle_canonicalize_collapses_equal_duplicate_ids() {
        let base = sample_discovery();
        let mut later = base.clone();
        later.sequence = base.sequence + 1;
        later.name = "later".into();
        let mut bundle = AtlasBundle {
            discoveries: vec![later, base],
            ..AtlasBundle::default()
        };

        bundle.canonicalize_checked().unwrap();

        assert_eq!(bundle.discoveries.len(), 1);
        assert_eq!(bundle.discoveries[0].name, "later");
    }

    #[test]
    fn bundle_canonicalize_rejects_same_id_unequal_body() {
        let base = sample_discovery();
        let mut tampered = base.clone();
        tampered.strength_q ^= 1;
        tampered.id = base.id;
        let mut bundle = AtlasBundle {
            discoveries: vec![base.clone(), tampered],
            ..AtlasBundle::default()
        };

        assert!(matches!(
            bundle.canonicalize_checked(),
            Err(BundleCanonicalError::Record(
                RecordCanonicalError::ContentIdMismatch { .. }
            ))
        ));
    }

    #[test]
    fn bundle_validates_content_ids() {
        let mut bundle = AtlasBundle {
            discoveries: vec![sample_discovery()],
            ..AtlasBundle::default()
        };
        assert!(bundle.ids_valid());
        bundle.discoveries[0].strength_q ^= 1; // tamper an immutable field
        assert!(!bundle.ids_valid());
    }
}
