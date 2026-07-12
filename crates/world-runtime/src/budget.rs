//! Per-frame temporal budgets (implementation-plan.md section 6.6;
//! phase-2-plan.md §8.2).
//!
//! Streaming, convergence, and regeneration are all capped per frame so a big
//! possibility change ripples outward over several frames instead of hitching.
//! Regeneration is budgeted by **cost, not count**: layers declare relative
//! costs ([`world_core::layer::LayerDecl::cost`]) and the frame budget spends
//! cost units, so one expensive macro drainage job weighs more than a stack of
//! cheap climate tiles. Deferring work to a later frame is expected and
//! healthy backpressure, not an error — [`crate::stream::FrameStats`] reports
//! it so profiling can size these caps.

/// Caps on how much world work one frame may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// New regions inserted into the window per frame.
    pub max_loads: usize,
    /// Regions stepped toward their target per frame.
    pub max_converge_regions: usize,
    /// Generation cost units dispatched per frame (phase-2-plan.md §8.2).
    pub max_regen_cost: u32,
    /// Near-field organisms instantiated per frame, so entering a dense biome
    /// amortizes realization over a few frames rather than hitching
    /// (phase-3-plan.md §8.4). Budgeted by whole regions: a region realizes
    /// fully once started, and the pass stops starting new regions past the cap.
    pub max_realize_organisms: usize,
    /// Contributing nodes the per-frame resonance graph may hold, so a dense
    /// biome does not build an unbounded graph (phase-4-plan.md §8.3) — the
    /// analogue of `max_realize_organisms`.
    pub max_resonance_nodes: usize,
    /// Records the vault encodes and writes per [`crate::vault::Vault::flush`]
    /// call, so persistence obeys temporal budgeting like every other
    /// subsystem and a bulk import never stalls a frame (phase-5-plan.md §7.7).
    pub max_persist_ops: usize,
    /// Route nodes contributing derived attraction anchors per frame
    /// (phase-5-plan.md §8.2) — the steering-side analogue of
    /// `max_resonance_nodes`, so a dense recorded corridor stays bounded.
    pub max_route_attraction_nodes: usize,
}

impl Budget {
    /// A budget scaled linearly from a 60 Hz baseline.
    ///
    /// The baseline constants were sized so a full window fill amortizes over
    /// roughly a second at 16.6 ms; the criterion benches (phase-2-plan.md
    /// §13) exist to refine them with measurements rather than taste. The cost
    /// baseline corresponds to Phase 1's 48 layer jobs at an average declared
    /// cost of ~2.
    #[must_use]
    pub fn per_frame(target_ms: f32) -> Self {
        let scale = (target_ms / 16.6).clamp(0.25, 8.0);
        Self {
            max_loads: ((48.0 * scale) as usize).max(1),
            max_converge_regions: ((512.0 * scale) as usize).max(1),
            max_regen_cost: ((96.0 * scale) as u32).max(1),
            // A few hundred organisms/frame keeps entering a dense biome smooth
            // while still filling the near window in a handful of frames.
            max_realize_organisms: ((400.0 * scale) as usize).max(1),
            // The resonance graph is cheap; a few dozen nearest nodes capture
            // the local density/diversity without an unbounded scan.
            max_resonance_nodes: 64,
            // Records are ~100 bytes; a handful per frame drains any realistic
            // dirty queue within a second without touching the frame budget.
            max_persist_ops: ((8.0 * scale) as usize).max(1),
            // A few dozen nearest corridor nodes bound route steering the same
            // way the resonance graph is bounded.
            max_route_attraction_nodes: 32,
        }
    }

    /// No caps — for headless tools and tests that want a settled world now.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_loads: usize::MAX,
            max_converge_regions: usize::MAX,
            max_regen_cost: u32::MAX,
            max_realize_organisms: usize::MAX,
            max_resonance_nodes: usize::MAX,
            max_persist_ops: usize::MAX,
            max_route_attraction_nodes: usize::MAX,
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::per_frame(16.6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_frame_scales_with_target() {
        let fast = Budget::per_frame(8.3);
        let base = Budget::per_frame(16.6);
        let slow = Budget::per_frame(33.3);
        assert!(fast.max_regen_cost < base.max_regen_cost);
        assert!(slow.max_regen_cost > base.max_regen_cost);
        assert!(fast.max_loads >= 1);
    }
}
