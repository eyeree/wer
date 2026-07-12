//! Bundle sharing end-to-end (phase-5-plan.md §12.3, the M3 exit criterion):
//! a bundle exported from one explorer's store and imported into another's
//! steers identically, import order does not matter, re-import never
//! double-counts, and the whole path runs over the real native file-tree
//! backend (`FileStorage`) — the same bytes `wer-atlas` moves around.

use std::path::PathBuf;

use tools::{check_bundle, decode_bundle, encode_bundle, FileStorage};
use world_core::{
    bound_target, domain_mask, project_plausible, steer, Anchor, AnchorKind, AnchorSource,
    PossibilityDomain, PossibilityVector,
};
use world_runtime::Vault;

fn temp_store(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wer-atlas-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn capture_like_anchor(x: f64, strength: f32) -> Anchor {
    let mask = domain_mask(&[PossibilityDomain::Morphology, PossibilityDomain::Aesthetics]);
    Anchor {
        world_pos: (x, -40.0),
        target: bound_target(mask, 0.85),
        mask,
        kind: AnchorKind::Emphasize,
        strength,
        falloff_radius: 1400.0,
        source: AnchorSource::Organism {
            species: 0x2340_6061_75CD_D2D2,
        },
    }
}

#[test]
fn a_bundle_exported_from_one_store_steers_another_identically() {
    let root_a = temp_store("a");
    let root_b = temp_store("b");

    // Explorer A records two discoveries and exports a bundle file.
    let mut vault_a = Vault::open(FileStorage::open(&root_a).unwrap()).unwrap();
    let id_1 = vault_a
        .record_discovery(&capture_like_anchor(100.0, 0.8), 0xAB, "one".into())
        .unwrap();
    let id_2 = vault_a
        .record_discovery(&capture_like_anchor(900.0, 0.6), 0xCD, "two".into())
        .unwrap();
    vault_a.flush_all().unwrap();
    let bundle_bytes = encode_bundle(vault_a.export());
    let bundle_path = temp_store("bundle").with_extension("bundle");
    std::fs::write(&bundle_path, &bundle_bytes).unwrap();

    // The bundle validates.
    let check = check_bundle(&std::fs::read(&bundle_path).unwrap()).unwrap();
    assert!(check.passed(), "{:?}", check.findings);
    assert_eq!(check.counts.0, 2);

    // Explorer B imports it into a fresh store (through the file backend),
    // then "restarts" (reopen) — records must persist.
    let (_, bundle) = decode_bundle(&std::fs::read(&bundle_path).unwrap()).unwrap();
    let mut vault_b = Vault::open(FileStorage::open(&root_b).unwrap()).unwrap();
    let stats = vault_b.import(&bundle);
    assert_eq!(stats.added, 2);
    assert_eq!(stats.rejected, 0);
    vault_b.flush_all().unwrap();
    let vault_b = Vault::open(FileStorage::open(&root_b).unwrap()).unwrap();

    // Shared steering is identical: the anchors B reconstructs steer the same
    // possibility vector A's do, bit for bit (ADR 0013 — quantized integers
    // in, float-deterministic steering math).
    let anchors_a: Vec<Anchor> = vault_a
        .discoveries()
        .values()
        .map(|r| r.to_anchor())
        .collect();
    let anchors_b: Vec<Anchor> = vault_b
        .discoveries()
        .values()
        .map(|r| r.to_anchor())
        .collect();
    let base = PossibilityVector::neutral();
    for at in [(0.0, 0.0), (500.0, -40.0), (1200.0, 300.0)] {
        let steered_a = project_plausible(steer(base, &anchors_a, at));
        let steered_b = project_plausible(steer(base, &anchors_b, at));
        assert_eq!(
            steered_a.dims, steered_b.dims,
            "shared steering diverged at {at:?}"
        );
    }
    assert!(vault_b.discoveries().contains_key(&id_1));
    assert!(vault_b.discoveries().contains_key(&id_2));

    // Re-import is idempotent (never double-counts, never re-dirties).
    let mut vault_b = Vault::open(FileStorage::open(&root_b).unwrap()).unwrap();
    let again = vault_b.import(&bundle);
    assert_eq!((again.added, again.merged, again.rejected), (0, 0, 0));
    assert_eq!(again.unchanged, 2);

    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
    let _ = std::fs::remove_file(&bundle_path);
}
