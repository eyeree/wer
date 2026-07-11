//! `wer-inspect` — resolve a world position to its region, identity hash, and
//! Phase 1 generated samples.
//!
//! Usage:
//!     wer-inspect <world_x> <world_y>
//!
//! Prints the deterministic region coordinate and origin-feature hash for the
//! given continuous world position, plus the terrain/climate/ecology samples
//! and the region's anchor-free target vector (phase-1-plan.md section 4.4) —
//! a scriptable determinism spot-check.

use std::process::ExitCode;

use tools::inspect_world_position;
use world_core::{PossibilityDomain, WORLD_ALGORITHM_VERSION};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (x, y) = match args.as_slice() {
        [x, y] => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => {
                eprintln!("error: world_x and world_y must be numbers");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: wer-inspect <world_x> <world_y>");
            return ExitCode::FAILURE;
        }
    };

    let report = inspect_world_position(x, y);
    println!("world algorithm version : {WORLD_ALGORITHM_VERSION}");
    println!("world position          : ({x}, {y})");
    println!(
        "region                  : x={} y={} level={}",
        report.region.x, report.region.y, report.region.level
    );
    println!("origin feature hash     : {:#018x}", report.feature_hash);
    println!(
        "elevation               : {:.2} ({})",
        report.elevation,
        if world_core::is_water(report.elevation) {
            "water"
        } else {
            "land"
        }
    );
    println!(
        "climate                 : {:.2} °C, moisture {:.3}",
        report.climate.temperature, report.climate.moisture
    );
    println!("vegetation density      : {:.3}", report.vegetation);
    println!("region target vector    :");
    for domain in PossibilityDomain::ALL {
        println!(
            "  {:<12} {:.4}",
            format!("{domain:?}"),
            report.target.get(domain)
        );
    }
    ExitCode::SUCCESS
}
