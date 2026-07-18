//! Stage 0A World Loom correctness and performance sign-off harness.

use std::time::{Duration, Instant};

use loom_core::{
    AtomEntry, IntentTerm, Mass, MeasureKind, NormalizedIntent, StatePacket, MASS_ONE,
};
use loom_transport::{parity_fixture, probe, verify_certificate, ProbeOutcome};

/// Summary returned by the Stage 0A sign-off harness.
#[derive(Debug)]
pub struct LoomReport {
    /// Exhaustive small distributions checked.
    pub exhaustive_cases: usize,
    /// Randomized permutation/schedule cases checked.
    pub randomized_cases: usize,
    /// Ordinary probes producing a complete prefix.
    pub ordinary_complete: usize,
    /// Total ordinary probes.
    pub ordinary_total: usize,
    /// Adversarial probes producing a complete prefix.
    pub adversarial_complete: usize,
    /// Total adversarial probes.
    pub adversarial_total: usize,
    /// Worst packet normalization duration in the frozen corpus.
    pub max_normalization: Duration,
    /// Worst Egress duration in the frozen ordinary corpus.
    pub max_probe: Duration,
    /// Gate violations.
    pub violations: Vec<String>,
}

impl LoomReport {
    /// Whether all correctness and native interaction gates passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Run exhaustive, randomized, representative, and adversarial Stage 0A gates.
#[must_use]
pub fn run_loom_harness() -> LoomReport {
    let mut violations = Vec::new();
    let exhaustive_cases = exhaustive_small(&mut violations);
    let randomized_cases = randomized_permutations(&mut violations);
    let (ordinary_complete, ordinary_total, max_normalization, max_probe) =
        ordinary_corpus(&mut violations);
    let (adversarial_complete, adversarial_total) = adversarial_corpus(&mut violations);
    if ordinary_complete * 100 < ordinary_total * 99 {
        violations.push(format!(
            "ordinary complete rate {ordinary_complete}/{ordinary_total} is below 99%"
        ));
    }
    if max_normalization >= Duration::from_millis(1) {
        violations.push(format!(
            "native normalization {:?} exceeded the 1 ms gate",
            max_normalization
        ));
    }
    if max_probe >= Duration::from_millis(4) {
        violations.push(format!(
            "native Egress {:?} exceeded the 4 ms gate",
            max_probe
        ));
    }
    LoomReport {
        exhaustive_cases,
        randomized_cases,
        ordinary_complete,
        ordinary_total,
        adversarial_complete,
        adversarial_total,
        max_normalization,
        max_probe,
        violations,
    }
}

fn exhaustive_small(violations: &mut Vec<String>) -> usize {
    let mut cases = 0;
    for left in 0..=8u32 {
        for middle in 0..=8 - left {
            let right = 8 - left - middle;
            let source = packet([left, middle, right], 8);
            for atom in 0..3u8 {
                let terms = [
                    IntentTerm {
                        id: 2,
                        kind: MeasureKind::Material,
                        level: 0,
                        atom,
                        delta: 1,
                        weight: 1,
                    },
                    IntentTerm {
                        id: 1,
                        kind: MeasureKind::Material,
                        level: 0,
                        atom: (atom + 1) % 3,
                        delta: -1,
                        weight: 1,
                    },
                ];
                compare_orders(&source, &terms, violations, "exhaustive");
                cases += 1;
            }
        }
    }
    cases
}

fn randomized_permutations(violations: &mut Vec<String>) -> usize {
    let mut rng = SplitMix64(0x4c6c_a5de_38f9_0b17);
    for case in 0..10_000usize {
        let a = (rng.next() % 9) as u32;
        let b = (rng.next() % (9 - u64::from(a))) as u32;
        let source = packet([a, b, 8 - a - b], 8);
        let terms = [
            IntentTerm {
                id: 10,
                kind: MeasureKind::Material,
                level: 0,
                atom: (rng.next() % 3) as u8,
                delta: (rng.next() % 5) as i32 - 2,
                weight: 1,
            },
            IntentTerm {
                id: 20,
                kind: MeasureKind::Trait,
                level: 0,
                atom: (rng.next() % 4) as u8,
                delta: (rng.next() % 4) as i32,
                weight: 2,
            },
            IntentTerm {
                id: 30,
                kind: MeasureKind::Material,
                level: 0,
                atom: (rng.next() % 3) as u8,
                delta: (rng.next() % 5) as i32 - 2,
                weight: 1,
            },
        ];
        compare_orders(&source, &terms, violations, &format!("random case {case}"));
        if !violations.is_empty() {
            break;
        }
    }
    10_000
}

fn compare_orders(
    source: &StatePacket,
    terms: &[IntentTerm],
    violations: &mut Vec<String>,
    label: &str,
) {
    let forward = NormalizedIntent::normalize(terms.to_vec()).expect("valid harness intent");
    let reverse = NormalizedIntent::normalize(terms.iter().rev().copied().collect())
        .expect("valid reversed harness intent");
    let a = probe(source, &forward, u64::MAX);
    let b = probe(source, &reverse, u64::MAX);
    if forward != reverse || a != b {
        violations.push(format!(
            "{label}: permutation or schedule changed canonical result"
        ));
        return;
    }
    if let ProbeOutcome::Complete(complete) = a {
        for mode in &complete.modes {
            if let Err(error) = verify_certificate(source, &forward, mode) {
                violations.push(format!("{label}: certificate replay failed: {error}"));
            }
        }
    }
}

fn ordinary_corpus(violations: &mut Vec<String>) -> (usize, usize, Duration, Duration) {
    let mut complete = 0;
    let mut max_normalization = Duration::ZERO;
    let mut max_probe = Duration::ZERO;
    for index in 0..128u32 {
        let started = Instant::now();
        let source = packet([2 + index % 3, 3, 3 - index % 3], 8);
        max_normalization = max_normalization.max(started.elapsed());
        let intent = NormalizedIntent::normalize(vec![
            IntentTerm {
                id: 1,
                kind: MeasureKind::Material,
                level: 0,
                atom: (index % 4) as u8,
                delta: 1,
                weight: 1,
            },
            IntentTerm {
                id: 2,
                kind: MeasureKind::Trait,
                level: 0,
                atom: (index % 8) as u8,
                delta: (index % 5) as i32,
                weight: 1,
            },
        ])
        .expect("ordinary intent");
        let started = Instant::now();
        let outcome = probe(&source, &intent, u64::MAX);
        max_probe = max_probe.max(started.elapsed());
        if let ProbeOutcome::Complete(result) = outcome {
            complete += 1;
            if result
                .modes
                .iter()
                .any(|mode| verify_certificate(&source, &intent, mode).is_err())
            {
                violations.push(format!("ordinary {index}: invalid certificate"));
            }
        }
    }
    (complete, 128, max_normalization, max_probe)
}

fn adversarial_corpus(violations: &mut Vec<String>) -> (usize, usize) {
    let (source, intent, fixture) = parity_fixture().expect("frozen parity fixture");
    let cases = [
        probe(&source, &intent, 0),
        probe(&source, &intent, u64::MAX),
    ];
    let complete = cases
        .iter()
        .filter(|case| matches!(case, ProbeOutcome::Complete(_)))
        .count();
    if !matches!(cases[0], ProbeOutcome::Unresolved(_)) || fixture.modes.is_empty() {
        violations.push("adversarial length boundary did not fail closed".into());
    }
    (complete, cases.len())
}

fn packet(material: [u32; 3], total: u32) -> StatePacket {
    let entries = material
        .into_iter()
        .enumerate()
        .filter(|(_, raw)| *raw != 0)
        .map(|(atom, raw)| AtomEntry {
            kind: MeasureKind::Material,
            level: 0,
            atom: atom as u8,
            mass: Mass::new(raw).unwrap(),
        })
        .collect();
    StatePacket::normalize(
        entries,
        Mass::new(total).unwrap(),
        Mass::new(MASS_ONE).unwrap(),
    )
    .expect("valid harness packet")
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
    fn stage_zero_a_correctness_ledger_passes() {
        let report = run_loom_harness();
        assert!(report.passed(), "{:?}", report.violations);
    }
}
