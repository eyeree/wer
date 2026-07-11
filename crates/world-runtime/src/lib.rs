//! `world-runtime` — platform-neutral orchestration of the world model.
//!
//! This crate coordinates *when* and *where* generation happens: region
//! lifecycle, streaming policy, dependency management, and generation
//! scheduling. Like `world-core` it must compile for native and wasm, so all
//! platform capabilities it needs (storage, task execution) are expressed as
//! abstract traits here and implemented by the platform crates
//! (`platform-native`, `platform-web`). See sections 16 and 19 of the plan.

pub mod region;
pub mod storage;
pub mod task;

pub use region::{GenerationStatus, RegionState};
pub use storage::{Storage, StorageError};
pub use task::{TaskExecutor, TaskPriority};
