//! Atlas bundles as files (phase-5-plan.md §5.3, §11; Overview "Community
//! Atlas"): the file-based proof of the sharing model. A bundle is one
//! enveloped [`AtlasBundle`] record — the shareable tier only — exchanged as
//! an ordinary file; `wer-atlas` exports, imports, validates, and lists them.
//! No server anywhere: merge needs no coordinator (ADR 0014).

use world_core::{
    decode_record, encode_record, AtlasBundle, Envelope, RecordError, RecordKind,
    RECORD_FORMAT_VERSION, WORLD_ALGORITHM_VERSION,
};

/// Encode a bundle for the wire/file (canonical form: records sorted by id).
#[must_use]
pub fn encode_bundle(mut bundle: AtlasBundle) -> Vec<u8> {
    bundle.canonicalize();
    encode_record(RecordKind::Bundle, &bundle)
}

/// Decode a bundle file, refusing future formats and wrong kinds.
pub fn decode_bundle(bytes: &[u8]) -> Result<(Envelope, AtlasBundle), RecordError> {
    decode_record(bytes, RecordKind::Bundle)
}

/// The `wer-atlas check` report for one bundle file.
#[derive(Debug)]
pub struct BundleCheck {
    /// The bundle's envelope (format + world versions).
    pub envelope: Envelope,
    /// Record counts: discoveries, routes, preserves.
    pub counts: (usize, usize, usize),
    /// Problems found. Empty means the bundle is valid and self-consistent.
    pub findings: Vec<String>,
}

impl BundleCheck {
    /// Whether the bundle passed every check.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Validate a bundle file: decodes, every content id matches its recomputed
/// fold, the merge self-test holds (importing a bundle into itself changes
/// nothing — idempotence), and version mismatches are surfaced as findings
/// (honest labels, not errors — phase-5-plan.md §7.2).
pub fn check_bundle(bytes: &[u8]) -> Result<BundleCheck, RecordError> {
    let (envelope, bundle) = decode_bundle(bytes)?;
    let mut findings = Vec::new();

    if envelope.world_version != WORLD_ALGORITHM_VERSION {
        findings.push(format!(
            "world algorithm v{} (this build is v{}): records keep their meaning, \
             but the same buckets realize a different world",
            envelope.world_version, WORLD_ALGORITHM_VERSION
        ));
    }
    if envelope.format_version < RECORD_FORMAT_VERSION {
        findings.push(format!(
            "older format v{} (current v{RECORD_FORMAT_VERSION}); still readable via migration",
            envelope.format_version
        ));
    }

    for r in &bundle.discoveries {
        if r.id != r.content_id() {
            findings.push(format!(
                "discovery {:#018x}: content id mismatch (corrupt or tampered)",
                r.id
            ));
        }
    }
    for r in &bundle.routes {
        if r.id != r.content_id() {
            findings.push(format!(
                "route {:#018x}: content id mismatch (corrupt or tampered)",
                r.id
            ));
        }
        if r.nodes.is_empty() {
            findings.push(format!("route {:#018x}: empty node path", r.id));
        }
    }
    for r in &bundle.preserves {
        if r.id != r.content_id() {
            findings.push(format!(
                "preserve {:#018x}: content id mismatch (corrupt or tampered)",
                r.id
            ));
        }
        if r.regions.is_empty() {
            findings.push(format!("preserve {:#018x}: empty region set", r.id));
        }
    }

    // Merge self-test (idempotence): a canonical bundle merged into itself is
    // itself. Cheap, and catches a broken canonical form immediately.
    let mut canonical = bundle.clone();
    canonical.canonicalize();
    if canonical != bundle {
        findings.push("bundle is not in canonical (id-sorted) form".into());
    }

    Ok(BundleCheck {
        envelope,
        counts: (
            bundle.discoveries.len(),
            bundle.routes.len(),
            bundle.preserves.len(),
        ),
        findings,
    })
}

/// Everything `wer-inspect --vault` reports for one position (phase-5-plan.md
/// §11): the records relevant to where the explorer is standing — the
/// persistence analogue of `--layers`.
#[derive(Debug)]
pub struct VaultPositionReport {
    /// Store totals: discoveries, routes, preserves, regions seen.
    pub totals: (usize, usize, usize, u64),
    /// The preserve pinning the covering region, with its buckets, if any.
    pub covering_preserve: Option<(u64, String, world_core::PossibilitySignature)>,
    /// Discoveries within reach: `(id, name, distance)`, nearest first.
    pub nearby_discoveries: Vec<(u64, String, f64)>,
    /// Route nodes whose corridor covers the position: `(route id, node
    /// index, distance)`, nearest first.
    pub nearby_route_nodes: Vec<(u64, usize, f64)>,
    /// Whether the covering region is in the discovered set.
    pub seen_here: bool,
    /// Non-fatal problems the vault reported on open.
    pub issues: Vec<String>,
}

/// Open a store and gather the records relevant to a world position.
pub fn inspect_vault(store_dir: &str, x: f64, y: f64) -> Result<VaultPositionReport, String> {
    use world_core::{RegionCoord, ROUTE_CORRIDOR_RADIUS};
    let store = crate::FileStorage::open(store_dir).map_err(|e| e.to_string())?;
    let vault = world_runtime::Vault::open(store).map_err(|e| e.to_string())?;
    let region = RegionCoord::from_world(x, y);
    let distance = |px: i64, py: i64| f64::hypot(px as f64 - x, py as f64 - y);

    let covering_preserve = vault.preserves().iter().find_map(|(&id, p)| {
        p.regions
            .iter()
            .find(|(c, _)| *c == region)
            .map(|&(_, sig)| (id, p.name.clone(), sig))
    });

    let mut nearby_discoveries: Vec<(u64, String, f64)> = vault
        .discoveries()
        .iter()
        .map(|(&id, r)| (id, r.name.clone(), distance(r.pos_q.0, r.pos_q.1)))
        .filter(|(_, _, d)| *d <= 4.0 * ROUTE_CORRIDOR_RADIUS)
        .collect();
    nearby_discoveries.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.0.cmp(&b.0)));

    let mut nearby_route_nodes: Vec<(u64, usize, f64)> = vault
        .routes()
        .iter()
        .flat_map(|(&id, r)| {
            r.nodes
                .iter()
                .enumerate()
                .map(move |(i, n)| (id, i, distance(n.pos_q.0, n.pos_q.1)))
        })
        .filter(|(_, _, d)| *d <= ROUTE_CORRIDOR_RADIUS)
        .collect();
    nearby_route_nodes.sort_by(|a, b| {
        a.2.total_cmp(&b.2)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    Ok(VaultPositionReport {
        totals: (
            vault.discoveries().len(),
            vault.routes().len(),
            vault.preserves().len(),
            vault.seen_count(),
        ),
        covering_preserve,
        nearby_discoveries,
        nearby_route_nodes,
        seen_here: vault.is_seen(region),
        issues: vault.issues().to_vec(),
    })
}

/// Everything `wer-inspect --routes` reports for one position: the route
/// graph queried in *possibility* space — which recorded corridors pass near
/// the possibility state this ground would realize (phase-5-plan.md §11).
#[derive(Debug)]
pub struct RouteQueryReport {
    /// The anchor-free possibility signature at the position (the query).
    pub signature: world_core::PossibilitySignature,
    /// Nearest recorded nodes: the hit plus its route's name and difficulty.
    pub hits: Vec<(world_core::RouteGraphHit, String, f32)>,
}

/// Open a store and query its route graph around a position's possibility
/// state.
pub fn inspect_routes(store_dir: &str, x: f64, y: f64) -> Result<RouteQueryReport, String> {
    use world_core::{
        route_difficulty, PossibilityField, PossibilitySignature, RegionCoord, RouteGraph,
    };
    let store = crate::FileStorage::open(store_dir).map_err(|e| e.to_string())?;
    let vault = world_runtime::Vault::open(store).map_err(|e| e.to_string())?;
    let region = RegionCoord::from_world(x, y);
    let signature = PossibilitySignature::of(PossibilityField::default().sample(region));
    let graph = RouteGraph::build(vault.routes().values());
    let hits = graph
        .near_possibility(signature, 8)
        .into_iter()
        .map(|hit| {
            let route = &vault.routes()[&hit.route];
            (hit, route.name.clone(), route_difficulty(&route.nodes))
        })
        .collect();
    Ok(RouteQueryReport { signature, hits })
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::{
        bound_target, domain_mask, Anchor, AnchorKind, AnchorSource, DiscoveryRecord,
        PossibilityDomain,
    };

    fn bundle_with_one_discovery() -> AtlasBundle {
        let mask = domain_mask(&[PossibilityDomain::Ecology]);
        let anchor = Anchor {
            world_pos: (10.0, 20.0),
            target: bound_target(mask, 0.9),
            mask,
            kind: AnchorKind::Emphasize,
            strength: 0.7,
            falloff_radius: 900.0,
            source: AnchorSource::River,
        };
        AtlasBundle {
            discoveries: vec![DiscoveryRecord::from_anchor(&anchor, 7, 1, "brook".into())],
            ..AtlasBundle::default()
        }
    }

    #[test]
    fn bundle_round_trips_and_checks_clean() {
        let bundle = bundle_with_one_discovery();
        let bytes = encode_bundle(bundle.clone());
        let (envelope, decoded) = decode_bundle(&bytes).unwrap();
        assert_eq!(envelope.kind, RecordKind::Bundle);
        assert_eq!(decoded, bundle);
        let check = check_bundle(&bytes).unwrap();
        assert!(check.passed(), "{:?}", check.findings);
        assert_eq!(check.counts, (1, 0, 0));
    }

    #[test]
    fn check_flags_tampering() {
        let mut bundle = bundle_with_one_discovery();
        bundle.discoveries[0].strength_q ^= 1;
        let bytes = encode_bundle(bundle);
        let check = check_bundle(&bytes).unwrap();
        assert!(!check.passed());
    }
}
