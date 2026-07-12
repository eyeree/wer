//! Routes through possibility space — the pure math
//! (implementation-plan.md section 13; phase-5-plan.md §4.4, §7.4; ADR 0015).
//!
//! A recorded route projects a **soft attraction field**, not exact replay:
//! its nodes become derived weak `Emphasize` anchors riding the existing
//! order-independent steering algebra (`steer` → `project_plausible`), so
//! route influence composes with player anchors, obeys plausibility
//! projection, and stays travel-fueled and resonance-gated — there is no
//! second steering system to keep coherent. Attraction strength saturates
//! with usage ("frequently used routes become easier to follow", Overview)
//! and the complete selected route channel is capped well below 1 so routes
//! bias but can never force a region to a remembered state.
//!
//! Everything here is pure and portable: routes are quantized records
//! (ADR 0013), so the same record attracts identically on every platform.

use crate::anchor::{anchor_peak_profile, Anchor, AnchorKind, AnchorSource};
use crate::record::{PossibilitySignature, RouteNode, RouteRecord};

// Preserve the Phase 5 public module path while `anchor` owns the single
// canonical steering projection and implementation (ADR 0025).
pub use crate::anchor::anchor_set_signature;

/// World-space radius of a route's attraction corridor: beyond this distance
/// from a node the route has no influence at all (section 13's "soft
/// attraction field" has edges).
pub const ROUTE_CORRIDOR_RADIUS: f64 = 768.0;

/// The ceiling on the complete selected route channel's combined peak pull.
/// Deliberately ≪ 1 (ADR 0026): all selected nodes across all routes share
/// this one budget, while explicit player anchors compose outside it.
pub const ROUTE_PULL_CAP: f32 = 0.35;

/// Contract-sized deterministic search bound for route normalization.
const ROUTE_SCALE_BISECTION_STEPS: usize = 32;

/// Usage count at which a route reaches half its maximum pull.
const ROUTE_PULL_HALF_USAGE: f32 = 4.0;

/// The domains a route's attraction may steer: the **fast domains only**.
/// Geology and Planetary are excluded so a followed route recreates the
/// remembered corridor's living character — climate, water expression, life,
/// its look and behaviour — without ever moving mountains or drainage
/// topology (section 9's stable-topology rule, and the precision invariant
/// the vault harness machine-checks: persisted influence never regenerates
/// the stable trio).
pub const ROUTE_ATTRACTION_MASK: u8 = {
    let geology = 1u8 << crate::possibility::PossibilityDomain::Geology.index();
    let planetary = 1u8 << crate::possibility::PossibilityDomain::Planetary.index();
    !geology & !planetary
};

/// A route candidate's raw attraction strength before selected-group
/// normalization. It is monotone in usage, nonzero at zero usage, and bounded
/// by [`ROUTE_PULL_CAP`]. A singleton retains these bits; overlapping selected
/// nodes share the aggregate ceiling through [`attraction_anchors`].
#[inline]
#[must_use]
pub fn route_pull(usage: u32) -> f32 {
    let u = usage as f32;
    ROUTE_PULL_CAP * (0.35 + 0.65 * (u / (u + ROUTE_PULL_HALF_USAGE)))
}

/// A route's difficulty, `0..=1`: the mean recorded transition cost of its
/// nodes. Cost was banded from `1 − resonance` at record time, so difficulty
/// falls out of the world model — a route through barren, low-resonance
/// ground is hard; one through dense living ground is easy (section 13).
#[must_use]
pub fn route_difficulty(nodes: &[RouteNode]) -> f32 {
    if nodes.is_empty() {
        return 0.0;
    }
    let sum: f32 = nodes.iter().map(|n| f32::from(n.cost_q) / 255.0).sum();
    sum / nodes.len() as f32
}

/// The derived anchors through which active routes attract (phase-5-plan.md
/// §7.4): every node of every route within [`ROUTE_CORRIDOR_RADIUS`] of the
/// player becomes a weak [`ROUTE_ATTRACTION_MASK`]-masked `Emphasize` anchor
/// toward the node's recorded possibility state, capped at `max_nodes`
/// nearest-first with a deterministic total tiebreak. After truncation, every
/// selected occurrence is scaled by one common factor when necessary so the
/// canonical saturating peak `1 - product(1 - strength)` is at most
/// [`ROUTE_PULL_CAP`] in every affected domain (ADR 0026). Spatial influence
/// cannot exceed peak strength, so the bound holds at every evaluation point.
/// Explicit anchors are not included in this route-only budget.
#[must_use]
pub fn attraction_anchors<'a>(
    routes: impl IntoIterator<Item = &'a RouteRecord>,
    player: (f64, f64),
    max_nodes: usize,
) -> Vec<Anchor> {
    let radius2 = ROUTE_CORRIDOR_RADIUS * ROUTE_CORRIDOR_RADIUS;
    // (distance bits, route id, node index) keys a deterministic order.
    let mut candidates: Vec<(u64, u64, usize, &RouteNode, u32)> = Vec::new();
    for route in routes {
        for (index, node) in route.nodes.iter().enumerate() {
            let dx = node.pos_q.0 as f64 - player.0;
            let dy = node.pos_q.1 as f64 - player.1;
            let d2 = dx * dx + dy * dy;
            if d2 <= radius2 {
                candidates.push((d2.to_bits(), route.id, index, node, route.usage));
            }
        }
    }
    candidates.sort_unstable_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    candidates.truncate(max_nodes);
    let anchors: Vec<_> = candidates
        .into_iter()
        .map(|(_, _, _, node, usage)| Anchor {
            world_pos: (node.pos_q.0 as f64, node.pos_q.1 as f64),
            target: node.signature.dequantize(),
            // A route remembers its moment's *fast* possibility state; the
            // stable-topology domains are never steered by a corridor.
            mask: ROUTE_ATTRACTION_MASK,
            kind: AnchorKind::Emphasize,
            strength: route_pull(usage),
            falloff_radius: ROUTE_CORRIDOR_RADIUS,
            source: AnchorSource::Manual,
        })
        .collect();
    normalize_route_anchors(anchors)
}

fn route_peak(anchors: &[Anchor]) -> f32 {
    let profile = anchor_peak_profile(anchors);
    profile
        .iter()
        .enumerate()
        .filter_map(|(domain, &peak)| {
            (ROUTE_ATTRACTION_MASK & (1 << domain as u8) != 0).then_some(peak)
        })
        .fold(0.0f32, f32::max)
}

fn normalize_route_anchors(raw: Vec<Anchor>) -> Vec<Anchor> {
    if route_peak(&raw) <= ROUTE_PULL_CAP {
        return raw;
    }

    let raw_strengths: Vec<f32> = raw.iter().map(|anchor| anchor.strength).collect();
    let mut safe_scale = 0.0f32;
    let mut unsafe_scale = 1.0f32;
    let mut trial = raw.clone();
    let mut safe = raw.clone();
    for anchor in &mut safe {
        anchor.strength = 0.0;
    }

    for _ in 0..ROUTE_SCALE_BISECTION_STEPS {
        let mid = safe_scale + (unsafe_scale - safe_scale) * 0.5;
        for (anchor, &strength) in trial.iter_mut().zip(&raw_strengths) {
            anchor.strength = strength * mid;
        }
        if route_peak(&trial) <= ROUTE_PULL_CAP {
            safe_scale = mid;
            safe.clone_from(&trial);
        } else {
            unsafe_scale = mid;
        }
    }
    debug_assert!(route_peak(&safe) <= ROUTE_PULL_CAP);
    safe
}

/// A rebuilt, in-memory index of recorded routes by their possibility-space
/// position (section 13's route graph) — a *view* over the records, never
/// persisted itself. Answers "which recorded corridors pass near this
/// possibility state" for the inspector (phase-5-plan.md §11).
#[derive(Debug, Default)]
pub struct RouteGraph {
    /// (signature seed, route id, node index, signature) per node.
    nodes: Vec<(u64, u64, usize, PossibilitySignature)>,
}

/// One route-graph query hit: a recorded node near the queried possibility
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteGraphHit {
    /// The route the node belongs to.
    pub route: u64,
    /// The node's index along the route.
    pub node: usize,
    /// L1 bucket distance from the queried signature (0 = same buckets).
    pub distance: u32,
}

impl RouteGraph {
    /// Build the view from a set of records.
    #[must_use]
    pub fn build<'a>(routes: impl IntoIterator<Item = &'a RouteRecord>) -> Self {
        let mut nodes = Vec::new();
        for route in routes {
            for (index, node) in route.nodes.iter().enumerate() {
                nodes.push((node.signature.seed(), route.id, index, node.signature));
            }
        }
        nodes.sort_unstable();
        Self { nodes }
    }

    /// Total indexed nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph indexes no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The `k` recorded nodes nearest a possibility state (L1 over buckets),
    /// deterministically ordered — "searching for target ecosystems" as an
    /// inspector query (§1.4: read-only in Phase 5).
    #[must_use]
    pub fn near_possibility(&self, sig: PossibilitySignature, k: usize) -> Vec<RouteGraphHit> {
        let mut hits: Vec<RouteGraphHit> = self
            .nodes
            .iter()
            .map(|&(_, route, node, node_sig)| RouteGraphHit {
                route,
                node,
                distance: sig
                    .buckets
                    .iter()
                    .zip(node_sig.buckets)
                    .map(|(&a, b)| u32::from(a.abs_diff(b)))
                    .sum(),
            })
            .collect();
        hits.sort_unstable_by(|a, b| {
            a.distance
                .cmp(&b.distance)
                .then_with(|| a.route.cmp(&b.route))
                .then_with(|| a.node.cmp(&b.node))
        });
        hits.truncate(k);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::bound_target;
    use crate::possibility::{PossibilityDomain, PossibilityVector};

    fn node_at(x: i64, ecology_bucket: u16, cost: u8) -> RouteNode {
        let mut signature = PossibilitySignature::of(PossibilityVector::neutral());
        signature.buckets[PossibilityDomain::Ecology.index()] = ecology_bucket;
        RouteNode {
            pos_q: (x, 0),
            signature,
            cost_q: cost,
            stability_q: 0,
            anchor_sig: 0,
        }
    }

    fn route_with(nodes: Vec<RouteNode>, usage: u32) -> RouteRecord {
        let mut route = RouteRecord::new(nodes, vec![], 1, "r".into());
        route.usage = usage;
        route
    }

    #[test]
    fn canonical_anchor_signature_preserves_multiplicity_and_exact_fields() {
        let mask = crate::anchor::domain_mask(&[PossibilityDomain::Ecology]);
        let a = Anchor {
            world_pos: (10.0, 0.0),
            target: bound_target(mask, 0.9),
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.7,
            falloff_radius: 500.0,
            source: AnchorSource::Manual,
        };
        let b = Anchor {
            world_pos: (-40.0, 25.0),
            target: bound_target(mask, 0.2),
            mask,
            kind: AnchorKind::Suppress,
            strength: 0.5,
            falloff_radius: 700.0,
            source: AnchorSource::Manual,
        };
        assert_eq!(anchor_set_signature(&[a, b]), anchor_set_signature(&[b, a]));
        assert_ne!(anchor_set_signature(&[a]), anchor_set_signature(&[b]));
        assert_ne!(
            anchor_set_signature(&[a, b]),
            anchor_set_signature(&[a]),
            "adding an anchor must move the signature"
        );
        assert_ne!(anchor_set_signature(&[]), anchor_set_signature(&[a]));
        assert_ne!(anchor_set_signature(&[a]), anchor_set_signature(&[a, a]));
        assert_ne!(
            anchor_set_signature(&[a, a]),
            anchor_set_signature(&[a, a, a])
        );

        let mut changed = a;
        changed.falloff_radius = f64::from_bits(a.falloff_radius.to_bits() + 1);
        assert_ne!(anchor_set_signature(&[a]), anchor_set_signature(&[changed]));
        changed = a;
        changed.world_pos.0 = f64::from_bits(a.world_pos.0.to_bits() + 1);
        assert_ne!(anchor_set_signature(&[a]), anchor_set_signature(&[changed]));
        changed = a;
        changed.target.set(
            PossibilityDomain::Ecology,
            f32::from_bits(a.target.get(PossibilityDomain::Ecology).to_bits() + 1),
        );
        assert_ne!(anchor_set_signature(&[a]), anchor_set_signature(&[changed]));

        let mut metadata_only = a;
        metadata_only.source = AnchorSource::River;
        metadata_only.target.set(PossibilityDomain::Climate, 0.99);
        assert_eq!(
            anchor_set_signature(&[a]),
            anchor_set_signature(&[metadata_only])
        );
    }

    #[test]
    fn route_pull_is_monotone_saturating_and_capped() {
        let mut last = 0.0f32;
        for usage in [0u32, 1, 2, 4, 8, 100, 100_000] {
            let pull = route_pull(usage);
            assert!(pull > 0.0 && pull <= ROUTE_PULL_CAP + 1e-6);
            assert!(pull >= last, "pull must be monotone in usage");
            last = pull;
        }
        assert!(route_pull(0) > 0.0, "a fresh shared route is followable");
    }

    #[test]
    fn aggregate_route_pull_is_one_global_worst_case_cap() {
        use crate::anchor::{anchor_influence_profile, steer};

        let nodes_a: Vec<_> = (0..16).map(|_| node_at(0, 4095, 10)).collect();
        let nodes_b: Vec<_> = (0..16).map(|_| node_at(0, 4095, 20)).collect();
        let route_a = route_with(nodes_a, u32::MAX);
        let route_b = route_with(nodes_b, u32::MAX - 1);
        assert_ne!(route_a.id, route_b.id);

        let anchors = attraction_anchors([&route_b, &route_a], (0.0, 0.0), 32);
        assert_eq!(anchors.len(), 32);
        assert!(route_peak(&anchors) <= ROUTE_PULL_CAP);
        assert!(anchors.iter().all(|anchor| {
            anchor.kind == AnchorKind::Emphasize
                && anchor.mask == ROUTE_ATTRACTION_MASK
                && anchor.falloff_radius == ROUTE_CORRIDOR_RADIUS
        }));

        // Co-location attains the peak. Give the already-normalized derived
        // anchors an exact all-one target so steering directly exposes the
        // exact saturating route pull end to end.
        let zero = PossibilityVector {
            dims: [0.0; crate::POSSIBILITY_DIMS],
        };
        let ecology = PossibilityDomain::Ecology.index();
        let profile = anchor_influence_profile(&anchors, (0.0, 0.0));
        let mut unit_target_anchors = anchors.clone();
        for anchor in &mut unit_target_anchors {
            anchor.target = bound_target(ROUTE_ATTRACTION_MASK, 1.0);
        }
        let steered = steer(zero, &unit_target_anchors, (0.0, 0.0));
        assert_eq!(steered.dims[ecology].to_bits(), profile[ecology].to_bits());
        assert!(profile[ecology] <= ROUTE_PULL_CAP);

        for at in [
            (0.0, 0.0),
            (128.0, 0.0),
            (384.0, 0.0),
            (ROUTE_CORRIDOR_RADIUS, 0.0),
            (ROUTE_CORRIDOR_RADIUS + 1.0, 0.0),
        ] {
            assert!(
                anchor_influence_profile(&anchors, at)[ecology] <= ROUTE_PULL_CAP,
                "route channel exceeded cap at {at:?}"
            );
        }

        let reversed = attraction_anchors([&route_a, &route_b], (0.0, 0.0), 32);
        assert_eq!(anchors, reversed);
        assert_eq!(
            anchors
                .iter()
                .map(|a| a.strength.to_bits())
                .collect::<Vec<_>>(),
            reversed
                .iter()
                .map(|a| a.strength.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn route_normalization_preserves_safe_bits_order_and_common_scale() {
        assert!(attraction_anchors([], (0.0, 0.0), 32).is_empty());
        let singleton = route_with(vec![node_at(0, 3500, 0)], 17);
        let one = attraction_anchors([&singleton], (0.0, 0.0), 1);
        assert_eq!(one[0].strength.to_bits(), route_pull(17).to_bits());
        assert!(attraction_anchors([&singleton], (0.0, 0.0), 0).is_empty());

        // Two small synthetic raw occurrences stay under the aggregate cap
        // and therefore keep their strength bits and nearest-first order.
        let template = one[0];
        let safe_raw = vec![
            Anchor {
                world_pos: (1.0, 0.0),
                strength: 0.1,
                ..template
            },
            Anchor {
                world_pos: (2.0, 0.0),
                strength: 0.2,
                ..template
            },
        ];
        let safe = normalize_route_anchors(safe_raw.clone());
        assert_eq!(safe, safe_raw);

        let dense_raw: Vec<_> = (0..8)
            .map(|i| Anchor {
                world_pos: (i as f64, 0.0),
                strength: if i % 2 == 0 { 0.2 } else { 0.3 },
                ..template
            })
            .collect();
        let dense = normalize_route_anchors(dense_raw.clone());
        assert!(route_peak(&dense) <= ROUTE_PULL_CAP);
        assert_eq!(dense, normalize_route_anchors(dense_raw.clone()));
        for pair in dense.windows(2).zip(dense_raw.windows(2)) {
            let (scaled, raw) = pair;
            assert_eq!(
                scaled[0].strength.total_cmp(&scaled[1].strength),
                raw[0].strength.total_cmp(&raw[1].strength)
            );
            assert_eq!(scaled[0].world_pos, raw[0].world_pos);
        }
    }

    #[test]
    fn aggregate_pull_is_monotone_through_saturation() {
        let nodes: Vec<_> = (0..8).map(|_| node_at(0, 4095, 0)).collect();
        let mut previous = 0.0f32;
        for usage in [0, 1, 2, 4, 8, 100, u32::MAX] {
            let route = route_with(nodes.clone(), usage);
            let peak = route_peak(&attraction_anchors([&route], (0.0, 0.0), 8));
            assert!(peak <= ROUTE_PULL_CAP);
            assert!(
                peak >= previous,
                "aggregate pull decreased at usage {usage}: {previous:?} -> {peak:?}"
            );
            previous = peak;
        }
    }

    #[test]
    fn route_difficulty_is_the_mean_cost_band() {
        assert_eq!(route_difficulty(&[]), 0.0);
        let nodes = [node_at(0, 2048, 0), node_at(10, 2048, 255)];
        let d = route_difficulty(&nodes);
        assert!((d - 0.5).abs() < 1e-3);
    }

    #[test]
    fn attraction_is_corridor_bounded_capped_and_deterministic() {
        let near = node_at(100, 3500, 10);
        let far = node_at((ROUTE_CORRIDOR_RADIUS as i64) * 3, 3500, 10);
        let route = route_with(vec![near, far], 5);
        let anchors = attraction_anchors([&route], (0.0, 0.0), 8);
        assert_eq!(
            anchors.len(),
            1,
            "nodes beyond the corridor contribute nothing"
        );
        assert_eq!(anchors[0].strength, route_pull(5));
        assert!(anchors[0].strength <= ROUTE_PULL_CAP);

        // The cap keeps a dense route bounded, nearest-first.
        let many: Vec<RouteNode> = (0..20).map(|i| node_at(i * 30, 3000, 5)).collect();
        let dense = route_with(many, 1);
        let capped = attraction_anchors([&dense], (0.0, 0.0), 4);
        assert_eq!(capped.len(), 4);
        // Deterministic: same inputs, same output.
        assert_eq!(capped, attraction_anchors([&dense], (0.0, 0.0), 4));
    }

    #[test]
    fn attraction_bends_the_target_softly_toward_the_route() {
        use crate::anchor::steer;
        let node = node_at(0, 3600, 10); // a lusher world than neutral
        let route = route_with(vec![node], 6);
        let anchors = attraction_anchors([&route], (50.0, 0.0), 8);
        let base = PossibilityVector::neutral();
        let steered = steer(base, &anchors, (50.0, 0.0));
        let ecology = steered.get(PossibilityDomain::Ecology);
        let node_value = PossibilityVector::dequantize(3600);
        // Soft: strictly between the base and the recorded value — a route
        // biases, it never replays (ADR 0015).
        assert!(ecology > 0.5, "route must pull toward its recorded state");
        assert!(
            ecology < node_value,
            "route must never force the recorded state exactly"
        );
        // Monotone in usage: a well-worn route pulls harder.
        let worn = route_with(vec![node], 50);
        let worn_anchors = attraction_anchors([&worn], (50.0, 0.0), 8);
        let worn_ecology = steer(base, &worn_anchors, (50.0, 0.0)).get(PossibilityDomain::Ecology);
        assert!(worn_ecology > ecology);
        // Beyond the corridor: untouched.
        let far_at = (ROUTE_CORRIDOR_RADIUS * 2.0, 0.0);
        let far_anchors = attraction_anchors([&route], far_at, 8);
        assert!(far_anchors.is_empty());
    }

    #[test]
    fn attraction_never_steers_the_stable_topology_domains() {
        // Section 9: a followed route recreates the corridor's living
        // character, never its mountains. The mask must exclude Geology and
        // Planetary, and steering through it must leave them untouched.
        use crate::anchor::steer;
        assert_eq!(
            ROUTE_ATTRACTION_MASK & (1 << PossibilityDomain::Geology.index()),
            0
        );
        assert_eq!(
            ROUTE_ATTRACTION_MASK & (1 << PossibilityDomain::Planetary.index()),
            0
        );
        let mut signature = PossibilitySignature::of(PossibilityVector::neutral());
        signature.buckets[PossibilityDomain::Geology.index()] = 4000;
        signature.buckets[PossibilityDomain::Planetary.index()] = 100;
        signature.buckets[PossibilityDomain::Ecology.index()] = 3600;
        let node = RouteNode {
            pos_q: (0, 0),
            signature,
            cost_q: 10,
            stability_q: 0,
            anchor_sig: 0,
        };
        let route = route_with(vec![node], 10);
        let anchors = attraction_anchors([&route], (0.0, 0.0), 8);
        let mut base = PossibilityVector::neutral();
        base.set(PossibilityDomain::Geology, 0.7);
        base.set(PossibilityDomain::Planetary, 0.3);
        let steered = steer(base, &anchors, (0.0, 0.0));
        assert_eq!(steered.get(PossibilityDomain::Geology), 0.7);
        assert_eq!(steered.get(PossibilityDomain::Planetary), 0.3);
        assert!(steered.get(PossibilityDomain::Ecology) > 0.5);
    }

    #[test]
    fn route_graph_finds_nearby_possibility_states() {
        let lush = route_with(vec![node_at(0, 3800, 10)], 0);
        let arid = route_with(vec![node_at(500, 300, 200)], 0);
        let graph = RouteGraph::build([&lush, &arid]);
        assert_eq!(graph.len(), 2);
        let mut query = PossibilitySignature::of(PossibilityVector::neutral());
        query.buckets[PossibilityDomain::Ecology.index()] = 3700;
        let hits = graph.near_possibility(query, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].route, lush.id,
            "the lush route is nearer in possibility space"
        );
    }
}
