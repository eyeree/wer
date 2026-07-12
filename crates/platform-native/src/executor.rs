//! Native [`world_runtime::TaskExecutor`] selection (phase-6-plan.md §6.2).
//!
//! Phase 6 replaced the Phase 1 Rayon shim with the [`LaneExecutor`] — three
//! FIFO priority lanes drained Critical > Normal > Background, plus the
//! cancellation tokens the runtime rides through job closures — justified by
//! the M1 measurements recorded in `docs/perf-baseline.md` (39% of executed
//! jobs superseded during drift storms; hundreds of lower-priority jobs
//! ahead of Critical submissions under a constrained FIFO). `rayon` left the
//! workspace with it.
//!
//! The implementation lives in `tools::executor` so the headless harnesses
//! (continuity replay, `wer-scale`'s ADR 0018 gates) drive the *production*
//! scheduler; this module is the shell's door to it. `wer --inline` keeps
//! the synchronous [`world_runtime::InlineExecutor`] available for A/B runs.

pub use tools::executor::LaneExecutor;
