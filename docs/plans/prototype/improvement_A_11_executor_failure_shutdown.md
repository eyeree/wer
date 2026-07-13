# Improvement A.11 — Executor failure and bounded shutdown

**Status:** Completed

**Roadmap item:** [Correctness and contract integrity A.11](../../world-model.md#prioritized-improvement-roadmap)

**Finding addressed:** [30](../../world-model.md#30-native-executor-shutdown-drains-work-it-says-it-discards)

This plan implements roadmap item A.11 in
[`docs/world-model.md`](../../world-model.md): **Make executor failure and
shutdown bounded**. The current native `LaneExecutor` documents that queued jobs
which have not started are discarded on drop, but workers pop queued jobs before
checking `shutdown`, so dropping the executor drains the backlog. Separately, a
panic inside a generation closure tears down the worker thread without sending a
failure result to `RegionMap`, leaving the corresponding in-flight dispatch
bookkeeping live forever unless some later invalidation happens to retire it.

The correction is a runtime/scheduler robustness change. It does not alter
world generation math, dependency hashes, persistence formats, record ids,
route attraction, rendering output, or cache key semantics.

Do not modify [`implementation-plan.md`](implementation-plan.md),
`docs/plans/prototype/phase-N-plan.md`, or any `docs/plans/phase-N-plan.md`
file. Those are historical phase records. Update only current-model
documentation in `docs/world-model.md` after implementation, and mark roadmap
A.11 completed there once code, docs, and tests pass.

---

## 1. Required outcome and invariants

The implementation must satisfy all of the following.

1. Dropping `LaneExecutor` bounds shutdown by the number of jobs already running
   on worker threads. Jobs still queued in the three priority lanes are cleared
   or made unreachable before workers can drain them.
2. Worker threads check shutdown before selecting more queued work. A worker
   woken during shutdown exits instead of starting another job.
3. `LaneExecutor::submit` does not accept new work after shutdown begins. It
   must drop the submitted closure without running it, and it must not panic.
   This can only happen through unusual ownership/race patterns, but the
   behavior should be explicit and tested.
4. Joining workers in `Drop` still completes already-running jobs. The executor
   cannot forcibly kill a Rust closure in progress, so the bounded contract is
   "running jobs may finish; queued jobs are discarded."
5. A panic inside a generation job is converted into a structured failed job
   result visible to the main-thread integrator. It must not leave
   `RegionMap::in_flight` permanently occupied.
6. Failed current dispatches retire their in-flight entry, reclaim any owned
   dispatch resources the runtime can reclaim, mark the failed layer and
   declared dependents dirty, and cancel obsolete dependent jobs before normal
   budgeted dispatch retries them.
7. Failed obsolete dispatches are counted and ignored the same way superseded
   results are: they must not dirty current state or retire a newer dispatch
   with the same `(coord, layer)` key.
8. Worker panic handling must not require the neutral `TaskExecutor` trait to
   become native-only. `world-core` and `world-runtime` still must not spawn
   threads, touch filesystem APIs, or depend on platform crates.
9. Lost native workers are replaced, or their loss is made deterministic and
   visible. Prefer replacement in `LaneExecutor`, because a permanently reduced
   worker pool can strand throughput and invalidate the executor's
   `parallelism()` claim.
10. Replacement must avoid unbounded worker growth. At steady state the
    executor owns exactly its configured worker count unless it is shutting
    down.
11. Queue and failure counters are telemetry only. They must not enter
    dependency hashes, persisted state, record encodings, atlas bundles, or
    world identity.
12. `WORLD_ALGORITHM_VERSION` remains 2, and no layer `algorithm_revision`
    changes. This item changes scheduling/failure recovery, not generated
    output for successful jobs.
13. Existing continuity and scale determinism guarantees remain true across
    `InlineExecutor`, `LaneExecutor`, cancellation on/off, budget slicing,
    resource tiers, and worker counts.
14. Finding 30's fairness, bounded-queue, and cancellation-aware queue-removal
    notes are improvement opportunities unless implemented completely here.
    This item must at least close the correctness half: bounded shutdown,
    structured failure, in-flight repair, and worker replacement/requeue.
15. `docs/world-model.md` must be updated after implementation so section 3.24
    describes bounded shutdown/failure behavior, finding 30 records the
    resolved correctness behavior and any remaining fairness/backpressure
    limitations, and roadmap item A.11 is marked completed.

## 2. Scope boundaries

### 2.1 In scope

- `crates/tools/src/executor.rs` shutdown order, queue clearing, submit-after-
  shutdown behavior, panic supervision, worker replacement, and focused tests.
- `crates/world-runtime/src/stream.rs` job result enum, generation closure
  wrappers, failure integration, in-flight retirement, dirty-closure repair,
  and counters.
- Native shell telemetry labels only if a new `FrameStats` counter should be
  displayed.
- Current-model documentation updates in `docs/world-model.md` after the code
  lands.
- Tests that prove shutdown does not drain queued work, worker panics do not
  reduce the pool, and generation panic/failure does not strand `RegionMap`.

### 2.2 Explicitly out of scope

- Editing historical phase plans or `implementation-plan.md`.
- Changing layer math, generator output, dependency hashing, golden world
  fixtures, record codecs, or atlas formats.
- Making `TaskExecutor` a rich result-returning API or adding a platform-native
  dependency to `world-runtime`.
- Solving all scheduling QoS concerns in finding 30, including weighted
  fairness/aging, hard bounded queues, or proactive removal of cancelled
  closures from queues, unless those changes are independently designed and
  tested.
- Forcibly aborting already-running Rust jobs during executor drop. The safe
  bounded behavior is to stop queued work and join running workers.
- Browser/Web Worker portability work from finding 31.

## 3. Current failure map

| Path | Current behavior | Required correction |
|---|---|---|
| `tools/src/executor.rs::worker_loop` | Pops the next job before checking `lanes.shutdown`, so workers drain backlog during drop. | Check `shutdown` first, or clear queues under the lock before waking workers; ideally do both for a simple bounded contract. |
| `tools/src/executor.rs::Drop` | Sets `shutdown` and notifies, but leaves all queued closures available. | Set `shutdown`, clear all three queues, then notify and join. Dropped boxed closures release captured senders, tiles, and buffers. |
| `tools/src/executor.rs::submit` | Always pushes a job, even if shutdown has started. | If `shutdown` is set, drop the job without enqueueing and return. |
| `tools/src/executor.rs` worker panic path | A panic ends the thread; `Drop` ignores the failed join and `parallelism()` still reports the original handle count until drop. | Catch job panics inside workers, report/continue or supervise and replace workers. Keep configured live worker count stable until shutdown. |
| `world-runtime/src/stream.rs` job closures | Macro and tile jobs send success results only. If generation panics before send, `in_flight` keeps the job forever. | Wrap generation in `catch_unwind(AssertUnwindSafe(...))` and send a structured failed result with key and job id. |
| `world-runtime/src/stream.rs::integrate_finished` | Handles `JobResult::Macro` and `JobResult::Tile` success paths only. | Add failure variants and route them through the same dispatch-id checks as success, retiring only the current matching dispatch. |
| `RegionMap::repair_missing_dependencies` | Can repair obsolete pending hashes, but cannot see a lost current job. | Failed-result integration must make lost current jobs retryable immediately. |
| Tests | Existing executor tests wait for all jobs before drop, avoiding shutdown discard. No panic/replacement tests exist. | Add tests that intentionally drop with queued work and panic a worker/job. |

## 4. Executor design

### 4.1 Bounded shutdown

Change `LaneExecutor::Drop` to perform all shutdown state changes under the
queue mutex:

1. lock `shared.lanes`;
2. set `lanes.shutdown = true`;
3. clear `lanes.queues[0]`, `[1]`, and `[2]`;
4. drop the lock;
5. `notify_all`;
6. join workers.

Then change `worker_loop` so shutdown wins over queued work:

```text
loop:
  lock lanes
  while !shutdown and all queues empty: wait
  if shutdown: return
  pop Critical > Normal > Background
  drop lock
  run job
```

With this shape, clearing queues makes the documented discard behavior
observable, while the shutdown-first worker check prevents future regressions
if a submit races after shutdown or if clearing is changed later.

`submit` should lock, check `shutdown`, and return early when the executor is
closing. Only notify a worker when a job was actually enqueued.

### 4.2 Panic supervision and replacement

Prefer catching panics inside `worker_loop` around each job:

```text
let outcome = catch_unwind(AssertUnwindSafe(job));
if outcome.is_err(): record a worker/job panic counter;
continue;
```

This keeps the OS thread alive, so no replacement path is needed for ordinary
job panics and the configured worker count remains exact. It also avoids a
supervisor thread or a self-referential handle registry.

If the implementation instead lets worker threads terminate, add explicit
supervision:

- store configured worker count separately from `workers.len()`;
- detect panicked handles at a bounded maintenance point, such as `submit` or
  a new private `reap_and_replace_finished_workers`;
- spawn replacements only when `shutdown == false`;
- remove joined handles so `Drop` cannot repeatedly join them;
- keep names monotonic, e.g. `wer-worker-{worker_id}` with a separate next-id.

The catch-and-continue design is simpler and should be used unless it conflicts
with panic reporting requirements. A panic still needs to be represented as a
failed generation job at the runtime layer, because executor-level catch alone
does not know which `(coord, layer, job_id)` was lost.

### 4.3 Executor observability

Add the smallest useful introspection for tests and telemetry:

- `queued_per_lane()` and `queued()` should keep returning zero after shutdown
  queues are cleared.
- If a panic counter is added to `LaneExecutor`, expose it as
  `worker_panics()` for tests/panel diagnostics. This counter is native
  telemetry only.
- `parallelism()` should remain the configured worker count while the executor
  is alive. It must not silently fall after a worker panic.

Do not add this telemetry to deterministic world hashes or persistence.

## 5. Runtime generation failure design

### 5.1 Add structured failure results

Extend the private `JobResult` enum in `crates/world-runtime/src/stream.rs`
with failure variants that carry enough identity to apply the same stale-result
checks used by success results:

```text
JobResult::TileFailed {
    coord: RegionCoord,
    layer: u16,
    job_id: u64,
}

JobResult::MacroFailed {
    coord: RegionCoord,
    job_id: u64,
}
```

Avoid storing panic payload text in `JobResult`. Panic payload formatting is
not needed for deterministic recovery and can accidentally pull nonportable
details into logs. A count in `FrameStats` is enough.

Add `FrameStats::jobs_failed` or a more specific
`generation_jobs_failed: usize` counter. It should count current matching
dispatches that were retired because a job failed. Obsolete failures can
increment `results_dropped`, matching stale success behavior, or a separate
`failed_results_dropped` if diagnostics need the distinction.

### 5.2 Wrap every runtime generation closure

In both `check_macro` and `submit_layer`, keep the existing cancellation check
as the first operation inside the submitted closure:

```text
if cancel.load(Ordering::Relaxed) {
    return;
}
```

After that, wrap only the generation body in
`std::panic::catch_unwind(std::panic::AssertUnwindSafe(...))`. On success,
send the existing `Macro` or `Tile` result. On panic, send the matching
failure result with the same `coord`, `layer` where applicable, and `job_id`.

This preserves cancellation as worker-time optimization: cancelled jobs still
do not send failure results. Their in-flight entries are already retired by
the main thread when cancellation is requested.

If the result receiver is gone because `RegionMap` was dropped, ignore send
failure exactly as the current success path does.

### 5.3 Integrate failures through dispatch identity

Add failure arms to `integrate_finished`:

- For `MacroFailed`, key by `(coord, LAYER_DRAINAGE)`.
- For `TileFailed`, key by `(coord, layer)`.
- Look up `in_flight` and compare `job_id`.
- If no current entry exists or the ids differ, count the result as dropped and
  leave current state untouched.
- If the ids match, remove the in-flight entry, increment the failed counter,
  and mark retryable dirty state.

The retry behavior should mirror existing provenance rejection:

- macro failure calls `mark_macro_dependents_dirty(coord, stats)` after
  removing the in-flight macro entry;
- tile failure calls `mark_dirty_closure(coord, layer, stats)` after removing
  the in-flight tile entry.

Do not publish partial output. The panic path should produce no `GeneratedTile`
or `DrainageTile`; there is nothing to reclaim beyond dispatch-owned inputs
that drop with the unwound closure.

Be careful with `mark_dirty_closure`: it calls `cancel_in_flight` for the
dependent closure. Remove the failed job from `in_flight` before calling it so
the failed key is not double-counted as cancelled.

### 5.4 Optional helper to reduce duplication

If the failure and rejection paths become repetitive, add small private helpers:

```text
fn reject_failed_macro_job(&mut self, coord, job_id, stats)
fn reject_failed_tile_job(&mut self, coord, layer, job_id, stats)
```

These helpers should perform the current-id check internally and return whether
they retired a current job. Keep them private to `stream.rs`; the public
runtime API does not need a new failure surface.

## 6. Tests

### 6.1 Executor unit tests

Add tests in `crates/tools/src/executor.rs`.

1. `drop_discards_queued_jobs_without_draining_backlog`
   - create `LaneExecutor::new(1)`;
   - submit a first job that blocks on a condition variable;
   - submit many queued jobs that increment a counter or send messages;
   - wait until the first job is running;
   - drop the executor from another thread or release the first job and drop
     promptly, depending on borrow constraints;
   - assert queued jobs did not run. The robust version owns the executor in a
     helper thread so the test can block the sole worker, enqueue the backlog,
     trigger drop, then release the running job and join the helper.
2. `submit_after_shutdown_drops_job`
   - if testing this directly is awkward because `shutdown` is private, cover
     it through the drop test or a small `#[cfg(test)]` helper that sets the
     flag under lock;
   - assert the closure is not run and no panic occurs.
3. `worker_panic_does_not_reduce_parallelism_or_stop_future_jobs`
   - submit a job that panics;
   - submit a later job that sends on a channel;
   - assert the later job runs and `parallelism()` still reports the configured
     worker count.
4. `queued_counts_clear_on_drop` if a test helper can observe shutdown state
   before destruction; otherwise rely on the discard test.

Avoid tests that depend on sleep timing alone. Use channels, mutexes, and
condition variables to establish worker state.

### 6.2 Runtime failure tests

Add focused tests in `crates/world-runtime/src/stream.rs` or
`crates/world-runtime/tests/streaming.rs`.

1. `tile_job_panic_retires_in_flight_and_redirties_layer`
   - use a test-only executor that catches the submitted closure boundary or a
     test-only generation hook to force a panic after dispatch identity is
     recorded;
   - dispatch one current tile job;
   - run the job so it sends `TileFailed`;
   - call `integrate_finished` through `update` or a private test helper;
   - assert `jobs_in_flight() == 0`, the layer/dependent closure is dirty, and
     `stats.jobs_failed == 1`.
2. `macro_job_panic_retires_macro_and_redirties_dependents`
   - force a Drainage macro job to fail;
   - assert the macro in-flight key is gone, covered resident dependents are
     dirty/generating as appropriate, and no old macro cache entry was
     overwritten.
3. `obsolete_failed_job_does_not_retire_newer_dispatch`
   - dispatch a job, retire/supersede it, dispatch a newer job for the same
     key, then deliver the older failure;
   - assert the newer `in_flight` entry remains and the failure is counted as
     dropped, not as a current failure.
4. `failed_dispatch_retries_to_settle`
   - after one injected failure, run subsequent frames with `InlineExecutor` or
     a manual executor and assert the region reaches the same fixed point as a
     no-failure baseline.

Because the production generation functions normally do not panic on demand,
prefer one of these testability hooks:

- a `#[cfg(test)]` private method that directly enqueues `JobResult::TileFailed`
  or `JobResult::MacroFailed` after a real dispatch, preserving current-id
  checks; or
- a `#[cfg(test)]` fault-injection field on `RegionMap` that makes the next
  submitted layer closure panic.

The direct-result helper is less invasive and keeps production code free of
fault-injection branches. It should still exercise the real failure integration
logic.

### 6.3 Existing regression suite

Run at least:

```sh
cargo test -p tools executor
cargo test -p world-runtime streaming
cargo test --workspace
```

Before considering the item complete, run the repository CI-equivalent checks:

```sh
cargo fmt --all -- --check
RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets
cargo check --workspace
cargo test --workspace
cargo check -p world-core -p world-runtime -p platform-web --target wasm32-unknown-unknown
wasm-pack test --node crates/platform-web
```

The wasm checks matter because any attempted panic/failure abstraction in
`world-runtime` must remain platform-neutral.

## 7. Documentation updates after implementation

Update `docs/world-model.md` only after code and tests are in place.

1. In section 3.24, replace the current description that queued jobs are
   discarded on drop with the precise bounded contract:
   - shutdown sets a flag and clears queued lanes under the mutex;
   - workers exit before taking more queued work once shutdown is set;
   - already-running jobs are joined;
   - job panics become failed dispatch results and retry through normal dirty
     scheduling.
2. In the prioritized roadmap, change item A.11 to
   `**Completed: Make executor failure and shutdown bounded**`, link this plan,
   and summarize the landed behavior in one paragraph.
3. In finding 30, move the corrected shutdown/panic/in-flight text into past
   tense, and leave any unimplemented fairness, bounded-queue, or cancelled-
   queue-removal limitations explicit as remaining performance/backpressure
   work rather than correctness blockers.
4. Do not edit `implementation-plan.md` or any phase plan.

## 8. Rollout order

1. Add runtime failure result variants and integration helpers, with tests that
   inject failed results directly.
2. Wrap `check_macro` and `submit_layer` generation closures in `catch_unwind`
   and send failure variants on panic.
3. Add `FrameStats` failure telemetry and any native panel/replay/scale summary
   wiring needed for compilation.
4. Fix `LaneExecutor` shutdown order, queue clearing, submit-after-shutdown,
   and job panic catch/continue.
5. Add executor unit tests for bounded drop and panic survival.
6. Run focused tests, then CI-equivalent checks.
7. Update `docs/world-model.md` to mark A.11 completed and record any remaining
   noncorrectness scheduling limitations.

This order makes the runtime able to consume structured failures before the
executor starts preserving worker threads after panics. It also keeps the
shutdown fix independent, so regressions are easy to localize.

## 9. Risks and review notes

- `catch_unwind` requires `AssertUnwindSafe` around boxed `FnOnce` jobs and
  around generation closures. Keep the caught boundary narrow and document that
  the purpose is dispatch recovery, not making arbitrary shared state
  exception-safe.
- The executor-level panic catch keeps a worker alive, but it cannot synthesize
  a `RegionMap` failure result because it does not know the dispatch key. The
  runtime closure wrapper is therefore required even if the executor also
  catches panics.
- A failure result must not retire a newer dispatch. Always check `job_id`
  before removing `in_flight`.
- A failure result for an obsolete job should not dirty current state. Treat it
  like a stale success result.
- Clearing queued closures during drop releases captured `Arc` tile snapshots,
  buffers, and senders without running their cancellation checks. That is the
  intended bounded behavior; tests should verify no queued side effects occur.
- Do not add native synchronization primitives or thread APIs to
  `world-runtime`. `catch_unwind`, channels, `Arc`, atomics, and bookkeeping are
  still neutral; thread creation stays in `tools`.
- Panic telemetry may change frame stats during injected-failure tests only.
  Normal successful runs, goldens, and settled state hashes should remain
  unchanged.
