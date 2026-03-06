//! Example 13: RocksDB Storage Backend
//!
//! Demonstrates the RocksDB-style key-value storage with:
//! - Column families
//! - Checkpoint persistence
//! - Event storage and queries
//! - Batch writes
//! - Block hash management
//!
//! Run: `cargo run --example 13_rocksdb_storage`

use chainindex_core::checkpoint::CheckpointStore;
use chainindex_core::handler::DecodedEvent;
use chainindex_storage::RocksDbStorage;

#[tokio::main]
async fn main() {
    println!("=== RocksDB Storage Backend Demo ===\n");

    // 1. Create in-memory RocksDB-style storage
    let storage = RocksDbStorage::in_memory();
    println!("Created in-memory RocksDB storage\n");

    // 2. Checkpoint management
    println!("--- Checkpoint Management ---");
    let checkpoint = chainindex_core::checkpoint::Checkpoint {
        chain_id: "ethereum".into(),
        indexer_id: "uniswap-tracker".into(),
        block_number: 19_000_500,
        block_hash: "0xabc123def456".into(),
        updated_at: chrono::Utc::now().timestamp(),
    };

    storage.save(checkpoint.clone()).await.unwrap();
    println!(
        "Saved checkpoint: block {} ({})",
        checkpoint.block_number, checkpoint.block_hash
    );

    let loaded = storage.load("ethereum", "uniswap-tracker").await.unwrap();
    if let Some(cp) = loaded {
        println!(
            "Loaded checkpoint: block {} ({})",
            cp.block_number, cp.block_hash
        );
    }

    // 3. Event storage
    println!("\n--- Event Storage ---");
    let events = vec![
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xUSDC".into(),
            tx_hash: "0xtx_100_0".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 1000}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Approval".into(),
            address: "0xUSDC".into(),
            tx_hash: "0xtx_101_0".into(),
            block_number: 101,
            log_index: 0,
            fields_json: serde_json::json!({"owner": "0xA", "spender": "0xRouter"}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xWETH".into(),
            tx_hash: "0xtx_102_0".into(),
            block_number: 102,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xC", "to": "0xD", "value": 2000}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Swap".into(),
            address: "0xPool".into(),
            tx_hash: "0xtx_103_0".into(),
            block_number: 103,
            log_index: 0,
            fields_json: serde_json::json!({"amount0": 500, "amount1": -250}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xUSDC".into(),
            tx_hash: "0xtx_200_0".into(),
            block_number: 200,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xE", "to": "0xF", "value": 5000}),
        },
    ];

    // Batch insert
    storage.insert_events_batch(&events).unwrap();
    println!("Batch inserted {} events", events.len());

    // Query by schema
    let transfers = storage.events_by_schema("Transfer").unwrap();
    println!("\nTransfer events: {}", transfers.len());
    for ev in &transfers {
        println!(
            "  Block {} — {} → tx: {}",
            ev.block_number, ev.address, ev.tx_hash
        );
    }

    // Query by address
    let usdc_events = storage.events_by_address("0xUSDC").unwrap();
    println!("\n0xUSDC events: {}", usdc_events.len());

    // Query by block range
    let range = storage.events_in_block_range(100, 102).unwrap();
    println!("Events in blocks 100-102: {}", range.len());

    // 4. Block hash management
    println!("\n--- Block Hash Management ---");
    for i in 100..=110 {
        storage
            .insert_block_hash("ethereum", i, &format!("0xhash_{i}"))
            .unwrap();
    }
    println!("Stored 11 block hashes (100-110)");

    let hash = storage.get_block_hash("ethereum", 105).unwrap();
    println!("Block 105 hash: {:?}", hash);

    // Prune old hashes
    let pruned = storage.prune_block_hashes("ethereum", 5).unwrap();
    println!("Pruned {} old block hashes (kept last 5)", pruned);

    // 5. Rollback (reorg recovery)
    println!("\n--- Rollback After Block 101 ---");
    let deleted = storage.rollback_after(101).unwrap();
    println!("Deleted {} events after block 101", deleted);

    let remaining = storage.events_by_schema("Transfer").unwrap();
    println!("Remaining Transfer events: {}", remaining.len());

    // 6. Storage stats
    println!("\n--- Storage Stats ---");
    let stats = chainindex_storage::rocksdb::StorageStats::collect(&storage);
    println!("  Total events:       {}", stats.total_events);
    println!("  Total checkpoints:  {}", stats.total_checkpoints);
    println!("  Total block hashes: {}", stats.total_block_hashes);
    println!("  Disk usage (est):   {} bytes", stats.disk_usage_bytes);

    println!("\nRocksDB storage demo complete!");
}
