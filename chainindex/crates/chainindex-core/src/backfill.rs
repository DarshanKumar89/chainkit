//! Parallel backfill engine for the chainindex pipeline.
//!
//! Divides a historical block range into fixed-size segments and processes
//! them concurrently, bounded by a [`tokio::sync::Semaphore`].  Each segment
//! fetches events in smaller batches via a [`BlockDataProvider`] and retries
//! on failure with exponential back-off.
//!
//! # Example
//!
//! ```rust,ignore
//! let config = BackfillConfig {
//!     from_block: 0,
//!     to_block: 1_000_000,
//!     concurrency: 8,
//!     ..BackfillConfig::default()
//! };
//! let engine = BackfillEngine::new(config, provider, filter, "ethereum".into());
//! let result = engine.run().await?;
//! println!("indexed {} events", result.total_events);
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, error, info, warn};

use crate::error::IndexerError;
use crate::handler::DecodedEvent;
use crate::types::EventFilter;

// ─── BackfillConfig ───────────────────────────────────────────────────────────

/// Configuration for the parallel backfill engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillConfig {
    /// First block to index (inclusive).
    pub from_block: u64,
    /// Last block to index (inclusive).
    pub to_block: u64,
    /// Maximum number of segments processed in parallel.
    pub concurrency: usize,
    /// Number of blocks in each parallel work unit (segment).
    pub segment_size: u64,
    /// Number of blocks fetched in a single RPC call within a segment.
    pub batch_size: u64,
    /// How many times to retry a failed segment before giving up.
    pub retry_attempts: u32,
    /// Base delay between retries; doubles on each attempt (exponential back-off).
    pub retry_delay: Duration,
}

impl Default for BackfillConfig {
    fn default() -> Self {
        Self {
            from_block: 0,
            to_block: 0,
            concurrency: 4,
            segment_size: 10_000,
            batch_size: 500,
            retry_attempts: 3,
            retry_delay: Duration::from_secs(1),
        }
    }
}

impl BackfillConfig {
    /// Validate that the config is internally consistent.
    pub fn validate(&self) -> Result<(), IndexerError> {
        if self.from_block > self.to_block {
            return Err(IndexerError::Other(format!(
                "backfill config invalid: from_block ({}) > to_block ({})",
                self.from_block, self.to_block
            )));
        }
        if self.concurrency == 0 {
            return Err(IndexerError::Other(
                "backfill config invalid: concurrency must be >= 1".into(),
            ));
        }
        if self.segment_size == 0 {
            return Err(IndexerError::Other(
                "backfill config invalid: segment_size must be >= 1".into(),
            ));
        }
        if self.batch_size == 0 {
            return Err(IndexerError::Other(
                "backfill config invalid: batch_size must be >= 1".into(),
            ));
        }
        Ok(())
    }

    /// Divide the configured block range into ordered [`BackfillSegment`]s.
    pub fn segments(&self) -> Vec<BackfillSegment> {
        if self.from_block > self.to_block {
            return vec![];
        }
        let mut segments = Vec::new();
        let mut current = self.from_block;
        let mut id = 0usize;

        while current <= self.to_block {
            let end = (current + self.segment_size - 1).min(self.to_block);
            segments.push(BackfillSegment {
                id,
                from_block: current,
                to_block: end,
                status: SegmentStatus::Pending,
                events_processed: 0,
                duration: None,
                error: None,
            });
            id += 1;
            // Avoid overflow when to_block == u64::MAX
            match end.checked_add(1) {
                Some(next) => current = next,
                None => break,
            }
        }
        segments
    }
}

// ─── SegmentStatus ────────────────────────────────────────────────────────────

/// Processing state of a single backfill segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentStatus {
    /// Not yet started.
    Pending,
    /// Currently being processed by a worker.
    InProgress,
    /// All batches within the segment completed successfully.
    Complete,
    /// All retry attempts exhausted; segment could not be processed.
    Failed,
}

// ─── BackfillSegment ──────────────────────────────────────────────────────────

/// A single work unit covering a contiguous block sub-range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillSegment {
    /// Zero-based index within the ordered segment list.
    pub id: usize,
    /// First block (inclusive).
    pub from_block: u64,
    /// Last block (inclusive).
    pub to_block: u64,
    /// Current processing status.
    pub status: SegmentStatus,
    /// Number of events collected from this segment.
    pub events_processed: u64,
    /// Wall-clock time taken to process the segment (set on completion).
    pub duration: Option<Duration>,
    /// Human-readable error message if the segment failed permanently.
    pub error: Option<String>,
}

impl BackfillSegment {
    /// Total blocks covered by this segment.
    pub fn block_count(&self) -> u64 {
        self.to_block - self.from_block + 1
    }
}

// ─── BackfillProgress ─────────────────────────────────────────────────────────

/// A point-in-time snapshot of backfill progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillProgress {
    /// Total number of segments in the job.
    pub total_segments: usize,
    /// Segments that have finished successfully.
    pub completed_segments: usize,
    /// Segments that exhausted all retries.
    pub failed_segments: usize,
    /// Total blocks in the range (including any failed segments).
    pub total_blocks: u64,
    /// Blocks whose events have been successfully collected.
    pub processed_blocks: u64,
    /// Total events seen so far.
    pub total_events: u64,
    /// Elapsed time since the backfill started.
    pub elapsed: Duration,
}

impl BackfillProgress {
    /// Throughput in blocks per second over the elapsed window.
    ///
    /// Returns `0.0` if elapsed is zero.
    pub fn blocks_per_second(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs == 0.0 {
            return 0.0;
        }
        self.processed_blocks as f64 / secs
    }

    /// Estimated time remaining based on current throughput.
    ///
    /// Returns [`Duration::ZERO`] if already complete or throughput is zero.
    pub fn eta(&self) -> Duration {
        let remaining = self.total_blocks.saturating_sub(self.processed_blocks);
        if remaining == 0 {
            return Duration::ZERO;
        }
        let bps = self.blocks_per_second();
        if bps == 0.0 {
            return Duration::MAX;
        }
        Duration::from_secs_f64(remaining as f64 / bps)
    }

    /// Fraction of blocks processed, expressed as a percentage in `[0.0, 100.0]`.
    pub fn percent_complete(&self) -> f64 {
        if self.total_blocks == 0 {
            return 100.0;
        }
        (self.processed_blocks as f64 / self.total_blocks as f64) * 100.0
    }
}

// ─── BackfillResult ───────────────────────────────────────────────────────────

/// Final summary produced by [`BackfillEngine::run`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillResult {
    /// All segments, in order, with their final statuses.
    pub segments: Vec<BackfillSegment>,
    /// Total events collected across all successful segments.
    pub total_events: u64,
    /// Wall-clock time for the entire backfill job.
    pub total_duration: Duration,
    /// IDs of segments that permanently failed (exhausted retries).
    pub failed_segments: Vec<usize>,
}

// ─── BlockDataProvider ────────────────────────────────────────────────────────

/// Abstraction over an RPC provider for backfill purposes.
///
/// Implementations should wrap an actual JSON-RPC client (e.g. via
/// `chainrpc`) and translate network errors into [`IndexerError::Rpc`].
#[async_trait]
pub trait BlockDataProvider: Send + Sync {
    /// Fetch decoded events within `[from, to]` matching `filter`.
    async fn get_events(
        &self,
        from: u64,
        to: u64,
        filter: &EventFilter,
    ) -> Result<Vec<DecodedEvent>, IndexerError>;

    /// Fetch a single block summary by number.  Returns `None` if the block
    /// does not exist on the node (e.g. beyond the chain head).
    async fn get_block(
        &self,
        number: u64,
    ) -> Result<Option<crate::types::BlockSummary>, IndexerError>;
}

// ─── Internal shared state ────────────────────────────────────────────────────

/// Mutable state shared between worker tasks via `Arc<Mutex<>>`.
struct EngineState {
    segments: Vec<BackfillSegment>,
    /// Per-segment event buffers; index matches segment id.
    events: Vec<Vec<DecodedEvent>>,
    processed_blocks: u64,
    total_events: u64,
    start_time: Instant,
}

impl EngineState {
    fn new(segments: Vec<BackfillSegment>) -> Self {
        let n = segments.len();
        Self {
            segments,
            events: vec![vec![]; n],
            processed_blocks: 0,
            total_events: 0,
            start_time: Instant::now(),
        }
    }

    fn progress(&self, total_blocks: u64) -> BackfillProgress {
        let completed = self
            .segments
            .iter()
            .filter(|s| s.status == SegmentStatus::Complete)
            .count();
        let failed = self
            .segments
            .iter()
            .filter(|s| s.status == SegmentStatus::Failed)
            .count();

        BackfillProgress {
            total_segments: self.segments.len(),
            completed_segments: completed,
            failed_segments: failed,
            total_blocks,
            processed_blocks: self.processed_blocks,
            total_events: self.total_events,
            elapsed: self.start_time.elapsed(),
        }
    }
}

// ─── BackfillEngine ───────────────────────────────────────────────────────────

/// Parallel backfill engine.
///
/// Construct with [`BackfillEngine::new`] and execute with [`BackfillEngine::run`].
pub struct BackfillEngine {
    config: BackfillConfig,
    provider: Arc<dyn BlockDataProvider>,
    filter: EventFilter,
    chain: String,
    state: Arc<Mutex<EngineState>>,
    total_blocks: u64,
}

impl BackfillEngine {
    /// Create a new engine.  The `provider` is shared across all worker tasks.
    pub fn new(
        config: BackfillConfig,
        provider: Arc<dyn BlockDataProvider>,
        filter: EventFilter,
        chain: impl Into<String>,
    ) -> Self {
        let segments = config.segments();
        let total_blocks = if config.from_block <= config.to_block {
            config.to_block - config.from_block + 1
        } else {
            0
        };
        let state = Arc::new(Mutex::new(EngineState::new(segments)));
        Self {
            config,
            provider,
            filter,
            chain: chain.into(),
            state,
            total_blocks,
        }
    }

    /// Return a snapshot of the current progress.
    ///
    /// Safe to call from any task while [`run`] is executing.
    pub async fn progress(&self) -> BackfillProgress {
        self.state.lock().await.progress(self.total_blocks)
    }

    /// Execute the backfill.
    ///
    /// Spawns up to `config.concurrency` concurrent worker tasks, each
    /// processing one segment at a time.  Blocks until all segments have
    /// either completed or exhausted their retry budget.
    pub async fn run(&self) -> Result<BackfillResult, IndexerError> {
        self.config.validate()?;

        let segment_count = {
            let guard = self.state.lock().await;
            guard.segments.len()
        };

        if segment_count == 0 {
            info!(chain = %self.chain, "backfill: empty range, nothing to do");
            return Ok(BackfillResult {
                segments: vec![],
                total_events: 0,
                total_duration: Duration::ZERO,
                failed_segments: vec![],
            });
        }

        info!(
            chain = %self.chain,
            from = self.config.from_block,
            to = self.config.to_block,
            segments = segment_count,
            concurrency = self.config.concurrency,
            "backfill: starting"
        );

        let start = Instant::now();
        let semaphore = Arc::new(Semaphore::new(self.config.concurrency));

        // Spawn one task per segment; the semaphore bounds parallelism.
        let mut handles = Vec::with_capacity(segment_count);

        for seg_id in 0..segment_count {
            let sem = semaphore.clone();
            let state = self.state.clone();
            let provider = self.provider.clone();
            let filter = self.filter.clone();
            let config = self.config.clone();
            let chain = self.chain.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");

                // Mark segment as in-progress.
                {
                    let mut guard = state.lock().await;
                    guard.segments[seg_id].status = SegmentStatus::InProgress;
                }

                let (from_block, to_block) = {
                    let guard = state.lock().await;
                    (
                        guard.segments[seg_id].from_block,
                        guard.segments[seg_id].to_block,
                    )
                };

                let seg_start = Instant::now();
                let mut last_error: Option<String> = None;
                let mut succeeded = false;
                let mut collected: Vec<DecodedEvent> = vec![];

                for attempt in 0..=config.retry_attempts {
                    if attempt > 0 {
                        // Exponential back-off: base * 2^(attempt-1)
                        let backoff = config.retry_delay * 2u32.pow(attempt - 1);
                        warn!(
                            chain = %chain,
                            seg = seg_id,
                            attempt,
                            backoff_ms = backoff.as_millis(),
                            "backfill: retrying segment"
                        );
                        tokio::time::sleep(backoff).await;
                    }

                    match process_segment(
                        seg_id,
                        from_block,
                        to_block,
                        &config,
                        provider.as_ref(),
                        &filter,
                        &chain,
                    )
                    .await
                    {
                        Ok(events) => {
                            collected = events;
                            succeeded = true;
                            break;
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            error!(
                                chain = %chain,
                                seg = seg_id,
                                attempt,
                                error = %msg,
                                "backfill: segment attempt failed"
                            );
                            last_error = Some(msg);
                        }
                    }
                }

                let elapsed = seg_start.elapsed();
                let event_count = collected.len() as u64;
                let block_count = to_block - from_block + 1;

                // Update shared state.
                {
                    let mut guard = state.lock().await;
                    if succeeded {
                        guard.segments[seg_id].status = SegmentStatus::Complete;
                        guard.segments[seg_id].events_processed = event_count;
                        guard.segments[seg_id].duration = Some(elapsed);
                        guard.events[seg_id] = collected;
                        guard.processed_blocks += block_count;
                        guard.total_events += event_count;
                        debug!(
                            chain = %chain,
                            seg = seg_id,
                            events = event_count,
                            blocks = block_count,
                            elapsed_ms = elapsed.as_millis(),
                            "backfill: segment complete"
                        );
                    } else {
                        guard.segments[seg_id].status = SegmentStatus::Failed;
                        guard.segments[seg_id].error = last_error;
                        guard.segments[seg_id].duration = Some(elapsed);
                    }
                }
            });

            handles.push(handle);
        }

        join_all(handles).await;

        let total_duration = start.elapsed();
        let guard = self.state.lock().await;

        let total_events = guard.total_events;
        let failed_segments: Vec<usize> = guard
            .segments
            .iter()
            .filter(|s| s.status == SegmentStatus::Failed)
            .map(|s| s.id)
            .collect();

        info!(
            chain = %self.chain,
            total_events,
            failed = failed_segments.len(),
            elapsed_ms = total_duration.as_millis(),
            "backfill: complete"
        );

        Ok(BackfillResult {
            segments: guard.segments.clone(),
            total_events,
            total_duration,
            failed_segments,
        })
    }
}

/// Process a single segment: fetch events in `batch_size` sub-ranges and
/// aggregate the results.
async fn process_segment(
    seg_id: usize,
    from_block: u64,
    to_block: u64,
    config: &BackfillConfig,
    provider: &dyn BlockDataProvider,
    filter: &EventFilter,
    chain: &str,
) -> Result<Vec<DecodedEvent>, IndexerError> {
    let mut all_events = Vec::new();
    let mut batch_from = from_block;

    while batch_from <= to_block {
        let batch_to = (batch_from + config.batch_size - 1).min(to_block);

        debug!(
            chain = %chain,
            seg = seg_id,
            batch_from,
            batch_to,
            "backfill: fetching batch"
        );

        let events = provider.get_events(batch_from, batch_to, filter).await?;
        all_events.extend(events);

        match batch_to.checked_add(1) {
            Some(next) => batch_from = next,
            None => break,
        }
    }

    Ok(all_events)
}

// ─── SegmentMerger ────────────────────────────────────────────────────────────

/// Merges results from multiple completed segments into a single, ordered
/// event stream.
///
/// Events are ordered by `(block_number, log_index)` to ensure a stable,
/// deterministic output regardless of how tasks were scheduled.
pub struct SegmentMerger;

impl SegmentMerger {
    /// Merge the per-segment event buffers into one ordered `Vec<DecodedEvent>`.
    ///
    /// Only segments with [`SegmentStatus::Complete`] contribute events.
    /// `segments` and `events` must be parallel slices of the same length.
    pub fn merge(segments: &[BackfillSegment], events: &[Vec<DecodedEvent>]) -> Vec<DecodedEvent> {
        assert_eq!(
            segments.len(),
            events.len(),
            "segments and events slices must have equal length"
        );

        // Collect events from successful segments only, in segment order.
        let mut merged: Vec<DecodedEvent> = segments
            .iter()
            .zip(events.iter())
            .filter(|(seg, _)| seg.status == SegmentStatus::Complete)
            .flat_map(|(_, evts)| evts.iter().cloned())
            .collect();

        // Final sort guarantees strict block/log ordering regardless of
        // any within-segment ordering that the provider may return.
        merged.sort_by_key(|e| (e.block_number, e.log_index));
        merged
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::time::Duration;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_event(block_number: u64, log_index: u32) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "TestEvent".into(),
            address: "0xdeadbeef".into(),
            tx_hash: format!("0x{block_number:064x}"),
            block_number,
            log_index,
            fields_json: serde_json::Value::Null,
        }
    }

    fn make_segment(id: usize, from: u64, to: u64, status: SegmentStatus) -> BackfillSegment {
        BackfillSegment {
            id,
            from_block: from,
            to_block: to,
            status,
            events_processed: 0,
            duration: None,
            error: None,
        }
    }

    // ── MockProvider ──────────────────────────────────────────────────────────

    /// A mock provider that returns one event per block in the requested range.
    struct MockProvider {
        /// Number of times `get_events` was called.
        call_count: Arc<AtomicU32>,
        /// If set, the provider fails this many times before succeeding.
        fail_times: Arc<AtomicU32>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                call_count: Arc::new(AtomicU32::new(0)),
                fail_times: Arc::new(AtomicU32::new(0)),
            }
        }

        fn with_failures(n: u32) -> Self {
            let p = Self::new();
            p.fail_times.store(n, Ordering::SeqCst);
            p
        }
    }

    #[async_trait]
    impl BlockDataProvider for MockProvider {
        async fn get_events(
            &self,
            from: u64,
            to: u64,
            _filter: &EventFilter,
        ) -> Result<Vec<DecodedEvent>, IndexerError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);

            // Fail if we still have remaining failures to burn.
            let remaining = self.fail_times.load(Ordering::SeqCst);
            if remaining > 0 {
                self.fail_times.store(remaining - 1, Ordering::SeqCst);
                return Err(IndexerError::Rpc("mock RPC error".into()));
            }

            // One event per block.
            let events = (from..=to).map(|b| make_event(b, 0)).collect();
            Ok(events)
        }

        async fn get_block(
            &self,
            number: u64,
        ) -> Result<Option<crate::types::BlockSummary>, IndexerError> {
            Ok(Some(crate::types::BlockSummary {
                number,
                hash: format!("0x{number:064x}"),
                parent_hash: format!("0x{:064x}", number.saturating_sub(1)),
                timestamp: number as i64 * 12,
                tx_count: 0,
            }))
        }
    }

    // ── Segment calculation tests ─────────────────────────────────────────────

    #[test]
    fn segments_exact_multiple() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 29_999,
            segment_size: 10_000,
            ..BackfillConfig::default()
        };
        let segs = cfg.segments();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].from_block, 0);
        assert_eq!(segs[0].to_block, 9_999);
        assert_eq!(segs[1].from_block, 10_000);
        assert_eq!(segs[1].to_block, 19_999);
        assert_eq!(segs[2].from_block, 20_000);
        assert_eq!(segs[2].to_block, 29_999);
    }

    #[test]
    fn segments_non_multiple_range() {
        let cfg = BackfillConfig {
            from_block: 100,
            to_block: 10_250,
            segment_size: 10_000,
            ..BackfillConfig::default()
        };
        let segs = cfg.segments();
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].from_block, 100);
        assert_eq!(segs[0].to_block, 10_099); // 100 + 10_000 - 1
        assert_eq!(segs[1].from_block, 10_100);
        assert_eq!(segs[1].to_block, 10_250);
    }

    #[test]
    fn segments_single_block() {
        let cfg = BackfillConfig {
            from_block: 42,
            to_block: 42,
            segment_size: 10_000,
            ..BackfillConfig::default()
        };
        let segs = cfg.segments();
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].from_block, 42);
        assert_eq!(segs[0].to_block, 42);
        assert_eq!(segs[0].block_count(), 1);
    }

    #[test]
    fn segments_empty_when_from_gt_to() {
        let cfg = BackfillConfig {
            from_block: 100,
            to_block: 50, // invalid but should not panic
            segment_size: 10_000,
            ..BackfillConfig::default()
        };
        assert!(cfg.segments().is_empty());
    }

    #[test]
    fn segment_ids_are_sequential() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 49_999,
            segment_size: 10_000,
            ..BackfillConfig::default()
        };
        for (i, seg) in cfg.segments().iter().enumerate() {
            assert_eq!(seg.id, i);
        }
    }

    // ── Config validation tests ───────────────────────────────────────────────

    #[test]
    fn config_validate_ok() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 1_000,
            ..BackfillConfig::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn config_validate_from_gt_to() {
        let cfg = BackfillConfig {
            from_block: 500,
            to_block: 100,
            ..BackfillConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_validate_zero_concurrency() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 100,
            concurrency: 0,
            ..BackfillConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_validate_zero_segment_size() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 100,
            segment_size: 0,
            ..BackfillConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn config_validate_zero_batch_size() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 100,
            batch_size: 0,
            ..BackfillConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    // ── Progress tracking tests ───────────────────────────────────────────────

    #[test]
    fn progress_percent_zero_at_start() {
        let p = BackfillProgress {
            total_segments: 10,
            completed_segments: 0,
            failed_segments: 0,
            total_blocks: 100_000,
            processed_blocks: 0,
            total_events: 0,
            elapsed: Duration::from_secs(0),
        };
        assert_eq!(p.percent_complete(), 0.0);
    }

    #[test]
    fn progress_percent_complete_100() {
        let p = BackfillProgress {
            total_segments: 10,
            completed_segments: 10,
            failed_segments: 0,
            total_blocks: 100_000,
            processed_blocks: 100_000,
            total_events: 42,
            elapsed: Duration::from_secs(10),
        };
        assert!((p.percent_complete() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn progress_blocks_per_second() {
        let p = BackfillProgress {
            total_segments: 1,
            completed_segments: 1,
            failed_segments: 0,
            total_blocks: 1000,
            processed_blocks: 500,
            total_events: 0,
            elapsed: Duration::from_secs(5),
        };
        // 500 blocks / 5 s = 100 bps
        assert!((p.blocks_per_second() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn progress_blocks_per_second_zero_elapsed() {
        let p = BackfillProgress {
            total_segments: 1,
            completed_segments: 0,
            failed_segments: 0,
            total_blocks: 1000,
            processed_blocks: 0,
            total_events: 0,
            elapsed: Duration::from_secs(0),
        };
        assert_eq!(p.blocks_per_second(), 0.0);
    }

    #[test]
    fn progress_eta_zero_when_done() {
        let p = BackfillProgress {
            total_segments: 1,
            completed_segments: 1,
            failed_segments: 0,
            total_blocks: 1000,
            processed_blocks: 1000,
            total_events: 0,
            elapsed: Duration::from_secs(10),
        };
        assert_eq!(p.eta(), Duration::ZERO);
    }

    #[test]
    fn progress_eta_reasonable() {
        let p = BackfillProgress {
            total_segments: 2,
            completed_segments: 1,
            failed_segments: 0,
            total_blocks: 1000,
            processed_blocks: 500,
            total_events: 0,
            elapsed: Duration::from_secs(5),
        };
        // 500 remaining / 100 bps = 5 s
        let eta = p.eta();
        assert!(eta.as_secs_f64() > 4.9 && eta.as_secs_f64() < 5.1);
    }

    #[test]
    fn progress_percent_empty_range() {
        let p = BackfillProgress {
            total_segments: 0,
            completed_segments: 0,
            failed_segments: 0,
            total_blocks: 0,
            processed_blocks: 0,
            total_events: 0,
            elapsed: Duration::ZERO,
        };
        // Defined as 100% when there is nothing to process.
        assert_eq!(p.percent_complete(), 100.0);
    }

    // ── Engine integration tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn engine_empty_range() {
        let cfg = BackfillConfig {
            from_block: 100,
            to_block: 50, // intentionally inverted
            ..BackfillConfig::default()
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        // Validation should surface the error.
        let result = engine.run().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn engine_single_block_range() {
        let cfg = BackfillConfig {
            from_block: 42,
            to_block: 42,
            segment_size: 10_000,
            batch_size: 500,
            concurrency: 2,
            retry_attempts: 0,
            retry_delay: Duration::from_millis(10),
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.total_events, 1); // one event for block 42
        assert!(result.failed_segments.is_empty());
    }

    #[tokio::test]
    async fn engine_successful_backfill_counts_events() {
        // Range: 0..=99 → 10 segments of 10 blocks each
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 99,
            segment_size: 10,
            batch_size: 5,
            concurrency: 4,
            retry_attempts: 0,
            retry_delay: Duration::from_millis(10),
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        assert_eq!(result.segments.len(), 10);
        // Mock returns one event per block.
        assert_eq!(result.total_events, 100);
        assert!(result.failed_segments.is_empty());
        assert!(result
            .segments
            .iter()
            .all(|s| s.status == SegmentStatus::Complete));
    }

    #[tokio::test]
    async fn engine_concurrency_bounded_by_semaphore() {
        // Larger range with explicit concurrency cap.
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 499,
            segment_size: 50,
            batch_size: 25,
            concurrency: 3,
            retry_attempts: 0,
            retry_delay: Duration::from_millis(1),
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider.clone(), EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        // 500 blocks → 10 segments; all should complete.
        assert_eq!(result.segments.len(), 10);
        assert_eq!(result.total_events, 500);
        assert!(result.failed_segments.is_empty());
    }

    #[tokio::test]
    async fn engine_retry_succeeds_after_failures() {
        // Provider fails once, then succeeds — retry_attempts=3 should recover.
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 9,
            segment_size: 10,
            batch_size: 10,
            concurrency: 1,
            retry_attempts: 3,
            retry_delay: Duration::from_millis(1),
        };
        // Fail the first call; subsequent calls succeed.
        let provider = Arc::new(MockProvider::with_failures(1));
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.total_events, 10);
        assert!(result.failed_segments.is_empty());
        assert_eq!(result.segments[0].status, SegmentStatus::Complete);
    }

    #[tokio::test]
    async fn engine_segment_fails_after_all_retries() {
        // Provider always fails — segment should end up as Failed.
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 9,
            segment_size: 10,
            batch_size: 10,
            concurrency: 1,
            retry_attempts: 2,
            retry_delay: Duration::from_millis(1),
        };
        // Fail more times than retry_attempts allows.
        let provider = Arc::new(MockProvider::with_failures(10));
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.total_events, 0);
        assert_eq!(result.failed_segments, vec![0]);
        assert_eq!(result.segments[0].status, SegmentStatus::Failed);
    }

    #[tokio::test]
    async fn engine_large_range_many_segments() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 9_999,
            segment_size: 1_000,
            batch_size: 200,
            concurrency: 5,
            retry_attempts: 0,
            retry_delay: Duration::from_millis(1),
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");
        let result = engine.run().await.unwrap();

        assert_eq!(result.segments.len(), 10);
        assert_eq!(result.total_events, 10_000);
        assert!(result.failed_segments.is_empty());
    }

    #[tokio::test]
    async fn engine_progress_reflects_completed_segments() {
        let cfg = BackfillConfig {
            from_block: 0,
            to_block: 19,
            segment_size: 10,
            batch_size: 10,
            concurrency: 1,
            retry_attempts: 0,
            retry_delay: Duration::from_millis(1),
        };
        let provider = Arc::new(MockProvider::new());
        let engine = BackfillEngine::new(cfg, provider, EventFilter::default(), "ethereum");

        // Snapshot before run (nothing processed yet).
        let pre = engine.progress().await;
        assert_eq!(pre.completed_segments, 0);
        assert_eq!(pre.processed_blocks, 0);

        engine.run().await.unwrap();

        let post = engine.progress().await;
        assert_eq!(post.completed_segments, 2);
        assert_eq!(post.processed_blocks, 20);
        assert!((post.percent_complete() - 100.0).abs() < f64::EPSILON);
    }

    // ── SegmentMerger tests ───────────────────────────────────────────────────

    #[test]
    fn merger_preserves_block_order() {
        let segs = vec![
            make_segment(0, 0, 9, SegmentStatus::Complete),
            make_segment(1, 10, 19, SegmentStatus::Complete),
        ];
        // Provide segment 1's events first to verify sorting.
        let events = vec![
            vec![make_event(5, 0), make_event(3, 0)],
            vec![make_event(15, 0), make_event(10, 0)],
        ];

        let merged = SegmentMerger::merge(&segs, &events);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].block_number, 3);
        assert_eq!(merged[1].block_number, 5);
        assert_eq!(merged[2].block_number, 10);
        assert_eq!(merged[3].block_number, 15);
    }

    #[test]
    fn merger_skips_failed_segments() {
        let segs = vec![
            make_segment(0, 0, 9, SegmentStatus::Complete),
            make_segment(1, 10, 19, SegmentStatus::Failed),
            make_segment(2, 20, 29, SegmentStatus::Complete),
        ];
        let events = vec![
            vec![make_event(1, 0)],
            vec![make_event(15, 0)], // should be excluded
            vec![make_event(21, 0)],
        ];

        let merged = SegmentMerger::merge(&segs, &events);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().all(|e| e.block_number != 15));
    }

    #[test]
    fn merger_tiebreaks_by_log_index() {
        let segs = vec![make_segment(0, 100, 100, SegmentStatus::Complete)];
        let events = vec![vec![
            make_event(100, 3),
            make_event(100, 1),
            make_event(100, 2),
        ]];

        let merged = SegmentMerger::merge(&segs, &events);
        assert_eq!(merged[0].log_index, 1);
        assert_eq!(merged[1].log_index, 2);
        assert_eq!(merged[2].log_index, 3);
    }

    #[test]
    fn merger_empty_input() {
        let merged = SegmentMerger::merge(&[], &[]);
        assert!(merged.is_empty());
    }
}
