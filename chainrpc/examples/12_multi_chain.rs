//! # Multi-Chain Router
//!
//! Demonstrates `ChainRouter` — a single entry point that routes JSON-RPC
//! requests to the correct provider pool based on chain ID.
//!
//! Features shown:
//!
//! - **`add_chain()`** — register a transport for each chain
//! - **`send_to()`** — send a single request to a specific chain
//! - **`parallel()`** — fire concurrent cross-chain queries and collect results
//! - **`health_summary()`** — get the health status of every registered chain
//! - **`chain_ids()`** / **`chain_count()`** — introspect configured chains

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use chainrpc_core::error::TransportError;
use chainrpc_core::multi_chain::ChainRouter;
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
use chainrpc_core::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// Mock transport — in production you would use HttpTransport or a ProviderPool
// ---------------------------------------------------------------------------
struct MockTransport {
    chain_name: &'static str,
    chain_id: u64,
}

#[async_trait]
impl RpcTransport for MockTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Simulate a response that includes the chain name so we can see routing.
        let result = json!({
            "chain": self.chain_name,
            "chain_id": self.chain_id,
            "method": req.method,
        });
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(result),
            error: None,
        })
    }

    fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }

    fn url(&self) -> &str {
        match self.chain_id {
            1     => "https://eth-mainnet.example.com",
            137   => "https://polygon-rpc.example.com",
            42161 => "https://arb-mainnet.example.com",
            _     => "https://unknown.example.com",
        }
    }
}

#[tokio::main]
async fn main() {
    println!("=== Multi-Chain Router Demo ===\n");

    // ---------------------------------------------------------------
    // 1. Build a router with three chains
    // ---------------------------------------------------------------
    let mut router = ChainRouter::new();

    router.add_chain(
        1,
        Arc::new(MockTransport { chain_name: "Ethereum", chain_id: 1 }),
    );
    router.add_chain(
        137,
        Arc::new(MockTransport { chain_name: "Polygon", chain_id: 137 }),
    );
    router.add_chain(
        42161,
        Arc::new(MockTransport { chain_name: "Arbitrum", chain_id: 42161 }),
    );

    println!("Configured chains: {:?}", router.chain_ids());
    println!("Chain count      : {}\n", router.chain_count());

    // ---------------------------------------------------------------
    // 2. send_to() — single-chain request
    // ---------------------------------------------------------------
    println!("--- send_to() — single chain ---");

    let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    let resp = router.send_to(1, req).await.expect("Ethereum send");
    println!("  Ethereum  : {}", resp.result.unwrap());

    let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    let resp = router.send_to(137, req).await.expect("Polygon send");
    println!("  Polygon   : {}", resp.result.unwrap());

    let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    let resp = router.send_to(42161, req).await.expect("Arbitrum send");
    println!("  Arbitrum  : {}", resp.result.unwrap());

    // Unknown chain should fail gracefully.
    let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    let err = router.send_to(999, req).await.unwrap_err();
    println!("  Chain 999 : {err}");

    // ---------------------------------------------------------------
    // 3. parallel() — concurrent cross-chain queries
    // ---------------------------------------------------------------
    println!("\n--- parallel() — cross-chain block numbers ---");

    let requests = vec![
        (1,     JsonRpcRequest::auto("eth_blockNumber", vec![])),
        (137,   JsonRpcRequest::auto("eth_blockNumber", vec![])),
        (42161, JsonRpcRequest::auto("eth_blockNumber", vec![])),
    ];

    let results = router.parallel(requests).await;
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(resp) => {
                println!("  result[{i}]: {}", resp.result.as_ref().unwrap());
            }
            Err(e) => {
                println!("  result[{i}]: ERROR — {e}");
            }
        }
    }
    // All three requests execute concurrently via tokio::spawn.

    // ---------------------------------------------------------------
    // 4. parallel() with a mix of valid and unknown chains
    // ---------------------------------------------------------------
    println!("\n--- parallel() — partial failure ---");

    let mixed = vec![
        (1,   JsonRpcRequest::auto("eth_getBalance", vec![json!("0xdead"), json!("latest")])),
        (999, JsonRpcRequest::auto("eth_blockNumber", vec![])), // no such chain
        (137, JsonRpcRequest::auto("eth_gasPrice", vec![])),
    ];

    let mixed_results = router.parallel(mixed).await;
    for (i, result) in mixed_results.iter().enumerate() {
        match result {
            Ok(resp) => println!("  [{i}] OK:  {}", resp.result.as_ref().unwrap()),
            Err(e)   => println!("  [{i}] ERR: {e}"),
        }
    }
    // Index 1 should be an error; indices 0 and 2 should succeed.

    // ---------------------------------------------------------------
    // 5. health_summary() — check all providers at a glance
    // ---------------------------------------------------------------
    println!("\n--- health_summary() ---");
    let summary = router.health_summary();
    for (chain_id, status) in &summary {
        let name = match chain_id {
            1     => "Ethereum",
            137   => "Polygon",
            42161 => "Arbitrum",
            _     => "Unknown",
        };
        println!("  chain {chain_id:>5} ({name:10}): {status}");
    }

    // ---------------------------------------------------------------
    // 6. Accessing a chain's transport directly
    // ---------------------------------------------------------------
    println!("\n--- chain() — direct transport access ---");
    let eth_transport = router.chain(1).expect("chain 1 exists");
    let req = JsonRpcRequest::auto("eth_chainId", vec![]);
    let resp = eth_transport.send(req).await.expect("direct send");
    println!("  direct call result: {}", resp.result.unwrap());

    println!("\n=== Done ===");
}
