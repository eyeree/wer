//! Headless continuity replay (phase-1-plan.md section 11.3, milestone M4).
//!
//! Drives a [`RegionMap`] along a scripted, fully deterministic camera path —
//! moving the player, ramping possibility bias, dropping and clearing anchors —
//! and machine-checks the Phase 1 success criterion without graphics:
//!
//! - **Pinned stability**: a region that is pinned (`stability == 1`) across an
//!   update never bumps its revision, and its drift-layer tiles never change —
//!   the "no near-field pop" guarantee.
//! - **Bounded per-frame delta**: no cached sample within the window moves more
//!   than a small epsilon in one frame (no snapping).
//! - **No orphan seams**: adjacent resident regions never differ in target by
//!   more than the field's per-region gradient bound.
//! - **Determinism**: two runs of the same script produce bit-identical final
//!   state (asserted by callers via [`ReplayReport::state_hash`]).
//!
//! This replay is the automated proxy for the visual success criterion and
//! guards against regressions once the prototype "looks right".

use std::collections::BTreeMap;

use world_core::{
    domain_mask, mix, Anchor, AnchorKind, PossibilityDomain, PossibilityField, RegionCoord,
    POSSIBILITY_DIMS,
};
use world_runtime::{
    Budget, FrameStats, InlineExecutor, RegionMap, StreamConfig, CHANNEL_COUNT, CHANNEL_MOISTURE,
    CHANNEL_TEMPERATURE, CHANNEL_VEGETATION,
};

/// Script + thresholds for one replay run.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Frames to simulate.
    pub frames: u32,
    /// Streaming configuration under test.
    pub stream: StreamConfig,
    /// Per-frame work budget under test.
    pub budget: Budget,
    /// Player velocity per frame, world units.
    pub velocity: (f64, f64),
    /// Max allowed per-frame change of a `[0, 1]` field sample.
    pub unit_epsilon: f32,
    /// Max allowed per-frame change of a temperature sample (°C).
    pub temperature_epsilon: f32,
    /// Max allowed per-dimension target difference between adjacent regions.
    pub seam_bound: f32,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            frames: 300,
            stream: StreamConfig {
                // A reduced window keeps the replay fast while exercising the
                // same ramp shape as the interactive app.
                near_radius: 2.0 * world_core::REGION_SIZE,
                far_radius: 6.0 * world_core::REGION_SIZE,
                load_radius: 8.0 * world_core::REGION_SIZE,
                unload_radius: 9.5 * world_core::REGION_SIZE,
                // The scripted camera moves ~43.6 units/frame, saturating the
                // rate cap — the worst-case sustained transformation speed.
                converge_per_unit: 0.01,
                converge_rate_cap: 0.2,
                field_resolution: 8,
            },
            budget: Budget {
                max_loads: 64,
                max_converge_regions: 512,
                max_regen_layers: 512,
            },
            velocity: (37.0, 23.0),
            // One converge step moves a dimension ≤ converge_rate; generation
            // maps dimensions to samples with modest slopes. Snapping shows up
            // as near-full-range jumps, far above these.
            unit_epsilon: 0.35,
            temperature_epsilon: 15.0,
            seam_bound: 0.35,
        }
    }
}

/// Outcome of a replay run.
#[derive(Debug)]
pub struct ReplayReport {
    /// Frames simulated.
    pub frames: u32,
    /// Continuity violations found (capped; empty means the run passed).
    pub violations: Vec<String>,
    /// Order-stable hash of the final region + cache state. Two runs of the
    /// same script must produce the same value (single-platform determinism).
    pub state_hash: u64,
    /// Stats from the final frame, for logging.
    pub final_stats: FrameStats,
    /// Peak resident region count observed.
    pub peak_regions: usize,
    /// Peak field-cache bytes observed.
    pub peak_cache_bytes: usize,
}

impl ReplayReport {
    /// Whether the run satisfied every continuity assertion.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

const MAX_VIOLATIONS: usize = 32;

fn record(violations: &mut Vec<String>, message: String) {
    if violations.len() < MAX_VIOLATIONS {
        violations.push(message);
    }
}

/// The scripted possibility bias at a frame: a piecewise-linear ramp that
/// pushes Ecology and Hydrology up through the middle of the run and eases
/// back — deterministic, no clocks, no randomness.
fn scripted_bias(frame: u32, frames: u32) -> [f32; POSSIBILITY_DIMS] {
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    let t = frame as f32 / frames.max(1) as f32;
    let ramp = if t < 0.2 {
        0.0
    } else if t < 0.5 {
        (t - 0.2) / 0.3
    } else if t < 0.8 {
        1.0 - (t - 0.5) / 0.3
    } else {
        0.0
    };
    bias[PossibilityDomain::Ecology.index()] = 0.35 * ramp;
    bias[PossibilityDomain::Hydrology.index()] = 0.30 * ramp;
    bias[PossibilityDomain::Climate.index()] = -0.20 * ramp;
    bias
}

/// The scripted anchor set at a frame: an Emphasize anchor drops a third of
/// the way in (frozen at the player position of that frame), a Suppress anchor
/// at two thirds, both cleared near the end.
fn scripted_anchors(frame: u32, frames: u32, velocity: (f64, f64)) -> Vec<Anchor> {
    let position_at = |f: u32| (f64::from(f) * velocity.0, f64::from(f) * velocity.1);
    let mut anchors = Vec::new();
    let drop_a = frames / 3;
    let drop_b = 2 * frames / 3;
    let clear = frames.saturating_sub(frames / 10);
    if frame >= clear {
        return anchors;
    }
    if frame >= drop_a {
        anchors.push(Anchor {
            world_pos: position_at(drop_a),
            mask: domain_mask(&[PossibilityDomain::Ecology, PossibilityDomain::Hydrology]),
            kind: AnchorKind::Emphasize,
            strength: 0.6,
            falloff_radius: 1500.0,
        });
    }
    if frame >= drop_b {
        anchors.push(Anchor {
            world_pos: position_at(drop_b),
            mask: domain_mask(&[PossibilityDomain::Climate]),
            kind: AnchorKind::Suppress,
            strength: 0.5,
            falloff_radius: 1200.0,
        });
    }
    anchors
}

/// Snapshot of the drift-layer samples of every cached region, used for the
/// frame-to-frame delta checks.
type TileSnapshot = BTreeMap<RegionCoord, [Option<Vec<f32>>; CHANNEL_COUNT]>;

fn snapshot_tiles(map: &RegionMap) -> TileSnapshot {
    let mut snap = TileSnapshot::new();
    for (&coord, tiles) in map.cache().iter() {
        let mut channels: [Option<Vec<f32>>; CHANNEL_COUNT] = Default::default();
        for (i, tile) in tiles.channels.iter().enumerate() {
            channels[i] = tile.as_ref().map(|t| t.samples().to_vec());
        }
        snap.insert(coord, channels);
    }
    snap
}

/// Order-stable hash of the full end-of-run state (regions + cache).
fn state_hash(map: &RegionMap) -> u64 {
    let mut h: u64 = 0xC017_1401_7CBE_11A5;
    for region in map.iter_active() {
        h = mix(h, region.coord.x as u32 as u64);
        h = mix(h, region.coord.y as u32 as u64);
        h = mix(h, u64::from(region.revision));
        h = mix(h, u64::from(region.stability.to_bits()));
        for d in region.current.dims {
            h = mix(h, u64::from(d.to_bits()));
        }
    }
    for (_, tiles) in map.cache().iter() {
        for tile in tiles.channels.iter().flatten() {
            h = mix(h, tile.content_hash());
        }
    }
    h
}

/// Run the scripted continuity replay and collect violations.
#[must_use]
pub fn run_continuity_replay(cfg: &ReplayConfig) -> ReplayReport {
    let field = PossibilityField::default();
    let mut map = RegionMap::new(cfg.stream);
    let mut violations = Vec::new();
    let mut final_stats = FrameStats::default();
    let mut peak_regions = 0usize;
    let mut peak_cache_bytes = 0usize;

    // (revision, pinned) per region from the previous frame.
    let mut prev_state: BTreeMap<RegionCoord, (u32, bool)> = BTreeMap::new();
    let mut prev_tiles = TileSnapshot::new();

    // Travel per frame fuels convergence (ADR 0006); the scripted camera
    // moves at constant velocity except frame 0 (nothing traveled yet).
    let frame_travel = f64::hypot(cfg.velocity.0, cfg.velocity.1);

    for frame in 0..cfg.frames {
        let player = (
            f64::from(frame) * cfg.velocity.0,
            f64::from(frame) * cfg.velocity.1,
        );
        let travel = if frame == 0 { 0.0 } else { frame_travel };
        let bias = scripted_bias(frame, cfg.frames);
        let anchors = scripted_anchors(frame, cfg.frames, cfg.velocity);

        final_stats = map.update(
            player,
            travel,
            &field,
            &anchors,
            &bias,
            &cfg.budget,
            &InlineExecutor,
        );
        peak_regions = peak_regions.max(final_stats.active_regions);
        peak_cache_bytes = peak_cache_bytes.max(final_stats.cache_bytes);

        // -- Pinned stability: pinned before and after ⇒ revision unchanged.
        for region in map.iter_active() {
            if region.stability >= 1.0 {
                if let Some(&(prev_rev, was_pinned)) = prev_state.get(&region.coord) {
                    if was_pinned && region.revision != prev_rev {
                        record(
                            &mut violations,
                            format!(
                                "frame {frame}: pinned region ({}, {}) revision {} -> {} (changed while pinned)",
                                region.coord.x, region.coord.y, prev_rev, region.revision
                            ),
                        );
                    }
                }
            }
        }

        // -- Bounded per-frame delta on every cached sample still resident.
        let tiles_now = snapshot_tiles(&map);
        for (coord, channels) in &tiles_now {
            let Some(prev_channels) = prev_tiles.get(coord) else {
                continue;
            };
            for (i, samples) in channels.iter().enumerate() {
                let (Some(now), Some(before)) = (samples, &prev_channels[i]) else {
                    continue;
                };
                if now.len() != before.len() {
                    continue;
                }
                let eps = if i == CHANNEL_TEMPERATURE {
                    cfg.temperature_epsilon
                } else {
                    cfg.unit_epsilon
                };
                let mut worst = 0.0f32;
                for (a, b) in now.iter().zip(before) {
                    worst = worst.max((a - b).abs());
                }
                if worst > eps {
                    record(
                        &mut violations,
                        format!(
                            "frame {frame}: region ({}, {}) channel {i} sample jumped {worst} (> {eps})",
                            coord.x, coord.y
                        ),
                    );
                }
                // Pinned regions must not change at all.
                if let (Some(region), Some(&(_, was_pinned))) =
                    (map.get(*coord), prev_state.get(coord))
                {
                    if was_pinned
                        && region.stability >= 1.0
                        && worst > 0.0
                        && (i == CHANNEL_MOISTURE || i == CHANNEL_VEGETATION)
                    {
                        record(
                            &mut violations,
                            format!(
                                "frame {frame}: pinned region ({}, {}) drift channel {i} changed by {worst}",
                                coord.x, coord.y
                            ),
                        );
                    }
                }
            }
        }

        // -- No orphan seams: adjacent targets differ by a bounded gradient.
        for region in map.iter_active() {
            for (dx, dy) in [(1, 0), (0, 1)] {
                let neighbor = RegionCoord::new(region.coord.x + dx, region.coord.y + dy);
                let Some(other) = map.get(neighbor) else {
                    continue;
                };
                for i in 0..POSSIBILITY_DIMS {
                    let diff = (region.target.dims[i] - other.target.dims[i]).abs();
                    if diff > cfg.seam_bound {
                        record(
                            &mut violations,
                            format!(
                                "frame {frame}: target seam {diff} in dim {i} between ({}, {}) and ({}, {})",
                                region.coord.x, region.coord.y, neighbor.x, neighbor.y
                            ),
                        );
                    }
                }
            }
        }

        prev_state = map
            .iter_active()
            .map(|r| (r.coord, (r.revision, r.stability >= 1.0)))
            .collect();
        prev_tiles = tiles_now;
    }

    ReplayReport {
        frames: cfg.frames,
        violations,
        state_hash: state_hash(&map),
        final_stats,
        peak_regions,
        peak_cache_bytes,
    }
}
