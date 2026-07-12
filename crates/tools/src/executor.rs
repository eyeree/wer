//! The native priority-lane executor (phase-6-plan.md §6.2) — the job-system
//! slice: three FIFO lanes drained Critical > Normal > Background by a small
//! std thread pool, honoring the [`TaskPriority`] the `TaskExecutor` trait
//! has declared since Phase 0.
//!
//! Hosted here rather than in `platform-native` so the headless harnesses
//! (the continuity replay's Inline-vs-Lane state-hash equality, `wer-scale`'s
//! schedule-independence gates, ADR 0018) can drive the *production*
//! scheduler; the shell re-exports it (`platform-native/src/executor.rs`).
//! `tools` is a native platform crate, so spawning threads here keeps the
//! neutral-crate rule intact: `world-core`/`world-runtime` still spawn
//! nothing.
//!
//! Determinism: none is required of the executor beyond what Phase 0
//! established — jobs are pure, results integrate keyed by job id and
//! dependency hash, and completion order never matters. That claim is
//! machine-checked (not just asserted) by the ADR 0018 gates. Cancellation
//! rides the token captured inside each job closure
//! (`world-runtime`'s `stream.rs`); the executor itself needs no cancel API.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use world_runtime::{TaskExecutor, TaskPriority};

type Job = Box<dyn FnOnce() + Send>;

/// Queue state shared between submitters and workers. One `VecDeque` per
/// lane, indexed by `TaskPriority as usize` (Background = 0 … Critical = 2).
#[derive(Default)]
struct Lanes {
    queues: [VecDeque<Job>; 3],
    shutdown: bool,
}

struct Shared {
    lanes: Mutex<Lanes>,
    ready: Condvar,
}

/// Executes jobs on `N` std worker threads, draining the highest-priority
/// non-empty lane first. FIFO within a lane. Dropping the executor wakes and
/// joins every worker; queued jobs that have not started are discarded
/// (their results were fire-and-forget anyway, and the owning `RegionMap` is
/// gone with them).
pub struct LaneExecutor {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl fmt::Debug for LaneExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneExecutor")
            .field("workers", &self.workers.len())
            .field("queued", &self.queued())
            .finish()
    }
}

impl LaneExecutor {
    /// An executor with `workers` threads (clamped to at least 1).
    #[must_use]
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            lanes: Mutex::new(Lanes::default()),
            ready: Condvar::new(),
        });
        let workers = (1..=workers.max(1))
            .map(|i| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("wer-worker-{i}"))
                    .spawn(move || worker_loop(&shared))
                    .expect("spawn worker thread")
            })
            .collect();
        Self { shared, workers }
    }

    /// An executor sized for this machine: `available_parallelism() - 1`
    /// workers (the main thread keeps a core), at least 1.
    #[must_use]
    pub fn auto() -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self::new(cores.saturating_sub(1))
    }

    /// Jobs queued but not yet started, per lane (Background, Normal,
    /// Critical) — panel telemetry.
    #[must_use]
    pub fn queued_per_lane(&self) -> [usize; 3] {
        let lanes = self.shared.lanes.lock().expect("executor lock");
        [
            lanes.queues[0].len(),
            lanes.queues[1].len(),
            lanes.queues[2].len(),
        ]
    }

    /// Total jobs queued but not yet started.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queued_per_lane().iter().sum()
    }
}

fn worker_loop(shared: &Shared) {
    let mut lanes = shared.lanes.lock().expect("executor lock");
    loop {
        // Drain Critical > Normal > Background, FIFO within a lane.
        let job = lanes
            .queues
            .iter_mut()
            .rev()
            .find_map(std::collections::VecDeque::pop_front);
        if let Some(job) = job {
            drop(lanes);
            job();
            lanes = shared.lanes.lock().expect("executor lock");
            continue;
        }
        if lanes.shutdown {
            return;
        }
        lanes = shared.ready.wait(lanes).expect("executor lock");
    }
}

impl TaskExecutor for LaneExecutor {
    fn submit(&self, priority: TaskPriority, job: Box<dyn FnOnce() + Send>) {
        let mut lanes = self.shared.lanes.lock().expect("executor lock");
        lanes.queues[priority as usize].push_back(job);
        drop(lanes);
        self.shared.ready.notify_one();
    }

    fn parallelism(&self) -> usize {
        self.workers.len().max(1)
    }
}

impl Drop for LaneExecutor {
    fn drop(&mut self) {
        {
            let mut lanes = self.shared.lanes.lock().expect("executor lock");
            lanes.shutdown = true;
        }
        self.shared.ready.notify_all();
        for handle in self.workers.drain(..) {
            // A worker that panicked already poisoned nothing we still read;
            // shutdown proceeds regardless.
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;

    #[test]
    fn runs_submitted_jobs_and_joins_on_drop() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let exec = LaneExecutor::new(4);
            let (tx, rx) = channel();
            for _ in 0..64 {
                let counter = Arc::clone(&counter);
                let tx = tx.clone();
                exec.submit(
                    TaskPriority::Normal,
                    Box::new(move || {
                        counter.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(());
                    }),
                );
            }
            // Wait for all results so none are lost to shutdown discard.
            for _ in 0..64 {
                rx.recv().expect("job completed");
            }
        }
        assert_eq!(counter.load(Ordering::Relaxed), 64);
    }

    #[test]
    fn critical_lane_drains_before_background() {
        // One worker; the first job blocks until everything is queued, then
        // the drain order of the rest must be priority-first.
        let exec = LaneExecutor::new(1);
        let order = Arc::new(Mutex::new(Vec::new()));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        {
            let gate = Arc::clone(&gate);
            exec.submit(
                TaskPriority::Normal,
                Box::new(move || {
                    let (lock, cv) = &*gate;
                    let mut open = lock.lock().unwrap();
                    while !*open {
                        open = cv.wait(open).unwrap();
                    }
                }),
            );
        }
        let (tx, rx) = channel();
        for (priority, label) in [
            (TaskPriority::Background, "bg1"),
            (TaskPriority::Background, "bg2"),
            (TaskPriority::Critical, "crit"),
            (TaskPriority::Normal, "norm"),
        ] {
            let order = Arc::clone(&order);
            let tx = tx.clone();
            exec.submit(
                priority,
                Box::new(move || {
                    order.lock().unwrap().push(label);
                    let _ = tx.send(());
                }),
            );
        }
        // Open the gate; the worker now drains the queues.
        {
            let (lock, cv) = &*gate;
            *lock.lock().unwrap() = true;
            cv.notify_all();
        }
        for _ in 0..4 {
            rx.recv().expect("job completed");
        }
        assert_eq!(*order.lock().unwrap(), vec!["crit", "norm", "bg1", "bg2"]);
    }

    #[test]
    fn parallelism_reports_worker_count() {
        let exec = LaneExecutor::new(3);
        assert_eq!(exec.parallelism(), 3);
        assert!(LaneExecutor::auto().parallelism() >= 1);
    }
}
