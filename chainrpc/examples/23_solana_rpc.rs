//! # Example 23: Solana RPC Support
//!
//! Demonstrates ChainRPC's Solana-specific features: commitment levels,
//! method safety classification, CU cost tracking, and the SolanaTransport
//! wrapper that auto-injects commitment config.
//!
//! ChainRPC's `RpcTransport` trait works with Solana's JSON-RPC natively.
//! The `solana` module adds Solana-aware semantics on top.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::sync::Arc;
use chainrpc_core::solana::{
    SolanaCommitment, SolanaTransport, SolanaCuCostTable,
    classify_solana_method, is_solana_safe_to_retry,
    solana_mainnet_endpoints, solana_devnet_endpoints,
};
use chainrpc_core::method_safety::MethodSafety;
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::RpcTransport;
use serde_json::json;

#[tokio::main]
async fn main() {
    println!("=== Solana RPC Support ===\n");

    // =====================================================================
    // 1. Commitment Levels
    // =====================================================================
    // Solana has 3 commitment levels (similar to Ethereum's block tags):
    let processed = SolanaCommitment::Processed;  // Fastest, least safe
    let confirmed = SolanaCommitment::Confirmed;   // Default — supermajority voted
    let finalized = SolanaCommitment::Finalized;   // Rooted, cannot be rolled back

    println!("--- Commitment Levels ---\n");
    println!("Processed safe for indexing? {}", processed.is_safe_for_indexing()); // false
    println!("Confirmed safe for display? {}", confirmed.is_safe_for_display());   // true
    println!("Finalized safe for indexing? {}", finalized.is_safe_for_indexing()); // true

    // =====================================================================
    // 2. Method Safety Classification
    // =====================================================================
    // Just like EVM, Solana methods are classified for retry/dedup/cache decisions:
    println!("\n--- Method Safety Classification ---\n");

    assert_eq!(classify_solana_method("getBalance"), MethodSafety::Safe);
    assert_eq!(classify_solana_method("getSlot"), MethodSafety::Safe);
    assert_eq!(classify_solana_method("sendTransaction"), MethodSafety::Idempotent);
    assert_eq!(classify_solana_method("requestAirdrop"), MethodSafety::Unsafe);

    println!("getBalance       -> {:?}", classify_solana_method("getBalance"));
    println!("getSlot          -> {:?}", classify_solana_method("getSlot"));
    println!("sendTransaction  -> {:?}", classify_solana_method("sendTransaction"));
    println!("requestAirdrop   -> {:?}", classify_solana_method("requestAirdrop"));

    // Safe methods can be retried on transient failure
    assert!(is_solana_safe_to_retry("getAccountInfo"));
    assert!(!is_solana_safe_to_retry("requestAirdrop"));

    println!("\ngetAccountInfo safe to retry? {}", is_solana_safe_to_retry("getAccountInfo"));
    println!("requestAirdrop safe to retry? {}", is_solana_safe_to_retry("requestAirdrop"));

    // =====================================================================
    // 3. CU Cost Table
    // =====================================================================
    // Solana method costs for rate limiting:
    println!("\n--- CU Cost Table ---\n");

    let costs = SolanaCuCostTable::defaults();
    println!("getSlot costs {} CU", costs.cost_for("getSlot"));                       // 5
    println!("getProgramAccounts costs {} CU", costs.cost_for("getProgramAccounts")); // 100
    println!("getBalance costs {} CU", costs.cost_for("getBalance"));                 // 10

    // =====================================================================
    // 4. SolanaTransport
    // =====================================================================
    // Wraps any RpcTransport and auto-injects commitment into requests.
    // In production, you'd wrap an HttpRpcClient:
    //
    //   let http = Arc::new(HttpRpcClient::default_for(
    //       "https://api.mainnet-beta.solana.com"
    //   ));
    //   let solana = SolanaTransport::new(http, SolanaCommitment::Finalized);
    //
    //   // All requests now include {"commitment": "finalized"}
    //   let balance = solana.send(JsonRpcRequest::auto("getBalance", vec![
    //       json!("TokenAddress..."),
    //   ])).await?;
    //
    //   // Switch commitment for a specific operation
    //   let fast = solana.with_commitment(SolanaCommitment::Processed);
    //   let slot = fast.send(JsonRpcRequest::auto("getSlot", vec![])).await?;

    println!("\n--- SolanaTransport ---\n");
    println!("SolanaTransport wraps any RpcTransport and auto-injects commitment.");
    println!("Usage: SolanaTransport::new(inner, SolanaCommitment::Finalized)");
    println!("Override: .with_commitment(SolanaCommitment::Processed)");

    // =====================================================================
    // 5. Known Endpoints
    // =====================================================================
    println!("\n--- Known Endpoints ---\n");

    println!("Solana mainnet endpoints:");
    for url in solana_mainnet_endpoints() {
        println!("  {url}");
    }

    println!("\nSolana devnet endpoints:");
    for url in solana_devnet_endpoints() {
        println!("  {url}");
    }

    println!("\nDone.");
}
