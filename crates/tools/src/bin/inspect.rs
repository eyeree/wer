//! `wer-inspect` — resolve a world position to its region, identity hash, the
//! full layer stack, and its ecology.
//!
//! Usage:
//!     wer-inspect <world_x> <world_y> [--layers] [--species] [--ecology]
//!
//! Prints the deterministic region coordinate and origin-feature hash for the
//! given continuous world position, plus every layer's generated sample and
//! the region's realized possibility vector — a scriptable determinism
//! spot-check. With `--layers`, also dumps the dependency-hash chain: each
//! layer's declared inputs, the quantized buckets it consumed, expected vs
//! stored hash, and the stale/fresh verdict (phase-2-plan.md §11) — the tool
//! that makes invalidation *legible*. With `--species`, dumps the cell's
//! habitat signature, full species roster (each species' id, genome, trophic
//! role), and food-web edges. With `--ecology`, dumps the L8 aggregate values
//! (phase-3-plan.md §11). With `--steer`, dumps the base / steered / projected
//! possibility vectors for a scripted anchor set and which domains moved — the
//! steering analogue of `--layers` (phase-4-plan.md §11).

use std::process::ExitCode;

use tools::{inspect_ecology, inspect_steer, inspect_world_position};
use world_core::layer::{layer_decl, LAYERS};
use world_core::{PossibilityDomain, WORLD_ALGORITHM_VERSION};

fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(i) = args.iter().position(|a| a == name) {
        args.remove(i);
        true
    } else {
        false
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let layers_flag = take_flag(&mut args, "--layers");
    let species_flag = take_flag(&mut args, "--species");
    let ecology_flag = take_flag(&mut args, "--ecology");
    let steer_flag = take_flag(&mut args, "--steer");
    let vault_flag = take_flag(&mut args, "--vault");
    let routes_flag = take_flag(&mut args, "--routes");
    let (x, y) = match args.as_slice() {
        [x, y] => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => {
                eprintln!("error: world_x and world_y must be numbers");
                return ExitCode::FAILURE;
            }
        },
        _ => {
            eprintln!(
                "usage: wer-inspect <world_x> <world_y> [--layers] [--species] [--ecology] \
                 [--steer] [--vault] [--routes]\n\
                 (--vault/--routes read the store at $WER_VAULT_DIR, default ./wer-vault)"
            );
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

    if layers_flag {
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

    if ecology_flag || species_flag {
        match inspect_ecology(x, y) {
            None => {
                println!();
                println!("ecology                 : (L8 not settled for this cell)");
            }
            Some(eco) => {
                if ecology_flag {
                    println!();
                    println!("aggregate ecology (L8, phase-3-plan.md §7.5):");
                    println!(
                        "  herbivore   {}",
                        eco.herbivore.map_or("-".into(), |v| format!("{v:.4}"))
                    );
                    println!(
                        "  predator    {}",
                        eco.predator.map_or("-".into(), |v| format!("{v:.4}"))
                    );
                    println!(
                        "  diversity   {}",
                        eco.diversity.map_or("-".into(), |v| format!("{v:.4}"))
                    );
                    println!(
                        "  dominant    index {} id {:#018x}",
                        eco.dominant_index, eco.dominant_id
                    );
                }
                if species_flag {
                    let sig = eco.signature;
                    println!();
                    println!(
                        "habitat signature       : biome={} temp_band={} moist_band={} fert_band={} (seed {:#018x})",
                        sig.biome().name(),
                        sig.temperature_band,
                        sig.moisture_band,
                        sig.fertility_band,
                        sig.seed(),
                    );
                    let roster = &eco.roster.roster;
                    println!("species roster ({} species):", roster.species.len());
                    for (i, sp) in roster.species.iter().enumerate() {
                        let a = &sp.genome.appearance;
                        let dominant = if i == eco.dominant_index as usize {
                            " <- dominant"
                        } else {
                            ""
                        };
                        println!(
                            "  [{i:2}] {:<10} id {:#018x}  hue {:3} size-class {} form {:2}{dominant}",
                            sp.trophic.name(),
                            sp.id,
                            a.hue,
                            a.size_class,
                            a.form,
                        );
                    }
                    let web = &eco.roster.web;
                    println!(
                        "food web ({} edges, max body size {:.2}):",
                        web.edges.len(),
                        web.max_body_size
                    );
                    for &(pred, prey) in &web.edges {
                        println!(
                            "  {} [{pred}] -> {} [{prey}]",
                            roster.species[pred as usize].trophic.name(),
                            roster.species[prey as usize].trophic.name(),
                        );
                    }
                    if !web.pruned.is_empty() {
                        println!("  pruned (unsustainable): {:?}", web.pruned);
                    }
                }
            }
        }
    }

    if steer_flag {
        let steer = inspect_steer(x, y);
        println!();
        println!("steering (phase-4-plan.md §11, scripted demo anchor set):");
        for anchor in &steer.anchors {
            let kind = match anchor.kind {
                world_core::AnchorKind::Emphasize => "emphasize",
                world_core::AnchorKind::Suppress => "suppress",
            };
            let domains: Vec<&str> = PossibilityDomain::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| anchor.mask & (1 << i) != 0)
                .map(|(_, d)| domain_short(*d))
                .collect();
            println!(
                "  {kind:<9} mask [{}] strength {:.2} radius {:.0}",
                domains.join(", "),
                anchor.strength,
                anchor.falloff_radius,
            );
        }
        println!(
            "  {:<12} {:>9} {:>9} {:>9}",
            "domain", "base", "steered", "projected"
        );
        for domain in PossibilityDomain::ALL {
            let b = steer.base.get(domain);
            let s = steer.steered.get(domain);
            let p = steer.projected.get(domain);
            let moved = if (b - p).abs() > 1e-4 { " *" } else { "" };
            println!(
                "  {:<12} {b:>9.4} {s:>9.4} {p:>9.4}{moved}",
                format!("{domain:?}")
            );
        }
        println!("  (* = domain moved from base to projected target)");
    }

    if vault_flag || routes_flag {
        let store_dir =
            std::env::var("WER_VAULT_DIR").unwrap_or_else(|_| String::from("wer-vault"));
        if vault_flag {
            match tools::inspect_vault(&store_dir, x, y) {
                Err(e) => {
                    eprintln!("--vault: {e}");
                    return ExitCode::FAILURE;
                }
                Ok(v) => {
                    println!();
                    println!("vault {store_dir} (phase-5-plan.md §11):");
                    println!(
                        "  totals      {} discoveries, {} routes, {} preserves, {} seen",
                        v.totals.0, v.totals.1, v.totals.2, v.totals.3
                    );
                    println!(
                        "  discovered  {}",
                        if v.seen_here { "yes" } else { "not yet" }
                    );
                    match &v.covering_preserve {
                        Some((id, name, sig)) => {
                            println!("  preserve    {name} ({id:#018x}) pins this region");
                            println!("              buckets {:?}", sig.buckets);
                        }
                        None => println!("  preserve    none covers this region"),
                    }
                    for (id, name, d) in &v.nearby_discoveries {
                        println!("  discovery   {name} ({id:#018x}) {d:.0} units away");
                    }
                    for (route, node, d) in &v.nearby_route_nodes {
                        println!(
                            "  route node  {route:#018x}[{node}] {d:.0} units away (in corridor)"
                        );
                    }
                    for issue in &v.issues {
                        println!("  issue       {issue}");
                    }
                }
            }
        }
        if routes_flag {
            match tools::inspect_routes(&store_dir, x, y) {
                Err(e) => {
                    eprintln!("--routes: {e}");
                    return ExitCode::FAILURE;
                }
                Ok(r) => {
                    println!();
                    println!("route graph query (possibility space, phase-5-plan.md §11):");
                    println!("  here        buckets {:?}", r.signature.buckets);
                    if r.hits.is_empty() {
                        println!("  (no recorded routes in the store)");
                    }
                    for (hit, name, difficulty) in &r.hits {
                        println!(
                            "  {name} ({:#018x})[{}]  possibility distance {}  difficulty {difficulty:.2}",
                            hit.route, hit.node, hit.distance
                        );
                    }
                }
            }
        }
    }

    ExitCode::SUCCESS
}

/// Four-letter domain tags for the compact steering mask listing.
fn domain_short(domain: PossibilityDomain) -> &'static str {
    match domain {
        PossibilityDomain::Planetary => "plan",
        PossibilityDomain::Climate => "clim",
        PossibilityDomain::Geology => "geol",
        PossibilityDomain::Hydrology => "hydr",
        PossibilityDomain::Ecology => "ecol",
        PossibilityDomain::Morphology => "morp",
        PossibilityDomain::Behavior => "behv",
        PossibilityDomain::Aesthetics => "aest",
    }
}
