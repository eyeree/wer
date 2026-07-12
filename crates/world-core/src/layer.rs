//! The Phase 2 layer dependency graph (phase-2-plan.md sections 3 and 4.1;
//! implementation-plan.md section 6.5).
//!
//! Phase 1's hard-coded 3-layer stack is generalized into a static declaration
//! table: each layer declares its input layers and the possibility domains it
//! reads *directly*. Dirtiness propagates along declared edges only — the
//! static drift mask of ADR 0005 is superseded by these declarations
//! (ADR 0007). Layer ids are stable integers in topological order: they
//! participate in feature identity hashing ([`crate::FeatureKey::layer`]) and
//! in dependency hashes ([`crate::dephash`]), scanning a dirty bitset in id
//! order *is* a dependency-order traversal, and the id doubles as the dispatch
//! tiebreak. Ids were reassigned once for Phase 2 (nothing is persisted yet
//! and the phase bumped [`crate::WORLD_ALGORITHM_VERSION`]); they are frozen
//! now that Phase 2 has landed.

use crate::possibility::PossibilityDomain;

/// Elevation — the stable heightfield (ADR 0004). Slow domains only.
pub const LAYER_TERRAIN: u16 = 0;
/// Lithology and rock hardness — stable rock provinces (phase-2-plan.md §7.2).
pub const LAYER_GEOLOGY: u16 = 1;
/// Macro flow topology: one cell per region at [`crate::drainage::MACRO_LEVEL`]
/// (phase-2-plan.md §7.3, ADR 0009). Stable — rivers do not walk.
pub const LAYER_DRAINAGE: u16 = 2;
/// Temperature and moisture (Phase 1 math, re-plumbed through the graph).
pub const LAYER_CLIMATE: u16 = 3;
/// River width and surface wetness — the drifting *expression* of the stable
/// drainage topology (phase-2-plan.md §7.4).
pub const LAYER_HYDROLOGY: u16 = 4;
/// Soil depth and fertility. No direct domains: all sensitivity is inherited
/// through its inputs (phase-2-plan.md §7.5).
pub const LAYER_SOILS: u16 = 5;
/// Biome classification of its inputs (phase-2-plan.md §7.6).
pub const LAYER_BIOME: u16 = 6;
/// Aggregate vegetation: density and canopy height (phase-2-plan.md §7.7).
pub const LAYER_VEGETATION: u16 = 7;
/// Aggregate ecology: herbivore/predator pressure, diversity, dominant species
/// (phase-3-plan.md §4.1, §7.5). The first reader of the Morphology, Behavior,
/// and Aesthetics domains, appended to the Phase 2 graph with no id churn (§3).
pub const LAYER_ECOLOGY: u16 = 8;
/// Number of layers in the stack (Phase 2's eight + Phase 3's L8).
pub const LAYER_COUNT: u16 = 9;

/// The dirty-bitset bit for a layer id.
#[inline]
#[must_use]
pub const fn layer_bit(layer: u16) -> u32 {
    1 << layer
}

/// The dirty mask of a freshly loaded region that has generated nothing yet:
/// every layer. (Not a drift mask — drift dirtying flows from declarations
/// via [`domain_dirty_mask`]; ADR 0007.)
#[inline]
#[must_use]
pub const fn all_layers_mask() -> u32 {
    (1 << LAYER_COUNT) - 1
}

/// The domain-bitmask bit for a possibility domain (bit = `domain.index()`).
#[inline]
#[must_use]
pub const fn domain_bit(domain: PossibilityDomain) -> u8 {
    1 << domain.index()
}

/// Everything the graph needs to know about one layer, declared statically
/// (phase-2-plan.md §4.1).
#[derive(Debug)]
pub struct LayerDecl {
    /// Stable layer id (index into [`LAYERS`]).
    pub id: u16,
    /// Human-readable name for tools and the debug panel.
    pub name: &'static str,
    /// Input layers, each with a strictly lower id — acyclicity by
    /// construction, checked by [`tests::deps_are_strictly_lower_id`].
    pub deps: &'static [u16],
    /// Possibility domains this layer reads *directly* (bit = domain index).
    pub domains: u8,
    /// Bumped when this layer's algorithm changes without a world-version bump
    /// being warranted; folded into the dependency hash (phase-2-plan.md §9.2).
    pub algorithm_revision: u16,
    /// Relative generation cost in budget units (phase-2-plan.md §8.2),
    /// recalibrated at Phase 6 M4 against the post-SIMD criterion benches
    /// (docs/perf-baseline.md): one unit ≈ 25 µs of a 32² tile on the
    /// reference machine — terrain/geology/soils/ecology ≈ 2, the cheap
    /// arithmetic layers ≈ 1, the macro drainage job ≈ 17. Costs are
    /// scheduling metadata only; they fold into no hash (ADR 0018 lets
    /// pacing change freely),
    /// calibrated by the criterion benches rather than taste.
    pub cost: u32,
}

const G: u8 = domain_bit(PossibilityDomain::Geology);
const P: u8 = domain_bit(PossibilityDomain::Planetary);
const C: u8 = domain_bit(PossibilityDomain::Climate);
const H: u8 = domain_bit(PossibilityDomain::Hydrology);
const E: u8 = domain_bit(PossibilityDomain::Ecology);
const M: u8 = domain_bit(PossibilityDomain::Morphology);
const B: u8 = domain_bit(PossibilityDomain::Behavior);
const A: u8 = domain_bit(PossibilityDomain::Aesthetics);

/// The static layer declaration table (phase-2-plan.md §4.1).
///
/// One deliberate addition to the plan's table: Biome declares Terrain, because
/// [`crate::biome::classify`] reads elevation directly (ocean and altitude
/// overrides). Declared edges must match what generators actually consume —
/// that honesty is the whole point of the graph (ADR 0007).
pub const LAYERS: [LayerDecl; LAYER_COUNT as usize] = [
    LayerDecl {
        id: LAYER_TERRAIN,
        name: "terrain",
        deps: &[],
        domains: G | P,
        algorithm_revision: 0,
        cost: 2,
    },
    LayerDecl {
        id: LAYER_GEOLOGY,
        name: "geology",
        deps: &[],
        domains: G,
        algorithm_revision: 0,
        cost: 2,
    },
    LayerDecl {
        id: LAYER_DRAINAGE,
        name: "drainage",
        // Drainage routes over the terrain *algorithm* at macro level, not
        // over terrain tiles; the edge still exists so a terrain revision bump
        // invalidates the routing (see crate::dephash::drainage_dep_hash).
        deps: &[LAYER_TERRAIN],
        // Deliberately empty: routing is identity-grade topology and consumes
        // no runtime possibility state (ADR 0009). Slow-dim character arrives
        // via the quantized anchor-free field base inside the generator.
        domains: 0,
        algorithm_revision: 0,
        cost: 17,
    },
    LayerDecl {
        id: LAYER_CLIMATE,
        name: "climate",
        deps: &[LAYER_TERRAIN],
        domains: C | H | P,
        algorithm_revision: 0,
        cost: 1,
    },
    LayerDecl {
        id: LAYER_HYDROLOGY,
        name: "hydrology",
        deps: &[LAYER_TERRAIN, LAYER_DRAINAGE, LAYER_CLIMATE],
        domains: H | P,
        algorithm_revision: 0,
        cost: 1,
    },
    LayerDecl {
        id: LAYER_SOILS,
        name: "soils",
        deps: &[LAYER_TERRAIN, LAYER_GEOLOGY, LAYER_CLIMATE, LAYER_HYDROLOGY],
        domains: 0,
        algorithm_revision: 0,
        cost: 2,
    },
    LayerDecl {
        id: LAYER_BIOME,
        name: "biome",
        deps: &[LAYER_TERRAIN, LAYER_CLIMATE, LAYER_HYDROLOGY, LAYER_SOILS],
        domains: 0,
        algorithm_revision: 0,
        cost: 1,
    },
    LayerDecl {
        id: LAYER_VEGETATION,
        name: "vegetation",
        deps: &[LAYER_CLIMATE, LAYER_SOILS, LAYER_BIOME],
        domains: E,
        algorithm_revision: 0,
        cost: 1,
    },
    LayerDecl {
        id: LAYER_ECOLOGY,
        name: "ecology",
        // Aggregate populations: rosters key off biome + banded climate/soil,
        // pressure scales with vegetation density (primary productivity). L8 is
        // the first — and only — reader of Morphology, Behavior, and Aesthetics:
        // the aggregate fields are Ecology-driven, while M/B/A fold into the
        // dependency hash because near-field realization (L8's transient
        // consumer, §7.6) expresses genomes under them, so steering M/B/A must
        // regenerate L8 and re-realize its organisms (phase-3-plan.md §5.1, §7.5).
        deps: &[LAYER_CLIMATE, LAYER_SOILS, LAYER_BIOME, LAYER_VEGETATION],
        domains: E | M | B | A,
        // Roster-backed but per-cell arithmetic over four input tiles; the §13
        // benches calibrate this (mid-cost, comparable to hydrology).
        algorithm_revision: 0,
        cost: 2,
    },
];

/// The layer itself plus every transitive dependent, as a layer bitmask —
/// what must be re-checked when the layer's output (or an input it reads)
/// changes. Because deps have strictly lower ids, one ascending pass reaches
/// the fixed point.
#[must_use]
pub const fn dependents_closure(layer: u16) -> u32 {
    let mut closure = layer_bit(layer);
    let mut id = 0;
    while id < LAYER_COUNT as usize {
        let decl = &LAYERS[id];
        let mut i = 0;
        while i < decl.deps.len() {
            if closure & layer_bit(decl.deps[i]) != 0 {
                closure |= layer_bit(decl.id);
            }
            i += 1;
        }
        id += 1;
    }
    closure
}

/// Layers whose *own* declaration reads `domain` (no closure applied).
#[must_use]
pub const fn domain_readers(domain: PossibilityDomain) -> u32 {
    let bit = domain_bit(domain);
    let mut readers = 0u32;
    let mut id = 0;
    while id < LAYER_COUNT as usize {
        if LAYERS[id].domains & bit != 0 {
            readers |= layer_bit(LAYERS[id].id);
        }
        id += 1;
    }
    readers
}

/// The layers to re-check when the quantized buckets of `domain_bits` flip:
/// the direct readers of each flipped domain, closed over transitive
/// dependents (phase-2-plan.md §7.8). This is the drift-propagation rule that
/// supersedes ADR 0005's static mask.
#[must_use]
pub const fn domain_dirty_mask(domain_bits: u8) -> u32 {
    let mut dirty = 0u32;
    let mut d = 0;
    while d < PossibilityDomain::ALL.len() {
        if domain_bits & (1 << d) != 0 {
            let readers = domain_readers(PossibilityDomain::ALL[d]);
            let mut layer = 0u16;
            while layer < LAYER_COUNT {
                if readers & layer_bit(layer) != 0 {
                    dirty |= dependents_closure(layer);
                }
                layer += 1;
            }
        }
        d += 1;
    }
    dirty
}

/// A layer's declaration.
#[inline]
#[must_use]
pub const fn layer_decl(layer: u16) -> &'static LayerDecl {
    &LAYERS[layer as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_table_positions() {
        for (i, decl) in LAYERS.iter().enumerate() {
            assert_eq!(decl.id as usize, i);
        }
    }

    #[test]
    pub fn deps_are_strictly_lower_id() {
        // Acyclicity by construction (phase-2-plan.md §12.4).
        for decl in &LAYERS {
            for &dep in decl.deps {
                assert!(
                    dep < decl.id,
                    "layer {} depends on {dep}, which is not a lower id",
                    decl.id
                );
            }
        }
    }

    #[test]
    fn closures_match_a_direct_fixed_point() {
        // Independent computation: iterate propagation until stable.
        for layer in 0..LAYER_COUNT {
            let mut expected = layer_bit(layer);
            loop {
                let mut next = expected;
                for decl in &LAYERS {
                    for &dep in decl.deps {
                        if expected & layer_bit(dep) != 0 {
                            next |= layer_bit(decl.id);
                        }
                    }
                }
                if next == expected {
                    break;
                }
                expected = next;
            }
            assert_eq!(
                dependents_closure(layer),
                expected,
                "closure mismatch for layer {layer}"
            );
        }
    }

    #[test]
    fn stable_trio_never_reads_fast_domains() {
        // Section 9's stability commitment, as a property of the declarations:
        // no fast domain reaches terrain, geology, or drainage.
        let fast = domain_bit(PossibilityDomain::Climate)
            | domain_bit(PossibilityDomain::Hydrology)
            | domain_bit(PossibilityDomain::Ecology)
            | domain_bit(PossibilityDomain::Morphology)
            | domain_bit(PossibilityDomain::Behavior)
            | domain_bit(PossibilityDomain::Aesthetics);
        let trio = layer_bit(LAYER_TERRAIN) | layer_bit(LAYER_GEOLOGY) | layer_bit(LAYER_DRAINAGE);
        assert_eq!(domain_dirty_mask(fast) & trio, 0);
    }

    #[test]
    fn domain_dirty_masks_match_the_declared_graph() {
        // Ecology drives vegetation (since Phase 2) and now L8; L8 is downstream
        // of vegetation, so an Ecology flip reaches exactly {Vegetation, L8}.
        assert_eq!(
            domain_dirty_mask(domain_bit(PossibilityDomain::Ecology)),
            layer_bit(LAYER_VEGETATION) | layer_bit(LAYER_ECOLOGY)
        );
        // A Climate flip re-checks climate and everything downstream of it —
        // now including L8, which depends on climate through several paths.
        assert_eq!(
            domain_dirty_mask(domain_bit(PossibilityDomain::Climate)),
            layer_bit(LAYER_CLIMATE)
                | layer_bit(LAYER_HYDROLOGY)
                | layer_bit(LAYER_SOILS)
                | layer_bit(LAYER_BIOME)
                | layer_bit(LAYER_VEGETATION)
                | layer_bit(LAYER_ECOLOGY)
        );
        // Morphology/Behavior/Aesthetics now reach exactly L8 (Phase 3 wired
        // them in; they invalidate nothing upstream).
        for domain in [
            PossibilityDomain::Morphology,
            PossibilityDomain::Behavior,
            PossibilityDomain::Aesthetics,
        ] {
            assert_eq!(
                domain_dirty_mask(domain_bit(domain)),
                layer_bit(LAYER_ECOLOGY),
                "{domain:?} must reach exactly L8"
            );
        }
    }

    #[test]
    fn soils_inherits_all_sensitivity() {
        // Soils reads no domain directly; every path to it is through inputs.
        assert_eq!(layer_decl(LAYER_SOILS).domains, 0);
        assert_ne!(
            domain_dirty_mask(domain_bit(PossibilityDomain::Climate)) & layer_bit(LAYER_SOILS),
            0,
            "a climate flip must reach soils transitively"
        );
    }
}
