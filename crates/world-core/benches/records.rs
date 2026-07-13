//! Phase 5 record and route benches (phase-5-plan.md §13): codec throughput,
//! content-id folds, and attraction assembly — the numbers that calibrate
//! `max_persist_ops` and `max_route_attraction_nodes`.

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use world_core::{
    attraction_anchors, bound_target, decode_record, domain_mask, encode_record, Anchor,
    AnchorKind, AnchorSource, DiscoveryRecord, Envelope, PossibilityDomain, PossibilitySignature,
    PossibilityVector, RecordKind, RouteNode, RouteRecord,
};

fn sample_discovery() -> DiscoveryRecord {
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    let anchor = Anchor {
        world_pos: (300.0, -10.0),
        target: bound_target(mask, 0.9),
        mask,
        kind: AnchorKind::Emphasize,
        strength: 0.8,
        falloff_radius: 1500.0,
        source: AnchorSource::Organism {
            species: 0x2340_6061_75CD_D2D2,
        },
    };
    DiscoveryRecord::from_anchor(&anchor, 0x4204_1386_32E9_C315, 7, String::from("glowfin"))
}

fn long_route(nodes: usize) -> RouteRecord {
    let sig = PossibilitySignature::of(PossibilityVector::neutral());
    let nodes: Vec<RouteNode> = (0..nodes)
        .map(|i| RouteNode {
            pos_q: ((i as i64) * 192, (i as i64) * 7),
            signature: sig,
            current_signature: None,
            cost_q: (i % 200) as u8,
            stability_q: 0,
            anchor_sig: 0x1234_5678,
            distance_q: 0,
        })
        .collect();
    RouteRecord::new(nodes, vec![], 1, String::from("trek"))
}

fn bench_records(c: &mut Criterion) {
    let discovery = sample_discovery();
    c.bench_function("record_encode_discovery", |b| {
        b.iter(|| encode_record(RecordKind::Discovery, black_box(&discovery)))
    });

    let bytes = encode_record(RecordKind::Discovery, &discovery);
    c.bench_function("record_decode_discovery", |b| {
        b.iter(|| {
            let decoded: (Envelope, DiscoveryRecord) =
                decode_record(black_box(&bytes), RecordKind::Discovery).expect("decodes");
            decoded
        })
    });

    c.bench_function("discovery_content_id", |b| {
        b.iter(|| black_box(&discovery).content_id())
    });

    let route = long_route(512);
    c.bench_function("route_content_id_512_nodes", |b| {
        b.iter(|| black_box(&route).content_id())
    });

    c.bench_function("attraction_assembly_512_nodes", |b| {
        b.iter(|| attraction_anchors([black_box(&route)], (5000.0, 180.0), 32))
    });
}

criterion_group!(benches, bench_records);
criterion_main!(benches);
