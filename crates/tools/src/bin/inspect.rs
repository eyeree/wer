//! `wer-inspect` — resolve a world position to its region and feature hash.
//!
//! Usage:
//!     wer-inspect <world_x> <world_y>
//!
//! Prints the deterministic region coordinate and origin-feature hash for the
//! given continuous world position. Handy as a determinism spot-check and as the
//! seed of the future validation/replay tooling.

use std::process::ExitCode;

use tools::probe_world_position;
use world_core::WORLD_ALGORITHM_VERSION;

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

    let (region, hash) = probe_world_position(x, y);
    println!("world algorithm version : {WORLD_ALGORITHM_VERSION}");
    println!("world position          : ({x}, {y})");
    println!(
        "region                  : x={} y={} level={}",
        region.x, region.y, region.level
    );
    println!("origin feature hash     : {hash:#018x}");
    ExitCode::SUCCESS
}
