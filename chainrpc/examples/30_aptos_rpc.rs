//! # Example 30: Aptos RPC Support
//!
//! Demonstrates ChainRPC's Aptos-specific features: CU cost tracking,
//! AptosTransport (REST-to-JSON-RPC adapter), and AptosChainClient.
//!
//! Aptos uses a REST API (not JSON-RPC), so the transport internally maps
//! method names to REST operations.

use chainrpc_core::aptos::{
    AptosCuCostTable,
    aptos_mainnet_endpoints, aptos_testnet_endpoints, aptos_devnet_endpoints,
};

#[tokio::main]
async fn main() {
    println!("=== Aptos RPC Support ===\n");

    // =====================================================================
    // 1. CU Cost Table
    // =====================================================================
    println!("--- CU Cost Table ---\n");

    let costs = AptosCuCostTable::defaults();
    println!("get_ledger_info costs {} CU", costs.cost_for("get_ledger_info"));             // 5
    println!("get_block_by_height costs {} CU", costs.cost_for("get_block_by_height"));     // 20
    println!("simulate_transaction costs {} CU", costs.cost_for("simulate_transaction"));   // 30

    // =====================================================================
    // 2. AptosTransport
    // =====================================================================
    // AptosTransport wraps any RpcTransport to handle Aptos REST API:
    //
    //   let http = Arc::new(HttpRpcClient::default_for(
    //       "https://fullnode.mainnet.aptoslabs.com/v1"
    //   ));
    //   let aptos = AptosTransport::new(http);
    //
    //   // Methods are mapped internally to REST calls
    //   let req = JsonRpcRequest::new(1, "get_ledger_info", vec![]);
    //   let resp = aptos.send(req).await?;

    println!("\n--- AptosTransport ---\n");
    println!("AptosTransport wraps any RpcTransport (REST adapter).");
    println!("Method names map to Aptos REST API endpoints:");
    println!("  get_ledger_info       → GET /v1/");
    println!("  get_block_by_height   → GET /v1/blocks/by_height/{{h}}");
    println!("  get_account_resources → GET /v1/accounts/{{addr}}/resources");

    // =====================================================================
    // 3. AptosChainClient
    // =====================================================================
    // AptosChainClient implements the unified ChainClient trait:
    //
    //   let client = AptosChainClient::new(transport, "1");
    //   let height = client.get_head_height().await?;  // get_ledger_info → block_height
    //   let block = client.get_block_by_height(height).await?;
    //   assert_eq!(client.chain_family(), "aptos");

    println!("\n--- AptosChainClient ---\n");
    println!("Implements unified ChainClient trait.");
    println!("Maps: get_ledger_info → height, get_block_by_height → block");
    println!("Move-based events are emitted per transaction.");
    println!("Timestamps in microseconds (converted to seconds).");

    // =====================================================================
    // 4. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Aptos mainnet:");
    for url in aptos_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nAptos testnet:");
    for url in aptos_testnet_endpoints() {
        println!("  {url}");
    }

    println!("\nAptos devnet:");
    for url in aptos_devnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
