//! Route recording and traversal tracking (phase-5-plan.md §7.3–7.4).
//!
//! The [`RouteRecorder`] samples the journey into quantized
//! [`RouteNode`]s at fixed travel intervals — every input is frame state the
//! continuity replay already reproduces, so recording is deterministic per
//! run. The [`RouteTracker`] detects when the player re-walks a recorded
//! route (enough of its corridor visited in one leg) so the vault can bump
//! its usage count — "frequently used routes become easier to follow"
//! (Overview). Both are bounded, transient runtime state; the records are
//! the truth (ADR 0015).

use std::collections::{BTreeMap, BTreeSet};

use world_core::{
    anchor_set_signature, Anchor, PossibilitySignature, RegionCoord, RouteNode, RouteRecord,
    ROUTE_CORRIDOR_RADIUS,
};

use crate::stream::RegionMap;

/// World units of travel between recorded nodes.
pub const ROUTE_SAMPLE_SPACING: f64 = 192.0;

/// Nodes a single recording may hold before it stops sampling (a runaway
/// recorder must not grow a record without bound; §6.2's non-accumulation).
pub const MAX_ROUTE_NODES: usize = 1024;

/// Fraction of a route's nodes that must be visited in one leg to count as a
/// traversal.
const TRAVERSAL_FRACTION: f32 = 0.6;

/// Samples the journey into route nodes (phase-5-plan.md §7.3). Feed it once
/// per frame while recording; every input is deterministic frame state.
#[derive(Debug, Default)]
pub struct RouteRecorder {
    accumulated: f64,
    nodes: Vec<RouteNode>,
    discoveries: Vec<u64>,
}

impl RouteRecorder {
    /// Start a fresh recording.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Nodes recorded so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether nothing has been recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Attach a discovery made along the way (its record id becomes part of
    /// the expedition).
    pub fn attach_discovery(&mut self, id: u64) {
        if !self.discoveries.contains(&id) {
            self.discoveries.push(id);
        }
    }

    /// Observe one frame: accumulate travel and emit a node every
    /// [`ROUTE_SAMPLE_SPACING`] units. The node captures the covering
    /// region's steered *target* (the possibility-space coordinate), the
    /// frame's authoritative transition cost (`1 − resonance` — difficulty
    /// falls out of canonical slot-0 gameplay under ADR 0024), the region's
    /// stability, and the order-independent
    /// signature of the active anchor set (section 13's node shape).
    /// `resonance_strength` must be the canonical [`crate::FrameStats`] value
    /// returned by the immediately preceding map update for this observation,
    /// never a separately sampled visual-density statistic.
    pub fn observe(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        travel: f64,
        anchors: &[Anchor],
        resonance_strength: f32,
    ) {
        if self.nodes.len() >= MAX_ROUTE_NODES {
            return;
        }
        self.accumulated += travel;
        let due = if self.nodes.is_empty() {
            true // the first node drops where recording starts
        } else {
            self.accumulated >= ROUTE_SAMPLE_SPACING
        };
        if !due {
            return;
        }
        let coord = RegionCoord::from_world(player.0, player.1);
        let Some(region) = map.get(coord) else {
            return; // keep accumulating until the ground under us is resident
        };
        self.accumulated = 0.0;
        self.nodes.push(RouteNode {
            pos_q: (player.0.round() as i64, player.1.round() as i64),
            signature: PossibilitySignature::of(region.target),
            cost_q: (((1.0 - resonance_strength.clamp(0.0, 1.0)) * 255.0) as u8),
            stability_q: ((region.stability.clamp(0.0, 1.0) * 255.0) as u8),
            anchor_sig: anchor_set_signature(anchors),
        });
    }

    /// Close the recording into `(nodes, discoveries)` for
    /// [`crate::vault::Vault::record_route`]. Empty if nothing was sampled.
    #[must_use]
    pub fn finish(self) -> (Vec<RouteNode>, Vec<u64>) {
        (self.nodes, self.discoveries)
    }
}

/// Detects route traversals (phase-5-plan.md §7.4): a *leg* is one continuous
/// stay inside a route's corridor; when the player leaves the corridor, the
/// leg ends and counts as a traversal if enough distinct nodes were covered.
/// Firing on exit debounces naturally — standing on a route, or oscillating
/// within its corridor, can never bump usage more than once per leg.
#[derive(Debug, Default)]
pub struct RouteTracker {
    /// Per route: the nodes covered during the current leg (empty when the
    /// player is outside the corridor).
    visited: BTreeMap<u64, BTreeSet<usize>>,
}

impl RouteTracker {
    /// A fresh tracker.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one frame against the active route set. Returns the ids of
    /// routes whose leg just ended as a completed traversal (usage bumps for
    /// the vault), in deterministic order.
    pub fn observe<'a>(
        &mut self,
        routes: impl IntoIterator<Item = &'a RouteRecord>,
        player: (f64, f64),
    ) -> Vec<u64> {
        let radius2 = ROUTE_CORRIDOR_RADIUS * ROUTE_CORRIDOR_RADIUS;
        let mut traversed = Vec::new();
        for route in routes {
            if route.nodes.is_empty() {
                continue;
            }
            let mut inside = false;
            let mut near_nodes: Vec<usize> = Vec::new();
            for (index, node) in route.nodes.iter().enumerate() {
                let dx = node.pos_q.0 as f64 - player.0;
                let dy = node.pos_q.1 as f64 - player.1;
                if dx * dx + dy * dy <= radius2 {
                    inside = true;
                    near_nodes.push(index);
                }
            }
            if inside {
                self.visited.entry(route.id).or_default().extend(near_nodes);
            } else if let Some(visited) = self.visited.remove(&route.id) {
                // The leg just ended: a traversal if enough of the corridor
                // was covered while inside.
                let needed =
                    ((route.nodes.len() as f32 * TRAVERSAL_FRACTION).ceil() as usize).max(1);
                if visited.len() >= needed {
                    traversed.push(route.id);
                }
            }
        }
        traversed
    }

    /// Drop tracking state for routes that no longer exist.
    pub fn retain(&mut self, exists: impl Fn(u64) -> bool) {
        self.visited.retain(|id, _| exists(*id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight_route(nodes: usize, spacing: f64) -> RouteRecord {
        let nodes: Vec<RouteNode> = (0..nodes)
            .map(|i| RouteNode {
                pos_q: ((i as f64 * spacing) as i64, 0),
                signature: PossibilitySignature::of(world_core::PossibilityVector::neutral()),
                cost_q: 10,
                stability_q: 0,
                anchor_sig: 0,
            })
            .collect();
        RouteRecord::new(nodes, vec![], 1, "straight".into())
    }

    #[test]
    fn traversal_fires_once_per_leg_on_corridor_exit() {
        let route = straight_route(5, 400.0);
        let mut tracker = RouteTracker::new();
        // Walk the route end to end, lingering: nothing fires while inside.
        let mut bumps = 0;
        for step in 0..5 {
            let at = (f64::from(step) * 400.0, 0.0);
            bumps += tracker.observe([&route], at).len();
            bumps += tracker.observe([&route], at).len(); // linger a frame
        }
        assert_eq!(bumps, 0, "a leg fires on exit, never while inside");
        // Leave the corridor: the completed leg counts exactly once.
        let away = (1600.0, ROUTE_CORRIDOR_RADIUS * 3.0);
        bumps += tracker.observe([&route], away).len();
        assert_eq!(bumps, 1, "one traversal per walked leg");
        bumps += tracker.observe([&route], away).len();
        assert_eq!(bumps, 1, "staying outside must not re-fire");
        // A brief touch (too little coverage) ends a leg without a traversal.
        tracker.observe([&route], (0.0, 0.0));
        let touch_exit = tracker.observe([&route], away);
        assert!(touch_exit.is_empty(), "a brief touch is not a traversal");
    }

    #[test]
    fn far_away_walking_never_traverses() {
        let route = straight_route(5, 400.0);
        let mut tracker = RouteTracker::new();
        for step in 0..20 {
            let at = (f64::from(step) * 400.0, ROUTE_CORRIDOR_RADIUS * 3.0);
            assert!(tracker.observe([&route], at).is_empty());
        }
    }
}
