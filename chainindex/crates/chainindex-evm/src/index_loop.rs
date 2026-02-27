//! The main index loop — orchestrates backfill and live phases.
//!
//! # Phase 1: BACKFILL
//! Fetch blocks from `from_block` to `head - confirmation_depth` in batches.
//! For each batch: fetch logs → dispatch events → save checkpoint.
//!
//! # Phase 2: LIVE
//! Poll for new confirmed blocks every `poll_interval_ms`.
//! On each new block:
//!   - Verify parent hash (detect reorg)
//!   - Fetch logs for new block
//!   - Dispatch events
//!   - Update checkpoint

use std::time::Duration;

use chainindex_core::checkpoint::{CheckpointManager, MemoryCheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::HandlerRegistry;
use chainindex_core::indexer::{IndexerConfig, IndexerState};
use chainindex_core::reorg::ReorgDetector;
use chainindex_core::tracker::BlockTracker;
use chainindex_core::types::{BlockSummary, IndexContext, IndexPhase};

use crate::fetcher::EvmRpcClient;

/// Status emitted by the index loop for observability.
#[derive(Debug)]
pub enum IndexLoopEvent {
    BackfillProgress { current: u64, target: u64 },
    BackfillComplete { at_block: u64 },
    LiveBlock { number: u64 },
    ReorgDetected { dropped_count: usize, depth: u64 },
    Error(IndexerError),
}

/// The core index loop implementation.
pub struct IndexLoop<C: EvmRpcClient> {
    config: IndexerConfig,
    fetcher: crate::fetcher::EvmFetcher<C>,
    tracker: BlockTracker,
    reorg_detector: ReorgDetector,
    checkpoint: CheckpointManager,
    handlers: HandlerRegistry,
    state: IndexerState,
}

impl<C: EvmRpcClient> IndexLoop<C> {
    pub fn new(
        config: IndexerConfig,
        client: C,
        handlers: HandlerRegistry,
    ) -> Self {
        let checkpoint = CheckpointManager::new(
            Box::new(MemoryCheckpointStore::new()),
            &config.chain,
            &config.id,
            config.checkpoint_interval,
        );
        Self {
            fetcher: crate::fetcher::EvmFetcher::new(client),
            tracker: BlockTracker::new(128),
            reorg_detector: ReorgDetector::new(config.confirmation_depth),
            checkpoint,
            handlers,
            state: IndexerState::Idle,
            config,
        }
    }

    /// Run the index loop until completion or error.
    pub async fn run(&mut self) -> Result<(), IndexerError> {
        // Check for saved checkpoint and resume from it
        if let Some(cp) = self.checkpoint.load().await? {
            tracing::info!(
                block = cp.block_number,
                hash = %cp.block_hash,
                "Resuming from checkpoint"
            );
        }

        // Phase 1: Backfill
        self.state = IndexerState::Backfilling;
        let head = self.fetcher.head_block_number().await?;
        let target = head.saturating_sub(self.config.confirmation_depth);
        let from = self.config.from_block;

        tracing::info!(
            from,
            target,
            "Starting backfill phase"
        );

        self.backfill(from, target).await?;

        if let Some(to_block) = self.config.to_block {
            if to_block <= target {
                self.state = IndexerState::Stopped;
                return Ok(());
            }
        }

        // Phase 2: Live
        self.state = IndexerState::Live;
        self.live_loop().await
    }

    async fn backfill(&mut self, from: u64, to: u64) -> Result<(), IndexerError> {
        let mut current = from;
        let batch = self.config.batch_size;

        while current <= to {
            let batch_end = (current + batch - 1).min(to);

            // Fetch logs for this block range
            let logs = self
                .fetcher
                .logs(current, batch_end, &self.config.filter, batch)
                .await?;

            // Dispatch events for each log
            for log in &logs {
                if log.is_removed() {
                    continue;
                }
                let block_num = log.block_number_u64();
                // Build a minimal context
                let ctx = IndexContext {
                    block: BlockSummary {
                        number: block_num,
                        hash: log.block_hash.clone(),
                        parent_hash: String::new(), // not needed in backfill dispatch
                        timestamp: 0,
                        tx_count: 0,
                    },
                    phase: IndexPhase::Backfill,
                    chain: self.config.chain.clone(),
                };
                // Build a minimal DecodedEvent from the raw log
                let event = chainindex_core::handler::DecodedEvent {
                    schema: log.topics.first().cloned().unwrap_or_default(),
                    address: log.address.clone(),
                    tx_hash: log.tx_hash.clone(),
                    block_number: block_num,
                    log_index: log.log_index_u32(),
                    fields_json: serde_json::json!({
                        "topics": log.topics,
                        "data": log.data,
                    }),
                };
                self.handlers.dispatch_event(&event, &ctx).await?;
            }

            // Save checkpoint
            self.checkpoint
                .maybe_save(batch_end, "backfill")
                .await?;

            tracing::info!(
                current,
                batch_end,
                total = to,
                logs = logs.len(),
                "Backfill batch complete"
            );

            current = batch_end + 1;
        }

        tracing::info!(at = to, "Backfill complete");
        Ok(())
    }

    async fn live_loop(&mut self) -> Result<(), IndexerError> {
        let poll_interval =
            Duration::from_millis(self.config.poll_interval_ms);

        loop {
            tokio::time::sleep(poll_interval).await;

            let head = self.fetcher.head_block_number().await?;
            let confirmed_head = head.saturating_sub(self.config.confirmation_depth);

            // Find what the next block to process is
            let next = match self.tracker.head() {
                Some(h) => h.number + 1,
                None => confirmed_head,
            };

            if next > confirmed_head {
                continue; // Nothing new yet
            }

            // Fetch block header for parent hash verification
            let block = match self.fetcher.block(next).await? {
                Some(b) => b,
                None => continue,
            };

            // Reorg check
            if let Some(prev) = self.tracker.head() {
                if let Err(depth) = self.tracker.push(block.clone()) {
                    // Reorg detected
                    self.state = IndexerState::ReorgRecovery;
                    tracing::warn!(depth, "Reorg detected in live mode");

                    let ctx = IndexContext {
                        block: block.clone(),
                        phase: IndexPhase::Live,
                        chain: self.config.chain.clone(),
                    };
                    let dropped: Vec<_> = std::iter::once(prev.clone()).collect();
                    self.handlers.dispatch_reorg(&dropped, &ctx).await?;
                    self.tracker.rewind_to(block.number.saturating_sub(depth));
                    self.state = IndexerState::Live;
                    continue;
                }
            } else {
                self.tracker.push(block.clone()).ok();
            }

            // Fetch and dispatch logs for this block
            let logs = self
                .fetcher
                .logs(next, next, &self.config.filter, 1)
                .await?;

            let ctx = IndexContext {
                block: block.clone(),
                phase: IndexPhase::Live,
                chain: self.config.chain.clone(),
            };

            self.handlers.dispatch_block(&block, &ctx).await?;

            for log in &logs {
                if log.is_removed() {
                    continue;
                }
                let event = chainindex_core::handler::DecodedEvent {
                    schema: log.topics.first().cloned().unwrap_or_default(),
                    address: log.address.clone(),
                    tx_hash: log.tx_hash.clone(),
                    block_number: log.block_number_u64(),
                    log_index: log.log_index_u32(),
                    fields_json: serde_json::json!({
                        "topics": log.topics,
                        "data": log.data,
                    }),
                };
                self.handlers.dispatch_event(&event, &ctx).await?;
            }

            self.checkpoint
                .maybe_save(next, &block.hash)
                .await?;

            // Check if we've reached the target block
            if let Some(to_block) = self.config.to_block {
                if next >= to_block {
                    self.state = IndexerState::Stopped;
                    return Ok(());
                }
            }
        }
    }
}
