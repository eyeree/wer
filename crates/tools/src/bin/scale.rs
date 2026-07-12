//! `wer-scale` — Phase 6 sign-off harness (phase-6-plan.md §10, §11.4).
//!
//! Runs the scripted stress scenarios headlessly and gates on deterministic
//! counts/bytes/hashes; wall-clock is printed for the committed baseline
//! (`docs/perf-baseline.md`) but never asserted.
//!
//! Usage: `wer-scale [--quick] [--report]`
//! - `--quick`: CI-sized scenarios (the full run is the sign-off run).
//! - `--report`: print the per-pass timing / counter table the baseline
//!   document snapshots.

use tools::scale::{print_report, run_scale_harness, ScaleConfig};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut quick = false;
    let mut report = false;
    for arg in &args {
        match arg.as_str() {
            "--quick" => quick = true,
            "--report" => report = true,
            other => {
                eprintln!("unknown argument {other:?}\nusage: wer-scale [--quick] [--report]");
                std::process::exit(2);
            }
        }
    }

    let cfg = if quick {
        ScaleConfig::quick()
    } else {
        ScaleConfig::default()
    };
    let outcome = run_scale_harness(&cfg);

    if report {
        print_report(&outcome);
    }

    let mut failed = 0usize;
    for scenario in &outcome.scenarios {
        for gate in &scenario.gates {
            let verdict = if gate.passed { "PASS" } else { "FAIL" };
            println!(
                "[{verdict}] {}: {} — {}",
                scenario.name, gate.name, gate.detail
            );
            if !gate.passed {
                failed += 1;
            }
        }
    }
    if failed > 0 {
        eprintln!("wer-scale: {failed} gate(s) failed");
        std::process::exit(1);
    }
    println!("wer-scale: all gates green");
}
