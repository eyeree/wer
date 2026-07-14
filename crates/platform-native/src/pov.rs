//! POV mode host, re-exported from the shared [`pov_host`] crate
//! (phase-7-plan.md §9.9): the fly/walk camera, the pure region mesher, and
//! the chunk lifecycle manager are one implementation for the native shell
//! and the browser shell. This module exists so the shell keeps its
//! `crate::pov::` paths; nothing native-specific remains here.

pub use pov_host::*;
