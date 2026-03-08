//! Example 18: Substrate (Polkadot/Kusama) Indexer
//!
//! Demonstrates Substrate-specific indexing: pallet event parsing,
//! extrinsic tracking, and the Substrate indexer builder.
//!
//! Run: `cargo run --example 18_substrate_indexer`

use chainindex_substrate::*;

fn main() {
    println!("=== Substrate Indexer Demo ===\n");

    // 1. Build Substrate indexer config
    let builder = SubstrateIndexerBuilder::new()
        .id("polkadot-balances")
        .chain("polkadot")
        .from_height(20_000_000)
        .pallet("Balances")
        .variant("Transfer")
        .exclude_system(true)
        .batch_size(100);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:           {}", config.id);
    println!("  Chain:        {}", config.chain);
    println!("  From height:  {}", config.from_block);
    println!("  Batch size:   {}", config.batch_size);
    println!("\nEvent filter:");
    println!("  Pallets:        {:?}", filter.pallets);
    println!("  Variants:       {:?}", filter.variants);
    println!("  Exclude system: {}", filter.exclude_system);

    // 2. Simulate a Substrate block
    let block = SubstrateBlock {
        height: 20_000_001,
        hash: "0xblock_hash_abc123".into(),
        parent_hash: "0xparent_hash_def456".into(),
        state_root: "0xstate_root_789".into(),
        extrinsics_root: "0xext_root_012".into(),
        timestamp: 1700000012,
        extrinsic_count: 5,
        author: Some("16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD".into()),
        spec_version: Some(1001000),
    };

    let summary = block.to_block_summary();
    println!("\nBlock {} summary:", block.height);
    println!("  Hash:            {}", summary.hash);
    println!("  Parent:          {}", summary.parent_hash);
    println!("  Extrinsic count: {}", summary.tx_count);
    println!("  Author:          {:?}", block.author);
    println!("  Spec version:    {:?}", block.spec_version);

    // 3. Simulate Substrate events
    let transfer_event = SubstrateEvent {
        pallet: "Balances".into(),
        variant: "Transfer".into(),
        fields: serde_json::json!({
            "from": "16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD",
            "to": "14ShUZUYUR35RBZW6uVVt1zXDqmvNcY81wKMpX2pTVyD5jK",
            "amount": "1000000000000"
        }),
        height: 20_000_001,
        event_index: 2,
        extrinsic_index: Some(1),
        phase: "ApplyExtrinsic".into(),
    };

    println!("\nBalances.Transfer event:");
    println!("  Full name:    {}", transfer_event.full_name());
    println!("  Extrinsic:    {:?}", transfer_event.extrinsic_index);
    println!("  Event index:  {}", transfer_event.event_index);
    println!("  Fields:       {}", transfer_event.fields);

    // Filter matching
    println!("\n  Matches filter: {}", filter.matches(&transfer_event));

    // Convert to DecodedEvent
    let decoded = transfer_event.to_decoded_event("polkadot");
    println!("\nDecoded event:");
    println!("  Chain:     {}", decoded.chain);
    println!("  Schema:    {}", decoded.schema);
    println!("  Address:   {}", decoded.address);

    // 4. System event (should be filtered out)
    let system_event = SubstrateEvent {
        pallet: "System".into(),
        variant: "ExtrinsicSuccess".into(),
        fields: serde_json::json!({
            "dispatchInfo": { "weight": { "refTime": 123456 } }
        }),
        height: 20_000_001,
        event_index: 3,
        extrinsic_index: Some(1),
        phase: "ApplyExtrinsic".into(),
    };

    println!("\nSystem.ExtrinsicSuccess:");
    println!("  Matches filter (exclude_system=true): {}", filter.matches(&system_event));

    // 5. Parse events from JSON
    let events_json = serde_json::json!([
        {
            "pallet": "Balances",
            "method": "Transfer",
            "phase": "ApplyExtrinsic",
            "extrinsicIndex": 1,
            "data": { "from": "alice", "to": "bob", "amount": "1000000000000" }
        },
        {
            "pallet": "System",
            "method": "ExtrinsicSuccess",
            "phase": "ApplyExtrinsic",
            "extrinsicIndex": 1,
            "data": {}
        }
    ]);

    let parsed = SubstrateEventParser::parse_events(&events_json, 20_000_001);
    println!("\nParsed {} events from JSON", parsed.len());
    for ev in &parsed {
        println!("  {} (matches: {})", ev.full_name(), filter.matches(ev));
    }

    println!("\nDone.");
}
