//! `wer-loom-0b` — pre-visualization World Loom Stage 0B sign-off.

use tools::run_loom_0b_harness;

fn main() {
    println!("running World Loom Stage 0B pre-visualization gates...");
    let report = run_loom_0b_harness();
    println!(
        "ordinary complete: {}/{}",
        report.ordinary_complete, report.ordinary_total
    );
    println!("randomized cases: {}", report.randomized_cases);
    println!(
        "adversarial checks: {}/{}",
        report.adversarial_passed, report.adversarial_total
    );
    println!("maximum ordinary case: {:?}", report.max_case);
    if report.passed() {
        println!("ReadyForVisualization: pre-visualization gates passed; Stage 0B remains open");
    } else {
        for violation in &report.violations {
            eprintln!("- {violation}");
        }
        eprintln!("NotReadyForVisualization");
        std::process::exit(1);
    }
}
