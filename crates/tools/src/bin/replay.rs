//! `wer-replay` — run the headless continuity replay and report the result.
//!
//! Usage:
//!     wer-replay [frames]
//!
//! Drives a scripted camera path with possibility nudges and anchors through
//! the streaming runtime twice, then asserts the continuity and determinism
//! guarantees of phase-1-plan.md section 11.3. Exits non-zero on violation, so
//! it doubles as a CI-friendly regression gate.

use std::process::ExitCode;

use tools::{run_continuity_replay, ReplayConfig};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = ReplayConfig::default();
    match args.as_slice() {
        [] => {}
        [frames] => match frames.parse::<u32>() {
            Ok(f) if f > 0 => cfg.frames = f,
            _ => {
                eprintln!("error: frames must be a positive integer");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: wer-replay [frames]");
            return ExitCode::FAILURE;
        }
    }

    println!(
        "running continuity replay ({} frames, twice)...",
        cfg.frames
    );
    let a = run_continuity_replay(&cfg);
    let b = run_continuity_replay(&cfg);

    println!("frames               : {}", a.frames);
    println!("peak regions         : {}", a.peak_regions);
    println!("peak cache bytes     : {}", a.peak_cache_bytes);
    println!("final active regions : {}", a.final_stats.active_regions);
    println!("state hash (run 1)   : {:#018x}", a.state_hash);
    println!("state hash (run 2)   : {:#018x}", b.state_hash);

    let mut failed = false;
    for v in a.violations.iter().chain(&b.violations) {
        eprintln!("violation: {v}");
        failed = true;
    }
    if a.state_hash != b.state_hash {
        eprintln!("violation: two runs of the same script diverged (determinism)");
        failed = true;
    }

    if failed {
        eprintln!("continuity replay FAILED");
        ExitCode::FAILURE
    } else {
        println!("continuity replay passed");
        ExitCode::SUCCESS
    }
}
