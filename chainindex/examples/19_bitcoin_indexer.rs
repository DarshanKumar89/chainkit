//! Example 19: Bitcoin Indexer
//!
//! Demonstrates Bitcoin-specific indexing: UTXO tracking, address monitoring,
//! transaction parsing, and the Bitcoin indexer builder.
//!
//! Run: `cargo run --example 19_bitcoin_indexer`

use chainindex_bitcoin::*;

fn main() {
    println!("=== Bitcoin Indexer Demo ===\n");

    // 1. Build Bitcoin indexer config
    let builder = BitcoinIndexerBuilder::new()
        .id("whale-tracker")
        .from_height(830_000)
        .address("bc1qexample_whale_address")
        .min_value(100_000_000) // 1 BTC minimum
        .include_coinbase(false)
        .batch_size(5)
        .confirmation_depth(6);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:              {}", config.id);
    println!("  Chain:           {}", config.chain);
    println!("  From height:     {}", config.from_block);
    println!("  Confirmation:    {}", config.confirmation_depth);
    println!("  Batch size:      {}", config.batch_size);
    println!("  Poll interval:   {}ms", config.poll_interval_ms);
    println!("\nAddress filter:");
    println!("  Addresses:       {:?}", filter.addresses);
    println!("  Min value:       {:?} sats", filter.min_value);
    println!("  Include coinbase: {}", filter.include_coinbase);

    // 2. Simulate a Bitcoin block
    let block = BitcoinBlock {
        height: 830_001,
        hash: "00000000000000000002a1b3c4d5e6f7".into(),
        parent_hash: "00000000000000000001f7e6d5c4b3a2".into(),
        timestamp: 1700000600,
        tx_count: 2500,
        merkle_root: "merkle_root_hash_abc123".into(),
        bits: "17034219".into(),
        nonce: 123456789,
        size: 1_500_000,
        weight: 3_993_456,
    };

    let summary = block.to_block_summary();
    println!("\nBlock {} summary:", block.height);
    println!("  Hash:        {}", summary.hash);
    println!("  Tx count:    {}", summary.tx_count);
    println!("  Size:        {} bytes", block.size);
    println!("  Weight:      {} WU", block.weight);

    // 3. Simulate a transaction
    let tx = BitcoinTransaction {
        txid: "abc123def456789".into(),
        block_height: 830_001,
        version: 2,
        inputs: vec![BitcoinInput {
            prev_txid: "prev_tx_hash_1".into(),
            prev_vout: 0,
            script_sig: "".into(),
            witness: vec!["witness_data".into()],
            sequence: 0xFFFFFFFF,
            address: Some("bc1qsender_address".into()),
            value: Some(200_000_000), // 2 BTC
        }],
        outputs: vec![
            BitcoinOutput {
                vout: 0,
                value: 150_000_000, // 1.5 BTC to recipient
                script_pubkey: "0014abc".into(),
                script_type: "witness_v0_keyhash".into(),
                address: Some("bc1qexample_whale_address".into()),
            },
            BitcoinOutput {
                vout: 1,
                value: 49_990_000, // Change
                script_pubkey: "0014def".into(),
                script_type: "witness_v0_keyhash".into(),
                address: Some("bc1qchange_address".into()),
            },
        ],
        locktime: 0,
        is_coinbase: false,
        total_input: 200_000_000,
        total_output: 199_990_000,
        fee: 10_000,
    };

    println!("\nTransaction {}:", tx.txid);
    println!("  Inputs:     {}", tx.inputs.len());
    println!("  Outputs:    {}", tx.outputs.len());
    println!("  Total in:   {} sats ({:.8} BTC)", tx.total_input, tx.total_input as f64 / 1e8);
    println!("  Total out:  {} sats ({:.8} BTC)", tx.total_output, tx.total_output as f64 / 1e8);
    println!("  Fee:        {} sats", tx.fee);
    println!("  Is coinbase: {}", tx.is_coinbase);

    // Filter matching
    println!("\n  Matches filter: {}", filter.matches_transaction(&tx));

    // 4. Convert to DecodedEvent
    let decoded = BitcoinEventDecoder::tx_to_decoded_event(&tx, "bitcoin");
    println!("\nDecoded event:");
    println!("  Schema:    {}", decoded.schema);
    println!("  Chain:     {}", decoded.chain);
    println!("  Fields:    {}", decoded.fields_json);

    // 5. Output-level decoded events
    for output in &tx.outputs {
        let output_event = BitcoinEventDecoder::output_to_decoded_event(output, &tx, "bitcoin");
        println!("\n  UTXO created:");
        println!("    Address: {}", output.address.as_deref().unwrap_or("unknown"));
        println!("    Value:   {} sats", output.value);
        println!("    Matches: {}", filter.matches_output(output));
    }

    // 6. Parse block from JSON
    let block_json = serde_json::json!({
        "height": 830001,
        "hash": "00000000000000000002abc",
        "previousblockhash": "00000000000000000001def",
        "time": 1700000600,
        "nTx": 2500,
        "merkleroot": "merkle_root",
        "bits": "17034219",
        "nonce": 123456789,
        "size": 1500000,
        "weight": 3993456
    });

    let parsed_block = BitcoinBlockParser::parse_block(&block_json).unwrap();
    println!("\nParsed block from JSON:");
    println!("  Height: {}, Txs: {}", parsed_block.height, parsed_block.tx_count);

    println!("\nDone.");
}
