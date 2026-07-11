//! Abstract task-execution interface (section 16 of the plan).
//!
//! Generation work is expressed as coarse region/layer jobs. The runtime submits
//! them through this trait rather than depending on a concrete scheduler, so the
//! same code can run on a native Rayon pool or a browser Web Worker pool. Jobs
//! must be safe to cancel or supersede (section 6.6).

/// Relative importance of a job, used by the executor to order work within the
/// per-frame budget. Higher = sooner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    /// Speculative work (regions the player probably won't reach soon).
    Background,
    /// Regions likely to be entered or currently transforming.
    Normal,
    /// Visible, near-field work that gates presentation.
    Critical,
}

/// Submits CPU work for parallel execution.
///
/// The interface is intentionally minimal for the bootstrap: a fire-and-forget
/// submission of a boxed closure with a priority. It will grow dependency
/// tracking, cancellation handles, and output-revision plumbing as the job
/// system plan (`job-system-plan.md`) is written.
pub trait TaskExecutor {
    /// Submit `job` to run at `priority`. The executor may run it on another
    /// thread/worker, so the closure is `Send`.
    fn submit(&self, priority: TaskPriority, job: Box<dyn FnOnce() + Send>);

    /// Number of parallel execution lanes available (threads or workers), for
    /// budgeting. Returns at least 1.
    fn parallelism(&self) -> usize;
}
