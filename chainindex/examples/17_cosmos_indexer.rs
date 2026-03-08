//! Example 17: Cosmos Indexer
//!
//! Demonstrates Cosmos-specific indexing: event parsing, IBC tracking,
//! module filtering, and the Cosmos indexer builder.
//!
//! Run: `cargo run --example 17_cosmos_indexer`

use chainindex_cosmos::*;

fn main() {
    println!("=== Cosmos Indexer Demo ===\n");

    // 1. Build Cosmos indexer config
    let builder = CosmosIndexerBuilder::new()
        .id("cosmoshub-transfers")
        .chain("cosmoshub")
        .from_height(18_500_000)
        .event_type("transfer")
        .module("bank")
        .batch_size(50)
        .poll_interval_ms(6000);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:           {}", config.id);
    println!("  Chain:        {}", config.chain);
    println!("  From height:  {}", config.from_block);
    println!("  Batch size:   {}", config.batch_size);
    println!("  Poll interval: {}ms", config.poll_interval_ms);
    println!("\nEvent filter:");
    println!("  Event types:  {:?}", filter.event_types);
    println!("  Modules:      {:?}", filter.modules);
    println!("  IBC only:     {}", filter.ibc_only);

    // 2. Simulate a Cosmos block
    let block = CosmosBlock {
        height: 18_500_001,
        hash: "ABCDEF1234567890".into(),
        parent_hash: "FEDCBA0987654321".into(),
        timestamp: 1700000012,
        tx_count: 15,
        proposer: "cosmosvalcons1abc123".into(),
        chain_id: "cosmoshub-4".into(),
    };

    let summary = block.to_block_summary();
    println!("\nBlock {} summary:", block.height);
    println!("  Hash:       {}", summary.hash);
    println!("  Parent:     {}", summary.parent_hash);
    println!("  Timestamp:  {}", summary.timestamp);
    println!("  Tx count:   {}", summary.tx_count);
    println!("  Proposer:   {}", block.proposer);
    println!("  Chain ID:   {}", block.chain_id);

    // 3. Simulate Cosmos events
    let transfer_event = CosmosEvent {
        event_type: "transfer".into(),
        attributes: vec![
            EventAttribute { key: "sender".into(), value: "cosmos1abc...".into(), index: true },
            EventAttribute { key: "recipient".into(), value: "cosmos1def...".into(), index: true },
            EventAttribute { key: "amount".into(), value: "1000000uatom".into(), index: false },
            EventAttribute { key: "module".into(), value: "bank".into(), index: false },
        ],
        tx_hash: "TX_HASH_ABC123".into(),
        height: 18_500_001,
        msg_index: 0,
    };

    println!("\nTransfer event:");
    println!("  Sender:    {}", transfer_event.attribute("sender").unwrap());
    println!("  Recipient: {}", transfer_event.attribute("recipient").unwrap());
    println!("  Amount:    {}", transfer_event.attribute("amount").unwrap());
    println!("  Is IBC:    {}", transfer_event.is_ibc());

    // Filter matching
    println!("\n  Matches filter: {}", filter.matches(&transfer_event));

    // Convert to DecodedEvent
    let decoded = transfer_event.to_decoded_event("cosmoshub");
    println!("\nDecoded event:");
    println!("  Chain:     {}", decoded.chain);
    println!("  Schema:    {}", decoded.schema);
    println!("  Address:   {}", decoded.address);
    println!("  Fields:    {}", decoded.fields_json);

    // 4. IBC event example
    let ibc_event = CosmosEvent {
        event_type: "send_packet".into(),
        attributes: vec![
            EventAttribute { key: "packet_src_port".into(), value: "transfer".into(), index: false },
            EventAttribute { key: "packet_src_channel".into(), value: "channel-0".into(), index: false },
            EventAttribute { key: "packet_dst_port".into(), value: "transfer".into(), index: false },
            EventAttribute { key: "packet_dst_channel".into(), value: "channel-141".into(), index: false },
            EventAttribute { key: "packet_sequence".into(), value: "12345".into(), index: false },
            EventAttribute { key: "packet_data".into(), value: r#"{"amount":"1000000","denom":"uatom"}"#.into(), index: false },
        ],
        tx_hash: "TX_IBC_456".into(),
        height: 18_500_001,
        msg_index: 1,
    };

    println!("\nIBC event:");
    println!("  Is IBC: {}", ibc_event.is_ibc());

    if let Some(packet) = extract_ibc_packet(&ibc_event) {
        println!("  Source:  {}:{}", packet.source_port, packet.source_channel);
        println!("  Dest:    {}:{}", packet.dest_port, packet.dest_channel);
        println!("  Seq:     {}", packet.sequence);
    }

    // 5. Parse block results JSON
    let block_results_json = serde_json::json!({
        "txs_results": [{
            "events": [
                {
                    "type": "transfer",
                    "attributes": [
                        { "key": "sender", "value": "cosmos1abc", "index": true },
                        { "key": "recipient", "value": "cosmos1def", "index": true },
                        { "key": "amount", "value": "500uatom", "index": false }
                    ]
                }
            ]
        }]
    });

    let parsed_events = CosmosEventParser::parse_block_results(&block_results_json, 18_500_001);
    println!("\nParsed {} events from block results", parsed_events.len());

    // 6. Known IBC event types
    println!("\nKnown IBC event types ({}):", ibc_event_types().len());
    for et in &ibc_event_types()[..5] {
        println!("  {et}");
    }
    println!("  ...");

    println!("\nDone.");
}
