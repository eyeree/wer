//! Compatibility exports for shared GPU-map atlas preparation.
//!
//! Atlas assignment, region packing, channel mapping, refinement parameters,
//! and their tests live in `viewer-host` so native and browser presenters use
//! one implementation (`native-web-alignment.md` Milestone 4).

pub use viewer_host::atlas::{gpu_channel, refinement_octaves, AtlasManager};
