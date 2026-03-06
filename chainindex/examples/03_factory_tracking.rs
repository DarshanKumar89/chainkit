//! Example 03: Factory Contract Tracking
//!
//! Demonstrates dynamic address tracking for factory patterns
//! (Uniswap V3 Factory, Compound cToken Factory, etc.).
//!
//! Run: `cargo run --example 03_factory_tracking`

use chainindex_core::factory::{FactoryConfig, FactoryRegistry};
use chainindex_core::handler::DecodedEvent;

fn main() {
    println!("=== Factory Contract Tracking Demo ===\n");

    // 1. Create a factory registry
    let registry = FactoryRegistry::new();

    // 2. Register Uniswap V3 Factory
    registry.register(FactoryConfig {
        factory_address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".into(),
        creation_event_topic0: "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118"
            .into(),
        child_address_field: "pool".into(),
        name: Some("Uniswap V3 Factory".into()),
    });

    // 3. Register Compound cToken Factory
    registry.register(FactoryConfig {
        factory_address: "0x3d9819210a31b4961b30ef54be2aed79b9c9cd3b".into(),
        creation_event_topic0: "0xd8bfee8c471ee8d0dab0bbce5e9f30f1e03e0ae69e9a1e1baf7a7a7e7aa2d1a5"
            .into(),
        child_address_field: "cToken".into(),
        name: Some("Compound Comptroller".into()),
    });

    println!("Registered {} factories", registry.factory_count());

    // 4. Simulate factory events creating child contracts
    let events = vec![
        // Uniswap V3 PoolCreated event
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".into(),
            tx_hash: "0xtx001".into(),
            block_number: 19_000_100,
            log_index: 0,
            fields_json: serde_json::json!({
                "token0": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                "token1": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
                "fee": 3000,
                "pool": "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"
            }),
        },
        // Another pool
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".into(),
            tx_hash: "0xtx002".into(),
            block_number: 19_000_200,
            log_index: 0,
            fields_json: serde_json::json!({
                "token0": "0x6B175474E89094C44Da98b954EedeAC495271d0F",
                "token1": "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2",
                "fee": 500,
                "pool": "0x60594a405d53811d3BC4766596EFD80fd545A270"
            }),
        },
    ];

    for event in &events {
        if let Some(child) = registry.process_event(event) {
            println!(
                "NEW CHILD: factory {} discovered at block {} — address: {}",
                child.factory_address, child.discovered_at_block, child.address
            );
        }
    }

    // 5. Get all tracked addresses
    let all_addresses = registry.get_all_addresses();
    println!("\nAll tracked addresses ({}):", all_addresses.len());
    for addr in &all_addresses {
        println!("  {}", addr);
    }

    // 6. Snapshot for persistence
    let snapshot = registry.snapshot();
    println!(
        "\nSnapshot: {} factories, {} children tracked",
        snapshot.configs.len(),
        snapshot.children.len()
    );

    println!("\nFactory tracking demo complete!");
}
