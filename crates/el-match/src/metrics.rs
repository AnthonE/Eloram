//! Shard metrics: relaxed atomics written by the shard thread, read by `/status`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Durations in log2(µs) buckets.
const BUCKETS: usize = 32;

/// A histogram of durations in log2(µs) buckets, written by the shard thread.
#[derive(Default)]
pub struct Hist {
    buckets: [AtomicU64; BUCKETS],
}

impl Hist {
    pub fn record(&self, us: u64) {
        let b = (64 - us.max(1).leading_zeros() as usize - 1).min(BUCKETS - 1);
        self.buckets[b].fetch_add(1, Ordering::Relaxed);
    }

    /// Upper bound of the bucket holding quantile `q`, µs (0 before anything is recorded).
    pub fn quantile_us(&self, q: f64) -> u64 {
        let counts = self.buckets.each_ref().map(|c| c.load(Ordering::Relaxed));
        let total: u64 = counts.iter().sum();
        if total == 0 {
            return 0;
        }
        let target = (total as f64 * q).ceil() as u64;
        let mut acc = 0;
        for (b, c) in counts.iter().enumerate() {
            acc += c;
            if acc >= target {
                return 1u64 << (b + 1);
            }
        }
        1u64 << BUCKETS
    }
}

/// What a shard thread reports.
#[derive(Default)]
pub struct Metrics {
    pub ticks: AtomicU64,
    pub overruns: AtomicU64,
    /// The shard's whole tick, every match on it.
    pub tick_hist: Hist,
    /// A match's tick in its halves: the simulation (inputs applied, the step) and replication
    /// (every seat's snapshot built and queued).
    pub sim_hist: Hist,
    pub replicate_hist: Hist,
    pub tick_last_us: AtomicU64,
    pub tick_max_us: AtomicU64,
    pub matches: AtomicU64,
    pub seats: AtomicU64,
    pub snapshots: AtomicU64,
    pub snapshot_bytes: AtomicU64,
    pub snapshot_max_bytes: AtomicU64,
    pub records: AtomicU64,
    pub record_bits: AtomicU64,
    pub deltas: AtomicU64,
    pub out_drops: AtomicU64,
    /// Record frames that didn't fit their ring (the recorder fell behind): each breaks a record.
    pub record_drops: AtomicU64,
    pub record_bytes: AtomicU64,
    /// Checkpoints written, and those that didn't fit the ring (the recorder fell behind).
    pub checkpoints: AtomicU64,
    pub checkpoint_drops: AtomicU64,
    /// Matches stopped because they panicked.
    pub failed: AtomicU64,
    pub inputs: AtomicU64,
    pub inputs_stale: AtomicU64,
    pub inputs_missing: AtomicU64,
    pub orders: AtomicU64,
}

impl Metrics {
    #[inline]
    pub fn add(c: &AtomicU64, n: u64) {
        c.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn set(c: &AtomicU64, v: u64) {
        c.store(v, Ordering::Relaxed);
    }

    #[inline]
    pub fn max(c: &AtomicU64, v: u64) {
        c.fetch_max(v, Ordering::Relaxed);
    }

    pub fn load(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }

    pub fn record_tick(&self, us: u64) {
        self.tick_hist.record(us);
        Self::add(&self.ticks, 1);
        Self::set(&self.tick_last_us, us);
        Self::max(&self.tick_max_us, us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_land_in_their_buckets() {
        let h = Hist::default();
        assert_eq!(h.quantile_us(0.5), 0);
        for us in [3u64, 3, 3, 3, 100] {
            h.record(us);
        }
        assert_eq!(h.quantile_us(0.5), 4);
        assert_eq!(h.quantile_us(1.0), 128);
    }
}
