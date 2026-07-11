//! `wer-anchor` — the Phase 4 steering sign-off harness (phase-4-plan.md §12.3).
//!
//! Usage:
//!     wer-anchor
//!
//! Runs every scenario of the §12.3 table over settled, steered streaming
//! windows and asserts the success criterion: steering is intentional (masked
//! domains move toward a capture, monotone in strength) and selective, yet
//! surprising (projection and combination reshape naive targets) and remains
//! ecologically coherent (section-8 plausibility + the Phase 3 invariants hold
//! in every steered world), while resonance gates transition. Exits non-zero on
//! any violation, so it doubles as a CI-friendly gate.

use std::process::ExitCode;

use tools::run_anchor_harness;

fn main() -> ExitCode {
    println!("running the Phase 4 anchor harness...");
    let reports = run_anchor_harness();
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
        eprintln!("anchor harness FAILED");
        ExitCode::FAILURE
    } else {
        println!("anchor harness passed");
        ExitCode::SUCCESS
    }
}
