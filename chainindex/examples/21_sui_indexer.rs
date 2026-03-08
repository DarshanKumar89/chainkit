//! Example 21: Sui Indexer
//!
//! Demonstrates Sui-specific indexing: checkpoint tracking, Move event parsing,
//! object change tracking, and the Sui indexer builder.
//!
//! Run: `cargo run --example 21_sui_indexer`

use chainindex_sui::*;

fn main() {
    println!("=== Sui Indexer Demo ===\n");

    // 1. Build Sui indexer config
    let builder = SuiIndexerBuilder::new()
        .id("sui-defi-tracker")
        .from_checkpoint(10_000_000)
        .package("0x2")
        .module("coin")
        .event_type("CoinDeposit")
        .batch_size(100)
        .poll_interval_ms(500);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:           {}", config.id);
    println!("  Chain:        {}", config.chain);
    println!("  From CP:      {}", config.from_block);
    println!("  Confirmation: {}", config.confirmation_depth);
    println!("  Poll interval: {}ms", config.poll_interval_ms);
    println!("\nEvent filter:");
    println!("  Packages:    {:?}", filter.packages);
    println!("  Modules:     {:?}", filter.modules);
    println!("  Event types: {:?}", filter.event_types);

    // 2. Simulate a Sui checkpoint
    let checkpoint = SuiCheckpoint {
        sequence_number: 10_000_001,
        digest: "checkpoint_digest_abc123".into(),
        previous_digest: Some("checkpoint_digest_prev".into()),
        timestamp: 1700000000,
        tx_count: 150,
        epoch: 500,
        total_gas_cost: 5_000_000,
        total_computation_cost: 2_500_000,
    };

    let summary = checkpoint.to_block_summary();
    println!("\nCheckpoint {} summary:", checkpoint.sequence_number);
    println!("  Digest:       {}", summary.hash);
    println!("  Prev digest:  {}", summary.parent_hash);
    println!("  Tx count:     {}", summary.tx_count);
    println!("  Epoch:        {}", checkpoint.epoch);
    println!("  Gas cost:     {}", checkpoint.total_gas_cost);

    // 3. Simulate Sui events
    let coin_event = SuiEvent {
        event_type: "0x2::coin::CoinDeposit<0x2::sui::SUI>".into(),
        package_id: "0x2".into(),
        module_name: "coin".into(),
        sender: "0xsender_address_abc".into(),
        tx_digest: "tx_digest_123".into(),
        checkpoint: 10_000_001,
        event_seq: 0,
        parsed_json: serde_json::json!({
            "amount": "1000000000",
            "coin_type": "0x2::sui::SUI"
        }),
        bcs: None,
        timestamp_ms: Some(1700000000500),
    };

    println!("\nSui event:");
    println!("  Type:        {}", coin_event.event_type);
    println!("  Struct name: {}", coin_event.struct_name());
    println!("  Package:     {}", coin_event.package_id);
    println!("  Module:      {}", coin_event.module_name);
    println!("  Sender:      {}", coin_event.sender);
    println!("  Data:        {}", coin_event.parsed_json);

    // Filter matching
    println!("\n  Matches filter: {}", filter.matches(&coin_event));

    // Convert to DecodedEvent
    let decoded = coin_event.to_decoded_event("sui");
    println!("\nDecoded event:");
    println!("  Chain:     {}", decoded.chain);
    println!("  Schema:    {}", decoded.schema);
    println!("  Address:   {}", decoded.address);

    // 4. Simulate object changes
    let changes = vec![
        SuiObjectChange {
            change_type: ObjectChangeType::Created,
            object_id: "0xnew_coin_object".into(),
            object_type: Some("0x2::coin::Coin<0x2::sui::SUI>".into()),
            version: 100,
            digest: Some("obj_digest_1".into()),
            owner: Some("0xowner_address".into()),
            tx_digest: "tx_digest_123".into(),
        },
        SuiObjectChange {
            change_type: ObjectChangeType::Mutated,
            object_id: "0xexisting_object".into(),
            object_type: Some("0x2::coin::Coin<0x2::sui::SUI>".into()),
            version: 101,
            digest: Some("obj_digest_2".into()),
            owner: Some("0xowner_address".into()),
            tx_digest: "tx_digest_123".into(),
        },
    ];

    println!("\nObject changes:");
    for change in &changes {
        println!("  {:?}: {} (v{})",
            change.change_type,
            change.object_id,
            change.version,
        );
    }

    // 5. Parse checkpoint from JSON
    let cp_json = serde_json::json!({
        "sequenceNumber": "10000001",
        "digest": "cp_digest",
        "previousDigest": "prev_cp_digest",
        "timestampMs": "1700000000000",
        "transactions": ["tx1", "tx2", "tx3"],
        "epoch": "500",
        "epochRollingGasCostSummary": {
            "computationCost": "2500000"
        }
    });

    let parsed_cp = SuiResponseParser::parse_checkpoint(&cp_json).unwrap();
    println!("\nParsed checkpoint from JSON:");
    println!("  Seq: {}, Epoch: {}, Txs: {}", parsed_cp.sequence_number, parsed_cp.epoch, parsed_cp.tx_count);

    // 6. Parse object changes from JSON
    let changes_json = serde_json::json!([
        {
            "type": "created",
            "objectId": "0xobj1",
            "objectType": "0x2::coin::Coin<0x2::sui::SUI>",
            "version": "100",
            "digest": "d1",
            "owner": { "AddressOwner": "0xowner" }
        }
    ]);

    let parsed_changes = SuiResponseParser::parse_object_changes(&changes_json, "tx_digest");
    println!("\nParsed {} object changes from JSON", parsed_changes.len());

    println!("\nDone.");
}
