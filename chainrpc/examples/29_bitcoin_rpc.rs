//! # Example 29: Bitcoin RPC Support
//!
//! Demonstrates ChainRPC's Bitcoin-specific features: method safety,
//! CU cost tracking, BitcoinTransport, and BitcoinChainClient.
//!
//! Bitcoin Core uses JSON-RPC on port 8332 with HTTP Basic Auth.

use chainrpc_core::bitcoin::{
    classify_bitcoin_method, is_bitcoin_safe_to_retry,
    BitcoinCuCostTable,
    bitcoin_mainnet_endpoints, bitcoin_testnet_endpoints,
};
use chainrpc_core::method_safety::MethodSafety;

#[tokio::main]
async fn main() {
    println!("=== Bitcoin RPC Support ===\n");

    // =====================================================================
    // 1. Method Safety Classification
    // =====================================================================
    println!("--- Method Safety Classification ---\n");

    assert_eq!(classify_bitcoin_method("getblock"), MethodSafety::Safe);
    assert_eq!(classify_bitcoin_method("getblockcount"), MethodSafety::Safe);
    assert_eq!(classify_bitcoin_method("sendrawtransaction"), MethodSafety::Idempotent);
    assert_eq!(classify_bitcoin_method("walletpassphrase"), MethodSafety::Unsafe);

    println!("getblock             -> {:?}", classify_bitcoin_method("getblock"));
    println!("getblockcount        -> {:?}", classify_bitcoin_method("getblockcount"));
    println!("sendrawtransaction   -> {:?}", classify_bitcoin_method("sendrawtransaction"));
    println!("walletpassphrase     -> {:?}", classify_bitcoin_method("walletpassphrase"));

    assert!(is_bitcoin_safe_to_retry("getblock"));
    assert!(!is_bitcoin_safe_to_retry("walletpassphrase"));

    // =====================================================================
    // 2. CU Cost Table
    // =====================================================================
    println!("\n--- CU Cost Table ---\n");

    let costs = BitcoinCuCostTable::defaults();
    println!("getblockcount costs {} CU", costs.cost_for("getblockcount"));       // 5
    println!("getblock costs {} CU", costs.cost_for("getblock"));                 // 20
    println!("getrawtransaction costs {} CU", costs.cost_for("getrawtransaction")); // 15

    // =====================================================================
    // 3. BitcoinChainClient
    // =====================================================================
    // BitcoinChainClient implements the unified ChainClient trait:
    //
    //   let client = BitcoinChainClient::new(transport, "mainnet");
    //   let height = client.get_head_height().await?;     // getblockcount
    //   let block = client.get_block_by_height(height).await?;  // getblockhash + getblock
    //   assert_eq!(client.chain_family(), "bitcoin");

    println!("\n--- BitcoinChainClient ---\n");
    println!("Implements unified ChainClient trait.");
    println!("Maps: getblockcount → height, getblockhash+getblock → block");
    println!("UTXO model: blocks contain transaction IDs, not logs/events.");

    // =====================================================================
    // 4. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Bitcoin mainnet:");
    for url in bitcoin_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nBitcoin testnet:");
    for url in bitcoin_testnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
