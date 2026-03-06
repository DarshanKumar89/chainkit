//! Example 08: Data Export (JSONL/CSV)
//!
//! Demonstrates exporting indexed events to JSONL and CSV for analytics.
//!
//! Run: `cargo run --example 08_data_export`

use chainindex_core::export::{export_events, ExportConfig, ExportFormat};
use chainindex_core::handler::DecodedEvent;

fn main() {
    println!("=== Data Export Demo ===\n");

    // 1. Create sample events
    let events = vec![
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken1".into(),
            tx_hash: "0xtx_100".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 1000}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Approval".into(),
            address: "0xToken1".into(),
            tx_hash: "0xtx_101".into(),
            block_number: 101,
            log_index: 0,
            fields_json: serde_json::json!({"owner": "0xA", "spender": "0xRouter", "value": 999}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken2".into(),
            tx_hash: "0xtx_102".into(),
            block_number: 102,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xC", "to": "0xD", "value": 2000}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Swap".into(),
            address: "0xPool1".into(),
            tx_hash: "0xtx_103".into(),
            block_number: 103,
            log_index: 0,
            fields_json: serde_json::json!({"amount0": 500, "amount1": -250}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken1".into(),
            tx_hash: "0xtx_200".into(),
            block_number: 200,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xE", "to": "0xF", "value": 5000}),
        },
    ];

    println!("Total events: {}\n", events.len());

    // 2. Export all events as JSONL
    println!("--- JSONL Export (all events) ---");
    let mut buf = Vec::new();
    let config = ExportConfig::default(); // JSONL, no filters
    let stats = export_events(&events, &config, &mut buf).unwrap();
    println!(
        "Exported: {} events, {} bytes",
        stats.events_exported, stats.bytes_written
    );
    println!("Preview (first 200 chars):");
    let output = String::from_utf8(buf).unwrap();
    println!("  {}", &output[..output.len().min(200)]);

    // 3. Export as CSV
    println!("\n--- CSV Export (all events) ---");
    let mut buf = Vec::new();
    let config = ExportConfig {
        format: ExportFormat::Csv,
        ..Default::default()
    };
    let stats = export_events(&events, &config, &mut buf).unwrap();
    println!(
        "Exported: {} events, {} bytes",
        stats.events_exported, stats.bytes_written
    );
    let output = String::from_utf8(buf).unwrap();
    for line in output.lines().take(3) {
        println!("  {}", line);
    }
    println!("  ...");

    // 4. Export with filters
    println!("\n--- Filtered Export (Transfer only, blocks 100-150) ---");
    let mut buf = Vec::new();
    let config = ExportConfig {
        format: ExportFormat::Jsonl,
        from_block: Some(100),
        to_block: Some(150),
        schema_filter: vec!["Transfer".into()],
        ..Default::default()
    };
    let stats = export_events(&events, &config, &mut buf).unwrap();
    println!(
        "Exported: {} events, skipped: {}",
        stats.events_exported, stats.events_skipped
    );

    // 5. Export by address
    println!("\n--- Address Filter (0xToken1 only) ---");
    let mut buf = Vec::new();
    let config = ExportConfig {
        address_filter: vec!["0xToken1".into()],
        ..Default::default()
    };
    let stats = export_events(&events, &config, &mut buf).unwrap();
    println!(
        "Exported: {} events from 0xToken1, skipped: {}",
        stats.events_exported, stats.events_skipped
    );

    println!("\nData export demo complete!");
    println!("Tip: pipe JSONL to DuckDB with:");
    println!("  duckdb -c \"SELECT * FROM read_json_auto('transfers.jsonl')\"");
}
