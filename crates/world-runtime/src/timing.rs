//! Per-pass timing inside [`crate::stream::RegionMap::update`]
//! (phase-6-plan.md §5.2, §12) — the measurement layer every Phase 6
//! optimization is judged against.
//!
//! Timing is feature-gated (`pass-timing`, enabled by the native shell and
//! the headless tools) because `std::time::Instant` is unavailable on
//! `wasm32-unknown-unknown`: without the feature the hooks compile to
//! nothing and every reported duration is zero, keeping the neutral crates
//! wasm-clean (AGENTS.md boundary rule). Wall-clock is *telemetry only* —
//! it is reported, compared against the committed baseline by `wer-scale
//! --report`, and never CI-gated or folded into any hash (§12.6): outputs
//! must not depend on machine speed (ADR 0018).

/// One step of the update pipeline (module docs of [`crate::stream`]), plus
/// the caller-owned vault flush. The pipeline's two integrate steps both
/// accumulate into [`Pass::Integrate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    /// Drain finished generation jobs into the caches.
    Integrate,
    /// Radius + capacity eviction sweep.
    Evict,
    /// Insert missing regions nearest-first.
    Load,
    /// Recompute stability + steered targets.
    Retarget,
    /// Step unpinned regions toward their targets.
    Converge,
    /// Topological dispatch of stale region-layers.
    Dispatch,
    /// Near-field organism realization.
    Realize,
    /// Vault record flush — timed by the shell (the vault is driven by the
    /// caller, not by `update`), reported through the same table.
    Flush,
    /// POV chunk sync (scheduling + amortized mesh integration) — timed by
    /// the shell, following the `Flush` precedent (3d-phase-1-plan.md §8.1).
    /// Derived presentation work; zero whenever POV mode is off.
    Mesh,
}

/// Number of [`Pass`] variants (the length of `FrameStats::pass_ms`).
pub const PASS_COUNT: usize = 9;

impl Pass {
    /// Every pass, in pipeline order.
    pub const ALL: [Pass; PASS_COUNT] = [
        Pass::Integrate,
        Pass::Evict,
        Pass::Load,
        Pass::Retarget,
        Pass::Converge,
        Pass::Dispatch,
        Pass::Realize,
        Pass::Flush,
        Pass::Mesh,
    ];

    /// Index into `pass_ms`.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Short display name for panels and reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Pass::Integrate => "integrate",
            Pass::Evict => "evict",
            Pass::Load => "load",
            Pass::Retarget => "retarget",
            Pass::Converge => "converge",
            Pass::Dispatch => "dispatch",
            Pass::Realize => "realize",
            Pass::Flush => "flush",
            Pass::Mesh => "mesh",
        }
    }
}

/// Accumulated per-pass milliseconds for one frame. Cheap to construct every
/// frame; all zeros when the `pass-timing` feature is off.
#[derive(Debug, Default, Clone, Copy)]
pub struct PassTimings {
    /// Milliseconds per pass, indexed by [`Pass::index`].
    pub ms: [f32; PASS_COUNT],
}

impl PassTimings {
    /// Run `f`, attributing its wall-clock to `pass` (accumulating, so a pass
    /// that runs twice per frame — integrate — reports its total).
    #[inline]
    pub fn time<R>(&mut self, pass: Pass, f: impl FnOnce() -> R) -> R {
        #[cfg(feature = "pass-timing")]
        {
            let start = std::time::Instant::now();
            let out = f();
            self.ms[pass.index()] += start.elapsed().as_secs_f32() * 1000.0;
            out
        }
        #[cfg(not(feature = "pass-timing"))]
        {
            let _ = pass;
            f()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_index_their_slot() {
        for (i, pass) in Pass::ALL.iter().enumerate() {
            assert_eq!(pass.index(), i);
        }
    }

    #[test]
    fn time_returns_the_closure_result() {
        let mut t = PassTimings::default();
        let v = t.time(Pass::Load, || 41 + 1);
        assert_eq!(v, 42);
    }
}
