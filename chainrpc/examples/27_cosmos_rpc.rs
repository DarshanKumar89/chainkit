//! # Example 27: Cosmos RPC Support
//!
//! Demonstrates ChainRPC's Cosmos-specific features: method safety classification,
//! CU cost tracking, the CosmosTransport wrapper, and the CosmosChainClient.
//!
//! Cosmos chains (Cosmos Hub, Osmosis, Sei) use Tendermint JSON-RPC on port 26657.

use chainrpc_core::cosmos::{
    classify_cosmos_method, is_cosmos_safe_to_retry,
    CosmosCuCostTable, CosmosTransport,
    cosmos_mainnet_endpoints, osmosis_mainnet_endpoints,
};
use chainrpc_core::method_safety::MethodSafety;

#[tokio::main]
async fn main() {
    println!("=== Cosmos RPC Support ===\n");

    // =====================================================================
    // 1. Method Safety Classification
    // =====================================================================
    println!("--- Method Safety Classification ---\n");

    assert_eq!(classify_cosmos_method("block"), MethodSafety::Safe);
    assert_eq!(classify_cosmos_method("status"), MethodSafety::Safe);
    assert_eq!(classify_cosmos_method("broadcast_tx_sync"), MethodSafety::Idempotent);
    assert_eq!(classify_cosmos_method("broadcast_tx_async"), MethodSafety::Unsafe);

    println!("block               -> {:?}", classify_cosmos_method("block"));
    println!("status              -> {:?}", classify_cosmos_method("status"));
    println!("broadcast_tx_sync   -> {:?}", classify_cosmos_method("broadcast_tx_sync"));
    println!("broadcast_tx_async  -> {:?}", classify_cosmos_method("broadcast_tx_async"));

    assert!(is_cosmos_safe_to_retry("validators"));
    assert!(!is_cosmos_safe_to_retry("broadcast_tx_async"));

    // =====================================================================
    // 2. CU Cost Table
    // =====================================================================
    println!("\n--- CU Cost Table ---\n");

    let costs = CosmosCuCostTable::defaults();
    println!("status costs {} CU", costs.cost_for("status"));           // 5
    println!("block costs {} CU", costs.cost_for("block"));             // 20
    println!("tx_search costs {} CU", costs.cost_for("tx_search"));     // 50

    // =====================================================================
    // 3. CosmosTransport
    // =====================================================================
    // CosmosTransport wraps any RpcTransport for Cosmos RPC:
    //
    //   let http = Arc::new(HttpRpcClient::default_for(
    //       "https://cosmos-rpc.polkachu.com"
    //   ));
    //   let cosmos = CosmosTransport::new(http);
    //
    //   let status = cosmos.send(JsonRpcRequest::new(1, "status", vec![])).await?;

    println!("\n--- CosmosTransport ---\n");
    println!("CosmosTransport wraps any RpcTransport for Tendermint JSON-RPC.");
    println!("Usage: CosmosTransport::new(inner_transport)");

    // =====================================================================
    // 4. CosmosChainClient
    // =====================================================================
    // CosmosChainClient implements the unified ChainClient trait:
    //
    //   let client = CosmosChainClient::new(transport, "cosmoshub-4");
    //   let height = client.get_head_height().await?;
    //   let block = client.get_block_by_height(height).await?;
    //   assert_eq!(client.chain_family(), "cosmos");

    println!("\n--- CosmosChainClient ---\n");
    println!("Implements unified ChainClient trait.");
    println!("Maps: status → get_head_height(), block → get_block_by_height()");

    // =====================================================================
    // 5. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Cosmos Hub mainnet:");
    for url in cosmos_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nOsmosis mainnet:");
    for url in osmosis_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
