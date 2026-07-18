//! Closed 20-face World Loom experiment and host boundary for Stage 0B.
//!
//! This crate stops before Visualization. It owns canonical integer Model
//! outputs and one-traveler update semantics, but no renderer or platform API.

use core::fmt;
use loom_core::{
    IntentTerm, Mass, MeasureKind, NormalizedIntent, StatePacket, StateRoot, MASS_ONE,
};
use loom_transport::{advance, probe, CompleteProbe, EgressMode, ProbeOutcome, UnresolvedReason};
use sha2::{Digest, Sha256};

/// Number of triangular faces in the closed Stage 0B planet.
pub const FACE_COUNT: usize = 20;
/// Barycentric fixed-point unit.
pub const BARYCENTRIC_ONE: u32 = 1 << 16;
/// Egress micro-length granted by one millimetre of physical travel.
pub const CREDIT_PER_MILLIMETRE: u64 = 1;

// Standard combinatorial icosahedron. Geometry is deliberately deferred.
const FACE_VERTICES: [[u8; 3]; FACE_COUNT] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];

/// Canonical face id.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FaceId(u8);

impl FaceId {
    /// Construct a checked face id.
    pub const fn new(value: u8) -> Result<Self, WorldError> {
        if value < FACE_COUNT as u8 {
            Ok(Self(value))
        } else {
            Err(WorldError::FaceOutOfRange)
        }
    }

    /// Integer face index.
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Return the three neighboring faces in ascending id order.
#[must_use]
pub fn neighbors(face: FaceId) -> [FaceId; 3] {
    let vertices = FACE_VERTICES[usize::from(face.get())];
    let mut result = [FaceId(0); 3];
    let mut count = 0;
    for (index, candidate) in FACE_VERTICES.iter().enumerate() {
        if index == usize::from(face.get()) {
            continue;
        }
        let shared = vertices
            .iter()
            .filter(|vertex| candidate.contains(vertex))
            .count();
        if shared == 2 {
            result[count] = FaceId(index as u8);
            count += 1;
        }
    }
    debug_assert_eq!(count, 3);
    result
}

/// Exact Stage 0B position on a face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanetPosition {
    /// Containing face.
    pub face: FaceId,
    /// First barycentric coordinate in Q16.
    pub u: u32,
    /// Second barycentric coordinate in Q16.
    pub v: u32,
    /// Altitude in centimetres.
    pub altitude_cm: i32,
}

impl PlanetPosition {
    /// Validate an exact face-local position.
    pub const fn new(face: FaceId, u: u32, v: u32, altitude_cm: i32) -> Result<Self, WorldError> {
        if u > BARYCENTRIC_ONE || v > BARYCENTRIC_ONE || u.saturating_add(v) > BARYCENTRIC_ONE {
            Err(WorldError::BarycentricOutOfRange)
        } else {
            Ok(Self {
                face,
                u,
                v,
                altitude_cm,
            })
        }
    }
}

/// Canonical physical movement supplied once to a host update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TravelerPathSegment {
    /// Authoritative pre-update position.
    pub start: PlanetPosition,
    /// Authoritative post-update position.
    pub end: PlanetPosition,
    /// Quantized spherical surface length.
    pub distance_mm: u64,
}

impl TravelerPathSegment {
    /// Validate locality and zero-distance identity.
    pub fn validate(self) -> Result<(), WorldError> {
        if self.distance_mm == 0 && self.start != self.end {
            return Err(WorldError::InvalidZeroSegment);
        }
        if self.start.face != self.end.face && !neighbors(self.start.face).contains(&self.end.face)
        {
            return Err(WorldError::NonAdjacentSegment);
        }
        Ok(())
    }
}

/// One tiny canonical face realization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaceSample {
    /// Face address.
    pub face: FaceId,
    /// Material field sample, Q24.
    pub material: u32,
    /// Two-atom habitat measure summing exactly to Q24 one.
    pub habitat: [u32; 2],
    /// Organism trait sample, Q24.
    pub organism_trait: u32,
}

/// Realize the complete tiny planet in canonical face order.
#[must_use]
pub fn realize(packet: &StatePacket) -> [FaceSample; FACE_COUNT] {
    core::array::from_fn(|index| realize_face(packet, FaceId(index as u8)))
}

fn realize_face(packet: &StatePacket, face: FaceId) -> FaceSample {
    let material_bias = packet.mass(MeasureKind::Material, 0, face.get() % 8).raw();
    let trait_bias = packet.mass(MeasureKind::Trait, 0, face.get() % 8).raw();
    let material_noise = sample_q24(packet.root(), b"material", face);
    let habitat_noise = sample_q24(packet.root(), b"habitat", face);
    let trait_noise = sample_q24(packet.root(), b"organism-trait", face);
    let material = average_q24(material_bias, material_noise);
    let habitat_a = average_q24(trait_bias, habitat_noise);
    let organism_trait = average_q24(trait_bias, trait_noise);
    FaceSample {
        face,
        material,
        habitat: [habitat_a, MASS_ONE - habitat_a],
        organism_trait,
    }
}

fn average_q24(a: u32, b: u32) -> u32 {
    ((u64::from(a) + u64::from(b)) / 2) as u32
}

fn sample_q24(root: StateRoot, channel: &[u8], face: FaceId) -> u32 {
    let mut digest = Sha256::new();
    digest.update(b"loom-world-0b-field-v1");
    digest.update(channel);
    digest.update(root);
    digest.update([face.get()]);
    let bytes: [u8; 32] = digest.finalize().into();
    u32::from_be_bytes(bytes[..4].try_into().expect("four digest bytes")) & (MASS_ONE - 1)
}

/// Interaction target in the tiny closed world.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Subject {
    /// Conserved material atom.
    Material(u8),
    /// Habitat trait atom.
    Habitat(u8),
    /// Organism trait atom.
    OrganismTrait(u8),
}

/// Stage 0B influence vocabulary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Influence {
    Accentuate,
    Repress,
    Hold,
}

/// One order-independent player interaction.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Interaction {
    /// Semantic subject.
    pub subject: Subject,
    /// Direction or preservation request.
    pub influence: Influence,
    /// Q24 control magnitude before weight.
    pub amount: u32,
    /// Nonnegative importance.
    pub weight: u16,
}

/// Lower interactions to the Stage 0A canonical intent surface.
pub fn lower_interactions(interactions: &[Interaction]) -> Result<NormalizedIntent, WorldError> {
    let mut canonical = interactions.to_vec();
    canonical.sort();
    canonical.dedup();
    let mut terms = Vec::new();
    for interaction in &canonical {
        if interaction.amount > MASS_ONE {
            return Err(WorldError::InteractionOutOfRange);
        }
        let (kind, atom) = subject_address(interaction.subject)?;
        let mut delta =
            match interaction.influence {
                Influence::Accentuate => i32::try_from(interaction.amount)
                    .map_err(|_| WorldError::InteractionOutOfRange)?,
                Influence::Repress => -i32::try_from(interaction.amount)
                    .map_err(|_| WorldError::InteractionOutOfRange)?,
                Influence::Hold => 0,
            };
        if interaction.influence != Influence::Hold
            && canonical.iter().any(|other| {
                other.subject == interaction.subject && other.influence == Influence::Hold
            })
        {
            delta /= 4;
        }
        terms.push(IntentTerm {
            id: interaction_id(*interaction),
            kind,
            level: 0,
            atom,
            delta,
            weight: interaction.weight,
        });
    }
    NormalizedIntent::normalize(terms).map_err(|_| WorldError::InvalidIntent)
}

fn subject_address(subject: Subject) -> Result<(MeasureKind, u8), WorldError> {
    let (kind, atom) = match subject {
        Subject::Material(atom) => (MeasureKind::Material, atom),
        Subject::Habitat(atom) | Subject::OrganismTrait(atom) => (MeasureKind::Trait, atom),
    };
    if atom >= 64 {
        Err(WorldError::InteractionOutOfRange)
    } else {
        Ok((kind, atom))
    }
}

fn interaction_id(interaction: Interaction) -> u64 {
    let mut digest = Sha256::new();
    digest.update(b"loom-world-0b-interaction-v1");
    let (subject, atom) = match interaction.subject {
        Subject::Material(a) => (0, a),
        Subject::Habitat(a) => (1, a),
        Subject::OrganismTrait(a) => (2, a),
    };
    digest.update([subject, atom, interaction.influence as u8]);
    digest.update(interaction.amount.to_be_bytes());
    digest.update(interaction.weight.to_be_bytes());
    let bytes: [u8; 32] = digest.finalize().into();
    u64::from_be_bytes(bytes[..8].try_into().expect("eight digest bytes"))
}

/// One identity face correspondence for the unrefined planet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FaceChange {
    pub face: FaceId,
    pub before: FaceSample,
    pub after: FaceSample,
}

/// Threshold event exposed to a future Visualization.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TransitionEvent {
    Birth(FaceId),
    Death(FaceId),
}

/// Complete bounded Stage 0B transition description.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionPlan {
    pub source_root: StateRoot,
    pub destination_root: StateRoot,
    pub mode_id: [u8; 32],
    pub total_length: u64,
    pub correspondences: [(FaceId, FaceId); FACE_COUNT],
    pub changes: Vec<FaceChange>,
    pub events: Vec<TransitionEvent>,
}

impl TransitionPlan {
    /// Stable replay payload for parity and later presentation tests.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LWTP");
        bytes.extend_from_slice(&self.source_root);
        bytes.extend_from_slice(&self.destination_root);
        bytes.extend_from_slice(&self.mode_id);
        bytes.extend_from_slice(&self.total_length.to_be_bytes());
        bytes.push(self.changes.len() as u8);
        bytes.push(self.events.len() as u8);
        for change in &self.changes {
            bytes.push(change.face.get());
            encode_sample(&mut bytes, change.before);
            encode_sample(&mut bytes, change.after);
        }
        for event in &self.events {
            match event {
                TransitionEvent::Birth(face) => bytes.extend_from_slice(&[0, face.get()]),
                TransitionEvent::Death(face) => bytes.extend_from_slice(&[1, face.get()]),
            }
        }
        bytes
    }
}

fn transition(source: &StatePacket, mode: &EgressMode) -> TransitionPlan {
    let before = realize(source);
    let after = realize(&mode.endpoint);
    let mut changes = Vec::new();
    let mut events = Vec::new();
    for index in 0..FACE_COUNT {
        if before[index] != after[index] {
            changes.push(FaceChange {
                face: before[index].face,
                before: before[index],
                after: after[index],
            });
        }
        let threshold = MASS_ONE / 2;
        if before[index].organism_trait < threshold && after[index].organism_trait >= threshold {
            events.push(TransitionEvent::Birth(before[index].face));
        }
        if before[index].organism_trait >= threshold && after[index].organism_trait < threshold {
            events.push(TransitionEvent::Death(before[index].face));
        }
    }
    events.sort();
    TransitionPlan {
        source_root: source.root(),
        destination_root: mode.endpoint.root(),
        mode_id: mode.mode_id,
        total_length: mode.path_length,
        correspondences: core::array::from_fn(|index| (FaceId(index as u8), FaceId(index as u8))),
        changes,
        events,
    }
}

fn encode_sample(bytes: &mut Vec<u8>, sample: FaceSample) {
    bytes.extend_from_slice(&sample.material.to_be_bytes());
    bytes.extend_from_slice(&sample.habitat[0].to_be_bytes());
    bytes.extend_from_slice(&sample.habitat[1].to_be_bytes());
    bytes.extend_from_slice(&sample.organism_trait.to_be_bytes());
}

/// Model-neutral Map input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapSnapshot {
    pub state_root: StateRoot,
    pub traveler: PlanetPosition,
    pub faces: [FaceSample; FACE_COUNT],
}

/// Integer local tangent descriptor; Visualization chooses actual projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TangentFrame {
    pub face: FaceId,
    pub orientation_code: u8,
}

/// Model-neutral local POV input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PovSnapshot {
    pub state_root: StateRoot,
    pub traveler: PlanetPosition,
    pub tangent: TangentFrame,
    pub neighborhood: [FaceSample; 4],
}

/// Outputs of one authoritative host update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostFrame {
    pub map: MapSnapshot,
    pub pov: PovSnapshot,
    pub transition: Option<TransitionPlan>,
    pub unused_credit: u64,
}

/// One-traveler Stage 0B host state.
#[derive(Clone, Debug)]
pub struct LoomHost {
    packet: StatePacket,
    traveler: PlanetPosition,
    selected: Option<EgressMode>,
    credit: u64,
}

impl LoomHost {
    /// Start one host at a canonical packet and position.
    pub const fn new(packet: StatePacket, traveler: PlanetPosition) -> Self {
        Self {
            packet,
            traveler,
            selected: None,
            credit: 0,
        }
    }
    /// Current canonical state.
    pub const fn packet(&self) -> &StatePacket {
        &self.packet
    }
    /// Current unused travel credit.
    pub const fn credit(&self) -> u64 {
        self.credit
    }
    /// Probe and select one mode by id.
    pub fn plan(
        &mut self,
        intent: &NormalizedIntent,
        mode_id: Option<[u8; 32]>,
    ) -> Result<CompleteProbe, WorldError> {
        let ProbeOutcome::Complete(complete) = probe(&self.packet, intent, u64::MAX) else {
            return Err(WorldError::Unresolved(UnresolvedReason::Infeasible));
        };
        let selected_id = mode_id.unwrap_or(complete.default_mode_id);
        self.selected = complete
            .modes
            .iter()
            .find(|mode| mode.mode_id == selected_id)
            .cloned();
        if self.selected.is_none() {
            return Err(WorldError::UnknownMode);
        }
        Ok(complete)
    }
    /// Apply travel once, commit at most one selected route, then build both DTOs.
    pub fn update(&mut self, segment: TravelerPathSegment) -> Result<HostFrame, WorldError> {
        segment.validate()?;
        if segment.start != self.traveler {
            return Err(WorldError::TravelerDiscontinuity);
        }
        self.credit = self
            .credit
            .checked_add(
                segment
                    .distance_mm
                    .checked_mul(CREDIT_PER_MILLIMETRE)
                    .ok_or(WorldError::CreditOverflow)?,
            )
            .ok_or(WorldError::CreditOverflow)?;
        self.traveler = segment.end;
        let mut transition_plan = None;
        if let Some(selected) = self.selected.take() {
            let (packet, remainder) = advance(&self.packet, &selected, self.credit);
            if packet.root() != self.packet.root() {
                transition_plan = Some(transition(&self.packet, &selected));
                self.packet = packet;
            } else {
                self.selected = Some(selected);
            }
            self.credit = remainder;
        }
        Ok(self.frame(transition_plan))
    }
    /// Read presentation inputs without mutating Model or travel state.
    #[must_use]
    pub fn snapshot(&self) -> HostFrame {
        self.frame(None)
    }
    fn frame(&self, transition_plan: Option<TransitionPlan>) -> HostFrame {
        let faces = realize(&self.packet);
        let current = usize::from(self.traveler.face.get());
        let adjacent = neighbors(self.traveler.face);
        let neighborhood = [
            faces[current],
            faces[usize::from(adjacent[0].get())],
            faces[usize::from(adjacent[1].get())],
            faces[usize::from(adjacent[2].get())],
        ];
        HostFrame {
            map: MapSnapshot {
                state_root: self.packet.root(),
                traveler: self.traveler,
                faces,
            },
            pov: PovSnapshot {
                state_root: self.packet.root(),
                traveler: self.traveler,
                tangent: TangentFrame {
                    face: self.traveler.face,
                    orientation_code: self.traveler.face.get(),
                },
                neighborhood,
            },
            transition: transition_plan,
            unused_credit: self.credit,
        }
    }
}

/// Stage 0B bounded failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorldError {
    FaceOutOfRange,
    BarycentricOutOfRange,
    InvalidZeroSegment,
    NonAdjacentSegment,
    TravelerDiscontinuity,
    InteractionOutOfRange,
    InvalidIntent,
    CreditOverflow,
    UnknownMode,
    Unresolved(UnresolvedReason),
}

impl fmt::Display for WorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for WorldError {}

/// Frozen source used by native and wasm Stage 0B checks.
pub fn fixture() -> Result<(StatePacket, PlanetPosition, NormalizedIntent), WorldError> {
    let packet = StatePacket::normalize(
        vec![loom_core::AtomEntry {
            kind: MeasureKind::Material,
            level: 0,
            atom: 0,
            mass: Mass::ONE,
        }],
        Mass::ONE,
        Mass::ONE,
    )
    .map_err(|_| WorldError::InvalidIntent)?;
    let position =
        PlanetPosition::new(FaceId::new(0)?, BARYCENTRIC_ONE / 3, BARYCENTRIC_ONE / 3, 0)?;
    let intent = lower_interactions(&[
        Interaction {
            subject: Subject::Material(3),
            influence: Influence::Accentuate,
            amount: MASS_ONE / 4,
            weight: 2,
        },
        Interaction {
            subject: Subject::OrganismTrait(5),
            influence: Influence::Accentuate,
            amount: MASS_ONE / 3,
            weight: 2,
        },
    ])?;
    Ok((packet, position, intent))
}

/// Digest of the frozen topology, realization, host, and transition fixture.
pub fn parity_digest() -> Result<[u8; 32], WorldError> {
    let (packet, position, intent) = fixture()?;
    let mut host = LoomHost::new(packet, position);
    let complete = host.plan(&intent, None)?;
    let frame = host.update(TravelerPathSegment {
        start: position,
        end: position,
        distance_mm: complete.modes[0].path_length,
    })?;
    let mut digest = Sha256::new();
    digest.update(b"loom-world-0b-parity-v1");
    for face in frame.map.faces {
        digest.update([face.face.get()]);
        let mut bytes = Vec::new();
        encode_sample(&mut bytes, face);
        digest.update(bytes);
        for neighbor in neighbors(face.face) {
            digest.update([neighbor.get()]);
        }
    }
    digest.update(frame.map.state_root);
    digest.update(frame.pov.state_root);
    digest.update(frame.pov.traveler.u.to_be_bytes());
    digest.update(frame.pov.traveler.v.to_be_bytes());
    digest.update(
        frame
            .transition
            .expect("a full fixture segment commits")
            .canonical_bytes(),
    );
    Ok(digest.finalize().into())
}

/// Frozen native/wasm Stage 0B vector.
#[must_use]
pub fn frozen_parity_vector_matches() -> bool {
    parity_digest().ok()
        == Some([
            0xbb, 0x9e, 0x7b, 0x7c, 0xf5, 0xed, 0xef, 0x53, 0xb8, 0x03, 0x2a, 0xda, 0x4f, 0x55,
            0xdc, 0xc9, 0x37, 0xb2, 0xef, 0x0d, 0x5e, 0x63, 0x2f, 0x2c, 0xf8, 0xbb, 0x24, 0xdf,
            0xdd, 0xb9, 0x95, 0x53,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_is_closed_symmetric_and_has_thirty_edges() {
        let mut edges = 0;
        for raw in 0..FACE_COUNT as u8 {
            let face = FaceId::new(raw).unwrap();
            let adjacent = neighbors(face);
            assert!(adjacent.windows(2).all(|pair| pair[0] < pair[1]));
            for other in adjacent {
                assert!(neighbors(other).contains(&face));
                if face < other {
                    edges += 1;
                }
            }
        }
        assert_eq!(edges, 30);
    }

    #[test]
    fn one_update_keeps_map_and_pov_on_one_state() {
        let (packet, position, intent) = fixture().unwrap();
        let mut host = LoomHost::new(packet, position);
        let complete = host.plan(&intent, None).unwrap();
        assert_eq!(complete.modes.len(), 2);
        assert_ne!(
            complete.modes[0].endpoint.root(),
            complete.modes[1].endpoint.root()
        );
        let distance = complete.modes[0].path_length;
        let frame = host
            .update(TravelerPathSegment {
                start: position,
                end: position,
                distance_mm: distance,
            })
            .unwrap();
        assert_eq!(frame.map.state_root, frame.pov.state_root);
        assert_eq!(frame.map.traveler, frame.pov.traveler);
        let plan = frame.transition.unwrap();
        assert_eq!(plan.correspondences.len(), FACE_COUNT);
        assert!(!plan.changes.is_empty());
        assert_eq!(plan.destination_root, frame.map.state_root);
    }

    #[test]
    fn hold_reduces_requested_delta_order_independently() {
        let accent = Interaction {
            subject: Subject::OrganismTrait(2),
            influence: Influence::Accentuate,
            amount: MASS_ONE / 2,
            weight: 1,
        };
        let hold = Interaction {
            subject: Subject::OrganismTrait(2),
            influence: Influence::Hold,
            amount: 0,
            weight: 4,
        };
        let plain = lower_interactions(&[accent]).unwrap();
        let held = lower_interactions(&[accent, hold]).unwrap();
        let reversed = lower_interactions(&[hold, accent]).unwrap();
        assert_eq!(held, reversed);
        assert!(held.terms()[0].delta.abs() < plain.terms()[0].delta.abs());
    }

    #[test]
    fn frozen_parity_vector_matches_native() {
        assert!(
            frozen_parity_vector_matches(),
            "actual parity digest: {:02x?}",
            parity_digest().unwrap()
        );
    }
}
