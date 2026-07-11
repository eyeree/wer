//! Region-layer dependency hashes (phase-2-plan.md §4.3, ADR 0008).
//!
//! A tile's content is a pure function of its dependency key: the world
//! algorithm version, the layer and its algorithm revision, the region, the
//! field resolution, the quantized possibility buckets the layer reads
//! directly, and the dependency hashes of its input tiles. Staleness is
//! therefore a single integer comparison — `stored_hash != expected_hash` —
//! exact, cheap, and free of both over-invalidation (Phase 1's revision
//! coupling) and silent skew (ADR 0005's accepted terrain drift-lag). Because
//! input hashes chain, a change anywhere upstream changes every downstream
//! expected hash automatically; there is no second invalidation mechanism to
//! keep in sync.
//!
//! Dependency hashes are run-local cache keys, not a persistence format
//! (phase-2-plan.md §1.4): the folded buckets come from runtime float state.

use crate::coord::RegionCoord;
use crate::drainage::MACRO_GRID;
use crate::hash::mix;
use crate::layer::{layer_decl, LAYER_DRAINAGE, LAYER_TERRAIN};
use crate::WORLD_ALGORITHM_VERSION;

/// Fixed basis separating dependency hashing from every other hash domain.
const DEPHASH_BASIS: u64 = 0xD1E9_4A57_B3C6_02F1;

/// The per-(region, layer) dependency hash: a stable fold of everything the
/// layer's output depends on. The fold order is part of the stable contract:
///
/// ```text
/// basis → WORLD_ALGORITHM_VERSION → layer id → layer algorithm revision
///       → region (x, y, level) → field resolution
///       → quantized bucket of each directly-read domain (stable domain order)
///       → dep hash of each input tile (declaration order; the macro drainage
///         tile's hash rides in the same slot where declared)
/// ```
///
/// `algorithm_revision` is a parameter rather than a table lookup so the
/// runtime can apply run-local revision bumps (the invalidation-precision
/// harness exercises the §12.3 "revision bump" scenario without recompiling).
#[must_use]
pub fn layer_dep_hash(
    region: RegionCoord,
    layer: u16,
    algorithm_revision: u16,
    quantized: &[u16],
    input_hashes: &[u64],
    resolution: u16,
) -> u64 {
    let mut h = DEPHASH_BASIS;
    h = mix(h, WORLD_ALGORITHM_VERSION as u64);
    h = mix(h, layer as u64);
    h = mix(h, algorithm_revision as u64);
    // Signed coordinates fold as their unsigned bit patterns for portability.
    h = mix(h, region.x as u32 as u64);
    h = mix(h, region.y as u32 as u64);
    h = mix(h, region.level as u64);
    h = mix(h, resolution as u64);
    for &bucket in quantized {
        h = mix(h, bucket as u64);
    }
    for &input in input_hashes {
        h = mix(h, input);
    }
    h
}

/// Dependency hash of a macro drainage tile (ADR 0009).
///
/// Drainage routes over the terrain *algorithm* (sampled directly at macro
/// cell centers), not over terrain tiles, and consumes no runtime possibility
/// state — routing is identity-grade topology. Its declared Terrain edge is
/// honored by folding the terrain algorithm revision where an input-tile hash
/// would otherwise go, so a terrain revision bump still invalidates every
/// river network.
#[must_use]
pub fn drainage_dep_hash(
    macro_coord: RegionCoord,
    drainage_revision: u16,
    terrain_revision: u16,
) -> u64 {
    layer_dep_hash(
        macro_coord,
        LAYER_DRAINAGE,
        drainage_revision,
        &[],
        &[terrain_revision as u64],
        MACRO_GRID as u16,
    )
}

/// [`drainage_dep_hash`] at the declared table revisions (no run-local bumps).
#[must_use]
pub fn drainage_dep_hash_default(macro_coord: RegionCoord) -> u64 {
    drainage_dep_hash(
        macro_coord,
        layer_decl(LAYER_DRAINAGE).algorithm_revision,
        layer_decl(LAYER_TERRAIN).algorithm_revision,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_folded_input_changes_the_hash() {
        // Mirrors `feature_hash_separates_every_field` (phase-2-plan.md §12.4).
        let region = RegionCoord::new(-3, 7);
        let base = layer_dep_hash(region, 3, 1, &[100, 200], &[11, 22], 32);
        let variants = [
            layer_dep_hash(RegionCoord::new(-2, 7), 3, 1, &[100, 200], &[11, 22], 32),
            layer_dep_hash(RegionCoord::new(-3, 8), 3, 1, &[100, 200], &[11, 22], 32),
            layer_dep_hash(
                RegionCoord::at_level(-3, 7, 1),
                3,
                1,
                &[100, 200],
                &[11, 22],
                32,
            ),
            layer_dep_hash(region, 4, 1, &[100, 200], &[11, 22], 32),
            layer_dep_hash(region, 3, 2, &[100, 200], &[11, 22], 32),
            layer_dep_hash(region, 3, 1, &[101, 200], &[11, 22], 32),
            layer_dep_hash(region, 3, 1, &[100, 201], &[11, 22], 32),
            layer_dep_hash(region, 3, 1, &[100, 200], &[12, 22], 32),
            layer_dep_hash(region, 3, 1, &[100, 200], &[11, 23], 32),
            layer_dep_hash(region, 3, 1, &[100, 200], &[11, 22], 16),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(*v, base, "variant {i} did not change the hash");
        }
    }

    #[test]
    fn hash_is_pure() {
        let region = RegionCoord::new(5, -9);
        assert_eq!(
            layer_dep_hash(region, 0, 0, &[42], &[], 32),
            layer_dep_hash(region, 0, 0, &[42], &[], 32)
        );
    }

    #[test]
    fn drainage_hash_tracks_both_revisions() {
        let mc = RegionCoord::at_level(1, -2, crate::drainage::MACRO_LEVEL);
        let base = drainage_dep_hash(mc, 0, 0);
        assert_ne!(drainage_dep_hash(mc, 1, 0), base);
        assert_ne!(drainage_dep_hash(mc, 0, 1), base);
        assert_eq!(drainage_dep_hash_default(mc), base);
    }
}
