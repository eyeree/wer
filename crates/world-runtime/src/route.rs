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
    RouteRecorderSnapshot, RouteTrackerLegSnapshot, RouteTrackerSnapshot, ROUTE_CORRIDOR_RADIUS,
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
    last_observed: Option<(f64, f64)>,
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

    /// Snapshot active recorder state into the session tier.
    #[must_use]
    pub fn snapshot(&self) -> RouteRecorderSnapshot {
        RouteRecorderSnapshot {
            accumulated: self.accumulated,
            last_observed: self.last_observed,
            nodes: self.nodes.clone(),
            discoveries: self.discoveries.clone(),
        }
    }

    /// Restore active recorder state from a session snapshot.
    #[must_use]
    pub fn from_snapshot(snapshot: RouteRecorderSnapshot) -> Self {
        Self {
            accumulated: snapshot.accumulated,
            last_observed: snapshot.last_observed,
            nodes: snapshot.nodes,
            discoveries: snapshot.discoveries,
        }
    }

    /// Observe one frame: accumulate travel and emit a node every
    /// [`ROUTE_SAMPLE_SPACING`] units. The node captures the covering
    /// region's steered *target* (the possibility-space coordinate), the
    /// frame's authoritative transition cost (`1 − resonance` — difficulty
    /// falls out of canonical slot-0 gameplay under ADR 0024), the region's
    /// stability, and the canonical signature of the effective anchor
    /// multiset (section 13's node shape). `effective_anchors` must be the
    /// exact explicit-plus-derived slice supplied to the immediately preceding
    /// map update; passing a different slice violates this API contract.
    /// `resonance_strength` must be the canonical [`crate::FrameStats`] value
    /// returned by the immediately preceding map update for this observation,
    /// never a separately sampled visual-density statistic.
    pub fn observe(
        &mut self,
        map: &RegionMap,
        player: (f64, f64),
        _travel: f64,
        effective_anchors: &[Anchor],
        resonance_strength: f32,
    ) {
        if self.nodes.len() >= MAX_ROUTE_NODES {
            return;
        }

        let Some(mut segment_start) = self.last_observed else {
            if self.push_node(map, player, effective_anchors, resonance_strength, 0) {
                self.last_observed = Some(player);
            }
            return;
        };

        let mut segment_len = f64::hypot(player.0 - segment_start.0, player.1 - segment_start.1);
        if segment_len <= f64::EPSILON {
            return;
        }

        while self.accumulated + segment_len + f64::EPSILON >= ROUTE_SAMPLE_SPACING
            && self.nodes.len() < MAX_ROUTE_NODES
        {
            let distance_from_start = (ROUTE_SAMPLE_SPACING - self.accumulated).max(0.0);
            let t = (distance_from_start / segment_len).clamp(0.0, 1.0);
            let sample = (
                segment_start.0 + (player.0 - segment_start.0) * t,
                segment_start.1 + (player.1 - segment_start.1) * t,
            );
            if !self.push_node(
                map,
                sample,
                effective_anchors,
                resonance_strength,
                ROUTE_SAMPLE_SPACING.round() as u32,
            ) {
                return;
            }
            segment_start = sample;
            self.last_observed = Some(sample);
            self.accumulated = 0.0;
            segment_len = f64::hypot(player.0 - segment_start.0, player.1 - segment_start.1);
            if segment_len <= f64::EPSILON {
                self.last_observed = Some(player);
                return;
            }
        }

        self.accumulated += segment_len;
        if (self.accumulated - ROUTE_SAMPLE_SPACING).abs() <= f64::EPSILON {
            self.accumulated = ROUTE_SAMPLE_SPACING;
        }
        self.last_observed = Some(player);
    }

    fn push_node(
        &mut self,
        map: &RegionMap,
        at: (f64, f64),
        effective_anchors: &[Anchor],
        resonance_strength: f32,
        distance_q: u32,
    ) -> bool {
        let coord = RegionCoord::from_world(at.0, at.1);
        let Some(region) = map.get(coord) else {
            return false;
        };
        self.nodes.push(RouteNode {
            pos_q: (at.0.round() as i64, at.1.round() as i64),
            signature: PossibilitySignature::of(region.target),
            current_signature: Some(PossibilitySignature::of(region.current)),
            cost_q: (((1.0 - resonance_strength.clamp(0.0, 1.0)) * 255.0) as u8),
            stability_q: ((region.stability.clamp(0.0, 1.0) * 255.0) as u8),
            anchor_sig: anchor_set_signature(effective_anchors),
            distance_q,
        });
        true
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

    /// Snapshot current leg state into sorted session records.
    #[must_use]
    pub fn snapshot(&self) -> RouteTrackerSnapshot {
        RouteTrackerSnapshot {
            legs: self
                .visited
                .iter()
                .map(|(&route_id, visited)| RouteTrackerLegSnapshot {
                    route_id,
                    visited_nodes: visited.iter().map(|&node| node as u32).collect(),
                })
                .collect(),
        }
    }

    /// Restore current leg state from a session snapshot.
    #[must_use]
    pub fn from_snapshot(snapshot: RouteTrackerSnapshot) -> Self {
        let visited = snapshot
            .legs
            .into_iter()
            .map(|leg| {
                (
                    leg.route_id,
                    leg.visited_nodes
                        .into_iter()
                        .map(|node| node as usize)
                        .collect(),
                )
            })
            .collect();
        Self { visited }
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
    use world_core::{PossibilityVector, REGION_SIZE};

    fn map_with_regions(coords: impl IntoIterator<Item = RegionCoord>) -> RegionMap {
        let mut map = RegionMap::new(crate::StreamConfig::default());
        for coord in coords {
            let mut target = PossibilityVector::neutral();
            target.set(world_core::PossibilityDomain::Ecology, 0.75);
            map.restore_region(&world_core::RegionSnapshotRecord {
                coord,
                current: PossibilityVector::neutral().dims,
                target: target.dims,
                stability: 1.0,
                revision: 1,
            });
        }
        map
    }

    fn straight_route(nodes: usize, spacing: f64) -> RouteRecord {
        let nodes: Vec<RouteNode> = (0..nodes)
            .map(|i| RouteNode {
                pos_q: ((i as f64 * spacing) as i64, 0),
                signature: PossibilitySignature::of(world_core::PossibilityVector::neutral()),
                current_signature: None,
                cost_q: 10,
                stability_q: 0,
                anchor_sig: 0,
                distance_q: 0,
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

    #[test]
    fn recorder_emits_every_crossed_interval_and_carries_remainder() {
        let map = map_with_regions([
            RegionCoord::new(0, 0),
            RegionCoord::new(1, 0),
            RegionCoord::new(2, 0),
        ]);
        let mut recorder = RouteRecorder::new();
        recorder.observe(&map, (0.0, 0.0), 0.0, &[], 0.75);
        recorder.observe(&map, (700.0, 0.0), 700.0, &[], 0.75);

        let snap = recorder.snapshot();
        assert_eq!(snap.nodes.len(), 4);
        assert_eq!(snap.nodes[0].distance_q, 0);
        assert_eq!(snap.nodes[1].pos_q, (192, 0));
        assert_eq!(snap.nodes[2].pos_q, (384, 0));
        assert_eq!(snap.nodes[3].pos_q, (576, 0));
        assert_eq!(
            snap.nodes[1].distance_q,
            ROUTE_SAMPLE_SPACING.round() as u32
        );
        assert!((snap.accumulated - 124.0).abs() < 1e-9);
        assert!(snap.nodes[1].current_signature.is_some());
    }

    #[test]
    fn recorder_carries_overshoot_across_frames() {
        let map = map_with_regions([RegionCoord::new(0, 0), RegionCoord::new(1, 0)]);
        let mut recorder = RouteRecorder::new();
        recorder.observe(&map, (0.0, 0.0), 0.0, &[], 1.0);
        recorder.observe(&map, (100.0, 0.0), 100.0, &[], 1.0);
        assert_eq!(recorder.snapshot().nodes.len(), 1);
        recorder.observe(&map, (250.0, 0.0), 150.0, &[], 1.0);

        let snap = recorder.snapshot();
        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.nodes[1].pos_q, (192, 0));
        assert!((snap.accumulated - 58.0).abs() < 1e-9);
    }

    #[test]
    fn recorder_retries_missing_due_interval_without_moving_later_nodes_earlier() {
        let mut recorder = RouteRecorder::new();
        let only_start = map_with_regions([RegionCoord::new(0, 0)]);
        recorder.observe(&only_start, (0.0, 0.0), 0.0, &[], 1.0);
        recorder.observe(
            &only_start,
            (REGION_SIZE * 2.0, 0.0),
            REGION_SIZE * 2.0,
            &[],
            1.0,
        );
        let stalled = recorder.snapshot();
        assert_eq!(stalled.nodes.len(), 2);
        assert_eq!(stalled.nodes[1].pos_q, (192, 0));
        assert_eq!(stalled.last_observed, Some((192.0, 0.0)));
        assert_eq!(stalled.accumulated, 0.0);

        let with_next = map_with_regions([RegionCoord::new(0, 0), RegionCoord::new(1, 0)]);
        recorder.observe(&with_next, (REGION_SIZE * 2.0, 0.0), 0.0, &[], 1.0);
        let resumed = recorder.snapshot();
        assert_eq!(resumed.nodes.len(), 3);
        assert_eq!(resumed.nodes[2].pos_q, (384, 0));
    }

    #[test]
    fn recorder_snapshot_restore_continues_like_uninterrupted() {
        let map = map_with_regions([
            RegionCoord::new(0, 0),
            RegionCoord::new(1, 0),
            RegionCoord::new(2, 0),
        ]);
        let mut uninterrupted = RouteRecorder::new();
        uninterrupted.observe(&map, (0.0, 0.0), 0.0, &[], 0.5);
        uninterrupted.observe(&map, (250.0, 0.0), 250.0, &[], 0.5);

        let mut restored = RouteRecorder::new();
        restored.observe(&map, (0.0, 0.0), 0.0, &[], 0.5);
        restored.observe(&map, (100.0, 0.0), 100.0, &[], 0.5);
        restored = RouteRecorder::from_snapshot(restored.snapshot());
        restored.observe(&map, (250.0, 0.0), 150.0, &[], 0.5);

        assert_eq!(restored.snapshot(), uninterrupted.snapshot());
    }

    #[test]
    fn tracker_snapshot_restore_preserves_current_leg() {
        let route = straight_route(5, 400.0);
        let away = (1600.0, ROUTE_CORRIDOR_RADIUS * 3.0);
        let mut uninterrupted = RouteTracker::new();
        let mut restored = RouteTracker::new();
        for step in 0..3 {
            let at = (f64::from(step) * 400.0, 0.0);
            assert!(uninterrupted.observe([&route], at).is_empty());
            assert!(restored.observe([&route], at).is_empty());
        }
        restored = RouteTracker::from_snapshot(restored.snapshot());
        for step in 3..5 {
            let at = (f64::from(step) * 400.0, 0.0);
            assert!(uninterrupted.observe([&route], at).is_empty());
            assert!(restored.observe([&route], at).is_empty());
        }
        assert_eq!(
            restored.observe([&route], away),
            uninterrupted.observe([&route], away)
        );
    }
}
