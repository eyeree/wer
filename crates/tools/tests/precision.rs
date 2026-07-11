//! The invalidation-precision regression test: the machine check of the
//! Phase 2 success criterion (phase-2-plan.md §12.3). Every scenario must
//! assert its exact regeneration set.

use tools::run_invalidation_ledger;

#[test]
fn invalidation_ledger_scenarios_assert_their_exact_regen_sets() {
    for report in run_invalidation_ledger() {
        assert!(
            report.passed(),
            "scenario {:?} failed:\n{}",
            report.name,
            report.violations.join("\n")
        );
    }
}
