//! The continuity regression test (phase-1-plan.md section 11.3): the
//! machine-checked proxy for the Phase 1 visual success criterion. Phase 6
//! re-runs it under the lane executor and quartered budgets
//! (phase-6-plan.md §11.3) — the same script and the same assertions must
//! hold under any schedule.

use tools::replay::run_continuity_replay_with;
use tools::{run_continuity_replay, LaneExecutor, ReplayConfig};
use world_runtime::Budget;

#[test]
fn continuity_replay_passes_and_is_deterministic() {
    let cfg = ReplayConfig {
        frames: 180,
        ..ReplayConfig::default()
    };
    let a = run_continuity_replay(&cfg);
    assert!(
        a.passed(),
        "continuity violations:\n{}",
        a.violations.join("\n")
    );
    assert!(a.peak_regions > 0 && a.final_stats.active_regions > 0);

    // Same script, second run: bit-identical final state.
    let b = run_continuity_replay(&cfg);
    assert!(b.passed(), "second run violated continuity");
    assert_eq!(
        a.state_hash, b.state_hash,
        "two runs of the same script must produce identical worlds"
    );
}

/// Phase 6 (§11.3): the replay's continuity assertions hold under the
/// threaded lane executor too — order-independence as a machine-checked
/// contract, not an Inline-only circumstance. Settled-state *equality*
/// across executors is gated by the scale harness's run-out script
/// (ADR 0018): mid-script realized state legitimately depends on pacing
/// (travel-fueled, resonance-gated convergence), so this test asserts the
/// continuity bounds, not hash equality against the inline run.
#[test]
fn continuity_replay_passes_under_lane_executor() {
    let cfg = ReplayConfig {
        frames: 180,
        ..ReplayConfig::default()
    };
    let executor = LaneExecutor::auto();
    let report = run_continuity_replay_with(&cfg, &executor, true);
    assert!(
        report.passed(),
        "continuity violations under LaneExecutor:\n{}",
        report.violations.join("\n")
    );
}

/// Phase 6 (§11.3): quartered budgets change pacing, never continuity — the
/// same script passes with every per-frame cap cut to a quarter, and stays
/// two-run deterministic.
#[test]
fn continuity_replay_passes_with_quartered_budgets() {
    let base = ReplayConfig::default();
    let cfg = ReplayConfig {
        frames: 180,
        budget: Budget {
            max_loads: base.budget.max_loads / 4,
            max_converge_regions: base.budget.max_converge_regions / 4,
            max_regen_cost: base.budget.max_regen_cost / 4,
            ..base.budget
        },
        ..base
    };
    let a = run_continuity_replay(&cfg);
    assert!(
        a.passed(),
        "continuity violations at quartered budgets:\n{}",
        a.violations.join("\n")
    );
    let b = run_continuity_replay(&cfg);
    assert_eq!(a.state_hash, b.state_hash);
}

/// Phase 6 (§11.3): the continuity bounds hold at the High-tier streaming
/// preset (larger radii and ceilings) with `organisms_per_cell = 1` —
/// radii/budget scaling must not perturb continuity.
#[test]
fn continuity_replay_passes_at_high_tier_config() {
    use world_runtime::{ResourceTier, StreamConfig};
    let base = ReplayConfig::default();
    let cfg = ReplayConfig {
        frames: 120,
        stream: StreamConfig {
            field_resolution: 8,
            organisms_per_cell: 1,
            // Ceilings scaled for the reduced resolution as in wer-scale.
            max_field_cache_bytes: 10 * 1024 * 1024,
            ..ResourceTier::High.stream_config()
        },
        budget: ResourceTier::High.budget(),
        ..base
    };
    let a = run_continuity_replay(&cfg);
    assert!(
        a.passed(),
        "continuity violations at High tier:\n{}",
        a.violations.join("\n")
    );
    let b = run_continuity_replay(&cfg);
    assert_eq!(a.state_hash, b.state_hash);
}
