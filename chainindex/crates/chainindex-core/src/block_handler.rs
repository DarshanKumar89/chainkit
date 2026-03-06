//! Enhanced block handler system — interval handlers, setup handlers,
//! and execution ordering for block-level operations.
//!
//! Block handlers fire for every block. Interval handlers fire every N blocks.
//! Setup handlers fire once before indexing begins. All block-level handlers
//! execute BEFORE event handlers for the same block.
//!
//! # Example
//!
//! ```rust,ignore
//! use chainindex_core::block_handler::{IntervalHandler, SetupHandler, BlockHandlerScheduler};
//!
//! // Snapshot handler fires every 1000 blocks
//! struct SnapshotHandler;
//!
//! #[async_trait::async_trait]
//! impl IntervalHandler for SnapshotHandler {
//!     async fn handle(&self, block: &BlockSummary, ctx: &IndexContext) -> Result<(), IndexerError> {
//!         println!("Taking snapshot at block {}", block.number);
//!         Ok(())
//!     }
//!     fn interval(&self) -> u64 { 1000 }
//!     fn name(&self) -> &str { "snapshot" }
//! }
//! ```

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::IndexerError;
use crate::types::{BlockSummary, IndexContext};

// ─── IntervalHandler ─────────────────────────────────────────────────────────

/// Handler that fires every N blocks.
///
/// Useful for periodic tasks such as taking snapshots, computing aggregates,
/// flushing caches, or emitting metrics at regular block intervals.
#[async_trait]
pub trait IntervalHandler: Send + Sync {
    /// Called every `interval()` blocks.
    ///
    /// The handler receives the current block summary and indexing context.
    /// Returning an error will propagate up to the index loop.
    async fn handle(&self, block: &BlockSummary, ctx: &IndexContext) -> Result<(), IndexerError>;

    /// How often this handler should fire, measured in blocks.
    ///
    /// For example, returning `100` means this handler fires on blocks
    /// 0, 100, 200, 300, etc.
    fn interval(&self) -> u64;

    /// Human-readable handler name for logging and diagnostics.
    fn name(&self) -> &str;
}

// ─── SetupHandler ────────────────────────────────────────────────────────────

/// Handler that fires once before indexing starts.
///
/// Use this for one-time initialization tasks such as creating database
/// tables, registering metrics, or loading reference data.
#[async_trait]
pub trait SetupHandler: Send + Sync {
    /// Called once during indexer initialization, before any blocks are processed.
    ///
    /// The context contains the starting block information.
    async fn setup(&self, ctx: &IndexContext) -> Result<(), IndexerError>;

    /// Human-readable handler name for logging and diagnostics.
    fn name(&self) -> &str;
}

// ─── BlockHandlerScheduler ───────────────────────────────────────────────────

/// Manages block-level handler scheduling and execution.
///
/// The scheduler maintains a list of interval handlers (which fire every N
/// blocks) and setup handlers (which fire once). It determines when each
/// handler should run and executes them in registration order.
pub struct BlockHandlerScheduler {
    /// Interval handlers, each with its own cadence.
    interval_handlers: Vec<Arc<dyn IntervalHandler>>,
    /// Setup handlers that run once before indexing.
    setup_handlers: Vec<Arc<dyn SetupHandler>>,
    /// Whether `run_setup` has already been called.
    setup_complete: bool,
}

impl BlockHandlerScheduler {
    /// Create a new empty scheduler.
    pub fn new() -> Self {
        Self {
            interval_handlers: Vec::new(),
            setup_handlers: Vec::new(),
            setup_complete: false,
        }
    }

    /// Register an interval handler.
    ///
    /// Handlers are executed in registration order when their interval is due.
    pub fn register_interval(&mut self, handler: Arc<dyn IntervalHandler>) {
        tracing::debug!(
            handler = handler.name(),
            interval = handler.interval(),
            "registered interval handler"
        );
        self.interval_handlers.push(handler);
    }

    /// Register a setup handler.
    ///
    /// Setup handlers run once during `run_setup`, in registration order.
    pub fn register_setup(&mut self, handler: Arc<dyn SetupHandler>) {
        tracing::debug!(handler = handler.name(), "registered setup handler");
        self.setup_handlers.push(handler);
    }

    /// Run all setup handlers once.
    ///
    /// This method is idempotent — calling it more than once has no effect.
    /// Returns an error if any setup handler fails.
    pub async fn run_setup(&mut self, ctx: &IndexContext) -> Result<(), IndexerError> {
        if self.setup_complete {
            tracing::debug!("setup already complete, skipping");
            return Ok(());
        }

        for handler in &self.setup_handlers {
            tracing::info!(handler = handler.name(), "running setup handler");
            handler
                .setup(ctx)
                .await
                .map_err(|e| IndexerError::Handler {
                    handler: handler.name().to_string(),
                    reason: e.to_string(),
                })?;
        }

        self.setup_complete = true;
        Ok(())
    }

    /// Run all interval handlers that are due for the given block.
    ///
    /// A handler fires when `block.number % handler.interval() == 0`.
    /// Handlers are executed in registration order.
    pub async fn run_block(
        &self,
        block: &BlockSummary,
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        for handler in &self.interval_handlers {
            if self.should_run_interval(handler.as_ref(), block.number) {
                tracing::debug!(
                    handler = handler.name(),
                    block = block.number,
                    "running interval handler"
                );
                handler
                    .handle(block, ctx)
                    .await
                    .map_err(|e| IndexerError::Handler {
                        handler: handler.name().to_string(),
                        reason: e.to_string(),
                    })?;
            }
        }
        Ok(())
    }

    /// Check whether an interval handler should fire at the given block number.
    ///
    /// Returns `true` if `block_number % interval == 0`. An interval of 0 is
    /// treated as "never fire" to avoid division by zero.
    pub fn should_run_interval(&self, handler: &dyn IntervalHandler, block_number: u64) -> bool {
        let interval = handler.interval();
        if interval == 0 {
            return false;
        }
        block_number.is_multiple_of(interval)
    }

    /// Returns whether setup has been completed.
    pub fn is_setup_complete(&self) -> bool {
        self.setup_complete
    }

    /// Returns the number of registered interval handlers.
    pub fn interval_handler_count(&self) -> usize {
        self.interval_handlers.len()
    }

    /// Returns the number of registered setup handlers.
    pub fn setup_handler_count(&self) -> usize {
        self.setup_handlers.len()
    }
}

impl Default for BlockHandlerScheduler {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Helper: create a dummy IndexContext for testing.
    fn dummy_ctx() -> IndexContext {
        IndexContext {
            block: BlockSummary {
                number: 0,
                hash: "0x0".into(),
                parent_hash: "0x0".into(),
                timestamp: 0,
                tx_count: 0,
            },
            phase: crate::types::IndexPhase::Backfill,
            chain: "ethereum".into(),
        }
    }

    /// Helper: create a BlockSummary at the given block number.
    fn block_at(number: u64) -> BlockSummary {
        BlockSummary {
            number,
            hash: format!("0x{:x}", number),
            parent_hash: format!("0x{:x}", number.saturating_sub(1)),
            timestamp: number as i64 * 12,
            tx_count: 0,
        }
    }

    /// A test interval handler that counts invocations.
    struct CountingInterval {
        count: Arc<AtomicU32>,
        interval: u64,
        name: String,
    }

    impl CountingInterval {
        fn new(interval: u64, name: &str) -> (Arc<Self>, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            let handler = Arc::new(Self {
                count: count.clone(),
                interval,
                name: name.to_string(),
            });
            (handler, count)
        }
    }

    #[async_trait]
    impl IntervalHandler for CountingInterval {
        async fn handle(
            &self,
            _block: &BlockSummary,
            _ctx: &IndexContext,
        ) -> Result<(), IndexerError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn interval(&self) -> u64 {
            self.interval
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    /// A test setup handler that counts invocations.
    struct CountingSetup {
        count: Arc<AtomicU32>,
        name: String,
    }

    impl CountingSetup {
        fn new(name: &str) -> (Arc<Self>, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            let handler = Arc::new(Self {
                count: count.clone(),
                name: name.to_string(),
            });
            (handler, count)
        }
    }

    #[async_trait]
    impl SetupHandler for CountingSetup {
        async fn setup(&self, _ctx: &IndexContext) -> Result<(), IndexerError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    /// A failing interval handler for error propagation tests.
    struct FailingInterval;

    #[async_trait]
    impl IntervalHandler for FailingInterval {
        async fn handle(
            &self,
            _block: &BlockSummary,
            _ctx: &IndexContext,
        ) -> Result<(), IndexerError> {
            Err(IndexerError::Other("interval handler failed".into()))
        }

        fn interval(&self) -> u64 {
            1
        }

        fn name(&self) -> &str {
            "failing"
        }
    }

    /// A failing setup handler for error propagation tests.
    struct FailingSetup;

    #[async_trait]
    impl SetupHandler for FailingSetup {
        async fn setup(&self, _ctx: &IndexContext) -> Result<(), IndexerError> {
            Err(IndexerError::Other("setup failed".into()))
        }

        fn name(&self) -> &str {
            "failing_setup"
        }
    }

    // ── Test: register interval handler ──────────────────────────────────────

    #[test]
    fn register_interval_handler() {
        let mut scheduler = BlockHandlerScheduler::new();
        assert_eq!(scheduler.interval_handler_count(), 0);

        let (handler, _) = CountingInterval::new(10, "test");
        scheduler.register_interval(handler);
        assert_eq!(scheduler.interval_handler_count(), 1);
    }

    // ── Test: interval handler fires at correct interval ─────────────────────

    #[tokio::test]
    async fn interval_handler_fires_at_correct_interval() {
        let mut scheduler = BlockHandlerScheduler::new();
        let (handler, count) = CountingInterval::new(10, "every_10");
        scheduler.register_interval(handler);

        let ctx = dummy_ctx();

        // Process blocks 0..30 — handler should fire at 0, 10, 20 = 3 times
        for i in 0..30 {
            scheduler.run_block(&block_at(i), &ctx).await.unwrap();
        }

        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    // ── Test: setup handler runs once ────────────────────────────────────────

    #[tokio::test]
    async fn setup_runs_once() {
        let mut scheduler = BlockHandlerScheduler::new();
        let (handler, count) = CountingSetup::new("init");
        scheduler.register_setup(handler);

        let ctx = dummy_ctx();

        // Run setup twice — should only execute handlers once
        scheduler.run_setup(&ctx).await.unwrap();
        scheduler.run_setup(&ctx).await.unwrap();

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(scheduler.is_setup_complete());
    }

    // ── Test: multiple interval handlers with different intervals ─────────────

    #[tokio::test]
    async fn multiple_interval_handlers_different_intervals() {
        let mut scheduler = BlockHandlerScheduler::new();

        let (h5, count5) = CountingInterval::new(5, "every_5");
        let (h7, count7) = CountingInterval::new(7, "every_7");
        scheduler.register_interval(h5);
        scheduler.register_interval(h7);

        let ctx = dummy_ctx();

        // Process blocks 0..35
        // every_5 fires at: 0, 5, 10, 15, 20, 25, 30 = 7 times
        // every_7 fires at: 0, 7, 14, 21, 28 = 5 times
        for i in 0..35 {
            scheduler.run_block(&block_at(i), &ctx).await.unwrap();
        }

        assert_eq!(count5.load(Ordering::Relaxed), 7);
        assert_eq!(count7.load(Ordering::Relaxed), 5);
    }

    // ── Test: block 0 handling ───────────────────────────────────────────────

    #[tokio::test]
    async fn block_zero_fires_all_interval_handlers() {
        let mut scheduler = BlockHandlerScheduler::new();

        let (h100, count100) = CountingInterval::new(100, "every_100");
        let (h1000, count1000) = CountingInterval::new(1000, "every_1000");
        scheduler.register_interval(h100);
        scheduler.register_interval(h1000);

        let ctx = dummy_ctx();

        // Block 0 — all interval handlers should fire (0 % N == 0 for all N)
        scheduler.run_block(&block_at(0), &ctx).await.unwrap();

        assert_eq!(count100.load(Ordering::Relaxed), 1);
        assert_eq!(count1000.load(Ordering::Relaxed), 1);
    }

    // ── Test: handler error propagation (interval) ───────────────────────────

    #[tokio::test]
    async fn interval_handler_error_propagation() {
        let mut scheduler = BlockHandlerScheduler::new();
        scheduler.register_interval(Arc::new(FailingInterval));

        let ctx = dummy_ctx();
        let result = scheduler.run_block(&block_at(0), &ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            IndexerError::Handler { handler, reason } => {
                assert_eq!(handler, "failing");
                assert!(reason.contains("interval handler failed"));
            }
            _ => panic!("expected Handler error, got {:?}", err),
        }
    }

    // ── Test: setup handler error propagation ────────────────────────────────

    #[tokio::test]
    async fn setup_handler_error_propagation() {
        let mut scheduler = BlockHandlerScheduler::new();
        scheduler.register_setup(Arc::new(FailingSetup));

        let ctx = dummy_ctx();
        let result = scheduler.run_setup(&ctx).await;

        assert!(result.is_err());
        assert!(!scheduler.is_setup_complete());
    }

    // ── Test: zero interval never fires ──────────────────────────────────────

    #[tokio::test]
    async fn zero_interval_never_fires() {
        let mut scheduler = BlockHandlerScheduler::new();
        let (handler, count) = CountingInterval::new(0, "never");
        scheduler.register_interval(handler);

        let ctx = dummy_ctx();

        for i in 0..100 {
            scheduler.run_block(&block_at(i), &ctx).await.unwrap();
        }

        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    // ── Test: should_run_interval correctness ────────────────────────────────

    #[test]
    fn should_run_interval_correctness() {
        let scheduler = BlockHandlerScheduler::new();
        let (handler, _) = CountingInterval::new(10, "test");

        assert!(scheduler.should_run_interval(handler.as_ref(), 0));
        assert!(!scheduler.should_run_interval(handler.as_ref(), 1));
        assert!(!scheduler.should_run_interval(handler.as_ref(), 9));
        assert!(scheduler.should_run_interval(handler.as_ref(), 10));
        assert!(scheduler.should_run_interval(handler.as_ref(), 100));
        assert!(!scheduler.should_run_interval(handler.as_ref(), 101));
    }

    // ── Test: multiple setup handlers all run ────────────────────────────────

    #[tokio::test]
    async fn multiple_setup_handlers_all_run() {
        let mut scheduler = BlockHandlerScheduler::new();

        let (h1, count1) = CountingSetup::new("setup_a");
        let (h2, count2) = CountingSetup::new("setup_b");
        let (h3, count3) = CountingSetup::new("setup_c");

        scheduler.register_setup(h1);
        scheduler.register_setup(h2);
        scheduler.register_setup(h3);

        let ctx = dummy_ctx();
        scheduler.run_setup(&ctx).await.unwrap();

        assert_eq!(count1.load(Ordering::Relaxed), 1);
        assert_eq!(count2.load(Ordering::Relaxed), 1);
        assert_eq!(count3.load(Ordering::Relaxed), 1);
    }
}
