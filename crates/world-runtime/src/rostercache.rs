//! The roster cache (phase-3-plan.md §6.3): memoized `(roster, food web)` per
//! [`HabitatSignature`].
//!
//! The Tier-A analogue of the macro drainage cache. Distinct habitats are far
//! fewer than regions, so a roster and its food web are computed once per
//! distinct signature and shared through cheap `Arc` clones across every cell
//! and region that resolves to that signature. Because coarse banding makes
//! signatures repeat heavily, the cache is naturally bounded
//! (`≤ Biome × band³` entries) and evicts signatures no resident region uses
//! any more (the macro cache's dependent-sweep shape). Resident regions'
//! signatures form an indispensable working set: a byte ceiling is a soft
//! target that may be exceeded when that working set alone is larger (ADR
//! 0019).

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

/// Work performed by one byte-target eviction pass.
///
/// The resident cache can remain above its target when all remaining entries
/// are protected; these fields report only entries that were actually
/// disposable and removed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RosterEviction {
    /// Number of entries removed from the cache.
    pub entries_removed: usize,
    /// Approximate heap bytes removed from the cache.
    pub bytes_removed: usize,
}

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

    /// Evict unprotected entries until total bytes fit under `target`, or no
    /// disposable entries remain.
    ///
    /// Victims are selected highest-signature first for deterministic Phase 6
    /// capacity behavior (phase-6-plan.md §4.3). `target` is deliberately a
    /// soft cache target, not a correctness ceiling: `protected` is the
    /// indispensable resident working set, so its entries remain even when
    /// their bytes alone exceed the target (ADR 0019). Removed entries rebuild
    /// on demand as pure functions of their signatures.
    pub fn evict_to_bytes(
        &mut self,
        target: usize,
        protected: &BTreeSet<HabitatSignature>,
    ) -> RosterEviction {
        let mut resident_bytes = self.bytes();
        let victims: Vec<_> = self
            .entries
            .keys()
            .rev()
            .filter(|signature| !protected.contains(signature))
            .copied()
            .collect();
        let mut eviction = RosterEviction::default();

        for signature in victims {
            if resident_bytes <= target {
                break;
            }
            let Some(entry) = self.entries.remove(&signature) else {
                continue;
            };
            let bytes = entry.bytes();
            resident_bytes = resident_bytes.saturating_sub(bytes);
            eviction.entries_removed += 1;
            eviction.bytes_removed += bytes;
        }

        eviction
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

    #[test]
    fn byte_target_evicts_disposable_entries_around_protected_entries() {
        let mut cache = RosterCache::default();
        let low = sig(Biome::Grassland, 1, 2, 3);
        let middle = sig(Biome::Grassland, 2, 2, 3);
        let high = sig(Biome::Grassland, 3, 2, 3);
        cache.ensure(low);
        cache.ensure(middle);
        cache.ensure(high);
        let middle_bytes = cache.get(middle).expect("middle entry").bytes();
        let target = cache.bytes() - middle_bytes;
        let protected: BTreeSet<_> = [low, high].into_iter().collect();

        let eviction = cache.evict_to_bytes(target, &protected);

        assert_eq!(
            eviction,
            RosterEviction {
                entries_removed: 1,
                bytes_removed: middle_bytes,
            }
        );
        assert!(cache.get(low).is_some());
        assert!(cache.get(middle).is_none());
        assert!(cache.get(high).is_some());
        assert_eq!(cache.bytes(), target);
    }

    #[test]
    fn zero_byte_target_removes_every_disposable_entry() {
        let mut cache = RosterCache::default();
        let a = sig(Biome::Rainforest, 5, 4, 3);
        let b = sig(Biome::Rainforest, 6, 4, 3);
        cache.ensure(a);
        cache.ensure(b);
        let bytes = cache.bytes();
        assert!(cache.iter().all(|(_, entry)| entry.bytes() > 0));

        let eviction = cache.evict_to_bytes(0, &BTreeSet::new());

        assert_eq!(eviction.entries_removed, 2);
        assert_eq!(eviction.bytes_removed, bytes);
        assert!(cache.is_empty());
    }

    #[test]
    fn all_protected_entries_may_exceed_the_byte_target() {
        let mut cache = RosterCache::default();
        let a = sig(Biome::Tundra, 0, 1, 2);
        let b = sig(Biome::Tundra, 1, 1, 2);
        cache.ensure(a);
        cache.ensure(b);
        let bytes = cache.bytes();
        assert!(bytes > 0);
        let protected: BTreeSet<_> = [a, b].into_iter().collect();

        let eviction = cache.evict_to_bytes(0, &protected);

        assert_eq!(eviction, RosterEviction::default());
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.bytes(), bytes);
    }

    #[test]
    fn byte_target_uses_reverse_signature_victim_order() {
        let mut cache = RosterCache::default();
        let signatures = [
            sig(Biome::Wetland, 1, 2, 3),
            sig(Biome::Wetland, 2, 2, 3),
            sig(Biome::Wetland, 3, 2, 3),
        ];
        for signature in signatures {
            cache.ensure(signature);
        }
        let highest = signatures.into_iter().max().expect("signature");
        let highest_bytes = cache.get(highest).expect("highest entry").bytes();
        assert!(highest_bytes > 0);
        let target = cache.bytes() - highest_bytes;

        let eviction = cache.evict_to_bytes(target, &BTreeSet::new());

        assert_eq!(eviction.entries_removed, 1);
        assert_eq!(eviction.bytes_removed, highest_bytes);
        assert!(cache.get(highest).is_none());
        assert!(cache.iter().all(|(signature, _)| *signature < highest));
    }

    #[test]
    fn removed_entry_rebuilds_identically_when_it_becomes_required() {
        let mut cache = RosterCache::default();
        let signature = sig(Biome::TemperateForest, 4, 3, 2);
        let original = cache.ensure(signature);
        assert!(original.bytes() > 0);
        assert_eq!(cache.take_builds(), 1);

        let eviction = cache.evict_to_bytes(0, &BTreeSet::new());
        assert_eq!(eviction.entries_removed, 1);
        assert!(cache.get(signature).is_none());

        let rebuilt = cache.ensure(signature);
        assert_eq!(*rebuilt, *original);
        assert!(!Arc::ptr_eq(&rebuilt, &original));
        assert_eq!(cache.take_builds(), 1);

        let protected: BTreeSet<_> = [signature].into_iter().collect();
        assert_eq!(
            cache.evict_to_bytes(0, &protected),
            RosterEviction::default()
        );
        assert!(cache.get(signature).is_some());
    }
}
