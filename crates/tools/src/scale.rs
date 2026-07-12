//! `wer-scale` — the Phase 6 performance-and-scale harness
//! (phase-6-plan.md §11.4): scripted stress scenarios, machine-checked on
//! deterministic counts/bytes/hashes, with wall-clock *reported* for the
//! committed baseline (`docs/perf-baseline.md`) but never gated (§12.6).
//!
//! M1 ships the skeleton: the long-haul and teleport-storm scripts, the
//! backpressure drain gates, and the two measurements that decide the
//! LaneExecutor's go/no-go (§6.2) — priority inversion and wasted
//! (superseded) work — taken on the [`QueueExecutor`], a deterministic
//! stand-in for a threaded pool. Later milestones grow the schedule-
//! independence, memory-ceiling, and density scenario families.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use world_core::{
    domain_mask, encode_record, Anchor, AnchorKind, AnchorSource, PossibilityDomain,
    PossibilityField, PossibilityVector, RecordKind, RegionCoord, RouteRecord, POSSIBILITY_DIMS,
    REGION_SIZE,
};
use world_runtime::{
    full_region_payload_bytes, Budget, FrameStats, GenerationStatus, InlineExecutor, MemoryStorage,
    Organism, Pass, RegionMap, Resonance, RouteRecorder, StreamConfig, TaskExecutor, TaskPriority,
    Vault, PASS_COUNT,
};

use crate::executor::LaneExecutor;
use crate::replay::{regional_history_hash, state_hash};

type QueuedJob = (TaskPriority, Box<dyn FnOnce() + Send>);

/// A deterministic stand-in for a threaded executor: jobs queue FIFO at
/// submission and run *on the calling thread* when the harness pumps the
/// queue between frames, simulating bounded worker throughput without any
/// actual threads — so every run is bit-reproducible, and queue-shape
/// measurements (jobs ahead of a Critical submission, superseded results)
/// are exact counts rather than racy wall-clock samples.
///
/// This is the measurement instrument behind the §6.2 go/no-go evidence; the
/// real priority-lane executor is the native shell's.
#[derive(Default)]
pub struct QueueExecutor {
    queue: RefCell<VecDeque<QueuedJob>>,
    /// Jobs actually executed.
    pub executed: Cell<u64>,
    /// Total lower-priority jobs queued ahead of Critical submissions — the
    /// priority-inversion measure (a lane executor would drain Critical
    /// first, making this 0).
    pub critical_blocked_by: Cell<u64>,
    /// Critical jobs submitted.
    pub critical_submitted: Cell<u64>,
    /// Peak queue depth observed.
    pub max_queue: Cell<usize>,
}

impl fmt::Debug for QueueExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueExecutor")
            .field("queued", &self.queue.borrow().len())
            .field("executed", &self.executed.get())
            .finish_non_exhaustive()
    }
}

impl QueueExecutor {
    /// Run up to `n` queued jobs in FIFO order (the throughput of one
    /// simulated frame of worker time).
    pub fn run_jobs(&self, n: usize) {
        for _ in 0..n {
            let Some((_, job)) = self.queue.borrow_mut().pop_front() else {
                return;
            };
            self.executed.set(self.executed.get() + 1);
            job();
        }
    }

    /// Jobs still queued.
    #[must_use]
    pub fn queue_len(&self) -> usize {
        self.queue.borrow().len()
    }
}

impl TaskExecutor for QueueExecutor {
    fn submit(&self, priority: TaskPriority, job: Box<dyn FnOnce() + Send>) {
        let mut queue = self.queue.borrow_mut();
        if priority == TaskPriority::Critical {
            self.critical_submitted
                .set(self.critical_submitted.get() + 1);
            let ahead = queue
                .iter()
                .filter(|(p, _)| *p < TaskPriority::Critical)
                .count() as u64;
            self.critical_blocked_by
                .set(self.critical_blocked_by.get() + ahead);
        }
        queue.push_back((priority, job));
        self.max_queue.set(self.max_queue.get().max(queue.len()));
    }

    fn parallelism(&self) -> usize {
        1
    }
}

/// One machine-checked gate of a scenario.
#[derive(Debug)]
pub struct Gate {
    /// What the gate asserts.
    pub name: String,
    /// Whether it held.
    pub passed: bool,
    /// The observed numbers behind the verdict.
    pub detail: String,
}

/// The outcome of one scenario: its gates plus the metrics the baseline
/// document snapshots.
#[derive(Debug)]
pub struct ScenarioOutcome {
    /// Scenario name.
    pub name: &'static str,
    /// Frames simulated.
    pub frames: u32,
    /// Machine-checked gates (counts/bytes/hashes only — never wall-clock).
    pub gates: Vec<Gate>,
    /// Ordered `(label, value)` metrics for `--report`.
    pub metrics: Vec<(String, f64)>,
    /// Mean per-pass milliseconds over the run (wall-clock; report-only).
    pub pass_ms_avg: [f64; PASS_COUNT],
    /// Peak per-pass milliseconds over the run (wall-clock; report-only).
    pub pass_ms_max: [f64; PASS_COUNT],
}

impl ScenarioOutcome {
    /// Whether every gate held.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.gates.iter().all(|g| g.passed)
    }
}

/// The whole harness run.
#[derive(Debug)]
pub struct ScaleReport {
    /// Every scenario's outcome.
    pub scenarios: Vec<ScenarioOutcome>,
}

impl ScaleReport {
    /// Whether every scenario passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.scenarios.iter().all(ScenarioOutcome::passed)
    }
}

/// Sizing for a harness run. The default is the full sign-off run; `quick()`
/// is sized for CI and `cargo test`.
#[derive(Debug, Clone)]
pub struct ScaleConfig {
    /// Streaming window under test.
    pub stream: StreamConfig,
    /// Frame budget under test.
    pub budget: Budget,
    /// Frames of travel in the long-haul scenario.
    pub long_haul_frames: u32,
    /// World units traveled per long-haul frame.
    pub velocity: (f64, f64),
    /// Far teleports in the teleport storm.
    pub teleports: u32,
    /// Frames a teleport may take to settle before the gate fails.
    pub settle_limit: u32,
    /// Frames the backlog may take to drain after pressure stops.
    pub drain_limit: u32,
    /// Simulated worker throughput: jobs the [`QueueExecutor`] runs per frame.
    pub jobs_per_frame: usize,
}

impl Default for ScaleConfig {
    fn default() -> Self {
        Self {
            stream: StreamConfig {
                // The replay's reduced resolution keeps headless runs brisk
                // while exercising the full pipeline shape.
                field_resolution: 8,
                ..StreamConfig::default()
            },
            budget: Budget::default(),
            long_haul_frames: 1200, // ~52k units at the default velocity
            velocity: (37.0, 23.0),
            teleports: 6,
            settle_limit: 600,
            drain_limit: 300,
            // Simulated worker throughput. Sub-millisecond kernels on a
            // handful of workers clear a frame's dispatches within a frame
            // or two; 64/frame models that without letting the queue lag
            // unrealistically far behind the dispatcher.
            jobs_per_frame: 64,
        }
    }
}

impl ScaleConfig {
    /// A reduced run for CI and `cargo test`.
    #[must_use]
    pub fn quick() -> Self {
        Self {
            stream: StreamConfig {
                near_radius: 2.0 * REGION_SIZE,
                far_radius: 5.0 * REGION_SIZE,
                load_radius: 6.0 * REGION_SIZE,
                unload_radius: 7.5 * REGION_SIZE,
                field_resolution: 8,
                ..StreamConfig::default()
            },
            long_haul_frames: 240,
            teleports: 3,
            settle_limit: 400,
            drain_limit: 200,
            ..Self::default()
        }
    }
}

/// Whether every field-active region is fully generated, every near canonical
/// realization is current, every parked region is intentionally quiescent, and
/// no work is queued or in flight.
fn settled(map: &RegionMap, exec: &QueueExecutor, player: (f64, f64)) -> bool {
    exec.queue_len() == 0
        && map.jobs_in_flight() == 0
        && map.authoritative_realization_complete(player)
        && map
            .iter_active()
            .all(|r| r.status != GenerationStatus::Generating)
}

/// The scripted bias storm for the long haul: a repeating ramp that pushes
/// the fast domains around, forcing bucket flips and regeneration ripples —
/// deterministic, fast-domains-only (the stable trio must hold).
fn storm_bias(frame: u32) -> [f32; POSSIBILITY_DIMS] {
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    let t = (frame % 240) as f32 / 240.0;
    let ramp = if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 };
    bias[PossibilityDomain::Ecology.index()] = 0.35 * ramp;
    bias[PossibilityDomain::Hydrology.index()] = 0.30 * ramp;
    bias[PossibilityDomain::Climate.index()] = -0.25 * ramp;
    bias
}

/// Accumulates per-frame stats into the numbers the report prints.
#[derive(Debug, Default)]
struct Accum {
    frames: u32,
    pass_sum: [f64; PASS_COUNT],
    pass_max: [f64; PASS_COUNT],
    dispatched: u64,
    integrated: u64,
    cancelled: u64,
    dropped: u64,
    peak_deferred_regens: usize,
    peak_cache_bytes: usize,
    peak_regions: usize,
}

impl Accum {
    fn absorb(&mut self, stats: &FrameStats) {
        self.frames += 1;
        for (i, &ms) in stats.pass_ms.iter().enumerate() {
            self.pass_sum[i] += f64::from(ms);
            self.pass_max[i] = self.pass_max[i].max(f64::from(ms));
        }
        self.dispatched += stats.layers_dispatched as u64;
        self.integrated += stats.layers_regenerated as u64;
        self.cancelled += stats.jobs_cancelled as u64;
        self.dropped += stats.results_dropped as u64;
        self.peak_deferred_regens = self.peak_deferred_regens.max(stats.deferred_regens);
        self.peak_cache_bytes = self
            .peak_cache_bytes
            .max(stats.cache_bytes + stats.macro_cache_bytes + stats.roster_cache_bytes);
        self.peak_regions = self.peak_regions.max(stats.active_regions);
    }

    fn pass_avg(&self) -> [f64; PASS_COUNT] {
        let mut avg = [0.0; PASS_COUNT];
        let n = f64::from(self.frames.max(1));
        for (a, s) in avg.iter_mut().zip(&self.pass_sum) {
            *a = s / n;
        }
        avg
    }
}

/// Long-haul travel under bias storms, then a full stop: backpressure must
/// stay bounded while traveling and *drain* once pressure stops — a backlog
/// that only grows is a failed budget, not backpressure (§1.2).
#[must_use]
pub fn long_haul(cfg: &ScaleConfig) -> ScenarioOutcome {
    let field = PossibilityField::default();
    let mut map = RegionMap::new(cfg.stream);
    let exec = QueueExecutor::default();
    let mut acc = Accum::default();
    let travel = f64::hypot(cfg.velocity.0, cfg.velocity.1);

    // Deferred counters sampled over the last quarter vs the run mean: an
    // oscillating (healthy) backlog has a bounded tail; a diverging one
    // fails the gate.
    let mut deferred_series: Vec<usize> = Vec::with_capacity(cfg.long_haul_frames as usize);

    for frame in 0..cfg.long_haul_frames {
        let player = (
            f64::from(frame) * cfg.velocity.0,
            f64::from(frame) * cfg.velocity.1,
        );
        let bias = storm_bias(frame);
        let stats = map.update(
            player,
            if frame == 0 { 0.0 } else { travel },
            &field,
            &[],
            &bias,
            &cfg.budget,
            &exec,
            false,
        );
        exec.run_jobs(cfg.jobs_per_frame);
        deferred_series.push(stats.deferred_regens + stats.deferred_loads);
        acc.absorb(&stats);
    }

    // Pressure release: hold still (bias back to neutral) and count frames
    // until the whole window settles.
    let stop = (
        f64::from(cfg.long_haul_frames) * cfg.velocity.0,
        f64::from(cfg.long_haul_frames) * cfg.velocity.1,
    );
    let neutral = [0.0f32; POSSIBILITY_DIMS];
    let mut drain_frames = None;
    for frame in 0..cfg.drain_limit {
        let stats = map.update(stop, 0.0, &field, &[], &neutral, &cfg.budget, &exec, false);
        exec.run_jobs(cfg.jobs_per_frame);
        acc.absorb(&stats);
        if stats.deferred_regens == 0 && stats.deferred_loads == 0 && settled(&map, &exec, stop) {
            drain_frames = Some(frame + 1);
            break;
        }
    }

    let tail_start = deferred_series.len() * 3 / 4;
    let mean = deferred_series.iter().sum::<usize>() as f64 / deferred_series.len().max(1) as f64;
    let tail_mean = deferred_series[tail_start..].iter().sum::<usize>() as f64
        / deferred_series[tail_start..].len().max(1) as f64;
    // Bounded means the tail is not materially above the run mean (divergence
    // would show the backlog compounding). The +4 floor forgives tiny means.
    let bounded = tail_mean <= mean * 2.0 + 4.0;

    let gates = vec![
        Gate {
            name: "backpressure bounded (deferred tail vs mean)".into(),
            passed: bounded,
            detail: format!("run mean {mean:.1}, last-quarter mean {tail_mean:.1}"),
        },
        Gate {
            name: format!("backlog drains after stop (≤ {} frames)", cfg.drain_limit),
            passed: drain_frames.is_some(),
            detail: match drain_frames {
                Some(f) => format!("drained in {f} frames"),
                None => format!("still undrained after {} frames", cfg.drain_limit),
            },
        },
    ];

    // Kernel runs = results actually produced (integrated + arrived-but-
    // dropped); cancelled jobs no-op before their kernel, so the split
    // between `jobs cancelled` and `wasted kernel fraction` is exactly the
    // worker time cancellation reclaims (§6.2).
    let kernel_runs = acc.integrated + acc.dropped;
    let metrics = vec![
        ("peak resident regions".into(), acc.peak_regions as f64),
        (
            "peak cache MB".into(),
            acc.peak_cache_bytes as f64 / (1024.0 * 1024.0),
        ),
        (
            "peak deferred regens".into(),
            acc.peak_deferred_regens as f64,
        ),
        ("jobs dispatched".into(), acc.dispatched as f64),
        ("kernel runs".into(), kernel_runs as f64),
        ("jobs cancelled before running".into(), acc.cancelled as f64),
        ("results superseded on arrival".into(), acc.dropped as f64),
        (
            "wasted kernel fraction".into(),
            acc.dropped as f64 / kernel_runs.max(1) as f64,
        ),
        (
            "drain frames".into(),
            drain_frames.map_or(f64::NAN, f64::from),
        ),
    ];

    ScenarioOutcome {
        name: "long-haul",
        frames: acc.frames,
        gates,
        metrics,
        pass_ms_avg: acc.pass_avg(),
        pass_ms_max: acc.pass_max,
    }
}

/// Repeated far teleports: every teleport must settle its whole window
/// within the stated frame bound at the configured budget, and the run
/// reports the priority-inversion and wasted-work numbers that justify (or
/// shelve) the lane executor (§6.2).
#[must_use]
pub fn teleport_storm(cfg: &ScaleConfig) -> ScenarioOutcome {
    let field = PossibilityField::default();
    let mut map = RegionMap::new(cfg.stream);
    let exec = QueueExecutor::default();
    let neutral = [0.0f32; POSSIBILITY_DIMS];
    let mut acc = Accum::default();
    let mut worst_settle = 0u32;
    let mut unsettled = 0u32;

    // Phase 1 — impatient jumps: teleport again long before the window
    // settles, so in-flight/queued work for the abandoned window must be
    // cancelled rather than run to waste (§6.2's supersession counter gate).
    for k in 0..cfg.teleports {
        let pos = (
            -f64::from(k + 1) * 60.0 * REGION_SIZE,
            f64::from(k + 1) * 45.0 * REGION_SIZE,
        );
        for _ in 0..12 {
            let stats = map.update(pos, 0.0, &field, &[], &neutral, &cfg.budget, &exec, false);
            acc.absorb(&stats);
            // Pump only a fraction of the queue: the next jump arrives while
            // jobs are still pending, exactly the storm the tokens exist for.
            exec.run_jobs(cfg.jobs_per_frame / 4);
        }
    }
    // Let the abandoned backlog run dry (cancelled jobs no-op through it).
    while exec.queue_len() > 0 {
        exec.run_jobs(cfg.jobs_per_frame);
        let stats = map.update(
            (
                -f64::from(cfg.teleports) * 60.0 * REGION_SIZE,
                f64::from(cfg.teleports) * 45.0 * REGION_SIZE,
            ),
            0.0,
            &field,
            &[],
            &neutral,
            &cfg.budget,
            &exec,
            false,
        );
        acc.absorb(&stats);
    }
    let phase1_cancelled = acc.cancelled;

    // Phase 2 — settle-gated far teleports (the §11.4 stability gate).
    for k in 0..cfg.teleports {
        // Far, direction-alternating jumps — well beyond unload_radius, so
        // every teleport is a cold window fill with the old window dying.
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        let pos = (
            sign * f64::from(k + 1) * 40.0 * REGION_SIZE,
            -sign * f64::from(k + 1) * 25.0 * REGION_SIZE,
        );
        let mut settle = None;
        for frame in 0..cfg.settle_limit {
            let stats = map.update(pos, 0.0, &field, &[], &neutral, &cfg.budget, &exec, false);
            exec.run_jobs(cfg.jobs_per_frame);
            acc.absorb(&stats);
            if settled(&map, &exec, pos) && stats.loaded == 0 && stats.deferred_loads == 0 {
                settle = Some(frame + 1);
                break;
            }
        }
        match settle {
            Some(f) => worst_settle = worst_settle.max(f),
            None => unsettled += 1,
        }
    }

    let wasted = exec.executed.get().saturating_sub(acc.integrated);
    let inversion =
        exec.critical_blocked_by.get() as f64 / exec.critical_submitted.get().max(1) as f64;

    let gates = vec![
        Gate {
            name: format!(
                "every teleport settles within {} frames at budget",
                cfg.settle_limit
            ),
            passed: unsettled == 0,
            detail: format!("worst settle {worst_settle} frames, {unsettled} unsettled"),
        },
        Gate {
            name: "supersession cancels doomed jobs (counter, not time)".into(),
            passed: phase1_cancelled > 0,
            detail: format!(
                "{phase1_cancelled} jobs cancelled during impatient jumps, \
                 {} superseded results dropped over the run",
                acc.dropped
            ),
        },
    ];

    let metrics = vec![
        ("teleports".into(), f64::from(cfg.teleports)),
        ("worst settle frames".into(), f64::from(worst_settle)),
        ("jobs executed".into(), exec.executed.get() as f64),
        ("results superseded/dropped".into(), wasted as f64),
        (
            "wasted-work fraction".into(),
            wasted as f64 / exec.executed.get().max(1) as f64,
        ),
        (
            "critical jobs submitted".into(),
            exec.critical_submitted.get() as f64,
        ),
        (
            "lower-priority jobs ahead per critical (FIFO)".into(),
            inversion,
        ),
        ("peak queue depth".into(), exec.max_queue.get() as f64),
        ("peak resident regions".into(), acc.peak_regions as f64),
        ("jobs cancelled".into(), acc.cancelled as f64),
        ("results dropped".into(), acc.dropped as f64),
    ];

    ScenarioOutcome {
        name: "teleport-storm",
        frames: acc.frames,
        gates,
        metrics,
        pass_ms_avg: acc.pass_avg(),
        pass_ms_max: acc.pass_max,
    }
}

/// Scale a budget's counting knobs by `factor` (¼× / 1× / 4× in the
/// ADR 0018 gates). Saturating; every knob stays ≥ 1 so progress is always
/// possible.
#[must_use]
pub fn scale_budget(budget: &Budget, factor: f64) -> Budget {
    let scale_usize = |v: usize| -> usize {
        if v == usize::MAX {
            v
        } else {
            ((v as f64 * factor) as usize).max(1)
        }
    };
    Budget {
        max_loads: scale_usize(budget.max_loads),
        max_converge_regions: scale_usize(budget.max_converge_regions),
        max_regen_cost: if budget.max_regen_cost == u32::MAX {
            u32::MAX
        } else {
            ((f64::from(budget.max_regen_cost) * factor) as u32).max(1)
        },
        max_realize_organisms: scale_usize(budget.max_realize_organisms),
        max_persist_ops: scale_usize(budget.max_persist_ops),
        max_route_attraction_nodes: budget.max_route_attraction_nodes,
        max_retarget_regions: budget.max_retarget_regions,
    }
}

/// Update at `pos` (zero travel, neutral steering) until the map reaches a
/// fixed point — nothing stale, nothing in flight, state hash stable across
/// frames — and return the settled hash. Waits out budget-paced loading and
/// realization; sleeps briefly for threaded executors.
fn settle_fixed_point(
    map: &mut RegionMap,
    field: &PossibilityField,
    budget: &Budget,
    executor: &dyn TaskExecutor,
    pos: (f64, f64),
    mut per_frame: impl FnMut(&FrameStats),
) -> u64 {
    settle_fixed_point_with_anchors(map, field, budget, executor, pos, &[], &mut per_frame)
}

fn settle_fixed_point_with_anchors(
    map: &mut RegionMap,
    field: &PossibilityField,
    budget: &Budget,
    executor: &dyn TaskExecutor,
    pos: (f64, f64),
    anchors: &[world_core::Anchor],
    mut per_frame: impl FnMut(&FrameStats),
) -> u64 {
    let neutral = [0.0f32; POSSIBILITY_DIMS];
    const SETTLE_LIMIT: u32 = 20_000;
    let mut last = 0u64;
    let mut stable = 0u32;
    for _ in 0..SETTLE_LIMIT {
        let stats = map.update(pos, 0.0, field, anchors, &neutral, budget, executor, false);
        per_frame(&stats);
        let quiet = map.jobs_in_flight() == 0
            && map.authoritative_realization_complete(pos)
            && map
                .iter_active()
                .all(|r| r.status != GenerationStatus::Generating);
        if !quiet {
            if executor.parallelism() > 1 {
                std::thread::sleep(std::time::Duration::from_micros(50));
            }
            stable = 0;
            continue;
        }
        let hash = state_hash(map);
        if hash == last {
            stable += 1;
            if stable >= 3 {
                break;
            }
        } else {
            stable = 0;
            last = hash;
        }
    }
    last
}

/// Run the schedule-independence script — a bias storm, then a neutral
/// run-out long enough that every storm-era region is evicted, then a stop —
/// and settle the map to a fixed point; returns the settled state hash.
///
/// The run-out matters (ADR 0018's scope): *while traveling*, realized state
/// legitimately depends on pacing — convergence is travel-fueled and
/// resonance-gated, so a slower executor realizes organisms later, which
/// gates convergence differently. What must NOT depend on the schedule is
/// the settled end state of a script that ends quiescent: the terminal
/// window is freshly loaded and generated purely from the possibility state,
/// so executor choice, worker count, budget scale, and cancellation must all
/// produce bit-identical hashes.
#[must_use]
pub fn settled_script_hash(
    cfg: &ScaleConfig,
    executor: &dyn TaskExecutor,
    cancellation: bool,
    budget_factor: f64,
    max_retarget_regions: usize,
) -> u64 {
    let field = PossibilityField::default();
    let mut map = RegionMap::new(cfg.stream);
    map.set_cancellation_enabled(cancellation);
    let budget = Budget {
        max_retarget_regions,
        ..scale_budget(&cfg.budget, budget_factor)
    };
    let speed = f64::hypot(cfg.velocity.0, cfg.velocity.1);
    let storm_frames = cfg.long_haul_frames / 4;
    // Travel two unload radii past the last storm position: everything the
    // storm touched is evicted, and the terminal window is history-free.
    let runout_frames = ((2.5 * cfg.stream.unload_radius / speed).ceil() as u32).max(8);
    let total = storm_frames + runout_frames;
    let neutral = [0.0f32; POSSIBILITY_DIMS];

    for frame in 0..total {
        let player = (
            f64::from(frame) * cfg.velocity.0,
            f64::from(frame) * cfg.velocity.1,
        );
        let bias = if frame < storm_frames {
            storm_bias(frame)
        } else {
            neutral
        };
        map.update(
            player,
            if frame == 0 { 0.0 } else { speed },
            &field,
            &[],
            &bias,
            &budget,
            executor,
            false,
        );
    }

    // Stop and settle to a fixed point: nothing stale, nothing in flight,
    // state hash frame-stable (waits out budget-paced loading/realization).
    let stop = (
        f64::from(total) * cfg.velocity.0,
        f64::from(total) * cfg.velocity.1,
    );
    settle_fixed_point(&mut map, &field, &budget, executor, stop, |_| {})
}

/// The ADR 0018 equality gates: same script + settle ⇒ same state hash
/// across executor choice and worker count, budget scale, and cancellation
/// on/off (phase-6-plan.md §9.3, §11.4).
#[must_use]
pub fn schedule_independence(cfg: &ScaleConfig) -> ScenarioOutcome {
    let reference = settled_script_hash(cfg, &InlineExecutor, true, 1.0, usize::MAX);
    let runs: Vec<(&'static str, u64)> = vec![
        (
            "lane(2)",
            settled_script_hash(cfg, &LaneExecutor::new(2), true, 1.0, usize::MAX),
        ),
        (
            "lane(8)",
            settled_script_hash(cfg, &LaneExecutor::new(8), true, 1.0, usize::MAX),
        ),
        (
            "budget 1/4x",
            settled_script_hash(cfg, &InlineExecutor, true, 0.25, usize::MAX),
        ),
        (
            "budget 4x",
            settled_script_hash(cfg, &InlineExecutor, true, 4.0, usize::MAX),
        ),
        (
            "cancellation off, lane(8)",
            settled_script_hash(cfg, &LaneExecutor::new(8), false, 1.0, usize::MAX),
        ),
        (
            "amortized retarget (16/frame)",
            settled_script_hash(cfg, &InlineExecutor, true, 1.0, 16),
        ),
    ];
    let gates = runs
        .iter()
        .map(|(label, hash)| Gate {
            name: format!("settled state hash equal: inline vs {label}"),
            passed: *hash == reference,
            detail: format!("inline {reference:#018x}, {label} {hash:#018x}"),
        })
        .collect();
    ScenarioOutcome {
        name: "schedule-independence",
        frames: 0,
        gates,
        metrics: vec![("reference hash".into(), reference as f64)],
        pass_ms_avg: [0.0; PASS_COUNT],
        pass_ms_max: [0.0; PASS_COUNT],
    }
}

/// Content hashes of every generated tile of the regions within the near
/// radius of `pos` — the return-trip probe set (always resident, ceiling-
/// exempt).
fn probe_content_hashes(
    map: &RegionMap,
    pos: (f64, f64),
) -> std::collections::BTreeMap<world_core::RegionCoord, Vec<u64>> {
    let mut out = std::collections::BTreeMap::new();
    let near = map.config().near_radius;
    for (&coord, tiles) in map.cache().iter() {
        let (ox, oy) = coord.origin();
        let cx = ox + REGION_SIZE * 0.5 - pos.0;
        let cy = oy + REGION_SIZE * 0.5 - pos.1;
        if f64::hypot(cx, cy) > near {
            continue;
        }
        let mut hashes = Vec::new();
        for tile in tiles.channels.iter().flatten() {
            hashes.push(tile.content_hash());
        }
        if let Some(b) = &tiles.biome {
            hashes.push(b.content_hash());
        }
        if let Some(d) = &tiles.dominant {
            hashes.push(d.content_hash());
        }
        out.insert(coord, hashes);
    }
    out
}

/// The §11.4 memory family: settle under a ceiling roughly half the full
/// window's tiles, take a return trip, and prove (a) capacity parking fired,
/// (b) cache bytes plateau at or under the target every frame,
/// (c) the revisited fixed point is bit-identical (ADR 0008: eviction costs
/// recompute, never correctness), and (d) the pool stays bounded.
#[must_use]
pub fn memory_ceiling(cfg: &ScaleConfig) -> ScenarioOutcome {
    let per_region = full_region_payload_bytes(cfg.stream.field_resolution);
    let radius_regions = (cfg.stream.load_radius / REGION_SIZE).ceil();
    let full_window = (std::f64::consts::PI * radius_regions * radius_regions) as usize;
    let ceiling = (full_window / 2).max(40) * per_region;
    let near_regions = {
        let r = cfg.stream.near_radius / REGION_SIZE + 1.0;
        (std::f64::consts::PI * r * r).ceil() as usize
    };
    let near_exempt_bytes = near_regions * per_region;
    let stream = StreamConfig {
        max_field_cache_bytes: ceiling,
        ..cfg.stream
    };

    // ADR 0023 trajectory gate: with generation inline and unlimited, field
    // capacity may change derived residency but never the ordered regional
    // history. Compare after every scripted frame, not merely at the end.
    let mut roomy_stream = stream;
    roomy_stream.max_field_cache_bytes = usize::MAX;
    let mut trajectory_tight = RegionMap::new(stream);
    let mut trajectory_roomy = RegionMap::new(roomy_stream);
    let trajectory_budget = Budget::unlimited();
    let trajectory_field = PossibilityField::default();
    let trajectory_exec = InlineExecutor;
    let trajectory_speed = f64::hypot(cfg.velocity.0, cfg.velocity.1);
    let trajectory_out_frames = ((2.5 * stream.unload_radius) / trajectory_speed).ceil() as u32;
    let mut authority_divergence = None;
    let mut trajectory_parks = 0usize;
    let mut parked_authority_seen = false;
    let mut material_convergences = 0usize;
    let mut parked_evolution_seen = false;
    let mut previous_authority: BTreeMap<_, _> = BTreeMap::new();
    let mut trajectory_frame = 0u32;
    let mut compare_frame = |position: (f64, f64), travel: f64, bias: [f32; POSSIBILITY_DIMS]| {
        let tight_stats = trajectory_tight.update(
            position,
            travel,
            &trajectory_field,
            &[],
            &bias,
            &trajectory_budget,
            &trajectory_exec,
            false,
        );
        trajectory_roomy.update(
            position,
            travel,
            &trajectory_field,
            &[],
            &bias,
            &trajectory_budget,
            &trajectory_exec,
            false,
        );
        trajectory_parks += tight_stats.evicted_for_capacity;
        material_convergences += tight_stats.converged;
        parked_authority_seen |= trajectory_tight.iter_active().any(|region| {
            region.status == GenerationStatus::Unloaded
                && trajectory_tight.cache().get(region.coord).is_none()
        });
        for region in trajectory_tight
            .iter_active()
            .filter(|region| region.status == GenerationStatus::Unloaded)
        {
            if previous_authority
                .get(&region.coord)
                .is_some_and(|(current, revision)| {
                    *revision != region.revision
                        || *current != region.current.dims.map(f32::to_bits)
                })
            {
                parked_evolution_seen = true;
            }
        }
        previous_authority = trajectory_tight
            .iter_active()
            .map(|region| {
                (
                    region.coord,
                    (region.current.dims.map(f32::to_bits), region.revision),
                )
            })
            .collect();
        let tight_hash = regional_history_hash(&trajectory_tight);
        let roomy_hash = regional_history_hash(&trajectory_roomy);
        if authority_divergence.is_none() && tight_hash != roomy_hash {
            authority_divergence = Some((trajectory_frame, tight_hash, roomy_hash));
        }
        trajectory_frame += 1;
    };
    for _ in 0..4 {
        compare_frame((0.0, 0.0), 0.0, [0.0; POSSIBILITY_DIMS]);
    }
    for frame in 0..(2 * trajectory_out_frames) {
        let t = if frame < trajectory_out_frames {
            f64::from(frame)
        } else {
            f64::from(2 * trajectory_out_frames - frame)
        };
        compare_frame(
            (t * cfg.velocity.0, t * cfg.velocity.1),
            trajectory_speed,
            storm_bias(frame),
        );
    }
    for _ in 0..4 {
        compare_frame((0.0, 0.0), 0.0, storm_bias(2 * trajectory_out_frames));
    }

    let field = PossibilityField::default();
    let mut map = RegionMap::new(stream);
    let exec = InlineExecutor;
    let neutral = [0.0f32; POSSIBILITY_DIMS];
    let mut acc = Accum::default();
    let mut capacity_evictions = 0usize;
    let mut peak_field_bytes = 0usize;
    let mut peak_pool_bytes = 0usize;
    let track = |stats: &FrameStats,
                 acc: &mut Accum,
                 evictions: &mut usize,
                 peak_field: &mut usize,
                 peak_pool: &mut usize| {
        acc.absorb(stats);
        *evictions += stats.evicted_for_capacity;
        *peak_field = (*peak_field).max(stats.cache_bytes);
        *peak_pool = (*peak_pool).max(stats.pool_bytes);
    };

    let origin = (0.0, 0.0);
    let first = settle_fixed_point(&mut map, &field, &cfg.budget, &exec, origin, |s| {
        track(
            s,
            &mut acc,
            &mut capacity_evictions,
            &mut peak_field_bytes,
            &mut peak_pool_bytes,
        );
    });
    let _ = first;
    // Probe: the near window is always field-active, so its content is the
    // return-trip oracle. Derived field admission legitimately differs under
    // a ceiling, while the paired authority hash above proves the regional
    // resident set and history do not (ADRs 0008 and 0023).
    let probe = probe_content_hashes(&map, origin);

    // Walk out past double the unload radius and back, fueling eviction and
    // regeneration along the way.
    let speed = f64::hypot(cfg.velocity.0, cfg.velocity.1);
    let out_frames = ((2.5 * stream.unload_radius) / speed).ceil() as u32;
    for frame in 0..(2 * out_frames) {
        let t = if frame < out_frames {
            f64::from(frame)
        } else {
            f64::from(2 * out_frames - frame)
        };
        let pos = (t * cfg.velocity.0, t * cfg.velocity.1);
        let stats = map.update(pos, speed, &field, &[], &neutral, &cfg.budget, &exec, false);
        track(
            &stats,
            &mut acc,
            &mut capacity_evictions,
            &mut peak_field_bytes,
            &mut peak_pool_bytes,
        );
    }

    let second = settle_fixed_point(&mut map, &field, &cfg.budget, &exec, origin, |s| {
        track(
            s,
            &mut acc,
            &mut capacity_evictions,
            &mut peak_field_bytes,
            &mut peak_pool_bytes,
        );
    });
    let _ = second;
    let probe_after = probe_content_hashes(&map, origin);
    let revisit_identical = !probe.is_empty() && probe == probe_after;

    let gates = vec![
        Gate {
            name: "capacity parking fires under a tight target".into(),
            passed: capacity_evictions > 0 && trajectory_parks > 0,
            detail: format!(
                "{capacity_evictions} scenario parks, {trajectory_parks} paired-trajectory parks"
            ),
        },
        Gate {
            name: "tight and roomy regional history matches after every frame".into(),
            passed: authority_divergence.is_none(),
            detail: authority_divergence.map_or_else(
                || format!("all {trajectory_frame} authority hashes matched"),
                |(frame, tight, roomy)| {
                    format!(
                        "first divergence frame {frame}: tight {tight:#018x}, roomy {roomy:#018x}"
                    )
                },
            ),
        },
        Gate {
            name: "tight ceiling parks fields without removing authority".into(),
            passed: parked_authority_seen,
            detail: if parked_authority_seen {
                "observed authoritative coordinate without field tiles".into()
            } else {
                "no parked authoritative coordinate observed".into()
            },
        },
        Gate {
            name: "parked authoritative history continues to evolve".into(),
            passed: material_convergences > 0 && parked_evolution_seen,
            detail: format!(
                "{material_convergences} convergence events; parked revision/current evolution observed: {parked_evolution_seen}"
            ),
        },
        Gate {
            // The near window is exempt from both parking and the disposable
            // admission target, so the plateau bound is target + near floor.
            name: "field cache plateaus at ceiling + exempt near window".into(),
            passed: peak_field_bytes <= ceiling + near_exempt_bytes,
            detail: format!(
                "peak {peak_field_bytes} B, ceiling {ceiling} B + near exemption {near_exempt_bytes} B"
            ),
        },
        Gate {
            name: "return trip reproduces identical content (near-window probe)".into(),
            passed: revisit_identical,
            detail: format!(
                "{} probe regions, {}",
                probe.len(),
                if revisit_identical {
                    "all content hashes identical"
                } else {
                    "content diverged"
                }
            ),
        },
        Gate {
            name: "pool stays bounded".into(),
            passed: peak_pool_bytes <= 8 * 1024 * 1024,
            detail: format!("peak pool {peak_pool_bytes} B"),
        },
    ];
    let metrics = vec![
        ("ceiling MB".into(), ceiling as f64 / (1024.0 * 1024.0)),
        (
            "peak field cache MB".into(),
            peak_field_bytes as f64 / (1024.0 * 1024.0),
        ),
        ("capacity parks".into(), capacity_evictions as f64),
        (
            "peak pool MB".into(),
            peak_pool_bytes as f64 / (1024.0 * 1024.0),
        ),
    ];
    ScenarioOutcome {
        name: "memory-ceiling",
        frames: acc.frames,
        gates,
        metrics,
        pass_ms_avg: acc.pass_avg(),
        pass_ms_max: acc.pass_max,
    }
}

/// A tier preset scaled to the harness resolution: real radii and budgets,
/// byte ceilings scaled by the resolution ratio so they stay proportionate
/// (the res-32 memory numbers are the local `wer-scale` run's job; CI gates
/// counts and drains at res 8, §12.6).
fn tier_preset(tier: world_runtime::ResourceTier, resolution: u16) -> (StreamConfig, Budget) {
    let mut stream = tier.stream_config();
    let scale = f64::from(resolution) * f64::from(resolution) / (32.0 * 32.0);
    stream.field_resolution = resolution;
    stream.max_field_cache_bytes =
        ((stream.max_field_cache_bytes as f64 * scale) as usize).max(1 << 20);
    (stream, tier.budget())
}

/// Per-tier stability (§11.4): the long-haul storm + stop at the tier's own
/// preset — bounded backpressure, backlog drain, realize-cap respected, and
/// cache plateau under the (scaled) ceiling + exempt near window. Run at
/// High, this is the success-criterion sentence executed: the density
/// targets holding the same stability gates.
#[must_use]
pub fn tier_stability(tier: world_runtime::ResourceTier, cfg: &ScaleConfig) -> ScenarioOutcome {
    let (stream, budget) = tier_preset(tier, cfg.stream.field_resolution);
    let field = PossibilityField::default();
    let mut map = RegionMap::new(stream);
    let exec = QueueExecutor::default();
    let mut acc = Accum::default();
    let speed = f64::hypot(cfg.velocity.0, cfg.velocity.1);
    // Simulated worker throughput scales with the tier's dispatch budget.
    let jobs_per_frame = cfg.jobs_per_frame * (budget.max_regen_cost as usize).div_ceil(96).max(1);

    let mut deferred_series: Vec<usize> = Vec::with_capacity(cfg.long_haul_frames as usize);
    let mut realize_overshoots = 0usize;
    let realize_slack = usize::from(stream.field_resolution)
        * usize::from(stream.field_resolution)
        * usize::from(stream.organisms_per_cell);
    let mut peak_field_bytes = 0usize;
    for frame in 0..cfg.long_haul_frames {
        let player = (
            f64::from(frame) * cfg.velocity.0,
            f64::from(frame) * cfg.velocity.1,
        );
        let stats = map.update(
            player,
            if frame == 0 { 0.0 } else { speed },
            &field,
            &[],
            &storm_bias(frame),
            &budget,
            &exec,
            false,
        );
        exec.run_jobs(jobs_per_frame);
        deferred_series.push(stats.deferred_regens + stats.deferred_loads);
        if stats.organisms_realized > budget.max_realize_organisms + realize_slack {
            realize_overshoots += 1;
        }
        peak_field_bytes = peak_field_bytes.max(stats.cache_bytes);
        acc.absorb(&stats);
    }

    let stop = (
        f64::from(cfg.long_haul_frames) * cfg.velocity.0,
        f64::from(cfg.long_haul_frames) * cfg.velocity.1,
    );
    let neutral = [0.0f32; POSSIBILITY_DIMS];
    let mut drain_frames = None;
    for frame in 0..cfg.drain_limit {
        let stats = map.update(stop, 0.0, &field, &[], &neutral, &budget, &exec, false);
        exec.run_jobs(jobs_per_frame);
        acc.absorb(&stats);
        if stats.deferred_regens == 0 && stats.deferred_loads == 0 && settled(&map, &exec, stop) {
            drain_frames = Some(frame + 1);
            break;
        }
    }

    let tail_start = deferred_series.len() * 3 / 4;
    let mean = deferred_series.iter().sum::<usize>() as f64 / deferred_series.len().max(1) as f64;
    let tail_mean = deferred_series[tail_start..].iter().sum::<usize>() as f64
        / deferred_series[tail_start..].len().max(1) as f64;
    let near_exempt = {
        let r = stream.near_radius / REGION_SIZE + 1.0;
        (std::f64::consts::PI * r * r).ceil() as usize
            * full_region_payload_bytes(stream.field_resolution)
    };

    let name: &'static str = match tier {
        world_runtime::ResourceTier::Low => "tier-low",
        world_runtime::ResourceTier::Mid => "tier-mid",
        world_runtime::ResourceTier::High => "tier-high",
    };
    let gates = vec![
        Gate {
            name: "backpressure bounded (deferred tail vs mean)".into(),
            passed: tail_mean <= mean * 2.0 + 4.0,
            detail: format!("run mean {mean:.1}, last-quarter mean {tail_mean:.1}"),
        },
        Gate {
            name: format!("backlog drains after stop (≤ {} frames)", cfg.drain_limit),
            passed: drain_frames.is_some(),
            detail: match drain_frames {
                Some(f) => format!("drained in {f} frames"),
                None => "still undrained".into(),
            },
        },
        Gate {
            name: "realize budget respected (whole-region overshoot only)".into(),
            passed: realize_overshoots == 0,
            detail: format!("{realize_overshoots} frames exceeded cap + one region"),
        },
        Gate {
            name: "field cache plateaus under ceiling + near exemption".into(),
            passed: peak_field_bytes <= stream.max_field_cache_bytes + near_exempt,
            detail: format!(
                "peak {peak_field_bytes} B vs {} B + {near_exempt} B",
                stream.max_field_cache_bytes
            ),
        },
    ];
    let metrics = vec![
        ("peak resident regions".into(), acc.peak_regions as f64),
        (
            "peak cache MB".into(),
            acc.peak_cache_bytes as f64 / (1024.0 * 1024.0),
        ),
        (
            "organisms resident (final)".into(),
            map.organism_count() as f64,
        ),
        ("jobs cancelled".into(), acc.cancelled as f64),
        (
            "drain frames".into(),
            drain_frames.map_or(f64::NAN, f64::from),
        ),
    ];
    ScenarioOutcome {
        name,
        frames: acc.frames,
        gates,
        metrics,
        pass_ms_avg: acc.pass_avg(),
        pass_ms_max: acc.pass_max,
    }
}

/// Tier identity-invariance (ADRs 0018 and 0024): generated surfaces,
/// canonical organisms, capture/resonance, and actual encoded route records
/// are identical across tiers even though presentation density differs.
#[must_use]
pub fn tier_identity(cfg: &ScaleConfig) -> ScenarioOutcome {
    let field = PossibilityField::default();
    let tiers = [
        world_runtime::ResourceTier::Low,
        world_runtime::ResourceTier::Mid,
        world_runtime::ResourceTier::High,
    ];
    let mut maps = Vec::new();
    for tier in tiers {
        let (stream, budget) = tier_preset(tier, cfg.stream.field_resolution);
        let mut map = RegionMap::new(stream);
        let _ = settle_fixed_point(
            &mut map,
            &field,
            &budget,
            &InlineExecutor,
            (0.0, 0.0),
            |_| {},
        );
        maps.push((tier, budget, map));
    }

    let high = &maps[2].2;
    let (extra_pos, canonical_species) = high
        .organisms()
        .filter(|organism| organism.slot >= 2)
        .find_map(|extra| {
            let coord = RegionCoord::from_world(extra.world_pos.0, extra.world_pos.1);
            let distance2 = |organism: &Organism| {
                let dx = organism.world_pos.0 - extra.world_pos.0;
                let dy = organism.world_pos.1 - extra.world_pos.1;
                dx * dx + dy * dy
            };
            let canonical = high.authoritative_organisms_in(coord).min_by(|a, b| {
                distance2(a)
                    .partial_cmp(&distance2(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
            (canonical.species != extra.species).then_some((extra.world_pos, canonical.species))
        })
        .expect("High tier needs a slot>=2 probe differing from nearest canonical species");
    let player = (REGION_SIZE * 0.5, REGION_SIZE * 0.5);
    let capture_mask = domain_mask(&[
        PossibilityDomain::Morphology,
        PossibilityDomain::Behavior,
        PossibilityDomain::Aesthetics,
        PossibilityDomain::Ecology,
    ]);
    let mut anchor_target = PossibilityVector::neutral();
    anchor_target.set(PossibilityDomain::Aesthetics, 0.8);
    let gameplay_anchors = [Anchor {
        world_pos: player,
        target: anchor_target,
        mask: domain_mask(&[PossibilityDomain::Aesthetics]),
        kind: AnchorKind::Emphasize,
        strength: 0.7,
        falloff_radius: 8.0 * REGION_SIZE,
        source: AnchorSource::Manual,
    }];

    #[derive(Debug)]
    struct Probe {
        name: &'static str,
        hashes: Vec<u64>,
        signature: Vec<u16>,
        canonical: Vec<Organism>,
        resonance: Resonance,
        capture: world_core::Anchor,
        route: RouteRecord,
        route_bytes: Vec<u8>,
        route_resonant: bool,
    }

    let mut probes = Vec::new();
    for (tier, budget, mut map) in maps {
        // Direct resonance reads require the same effective slice that most
        // recently produced authoritative targets (ADR 0026).
        map.update(
            (0.0, 0.0),
            0.0,
            &field,
            &gameplay_anchors,
            &[0.0; POSSIBILITY_DIMS],
            &budget,
            &InlineExecutor,
            false,
        );
        let center = world_core::RegionCoord::new(0, 0);
        let hashes = map
            .layer_diagnostics(center)
            .expect("center resident")
            .iter()
            .filter_map(|diagnostic| diagnostic.stored)
            .collect();
        let signature = world_core::PossibilitySignature::of(
            map.get(center).expect("center authority").current,
        )
        .buckets
        .to_vec();
        let canonical = map.authoritative_organisms().copied().collect();
        let resonance = map.resonance_at(player, &gameplay_anchors);
        let capture = map
            .capture_at(
                extra_pos,
                capture_mask,
                AnchorKind::Emphasize,
                0.8,
                2.0 * REGION_SIZE,
            )
            .expect("settled extra-slot position is capturable");

        let mut recorder = RouteRecorder::new();
        let mut route_resonant = true;
        for index in 0..5_u32 {
            let waypoint = (128.0 + f64::from(index) * 256.0, 128.0);
            let _ = settle_fixed_point_with_anchors(
                &mut map,
                &field,
                &budget,
                &InlineExecutor,
                waypoint,
                &gameplay_anchors,
                |_| {},
            );
            let stats = map.update(
                waypoint,
                0.0,
                &field,
                &gameplay_anchors,
                &[0.0; POSSIBILITY_DIMS],
                &budget,
                &InlineExecutor,
                false,
            );
            route_resonant &= stats.resonance_nodes > 0;
            recorder.observe(
                &map,
                waypoint,
                if index == 0 { 0.0 } else { 256.0 },
                &gameplay_anchors,
                stats.resonance_strength,
            );
        }
        let (nodes, discoveries) = recorder.finish();
        let mut vault = Vault::open(MemoryStorage::new()).expect("memory vault");
        let route_id = vault
            .record_route(nodes, discoveries, "tier expedition".into())
            .expect("fresh vault sequence");
        let route = vault.routes()[&route_id].clone();
        let route_bytes = encode_record(RecordKind::Route, &route);
        probes.push(Probe {
            name: tier.name(),
            hashes,
            signature,
            canonical,
            resonance,
            capture,
            route,
            route_bytes,
            route_resonant,
        });
    }

    let reference = &probes[0];
    let mut gates = Vec::new();
    for probe in &probes {
        gates.push(Gate {
            name: format!("generated surfaces tier-invariant: {}", probe.name),
            passed: !probe.hashes.is_empty()
                && probe.hashes == reference.hashes
                && probe.signature == reference.signature,
            detail: format!(
                "{} layer hashes, first {:#018x}",
                probe.hashes.len(),
                probe.hashes.first().copied().unwrap_or(0)
            ),
        });
        gates.push(Gate {
            name: format!("canonical organisms tier-invariant: {}", probe.name),
            passed: !probe.canonical.is_empty() && probe.canonical == reference.canonical,
            detail: format!("{} ordered slot-0 organisms", probe.canonical.len()),
        });
        gates.push(Gate {
            name: format!("capture and resonance tier-invariant: {}", probe.name),
            passed: probe.capture == reference.capture
                && matches!(
                    probe.capture.source,
                    AnchorSource::Organism { species } if species == canonical_species
                )
                && probe.resonance.strength.to_bits() == reference.resonance.strength.to_bits()
                && probe.resonance.anchor_compatibility.to_bits()
                    == reference.resonance.anchor_compatibility.to_bits()
                && probe.resonance.nodes == reference.resonance.nodes,
            detail: format!(
                "capture {:?}, resonance {:08x}/{} nodes",
                probe.capture.source,
                probe.resonance.strength.to_bits(),
                probe.resonance.nodes.len()
            ),
        });
        gates.push(Gate {
            name: format!("encoded route record tier-invariant: {}", probe.name),
            passed: probe.route.nodes == reference.route.nodes
                && probe.route.id == reference.route.id
                && probe.route_bytes == reference.route_bytes
                && probe.route.nodes.len() >= 2
                && probe.route_resonant,
            detail: format!(
                "{} nodes, id {:#018x}, {} bytes",
                probe.route.nodes.len(),
                probe.route.id,
                probe.route_bytes.len()
            ),
        });
    }
    ScenarioOutcome {
        name: "tier-identity",
        frames: 0,
        gates,
        metrics: Vec::new(),
        pass_ms_avg: [0.0; PASS_COUNT],
        pass_ms_max: [0.0; PASS_COUNT],
    }
}

/// The density lever's coherence gates (§6.6, §11.4): at
/// `organisms_per_cell = 4`, realized population scales ≈ 4× linearly, the
/// density-1 identities survive as slot 0 (additivity — no existing identity
/// changes), and ids stay unique.
#[must_use]
pub fn density_realization(cfg: &ScaleConfig) -> ScenarioOutcome {
    let field = PossibilityField::default();
    let base_stream = StreamConfig {
        near_radius: 2.0 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 5.0 * REGION_SIZE,
        unload_radius: 6.0 * REGION_SIZE,
        field_resolution: cfg.stream.field_resolution,
        ..StreamConfig::default()
    };
    let settle = |organisms_per_cell: u16| {
        let mut map = RegionMap::new(StreamConfig {
            organisms_per_cell,
            ..base_stream
        });
        let _ = settle_fixed_point(
            &mut map,
            &field,
            &Budget::unlimited(),
            &InlineExecutor,
            (0.0, 0.0),
            |_| {},
        );
        map
    };
    let map1 = settle(1);
    let map4 = settle(4);
    let count1 = map1.organism_count();
    let count4 = map4.organism_count();
    let ratio = count4 as f64 / count1.max(1) as f64;

    let ids1: std::collections::BTreeSet<u64> = map1.organisms().map(|o| o.id).collect();
    let ids4: std::collections::BTreeSet<u64> = map4.organisms().map(|o| o.id).collect();
    let additive = ids1.is_subset(&ids4);
    let unique = ids4.len() == count4;
    let canonical_exact = map1.organisms().copied().collect::<Vec<_>>()
        == map4.authoritative_organisms().copied().collect::<Vec<_>>();
    let slots_labeled = map1.organisms().all(|organism| organism.slot == 0)
        && map4.organisms().all(|organism| organism.slot < 4);

    let gates = vec![
        Gate {
            name: "density 4 scales population ≈ linearly".into(),
            passed: count1 > 0 && (3.2..=4.8).contains(&ratio),
            detail: format!("{count1} organisms at 1/cell, {count4} at 4/cell (×{ratio:.2})"),
        },
        Gate {
            name: "density-1 identities survive as slot 0 (additive)".into(),
            passed: additive && canonical_exact && slots_labeled,
            detail: format!(
                "{} of {} density-1 ids present; exact canonical={canonical_exact}, labeled={slots_labeled}",
                ids1.intersection(&ids4).count(),
                ids1.len()
            ),
        },
        Gate {
            name: "organism ids unique at density 4".into(),
            passed: unique,
            detail: format!("{} unique of {count4}", ids4.len()),
        },
    ];
    ScenarioOutcome {
        name: "density-4-realization",
        frames: 0,
        gates,
        metrics: vec![("density ratio".into(), ratio)],
        pass_ms_avg: [0.0; PASS_COUNT],
        pass_ms_max: [0.0; PASS_COUNT],
    }
}

/// Run every scenario family (§11.4): schedule independence, stability per
/// tier, memory, and density.
#[must_use]
pub fn run_scale_harness(cfg: &ScaleConfig) -> ScaleReport {
    ScaleReport {
        scenarios: vec![
            long_haul(cfg),
            teleport_storm(cfg),
            schedule_independence(cfg),
            memory_ceiling(cfg),
            tier_identity(cfg),
            density_realization(cfg),
            tier_stability(world_runtime::ResourceTier::Low, cfg),
            tier_stability(world_runtime::ResourceTier::Mid, cfg),
            tier_stability(world_runtime::ResourceTier::High, cfg),
        ],
    }
}

/// Print the report table `docs/perf-baseline.md` snapshots (`--report`).
pub fn print_report(report: &ScaleReport) {
    for scenario in &report.scenarios {
        println!(
            "## scenario: {} ({} frames)",
            scenario.name, scenario.frames
        );
        println!("| pass | avg ms | max ms |");
        println!("|---|---|---|");
        for pass in Pass::ALL {
            let i = pass.index();
            println!(
                "| {} | {:.3} | {:.3} |",
                pass.name(),
                scenario.pass_ms_avg[i],
                scenario.pass_ms_max[i]
            );
        }
        println!("| metric | value |");
        println!("|---|---|");
        for (label, value) in &scenario.metrics {
            println!("| {label} | {value:.3} |");
        }
        for gate in &scenario.gates {
            println!(
                "- gate [{}] {} — {}",
                if gate.passed { "PASS" } else { "FAIL" },
                gate.name,
                gate.detail
            );
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_harness_passes() {
        let report = run_scale_harness(&ScaleConfig::quick());
        for scenario in &report.scenarios {
            for gate in &scenario.gates {
                assert!(
                    gate.passed,
                    "{}: {} — {}",
                    scenario.name, gate.name, gate.detail
                );
            }
        }
    }
}
