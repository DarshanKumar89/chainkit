//! Example 04: Entity System
//!
//! Demonstrates structured storage with typed schemas, CRUD operations,
//! query filters, sorting, pagination, and reorg rollback.
//!
//! Run: `cargo run --example 04_entity_system`

use chainindex_core::entity::*;

#[tokio::main]
async fn main() {
    println!("=== Entity System Demo ===\n");

    // 1. Define schemas
    let swap_schema = EntitySchemaBuilder::new("swap")
        .primary_key("id")
        .field("pool", FieldType::String, true)
        .field("amount0", FieldType::Int64, false)
        .field("amount1", FieldType::Int64, false)
        .field("sender", FieldType::String, true)
        .build();

    let pool_schema = EntitySchemaBuilder::new("pool")
        .primary_key("id")
        .field("token0", FieldType::String, true)
        .field("token1", FieldType::String, true)
        .field("fee", FieldType::Uint64, false)
        .field("tvl", FieldType::Float64, false)
        .build();

    println!("Schemas defined:");
    println!("  swap: {} fields", swap_schema.fields.len());
    println!("  pool: {} fields", pool_schema.fields.len());

    // 2. Create in-memory store and register schemas
    let store = MemoryEntityStore::new();
    store.register_schema(&swap_schema).await.unwrap();
    store.register_schema(&pool_schema).await.unwrap();

    // 3. Insert entities
    let swaps = vec![
        EntityRow {
            id: "0xtx1-0".into(),
            entity_type: "swap".into(),
            block_number: 19_000_100,
            tx_hash: "0xtx1".into(),
            log_index: 0,
            data: [
                ("pool".to_string(), serde_json::json!("0xPool1")),
                ("amount0".to_string(), serde_json::json!(1000)),
                ("amount1".to_string(), serde_json::json!(-500)),
                ("sender".to_string(), serde_json::json!("0xAlice")),
            ]
            .into_iter()
            .collect(),
        },
        EntityRow {
            id: "0xtx2-0".into(),
            entity_type: "swap".into(),
            block_number: 19_000_200,
            tx_hash: "0xtx2".into(),
            log_index: 0,
            data: [
                ("pool".to_string(), serde_json::json!("0xPool1")),
                ("amount0".to_string(), serde_json::json!(2000)),
                ("amount1".to_string(), serde_json::json!(-1000)),
                ("sender".to_string(), serde_json::json!("0xBob")),
            ]
            .into_iter()
            .collect(),
        },
        EntityRow {
            id: "0xtx3-0".into(),
            entity_type: "swap".into(),
            block_number: 19_000_300,
            tx_hash: "0xtx3".into(),
            log_index: 0,
            data: [
                ("pool".to_string(), serde_json::json!("0xPool2")),
                ("amount0".to_string(), serde_json::json!(500)),
                ("amount1".to_string(), serde_json::json!(-250)),
                ("sender".to_string(), serde_json::json!("0xAlice")),
            ]
            .into_iter()
            .collect(),
        },
    ];

    for swap in &swaps {
        store.insert(swap.clone()).await.unwrap();
    }
    println!("\nInserted {} swap entities", swaps.len());

    // 4. Query with filters
    println!("\n--- Query: swaps from 0xPool1 ---");
    let query = EntityQuery::new("swap")
        .filter(QueryFilter::Eq(
            "pool".into(),
            serde_json::json!("0xPool1"),
        ))
        .order_by("block_number", SortOrder::Desc)
        .limit(10);

    let results = store.query(query).await.unwrap();
    for row in &results {
        println!(
            "  {} at block {} — amount0: {}",
            row.id,
            row.block_number,
            row.data.get("amount0").unwrap_or(&serde_json::Value::Null)
        );
    }

    // 5. Count
    let total = store.count("swap").await.unwrap();
    println!("\nTotal swaps: {}", total);

    // 6. Reorg rollback — delete entities after block 19_000_200
    println!("\n--- Reorg Rollback (delete after block 19_000_200) ---");
    let deleted = store.delete_after_block("swap", 19_000_200).await.unwrap();
    println!("Deleted {} entities", deleted);

    let remaining = store.count("swap").await.unwrap();
    println!("Remaining swaps: {}", remaining);

    // 7. Upsert
    println!("\n--- Upsert Demo ---");
    let updated_swap = EntityRow {
        id: "0xtx1-0".into(),
        entity_type: "swap".into(),
        block_number: 19_000_100,
        tx_hash: "0xtx1".into(),
        log_index: 0,
        data: [
            ("pool".to_string(), serde_json::json!("0xPool1")),
            ("amount0".to_string(), serde_json::json!(1500)),
            ("amount1".to_string(), serde_json::json!(-750)),
            ("sender".to_string(), serde_json::json!("0xAlice")),
        ]
        .into_iter()
        .collect(),
    };

    store.upsert(updated_swap).await.unwrap();
    let q = EntityQuery::new("swap").filter(QueryFilter::Eq(
        "id".into(),
        serde_json::json!("0xtx1-0"),
    ));
    let after = store.query(q).await.unwrap();
    println!(
        "After upsert: amount0 = {}",
        after[0]
            .data
            .get("amount0")
            .unwrap_or(&serde_json::Value::Null)
    );

    println!("\nEntity system demo complete!");
}
