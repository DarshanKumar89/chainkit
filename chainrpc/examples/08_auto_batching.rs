//! # Example 08: Auto-Batching Transport
//!
//! Demonstrates `BatchingTransport` which automatically coalesces multiple
//! requests arriving within a time window into a single JSON-RPC batch call.
//!
//! ## What this demonstrates
//!
//! - Creating a `BatchingTransport` with a 10ms batching window
//! - Firing 5 concurrent requests within the window
//! - All 5 are batched into a single HTTP call (`send_batch`)
//! - A single request that arrives alone bypasses batching
//! - Each caller receives their individual response via oneshot channels
//! - How to use `BatchingTransport` as an `Arc<dyn RpcTransport>`

use std::sync::Arc;
use std::time::Duration;

use chainrpc_core::batch::BatchingTransport;
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;
use serde_json::json;

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Step 1: Create the underlying HTTP transport.
    //
    // BatchingTransport wraps any Arc<dyn RpcTransport> and delegates the
    // actual HTTP calls (including true HTTP batching) to it.
    // -----------------------------------------------------------------------
    let http_client = Arc::new(
        HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"),
    ) as Arc<dyn RpcTransport>;

    // -----------------------------------------------------------------------
    // Step 2: Create the BatchingTransport with a 10ms window.
    //
    // How it works:
    //   1. When the first request arrives, a timer starts (10ms window).
    //   2. Any requests arriving during the window are queued.
    //   3. When the window expires, all queued requests are flushed as a
    //      single JSON-RPC batch call (`send_batch`).
    //   4. If only one request arrived, it's sent individually (skip batch
    //      overhead).
    //   5. Each caller gets their response via a oneshot channel.
    //
    // Note: `new()` returns an `Arc<BatchingTransport>` because it spawns
    // a background flush task that holds a reference.
    // -----------------------------------------------------------------------
    let batcher: Arc<BatchingTransport> = BatchingTransport::new(
        http_client.clone(),
        Duration::from_millis(10), // 10ms batching window
    );

    // -----------------------------------------------------------------------
    // Step 3: Fire 5 concurrent requests within the batching window.
    //
    // Because all 5 arrive within 10ms, they'll be collected and sent as
    // a single JSON-RPC batch array:
    //
    //   POST /v2/KEY
    //   [
    //     {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
    //     {"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":2},
    //     {"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":3},
    //     {"jsonrpc":"2.0","method":"eth_getBalance","params":[...],"id":4},
    //     {"jsonrpc":"2.0","method":"eth_getTransactionCount","params":[...],"id":5}
    //   ]
    // -----------------------------------------------------------------------
    println!("--- 5 Concurrent Requests (Batched) ---\n");

    let vitalik = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";

    // Spawn 5 concurrent tasks, each sending a different RPC method.
    let b1 = batcher.clone();
    let b2 = batcher.clone();
    let b3 = batcher.clone();
    let b4 = batcher.clone();
    let b5 = batcher.clone();

    let addr = vitalik.to_string();
    let addr2 = vitalik.to_string();

    let (r1, r2, r3, r4, r5) = tokio::join!(
        // Request 1: eth_blockNumber
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(1, "eth_blockNumber", vec![]);
            b1.send(req).await
        }),
        // Request 2: eth_chainId
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(2, "eth_chainId", vec![]);
            b2.send(req).await
        }),
        // Request 3: eth_gasPrice
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(3, "eth_gasPrice", vec![]);
            b3.send(req).await
        }),
        // Request 4: eth_getBalance
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(4, "eth_getBalance", vec![json!(addr), json!("latest")]);
            b4.send(req).await
        }),
        // Request 5: eth_getTransactionCount
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(5, "eth_getTransactionCount", vec![json!(addr2), json!("latest")]);
            b5.send(req).await
        }),
    );

    // Each task gets its own individual response, matched by position.
    print_result("eth_blockNumber", r1.unwrap());
    print_result("eth_chainId", r2.unwrap());
    print_result("eth_gasPrice", r3.unwrap());
    print_result("eth_getBalance", r4.unwrap());
    print_result("eth_getTransactionCount", r5.unwrap());

    println!("\nAll 5 requests were sent in a single HTTP batch call.");

    // -----------------------------------------------------------------------
    // Step 4: A single request bypasses batching.
    //
    // When only one request arrives during the window, BatchingTransport
    // sends it individually using `send()` instead of `send_batch()`.
    // This avoids the overhead of JSON array wrapping for single calls.
    // -----------------------------------------------------------------------
    println!("\n--- Single Request (Bypasses Batching) ---\n");

    // Wait for the previous batch window to close.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let single_req = JsonRpcRequest::new(100, "eth_blockNumber", vec![]);
    let single_resp = batcher.send(single_req).await;
    print_result("eth_blockNumber (single)", single_resp);

    println!("\nThis single request was sent individually, not as a batch.");

    // -----------------------------------------------------------------------
    // Step 5: BatchingTransport implements RpcTransport.
    //
    // You can use it anywhere an `Arc<dyn RpcTransport>` is expected,
    // including as the inner transport for ProviderPool, CacheTransport,
    // or DedupTransport.
    // -----------------------------------------------------------------------
    println!("\n--- Composing with Other Transports ---\n");

    // BatchingTransport can be used as the inner transport for a cache.
    // This means cached misses get batched automatically.
    //
    // Example composition (pseudo-code):
    //   let http = Arc::new(HttpRpcClient::default_for(url));
    //   let batcher = BatchingTransport::new(http, Duration::from_millis(10));
    //   let cache = CacheTransport::new(batcher, cache_config);
    //   let dedup = DedupTransport::new(Arc::new(cache));
    //
    // The full stack:
    //   Request -> DedupTransport -> CacheTransport -> BatchingTransport -> HttpRpcClient
    //
    // This gives you: dedup + caching + auto-batching + retry + circuit breaker + rate limiting

    // Use batcher as a generic RpcTransport.
    let transport: Arc<dyn RpcTransport> = batcher;
    let block_hex: String = transport
        .call(200, "eth_blockNumber", vec![])
        .await
        .expect("generic call failed");
    println!("Via Arc<dyn RpcTransport>: block = {block_hex}");
    println!("Health: {}", transport.health());
    println!("URL:    {}", transport.url());
}

/// Helper to print a request result.
fn print_result(
    label: &str,
    result: Result<chainrpc_core::request::JsonRpcResponse, chainrpc_core::error::TransportError>,
) {
    match result {
        Ok(resp) => {
            let value = resp.result.as_ref().map(|v| v.to_string()).unwrap_or_default();
            println!("  {label:<30} => {value}");
        }
        Err(e) => {
            println!("  {label:<30} => ERROR: {e}");
        }
    }
}
