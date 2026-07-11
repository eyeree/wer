//! The continuity regression test (phase-1-plan.md section 11.3): the
//! machine-checked proxy for the Phase 1 visual success criterion.

use tools::{run_continuity_replay, ReplayConfig};

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
