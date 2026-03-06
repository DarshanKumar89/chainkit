//! Example 02: Reorg Detection
//!
//! Demonstrates the 4-scenario reorg detection system:
//! - Short reorg (1-3 blocks)
//! - Deep reorg (checkpoint mismatch)
//! - BlockTracker sliding window
//! - ReorgDetector with finality models
//!
//! Run: `cargo run --example 02_reorg_detection`

use chainindex_core::finality::FinalityRegistry;
use chainindex_core::reorg::ReorgDetector;
use chainindex_core::tracker::BlockTracker;
use chainindex_core::types::BlockSummary;

fn block(number: u64, hash: &str, parent_hash: &str) -> BlockSummary {
    BlockSummary {
        number,
        hash: hash.into(),
        parent_hash: parent_hash.into(),
        timestamp: 1700000000 + (number as i64 * 12),
        tx_count: 10,
    }
}

fn main() {
    println!("=== Reorg Detection Demo ===\n");

    // 1. Block Tracker — sliding window of recent blocks
    let mut tracker = BlockTracker::new(128);

    // Push a normal chain of blocks
    let blocks = vec![
        block(100, "0xaaa", "0x099"),
        block(101, "0xbbb", "0xaaa"),
        block(102, "0xccc", "0xbbb"),
        block(103, "0xddd", "0xccc"),
    ];

    for b in &blocks {
        tracker.push(b.clone()).unwrap();
        println!("Pushed block {} (hash: {})", b.number, b.hash);
    }
    println!("Tracker head: block {}", tracker.head().unwrap().number);

    // 2. Simulate a short reorg — block 103 has different parent
    println!("\n--- Short Reorg Simulation ---");
    let reorged_103 = block(103, "0xeee", "0xfff"); // wrong parent
    match tracker.push(reorged_103.clone()) {
        Ok(()) => println!("Block accepted (no reorg)"),
        Err(depth) => println!(
            "REORG DETECTED! Depth: {} blocks (block {} has parent {} but expected {})",
            depth, reorged_103.number, reorged_103.parent_hash, blocks[2].hash
        ),
    }

    // 3. Reorg Detector
    println!("\n--- ReorgDetector ---");
    let detector = ReorgDetector::new(12);
    println!("Detector created with confirmation_depth=12");

    // 4. Finality models
    println!("\n--- Finality Models ---");
    let finality = FinalityRegistry::new();

    for chain in &["ethereum", "polygon", "arbitrum", "solana", "base"] {
        if let Some(config) = finality.get(chain) {
            println!(
                "  {:<12} safe={:>3} blocks  finalized={:>3} blocks  block_time={:>6}ms  reorg_window={:>3}  L2={}",
                chain,
                config.safe_confirmations,
                config.finalized_confirmations,
                config.block_time.as_millis(),
                config.reorg_window,
                config.settlement_chain.as_deref().unwrap_or("no")
            );
        }
    }

    // 5. Rewind on reorg
    println!("\n--- Rewind Demo ---");
    tracker.rewind_to(101);
    println!(
        "After rewind to block 101, tracker head: block {}",
        tracker.head().unwrap().number
    );

    println!("\nReorg detection demo complete!");
    let _ = detector;
}
