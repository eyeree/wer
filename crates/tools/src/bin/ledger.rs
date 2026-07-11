//! `wer-ledger` — the invalidation-precision harness (phase-2-plan.md §12.3).
//!
//! Usage:
//!     wer-ledger
//!
//! Runs every scenario of the §12.3 table over a settled streaming window and
//! asserts the exact regeneration set: a change recomputes precisely the
//! layers that declare a dependency on it — nothing more. Exits non-zero on
//! any mismatch, so it doubles as a CI-friendly gate on the Phase 2 success
//! criterion.

use std::process::ExitCode;

use tools::run_invalidation_ledger;

fn main() -> ExitCode {
    println!("running the invalidation-precision ledger...");
    let reports = run_invalidation_ledger();
    let mut failed = false;
    for report in &reports {
        println!(
            "[{}] {} ({} regenerations, {} regions flipped)",
            if report.passed() { "pass" } else { "FAIL" },
            report.name,
            report.regenerated,
            report.regions_flipped
        );
        for violation in &report.violations {
            eprintln!("    violation: {violation}");
            failed = true;
        }
    }
    if failed {
        eprintln!("invalidation ledger FAILED");
        ExitCode::FAILURE
    } else {
        println!("invalidation ledger passed");
        ExitCode::SUCCESS
    }
}
