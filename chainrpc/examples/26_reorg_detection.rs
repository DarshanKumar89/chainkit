//! # Example 26: Chain Reorg Detection
//!
//! Demonstrates `ReorgDetector` — monitors block hashes via a sliding window
//! and detects chain reorganizations at the RPC layer. When a reorg is
//! detected, callbacks fire with the fork point for cache invalidation
//! and data correction.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use chainrpc_core::reorg::{ReorgDetector, ReorgConfig, ReorgEvent};

fn main() {
    println!("=== Chain Reorg Detection ===\n");

    // =====================================================================
    // 1. Basic Setup
    // =====================================================================
    println!("--- Setup ---\n");

    let detector = ReorgDetector::new(ReorgConfig {
        window_size: 128,   // Track last 128 blocks
        safe_depth: 64,     // Blocks 64+ deep are "safe"
        use_finalized_tag: true,
    });

    println!("ReorgDetector created:");
    println!("  window_size: 128 blocks");
    println!("  safe_depth: 64 blocks");
    println!("  use_finalized_tag: true");

    // =====================================================================
    // 2. Register Callbacks
    // =====================================================================
    // Callbacks fire when a reorg is detected
    println!("\n--- Register Callbacks ---\n");

    let reorg_count = Arc::new(AtomicU64::new(0));
    let counter = reorg_count.clone();
    detector.on_reorg(move |event: &ReorgEvent| {
        println!("REORG DETECTED!");
        println!("  Fork at block: {}", event.fork_block);
        println!("  Depth: {} blocks", event.depth);
        println!("  Old hash: {}", event.old_hash);
        println!("  New hash: {}", event.new_hash);
        counter.fetch_add(1, Ordering::Relaxed);
    });

    println!("Reorg callback registered.");

    // =====================================================================
    // 3. Normal Block Progression
    // =====================================================================
    // Feed blocks as they arrive (normally from a block subscription)
    println!("\n--- Normal Block Progression ---\n");

    detector.check_block(100, "0xaaa100");
    detector.check_block(101, "0xaaa101");
    detector.check_block(102, "0xaaa102");
    detector.check_block(103, "0xaaa103");
    println!("Fed blocks 100-103 (all consistent)");
    println!("Window size: {}", detector.window_size()); // 4

    // No reorg — all consistent
    assert_eq!(reorg_count.load(Ordering::Relaxed), 0);
    println!("Reorg count: 0 (as expected)");

    // =====================================================================
    // 4. Reorg Happens
    // =====================================================================
    // Block 102 gets a different hash (chain reorganized)
    println!("\n--- Reorg Happens ---\n");

    let event = detector.check_block(102, "0xbbb102_new");
    assert!(event.is_some());
    println!("Reorg event: {:?}", event.unwrap());
    assert_eq!(reorg_count.load(Ordering::Relaxed), 1);

    // =====================================================================
    // 5. Safe Block Tracking
    // =====================================================================
    // After processing block 200:
    println!("\n--- Safe Block Tracking ---\n");

    for i in 104..=200 {
        detector.check_block(i, &format!("0xblock{i}"));
    }

    if let Some(safe) = detector.safe_block() {
        println!("Safe block: {} (tip - {} depth)", safe, 64);
        println!("Is block 100 safe? {}", detector.is_block_safe(100)); // true
        println!("Is block 190 safe? {}", detector.is_block_safe(190)); // false
    }

    // =====================================================================
    // 6. Reorg History
    // =====================================================================
    println!("\n--- Reorg History ---\n");

    let history = detector.reorg_history();
    println!("Total reorgs detected: {}", history.len());
    for event in &history {
        println!("  Block {} -> depth {}", event.fork_block, event.depth);
    }

    // =====================================================================
    // 7. In Production: Continuous Monitoring
    // =====================================================================
    // Use poll_and_check() with a live transport:
    //
    //   loop {
    //       match detector.poll_and_check(&transport).await? {
    //           Some(event) => {
    //               // Reorg detected! Invalidate caches:
    //               cache.invalidate_for_reorg(event.fork_block);
    //               // Notify indexers to re-process from fork point
    //               indexer.rollback_to(event.fork_block).await;
    //           }
    //           None => { /* no reorg */ }
    //       }
    //       tokio::time::sleep(Duration::from_secs(12)).await; // ~1 block
    //   }
    //
    //   // Or fetch the finalized block for maximum safety:
    //   let finalized = ReorgDetector::fetch_finalized_block(&transport).await?;
    //   println!("Finalized: {finalized}");

    println!("\n--- Production Usage ---\n");
    println!("Use poll_and_check() in a loop for continuous reorg monitoring.");
    println!("On reorg: invalidate caches and rollback indexers to fork point.");
    println!("Use fetch_finalized_block() for maximum confirmation safety.");

    println!("\nDone.");
}
