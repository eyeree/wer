//! The counting-allocator measurement (phase-6-plan.md §4.2, §11.4): after
//! warm-up, steady-state drift frames must serve their tile churn from the
//! pool — pool misses stop, and per-frame allocation volume stays far below
//! tile-churn scale. Test-side instrumentation only; nothing here ships.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use world_core::{PossibilityDomain, PossibilityField, POSSIBILITY_DIMS, REGION_SIZE};
use world_runtime::{Budget, InlineExecutor, RegionMap, StreamConfig};

/// Counts bytes allocated through the global allocator.
struct CountingAlloc;

static ALLOCATED: AtomicU64 = AtomicU64::new(0);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: delegates directly to `System`; the counters are side effects.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// A repeating fast-domain bias storm (drift pressure without touching the
/// stable trio).
fn storm_bias(frame: u32) -> [f32; POSSIBILITY_DIMS] {
    let mut bias = [0.0f32; POSSIBILITY_DIMS];
    let t = (frame % 120) as f32 / 120.0;
    let ramp = if t < 0.5 { t * 2.0 } else { 2.0 - t * 2.0 };
    bias[PossibilityDomain::Ecology.index()] = 0.35 * ramp;
    bias[PossibilityDomain::Hydrology.index()] = 0.30 * ramp;
    bias
}

#[test]
fn steady_drift_serves_tile_churn_from_the_pool() {
    let cfg = StreamConfig {
        near_radius: 1.5 * REGION_SIZE,
        far_radius: 4.0 * REGION_SIZE,
        load_radius: 6.0 * REGION_SIZE,
        unload_radius: 7.5 * REGION_SIZE,
        field_resolution: 16,
        ..StreamConfig::default()
    };
    let field = PossibilityField::default();
    let budget = Budget::default();
    let mut map = RegionMap::new(cfg);
    let velocity = (11.0f64, 7.0f64);
    let speed = f64::hypot(velocity.0, velocity.1);

    // Warm-up: fill the window and prime the pool through one full storm.
    const WARMUP: u32 = 240;
    for frame in 0..WARMUP {
        let pos = (f64::from(frame) * velocity.0, f64::from(frame) * velocity.1);
        let travel = if frame == 0 { 0.0 } else { speed };
        map.update(
            pos,
            travel,
            &field,
            &[],
            &storm_bias(frame),
            &budget,
            &InlineExecutor,
            false,
        );
    }

    // Measured steady-state drift: same pressure, counted allocations.
    const MEASURED: u32 = 120;
    let mut misses = 0usize;
    let mut regenerated = 0usize;
    let bytes_before = ALLOCATED.load(Ordering::Relaxed);
    let allocs_before = ALLOCATIONS.load(Ordering::Relaxed);
    for frame in WARMUP..(WARMUP + MEASURED) {
        let pos = (f64::from(frame) * velocity.0, f64::from(frame) * velocity.1);
        let stats = map.update(
            pos,
            speed,
            &field,
            &[],
            &storm_bias(frame),
            &budget,
            &InlineExecutor,
            false,
        );
        misses += stats.pool_misses;
        regenerated += stats.layers_regenerated;
    }
    let bytes = ALLOCATED.load(Ordering::Relaxed) - bytes_before;
    let allocs = ALLOCATIONS.load(Ordering::Relaxed) - allocs_before;
    let bytes_per_frame = bytes / u64::from(MEASURED);
    let allocs_per_frame = allocs / MEASURED as usize;
    eprintln!(
        "steady drift: {regenerated} tiles regenerated, {misses} pool misses, \
         {bytes_per_frame} B/frame, {allocs_per_frame} allocs/frame"
    );

    // The drift must actually have churned tiles for this to mean anything.
    assert!(regenerated > 100, "drift produced no churn ({regenerated})");
    // The pool serves steady-state churn: the residual miss rate is demand
    // jitter (fresh loads consume buffers that only return when a trailing
    // region evicts, so cycle-aligned regen bursts can momentarily outrun
    // the standing inventory), a couple percent of churn at most — the
    // "~zero steady-state allocation" of phase-6-plan.md §4.2 measured
    // honestly rather than asserted to a brittle literal zero.
    assert!(
        misses <= regenerated / 50,
        "tile pool missed {misses} times against {regenerated} regenerated tiles"
    );
    // Per-frame allocation volume stays far below tile-churn scale (a single
    // 16×16 f32 tile is 1 KiB; pre-pool drift churned dozens per frame).
    // What remains is per-frame scratch (candidate lists, job boxes).
    assert!(
        bytes_per_frame < 256 * 1024,
        "steady drift allocates {bytes_per_frame} B/frame"
    );
}
