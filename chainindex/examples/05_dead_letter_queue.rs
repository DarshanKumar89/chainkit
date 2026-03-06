//! Example 05: Dead Letter Queue
//!
//! Demonstrates failed handler retry with exponential backoff.
//!
//! Run: `cargo run --example 05_dead_letter_queue`

use std::time::Duration;

use chainindex_core::dlq::{DeadLetterQueue, DlqConfig, DlqStatus};
use chainindex_core::handler::DecodedEvent;

fn main() {
    println!("=== Dead Letter Queue Demo ===\n");

    // 1. Configure DLQ
    let config = DlqConfig {
        max_retries: 5,
        initial_backoff: Duration::from_secs(1),
        max_backoff: Duration::from_secs(300),
        backoff_multiplier: 2.0,
    };

    let dlq = DeadLetterQueue::new(config);
    println!("DLQ config:");
    println!("  Max retries:     5");
    println!("  Initial backoff: 1s");
    println!("  Max backoff:     300s");
    println!("  Multiplier:      2.0x");

    // 2. Simulate handler failures
    let events = vec![
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken1".into(),
            tx_hash: "0xtx1".into(),
            block_number: 19_000_100,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 100}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Swap".into(),
            address: "0xPool1".into(),
            tx_hash: "0xtx2".into(),
            block_number: 19_000_101,
            log_index: 0,
            fields_json: serde_json::json!({"amount0": 1000, "amount1": -500}),
        },
    ];

    // Push failed events
    for event in &events {
        dlq.push(event.clone(), "my_handler", "connection timeout");
        println!(
            "Pushed to DLQ: {} at block {} (handler: my_handler)",
            event.schema, event.block_number
        );
    }

    // 3. Check stats
    let stats = dlq.stats();
    println!("\nDLQ stats:");
    println!("  Total added:      {}", stats.total_added);
    println!("  Pending:          {}", stats.pending);
    println!("  Retried success:  {}", stats.retried_success);
    println!("  Failed:           {}", stats.failed);

    // 4. Simulate retry cycle
    println!("\n--- Retry Cycle ---");
    let now = chrono::Utc::now().timestamp();
    let future = now + 3600; // 1 hour in the future to get past backoff

    let ready = dlq.pop_ready(future);
    println!("Ready entries: {}", ready.len());

    for entry in &ready {
        println!(
            "  Retrying: {} (attempt {}/{}, handler: {})",
            entry.event.schema, entry.attempt_count, entry.max_attempts, entry.handler_name
        );
        // Simulate success on first entry, failure on second
        if entry.event.schema == "Transfer" {
            dlq.mark_success(&entry.id);
            println!("    -> SUCCESS");
        } else {
            dlq.mark_failed(&entry.id, "still failing");
            println!("    -> FAILED (will retry later)");
        }
    }

    // 5. Check status breakdown
    println!("\n--- Status Breakdown ---");
    let pending = dlq.get_by_status(DlqStatus::Pending);
    let retrying = dlq.get_by_status(DlqStatus::Retrying);
    let failed = dlq.get_by_status(DlqStatus::Failed);
    println!("  Pending:  {}", pending.len());
    println!("  Retrying: {}", retrying.len());
    println!("  Failed:   {}", failed.len());

    // 6. Final stats
    let stats = dlq.stats();
    println!("\nFinal stats:");
    println!("  Total added:      {}", stats.total_added);
    println!("  Pending:          {}", stats.pending);
    println!("  Retried success:  {}", stats.retried_success);

    println!("\nDead letter queue demo complete!");
}
