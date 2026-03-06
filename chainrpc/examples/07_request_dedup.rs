//! # Example 07: Request Deduplication
//!
//! Demonstrates `DedupTransport` which coalesces identical in-flight RPC
//! requests so that only one actual network call is made, regardless of how
//! many concurrent callers issue the same request.
//!
//! ## What this demonstrates
//!
//! - Wrapping an `HttpRpcClient` with `DedupTransport`
//! - Firing 10 concurrent `eth_blockNumber` calls
//! - Verifying that all 10 return the same result
//! - Only 1 actual RPC call is made to the network
//! - `in_flight_count()` to monitor pending requests
//! - Different methods are NOT deduplicated (each gets its own call)
//! - Sequential requests for the same method are NOT deduplicated
//!   (dedup only applies to concurrent in-flight requests)

use std::sync::Arc;

use chainrpc_core::dedup::DedupTransport;
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Step 1: Create the underlying HTTP transport.
    // -----------------------------------------------------------------------
    let http_client = Arc::new(
        HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"),
    ) as Arc<dyn RpcTransport>;

    // -----------------------------------------------------------------------
    // Step 2: Wrap it with DedupTransport.
    //
    // DedupTransport uses a hash of (method, params) as the dedup key.
    // If two tasks call send() with identical (method, params) while the
    // first is still in flight, the second waits for the first's result
    // instead of making a separate network call.
    // -----------------------------------------------------------------------
    let dedup = Arc::new(DedupTransport::new(http_client));

    // -----------------------------------------------------------------------
    // Step 3: Fire 10 concurrent eth_blockNumber requests.
    //
    // All 10 requests have the same method and params, so DedupTransport
    // will coalesce them into a single network call.
    // -----------------------------------------------------------------------
    println!("--- 10 Concurrent eth_blockNumber Requests ---\n");

    let mut handles = vec![];

    for i in 0..10 {
        let dedup_clone = dedup.clone();
        let handle = tokio::spawn(async move {
            // All 10 tasks send the exact same request.
            let req = JsonRpcRequest::new(1, "eth_blockNumber", vec![]);
            let result = dedup_clone.send(req).await;
            (i, result)
        });
        handles.push(handle);
    }

    // Collect results from all 10 tasks.
    let mut results = vec![];
    for handle in handles {
        let (i, result) = handle.await.expect("task panicked");
        match result {
            Ok(resp) => {
                let block = resp.result.as_ref().map(|v| v.to_string()).unwrap_or_default();
                println!("Task {i:>2}: block = {block}");
                results.push(block);
            }
            Err(e) => {
                println!("Task {i:>2}: error = {e}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Step 4: Verify all results are identical.
    //
    // Since only one network call was made, all 10 tasks receive a clone
    // of the same response.
    // -----------------------------------------------------------------------
    if !results.is_empty() {
        let first = &results[0];
        let all_same = results.iter().all(|r| r == first);
        println!("\nAll results identical: {all_same}");
    }

    // After all requests complete, in_flight_count should be 0.
    println!("In-flight requests: {}", dedup.in_flight_count());

    // -----------------------------------------------------------------------
    // Step 5: Different methods are NOT deduplicated.
    //
    // Requests with different method names (or different params) each get
    // their own network call, even if they're concurrent.
    // -----------------------------------------------------------------------
    println!("\n--- Different Methods (Not Deduplicated) ---\n");

    let dedup_a = dedup.clone();
    let dedup_b = dedup.clone();

    let (result_a, result_b) = tokio::join!(
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(1, "eth_blockNumber", vec![]);
            dedup_a.send(req).await
        }),
        tokio::spawn(async move {
            let req = JsonRpcRequest::new(2, "eth_chainId", vec![]);
            dedup_b.send(req).await
        }),
    );

    match result_a.unwrap() {
        Ok(resp) => println!("eth_blockNumber: {:?}", resp.result),
        Err(e) => println!("eth_blockNumber error: {e}"),
    }
    match result_b.unwrap() {
        Ok(resp) => println!("eth_chainId:     {:?}", resp.result),
        Err(e) => println!("eth_chainId error: {e}"),
    }

    println!("\nThese were two separate network calls (different methods).");

    // -----------------------------------------------------------------------
    // Step 6: Sequential requests are NOT deduplicated.
    //
    // Dedup only works for concurrent in-flight requests. Once a request
    // completes and the pending entry is removed, the next identical request
    // will make a fresh network call.
    // -----------------------------------------------------------------------
    println!("\n--- Sequential Requests (Not Deduplicated) ---\n");

    let req1 = JsonRpcRequest::new(1, "eth_blockNumber", vec![]);
    let resp1 = dedup.send(req1).await.expect("request 1 failed");
    println!("Sequential call 1: {:?}", resp1.result);

    // This is a new network call because the first has already completed.
    let req2 = JsonRpcRequest::new(2, "eth_blockNumber", vec![]);
    let resp2 = dedup.send(req2).await.expect("request 2 failed");
    println!("Sequential call 2: {:?}", resp2.result);

    println!("\nThese were two separate network calls (sequential, not concurrent).");
    println!("In-flight count: {}", dedup.in_flight_count());
}
