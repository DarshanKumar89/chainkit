//! # Request Hedging
//!
//! Demonstrates `hedged_send()` — a latency-optimization technique that races a
//! primary provider against a backup. If the primary does not respond within the
//! `hedge_delay`, a second request is fired to the backup and the first response
//! to arrive wins.
//!
//! Safety rules enforced by `hedged_send()`:
//!
//! - **Read-only (safe) methods** like `eth_blockNumber`, `eth_getBalance`,
//!   `eth_call`, and `eth_getLogs` are hedged normally.
//! - **Write methods** like `eth_sendRawTransaction` and `eth_sendTransaction`
//!   are NEVER hedged — they always go to the primary only, because sending the
//!   same transaction to two nodes could cause double-submission issues.
//!
//! This is determined by `method_safety::is_safe_to_retry()`.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use chainrpc_core::error::TransportError;
use chainrpc_core::hedging::{hedged_send, HedgingConfig};
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
use chainrpc_core::transport::RpcTransport;

// ---------------------------------------------------------------------------
// Mock transports with configurable latency
// ---------------------------------------------------------------------------
struct SlowPrimary;

#[async_trait]
impl RpcTransport for SlowPrimary {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Simulates a slow primary provider (300ms latency).
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!({
                "source": "primary",
                "method": req.method,
                "block": "0x1234567"
            })),
            error: None,
        })
    }

    fn url(&self) -> &str {
        "https://primary.example.com"
    }
}

struct FastBackup;

#[async_trait]
impl RpcTransport for FastBackup {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Simulates a fast backup provider (20ms latency).
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!({
                "source": "backup",
                "method": req.method,
                "block": "0x1234567"
            })),
            error: None,
        })
    }

    fn url(&self) -> &str {
        "https://backup.example.com"
    }
}

struct FastPrimary;

#[async_trait]
impl RpcTransport for FastPrimary {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Simulates a fast primary (10ms latency — faster than hedge delay).
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!({
                "source": "primary",
                "method": req.method,
            })),
            error: None,
        })
    }

    fn url(&self) -> &str {
        "https://fast-primary.example.com"
    }
}

#[tokio::main]
async fn main() {
    println!("=== Request Hedging Demo ===\n");

    // Hedge delay: if primary has not responded within 100ms, fire backup.
    let config = HedgingConfig {
        hedge_delay: Duration::from_millis(100),
    };

    // ---------------------------------------------------------------
    // 1. Slow primary — backup wins the race
    // ---------------------------------------------------------------
    println!("--- Scenario 1: slow primary (300ms), fast backup (20ms) ---");
    println!("    hedge_delay = 100ms\n");

    let primary = SlowPrimary;
    let backup = FastBackup;

    let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
    let start = std::time::Instant::now();
    let resp = hedged_send(&primary, &backup, req, &config)
        .await
        .expect("hedged_send");
    let elapsed = start.elapsed();

    let result = resp.result.unwrap();
    println!("  winner : {}", result["source"]);
    println!("  elapsed: {elapsed:?}");
    // The backup fires at t=100ms and responds at ~120ms.
    // The primary would not respond until t=300ms.
    // So the backup wins.

    // ---------------------------------------------------------------
    // 2. Fast primary — primary wins before hedge fires
    // ---------------------------------------------------------------
    println!("\n--- Scenario 2: fast primary (10ms), backup never fires ---");

    let fast_primary = FastPrimary;
    let backup = FastBackup;

    let req = JsonRpcRequest::auto("eth_getBalance", vec![json!("0xdead"), json!("latest")]);
    let start = std::time::Instant::now();
    let resp = hedged_send(&fast_primary, &backup, req, &config)
        .await
        .expect("hedged_send");
    let elapsed = start.elapsed();

    let result = resp.result.unwrap();
    println!("  winner : {}", result["source"]);
    println!("  elapsed: {elapsed:?}");
    // Primary responds in ~10ms, well before the 100ms hedge delay.
    // The backup request is never even sent.

    // ---------------------------------------------------------------
    // 3. Write method — NO hedging, primary only
    // ---------------------------------------------------------------
    println!("\n--- Scenario 3: write method (eth_sendRawTransaction) ---");
    println!("    Write methods are NEVER hedged — primary only.\n");

    let primary = SlowPrimary; // 300ms, but we accept the latency for safety
    let backup = FastBackup;

    let req = JsonRpcRequest::auto(
        "eth_sendRawTransaction",
        vec![json!("0xf86c808504a817c80082520894...")],
    );
    let start = std::time::Instant::now();
    let resp = hedged_send(&primary, &backup, req, &config)
        .await
        .expect("hedged_send");
    let elapsed = start.elapsed();

    let result = resp.result.unwrap();
    println!("  winner : {}", result["source"]);
    println!("  elapsed: {elapsed:?}");
    // Always "primary" — hedged_send() checks is_safe_to_retry() and skips
    // the backup entirely for non-safe methods.

    // ---------------------------------------------------------------
    // 4. Explanation of method safety classification
    // ---------------------------------------------------------------
    println!("\n--- Method safety for hedging ---");
    let methods = [
        ("eth_blockNumber",          true,  "read-only, safe to hedge"),
        ("eth_getBalance",           true,  "read-only, safe to hedge"),
        ("eth_call",                 true,  "read-only, safe to hedge"),
        ("eth_getLogs",              true,  "read-only, safe to hedge"),
        ("eth_getTransactionReceipt", true, "read-only, safe to hedge"),
        ("eth_sendRawTransaction",   false, "idempotent write, NOT hedged"),
        ("eth_sendTransaction",      false, "unsafe write, NOT hedged"),
    ];

    for (method, hedged, reason) in methods {
        let marker = if hedged { "HEDGE" } else { "PRIMARY ONLY" };
        println!("  {method:35} [{marker:>12}] — {reason}");
    }

    println!("\n=== Done ===");
}
