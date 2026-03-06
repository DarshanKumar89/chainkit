//! Indexer metrics and observability.
//!
//! Tracks block processing rate, event throughput, reorg frequency,
//! handler latency, and block lag for monitoring indexer health.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Core indexer metrics, thread-safe for concurrent access.
pub struct IndexerMetrics {
    // Block processing
    blocks_processed: AtomicU64,
    blocks_per_second: Mutex<f64>,
    last_processed_block: AtomicU64,
    chain_head_block: AtomicU64,

    // Event processing
    events_processed: AtomicU64,
    events_per_second: Mutex<f64>,

    // Reorg tracking
    reorgs_detected: AtomicU64,
    total_reorg_depth: AtomicU64,
    max_reorg_depth: AtomicU64,

    // Handler performance
    handler_calls: AtomicU64,
    handler_total_latency_us: AtomicU64,
    handler_max_latency_us: AtomicU64,

    // RPC calls
    rpc_calls: AtomicU64,
    rpc_errors: AtomicU64,
    rpc_total_latency_us: AtomicU64,

    // Checkpoint
    last_checkpoint_block: AtomicU64,
    checkpoints_saved: AtomicU64,

    // Timestamps (unix seconds)
    started_at: AtomicU64,
    last_block_at: AtomicU64,
}

impl IndexerMetrics {
    pub fn new() -> Self {
        Self {
            blocks_processed: AtomicU64::new(0),
            blocks_per_second: Mutex::new(0.0),
            last_processed_block: AtomicU64::new(0),
            chain_head_block: AtomicU64::new(0),
            events_processed: AtomicU64::new(0),
            events_per_second: Mutex::new(0.0),
            reorgs_detected: AtomicU64::new(0),
            total_reorg_depth: AtomicU64::new(0),
            max_reorg_depth: AtomicU64::new(0),
            handler_calls: AtomicU64::new(0),
            handler_total_latency_us: AtomicU64::new(0),
            handler_max_latency_us: AtomicU64::new(0),
            rpc_calls: AtomicU64::new(0),
            rpc_errors: AtomicU64::new(0),
            rpc_total_latency_us: AtomicU64::new(0),
            last_checkpoint_block: AtomicU64::new(0),
            checkpoints_saved: AtomicU64::new(0),
            started_at: AtomicU64::new(0),
            last_block_at: AtomicU64::new(0),
        }
    }

    pub fn set_started(&self, timestamp: u64) {
        self.started_at.store(timestamp, Ordering::Relaxed);
    }

    pub fn record_block(&self, block_number: u64, event_count: u64, timestamp: u64) {
        self.blocks_processed.fetch_add(1, Ordering::Relaxed);
        self.last_processed_block
            .store(block_number, Ordering::Relaxed);
        self.events_processed
            .fetch_add(event_count, Ordering::Relaxed);
        self.last_block_at.store(timestamp, Ordering::Relaxed);
    }

    pub fn record_reorg(&self, depth: u64) {
        self.reorgs_detected.fetch_add(1, Ordering::Relaxed);
        self.total_reorg_depth.fetch_add(depth, Ordering::Relaxed);
        self.max_reorg_depth.fetch_max(depth, Ordering::Relaxed);
    }

    pub fn record_handler_call(&self, latency: Duration) {
        let us = latency.as_micros() as u64;
        self.handler_calls.fetch_add(1, Ordering::Relaxed);
        self.handler_total_latency_us
            .fetch_add(us, Ordering::Relaxed);
        self.handler_max_latency_us
            .fetch_max(us, Ordering::Relaxed);
    }

    pub fn record_rpc_call(&self, latency: Duration, success: bool) {
        let us = latency.as_micros() as u64;
        self.rpc_calls.fetch_add(1, Ordering::Relaxed);
        self.rpc_total_latency_us.fetch_add(us, Ordering::Relaxed);
        if !success {
            self.rpc_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_checkpoint(&self, block_number: u64) {
        self.last_checkpoint_block
            .store(block_number, Ordering::Relaxed);
        self.checkpoints_saved.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_chain_head(&self, block_number: u64) {
        self.chain_head_block
            .store(block_number, Ordering::Relaxed);
    }

    /// Block lag = chain_head - last_processed
    pub fn block_lag(&self) -> u64 {
        let head = self.chain_head_block.load(Ordering::Relaxed);
        let processed = self.last_processed_block.load(Ordering::Relaxed);
        head.saturating_sub(processed)
    }

    /// Average handler latency.
    pub fn avg_handler_latency(&self) -> Duration {
        let calls = self.handler_calls.load(Ordering::Relaxed);
        if calls == 0 {
            return Duration::ZERO;
        }
        let total = self.handler_total_latency_us.load(Ordering::Relaxed);
        Duration::from_micros(total / calls)
    }

    /// Average reorg depth.
    pub fn avg_reorg_depth(&self) -> f64 {
        let count = self.reorgs_detected.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.total_reorg_depth.load(Ordering::Relaxed) as f64 / count as f64
    }

    /// RPC success rate (0.0 - 1.0).
    pub fn rpc_success_rate(&self) -> f64 {
        let total = self.rpc_calls.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        let errors = self.rpc_errors.load(Ordering::Relaxed);
        (total - errors) as f64 / total as f64
    }

    /// Create an immutable snapshot for reporting.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let handler_calls = self.handler_calls.load(Ordering::Relaxed);
        let handler_total = self.handler_total_latency_us.load(Ordering::Relaxed);
        let rpc_calls = self.rpc_calls.load(Ordering::Relaxed);
        let rpc_errors = self.rpc_errors.load(Ordering::Relaxed);
        let rpc_total = self.rpc_total_latency_us.load(Ordering::Relaxed);
        let reorgs = self.reorgs_detected.load(Ordering::Relaxed);
        let total_depth = self.total_reorg_depth.load(Ordering::Relaxed);

        MetricsSnapshot {
            blocks_processed: self.blocks_processed.load(Ordering::Relaxed),
            last_processed_block: self.last_processed_block.load(Ordering::Relaxed),
            chain_head_block: self.chain_head_block.load(Ordering::Relaxed),
            block_lag: self.block_lag(),
            events_processed: self.events_processed.load(Ordering::Relaxed),
            reorgs_detected: reorgs,
            avg_reorg_depth: if reorgs > 0 {
                total_depth as f64 / reorgs as f64
            } else {
                0.0
            },
            max_reorg_depth: self.max_reorg_depth.load(Ordering::Relaxed),
            handler_calls,
            avg_handler_latency_us: if handler_calls > 0 {
                handler_total / handler_calls
            } else {
                0
            },
            max_handler_latency_us: self.handler_max_latency_us.load(Ordering::Relaxed),
            rpc_calls,
            rpc_errors,
            avg_rpc_latency_us: if rpc_calls > 0 {
                rpc_total / rpc_calls
            } else {
                0
            },
            rpc_success_rate: if rpc_calls > 0 {
                (rpc_calls - rpc_errors) as f64 / rpc_calls as f64
            } else {
                1.0
            },
            checkpoints_saved: self.checkpoints_saved.load(Ordering::Relaxed),
            last_checkpoint_block: self.last_checkpoint_block.load(Ordering::Relaxed),
        }
    }
}

impl Default for IndexerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable snapshot of metrics for reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSnapshot {
    pub blocks_processed: u64,
    pub last_processed_block: u64,
    pub chain_head_block: u64,
    pub block_lag: u64,
    pub events_processed: u64,
    pub reorgs_detected: u64,
    pub avg_reorg_depth: f64,
    pub max_reorg_depth: u64,
    pub handler_calls: u64,
    pub avg_handler_latency_us: u64,
    pub max_handler_latency_us: u64,
    pub rpc_calls: u64,
    pub rpc_errors: u64,
    pub avg_rpc_latency_us: u64,
    pub rpc_success_rate: f64,
    pub checkpoints_saved: u64,
    pub last_checkpoint_block: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_block_updates_counters() {
        let m = IndexerMetrics::new();
        m.record_block(100, 5, 1000);
        m.record_block(101, 3, 1012);
        m.record_block(102, 10, 1024);

        let snap = m.snapshot();
        assert_eq!(snap.blocks_processed, 3);
        assert_eq!(snap.last_processed_block, 102);
        assert_eq!(snap.events_processed, 18); // 5 + 3 + 10
    }

    #[test]
    fn block_lag() {
        let m = IndexerMetrics::new();
        m.set_chain_head(1000);
        m.record_block(990, 0, 1000);
        assert_eq!(m.block_lag(), 10);

        // When processed catches up
        m.record_block(1000, 0, 1120);
        assert_eq!(m.block_lag(), 0);

        // When processed is ahead of stored head (should not go negative)
        m.record_block(1005, 0, 1180);
        assert_eq!(m.block_lag(), 0); // saturating_sub
    }

    #[test]
    fn reorg_metrics() {
        let m = IndexerMetrics::new();
        m.record_reorg(2);
        m.record_reorg(4);
        m.record_reorg(6);

        let snap = m.snapshot();
        assert_eq!(snap.reorgs_detected, 3);
        assert_eq!(snap.max_reorg_depth, 6);
        assert!((m.avg_reorg_depth() - 4.0).abs() < f64::EPSILON); // (2+4+6)/3 = 4.0
    }

    #[test]
    fn handler_latency() {
        let m = IndexerMetrics::new();
        m.record_handler_call(Duration::from_micros(100));
        m.record_handler_call(Duration::from_micros(200));
        m.record_handler_call(Duration::from_micros(300));

        // Average = (100+200+300)/3 = 200us
        assert_eq!(m.avg_handler_latency(), Duration::from_micros(200));

        let snap = m.snapshot();
        assert_eq!(snap.handler_calls, 3);
        assert_eq!(snap.avg_handler_latency_us, 200);
        assert_eq!(snap.max_handler_latency_us, 300);
    }

    #[test]
    fn rpc_success_rate() {
        let m = IndexerMetrics::new();

        // No calls yet — default is 1.0
        assert!((m.rpc_success_rate() - 1.0).abs() < f64::EPSILON);

        // 8 successes, 2 failures = 80% success
        for _ in 0..8 {
            m.record_rpc_call(Duration::from_micros(50), true);
        }
        for _ in 0..2 {
            m.record_rpc_call(Duration::from_micros(50), false);
        }

        assert!((m.rpc_success_rate() - 0.8).abs() < f64::EPSILON);

        let snap = m.snapshot();
        assert_eq!(snap.rpc_calls, 10);
        assert_eq!(snap.rpc_errors, 2);
    }

    #[test]
    fn snapshot_serialization() {
        let m = IndexerMetrics::new();
        m.set_started(1000);
        m.record_block(100, 5, 1012);
        m.set_chain_head(110);
        m.record_reorg(3);
        m.record_handler_call(Duration::from_micros(150));
        m.record_rpc_call(Duration::from_micros(200), true);
        m.record_checkpoint(100);

        let snap = m.snapshot();
        let json = serde_json::to_string(&snap).expect("snapshot must serialize to JSON");
        assert!(json.contains("\"blocks_processed\":1"));
        assert!(json.contains("\"block_lag\":10"));
        assert!(json.contains("\"reorgs_detected\":1"));
        assert!(json.contains("\"checkpoints_saved\":1"));
    }
}
