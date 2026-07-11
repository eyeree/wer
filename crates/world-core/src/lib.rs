//! `world-core` — the platform-neutral, deterministic heart of the world model.
//!
//! This crate contains only pure computation: deterministic hashing, coordinate
//! systems, and the possibility-space representation. It must compile for both
//! native and `wasm32-unknown-unknown` targets, so it may not touch the
//! filesystem, spawn threads, open sockets, or call platform graphics APIs.
//!
//! See `docs/adr/0002-workspace-crate-boundaries.md` and section 19 of
//! `implementation-plan.md` (Browser Portability Requirements).
//!
//! Permanent feature identities are derived from integer hashing over stable
//! inputs (see [`hash`]); floating point is reserved for approximate simulation
//! and presentation only (section 6.2 of the plan).

// Portability guard: `world-core` must not accidentally pull in `std`-only
// facilities that break the wasm build. We stay on `std` for now (allocation,
// collections) but forbid the obviously non-portable pieces via review + CI.

pub mod anchor;
pub mod climate;
pub mod coord;
pub mod ecology;
pub mod field;
pub mod hash;
pub mod layer;
pub mod possibility;
pub mod possibility_field;
pub mod terrain;

pub use anchor::{domain_mask, project_plausible, steer, Anchor, AnchorKind};
pub use climate::{climate, Climate};
pub use coord::{LocalPos, RegionCoord, REGION_SIZE};
pub use ecology::vegetation_density;
pub use field::{FieldTile, FIELD_RES};
pub use hash::{feature_hash, mix, splitmix64, FeatureKey, Rng};
pub use possibility::{PossibilityDomain, PossibilityVector, POSSIBILITY_DIMS};
pub use possibility_field::PossibilityField;
pub use terrain::{elevation, is_water, SEA_LEVEL};

/// Version of the world-generation algorithms. Any change that alters generated
/// output for the same inputs MUST bump this so persisted worlds can detect that
/// their deterministic base has changed (section 18, Persistence).
pub const WORLD_ALGORITHM_VERSION: u32 = 1;
