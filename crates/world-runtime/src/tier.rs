//! Resource tiers (phase-6-plan.md §6.7, §7.4): Low / Mid / High presets
//! selecting streaming radii, budgets, cache ceilings, and realization
//! density from platform-gathered inputs.
//!
//! The tier premise the scale harness enforces (ADR 0018): **tiers select
//! pacing and capacity presets; identity is tier-invariant.** A tier changes
//! *when and how much* work happens per frame — and, through
//! `organisms_per_cell`, how densely the same aggregates realize — never
//! *what* the world is: dependency hashes, quantized buckets, and every
//! shared/persisted surface are identical across tiers.
//!
//! Detection is a pure decision over [`TierInputs`]; the *inputs* (core
//! count, adapter class, `WER_TIER`/`WER_CACHE_MB` overrides) are gathered
//! by the platform crates — this module stays wasm-clean. Phase 7 feeds
//! browser inputs (worker count, memory hints) into the same table.

use crate::budget::Budget;
use crate::stream::StreamConfig;
use world_core::REGION_SIZE;

/// Coarse GPU adapter class, as reported by the platform shell (wgpu
/// `DeviceType`, collapsed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterClass {
    /// A discrete GPU.
    Discrete,
    /// An integrated GPU.
    Integrated,
    /// A software rasterizer (llvmpipe et al.).
    Cpu,
    /// Nothing detected (headless, or detection failed).
    Unknown,
}

/// The performance tier presets (phase-6-plan.md §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceTier {
    /// The Phase 5 defaults — the proven configuration.
    Low,
    /// The geometric midpoint.
    Mid,
    /// The §1.5 density targets.
    High,
}

/// The platform-gathered inputs the detection table decides from.
#[derive(Debug, Clone, Copy)]
pub struct TierInputs {
    /// Logical CPU cores (`available_parallelism`).
    pub cores: usize,
    /// GPU adapter class.
    pub adapter: AdapterClass,
    /// Explicit override (`WER_TIER`), which always wins.
    pub override_tier: Option<ResourceTier>,
}

impl ResourceTier {
    /// The detection table (phase-6-plan.md §6.7): override wins; ≤ 4 cores
    /// or a cpu-class adapter → Low; ≥ 8 cores and a discrete adapter →
    /// High; everything else Mid.
    #[must_use]
    pub fn detect(inputs: &TierInputs) -> Self {
        if let Some(tier) = inputs.override_tier {
            return tier;
        }
        if inputs.cores <= 4 || inputs.adapter == AdapterClass::Cpu {
            return ResourceTier::Low;
        }
        if inputs.cores >= 8 && inputs.adapter == AdapterClass::Discrete {
            return ResourceTier::High;
        }
        ResourceTier::Mid
    }

    /// Parse a `WER_TIER` override.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "low" => Some(ResourceTier::Low),
            "mid" => Some(ResourceTier::Mid),
            "high" => Some(ResourceTier::High),
            _ => None,
        }
    }

    /// Display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ResourceTier::Low => "low",
            ResourceTier::Mid => "mid",
            ResourceTier::High => "high",
        }
    }

    /// The tier's streaming preset (§7.4). Low is exactly
    /// [`StreamConfig::default`] — the proven Phase 5 configuration.
    #[must_use]
    pub fn stream_config(self) -> StreamConfig {
        let base = StreamConfig::default();
        match self {
            ResourceTier::Low => base,
            ResourceTier::Mid => StreamConfig {
                far_radius: 11.0 * REGION_SIZE,
                load_radius: 14.0 * REGION_SIZE,
                unload_radius: 16.0 * REGION_SIZE,
                max_field_cache_bytes: 96 * 1024 * 1024,
                max_macro_cache_bytes: 16 * 1024 * 1024,
                organisms_per_cell: 2,
                ..base
            },
            ResourceTier::High => StreamConfig {
                far_radius: 13.0 * REGION_SIZE,
                load_radius: 17.0 * REGION_SIZE,
                unload_radius: 19.0 * REGION_SIZE,
                max_field_cache_bytes: 160 * 1024 * 1024,
                max_macro_cache_bytes: 24 * 1024 * 1024,
                organisms_per_cell: 4,
                ..base
            },
        }
    }

    /// The tier's frame budget (§7.4). Low is the Phase 5 default; the
    /// bigger tiers spend the headroom the M2–M5 milestones measured
    /// (docs/perf-baseline.md) on regen throughput, realization, and
    /// resonance, and amortize the (larger) retarget pass over ≤ 4 frames.
    #[must_use]
    pub fn budget(self) -> Budget {
        let base = Budget::default();
        match self {
            ResourceTier::Low => base,
            ResourceTier::Mid => Budget {
                max_loads: 64,
                max_regen_cost: 192,
                max_realize_organisms: 800,
                max_resonance_nodes: 96,
                max_retarget_regions: 160,
                ..base
            },
            ResourceTier::High => Budget {
                max_loads: 96,
                max_regen_cost: 384,
                max_realize_organisms: 1_600,
                max_resonance_nodes: 128,
                max_retarget_regions: 240,
                ..base
            },
        }
    }

    /// Whether the GPU refinement octaves default on at this tier (§7.4).
    #[must_use]
    pub const fn refinement(self) -> bool {
        !matches!(self, ResourceTier::Low)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(cores: usize, adapter: AdapterClass) -> TierInputs {
        TierInputs {
            cores,
            adapter,
            override_tier: None,
        }
    }

    #[test]
    fn detection_table() {
        assert_eq!(
            ResourceTier::detect(&inputs(4, AdapterClass::Discrete)),
            ResourceTier::Low
        );
        assert_eq!(
            ResourceTier::detect(&inputs(16, AdapterClass::Cpu)),
            ResourceTier::Low
        );
        assert_eq!(
            ResourceTier::detect(&inputs(16, AdapterClass::Discrete)),
            ResourceTier::High
        );
        assert_eq!(
            ResourceTier::detect(&inputs(6, AdapterClass::Discrete)),
            ResourceTier::Mid
        );
        assert_eq!(
            ResourceTier::detect(&inputs(16, AdapterClass::Integrated)),
            ResourceTier::Mid
        );
        let overridden = TierInputs {
            cores: 2,
            adapter: AdapterClass::Cpu,
            override_tier: Some(ResourceTier::High),
        };
        assert_eq!(ResourceTier::detect(&overridden), ResourceTier::High);
    }

    #[test]
    fn low_is_the_phase5_default() {
        assert_eq!(ResourceTier::Low.stream_config(), StreamConfig::default());
        assert_eq!(ResourceTier::Low.budget(), Budget::default());
        assert!(!ResourceTier::Low.refinement());
        assert!(ResourceTier::High.refinement());
    }

    #[test]
    fn parse_round_trips() {
        for tier in [ResourceTier::Low, ResourceTier::Mid, ResourceTier::High] {
            assert_eq!(ResourceTier::parse(tier.name()), Some(tier));
        }
        assert_eq!(ResourceTier::parse("ultra"), None);
    }
}
