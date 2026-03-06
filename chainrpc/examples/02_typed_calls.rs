//! # Example 02: Typed RPC Calls
//!
//! Demonstrates the `call<T>()` convenience method on `RpcTransport`, which
//! sends a JSON-RPC request and deserializes the `result` field directly
//! into any type that implements `serde::de::DeserializeOwned`.
//!
//! ## What this demonstrates
//!
//! - `eth_blockNumber` -- returns a hex string
//! - `eth_chainId` -- returns a hex string (chain identifier)
//! - `eth_getBalance` -- returns a hex-encoded Wei balance
//! - `eth_getTransactionCount` -- returns a hex-encoded nonce
//! - Using `serde_json::Value` for params and results
//! - Error handling with `TransportError`

use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;
use serde_json::json;

#[tokio::main]
async fn main() {
    // Create a client using the Alchemy provider profile.
    // Replace with your actual API key.
    let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY");

    // -----------------------------------------------------------------------
    // 1. eth_blockNumber -- no params, returns hex string
    // -----------------------------------------------------------------------
    let block_hex: String = client
        .call(1, "eth_blockNumber", vec![])
        .await
        .expect("eth_blockNumber failed");

    let block_num = u64::from_str_radix(block_hex.trim_start_matches("0x"), 16).unwrap();
    println!("Block number: {block_hex} ({block_num})");

    // -----------------------------------------------------------------------
    // 2. eth_chainId -- no params, returns hex string
    //
    // Ethereum mainnet = "0x1", Polygon = "0x89", Arbitrum = "0xa4b1", etc.
    // -----------------------------------------------------------------------
    let chain_hex: String = client
        .call(2, "eth_chainId", vec![])
        .await
        .expect("eth_chainId failed");

    let chain_id = u64::from_str_radix(chain_hex.trim_start_matches("0x"), 16).unwrap();
    println!("Chain ID: {chain_hex} ({chain_id})");

    // -----------------------------------------------------------------------
    // 3. eth_getBalance -- params: [address, block_tag]
    //
    // Returns the balance of the account in Wei as a hex string.
    // Vitalik's address is used here as an example.
    // -----------------------------------------------------------------------
    let vitalik = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
    let balance_hex: String = client
        .call(
            3,
            "eth_getBalance",
            vec![json!(vitalik), json!("latest")],
        )
        .await
        .expect("eth_getBalance failed");

    // Convert Wei to Ether for display (1 ETH = 10^18 Wei).
    let balance_wei = u128::from_str_radix(balance_hex.trim_start_matches("0x"), 16).unwrap_or(0);
    let balance_eth = balance_wei as f64 / 1e18;
    println!("Balance of {vitalik}: {balance_hex} ({balance_eth:.4} ETH)");

    // -----------------------------------------------------------------------
    // 4. eth_getTransactionCount -- params: [address, block_tag]
    //
    // Returns the number of transactions sent from the address (the nonce).
    // -----------------------------------------------------------------------
    let nonce_hex: String = client
        .call(
            4,
            "eth_getTransactionCount",
            vec![json!(vitalik), json!("latest")],
        )
        .await
        .expect("eth_getTransactionCount failed");

    let nonce = u64::from_str_radix(nonce_hex.trim_start_matches("0x"), 16).unwrap_or(0);
    println!("Nonce of {vitalik}: {nonce_hex} ({nonce})");

    // -----------------------------------------------------------------------
    // 5. Deserializing into serde_json::Value for complex responses
    //
    // For methods that return complex objects (blocks, transactions, receipts),
    // you can deserialize into `serde_json::Value` and navigate the JSON tree.
    // -----------------------------------------------------------------------
    let block: serde_json::Value = client
        .call(
            5,
            "eth_getBlockByNumber",
            vec![json!("latest"), json!(false)], // false = don't include full txs
        )
        .await
        .expect("eth_getBlockByNumber failed");

    // Navigate the JSON object to extract specific fields.
    let block_hash = block.get("hash").and_then(|v| v.as_str()).unwrap_or("unknown");
    let timestamp_hex = block.get("timestamp").and_then(|v| v.as_str()).unwrap_or("0x0");
    let timestamp = u64::from_str_radix(timestamp_hex.trim_start_matches("0x"), 16).unwrap_or(0);
    let tx_count = block.get("transactions").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);

    println!("\nLatest block details:");
    println!("  Hash:         {block_hash}");
    println!("  Timestamp:    {timestamp} (unix)");
    println!("  Transactions: {tx_count}");

    // -----------------------------------------------------------------------
    // 6. Handling RPC errors gracefully
    //
    // If the node returns a JSON-RPC error (e.g., invalid params), the
    // `call()` method returns `Err(TransportError::Rpc(...))`.
    // -----------------------------------------------------------------------
    let result: Result<String, _> = client
        .call(
            6,
            "eth_getBalance",
            vec![json!("not_a_valid_address"), json!("latest")],
        )
        .await;

    match result {
        Ok(balance) => println!("\nUnexpected success: {balance}"),
        Err(e) => {
            println!("\nExpected error for invalid address:");
            println!("  Error: {e}");
            println!("  Is retryable?       {}", e.is_retryable());
            println!("  Is execution error? {}", e.is_execution_error());
        }
    }
}
