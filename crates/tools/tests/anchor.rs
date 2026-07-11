//! The anchor-harness regression test: the machine check of the Phase 4 success
//! criterion (phase-4-plan.md §12.3). Every scenario must hold — intentional and
//! selective steering, coherence, diversity retention, and resonance gating
//! simultaneously, so none is won by sacrificing another.

use tools::run_anchor_harness;

#[test]
fn anchor_harness_scenarios_hold() {
    for report in run_anchor_harness() {
        assert!(
            report.passed(),
            "anchor scenario {:?} failed ({}):\n{}",
            report.name,
            report.summary,
            report.violations.join("\n")
        );
    }
}
