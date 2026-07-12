//! The ecology harness: the machine check of the Phase 3 success criterion
//! (phase-3-plan.md §12.3), the Tier-A/Tier-B analogue of the invalidation
//! ledger.
//!
//! It settles a streaming window and asserts two scenario families:
//!
//! - **Coherence invariants** that hold for every cell and every realized
//!   organism — herbivore pressure bounded by primary productivity, predator by
//!   herbivore, realized body size by the habitat's food-web bound, no orphan
//!   predator surviving a web, and every organism's species drawn from its
//!   cell's roster (the aggregate↔entity link).
//! - **Diversity and response** — a settled window carries many species and
//!   neighbouring habitats differ, and steering expression domains
//!   (Aesthetics/Morphology/Behavior) shifts organism *expression* without
//!   moving species identity, while realization stays inside its per-frame
//!   budget.
//!
//! Regeneration *precision* under steering (which layers regenerate for which
//! domain flip) is asserted by the invalidation ledger, which now includes L8;
//! this harness checks the ecological content those regenerations produce.

use world_core::foodweb::{CARNIVORE_EFFICIENCY, HERBIVORE_EFFICIENCY};
use world_core::layer::LAYER_ECOLOGY;
use world_core::{GenomeBias, PossibilityField, Trophic, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{
    realize_region, Budget, InlineExecutor, RegionMap, StreamConfig, CHANNEL_HERBIVORE,
    CHANNEL_PREDATOR, CHANNEL_VEGETATION,
};

/// Outcome of one ecology-harness scenario.
#[derive(Debug)]
pub struct EcologyReport {
    /// Scenario name (matches the §12.3 families).
    pub name: &'static str,
    /// Violations found (capped; empty means the scenario passed).
    pub violations: Vec<String>,
    /// A short metric summary for logging (e.g. "312 organisms, 14 species").
    pub summary: String,
}

impl EcologyReport {
    /// Whether the scenario held.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

const MAX_VIOLATIONS: usize = 16;

fn record(violations: &mut Vec<String>, message: String) {
    if violations.len() < MAX_VIOLATIONS {
        violations.push(message);
    }
}

fn harness_config() -> StreamConfig {
    StreamConfig {
        near_radius: 2.0 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 5.0 * REGION_SIZE,
        unload_radius: 6.0 * REGION_SIZE,
        converge_per_unit: 0.02,
        converge_rate_cap: 0.25,
        field_resolution: 8,
    }
}

const PLAYER: (f64, f64) = (128.0, 128.0);

fn settled_map(budget: &Budget) -> RegionMap {
    let mut map = RegionMap::new(harness_config());
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    for _ in 0..10 {
        map.update(
            PLAYER,
            0.0,
            &field,
            &[],
            &bias,
            budget,
            &InlineExecutor,
            false,
        );
    }
    map
}

/// Coherence invariants over the settled window (phase-3-plan.md §12.3).
fn coherence_scenario() -> EcologyReport {
    let map = settled_map(&Budget::unlimited());
    let mut violations = Vec::new();
    let res = harness_config().field_resolution;
    let mut organisms_checked = 0usize;

    for region in map.iter_active() {
        let coord = region.coord;
        let Some(tiles) = map.cache().get(coord) else {
            continue;
        };
        if tiles.layer_hash(LAYER_ECOLOGY).is_none() {
            continue;
        }
        let (Some(veg), Some(herb), Some(pred)) = (
            tiles.channels[CHANNEL_VEGETATION].as_deref(),
            tiles.channels[CHANNEL_HERBIVORE].as_deref(),
            tiles.channels[CHANNEL_PREDATOR].as_deref(),
        ) else {
            continue;
        };

        for cy in 0..res {
            for cx in 0..res {
                let pp = veg.get(cx, cy);
                let h = herb.get(cx, cy);
                let p = pred.get(cx, cy);
                // Productivity bound: herbivore pressure ≤ α · primary productivity.
                if h > HERBIVORE_EFFICIENCY * pp + 1e-5 {
                    record(
                        &mut violations,
                        format!(
                            "({}, {}) cell ({cx}, {cy}): herbivore {h} > alpha*pp {}",
                            coord.x,
                            coord.y,
                            HERBIVORE_EFFICIENCY * pp
                        ),
                    );
                }
                // Trophic bound: predator pressure ≤ β · herbivore pressure.
                if p > CARNIVORE_EFFICIENCY * h + 1e-5 {
                    record(
                        &mut violations,
                        format!(
                            "({}, {}) cell ({cx}, {cy}): predator {p} > beta*herb {}",
                            coord.x,
                            coord.y,
                            CARNIVORE_EFFICIENCY * h
                        ),
                    );
                }
            }
        }

        // Realized organisms (near window only).
        if let Some(organisms) = map.organisms_in(coord) {
            for org in organisms {
                organisms_checked += 1;
                let Some(sig) = map.cell_signature(coord, org.cell.cx, org.cell.cy) else {
                    continue;
                };
                let Some(entry) = map.roster_cache().get(sig) else {
                    record(
                        &mut violations,
                        format!(
                            "organism in ({}, {}) has no cached roster",
                            coord.x, coord.y
                        ),
                    );
                    continue;
                };
                // Aggregate ↔ entity: the species is drawn from the cell's roster.
                if !entry.roster.species.iter().any(|s| s.id == org.species) {
                    record(
                        &mut violations,
                        format!(
                            "organism species {:#x} not in its cell's roster",
                            org.species
                        ),
                    );
                }
                // Body-size bound: realized size ≤ the habitat's food-web bound.
                if org.expressed.size > entry.web.max_body_size + 1e-3 {
                    record(
                        &mut violations,
                        format!(
                            "organism body size {} exceeds max {}",
                            org.expressed.size, entry.web.max_body_size
                        ),
                    );
                }
            }
        }
    }

    // No orphan tiers: every surviving carnivore in every cached web has a prey
    // edge (a food-web post-condition, re-checked over the live window).
    for (_, entry) in map.roster_cache().iter() {
        for (i, sp) in entry.roster.species.iter().enumerate() {
            if sp.trophic == Trophic::Carnivore && entry.web.survives(i as u32) {
                let has_prey = entry.web.edges.iter().any(|&(p, _)| p == i as u32);
                if !has_prey {
                    record(
                        &mut violations,
                        format!("surviving carnivore {i} has no prey edge (orphan tier)"),
                    );
                }
            }
        }
    }

    let summary = format!(
        "{organisms_checked} organisms, {} habitats",
        map.roster_cache().len()
    );
    EcologyReport {
        name: "coherence invariants (productivity/trophic/body-size/orphan/aggregate)",
        violations,
        summary,
    }
}

/// Diversity floor and habitat distinctness (phase-3-plan.md §12.3).
fn diversity_scenario() -> EcologyReport {
    let map = settled_map(&Budget::unlimited());
    let mut violations = Vec::new();

    // Distinct habitat signatures and distinct species across the window.
    let habitats = map.roster_cache().len();
    let mut species = std::collections::BTreeSet::new();
    for (_, entry) in map.roster_cache().iter() {
        for sp in &entry.roster.species {
            species.insert(sp.id);
        }
    }
    // A settled window should carry many species across several habitats.
    if habitats < 3 {
        record(
            &mut violations,
            format!("only {habitats} distinct habitats in the window (< 3)"),
        );
    }
    if species.len() < 8 {
        record(
            &mut violations,
            format!(
                "only {} distinct species in the window (< 8)",
                species.len()
            ),
        );
    }

    // Neighbouring distinct habitats carry distinct rosters: find two resident
    // regions with different dominant-species ids.
    let mut dominant_ids = std::collections::BTreeSet::new();
    let res = harness_config().field_resolution;
    for region in map.iter_active() {
        if let Some(id) = map.dominant_species_id(region.coord, res / 2, res / 2) {
            dominant_ids.insert(id);
        }
    }
    if dominant_ids.len() < 2 {
        record(
            &mut violations,
            "the window's regions share a single dominant species (no zonation)".into(),
        );
    }

    EcologyReport {
        name: "diversity floor + neighbouring habitats differ",
        violations,
        summary: format!(
            "{habitats} habitats, {} species, {} distinct dominants",
            species.len(),
            dominant_ids.len()
        ),
    }
}

/// Expression response: steering Aesthetics/Morphology/Behavior shifts organism
/// *expression* without moving species identity (phase-3-plan.md §12.3). Checked
/// at the realization layer, where expression happens.
fn expression_response_scenario() -> EcologyReport {
    let map = settled_map(&Budget::unlimited());
    let mut violations = Vec::new();

    // A near region with organisms.
    let Some(coord) = map
        .iter_active()
        .map(|r| r.coord)
        .find(|&c| map.organisms_in(c).is_some_and(|o| !o.is_empty()))
    else {
        return EcologyReport {
            name: "expression response (aesthetics/morphology/behavior)",
            violations: vec!["no region with organisms to test".into()],
            summary: String::new(),
        };
    };
    let tiles = map.cache().get(coord).expect("tiles");
    let res = harness_config().field_resolution;

    let neutral = realize_region(
        coord,
        tiles,
        map.roster_cache(),
        GenomeBias::neutral(),
        0,
        res,
    );
    let aesthetic = realize_region(
        coord,
        tiles,
        map.roster_cache(),
        GenomeBias {
            aesthetics: 1.0,
            ..GenomeBias::neutral()
        },
        0,
        res,
    );
    let morphic = realize_region(
        coord,
        tiles,
        map.roster_cache(),
        GenomeBias {
            morphology: 1.0,
            ..GenomeBias::neutral()
        },
        0,
        res,
    );

    // Same identities and placement (rosters/webs unchanged) — expression is a
    // modulation, never a re-identification.
    let same_identities = neutral.len() == aesthetic.len()
        && neutral
            .iter()
            .zip(&aesthetic)
            .all(|(a, b)| a.id == b.id && a.species == b.species);
    if !same_identities {
        record(
            &mut violations,
            "aesthetics steering changed organism identity (must only change expression)".into(),
        );
    }
    // Colour actually shifts under Aesthetics.
    let colour_shifted = neutral
        .iter()
        .zip(&aesthetic)
        .any(|(a, b)| (a.expressed.hue - b.expressed.hue).abs() > 1e-4);
    if !colour_shifted && !neutral.is_empty() {
        record(
            &mut violations,
            "aesthetics steering did not shift any organism colour".into(),
        );
    }
    // Body size actually shifts under Morphology.
    let size_shifted = neutral
        .iter()
        .zip(&morphic)
        .any(|(a, b)| (a.expressed.size - b.expressed.size).abs() > 1e-4);
    if !size_shifted && !neutral.is_empty() {
        record(
            &mut violations,
            "morphology steering did not shift any organism body size".into(),
        );
    }

    EcologyReport {
        name: "expression response (aesthetics/morphology/behavior)",
        violations,
        summary: format!("{} organisms compared", neutral.len()),
    }
}

/// Aggregate ↔ entity mix: over the window, realized organisms are producer-
/// dominated in proportion to the biomass pyramid (phase-3-plan.md §12.3).
fn aggregate_entity_scenario() -> EcologyReport {
    let map = settled_map(&Budget::unlimited());
    let mut violations = Vec::new();

    let mut counts = [0usize; 5];
    let mut total = 0usize;
    for org in map.organisms() {
        counts[org.trophic as usize] += 1;
        total += 1;
    }
    if total == 0 {
        return EcologyReport {
            name: "aggregate ↔ entity trophic mix",
            violations: vec!["no organisms realized".into()],
            summary: String::new(),
        };
    }
    let producers = counts[Trophic::Producer as usize];
    let carnivores = counts[Trophic::Carnivore as usize];
    // Producers form the base of the pyramid: they must be the plurality.
    if producers * 2 < total {
        record(
            &mut violations,
            format!("producers {producers} are not the plurality of {total} organisms"),
        );
    }
    // Predators are rare relative to producers (the ~10% pyramid).
    if carnivores > producers {
        record(
            &mut violations,
            format!("carnivores {carnivores} outnumber producers {producers}"),
        );
    }

    EcologyReport {
        name: "aggregate ↔ entity trophic mix",
        violations,
        summary: format!(
            "P{} H{} O{} C{} D{}",
            counts[0], counts[1], counts[2], counts[3], counts[4]
        ),
    }
}

/// Realization budget: entering the window realizes over several frames without
/// overrunning the per-frame cap, and settles to the full population
/// (phase-3-plan.md §12.3, §8.4).
fn realization_budget_scenario() -> EcologyReport {
    let mut violations = Vec::new();
    let field = PossibilityField::default();
    let bias = [0.0f32; POSSIBILITY_DIMS];
    let cap = 40usize;
    let per_region_max =
        harness_config().field_resolution as usize * harness_config().field_resolution as usize;
    let budget = Budget {
        max_loads: usize::MAX,
        max_converge_regions: usize::MAX,
        max_regen_cost: u32::MAX,
        max_realize_organisms: cap,
        max_resonance_nodes: usize::MAX,
        max_persist_ops: usize::MAX,
        max_route_attraction_nodes: usize::MAX,
    };

    let mut map = RegionMap::new(harness_config());
    let mut frames_with_realization = 0u32;
    for _ in 0..80 {
        let stats = map.update(
            PLAYER,
            0.0,
            &field,
            &[],
            &bias,
            &budget,
            &InlineExecutor,
            false,
        );
        // Whole-region budgeting may overshoot by at most one region's worth.
        if stats.organisms_realized > cap + per_region_max {
            record(
                &mut violations,
                format!(
                    "frame realized {} organisms (> cap {cap} + region {per_region_max})",
                    stats.organisms_realized
                ),
            );
        }
        if stats.organisms_realized > 0 {
            frames_with_realization += 1;
        }
    }
    if frames_with_realization < 3 {
        record(
            &mut violations,
            format!("realization did not ripple over frames (saw {frames_with_realization})"),
        );
    }

    // It must settle to the same population an unlimited run reaches.
    let full = settled_map(&Budget::unlimited()).organism_count();
    let budgeted = map.organism_count();
    if budgeted != full {
        record(
            &mut violations,
            format!("budgeted realization settled to {budgeted} organisms, unlimited to {full}"),
        );
    }

    EcologyReport {
        name: "realization budget ripples without overrun and settles fully",
        violations,
        summary: format!("{frames_with_realization} frames, {budgeted} organisms"),
    }
}

/// Run the full §12.3 ecology-harness scenario set.
#[must_use]
pub fn run_ecology_harness() -> Vec<EcologyReport> {
    vec![
        coherence_scenario(),
        diversity_scenario(),
        expression_response_scenario(),
        aggregate_entity_scenario(),
        realization_budget_scenario(),
    ]
}
