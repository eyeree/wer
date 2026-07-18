//! `wer-loom` — World Loom Stage 0A sign-off harness.

use std::process::ExitCode;

use tools::run_loom_harness;

fn main() -> ExitCode {
    println!("running World Loom Stage 0A gates...");
    let report = run_loom_harness();
    println!(
        "exhaustive={} randomized={} ordinary={}/{} adversarial={}/{}",
        report.exhaustive_cases,
        report.randomized_cases,
        report.ordinary_complete,
        report.ordinary_total,
        report.adversarial_complete,
        report.adversarial_total,
    );
    println!(
        "max normalization={:?}; max Egress={:?}",
        report.max_normalization, report.max_probe
    );
    if report.passed() {
        println!("World Loom Stage 0A native gates passed");
        ExitCode::SUCCESS
    } else {
        for violation in &report.violations {
            eprintln!("violation: {violation}");
        }
        eprintln!("World Loom Stage 0A gates FAILED");
        ExitCode::FAILURE
    }
}
