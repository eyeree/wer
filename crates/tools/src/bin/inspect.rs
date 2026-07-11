//! `wer-inspect` — resolve a world position to its region, identity hash, and
//! the full Phase 2 layer stack.
//!
//! Usage:
//!     wer-inspect <world_x> <world_y> [--layers]
//!
//! Prints the deterministic region coordinate and origin-feature hash for the
//! given continuous world position, plus every layer's generated sample and
//! the region's realized possibility vector — a scriptable determinism
//! spot-check. With `--layers`, also dumps the dependency-hash chain: each
//! layer's declared inputs, the quantized buckets it consumed, expected vs
//! stored hash, and the stale/fresh verdict (phase-2-plan.md §11) — the tool
//! that makes invalidation *legible*.

use std::process::ExitCode;

use tools::inspect_world_position;
use world_core::layer::{layer_decl, LAYERS};
use world_core::{PossibilityDomain, WORLD_ALGORITHM_VERSION};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let layers_flag = args.iter().position(|a| a == "--layers");
    if let Some(i) = layers_flag {
        args.remove(i);
    }
    let (x, y) = match args.as_slice() {
        [x, y] => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => {
                eprintln!("error: world_x and world_y must be numbers");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!("usage: wer-inspect <world_x> <world_y> [--layers]");
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
    println!("rock hardness           : {:.3}", report.hardness);
    println!(
        "climate                 : {:.2} °C, moisture {:.3}",
        report.temperature, report.moisture
    );
    println!(
        "hydrology               : river {:.3}, wetness {:.3}",
        report.river, report.wetness
    );
    println!(
        "soils                   : depth {:.3}, fertility {:.3}",
        report.soil_depth, report.fertility
    );
    println!(
        "vegetation              : density {:.3}, canopy {:.1}",
        report.vegetation, report.canopy
    );
    println!("biome                   : {}", report.biome.name());
    println!("region realized vector  :");
    for domain in PossibilityDomain::ALL {
        println!(
            "  {:<12} {:.4}",
            format!("{domain:?}"),
            report.target.get(domain)
        );
    }

    if layers_flag.is_some() {
        println!();
        println!("dependency-hash chain (phase-2-plan.md §4.3):");
        for diag in &report.layers {
            let decl = layer_decl(diag.layer);
            let deps: Vec<&str> = decl.deps.iter().map(|&d| LAYERS[d as usize].name).collect();
            println!(
                "  [{}] {:<10} rev {}  deps [{}]",
                diag.layer,
                decl.name,
                decl.algorithm_revision,
                deps.join(", ")
            );
            let domains: Vec<String> = PossibilityDomain::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| decl.domains & (1 << i) != 0)
                .map(|(_, d)| format!("{d:?}"))
                .collect();
            println!(
                "      domains [{}]  buckets {:?}",
                domains.join(", "),
                diag.buckets
            );
            println!(
                "      expected {}  stored {}",
                diag.expected
                    .map_or("(inputs not ready)".into(), |h| format!("{h:#018x}")),
                diag.stored
                    .map_or("(none)".into(), |h| format!("{h:#018x}")),
            );
            println!(
                "      verdict: {}{}{}",
                if diag.is_stale() { "STALE" } else { "fresh" },
                if diag.dirty { ", dirty-hint set" } else { "" },
                if diag.in_flight {
                    ", job in flight"
                } else {
                    ""
                },
            );
        }
    }
    ExitCode::SUCCESS
}
