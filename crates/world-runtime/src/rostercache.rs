//! The roster cache (phase-3-plan.md §6.3): memoized `(roster, food web)` per
//! [`HabitatSignature`].
//!
//! The Tier-A analogue of the macro drainage cache. Distinct habitats are far
//! fewer than regions, so a roster and its food web are computed once per
//! distinct signature and shared through cheap `Arc` clones across every cell
//! and region that resolves to that signature. Because coarse banding makes
//! signatures repeat heavily, the cache is naturally bounded
//! (`≤ Biome × band³` entries) and evicts signatures no resident region uses
//! any more (the macro cache's dependent-sweep shape).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use world_core::{
    food_web, population_table, signature_productivity, species_roster, FoodWeb, HabitatSignature,
    PopulationTable, SpeciesRoster,
};

/// One cached habitat: its deterministic roster, the food web projected over
/// it at the signature's representative productivity (§6.3), and the hoisted
/// cell-invariant population table (phase-6-plan.md §6.3) the L8 per-cell
/// loop reads instead of re-deriving per cell.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterEntry {
    /// The habitat this entry is a function of.
    pub signature: HabitatSignature,
    /// The deterministic, trophic-sorted roster.
    pub roster: SpeciesRoster,
    /// The plausibility-constrained food web over the roster.
    pub web: FoodWeb,
    /// Cell-invariant population values (dominant, tier biomasses,
    /// diversity), hoisted once per signature — same math, same results as
    /// the per-cell derivation (phase-6-plan.md §6.3).
    pub table: PopulationTable,
}

impl RosterEntry {
    /// Build the roster and food web for a signature (pure; the cache memoizes
    /// this so it runs once per distinct signature).
    #[must_use]
    pub fn build(signature: HabitatSignature) -> Self {
        let roster = species_roster(signature);
        let web = food_web(&roster, signature_productivity(signature));
        let table = population_table(&roster, &web);
        Self {
            signature,
            roster,
            web,
            table,
        }
    }

    /// Approximate heap bytes held by this entry (cache telemetry, §8.4).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.roster.species.len() * core::mem::size_of::<world_core::Species>()
            + self.web.edges.len() * core::mem::size_of::<(u32, u32)>()
            + self.web.pruned.len() * core::mem::size_of::<u32>()
    }
}

/// A snapshot of the roster entries a region's L8 job needs, keyed by signature
/// — the Tier-A analogue of the drainage macro tile a hydrology job snapshots.
/// The generation job looks each cell's signature up in it.
pub type RosterSnapshot = BTreeMap<HabitatSignature, Arc<RosterEntry>>;

/// Cache of `(roster, food web)` per habitat signature for the active window.
/// A `BTreeMap` for deterministic iteration order, as with the other caches.
#[derive(Debug, Default)]
pub struct RosterCache {
    entries: BTreeMap<HabitatSignature, Arc<RosterEntry>>,
    /// Count of entries actually built (not served from cache), cumulative —
    /// surfaced through [`Self::take_builds`] for the frame stats.
    builds: usize,
}

impl RosterCache {
    /// The entry for a signature, if resident.
    #[inline]
    #[must_use]
    pub fn get(&self, signature: HabitatSignature) -> Option<&Arc<RosterEntry>> {
        self.entries.get(&signature)
    }

    /// The entry for a signature, building and caching it on a miss. Pure
    /// function of the signature, so build order never affects content (§10).
    pub fn ensure(&mut self, signature: HabitatSignature) -> Arc<RosterEntry> {
        if let Some(entry) = self.entries.get(&signature) {
            return Arc::clone(entry);
        }
        let entry = Arc::new(RosterEntry::build(signature));
        self.builds += 1;
        self.entries.insert(signature, Arc::clone(&entry));
        entry
    }

    /// Read and reset the cumulative build counter (per-frame `rosters_built`).
    pub fn take_builds(&mut self) -> usize {
        core::mem::take(&mut self.builds)
    }

    /// Drop every entry whose signature is not in `needed` (dependent-tracked
    /// eviction, §6.3).
    pub fn evict_unused(&mut self, needed: &BTreeSet<HabitatSignature>) {
        self.entries.retain(|sig, _| needed.contains(sig));
    }

    /// Evict entries (highest signature first — deterministic) until total
    /// bytes fit under `ceiling` (the Phase 6 capacity ceiling,
    /// phase-6-plan.md §4.3). Safe: an entry rebuilds on demand as a pure
    /// function of its signature.
    pub fn evict_to_bytes(&mut self, ceiling: usize) {
        let mut bytes = self.bytes();
        while bytes > ceiling {
            let Some((_, entry)) = self.entries.pop_last() else {
                return;
            };
            bytes = bytes.saturating_sub(entry.bytes());
        }
    }

    /// Number of resident entries.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total approximate heap bytes held by cached entries (telemetry, §8.4).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.entries.values().map(|e| e.bytes()).sum()
    }

    /// Iterate entries in deterministic signature order.
    pub fn iter(&self) -> impl Iterator<Item = (&HabitatSignature, &Arc<RosterEntry>)> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::biome::Biome;

    fn sig(biome: Biome, t: u8, m: u8, f: u8) -> HabitatSignature {
        HabitatSignature {
            biome: biome.id(),
            temperature_band: t,
            moisture_band: m,
            fertility_band: f,
        }
    }

    #[test]
    fn ensure_memoizes_and_shares() {
        let mut cache = RosterCache::default();
        let s = sig(Biome::Rainforest, 5, 4, 3);
        let a = cache.ensure(s);
        let b = cache.ensure(s);
        assert!(Arc::ptr_eq(&a, &b), "same signature must share the Arc");
        assert_eq!(cache.take_builds(), 1);
        assert_eq!(cache.take_builds(), 0, "no rebuild on hit");
        assert_eq!(cache.len(), 1);
        assert!(cache.bytes() > 0);
    }

    #[test]
    fn eviction_sweeps_unused_signatures() {
        let mut cache = RosterCache::default();
        let keep = sig(Biome::Grassland, 3, 2, 1);
        let drop = sig(Biome::Desert, 4, 0, 0);
        cache.ensure(keep);
        cache.ensure(drop);
        assert_eq!(cache.len(), 2);
        let needed: BTreeSet<_> = [keep].into_iter().collect();
        cache.evict_unused(&needed);
        assert!(cache.get(keep).is_some());
        assert!(cache.get(drop).is_none());
    }
}
