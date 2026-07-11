//! Per-frame temporal budgets (implementation-plan.md section 6.6;
//! phase-1-plan.md section 4.2).
//!
//! Streaming, convergence, and regeneration are all capped per frame so a big
//! possibility change ripples outward over several frames instead of hitching.
//! Deferring work to a later frame is expected and healthy backpressure, not
//! an error — [`crate::stream::FrameStats`] reports it so profiling can size
//! these caps.

/// Caps on how much world work one frame may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// New regions inserted into the window per frame.
    pub max_loads: usize,
    /// Regions stepped toward their target per frame.
    pub max_converge_regions: usize,
    /// Region-layer generation jobs dispatched per frame.
    pub max_regen_layers: usize,
}

impl Budget {
    /// A budget scaled linearly from a 60 Hz baseline.
    ///
    /// The baseline constants were sized so a full window fill amortizes over
    /// roughly a second at 16.6 ms; the criterion benches (phase-1-plan.md
    /// section 12) exist to refine them with measurements rather than taste.
    #[must_use]
    pub fn per_frame(target_ms: f32) -> Self {
        let scale = (target_ms / 16.6).clamp(0.25, 8.0);
        Self {
            max_loads: ((48.0 * scale) as usize).max(1),
            max_converge_regions: ((512.0 * scale) as usize).max(1),
            max_regen_layers: ((48.0 * scale) as usize).max(1),
        }
    }

    /// No caps — for headless tools and tests that want a settled world now.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_loads: usize::MAX,
            max_converge_regions: usize::MAX,
            max_regen_layers: usize::MAX,
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
        assert!(fast.max_regen_layers < base.max_regen_layers);
        assert!(slow.max_regen_layers > base.max_regen_layers);
        assert!(fast.max_loads >= 1);
    }
}
