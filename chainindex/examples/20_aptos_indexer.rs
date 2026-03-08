//! Example 20: Aptos Indexer
//!
//! Demonstrates Aptos-specific indexing: Move event parsing, type tag filtering,
//! and the Aptos indexer builder.
//!
//! Run: `cargo run --example 20_aptos_indexer`

use chainindex_aptos::*;

fn main() {
    println!("=== Aptos Indexer Demo ===\n");

    // 1. Build Aptos indexer config
    let builder = AptosIndexerBuilder::new()
        .id("aptos-coin-tracker")
        .from_height(100_000_000)
        .type_prefix("0x1::coin")
        .module("coin")
        .batch_size(100)
        .poll_interval_ms(4000);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:           {}", config.id);
    println!("  Chain:        {}", config.chain);
    println!("  From height:  {}", config.from_block);
    println!("  Confirmation: {}", config.confirmation_depth);
    println!("  Batch size:   {}", config.batch_size);
    println!("\nEvent filter:");
    println!("  Type prefixes: {:?}", filter.type_prefixes);
    println!("  Modules:       {:?}", filter.modules);

    // 2. Simulate an Aptos block
    let block = AptosBlock {
        height: 100_000_001,
        hash: "0xblock_hash_abc123".into(),
        timestamp: 1700000004,
        first_version: 500_000_000,
        last_version: 500_000_050,
        tx_count: 51,
        epoch: 100,
        round: 5,
    };

    let summary = block.to_block_summary();
    println!("\nBlock {} summary:", block.height);
    println!("  Hash:          {}", summary.hash);
    println!("  Timestamp:     {}", summary.timestamp);
    println!("  Tx count:      {}", summary.tx_count);
    println!("  Epoch:         {}", block.epoch);
    println!("  Round:         {}", block.round);
    println!("  Versions:      {} - {}", block.first_version, block.last_version);

    // 3. Simulate Aptos Move events
    let deposit_event = AptosEvent {
        type_tag: "0x1::coin::DepositEvent".into(),
        sequence_number: 42,
        data: serde_json::json!({ "amount": "1000000000" }),
        version: 500_000_010,
        height: 100_000_001,
        tx_hash: "0xtx_hash_abc123".into(),
        account_address: "0x1234567890abcdef".into(),
        creation_number: 1,
    };

    println!("\nMove event:");
    println!("  Type tag:    {}", deposit_event.type_tag);
    println!("  Module:      {}", deposit_event.module_name());
    println!("  Event name:  {}", deposit_event.event_name());
    println!("  Address:     {}", deposit_event.type_address());
    println!("  Seq number:  {}", deposit_event.sequence_number);
    println!("  Data:        {}", deposit_event.data);

    // Filter matching
    println!("\n  Matches filter: {}", filter.matches(&deposit_event));

    // Convert to DecodedEvent
    let decoded = deposit_event.to_decoded_event("aptos");
    println!("\nDecoded event:");
    println!("  Chain:     {}", decoded.chain);
    println!("  Schema:    {}", decoded.schema);
    println!("  Address:   {}", decoded.address);
    println!("  Log index: {}", decoded.log_index);

    // 4. Different Move event (should not match filter)
    let staking_event = AptosEvent {
        type_tag: "0x1::staking_contract::StakeEvent".into(),
        sequence_number: 0,
        data: serde_json::json!({ "amount": "5000000000" }),
        version: 500_000_020,
        height: 100_000_001,
        tx_hash: "0xtx_hash_def456".into(),
        account_address: "0xstaker_address".into(),
        creation_number: 2,
    };

    println!("\nStaking event:");
    println!("  Module:         {}", staking_event.module_name());
    println!("  Matches filter: {}", filter.matches(&staking_event));

    // 5. Parse block from JSON
    let block_json = serde_json::json!({
        "block_height": "100000001",
        "block_hash": "0xblock_hash",
        "block_timestamp": "1700000004000000",
        "first_version": "500000000",
        "last_version": "500000050",
        "epoch": "100",
        "round": "5",
        "transactions": [{"type": "user"}, {"type": "user"}]
    });

    let parsed_block = AptosResponseParser::parse_block(&block_json).unwrap();
    println!("\nParsed block from JSON:");
    println!("  Height: {}, Epoch: {}", parsed_block.height, parsed_block.epoch);

    println!("\nDone.");
}
