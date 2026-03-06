//! # Example 04: Error Handling
//!
//! Demonstrates comprehensive error handling with `TransportError`.
//! ChainRPC uses a structured error enum so you can match on every possible
//! failure mode and decide how to handle it.
//!
//! ## What this demonstrates
//!
//! - Matching every `TransportError` variant
//! - Using `is_retryable()` and `is_execution_error()` helper methods
//! - Graceful degradation patterns (retry, failover, user feedback)
//! - Distinguishing between transient and permanent errors

use chainrpc_core::error::TransportError;
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;
use serde_json::json;

#[tokio::main]
async fn main() {
    let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY");

    // -----------------------------------------------------------------------
    // Example 1: Basic error handling with is_retryable() / is_execution_error()
    // -----------------------------------------------------------------------
    println!("--- Quick Error Classification ---\n");

    let result: Result<String, TransportError> = client
        .call(1, "eth_getBalance", vec![json!("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"), json!("latest")])
        .await;

    match result {
        Ok(balance) => println!("Balance: {balance}"),
        Err(ref e) if e.is_retryable() => {
            // Transient errors: HTTP failures, timeouts, rate limits.
            // The built-in retry policy already retried this, so if we're here
            // it means all retries were exhausted. Consider:
            //   - Switching to a backup provider
            //   - Queuing the request for later
            //   - Returning a cached result
            println!("Transient error (all retries exhausted): {e}");
        }
        Err(ref e) if e.is_execution_error() => {
            // Node-side execution errors: invalid params, reverted calls, etc.
            // These are NOT retryable -- the request itself is the problem.
            println!("Execution error (not retryable): {e}");
        }
        Err(e) => {
            // Structural errors: deserialization, circuit breaker, all providers down.
            println!("Other error: {e}");
        }
    }

    // -----------------------------------------------------------------------
    // Example 2: Exhaustive match on every TransportError variant
    // -----------------------------------------------------------------------
    println!("\n--- Exhaustive Error Handling ---\n");

    let req = JsonRpcRequest::new(2, "eth_blockNumber", vec![]);
    let result = client.send(req).await;

    match result {
        Ok(resp) => {
            if resp.is_ok() {
                println!("Success: {:?}", resp.result);
            } else {
                // The HTTP request succeeded, but the JSON-RPC response
                // itself contains an error (e.g., invalid method).
                let err = resp.error.unwrap();
                println!("JSON-RPC error {}: {}", err.code, err.message);
                if let Some(data) = &err.data {
                    println!("  Error data: {data}");
                }
            }
        }
        Err(e) => handle_transport_error(e),
    }

    // -----------------------------------------------------------------------
    // Example 3: Graceful degradation pattern
    // -----------------------------------------------------------------------
    println!("\n--- Graceful Degradation ---\n");

    let block = fetch_block_with_fallback(&client).await;
    println!("Block (with fallback): {block}");
}

/// Exhaustively handle every `TransportError` variant.
fn handle_transport_error(err: TransportError) {
    match err {
        // ----- Retryable (transient) errors -----

        TransportError::Http(msg) => {
            // Connection refused, DNS failure, TLS error, non-2xx HTTP status.
            // The client already retried this according to RetryConfig.
            println!("[HTTP] Connection/transport error: {msg}");
            println!("  Action: switch to backup provider or wait and retry");
        }

        TransportError::WebSocket(msg) => {
            // WebSocket connection dropped or failed.
            println!("[WS] WebSocket error: {msg}");
            println!("  Action: reconnect or fall back to HTTP polling");
        }

        TransportError::Timeout { ms } => {
            // Request exceeded the configured timeout (default 30s).
            println!("[TIMEOUT] Request timed out after {ms}ms");
            println!("  Action: increase timeout or switch provider");
        }

        TransportError::RateLimited { provider } => {
            // Provider returned HTTP 429 Too Many Requests.
            // The rate limiter should have prevented this, but the provider
            // may have stricter limits than configured.
            println!("[RATE_LIMITED] Provider rate limit hit: {provider}");
            println!("  Action: reduce request rate or upgrade provider tier");
        }

        // ----- Non-retryable (permanent) errors -----

        TransportError::Rpc(rpc_err) => {
            // The node processed the request and returned a JSON-RPC error.
            // This is never retryable -- the request itself is invalid.
            println!("[RPC] Node error {}: {}", rpc_err.code, rpc_err.message);

            // Common JSON-RPC error codes:
            match rpc_err.code {
                -32700 => println!("  Parse error: invalid JSON"),
                -32600 => println!("  Invalid request: malformed JSON-RPC"),
                -32601 => println!("  Method not found: check the method name"),
                -32602 => println!("  Invalid params: check parameter types/count"),
                -32603 => println!("  Internal error: node-side issue"),
                -32000 => println!("  Execution reverted: contract call failed"),
                _ => println!("  Custom error code: {}", rpc_err.code),
            }

            if let Some(data) = rpc_err.data {
                // The `data` field often contains the revert reason or ABI-encoded error.
                println!("  Error data: {data}");
            }
        }

        TransportError::CircuitOpen { provider } => {
            // The circuit breaker for this provider is open -- too many
            // consecutive failures. The provider is considered unhealthy.
            println!("[CIRCUIT_OPEN] Provider circuit breaker open: {provider}");
            println!("  Action: use a different provider; circuit will auto-reset after cool-down");
        }

        TransportError::AllProvidersDown => {
            // All providers in the pool have open circuit breakers.
            // This is a critical failure.
            println!("[ALL_DOWN] Every provider in the pool is unavailable!");
            println!("  Action: alert on-call, serve cached data, or return 503");
        }

        TransportError::Deserialization(serde_err) => {
            // The response JSON could not be deserialized into the target type.
            // This usually means the response format is unexpected.
            println!("[DESER] Failed to deserialize response: {serde_err}");
            println!("  Action: check the target type matches the RPC method's return type");
        }

        TransportError::Overloaded { queue_depth } => {
            // Backpressure: too many in-flight requests. The transport is
            // rejecting new requests to prevent resource exhaustion.
            println!("[OVERLOADED] Transport has {queue_depth} requests in flight");
            println!("  Action: apply backpressure to upstream callers");
        }

        TransportError::Cancelled => {
            // The operation was cancelled via a cancellation token.
            println!("[CANCELLED] Request was cancelled");
            println!("  Action: this is expected during graceful shutdown");
        }

        TransportError::Other(msg) => {
            // Catch-all for unexpected errors.
            println!("[OTHER] Unexpected error: {msg}");
        }
    }
}

/// Demonstrates graceful degradation: try the primary call, then fall back
/// to a cached/default value if everything fails.
async fn fetch_block_with_fallback(client: &HttpRpcClient) -> String {
    // Attempt the RPC call.
    match client.call::<String>(1, "eth_blockNumber", vec![]).await {
        Ok(block) => block,
        Err(e) if e.is_retryable() => {
            // All retries exhausted for a transient error.
            // In a real app, you might:
            //   1. Try a secondary client/provider
            //   2. Return a recently cached value
            //   3. Return a sentinel value and alert
            eprintln!("Warning: using fallback due to transient error: {e}");
            "0x0 (fallback)".to_string()
        }
        Err(e) => {
            // Permanent error -- something is fundamentally wrong.
            eprintln!("Error: permanent failure: {e}");
            "0x0 (error)".to_string()
        }
    }
}
