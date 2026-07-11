//! `world-runtime` — platform-neutral orchestration of the world model.
//!
//! This crate coordinates *when* and *where* generation happens: region
//! lifecycle, streaming policy, dependency management, and generation
//! scheduling. Like `world-core` it must compile for native and wasm, so all
//! platform capabilities it needs (storage, task execution) are expressed as
//! abstract traits here and implemented by the platform crates
//! (`platform-native`, `platform-web`). See sections 16 and 19 of the plan.
//!
//! Phase 2: staleness is dependency-hash comparison against the declared
//! layer graph (ADR 0007/0008), dispatch is topological, regeneration is
//! budgeted by declared cost, and macro drainage tiles live in their own
//! dependent-tracked cache (phase-2-plan.md §5.2, §6.3, §8).

pub mod budget;
pub mod generate;
pub mod macrocache;
pub mod realize;
pub mod region;
pub mod resonance;
pub mod rostercache;
pub mod storage;
pub mod stream;
pub mod task;

pub use budget::Budget;
pub use generate::{
    generate_layer, layer_channels, GeneratedTile, LayerInputs, RegionCache, RegionTiles,
    CHANNEL_CANOPY, CHANNEL_COUNT, CHANNEL_DIVERSITY, CHANNEL_ELEVATION, CHANNEL_FERTILITY,
    CHANNEL_HARDNESS, CHANNEL_HERBIVORE, CHANNEL_MOISTURE, CHANNEL_PREDATOR, CHANNEL_RIVER,
    CHANNEL_SOIL_DEPTH, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION, CHANNEL_WETNESS,
};
pub use macrocache::MacroCache;
pub use realize::{realize_region, Organism};
pub use region::{GenerationStatus, RegionState};
pub use resonance::{Resonance, ResonanceNode};
pub use rostercache::{RosterCache, RosterEntry, RosterSnapshot};
pub use storage::{Storage, StorageError};
pub use stream::{
    stability_for, CellEcology, FrameStats, LayerDiagnostic, RegionMap, StreamConfig,
};
pub use task::{InlineExecutor, TaskExecutor, TaskPriority};
