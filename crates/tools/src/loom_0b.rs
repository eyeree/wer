//! Stage 0B pre-visualization correctness and performance ledger.

use loom_core::MASS_ONE;
use loom_transport::{probe_with_execution, ProbeOutcome};
use loom_world::{
    fixture, lower_interactions, realize, FaceId, Influence, Interaction, LoomHost, PlanetPosition,
    Subject, TravelerPathSegment, BARYCENTRIC_ONE, FACE_COUNT,
};
use std::time::{Duration, Instant};

/// Machine-checkable evidence before the visual and human kill gates.
#[derive(Debug)]
pub struct Loom0bReport {
    pub ordinary_complete: usize,
    pub ordinary_total: usize,
    pub randomized_cases: usize,
    pub adversarial_passed: usize,
    pub adversarial_total: usize,
    pub max_case: Duration,
    pub violations: Vec<String>,
}

impl Loom0bReport {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Run the frozen Stage 0B pre-visualization gate.
#[must_use]
pub fn run_loom_0b_harness() -> Loom0bReport {
    let mut violations = Vec::new();
    let mut ordinary_complete = 0;
    let mut max_case = Duration::ZERO;
    for index in 0..128u32 {
        let started = Instant::now();
        let (packet, position, _) = fixture().expect("valid frozen fixture");
        let influence = match index % 3 {
            0 => Influence::Accentuate,
            1 => Influence::Repress,
            _ => Influence::Hold,
        };
        let subject = match index % 3 {
            0 => Subject::Material((index % 8) as u8),
            1 => Subject::Habitat((index % 8) as u8),
            _ => Subject::OrganismTrait((index % 8) as u8),
        };
        let interaction = Interaction {
            subject,
            influence,
            amount: if influence == Influence::Hold {
                0
            } else {
                MASS_ONE / 8
            },
            weight: 1 + (index % 4) as u16,
        };
        let intent = lower_interactions(&[interaction]).expect("ordinary interaction");
        let mut host = LoomHost::new(packet, position);
        if let Ok(complete) = host.plan(&intent, None) {
            ordinary_complete += 1;
            let distance = complete.modes[0].path_length;
            let frame = host
                .update(TravelerPathSegment {
                    start: position,
                    end: position,
                    distance_mm: distance,
                })
                .expect("ordinary travel");
            if frame.map.state_root != frame.pov.state_root
                || frame.map.traveler != frame.pov.traveler
            {
                violations.push(format!("ordinary {index}: Map/POV authority diverged"));
            }
            if let Some(plan) = frame.transition {
                if plan.correspondences.len() != FACE_COUNT || plan.changes.is_empty() {
                    violations.push(format!("ordinary {index}: incomplete transition"));
                }
            }
        }
        max_case = max_case.max(started.elapsed());
    }
    if ordinary_complete * 100 < 128 * 99 {
        violations.push(format!(
            "ordinary complete rate {ordinary_complete}/128 is below 99%"
        ));
    }
    if max_case >= Duration::from_millis(8) {
        violations.push(format!("native Stage 0B case {max_case:?} exceeded 8 ms"));
    }

    let randomized_cases = randomized(&mut violations);
    let (adversarial_passed, adversarial_total) = adversarial(&mut violations);
    Loom0bReport {
        ordinary_complete,
        ordinary_total: 128,
        randomized_cases,
        adversarial_passed,
        adversarial_total,
        max_case,
        violations,
    }
}

fn randomized(violations: &mut Vec<String>) -> usize {
    let (packet, position, _) = fixture().expect("valid fixture");
    let mut rng = SplitMix64(0x7d83_4a21_901c_fe02);
    for case in 0..10_000usize {
        let atom = (rng.next() % 8) as u8;
        let a = Interaction {
            subject: Subject::Material(atom),
            influence: Influence::Accentuate,
            amount: MASS_ONE / 16,
            weight: 1,
        };
        let b = Interaction {
            subject: Subject::OrganismTrait((atom + 1) % 8),
            influence: Influence::Accentuate,
            amount: MASS_ONE / 12,
            weight: 2,
        };
        let forward = lower_interactions(&[a, b]).expect("random interaction");
        let reverse = lower_interactions(&[b, a]).expect("random reverse interaction");
        if forward != reverse {
            violations.push(format!("random {case}: interaction order changed intent"));
            break;
        }
        let schedule = if rng.next() & 1 == 0 { [0, 1] } else { [1, 0] };
        let canonical = loom_transport::probe(&packet, &forward, u64::MAX);
        let scheduled = probe_with_execution(
            &packet,
            &forward,
            u64::MAX,
            &schedule,
            (rng.next() & 3) as u8,
        );
        if canonical != scheduled {
            violations.push(format!("random {case}: schedule changed probe"));
            break;
        }
        // Exercise query order without making it an identity input.
        let faces = realize(&packet);
        let first = (rng.next() % FACE_COUNT as u64) as usize;
        if faces[first].face != FaceId::new(first as u8).expect("bounded face") {
            violations.push(format!("random {case}: realization address mismatch"));
            break;
        }
        if case % 64 == 0 {
            let ProbeOutcome::Complete(complete) = canonical else {
                violations.push(format!("random {case}: unresolved ordinary probe"));
                break;
            };
            let distance = complete.modes[0].path_length;
            let split = distance / 2;
            let mut segmented = LoomHost::new(packet.clone(), position);
            segmented.plan(&forward, None).expect("segmented plan");
            segmented
                .update(TravelerPathSegment {
                    start: position,
                    end: position,
                    distance_mm: split,
                })
                .expect("first segment");
            let result = segmented
                .update(TravelerPathSegment {
                    start: position,
                    end: position,
                    distance_mm: distance - split,
                })
                .expect("second segment");
            let mut single = LoomHost::new(packet.clone(), position);
            single.plan(&forward, None).expect("single plan");
            let expected = single
                .update(TravelerPathSegment {
                    start: position,
                    end: position,
                    distance_mm: distance,
                })
                .expect("single segment");
            if result.map.state_root != expected.map.state_root
                || result.unused_credit != expected.unused_credit
            {
                violations.push(format!("random {case}: segmentation changed result"));
                break;
            }
        }
    }
    10_000
}

fn adversarial(violations: &mut Vec<String>) -> (usize, usize) {
    let (packet, position, intent) = fixture().expect("valid fixture");
    let mut passed = 0;
    let mut check = |condition: bool, name: &str| {
        if condition {
            passed += 1;
        } else {
            violations.push(format!("adversarial failed: {name}"));
        }
    };
    check(FaceId::new(20).is_err(), "face cap");
    check(
        PlanetPosition::new(FaceId::new(0).unwrap(), BARYCENTRIC_ONE, 1, 0).is_err(),
        "barycentric cap",
    );
    let nonadjacent = PlanetPosition::new(FaceId::new(10).unwrap(), 0, 0, 0).unwrap();
    check(
        TravelerPathSegment {
            start: position,
            end: nonadjacent,
            distance_mm: 1,
        }
        .validate()
        .is_err(),
        "nonadjacent travel",
    );
    let moved = PlanetPosition::new(position.face, position.u + 1, position.v, 0).unwrap();
    check(
        TravelerPathSegment {
            start: position,
            end: moved,
            distance_mm: 0,
        }
        .validate()
        .is_err(),
        "zero travel mismatch",
    );
    let complete = match loom_transport::probe(&packet, &intent, u64::MAX) {
        ProbeOutcome::Complete(value) => value,
        _ => panic!("fixture resolves"),
    };
    check(
        complete.modes.len() == 2
            && complete.modes[0].endpoint.root() != complete.modes[1].endpoint.root(),
        "distinct endpoints",
    );
    let mut host = LoomHost::new(packet, position);
    host.plan(&intent, None).unwrap();
    let before = host.snapshot();
    let after = host.snapshot();
    check(
        before == after && host.credit() == 0,
        "snapshot is read-only",
    );
    (passed, 6)
}

#[derive(Debug)]
struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stage_zero_b_pre_visualization_ledger_passes() {
        let report = run_loom_0b_harness();
        assert!(report.passed(), "{:?}", report.violations);
    }
}
