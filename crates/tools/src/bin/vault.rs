//! `wer-vault` — the Phase 5 persistence/sharing harness
//! (phase-5-plan.md §12.3), the phase's machine-checkable sign-off alongside
//! `wer-ledger` (invalidation precision), the ecology harness, and
//! `wer-anchor` (steering).
//!
//! Usage:
//!     wer-vault
//!
//! Runs the durable / sparse / shareable / preserve / route / precision
//! scenario families over scripted journeys (`MemoryStorage` +
//! `InlineExecutor`) and exits non-zero on any violation — a CI-friendly gate
//! on the Phase 5 success criterion: exploration creates durable, shareable
//! structure without storing generated world geometry.

use std::process::ExitCode;

use tools::run_vault_harness;

fn main() -> ExitCode {
    println!("running the vault harness...");
    let reports = run_vault_harness();
    let mut failed = false;
    for report in &reports {
        println!(
            "[{}] {} ({})",
            if report.passed() { "pass" } else { "FAIL" },
            report.name,
            report.summary
        );
        for violation in &report.violations {
            eprintln!("    violation: {violation}");
            failed = true;
        }
    }
    if failed {
        eprintln!("vault harness FAILED");
        ExitCode::FAILURE
    } else {
        println!("vault harness passed");
        ExitCode::SUCCESS
    }
}
