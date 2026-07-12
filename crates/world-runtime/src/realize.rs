//! Near-field organism realization (phase-3-plan.md §7.6, §8.3).
//!
//! Tier B of the Phase 3 architecture: individual organism instances near the
//! player, a **pure, un-cached function** of the settled L8 aggregate fields
//! plus an integer identity — never a source of cached state. Each cell of a
//! pinned near region instantiates organisms whose count and coverage *preserve
//! the aggregate* (section 10: 70% canopy → ~70% near-field coverage), each with
//! a stable [`FeatureKey`]-derived identity (the identity machine built in
//! Phase 0, first used here).
//!
//! Placement, species choice, and per-instance jitter are seeded from
//! `feature_hash(FeatureKey{ region, layer: LAYER_ECOLOGY, feature_index: cell,
//! possibility_revision })`, so a pinned region realizes bit-identical organisms
//! across frames and across a two-run replay. Distance-based regeneration and
//! offscreen replacement fall out for free: organisms are recomputed when the
//! region's source tiles change (its L8 dependency hash moves) and simply
//! dropped when it leaves the near window — no morphing, no stored entity state
//! (§7.6).
//!
//! The runtime coordinator verifies that every signature tracked for a
//! resident region is present in the roster cache before it reuses or
//! publishes that region's organism vector. These pure helpers remain
//! lookup-only: roster maintenance and retry policy belong to the coordinator
//! (ADR 0019).

use world_core::layer::LAYER_ECOLOGY;
use world_core::{
    feature_hash, species_biomass, Biome, Climate, Expressed, FeatureKey, GenomeBias,
    HabitatSignature, LocalPos, RegionCoord, Rng, Soils, Trophic, REGION_SIZE,
    WORLD_ALGORITHM_VERSION,
};

use crate::generate::{
    RegionTiles, CHANNEL_FERTILITY, CHANNEL_MOISTURE, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION,
};
use crate::rostercache::RosterCache;

/// A realized near-field organism instance. Transient presentation state — a
/// stable id, its species, where it sits, and its expressed appearance — never
/// cached and never a source of identity beyond its own [`FeatureKey`] hash.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Organism {
    /// Stable identity: `feature_hash(FeatureKey{ region, LAYER_ECOLOGY, cell,
    /// possibility_revision })`.
    pub id: u64,
    /// The species this organism instantiates ([`world_core::Species::id`]).
    pub species: u64,
    /// The species' trophic role.
    pub trophic: Trophic,
    /// Density slot that produced this instance. Slot 0 is the canonical
    /// gameplay sample; higher slots are additive presentation instances
    /// (ADR 0024).
    pub slot: u16,
    /// The cell within the region it was placed in.
    pub cell: LocalPos,
    /// Jittered world-space position.
    pub world_pos: (f64, f64),
    /// Expressed appearance/size after the region's [`GenomeBias`], with body
    /// size clamped to the habitat's max sustainable size (the §12.3 body-size
    /// invariant).
    pub expressed: Expressed,
}

/// Realize the near-field organisms of one region from its settled aggregate
/// tiles (phase-3-plan.md §7.6). Pure: a function of `(tiles, rosters, bias,
/// revision)` — no shared mutable state, safe to run on any thread.
///
/// One organism per cell is instantiated with probability equal to the cell's
/// vegetation density, so coverage preserves the aggregate. Its species is
/// sampled from the cell's roster weighted by biomass, so the realized trophic
/// mix matches the aggregate (the §12.3 aggregate↔entity invariant). `None`
/// inputs (tiles not yet generated) yield an empty realization. A missing
/// roster entry is likewise skipped here; the runtime's publishing path
/// preflights roster completeness before calling this lookup-only helper, so a
/// transient miss cannot replace a current realization with partial output.
#[must_use]
pub fn realize_region(
    coord: RegionCoord,
    tiles: &RegionTiles,
    rosters: &RosterCache,
    bias: GenomeBias,
    possibility_revision: u32,
    resolution: u16,
) -> Vec<Organism> {
    let mut out = Vec::new();
    realize_region_into(
        coord,
        tiles,
        rosters,
        bias,
        possibility_revision,
        resolution,
        1,
        &mut out,
    );
    out
}

/// [`realize_region`] writing into a caller-provided vector, so the runtime
/// can recycle organism allocations through the rebuild-on-L8-change path
/// (phase-6-plan.md §4.2). `out` is cleared first; content is identical to
/// [`realize_region`]. The runtime coordinator calls this only after its
/// resident roster-completeness preflight, keeping cache repair and deferred
/// retry outside this pure, lookup-only function.
///
/// `organisms_per_cell` is the Phase 6 density lever (phase-6-plan.md §6.6):
/// slot 0 derives the exact Phase 5 identity (`feature_index = cell`); slot
/// `s > 0` derives the additive identity `cell + s·res²` from the same
/// scheme, each slot independently density-gated so expected population
/// scales linearly and the aggregate↔entity ratios hold at any density.
#[allow(clippy::too_many_arguments)]
pub fn realize_region_into(
    coord: RegionCoord,
    tiles: &RegionTiles,
    rosters: &RosterCache,
    bias: GenomeBias,
    possibility_revision: u32,
    resolution: u16,
    organisms_per_cell: u16,
    out: &mut Vec<Organism>,
) {
    out.clear();
    let (Some(vegetation), Some(temperature), Some(moisture), Some(fertility), Some(biome_tile)) = (
        tiles.channels[CHANNEL_VEGETATION].as_ref(),
        tiles.channels[CHANNEL_TEMPERATURE].as_ref(),
        tiles.channels[CHANNEL_MOISTURE].as_ref(),
        tiles.channels[CHANNEL_FERTILITY].as_ref(),
        tiles.biome.as_ref(),
    ) else {
        return;
    };

    let (ox, oy) = coord.origin();
    let cell_size = REGION_SIZE / f64::from(resolution);

    for cy in 0..resolution {
        for cx in 0..resolution {
            let density = vegetation.get(cx, cy);
            if density <= 0.0 {
                continue;
            }
            let cell_index = u32::from(cy) * u32::from(resolution) + u32::from(cx);
            let cells = u32::from(resolution) * u32::from(resolution);
            for slot in 0..u32::from(organisms_per_cell) {
                // Slot 0 is the exact Phase 5 identity; higher slots are
                // additive identities from the same scheme (§6.6).
                let feature_index = cell_index + slot * cells;
                let id = feature_hash(&FeatureKey {
                    world_version: WORLD_ALGORITHM_VERSION,
                    region: coord,
                    layer: LAYER_ECOLOGY,
                    feature_index,
                    possibility_revision,
                });
                let mut rng = Rng::new(id);
                // Presence preserves the aggregate: each slot of a cell at
                // density d hosts an organism with probability d (section 10),
                // so expected population scales linearly with slots.
                if rng.next_f32() >= density {
                    continue;
                }
                // Classify the cell's habitat exactly as L8 does, then resolve
                // its roster/web from the cache the scheduler populated for
                // this region.
                let c = Climate {
                    temperature: temperature.get(cx, cy),
                    moisture: moisture.get(cx, cy),
                };
                let s = Soils {
                    depth: 0.0,
                    fertility: fertility.get(cx, cy),
                };
                let signature =
                    HabitatSignature::of(Biome::from_id(biome_tile.get(cx, cy)), &c, &s);
                let Some(entry) = rosters.get(signature) else {
                    continue;
                };
                if entry.roster.species.is_empty() {
                    continue;
                }
                let index = sample_species(&entry.roster, &entry.web, &mut rng);
                let species = &entry.roster.species[index];
                let mut expressed = species.genome.express(bias);
                // Clamp expressed body size to what the habitat can feed.
                expressed.size = expressed.size.min(entry.web.max_body_size);

                let jx = f64::from(rng.next_f32());
                let jy = f64::from(rng.next_f32());
                let world_pos = (
                    ox + (f64::from(cx) + jx) * cell_size,
                    oy + (f64::from(cy) + jy) * cell_size,
                );
                out.push(Organism {
                    id,
                    species: species.id,
                    trophic: species.trophic,
                    slot: slot as u16,
                    cell: LocalPos::new(cx, cy),
                    world_pos,
                    expressed,
                });
            }
        }
    }
}

/// Sample a roster index weighted by per-species biomass (producers dominate;
/// consumers appear in proportion to their tier biomass). Deterministic given
/// `rng`.
fn sample_species(
    roster: &world_core::SpeciesRoster,
    web: &world_core::FoodWeb,
    rng: &mut Rng,
) -> usize {
    let n = roster.species.len();
    let total: f32 = (0..n).map(|i| species_biomass(roster, web, i)).sum();
    if total <= 0.0 {
        return 0;
    }
    let mut threshold = rng.next_f32() * total;
    for i in 0..n {
        let bio = species_biomass(roster, web, i);
        if threshold < bio {
            return i;
        }
        threshold -= bio;
    }
    n - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;
    use crate::stream::{RegionMap, StreamConfig};
    use crate::InlineExecutor;
    use world_core::{PossibilityField, POSSIBILITY_DIMS};

    fn settled_map() -> (RegionMap, RegionCoord) {
        let cfg = StreamConfig {
            near_radius: 1.5 * REGION_SIZE,
            far_radius: 3.0 * REGION_SIZE,
            load_radius: 3.0 * REGION_SIZE,
            unload_radius: 4.0 * REGION_SIZE,
            field_resolution: 16,
            ..StreamConfig::default()
        };
        let field = PossibilityField::default();
        let bias = [0.0f32; POSSIBILITY_DIMS];
        let mut map = RegionMap::new(cfg);
        for _ in 0..6 {
            map.update(
                (0.0, 0.0),
                0.0,
                &field,
                &[],
                &bias,
                &Budget::unlimited(),
                &InlineExecutor,
                false,
            );
        }
        (map, RegionCoord::new(0, 0))
    }

    #[test]
    fn realization_is_a_pure_function_of_inputs() {
        let (map, coord) = settled_map();
        let tiles = map.cache().get(coord).expect("tiles");
        let a = realize_region(
            coord,
            tiles,
            map.roster_cache(),
            GenomeBias::neutral(),
            0,
            16,
        );
        let b = realize_region(
            coord,
            tiles,
            map.roster_cache(),
            GenomeBias::neutral(),
            0,
            16,
        );
        assert_eq!(a, b, "realization must be deterministic");
    }

    #[test]
    fn realization_preserves_aggregate_coverage() {
        let (map, coord) = settled_map();
        let tiles = map.cache().get(coord).expect("tiles");
        let res = 16u16;
        let organisms = realize_region(
            coord,
            tiles,
            map.roster_cache(),
            GenomeBias::neutral(),
            0,
            res,
        );
        // Expected count ≈ sum of vegetation density over cells (one organism
        // per cell with probability = density).
        let veg = tiles.channels[CHANNEL_VEGETATION].as_ref().unwrap();
        let expected: f32 = (0..res)
            .flat_map(|cy| (0..res).map(move |cx| (cx, cy)))
            .map(|(cx, cy)| veg.get(cx, cy))
            .sum();
        let realized = organisms.len() as f32;
        // Statistical tolerance for a 256-cell region.
        assert!(
            (realized - expected).abs() <= expected.max(4.0) * 0.5 + 8.0,
            "realized {realized} far from aggregate {expected}"
        );
        // No organism exceeds its habitat's body-size bound.
        for org in &organisms {
            assert!(org.expressed.size <= world_core::max_body_size(1.0) + 1e-3);
        }
    }

    #[test]
    fn revision_changes_organism_ids() {
        let (map, coord) = settled_map();
        let tiles = map.cache().get(coord).expect("tiles");
        let a = realize_region(
            coord,
            tiles,
            map.roster_cache(),
            GenomeBias::neutral(),
            0,
            16,
        );
        let b = realize_region(
            coord,
            tiles,
            map.roster_cache(),
            GenomeBias::neutral(),
            1,
            16,
        );
        // A different possibility revision re-rolls identities (succession).
        if let (Some(x), Some(y)) = (a.first(), b.first()) {
            assert_ne!(x.id, y.id);
        }
    }

    #[test]
    fn density_slots_are_explicit_and_additive() {
        let (map, coord) = settled_map();
        let tiles = map.cache().get(coord).expect("tiles");
        let mut density_one = Vec::new();
        let mut density_four = Vec::new();
        for (density, out) in [(1, &mut density_one), (4, &mut density_four)] {
            realize_region_into(
                coord,
                tiles,
                map.roster_cache(),
                GenomeBias::neutral(),
                0,
                16,
                density,
                out,
            );
        }

        assert!(density_one.iter().all(|organism| organism.slot == 0));
        let canonical: Vec<_> = density_four
            .iter()
            .copied()
            .filter(|organism| organism.slot == 0)
            .collect();
        assert_eq!(canonical, density_one);
        assert!(density_four.iter().all(|organism| organism.slot < 4));

        let mut ids: Vec<_> = density_four.iter().map(|organism| organism.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), density_four.len());

        let cells = 16_u32 * 16;
        for organism in &density_four {
            let feature_index = organism.cell.to_index(16) + u32::from(organism.slot) * cells;
            assert_eq!(
                organism.id,
                feature_hash(&FeatureKey {
                    world_version: WORLD_ALGORITHM_VERSION,
                    region: coord,
                    layer: LAYER_ECOLOGY,
                    feature_index,
                    possibility_revision: 0,
                })
            );
        }
    }
}
