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
        }
    }

    /// No caps — for headless tools and tests that want a settled world now.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_loads: usize::MAX,
            max_converge_regions: usize::MAX,
            max_regen_cost: u32::MAX,
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
