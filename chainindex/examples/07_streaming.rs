//! Example 07: Cursor-Based Event Streaming
//!
//! Demonstrates cursor-based streaming for downstream consumers.
//!
//! Run: `cargo run --example 07_streaming`

use chainindex_core::handler::DecodedEvent;
use chainindex_core::streaming::{EventStream, StreamCursor};

fn main() {
    println!("=== Event Streaming Demo ===\n");

    // 1. Create a stream with 10,000 event capacity
    let mut stream = EventStream::new(10_000);
    println!("Created event stream (capacity: 10,000)\n");

    // 2. Push some events (producer side)
    let events = vec![
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken".into(),
            tx_hash: "0xtx1".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 1000}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Swap".into(),
            address: "0xPool".into(),
            tx_hash: "0xtx2".into(),
            block_number: 101,
            log_index: 0,
            fields_json: serde_json::json!({"amount0": 500, "amount1": -250}),
        },
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xToken".into(),
            tx_hash: "0xtx3".into(),
            block_number: 102,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xC", "to": "0xD", "value": 2000}),
        },
    ];

    for event in &events {
        stream.push(event.clone());
    }
    println!("Pushed {} events to stream", events.len());

    // 3. Consumer reads from the beginning
    println!("\n--- Consumer 1: Read All ---");
    let cursor = StreamCursor::initial();
    let batch = stream.next_batch(&cursor, 10).unwrap();
    println!(
        "Batch: {} events, has_more: {}",
        batch.events.len(),
        batch.has_more
    );
    for event in &batch.events {
        println!(
            "  [Block {}] {} on {}",
            event.block_number, event.schema, event.address
        );
    }

    // 4. Save cursor and resume later
    let saved_cursor = batch.cursor.clone();
    let encoded = saved_cursor.encode();
    println!("\nSaved cursor: {}", encoded);

    // Push more events
    stream.push(DecodedEvent {
        chain: "ethereum".into(),
        schema: "Approval".into(),
        address: "0xToken".into(),
        tx_hash: "0xtx4".into(),
        block_number: 103,
        log_index: 0,
        fields_json: serde_json::json!({"owner": "0xA", "spender": "0xB", "value": 999}),
    });

    // Resume from saved cursor
    println!("\n--- Consumer 1: Resume from cursor ---");
    let resumed = StreamCursor::decode(&encoded).unwrap();
    let batch2 = stream.next_batch(&resumed, 10).unwrap();
    println!("New events since last read: {}", batch2.events.len());
    for event in &batch2.events {
        println!(
            "  [Block {}] {} on {}",
            event.block_number, event.schema, event.address
        );
    }

    // 5. Multi-consumer
    println!("\n--- Multi-Consumer Demo ---");
    stream.register_consumer("analytics");
    stream.register_consumer("notifications");
    let consumer_a = stream.get_consumer_cursor("analytics").unwrap();
    let consumer_b = stream.get_consumer_cursor("notifications").unwrap();
    println!("Consumer A cursor: block {}", consumer_a.block_number);
    println!("Consumer B cursor: block {}", consumer_b.block_number);

    // 6. Reorg invalidation
    println!("\n--- Reorg Invalidation ---");
    stream.invalidate_after(101);
    println!("Invalidated events after block 101");

    let batch3 = stream.next_batch(&StreamCursor::initial(), 100).unwrap();
    println!(
        "Events remaining after invalidation: {}",
        batch3.events.len()
    );

    println!("\nEvent streaming demo complete!");
}
