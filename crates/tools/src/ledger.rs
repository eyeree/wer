//! The invalidation ledger: the machine check of the Phase 2 success
//! criterion (phase-2-plan.md §12.3).
//!
//! Each scenario settles a streaming window, applies one scripted change
//! (a possibility bias or a layer revision bump), lets the runtime settle
//! again, and diffs every `(region, layer)` stored dependency hash. A tile
//! regenerated iff its stored hash changed — regeneration only happens on
//! hash mismatch and always lands a new hash — so the diff *is* the exact
//! regeneration set. The expected set is predicted independently from each
//! region's observed quantized-bucket flips through the declared graph
//! ([`world_core::layer::domain_dirty_mask`]): the scenario passes iff
//! `actual == predicted` for every region.
//!
//! Drainage is excluded from per-region predictions: macro routing consumes
//! no runtime possibility state (ADR 0009), so no drift scenario may ever
//! regenerate it — asserted separately via macro-tile content hashes.

use std::collections::BTreeMap;

use world_core::layer::{
    domain_dirty_mask, layer_bit, layer_decl, LAYER_COUNT, LAYER_DRAINAGE, LAYER_SOILS,
};
use world_core::{PossibilityDomain, PossibilityField, RegionCoord, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{Budget, InlineExecutor, RegionMap, StreamConfig};

/// Outcome of one scenario.
#[derive(Debug)]
pub struct ScenarioReport {
    /// Scenario name (matches the §12.3 table).
    pub name: &'static str,
    /// Mismatches found (capped; empty means the scenario passed).
    pub violations: Vec<String>,
    /// Total `(region, layer)` regenerations observed.
    pub regenerated: usize,
    /// Regions whose buckets flipped at least once.
    pub regions_flipped: usize,
}

impl ScenarioReport {
    /// Whether the scenario asserted its exact regeneration set.
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

fn ledger_config() -> StreamConfig {
    StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 6.0 * REGION_SIZE,
        unload_radius: 7.5 * REGION_SIZE,
        converge_per_unit: 0.01,
        converge_rate_cap: 0.25,
        field_resolution: 8,
    }
}

const PLAYER: (f64, f64) = (128.0, 128.0);

fn update(map: &mut RegionMap, travel: f64, bias: &[f32; POSSIBILITY_DIMS], budget: &Budget) {
    let field = PossibilityField::default();
    map.update(PLAYER, travel, &field, &[], bias, budget, &InlineExecutor);
}

/// A settled window: fully loaded and generated, nothing in flight.
fn settled_map() -> RegionMap {
    let mut map = RegionMap::new(ledger_config());
    let bias = [0.0f32; POSSIBILITY_DIMS];
    for _ in 0..4 {
        update(&mut map, 0.0, &bias, &Budget::unlimited());
    }
    assert_eq!(map.jobs_in_flight(), 0, "ledger window failed to settle");
    map
}

/// Stored dependency hash of every generated `(region, layer)` tile.
fn snapshot_hashes(map: &RegionMap) -> BTreeMap<(RegionCoord, u16), u64> {
    let mut snap = BTreeMap::new();
    for (&coord, tiles) in map.cache().iter() {
        for layer in 0..LAYER_COUNT {
            if layer == LAYER_DRAINAGE {
                continue; // tracked at macro level
            }
            if let Some(hash) = tiles.layer_hash(layer) {
                snap.insert((coord, layer), hash);
            }
        }
    }
    snap
}

/// Quantized buckets of every resident region, all domains.
fn snapshot_buckets(map: &RegionMap) -> BTreeMap<RegionCoord, [u16; POSSIBILITY_DIMS]> {
    map.iter_active()
        .map(|r| {
            let mut buckets = [0u16; POSSIBILITY_DIMS];
            for (i, domain) in PossibilityDomain::ALL.iter().enumerate() {
                buckets[i] = r.current.quantized(*domain);
            }
            (r.coord, buckets)
        })
        .collect()
}

/// Macro-tile content hashes (drainage must never regenerate under drift).
fn snapshot_macro(map: &RegionMap) -> BTreeMap<RegionCoord, u64> {
    map.macro_cache()
        .iter()
        .map(|(&c, t)| (c, t.content_hash()))
        .collect()
}

/// Run one drift scenario: settle, apply `bias`, converge to quiescence, then
/// assert the regeneration set equals the per-region prediction from observed
/// bucket flips.
///
/// `expect_flips` scenarios must flip buckets somewhere (else the bias was too
/// small to test anything); `!expect_flips` scenarios must instead produce at
/// least one region whose realized state *moved without flipping a bucket* —
/// the direct machine check that sub-bucket drift costs zero regeneration
/// (phase-2-plan.md §4.2). Regions that happen to sit within the bias of a
/// bucket boundary may still flip; the prediction equality absorbs them.
fn drift_scenario(
    name: &'static str,
    bias: [f32; POSSIBILITY_DIMS],
    expect_flips: bool,
    allowed_layers: Option<u32>,
) -> ScenarioReport {
    let mut map = settled_map();
    let hashes_before = snapshot_hashes(&map);
    let buckets_before = snapshot_buckets(&map);
    let currents_before: BTreeMap<RegionCoord, world_core::PossibilityVector> =
        map.iter_active().map(|r| (r.coord, r.current)).collect();
    let macro_before = snapshot_macro(&map);
    let pinned_before: BTreeMap<RegionCoord, bool> = map
        .iter_active()
        .map(|r| (r.coord, r.stability >= 1.0))
        .collect();

    // Converge to quiescence: the lerp is asymptotic, so after ~120 capped
    // steps the residual gap is far below one bucket and no further flips can
    // occur. Travel without displacement keeps the resident set fixed.
    for _ in 0..150 {
        update(&mut map, 25.0, &bias, &Budget::unlimited());
    }
    assert_eq!(map.jobs_in_flight(), 0);

    let hashes_after = snapshot_hashes(&map);
    let buckets_after = snapshot_buckets(&map);
    let mut violations = Vec::new();

    // Drainage: never regenerated by drift (ADR 0009).
    if snapshot_macro(&map) != macro_before {
        record(
            &mut violations,
            "macro drainage tile regenerated under possibility drift".into(),
        );
    }

    // Per-region: actual regen set == prediction from observed bucket flips.
    let mut regenerated = 0usize;
    let mut regions_flipped = 0usize;
    let mut regions_moved_sub_bucket = 0usize;
    for (&coord, before) in &buckets_before {
        let after = buckets_after
            .get(&coord)
            .expect("resident set fixed during scenario");
        let mut flipped = 0u8;
        for i in 0..POSSIBILITY_DIMS {
            if before[i] != after[i] {
                flipped |= 1 << i;
            }
        }
        if flipped != 0 {
            regions_flipped += 1;
        } else if map
            .get(coord)
            .is_some_and(|r| r.current != currents_before[&coord])
        {
            regions_moved_sub_bucket += 1;
        }
        if pinned_before[&coord] && flipped != 0 {
            record(
                &mut violations,
                format!("pinned region ({}, {}) flipped buckets", coord.x, coord.y),
            );
        }
        let predicted = domain_dirty_mask(flipped) & !layer_bit(LAYER_DRAINAGE);
        for layer in 0..LAYER_COUNT {
            if layer == LAYER_DRAINAGE {
                continue;
            }
            let changed = hashes_before.get(&(coord, layer)) != hashes_after.get(&(coord, layer));
            let expected = predicted & layer_bit(layer) != 0;
            if changed {
                regenerated += 1;
            }
            if changed != expected {
                record(
                    &mut violations,
                    format!(
                        "region ({}, {}) layer {} ({}): regenerated={changed}, predicted={expected} (flipped domains {flipped:#010b})",
                        coord.x, coord.y, layer, layer_decl(layer).name
                    ),
                );
            }
        }
    }

    if expect_flips && regions_flipped == 0 {
        record(
            &mut violations,
            "scenario expected bucket flips but none occurred (bias too small?)".into(),
        );
    }
    if !expect_flips && regions_moved_sub_bucket == 0 {
        record(
            &mut violations,
            "sub-bucket scenario produced no sub-bucket movement to test".into(),
        );
    }
    if let Some(allowed) = allowed_layers {
        // A scenario-level restatement of the table row ("Vegetation only"),
        // over and above the per-region prediction equality.
        for (&(coord, layer), _) in hashes_after
            .iter()
            .filter(|(k, v)| hashes_before.get(*k) != Some(*v))
        {
            if allowed & layer_bit(layer) == 0 {
                record(
                    &mut violations,
                    format!(
                        "layer {} ({}) of ({}, {}) regenerated outside the scenario's allowed set",
                        layer,
                        layer_decl(layer).name,
                        coord.x,
                        coord.y
                    ),
                );
            }
        }
    }

    ScenarioReport {
        name,
        violations,
        regenerated,
        regions_flipped,
    }
}

/// The §12.3 "revision bump" scenario: a soils algorithm revision must
/// regenerate soils, biome, and vegetation everywhere — nothing else.
fn revision_bump_scenario() -> ScenarioReport {
    let mut map = settled_map();
    let hashes_before = snapshot_hashes(&map);
    let macro_before = snapshot_macro(&map);
    map.bump_layer_revision(LAYER_SOILS);
    let bias = [0.0f32; POSSIBILITY_DIMS];
    for _ in 0..4 {
        update(&mut map, 0.0, &bias, &Budget::unlimited());
    }
    let hashes_after = snapshot_hashes(&map);
    let mut violations = Vec::new();
    if snapshot_macro(&map) != macro_before {
        record(
            &mut violations,
            "macro drainage regenerated on a soils revision bump".into(),
        );
    }
    let expected_mask = world_core::dependents_closure(LAYER_SOILS);
    let mut regenerated = 0usize;
    for (&(coord, layer), before) in &hashes_before {
        let changed = hashes_after.get(&(coord, layer)) != Some(before);
        let expected = expected_mask & layer_bit(layer) != 0;
        if changed {
            regenerated += 1;
        }
        if changed != expected {
            record(
                &mut violations,
                format!(
                    "region ({}, {}) layer {} ({}): regenerated={changed}, predicted={expected} after soils bump",
                    coord.x, coord.y, layer, layer_decl(layer).name
                ),
            );
        }
    }
    ScenarioReport {
        name: "soils algorithm_revision bump -> soils, biome, vegetation only",
        violations,
        regenerated,
        regions_flipped: 0,
    }
}

/// The §12.3 budget test: a world-scale change with a small cost budget must
/// ripple over many frames, spending at most the budget every frame, and
/// still settle completely.
fn budget_ripple_scenario() -> ScenarioReport {
    let mut map = settled_map();
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Climate.index()] = -0.4;
    let budget = Budget {
        max_loads: usize::MAX,
        max_converge_regions: usize::MAX,
        max_regen_cost: 24,
        max_realize_organisms: usize::MAX,
    };
    let mut violations = Vec::new();
    let mut frames_with_regen = 0u32;
    let mut regenerated = 0usize;
    for _ in 0..600 {
        let field = PossibilityField::default();
        let stats = map.update(PLAYER, 25.0, &field, &[], &bias, &budget, &InlineExecutor);
        if stats.regen_cost_spent > budget.max_regen_cost {
            record(
                &mut violations,
                format!(
                    "frame spent {} cost units (> {})",
                    stats.regen_cost_spent, budget.max_regen_cost
                ),
            );
        }
        if stats.regen_cost_spent > 0 {
            frames_with_regen += 1;
        }
        regenerated += stats.layers_regenerated;
    }
    if frames_with_regen < 10 {
        record(
            &mut violations,
            format!(
                "a world-scale change should ripple over many frames (saw {frames_with_regen})"
            ),
        );
    }
    // Everything must still settle: nothing in flight, nothing deferred.
    let field = PossibilityField::default();
    let stats = map.update(
        PLAYER,
        0.0,
        &field,
        &[],
        &bias,
        &Budget::unlimited(),
        &InlineExecutor,
    );
    if map.jobs_in_flight() != 0 || stats.layers_dispatched > 0 {
        record(
            &mut violations,
            "window failed to settle after the budgeted ripple".into(),
        );
    }
    ScenarioReport {
        name: "budgeted world-scale climate change ripples without overspend",
        violations,
        regenerated,
        regions_flipped: 0,
    }
}

/// Run the full §12.3 scenario table.
#[must_use]
pub fn run_invalidation_ledger() -> Vec<ScenarioReport> {
    let mut bias;
    let mut reports = Vec::new();

    // Aesthetics/Morphology/Behavior: buckets flip; L8 is their only reader, so
    // exactly ecology regenerates (its aggregate fields are Ecology-driven, but
    // its dependency hash folds M/B/A because near-field realization expresses
    // genomes under them — phase-3-plan.md §7.5).
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Aesthetics.index()] = 0.4;
    bias[PossibilityDomain::Morphology.index()] = 0.4;
    bias[PossibilityDomain::Behavior.index()] = -0.4;
    reports.push(drift_scenario(
        "aesthetics/morphology/behavior bias -> ecology (L8) only",
        bias,
        true,
        Some(layer_bit(world_core::layer::LAYER_ECOLOGY)),
    ));

    // Ecology: vegetation and L8 (Ecology has driven vegetation density since
    // Phase 2; L8 is the new reader downstream of it).
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Ecology.index()] = 0.3;
    reports.push(drift_scenario(
        "ecology bucket flip -> vegetation + ecology (L8)",
        bias,
        true,
        Some(
            layer_bit(world_core::layer::LAYER_VEGETATION)
                | layer_bit(world_core::layer::LAYER_ECOLOGY),
        ),
    ));

    // Climate: climate and everything downstream (now including L8); never the
    // stable trio.
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Climate.index()] = -0.3;
    reports.push(drift_scenario(
        "climate bucket flip -> climate..ecology, stable trio untouched",
        bias,
        true,
        Some(
            layer_bit(world_core::layer::LAYER_CLIMATE)
                | layer_bit(world_core::layer::LAYER_HYDROLOGY)
                | layer_bit(world_core::layer::LAYER_SOILS)
                | layer_bit(world_core::layer::LAYER_BIOME)
                | layer_bit(world_core::layer::LAYER_VEGETATION)
                | layer_bit(world_core::layer::LAYER_ECOLOGY),
        ),
    ));

    // Hydrology: climate reads H too, so climate and downstream regenerate;
    // the plausibility projection may also move Ecology targets (its cap
    // depends on Hydrology), which the per-region prediction absorbs.
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Hydrology.index()] = 0.3;
    reports.push(drift_scenario(
        "hydrology bucket flip -> climate (reads H) and downstream",
        bias,
        true,
        None,
    ));

    // Geology (slow): the full pyramid except drainage (ADR 0009) — and only
    // in unpinned regions, which the pinned-flip assertion checks.
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Geology.index()] = 0.25;
    reports.push(drift_scenario(
        "geology (slow) bucket flip -> full pyramid, unpinned regions only",
        bias,
        true,
        None,
    ));

    // Sub-bucket drift: far below one bucket (1/4096). Regions whose realized
    // state moves without a flip must regenerate nothing (the rare region
    // sitting within 1e-5 of a bucket edge legitimately flips and is absorbed
    // by the prediction equality).
    bias = [0.0f32; POSSIBILITY_DIMS];
    bias[PossibilityDomain::Climate.index()] = 1.0e-5;
    reports.push(drift_scenario(
        "sub-bucket drift -> nothing (where no bucket flips)",
        bias,
        false,
        None,
    ));

    reports.push(revision_bump_scenario());
    reports.push(budget_ripple_scenario());
    reports
}
