//! Native [`TaskExecutor`]: generation jobs on the Rayon global pool
//! (phase-1-plan.md section 9, milestone M6).
//!
//! Jobs are pure and post their owned results back through a channel, so this
//! executor needs no bookkeeping of its own: completion order is free to differ
//! from submission order, and the runtime's integration step is explicitly
//! order-independent — the same contract a Web Worker executor will satisfy in
//! the browser (implementation-plan.md section 19).

use world_runtime::{TaskExecutor, TaskPriority};

/// Executes jobs on the Rayon global thread pool.
#[derive(Debug, Default, Clone, Copy)]
pub struct RayonExecutor;

impl TaskExecutor for RayonExecutor {
    fn submit(&self, _priority: TaskPriority, job: Box<dyn FnOnce() + Send>) {
        // Phase 1 ignores priority: the per-frame dispatch budget already
        // orders work nearest-first, and Rayon's queue is FIFO enough at this
        // job count. Priority lanes arrive with the job-system plan.
        rayon::spawn(job);
    }

    fn parallelism(&self) -> usize {
        rayon::current_num_threads().max(1)
    }
}
