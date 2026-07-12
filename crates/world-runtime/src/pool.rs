//! `TilePool`: recycles tile sample buffers through the
//! dispatch→generate→integrate→evict cycle (phase-6-plan.md §4.2), so
//! steady-state generation stops touching the global allocator.
//!
//! Main-thread only by construction: the single-writer `RegionMap` is the
//! only pool toucher. Buffers are handed *into* job closures at dispatch
//! (workers just fill what they were given) and reclaimed when a superseded
//! or evicted tile's `Arc` refcount proves sole ownership — an in-flight
//! reader (viz sampling, a realize snapshot) merely delays reclaim, and the
//! pool falls back to allocation rather than ever blocking. Buffers that die
//! on a worker (a cancelled job dropping its closure) simply return to the
//! allocator; the pool is an optimization, never an owner of record.
//!
//! Allocation-only and wasm-clean: nothing here touches a platform API, so
//! the browser runtime inherits it unchanged.

/// How many buffers of each type the pool holds before returning further
/// reclaims to the allocator — it exists to serve churn, not to hoard
/// (phase-6-plan.md §4.2). At `FIELD_RES = 32` the f32 cap is 4 MB.
const MAX_F32_BUFS: usize = 1024;
const MAX_U8_BUFS: usize = 256;
const MAX_U16_BUFS: usize = 256;

/// Recycles tile sample buffers. See the module docs.
#[derive(Debug, Default)]
pub struct TilePool {
    f32_bufs: Vec<Vec<f32>>,
    u8_bufs: Vec<Vec<u8>>,
    u16_bufs: Vec<Vec<u16>>,
    hits: usize,
    misses: usize,
}

impl TilePool {
    /// Take an `f32` buffer (empty; capacity from a previous tile when one
    /// is pooled, fresh otherwise).
    #[must_use]
    pub fn take_f32(&mut self) -> Vec<f32> {
        match self.f32_bufs.pop() {
            Some(buf) => {
                self.hits += 1;
                buf
            }
            None => {
                self.misses += 1;
                Vec::new()
            }
        }
    }

    /// Take a `u8` buffer.
    #[must_use]
    pub fn take_u8(&mut self) -> Vec<u8> {
        match self.u8_bufs.pop() {
            Some(buf) => {
                self.hits += 1;
                buf
            }
            None => {
                self.misses += 1;
                Vec::new()
            }
        }
    }

    /// Take a `u16` buffer.
    #[must_use]
    pub fn take_u16(&mut self) -> Vec<u16> {
        match self.u16_bufs.pop() {
            Some(buf) => {
                self.hits += 1;
                buf
            }
            None => {
                self.misses += 1;
                Vec::new()
            }
        }
    }

    /// Return an `f32` buffer to the pool (dropped if the pool is at
    /// capacity).
    pub fn reclaim_f32(&mut self, buf: Vec<f32>) {
        if self.f32_bufs.len() < MAX_F32_BUFS {
            self.f32_bufs.push(buf);
        }
    }

    /// Return a `u8` buffer to the pool.
    pub fn reclaim_u8(&mut self, buf: Vec<u8>) {
        if self.u8_bufs.len() < MAX_U8_BUFS {
            self.u8_bufs.push(buf);
        }
    }

    /// Return a `u16` buffer to the pool.
    pub fn reclaim_u16(&mut self, buf: Vec<u16>) {
        if self.u16_bufs.len() < MAX_U16_BUFS {
            self.u16_bufs.push(buf);
        }
    }

    /// Heap bytes idling in the pool (`FrameStats::pool_bytes`).
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.f32_bufs
            .iter()
            .map(|b| b.capacity() * core::mem::size_of::<f32>())
            .sum::<usize>()
            + self.u8_bufs.iter().map(Vec::capacity).sum::<usize>()
            + self
                .u16_bufs
                .iter()
                .map(|b| b.capacity() * core::mem::size_of::<u16>())
                .sum::<usize>()
    }

    /// Drain and reset the hit/miss counters (per-frame telemetry).
    pub fn take_stats(&mut self) -> (usize, usize) {
        let out = (self.hits, self.misses);
        self.hits = 0;
        self.misses = 0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_reclaim_round_trip_reuses_capacity() {
        let mut pool = TilePool::default();
        let miss = pool.take_f32();
        assert_eq!(miss.capacity(), 0);
        let mut buf = Vec::with_capacity(1024);
        buf.push(1.0f32);
        pool.reclaim_f32(buf);
        assert!(pool.bytes() >= 1024 * 4);
        let hit = pool.take_f32();
        assert!(hit.capacity() >= 1024);
        assert_eq!(pool.take_stats(), (1, 1));
    }

    #[test]
    fn pool_is_capacity_capped() {
        let mut pool = TilePool::default();
        for _ in 0..(MAX_F32_BUFS + 100) {
            pool.reclaim_f32(Vec::with_capacity(16));
        }
        assert!(pool.bytes() <= MAX_F32_BUFS * 16 * 4);
    }
}
