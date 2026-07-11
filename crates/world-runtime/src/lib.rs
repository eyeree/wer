//! `world-runtime` — platform-neutral orchestration of the world model.
//!
//! This crate coordinates *when* and *where* generation happens: region
//! lifecycle, streaming policy, dependency management, and generation
//! scheduling. Like `world-core` it must compile for native and wasm, so all
//! platform capabilities it needs (storage, task execution) are expressed as
//! abstract traits here and implemented by the platform crates
//! (`platform-native`, `platform-web`). See sections 16 and 19 of the plan.

pub mod budget;
pub mod generate;
pub mod region;
pub mod storage;
pub mod stream;
pub mod task;

pub use budget::Budget;
pub use generate::{
    generate_layer, GeneratedTile, RegionCache, RegionTiles, CHANNEL_COUNT, CHANNEL_ELEVATION,
    CHANNEL_MOISTURE, CHANNEL_TEMPERATURE, CHANNEL_VEGETATION,
};
pub use region::{GenerationStatus, RegionState};
pub use storage::{Storage, StorageError};
pub use stream::{stability_for, FrameStats, RegionMap, StreamConfig};
pub use task::{InlineExecutor, TaskExecutor, TaskPriority};
