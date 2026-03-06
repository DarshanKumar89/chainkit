//! Example 01: Basic Indexer Setup
//!
//! Demonstrates configuring and building an indexer with:
//! - IndexerConfig for Ethereum
//! - EventFilter for a specific contract
//! - HandlerRegistry with a custom event handler
//! - CheckpointManager with in-memory store
//!
//! Run: `cargo run --example 01_basic_indexer`

use std::sync::Arc;

use chainindex_core::checkpoint::{CheckpointManager, MemoryCheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::{DecodedEvent, EventHandler, HandlerRegistry};
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter, IndexContext};

/// A simple handler that prints Transfer events.
struct TransferHandler;

#[async_trait::async_trait]
impl EventHandler for TransferHandler {
    async fn handle(&self, event: &DecodedEvent, ctx: &IndexContext) -> Result<(), IndexerError> {
        println!(
            "[Block {}] Transfer on {}: {}",
            ctx.block.number, event.address, event.fields_json
        );
        Ok(())
    }

    fn schema_name(&self) -> &str {
        "Transfer"
    }
}

fn main() {
    // 1. Configure the indexer
    let config = IndexerConfig {
        id: "uniswap-v3-tracker".into(),
        chain: "ethereum".into(),
        from_block: 19_000_000,
        confirmation_depth: 12,
        batch_size: 500,
        checkpoint_interval: 100,
        poll_interval_ms: 2000,
        filter: EventFilter::address("0x1F98431c8aD98523631AE4a59f267346ea31F984"),
        ..Default::default()
    };

    println!("Indexer config:");
    println!("  ID:                 {}", config.id);
    println!("  Chain:              {}", config.chain);
    println!("  From block:         {}", config.from_block);
    println!("  Confirmation depth: {}", config.confirmation_depth);
    println!("  Batch size:         {}", config.batch_size);

    // 2. Register handlers
    let mut registry = HandlerRegistry::new();
    registry.on_event(Arc::new(TransferHandler));

    // 3. Set up checkpoint manager
    let store = Box::new(MemoryCheckpointStore::new());
    let checkpoint = CheckpointManager::new(store, "ethereum", "uniswap-v3-tracker", 100);
    println!("\nCheckpoint manager ready (in-memory store, interval=100)");

    // 4. Create a sample event and dispatch it
    let event = DecodedEvent {
        chain: "ethereum".into(),
        schema: "Transfer".into(),
        address: "0x1F98431c8aD98523631AE4a59f267346ea31F984".into(),
        tx_hash: "0xabc123".into(),
        block_number: 19_000_001,
        log_index: 0,
        fields_json: serde_json::json!({
            "from": "0xAlice",
            "to": "0xBob",
            "value": "1000000000000000000"
        }),
    };

    let ctx = IndexContext {
        block: BlockSummary {
            number: 19_000_001,
            hash: "0xblockhash".into(),
            parent_hash: "0xparenthash".into(),
            timestamp: 1700000000,
            tx_count: 150,
        },
        phase: chainindex_core::types::IndexPhase::Backfill,
        chain: "ethereum".into(),
    };

    // Dispatch synchronously for demo
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        registry.dispatch_event(&event, &ctx).await.unwrap();
    });

    println!("\nBasic indexer setup complete!");
    let _ = checkpoint; // used in real indexing loop
}
