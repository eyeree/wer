//! The ecology-harness regression test: the machine check of the Phase 3
//! success criterion (phase-3-plan.md §12.3). Every scenario must hold —
//! coherence invariants and the diversity floor simultaneously, so neither is
//! won by sacrificing the other.

use tools::run_ecology_harness;

#[test]
fn ecology_harness_scenarios_hold() {
    for report in run_ecology_harness() {
        assert!(
            report.passed(),
            "ecology scenario {:?} failed ({}):\n{}",
            report.name,
            report.summary,
            report.violations.join("\n")
        );
    }
}
