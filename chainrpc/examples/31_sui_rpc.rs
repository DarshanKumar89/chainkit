//! # Example 31: Sui RPC Support
//!
//! Demonstrates ChainRPC's Sui-specific features: method safety classification,
//! CU cost tracking, SuiTransport, and SuiChainClient.
//!
//! Sui uses JSON-RPC directly and organizes data around checkpoints and objects.

use chainrpc_core::sui::{
    classify_sui_method, is_sui_safe_to_retry,
    SuiCuCostTable, SuiTransport,
    sui_mainnet_endpoints, sui_testnet_endpoints, sui_devnet_endpoints,
};
use chainrpc_core::method_safety::MethodSafety;

#[tokio::main]
async fn main() {
    println!("=== Sui RPC Support ===\n");

    // =====================================================================
    // 1. Method Safety Classification
    // =====================================================================
    println!("--- Method Safety Classification ---\n");

    assert_eq!(classify_sui_method("sui_getCheckpoint"), MethodSafety::Safe);
    assert_eq!(classify_sui_method("sui_getObject"), MethodSafety::Safe);
    assert_eq!(classify_sui_method("sui_executeTransactionBlock"), MethodSafety::Idempotent);

    println!("sui_getCheckpoint              -> {:?}", classify_sui_method("sui_getCheckpoint"));
    println!("sui_getObject                  -> {:?}", classify_sui_method("sui_getObject"));
    println!("sui_executeTransactionBlock     -> {:?}", classify_sui_method("sui_executeTransactionBlock"));

    assert!(is_sui_safe_to_retry("sui_getObject"));
    assert!(!is_sui_safe_to_retry("sui_executeTransactionBlock"));

    // =====================================================================
    // 2. CU Cost Table
    // =====================================================================
    println!("\n--- CU Cost Table ---\n");

    let costs = SuiCuCostTable::defaults();
    println!("sui_getLatestCheckpointSequenceNumber costs {} CU", costs.cost_for("sui_getLatestCheckpointSequenceNumber")); // 5
    println!("sui_getCheckpoint costs {} CU", costs.cost_for("sui_getCheckpoint")); // 20
    println!("sui_dryRunTransactionBlock costs {} CU", costs.cost_for("sui_dryRunTransactionBlock")); // 30

    // =====================================================================
    // 3. SuiChainClient
    // =====================================================================
    // SuiChainClient implements the unified ChainClient trait:
    //
    //   let client = SuiChainClient::new(transport, "mainnet");
    //   let height = client.get_head_height().await?;   // sui_getLatestCheckpointSequenceNumber
    //   let block = client.get_block_by_height(height).await?;  // sui_getCheckpoint
    //   assert_eq!(client.chain_family(), "sui");

    println!("\n--- SuiChainClient ---\n");
    println!("Implements unified ChainClient trait.");
    println!("Maps: checkpoint sequence number → height, checkpoint → block");
    println!("Sui organizes data around checkpoints and objects.");
    println!("~500ms checkpoint finality (BFT).");

    // =====================================================================
    // 4. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Sui mainnet:");
    for url in sui_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nSui testnet:");
    for url in sui_testnet_endpoints() {
        println!("  {url}");
    }

    println!("\nSui devnet:");
    for url in sui_devnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
