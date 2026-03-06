//! # Backpressure Transport
//!
//! Demonstrates `BackpressureTransport` — a wrapper that caps the number of
//! concurrent in-flight requests to a provider. When the limit is reached,
//! new requests are immediately rejected with `TransportError::Overloaded`
//! rather than being queued unboundedly (which risks OOM under load spikes).
//!
//! Key APIs:
//!
//! - `BackpressureTransport::new(inner, config)` — wrap any `RpcTransport`
//! - `in_flight()` — current number of pending requests
//! - `is_full()` — true when `in_flight == max_in_flight`
//! - Rejected requests receive `TransportError::Overloaded { queue_depth }`

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use chainrpc_core::backpressure::{BackpressureConfig, BackpressureTransport};
use chainrpc_core::error::TransportError;
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
use chainrpc_core::transport::RpcTransport;

// ---------------------------------------------------------------------------
// Mock transports
// ---------------------------------------------------------------------------

/// A transport that takes 500ms to respond — simulates a slow provider.
struct SlowTransport;

#[async_trait]
impl RpcTransport for SlowTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!("0x1")),
            error: None,
        })
    }

    fn url(&self) -> &str {
        "https://slow-provider.example.com"
    }
}

/// A transport that responds instantly.
struct InstantTransport;

#[async_trait]
impl RpcTransport for InstantTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: Some(json!("0x1")),
            error: None,
        })
    }

    fn url(&self) -> &str {
        "https://fast-provider.example.com"
    }
}

#[tokio::main]
async fn main() {
    println!("=== Backpressure Transport Demo ===\n");

    // ---------------------------------------------------------------
    // 1. Basic setup — max 50 concurrent requests
    // ---------------------------------------------------------------
    let inner = Arc::new(SlowTransport) as Arc<dyn RpcTransport>;
    let bp = Arc::new(BackpressureTransport::new(
        inner,
        BackpressureConfig { max_in_flight: 50 },
    ));

    println!("max_in_flight : 50");
    println!("in_flight()   : {}", bp.in_flight());
    println!("is_full()     : {}\n", bp.is_full());

    // ---------------------------------------------------------------
    // 2. Fire 60 requests — first 50 accepted, last 10 rejected
    // ---------------------------------------------------------------
    println!("--- Firing 60 concurrent requests (limit=50) ---");

    let mut handles = Vec::with_capacity(60);
    for i in 0..60 {
        let bp_clone = bp.clone();
        handles.push(tokio::spawn(async move {
            let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
            let result = bp_clone.send(req).await;
            (i, result)
        }));
    }

    // Give the spawned tasks a moment to acquire permits.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Check the gauge while requests are in flight.
    println!("  in_flight during burst: {}", bp.in_flight());
    println!("  is_full during burst : {}", bp.is_full());

    // Collect all results.
    let mut accepted = 0u32;
    let mut rejected = 0u32;

    for handle in handles {
        let (i, result) = handle.await.expect("task panicked");
        match &result {
            Ok(_) => {
                accepted += 1;
            }
            Err(TransportError::Overloaded { queue_depth }) => {
                if rejected < 3 {
                    // Print the first few rejections for illustration.
                    println!(
                        "  request #{i:>2}: REJECTED (Overloaded, queue_depth={queue_depth})"
                    );
                }
                rejected += 1;
            }
            Err(e) => {
                println!("  request #{i:>2}: unexpected error: {e}");
            }
        }
    }

    println!("\n  Summary:");
    println!("    accepted : {accepted}");
    println!("    rejected : {rejected}");
    // Expected: 50 accepted, 10 rejected.

    // After all requests complete, slots are released.
    println!("    in_flight after completion: {}", bp.in_flight());

    // ---------------------------------------------------------------
    // 3. Load shedding pattern — check before sending
    // ---------------------------------------------------------------
    println!("\n--- Load shedding pattern ---");

    let inner = Arc::new(InstantTransport) as Arc<dyn RpcTransport>;
    let bp = Arc::new(BackpressureTransport::new(
        inner,
        BackpressureConfig { max_in_flight: 50 },
    ));

    // A production caller can check is_full() before even attempting to send,
    // and shed load proactively (e.g., return a 503 to the upstream client).
    if bp.is_full() {
        println!("  Transport is full — shedding load (return 503 to caller)");
    } else {
        println!("  Transport has capacity ({}/50 in flight)", bp.in_flight());
        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let resp = bp.send(req).await.expect("send ok");
        println!("  Response: {}", resp.result.unwrap());
    }

    // ---------------------------------------------------------------
    // 4. Graceful degradation with backpressure metrics
    // ---------------------------------------------------------------
    println!("\n--- Backpressure metrics for monitoring ---");

    // Fill up 30 slots to simulate moderate load.
    let inner_slow = Arc::new(SlowTransport) as Arc<dyn RpcTransport>;
    let bp_monitor = Arc::new(BackpressureTransport::new(
        inner_slow,
        BackpressureConfig { max_in_flight: 50 },
    ));

    let mut background_tasks = Vec::new();
    for _ in 0..30 {
        let bp_clone = bp_monitor.clone();
        background_tasks.push(tokio::spawn(async move {
            let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
            let _ = bp_clone.send(req).await;
        }));
    }

    // Let tasks acquire permits.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let in_flight = bp_monitor.in_flight();
    let utilization = in_flight as f64 / 50.0 * 100.0;
    println!("  in_flight   : {in_flight}/50");
    println!("  utilization : {utilization:.0}%");
    println!("  is_full     : {}", bp_monitor.is_full());

    if utilization > 80.0 {
        println!("  WARNING: transport utilization above 80% — consider scaling");
    } else if utilization > 50.0 {
        println!("  INFO: moderate load — within acceptable range");
    } else {
        println!("  OK: healthy headroom available");
    }

    // Clean up background tasks.
    for handle in background_tasks {
        let _ = handle.await;
    }

    println!("\n=== Done ===");
}
