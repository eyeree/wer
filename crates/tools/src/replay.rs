//! Headless continuity replay (phase-1-plan.md section 11.3, extended for the
//! Phase 2 stack by phase-2-plan.md §12.2).
//!
//! Drives a [`RegionMap`] along a scripted, fully deterministic camera path —
//! moving the player, ramping possibility bias, dropping and clearing anchors —
//! and machine-checks the continuity guarantees without graphics:
//!
//! - **Pinned stability**: a region that is pinned (`stability == 1`) across an
//!   update never bumps its revision, and none of its tiles (any channel, the
//!   biome ids) ever change — the "no near-field pop" guarantee.
//! - **Stable trio**: terrain and geology tiles never change while resident,
//!   and macro drainage tiles never regenerate — the script drives only fast
//!   domains, under which the stable trio must hold *everywhere*, not just in
//!   the pinned zone (phase-2-plan.md §12.2).
//! - **Bounded per-frame delta**: no cached sample within the window moves more
//!   than its channel's epsilon in one frame (no snapping).
//! - **Macro seams**: river expression steps across macro-tile boundaries stay
//!   inside the truncated-catchment bound (phase-2-plan.md §7.3).
//! - **No orphan seams**: adjacent resident regions never differ in target by
//!   more than the field's per-region gradient bound.
//! - **Determinism**: two runs of the same script produce bit-identical final
//!   state — regions, field cache, biome tiles, and macro cache (asserted by
//!   callers via [`ReplayReport::state_hash`]).
//!
//! This replay is the automated proxy for the visual success criterion and
//! guards against regressions once the prototype "looks right".

use std::collections::BTreeMap;

use world_core::{
    domain_mask, macro_coord_for, mix, Anchor, AnchorKind, PossibilityDomain, PossibilityField,
    RegionCoord, POSSIBILITY_DIMS,
};
use world_runtime::{
    Budget, FrameStats, InlineExecutor, RegionMap, StreamConfig, CHANNEL_CANOPY, CHANNEL_COUNT,
    CHANNEL_ELEVATION, CHANNEL_HARDNESS, CHANNEL_RIVER, CHANNEL_TEMPERATURE,
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
    /// Max allowed per-frame change of a `[0, 1]` field sample. Sized to admit
    /// a biome-classification flip (adjacent classes differ in base density by
    /// up to ~0.45) while still catching full-range snaps.
    pub unit_epsilon: f32,
    /// Max allowed per-frame change of a temperature sample (°C).
    pub temperature_epsilon: f32,
    /// Max allowed per-frame change of a canopy-height sample (world units) —
    /// canopy legitimately steps when a cell's biome class flips.
    pub canopy_epsilon: f32,
    /// Max allowed river-expression step between edge-adjacent samples across
    /// a macro-tile boundary (the truncated-catchment bound, §7.3).
    pub river_seam_bound: f32,
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
                max_regen_cost: 2048,
            },
            velocity: (37.0, 23.0),
            // One converge step moves a dimension ≤ converge_rate; generation
            // maps dimensions to samples with modest slopes. Snapping shows up
            // as near-full-range jumps, far above these.
            unit_epsilon: 0.6,
            temperature_epsilon: 15.0,
            canopy_epsilon: 30.0,
            river_seam_bound: 0.6,
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
    /// Order-stable hash of the final region + cache + macro state. Two runs
    /// of the same script must produce the same value (single-platform
    /// determinism).
    pub state_hash: u64,
    /// Stats from the final frame, for logging.
    pub final_stats: FrameStats,
    /// Peak resident region count observed.
    pub peak_regions: usize,
    /// Peak field-cache bytes observed (macro cache included).
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
/// back — deterministic, no clocks, no randomness. Fast domains only: the
/// stable-trio assertion depends on the script never touching Geology or
/// Planetary.
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

/// Snapshot of every cached tile of every region, for the frame-to-frame
/// delta checks.
#[derive(Debug, Default, Clone)]
struct RegionSnapshot {
    channels: [Option<Vec<f32>>; CHANNEL_COUNT],
    biome: Option<Vec<u8>>,
}

type TileSnapshot = BTreeMap<RegionCoord, RegionSnapshot>;

fn snapshot_tiles(map: &RegionMap) -> TileSnapshot {
    let mut snap = TileSnapshot::new();
    for (&coord, tiles) in map.cache().iter() {
        let mut region = RegionSnapshot::default();
        for (i, tile) in tiles.channels.iter().enumerate() {
            region.channels[i] = tile.as_ref().map(|t| t.samples().to_vec());
        }
        region.biome = tiles.biome.as_ref().map(|t| t.samples().to_vec());
        snap.insert(coord, region);
    }
    snap
}

/// Order-stable hash of the full end-of-run state (regions + field cache +
/// biome tiles + macro cache).
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
        if let Some(biome) = &tiles.biome {
            h = mix(h, biome.content_hash());
        }
    }
    for (_, tile) in map.macro_cache().iter() {
        h = mix(h, tile.content_hash());
    }
    h
}

/// Per-frame delta bound for a channel.
fn channel_epsilon(cfg: &ReplayConfig, channel: usize) -> f32 {
    match channel {
        CHANNEL_TEMPERATURE => cfg.temperature_epsilon,
        CHANNEL_CANOPY => cfg.canopy_epsilon,
        _ => cfg.unit_epsilon,
    }
}

/// Run the scripted continuity replay and collect violations.
#[must_use]
#[allow(clippy::too_many_lines)]
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
    // Stable-trio ledger: (elevation, hardness) content hashes per region and
    // macro-tile content hashes, fixed for as long as the entry is resident.
    let mut trio_hashes: BTreeMap<RegionCoord, (Option<u64>, Option<u64>)> = BTreeMap::new();
    let mut macro_hashes: BTreeMap<RegionCoord, u64> = BTreeMap::new();

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
        peak_cache_bytes =
            peak_cache_bytes.max(final_stats.cache_bytes + final_stats.macro_cache_bytes);

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

        // -- Stable trio: terrain/geology tiles never change while resident
        //    (fast-dims-only script), and macro tiles never regenerate.
        trio_hashes.retain(|c, _| map.get(*c).is_some());
        for (&coord, tiles) in map.cache().iter() {
            let elevation = tiles.channels[CHANNEL_ELEVATION]
                .as_ref()
                .map(|t| t.content_hash());
            let hardness = tiles.channels[CHANNEL_HARDNESS]
                .as_ref()
                .map(|t| t.content_hash());
            let entry = trio_hashes.entry(coord).or_insert((None, None));
            for (label, now, seen) in [
                ("terrain", elevation, &mut entry.0),
                ("geology", hardness, &mut entry.1),
            ] {
                match (*seen, now) {
                    (Some(a), Some(b)) if a != b => record(
                        &mut violations,
                        format!(
                            "frame {frame}: stable-trio {label} tile of ({}, {}) changed under fast drift",
                            coord.x, coord.y
                        ),
                    ),
                    (None, Some(b)) => *seen = Some(b),
                    _ => {}
                }
            }
        }
        macro_hashes.retain(|c, _| map.iter_active().any(|r| macro_coord_for(r.coord) == *c));
        for (&mc, tile) in map.macro_cache().iter() {
            let now = tile.content_hash();
            match macro_hashes.get(&mc) {
                Some(&seen) if seen != now => record(
                    &mut violations,
                    format!(
                        "frame {frame}: macro drainage tile ({}, {}) regenerated under drift",
                        mc.x, mc.y
                    ),
                ),
                None => {
                    macro_hashes.insert(mc, now);
                }
                _ => {}
            }
        }

        // -- Bounded per-frame delta on every cached sample still resident;
        //    pinned regions must not change at all (any channel, biome ids).
        let tiles_now = snapshot_tiles(&map);
        for (coord, snapshot) in &tiles_now {
            let Some(prev) = prev_tiles.get(coord) else {
                continue;
            };
            let pinned_now = map.get(*coord).is_some_and(|r| r.stability >= 1.0);
            let was_pinned = prev_state.get(coord).is_some_and(|&(_, p)| p);
            let hold_still = pinned_now && was_pinned;
            for (i, samples) in snapshot.channels.iter().enumerate() {
                let (Some(now), Some(before)) = (samples, &prev.channels[i]) else {
                    continue;
                };
                if now.len() != before.len() {
                    continue;
                }
                let mut worst = 0.0f32;
                for (a, b) in now.iter().zip(before) {
                    worst = worst.max((a - b).abs());
                }
                let eps = channel_epsilon(cfg, i);
                if worst > eps {
                    record(
                        &mut violations,
                        format!(
                            "frame {frame}: region ({}, {}) channel {i} sample jumped {worst} (> {eps})",
                            coord.x, coord.y
                        ),
                    );
                }
                if hold_still && worst > 0.0 {
                    record(
                        &mut violations,
                        format!(
                            "frame {frame}: pinned region ({}, {}) channel {i} changed by {worst}",
                            coord.x, coord.y
                        ),
                    );
                }
            }
            if let (Some(now), Some(before)) = (&snapshot.biome, &prev.biome) {
                if hold_still && now != before {
                    record(
                        &mut violations,
                        format!(
                            "frame {frame}: pinned region ({}, {}) biome ids changed",
                            coord.x, coord.y
                        ),
                    );
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

    // -- Macro seam assertion (final frame): river expression across
    //    macro-tile boundaries steps by less than the truncation bound.
    check_macro_seams(&map, cfg, &mut violations);

    ReplayReport {
        frames: cfg.frames,
        violations,
        state_hash: state_hash(&map),
        final_stats,
        peak_regions,
        peak_cache_bytes,
    }
}

/// Compare edge-adjacent river samples of neighboring regions that live in
/// different macro tiles (phase-2-plan.md §12.2). The step includes the
/// legitimate cross-cell gradient plus the truncated-catchment error; the
/// bound is a tear detector, not a precision gauge.
fn check_macro_seams(map: &RegionMap, cfg: &ReplayConfig, violations: &mut Vec<String>) {
    let res = cfg.stream.field_resolution;
    for region in map.iter_active() {
        let coord = region.coord;
        for (dx, dy) in [(1i32, 0i32), (0, 1)] {
            let neighbor = RegionCoord::new(coord.x + dx, coord.y + dy);
            if macro_coord_for(coord) == macro_coord_for(neighbor) {
                continue;
            }
            let (Some(a), Some(b)) = (
                map.cache().channel(coord, CHANNEL_RIVER),
                map.cache().channel(neighbor, CHANNEL_RIVER),
            ) else {
                continue;
            };
            let mut worst = 0.0f32;
            for k in 0..res {
                let (av, bv) = if dx == 1 {
                    (a.get(res - 1, k), b.get(0, k))
                } else {
                    (a.get(k, res - 1), b.get(k, 0))
                };
                worst = worst.max((av - bv).abs());
            }
            if worst > cfg.river_seam_bound {
                record(
                    violations,
                    format!(
                        "macro seam: river steps {worst} (> {}) between ({}, {}) and ({}, {})",
                        cfg.river_seam_bound, coord.x, coord.y, neighbor.x, neighbor.y
                    ),
                );
            }
        }
    }
}
