//! Example 09: GraphQL Query Layer
//!
//! Demonstrates the built-in GraphQL schema generation and query execution
//! over the entity store.
//!
//! Run: `cargo run --example 09_graphql`

use std::sync::Arc;

use chainindex_core::entity::*;
use chainindex_core::graphql::{GraphqlExecutor, GraphqlSchema};

#[tokio::main]
async fn main() {
    println!("=== GraphQL Query Layer Demo ===\n");

    // 1. Define entity schemas
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
        .build();

    // 2. Generate GraphQL SDL
    let mut gql_schema = GraphqlSchema::new();
    gql_schema.add_entity(swap_schema.clone());
    gql_schema.add_entity(pool_schema.clone());

    println!("--- Generated GraphQL Schema ---");
    let sdl = gql_schema.sdl();
    // Print first 30 lines
    for line in sdl.lines().take(30) {
        println!("  {}", line);
    }
    println!("  ...\n");

    // 3. Create entity store and populate
    let store = Arc::new(MemoryEntityStore::new());
    store.register_schema(&swap_schema).await.unwrap();
    store.register_schema(&pool_schema).await.unwrap();

    // Insert pool
    store
        .insert(EntityRow {
            id: "0xPool1".into(),
            entity_type: "pool".into(),
            block_number: 100,
            tx_hash: "0xtx0".into(),
            log_index: 0,
            data: [
                ("token0".into(), serde_json::json!("USDC")),
                ("token1".into(), serde_json::json!("WETH")),
                ("fee".into(), serde_json::json!(3000)),
            ]
            .into_iter()
            .collect(),
        })
        .await
        .unwrap();

    // Insert swaps
    for i in 0..5 {
        store
            .insert(EntityRow {
                id: format!("swap-{}", i),
                entity_type: "swap".into(),
                block_number: 100 + i as u64,
                tx_hash: format!("0xtx{}", i),
                log_index: 0,
                data: [
                    ("pool".into(), serde_json::json!("0xPool1")),
                    ("amount0".into(), serde_json::json!(1000 * (i + 1))),
                    ("amount1".into(), serde_json::json!(-500i64 * (i as i64 + 1))),
                    ("sender".into(), serde_json::json!(format!("0xUser{}", i))),
                ]
                .into_iter()
                .collect(),
            })
            .await
            .unwrap();
    }
    println!("Populated store: 1 pool + 5 swaps\n");

    // 4. Create GraphQL executor
    let executor = GraphqlExecutor::new(store);
    executor.register_schema(swap_schema);
    executor.register_schema(pool_schema);

    // 5. Query: Get single pool by ID
    println!("--- Query: pool(id: \"0xPool1\") ---");
    let result = executor
        .execute(r#"{ pool(id: "0xPool1") { id token0 token1 fee blockNumber } }"#)
        .await;
    println!("{}\n", serde_json::to_string_pretty(&result).unwrap());

    // 6. Query: Get all swaps
    println!("--- Query: swaps(first: 3) ---");
    let result = executor
        .execute(r#"{ swaps(first: 3) { id pool amount0 sender blockNumber } }"#)
        .await;
    println!("{}\n", serde_json::to_string_pretty(&result).unwrap());

    // 7. Query: Filter swaps by pool
    println!("--- Query: swaps(where: {{ pool: \"0xPool1\" }}) ---");
    let result = executor
        .execute(r#"{ swaps(where: { pool: "0xPool1" }, first: 2, orderBy: "amount0", orderDirection: "desc") { id amount0 sender } }"#)
        .await;
    println!("{}\n", serde_json::to_string_pretty(&result).unwrap());

    // 8. Introspection
    println!("--- Introspection ---");
    let sdl = executor.introspect();
    println!("GraphQL SDL ({} chars):", sdl.len());
    for line in sdl.lines().take(10) {
        println!("  {}", line);
    }
    println!("  ...");

    println!("\nGraphQL query layer demo complete!");
}
