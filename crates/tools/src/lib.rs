//! `tools` — developer-facing utilities for inspecting and validating the
//! deterministic world model (section 5, `tools/`).
//!
//! Phase 2 ships three: `wer-inspect` (position → region, hashes, every
//! layer's generated samples, and the full dependency-hash chain),
//! `wer-replay` (the headless continuity replay), and `wer-ledger` (the
//! invalidation-precision harness of phase-2-plan.md §12.3).

pub mod ecology;
pub mod ledger;
pub mod replay;

pub use ecology::{run_ecology_harness, EcologyReport};
pub use ledger::{run_invalidation_ledger, ScenarioReport};
pub use replay::{run_continuity_replay, ReplayConfig, ReplayReport};

use world_core::{feature_hash, Biome, FeatureKey, RegionCoord, WORLD_ALGORITHM_VERSION};
use world_runtime::{
    Budget, CellEcology, InlineExecutor, LayerDiagnostic, RegionMap, StreamConfig, CHANNEL_CANOPY,
    CHANNEL_ELEVATION, CHANNEL_FERTILITY, CHANNEL_HARDNESS, CHANNEL_MOISTURE, CHANNEL_RIVER,
    CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};

/// Resolve a world-space position into its region and the deterministic hash of
/// that region's first feature — a minimal, scriptable determinism probe.
#[must_use]
pub fn probe_world_position(world_x: f64, world_y: f64) -> (RegionCoord, u64) {
    let region = RegionCoord::from_world(world_x, world_y);
    let key = FeatureKey {
        world_version: WORLD_ALGORITHM_VERSION,
        region,
        layer: 0,
        feature_index: 0,
        possibility_revision: 0,
    };
    (region, feature_hash(&key))
}

/// Everything `wer-inspect` reports for one world position: the region and
/// feature-hash identity, every layer's generated sample at the position's
/// cell, and the full dependency-hash chain with stale/fresh verdicts.
#[derive(Debug)]
pub struct PositionReport {
    /// The level-0 region containing the position.
    pub region: RegionCoord,
    /// Deterministic origin-feature hash of that region.
    pub feature_hash: u64,
    /// The region's realized vector (== its anchor-free target after a
    /// bias-free settle).
    pub target: world_core::PossibilityVector,
    /// Elevation at the position's cell.
    pub elevation: f32,
    /// Rock hardness at the position's cell.
    pub hardness: f32,
    /// Temperature (°C) at the position's cell.
    pub temperature: f32,
    /// Moisture at the position's cell.
    pub moisture: f32,
    /// River expression at the position's cell.
    pub river: f32,
    /// Surface wetness at the position's cell.
    pub wetness: f32,
    /// Soil depth at the position's cell.
    pub soil_depth: f32,
    /// Soil fertility at the position's cell.
    pub fertility: f32,
    /// Vegetation density at the position's cell.
    pub vegetation: f32,
    /// Canopy height at the position's cell.
    pub canopy: f32,
    /// Biome classification of the cell.
    pub biome: Biome,
    /// Per-layer dependency-hash diagnostics, in layer id order.
    pub layers: Vec<LayerDiagnostic>,
}

/// Generate the full Phase 2 stack around one position (no anchors, no bias —
/// the pure deterministic base the runtime steers from) and sample it.
///
/// Runs the real streaming pipeline over a small window with the inline
/// executor, so what it reports is exactly what the app would realize.
/// Settle a small streaming window around a position and return it with the
/// covering region and the cell `(cx, cy)` the position falls in — the shared
/// setup behind [`inspect_world_position`] and [`inspect_ecology`].
///
/// The near radius pins the center region so it fully realizes its organisms,
/// so `--species` / `--ecology` report exactly what the app would.
#[must_use]
fn settled_inspection(world_x: f64, world_y: f64) -> (RegionMap, RegionCoord, u16, u16) {
    let region = RegionCoord::from_world(world_x, world_y);
    let cfg = StreamConfig {
        near_radius: 1.0 * world_core::REGION_SIZE,
        far_radius: 2.0 * world_core::REGION_SIZE,
        load_radius: 2.0 * world_core::REGION_SIZE,
        unload_radius: 3.0 * world_core::REGION_SIZE,
        ..StreamConfig::default()
    };
    let field = world_core::PossibilityField::default();
    let bias = [0.0f32; world_core::POSSIBILITY_DIMS];
    let mut map = RegionMap::new(cfg);
    for _ in 0..4 {
        map.update(
            (world_x, world_y),
            0.0,
            &field,
            &[],
            &bias,
            &Budget::unlimited(),
            &InlineExecutor,
        );
    }
    let res = cfg.field_resolution;
    let (ox, oy) = region.origin();
    let cell = world_core::REGION_SIZE / f64::from(res);
    let cx = (((world_x - ox) / cell) as u16).min(res - 1);
    let cy = (((world_y - oy) / cell) as u16).min(res - 1);
    (map, region, cx, cy)
}

/// The aggregate ecology and full roster readout at a world position — the data
/// behind `wer-inspect --species` and `--ecology` (phase-3-plan.md §11).
/// `None` if the ecology layer has not settled for the cell.
#[must_use]
pub fn inspect_ecology(world_x: f64, world_y: f64) -> Option<CellEcology> {
    let (map, region, cx, cy) = settled_inspection(world_x, world_y);
    map.cell_ecology(region, cx, cy)
}

#[must_use]
pub fn inspect_world_position(world_x: f64, world_y: f64) -> PositionReport {
    let (_, hash) = probe_world_position(world_x, world_y);
    let (map, region, cx, cy) = settled_inspection(world_x, world_y);
    let state = map.get(region).expect("center region resident");
    let sample = |channel: usize| {
        map.cache()
            .channel(region, channel)
            .map_or(f32::NAN, |t| t.get(cx, cy))
    };

    PositionReport {
        region,
        feature_hash: hash,
        target: state.current,
        elevation: sample(CHANNEL_ELEVATION),
        hardness: sample(CHANNEL_HARDNESS),
        temperature: sample(CHANNEL_TEMPERATURE),
        moisture: sample(CHANNEL_MOISTURE),
        river: sample(CHANNEL_RIVER),
        wetness: sample(CHANNEL_WETNESS),
        soil_depth: sample(CHANNEL_SOIL_DEPTH),
        fertility: sample(CHANNEL_FERTILITY),
        vegetation: sample(CHANNEL_VEGETATION),
        canopy: sample(CHANNEL_CANOPY),
        biome: map
            .cache()
            .biome(region)
            .map_or(Biome::Bare, |t| Biome::from_id(t.get(cx, cy))),
        layers: map.layer_diagnostics(region).expect("resident"),
    }
}
