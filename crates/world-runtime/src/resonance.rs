//! The resonance graph and transition gate (phase-4-plan.md §6.2, §7.5, §7.6;
//! ADR 0012).
//!
//! Resonance is the "orb resonating with nearby reality" made machine-readable:
//! a **transient, locally-built** graph over the near-window features (Phase 3's
//! realized organisms and aggregate fields), rebuilt each frame and dropped at
//! end of frame — never a global stored structure (section 14). It yields a
//! single scalar `strength ∈ [0, 1]` that **gates** convergence:
//!
//! ```text
//!   converge_rate = converge_per_unit · travel · resonance · transition_scale
//! ```
//!
//! Resonance *multiplies* the travel-fueled rate; it never adds (ADR 0012, the
//! one-way door). So a stationary player's world is still perfectly still (zero
//! travel ⇒ zero rate), and a player in a barren region simply cannot transition
//! (zero resonance ⇒ zero rate) — the world holds until they reach richer,
//! anchor-compatible ground. Resonance can only slow or enable transformation,
//! never manufacture it, so the ADR 0006 stand-still cliff stays closed.

/// One contributing near-field feature in a frame's resonance graph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResonanceNode {
    /// Where the feature sits in world space.
    pub world_pos: (f64, f64),
    /// The species id of the contributing organism (0 for a non-organism
    /// feature; Phase 4 only contributes organisms).
    pub species: u64,
    /// Distance from the player, in world units.
    pub distance: f64,
}

/// The transition capability at the player for one frame (phase-4-plan.md §7.5).
/// Transient: rebuilt each frame, dropped at end of frame; only the scalar
/// `strength` is folded into convergence, the rest is for the viz/panel.
#[derive(Debug, Clone)]
pub struct Resonance {
    /// Transition capability at the player, `0..=1` — the gate multiplier.
    pub strength: f32,
    /// Contributing near-field features within reach, nearest-first, capped at
    /// `max_resonance_nodes`.
    pub nodes: Vec<ResonanceNode>,
    /// How well the local ecology matches the active anchor set, `0..=1` — why
    /// steering here pulls where it does (the influence viz reads it).
    pub anchor_compatibility: f32,
}

impl Resonance {
    /// The empty resonance (a barren, organism-free neighbourhood): zero
    /// strength, so the world holds still (ADR 0012).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            strength: 0.0,
            nodes: Vec::new(),
            anchor_compatibility: 1.0,
        }
    }
}

/// How many near-window nodes saturate the density term. A handful of realized
/// organisms already reads as "dense enough to steer strongly", so an
/// organism-rich window reaches full density while a bare one stays near zero.
const DENSITY_SATURATION: f32 = 8.0;

/// Combine the bounded resonance terms into a single `[0, 1]` strength
/// (phase-4-plan.md §7.5). Monotone increasing in `density` with the other terms
/// held fixed — the ADR 0012 property the unit test pins. All inputs are `[0, 1]`.
///
/// `density` dominates (an empty neighbourhood cannot resonate); `diversity` and
/// `distance` shape the quality of a dense neighbourhood; `anchor_compatibility`
/// rewards steering toward a world the player is near an example of; `occlusion`
/// is the line-of-sight proxy (dense canopy attenuates).
#[must_use]
pub fn combine_resonance(
    density: f32,
    diversity: f32,
    distance: f32,
    anchor_compatibility: f32,
    occlusion: f32,
) -> f32 {
    let quality =
        (0.5 + 0.25 * diversity + 0.25 * distance) * (0.6 + 0.4 * anchor_compatibility) * occlusion;
    (density * quality).clamp(0.0, 1.0)
}

/// The density term for `node_count` contributing nodes: saturating toward 1 as
/// the neighbourhood fills. Zero nodes ⇒ zero density ⇒ zero resonance.
#[must_use]
pub fn density_term(node_count: usize) -> f32 {
    (node_count as f32 / DENSITY_SATURATION).clamp(0.0, 1.0)
}

/// The resonance-gated convergence rate (ADR 0012): `converge_per_unit · travel ·
/// resonance · transition_scale`, clamped to `cap`. Zero when *either* travel or
/// resonance is zero — the stand-still and barren-region guarantees.
#[must_use]
pub fn gated_rate(
    converge_per_unit: f32,
    travel: f64,
    resonance: f32,
    transition_scale: f32,
    cap: f32,
) -> f32 {
    let rate = converge_per_unit
        * travel.max(0.0) as f32
        * resonance.clamp(0.0, 1.0)
        * transition_scale.max(0.0);
    rate.min(cap).max(0.0)
}

/// Species entropy among the nodes, normalized to `[0, 1]` — the diversity term
/// (phase-4-plan.md §7.5). A single-species crowd resonates less than a varied
/// one; an empty or single-node set has zero diversity.
#[must_use]
pub fn species_entropy(nodes: &[ResonanceNode]) -> f32 {
    if nodes.len() < 2 {
        return 0.0;
    }
    // Count per species (nodes are few — a linear scan over a small cap).
    let mut species: Vec<(u64, u32)> = Vec::new();
    for node in nodes {
        match species.iter_mut().find(|(id, _)| *id == node.species) {
            Some((_, count)) => *count += 1,
            None => species.push((node.species, 1)),
        }
    }
    if species.len() < 2 {
        return 0.0;
    }
    let total = nodes.len() as f32;
    let mut entropy = 0.0f32;
    for (_, count) in &species {
        let p = *count as f32 / total;
        entropy -= p * p.ln();
    }
    // Normalize by the maximum entropy (uniform over the distinct species).
    let max_entropy = (species.len() as f32).ln();
    if max_entropy > 0.0 {
        (entropy / max_entropy).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_is_bounded_and_monotone_in_density() {
        let mut last = -1.0f32;
        for count in [0usize, 1, 2, 4, 8, 16, 64] {
            let d = density_term(count);
            let s = combine_resonance(d, 0.5, 0.5, 0.5, 0.9);
            assert!((0.0..=1.0).contains(&s), "strength {s} out of range");
            assert!(s >= last, "strength not monotone in node density");
            last = s;
        }
    }

    #[test]
    fn gated_rate_is_zero_when_travel_or_resonance_is_zero() {
        // Zero travel ⇒ still world regardless of resonance (ADR 0006).
        assert_eq!(gated_rate(0.02, 0.0, 1.0, 1.0, 0.25), 0.0);
        // Zero resonance ⇒ barren region holds regardless of travel (ADR 0012).
        assert_eq!(gated_rate(0.02, 100.0, 0.0, 1.0, 0.25), 0.0);
        // Both positive ⇒ a positive, capped rate.
        let rate = gated_rate(0.02, 100.0, 0.5, 1.0, 0.25);
        assert!(rate > 0.0 && rate <= 0.25);
        // The cap holds under a huge travel.
        assert_eq!(gated_rate(0.02, 1.0e9, 1.0, 1.0, 0.25), 0.25);
    }

    #[test]
    fn transition_scale_slows_the_rate() {
        let free = gated_rate(0.02, 100.0, 1.0, 1.0, 1.0);
        let transition = gated_rate(0.02, 100.0, 1.0, 0.4, 1.0);
        assert!(transition < free);
    }

    #[test]
    fn entropy_rewards_variety() {
        let node = |species| ResonanceNode {
            world_pos: (0.0, 0.0),
            species,
            distance: 1.0,
        };
        // A single-species crowd has zero diversity.
        let uniform = [node(1), node(1), node(1)];
        assert_eq!(species_entropy(&uniform), 0.0);
        // A varied set has positive diversity.
        let varied = [node(1), node(2), node(3)];
        assert!(species_entropy(&varied) > 0.5);
    }
}
