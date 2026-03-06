//! Example 15: Idempotency & Replay Safety
//!
//! Demonstrates deterministic entity IDs, replay context detection,
//! and side-effect guards for safe reorg replay.
//!
//! Run: `cargo run --example 15_idempotency`

use chainindex_core::handler::DecodedEvent;
use chainindex_core::idempotency::*;

fn main() {
    println!("=== Idempotency & Replay Safety Demo ===\n");

    // 1. Deterministic entity IDs
    println!("--- Deterministic IDs ---");
    let event = DecodedEvent {
        chain: "ethereum".into(),
        schema: "Transfer".into(),
        address: "0xToken".into(),
        tx_hash: "0xabc123def456789".into(),
        block_number: 19_000_100,
        log_index: 3,
        fields_json: serde_json::json!({"from": "0xA", "to": "0xB", "value": 1000}),
    };

    let id = deterministic_id(&event);
    println!("Event: tx={} log_index={}", event.tx_hash, event.log_index);
    println!("Deterministic ID: {}", id);

    // Same event always produces same ID
    let id2 = deterministic_id(&event);
    assert_eq!(id, id2);
    println!("Same event → same ID: {}", id == id2);

    // With suffix for multiple entities from one event
    let id_pool = deterministic_id_with_suffix(&event, "pool");
    let id_token = deterministic_id_with_suffix(&event, "token");
    println!("\nWith suffix:");
    println!("  pool entity:  {}", id_pool);
    println!("  token entity: {}", id_token);
    assert_ne!(id_pool, id_token);
    println!("  Different suffixes → different IDs");

    // 2. Replay context
    println!("\n--- Replay Context ---");
    let normal_ctx = ReplayContext::normal();
    println!("Normal context:");
    println!("  is_replay: {}", normal_ctx.is_replay);
    println!("  reorg_from_block: {:?}", normal_ctx.reorg_from_block);

    let replay_ctx = ReplayContext::replay(19_000_090, Some("0xoriginal_hash".into()));
    println!("\nReplay context (after reorg at block 19_000_090):");
    println!("  is_replay: {}", replay_ctx.is_replay);
    println!("  reorg_from_block: {:?}", replay_ctx.reorg_from_block);
    println!(
        "  original_block_hash: {:?}",
        replay_ctx.original_block_hash
    );

    // 3. Side-effect guard
    println!("\n--- Side-Effect Guard ---");

    // During normal processing
    let guard = SideEffectGuard::new(&normal_ctx);
    println!("Normal mode:");
    println!("  should_execute(): {}", guard.should_execute());
    println!("  → Send webhook: YES");
    println!("  → Update external DB: YES");

    // During reorg replay
    let replay_guard = SideEffectGuard::new(&replay_ctx);
    println!("\nReplay mode (after reorg):");
    println!("  should_execute(): {}", replay_guard.should_execute());
    println!("  → Send webhook: SKIP (already sent)");
    println!("  → Update external DB: SKIP (will be re-done by entity upsert)");

    // 4. Practical usage pattern
    println!("\n--- Practical Pattern ---");
    println!("In your event handler:");
    println!("  1. Generate ID: deterministic_id(event)");
    println!("  2. Check replay: ctx.is_replay");
    println!("  3. Upsert entity: store.upsert(entity_with_deterministic_id)");
    println!("  4. Side effects: if guard.should_execute() {{ send_webhook() }}");
    println!("  5. On reorg: entities auto-rollback via delete_after_block()");
    println!("  6. On replay: same IDs = same entities overwritten (idempotent)");

    println!("\nIdempotency demo complete!");
}
