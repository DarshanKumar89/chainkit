//! Example 10: Parallel Backfill Engine
//!
//! Demonstrates concurrent block range processing for fast historical sync.
//!
//! Run: `cargo run --example 10_parallel_backfill`

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chainindex_core::backfill::*;
use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::types::{BlockSummary, EventFilter};

/// Mock block data provider for demonstration.
struct MockProvider;

#[async_trait]
impl BlockDataProvider for MockProvider {
    async fn get_events(
        &self,
        from: u64,
        to: u64,
        _filter: &EventFilter,
    ) -> Result<Vec<DecodedEvent>, IndexerError> {
        // Simulate 1 event per block
        let mut events = Vec::new();
        for block in from..=to {
            events.push(DecodedEvent {
                chain: "ethereum".into(),
                schema: "Transfer".into(),
                address: "0xToken".into(),
                tx_hash: format!("0xtx_{block}"),
                block_number: block,
                log_index: 0,
                fields_json: serde_json::json!({
                    "from": "0xA",
                    "to": "0xB",
                    "value": block * 100
                }),
            });
        }
        Ok(events)
    }

    async fn get_block(&self, number: u64) -> Result<Option<BlockSummary>, IndexerError> {
        Ok(Some(BlockSummary {
            number,
            hash: format!("0xhash_{number}"),
            parent_hash: format!("0xhash_{}", number.saturating_sub(1)),
            timestamp: 1700000000 + (number as i64 * 12),
            tx_count: 100,
        }))
    }
}

#[tokio::main]
async fn main() {
    println!("=== Parallel Backfill Engine Demo ===\n");

    // 1. Configure backfill
    let config = BackfillConfig {
        from_block: 19_000_000,
        to_block: 19_100_000,
        concurrency: 4,
        segment_size: 25_000,
        batch_size: 500,
        retry_attempts: 3,
        retry_delay: Duration::from_secs(1),
    };

    println!("Backfill config:");
    println!("  Range:       {} → {}", config.from_block, config.to_block);
    println!("  Concurrency: {} workers", config.concurrency);
    println!("  Segment size: {} blocks", config.segment_size);
    println!("  Batch size:  {} blocks", config.batch_size);

    // 2. Calculate segments
    let segments = config.segments();
    println!("\nSegments: {}", segments.len());
    for seg in &segments {
        println!(
            "  Segment {}: blocks {} → {} ({} blocks)",
            seg.id,
            seg.from_block,
            seg.to_block,
            seg.block_count()
        );
    }

    // 3. Run backfill
    println!("\n--- Running Backfill ---");
    let provider = Arc::new(MockProvider);
    let filter = EventFilter::address("0xToken");
    let engine = BackfillEngine::new(config, provider, filter, "ethereum");

    let result = engine.run().await.unwrap();

    println!("\n--- Backfill Results ---");
    println!("Total events:    {}", result.total_events);
    println!("Total duration:  {:?}", result.total_duration);
    println!("Failed segments: {:?}", result.failed_segments);

    // 4. Check progress
    let progress = engine.progress().await;
    println!("\n--- Final Progress ---");
    println!(
        "Completed: {}/{} segments",
        progress.completed_segments, progress.total_segments
    );
    println!(
        "Processed: {}/{} blocks",
        progress.processed_blocks, progress.total_blocks
    );
    println!("Events:    {}", progress.total_events);
    println!("Complete:  {:.1}%", progress.percent_complete());
    println!("Speed:     {:.0} blocks/sec", progress.blocks_per_second());

    // 5. Segment merger
    println!("\n--- Segment Merger ---");
    // Create some mock events out of order
    let events_seg1 = vec![DecodedEvent {
        chain: "ethereum".into(),
        schema: "Transfer".into(),
        address: "0xToken".into(),
        tx_hash: "0xtx_200".into(),
        block_number: 200,
        log_index: 0,
        fields_json: serde_json::Value::Null,
    }];
    let events_seg0 = vec![DecodedEvent {
        chain: "ethereum".into(),
        schema: "Transfer".into(),
        address: "0xToken".into(),
        tx_hash: "0xtx_100".into(),
        block_number: 100,
        log_index: 0,
        fields_json: serde_json::Value::Null,
    }];

    let seg0 = BackfillSegment {
        id: 0,
        from_block: 100,
        to_block: 150,
        status: SegmentStatus::Complete,
        events_processed: 1,
        duration: Some(Duration::from_millis(50)),
        error: None,
    };
    let seg1 = BackfillSegment {
        id: 1,
        from_block: 151,
        to_block: 200,
        status: SegmentStatus::Complete,
        events_processed: 1,
        duration: Some(Duration::from_millis(50)),
        error: None,
    };

    let merged = SegmentMerger::merge(&[seg0, seg1], &[events_seg0, events_seg1]);
    println!("Merged {} events in block order:", merged.len());
    for ev in &merged {
        println!("  Block {}: {}", ev.block_number, ev.tx_hash);
    }

    println!("\nParallel backfill demo complete!");
}
