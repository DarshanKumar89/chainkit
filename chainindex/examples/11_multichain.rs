//! Example 11: Multi-Chain Indexer
//!
//! Demonstrates coordinating multiple indexer instances across different chains.
//!
//! Run: `cargo run --example 11_multichain`

use std::time::Duration;

use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::{IndexerConfig, IndexerState};
use chainindex_core::multichain::*;
use chainindex_core::types::EventFilter;

#[tokio::main]
async fn main() {
    println!("=== Multi-Chain Indexer Demo ===\n");

    // 1. Configure multi-chain coordinator
    let config = MultiChainConfig {
        chains: vec![
            IndexerConfig {
                id: "eth-usdc-transfers".into(),
                chain: "ethereum".into(),
                from_block: 19_000_000,
                confirmation_depth: 12,
                batch_size: 500,
                filter: EventFilter::address("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
                ..Default::default()
            },
            IndexerConfig {
                id: "polygon-usdc-transfers".into(),
                chain: "polygon".into(),
                from_block: 50_000_000,
                confirmation_depth: 128,
                batch_size: 1000,
                filter: EventFilter::address("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"),
                ..Default::default()
            },
            IndexerConfig {
                id: "arb-usdc-transfers".into(),
                chain: "arbitrum".into(),
                from_block: 150_000_000,
                confirmation_depth: 1,
                batch_size: 2000,
                filter: EventFilter::address("0xaf88d065e77c8cC2239327C5EDb3A432268e5831"),
                ..Default::default()
            },
        ],
        max_concurrent_chains: 3,
        health_check_interval: Duration::from_secs(30),
        restart_on_error: true,
        restart_delay: Duration::from_secs(5),
    };

    println!(
        "Configured {} chains, max_concurrent={}",
        config.chains.len(),
        config.max_concurrent_chains
    );

    // 2. Create coordinator
    let coordinator = MultiChainCoordinator::new(config);

    // List all chains
    let chains = coordinator.chains().await;
    println!("\nRegistered chains:");
    for chain_id in &chains {
        println!("  - {}", chain_id);
    }

    // 3. Simulate state transitions
    println!("\n--- State Transitions ---");
    coordinator
        .update_state("eth-usdc-transfers", IndexerState::Backfilling, None)
        .await
        .unwrap();
    coordinator
        .update_state("polygon-usdc-transfers", IndexerState::Live, None)
        .await
        .unwrap();
    coordinator
        .update_state("arb-usdc-transfers", IndexerState::Live, None)
        .await
        .unwrap();

    // Record some blocks
    coordinator
        .record_block("eth-usdc-transfers", 19_000_500, 1000)
        .await
        .unwrap();
    coordinator
        .record_block("polygon-usdc-transfers", 50_001_000, 5000)
        .await
        .unwrap();
    coordinator
        .record_block("arb-usdc-transfers", 150_010_000, 2000)
        .await
        .unwrap();

    // 4. Health check
    println!("\n--- Health Report ---");
    let health = coordinator.health().await;
    for h in &health {
        println!(
            "  {:<30} state={:<15} head={:<12} events={:<6} healthy={}",
            h.chain,
            format!("{:?}", h.state),
            h.head_block,
            h.events_processed,
            h.is_healthy
        );
    }

    println!("\nAll healthy: {}", coordinator.is_all_healthy().await);

    // 5. Cross-chain event bus
    println!("\n--- Cross-Chain Event Bus ---");
    let bus = CrossChainEventBus::new(1000);
    let mut receiver = bus.subscribe();

    // Push events from different chains
    bus.push(
        "ethereum",
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xUSDC".into(),
            tx_hash: "0xeth_tx".into(),
            block_number: 19_000_500,
            log_index: 0,
            fields_json: serde_json::json!({"value": "1000000"}),
        },
    );

    bus.push(
        "polygon",
        DecodedEvent {
            chain: "polygon".into(),
            schema: "Transfer".into(),
            address: "0xUSDC".into(),
            tx_hash: "0xpoly_tx".into(),
            block_number: 50_001_000,
            log_index: 0,
            fields_json: serde_json::json!({"value": "2000000"}),
        },
    );

    // Receive events
    while let Ok(event) = receiver.try_recv() {
        println!(
            "  Received: {} on {} (block {})",
            event.event.schema, event.chain, event.event.block_number
        );
    }

    // 6. Chain sync status
    println!("\n--- Cross-Chain Sync Status ---");
    let mut sync = ChainSyncStatus::new();
    sync.update("ethereum", 19_000_500);
    sync.update("polygon", 50_001_000);
    sync.update("arbitrum", 150_010_000);
    sync.update_tip("ethereum", 19_001_000);
    sync.update_tip("polygon", 50_001_500);
    sync.update_tip("arbitrum", 150_010_100);

    for chain in &["ethereum", "polygon", "arbitrum"] {
        println!(
            "  {:<12} head={:<12} lag={} blocks",
            chain,
            sync.head_of(chain).unwrap_or(0),
            sync.lag_of(chain).unwrap_or(0)
        );
    }

    println!(
        "\nAll caught up (within 1000 blocks): {}",
        sync.all_caught_up(1000)
    );

    // 7. Pause/resume
    println!("\n--- Pause/Resume ---");
    coordinator.pause_chain("eth-usdc-transfers").await.unwrap();
    let active = coordinator.active_chains().await;
    println!("Active chains after pause: {:?}", active);

    coordinator
        .resume_chain("eth-usdc-transfers")
        .await
        .unwrap();
    let active = coordinator.active_chains().await;
    println!("Active chains after resume: {:?}", active);

    println!("\nMulti-chain indexer demo complete!");
}
