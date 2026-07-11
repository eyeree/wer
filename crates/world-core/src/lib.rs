//! `world-core` — the platform-neutral, deterministic heart of the world model.
//!
//! This crate contains only pure computation: deterministic hashing, coordinate
//! systems, the possibility-space representation, and the Phase 2 layered
//! environmental generators. It must compile for both native and
//! `wasm32-unknown-unknown` targets, so it may not touch the filesystem, spawn
//! threads, open sockets, or call platform graphics APIs.
//!
//! See `docs/adr/0002-workspace-crate-boundaries.md` and section 19 of
//! `implementation-plan.md` (Browser Portability Requirements).
//!
//! Permanent feature identities are derived from integer hashing over stable
//! inputs (see [`hash`]); floating point is reserved for approximate simulation
//! and presentation only (section 6.2 of the plan). Which layer depends on
//! what is declared statically in [`layer`]; tiles are functions of their
//! dependency hash ([`dephash`], ADR 0008).

// Portability guard: `world-core` must not accidentally pull in `std`-only
// facilities that break the wasm build. We stay on `std` for now (allocation,
// collections) but forbid the obviously non-portable pieces via review + CI.

pub mod anchor;
pub mod biome;
pub mod climate;
pub mod coord;
pub mod dephash;
pub mod drainage;
pub mod field;
pub mod foodweb;
pub mod genome;
pub mod geology;
pub mod habitat;
pub mod hash;
pub mod hydrology;
pub mod layer;
pub mod population;
pub mod possibility;
pub mod possibility_field;
pub mod soils;
pub mod species;
pub mod terrain;
pub mod vegetation;

pub use anchor::{domain_mask, project_plausible, steer, Anchor, AnchorKind};
pub use biome::{classify, Biome, BIOME_COUNT};
pub use climate::{climate, Climate};
pub use coord::{LocalPos, RegionCoord, REGION_SIZE};
pub use dephash::{drainage_dep_hash, drainage_dep_hash_default, layer_dep_hash};
pub use drainage::{drainage, macro_coord_for, DrainageTile, MACRO_GRID, MACRO_LEVEL};
pub use field::{FieldTile, FIELD_RES};
pub use foodweb::{food_web, max_body_size, species_biomass, FoodWeb};
pub use genome::{
    size_class_units, AppearanceGenes, BehaviorGenes, Expressed, Genome, GenomeBias, NicheGenes,
};
pub use geology::{geology, lithology_id, lithology_seed, Geology};
pub use habitat::HabitatSignature;
pub use hash::{feature_hash, mix, splitmix64, FeatureKey, Rng};
pub use hydrology::{hydrology, Hydrology};
pub use layer::{
    all_layers_mask, dependents_closure, domain_dirty_mask, domain_readers, layer_bit, layer_decl,
    LayerDecl, LAYERS, LAYER_COUNT,
};
pub use population::{diversity_of, population, signature_productivity, PopulationSample};
pub use possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS, POSSIBILITY_QUANT};
pub use possibility_field::PossibilityField;
pub use soils::{soils, Soils};
pub use species::{
    species_roster, species_seed, Species, SpeciesRoster, Trophic, ROSTER_MAX, TROPHIC_TIERS,
};
pub use terrain::{elevation, is_water, SEA_LEVEL};
pub use vegetation::{vegetation, Vegetation};

/// Version of the world-generation algorithms. Any change that alters generated
/// output for the same inputs MUST bump this so persisted worlds can detect that
/// their deterministic base has changed (section 18, Persistence).
///
/// Bumped 1 → 2 by Phase 2 milestone M1: the layer stack was generalized into
/// the declared dependency graph, generators consume quantized possibility
/// inputs, and the new environmental layers landed (phase-2-plan.md §9.1) —
/// the one sanctioned golden-fixture re-bless of the phase. Subsequent layer
/// tuning bumps the layer's `algorithm_revision` instead (§9.2).
pub const WORLD_ALGORITHM_VERSION: u32 = 2;
