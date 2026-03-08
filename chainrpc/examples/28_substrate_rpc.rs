//! # Example 28: Substrate (Polkadot/Kusama) RPC Support
//!
//! Demonstrates ChainRPC's Substrate-specific features: method safety,
//! CU cost tracking, SubstrateTransport, and SubstrateChainClient.
//!
//! Substrate chains use JSON-RPC (typically on port 9944 for WebSocket).

use chainrpc_core::substrate::{
    classify_substrate_method, is_substrate_safe_to_retry,
    SubstrateCuCostTable,
    polkadot_mainnet_endpoints, kusama_mainnet_endpoints,
};
use chainrpc_core::method_safety::MethodSafety;

#[tokio::main]
async fn main() {
    println!("=== Substrate RPC Support ===\n");

    // =====================================================================
    // 1. Method Safety Classification
    // =====================================================================
    println!("--- Method Safety Classification ---\n");

    assert_eq!(classify_substrate_method("chain_getBlock"), MethodSafety::Safe);
    assert_eq!(classify_substrate_method("state_getStorage"), MethodSafety::Safe);
    assert_eq!(classify_substrate_method("author_submitExtrinsic"), MethodSafety::Idempotent);

    println!("chain_getBlock           -> {:?}", classify_substrate_method("chain_getBlock"));
    println!("state_getStorage         -> {:?}", classify_substrate_method("state_getStorage"));
    println!("author_submitExtrinsic   -> {:?}", classify_substrate_method("author_submitExtrinsic"));

    assert!(is_substrate_safe_to_retry("chain_getHeader"));
    assert!(!is_substrate_safe_to_retry("author_submitExtrinsic"));

    // =====================================================================
    // 2. CU Cost Table
    // =====================================================================
    println!("\n--- CU Cost Table ---\n");

    let costs = SubstrateCuCostTable::defaults();
    println!("system_health costs {} CU", costs.cost_for("system_health"));       // 5
    println!("chain_getBlock costs {} CU", costs.cost_for("chain_getBlock"));     // 20
    println!("state_getMetadata costs {} CU", costs.cost_for("state_getMetadata")); // 50

    // =====================================================================
    // 3. SubstrateChainClient
    // =====================================================================
    // SubstrateChainClient implements the unified ChainClient trait:
    //
    //   let client = SubstrateChainClient::new(transport, "polkadot");
    //   let height = client.get_head_height().await?;   // chain_getHeader
    //   let block = client.get_block_by_height(height).await?;  // chain_getBlockHash + chain_getBlock
    //   assert_eq!(client.chain_family(), "substrate");

    println!("\n--- SubstrateChainClient ---\n");
    println!("Implements unified ChainClient trait.");
    println!("Maps: chain_getHeader → height, chain_getBlock → block");
    println!("Health: system_health (checks isSyncing)");

    // =====================================================================
    // 4. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Polkadot mainnet:");
    for url in polkadot_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nKusama mainnet:");
    for url in kusama_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
