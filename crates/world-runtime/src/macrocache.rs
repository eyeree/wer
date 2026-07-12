//! The macro drainage cache (phase-2-plan.md §6.3, milestone M4).
//!
//! Drainage tiles are keyed by their macro [`RegionCoord`] (level
//! [`world_core::drainage::MACRO_LEVEL`]) and shared by every level-0 region
//! under them via cheap `Arc` clones (tiles are immutable once integrated).
//! Orphan sweeping considers only field-active level-0 dependents; the separate
//! byte target may evict a clean macro earlier and dependency repair rebuilds it
//! on demand. Capacity-parked authority never pins derived inputs (ADR 0023).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use world_core::{macro_coord_for, DrainageTile, RegionCoord};

/// Cache of macro drainage tiles for the field-active working set. A
/// `BTreeMap` for the same reason as the region map: deterministic iteration
/// order is part of the replay's two-run equality contract.
#[derive(Debug, Default)]
pub struct MacroCache {
    tiles: BTreeMap<RegionCoord, Arc<DrainageTile>>,
}

impl MacroCache {
    /// The tile for a macro coordinate, if generated.
    #[inline]
    #[must_use]
    pub fn get(&self, macro_coord: RegionCoord) -> Option<&Arc<DrainageTile>> {
        self.tiles.get(&macro_coord)
    }

    /// Store a finished tile (replacing any stale predecessor).
    pub fn insert(&mut self, tile: Arc<DrainageTile>) {
        self.tiles.insert(tile.coord(), tile);
    }

    /// Drop every tile whose macro coordinate no longer covers a coordinate in
    /// `field_active` (dependent-tracked eviction, ADR 0023).
    pub fn evict_orphans<'a>(&mut self, field_active: impl Iterator<Item = &'a RegionCoord>) {
        let needed: BTreeSet<RegionCoord> = field_active.map(|&c| macro_coord_for(c)).collect();
        self.tiles.retain(|coord, _| needed.contains(coord));
    }

    /// Remove one tile (the Phase 6 capacity evictor, phase-6-plan.md §4.3).
    /// Safe under ADR 0008: the tile re-derives bit-identically whenever a
    /// dependent next needs it.
    pub fn remove(&mut self, macro_coord: RegionCoord) -> Option<Arc<DrainageTile>> {
        self.tiles.remove(&macro_coord)
    }

    /// Number of resident macro tiles.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether the cache is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Total heap bytes held by macro tiles (telemetry, phase-2-plan.md §13).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.tiles.values().map(|t| t.bytes()).sum()
    }

    /// Iterate tiles in deterministic coordinate order.
    pub fn iter(&self) -> impl Iterator<Item = (&RegionCoord, &Arc<DrainageTile>)> {
        self.tiles.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_core::drainage::{drainage, MACRO_LEVEL};
    use world_core::PossibilityField;

    #[test]
    fn dependent_tracking_evicts_orphans() {
        let field = PossibilityField::default();
        let mut cache = MacroCache::default();
        let mc0 = RegionCoord::at_level(0, 0, MACRO_LEVEL);
        let mc1 = RegionCoord::at_level(1, 0, MACRO_LEVEL);
        cache.insert(Arc::new(drainage(mc0, &field, 1)));
        cache.insert(Arc::new(drainage(mc1, &field, 2)));
        assert_eq!(cache.len(), 2);
        assert!(cache.bytes() > 0);

        // Only regions under mc0 remain resident.
        let resident = [RegionCoord::new(3, 5), RegionCoord::new(15, 15)];
        cache.evict_orphans(resident.iter());
        assert!(cache.get(mc0).is_some());
        assert!(cache.get(mc1).is_none());

        cache.evict_orphans([].iter());
        assert!(cache.is_empty());
    }
}
