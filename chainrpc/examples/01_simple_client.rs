//! # Example 01: Simple RPC Client
//!
//! The simplest way to use ChainRPC -- create an HTTP client and make calls.
//! The client has built-in retry, circuit breaker, and rate limiting.
//!
//! ## What this demonstrates
//!
//! - Creating an `HttpRpcClient` with `default_for()` (zero config)
//! - Making a typed RPC call with `call::<T>()` that deserializes the response
//! - Making a raw request/response using `send()` + `JsonRpcRequest`
//! - Inspecting the raw `JsonRpcResponse` fields

use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse};
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Step 1: Create an HTTP client with sensible defaults.
    //
    // `default_for()` gives you:
    //   - 3 retries with exponential backoff (100ms initial, 2x multiplier)
    //   - Circuit breaker (opens after 5 failures, 30s cool-down)
    //   - Token-bucket rate limiter (300 CU capacity, 300 CU/s refill)
    //   - 30s request timeout
    // -----------------------------------------------------------------------
    let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY");

    // -----------------------------------------------------------------------
    // Step 2: Typed call -- `call<T>()` sends the request and deserializes
    // the JSON-RPC `result` field directly into the Rust type `T`.
    //
    // `eth_blockNumber` returns a hex-encoded string like "0x10d4f",
    // so we deserialize into `String`.
    //
    // Arguments to `call()`:
    //   - id:     request ID (u64) -- can be any number, used to match responses
    //   - method: the JSON-RPC method name
    //   - params: a Vec<serde_json::Value> of positional parameters
    // -----------------------------------------------------------------------
    let block_number: String = client
        .call(1, "eth_blockNumber", vec![])
        .await
        .expect("eth_blockNumber failed");

    println!("Latest block (typed): {block_number}");

    // Parse the hex string to a u64 for display.
    let block_u64 = u64::from_str_radix(block_number.trim_start_matches("0x"), 16)
        .expect("invalid hex block number");
    println!("Latest block (decimal): {block_u64}");

    // -----------------------------------------------------------------------
    // Step 3: Raw request/response -- for full control over the JSON-RPC
    // envelope.  Construct a `JsonRpcRequest` manually, send it, and
    // inspect the raw `JsonRpcResponse`.
    // -----------------------------------------------------------------------

    // `JsonRpcRequest::new()` requires an explicit ID.
    let req = JsonRpcRequest::new(2, "eth_blockNumber", vec![]);
    println!("\nSending raw request: method={}, id={}", req.method, req.id);

    let resp: JsonRpcResponse = client.send(req).await.expect("raw request failed");

    // The response has optional `result` and `error` fields.
    if resp.is_ok() {
        // `into_result()` unwraps the `result` field or returns the error.
        let value = resp.into_result().expect("response had error");
        println!("Raw response result: {value}");
    } else {
        let err = resp.error.expect("expected error in response");
        eprintln!("RPC error {}: {}", err.code, err.message);
    }

    // -----------------------------------------------------------------------
    // Step 4: Auto-incrementing IDs with `JsonRpcRequest::auto()`.
    //
    // If you don't care about specific request IDs, `auto()` uses a global
    // atomic counter so each request gets a unique, sequential ID.
    // -----------------------------------------------------------------------
    let auto_req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    println!("\nAuto-assigned request ID: {}", auto_req.id);

    let auto_resp = client.send(auto_req).await.expect("auto request failed");
    println!("Auto response: {:?}", auto_resp.result);

    // -----------------------------------------------------------------------
    // Step 5: Check the client's health status.
    //
    // The health reflects the circuit breaker state:
    //   - Healthy   => circuit closed (normal operation)
    //   - Degraded  => circuit half-open (probing after failure)
    //   - Unhealthy => circuit open (provider is down)
    // -----------------------------------------------------------------------
    let health = client.health();
    println!("\nClient health: {health}");
    println!("Client URL:    {}", client.url());
}
