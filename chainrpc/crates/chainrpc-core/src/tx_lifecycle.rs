//! Transaction lifecycle management wired into [`RpcTransport`].
//!
//! This module provides async helper functions that compose the
//! [`TxTracker`] / [`ReceiptPoller`] primitives from [`crate::tx`] with
//! a live [`RpcTransport`] to drive the full send-track-confirm lifecycle.

use serde_json::Value;

use crate::error::TransportError;
use crate::request::JsonRpcRequest;
use crate::transport::RpcTransport;
use crate::tx::{ReceiptPoller, TrackedTx, TxStatus, TxTracker};

// ---------------------------------------------------------------------------
// poll_receipt
// ---------------------------------------------------------------------------

/// Poll for a transaction receipt with exponential backoff.
///
/// Uses the [`ReceiptPoller`] to determine the delay between attempts.
/// Calls `eth_getTransactionReceipt` on the transport.
///
/// Returns `Ok(Some(receipt))` when a receipt is found, `Ok(None)` if the
/// poller's maximum attempts are exhausted, or `Err` on transport failure.
pub async fn poll_receipt(
    transport: &dyn RpcTransport,
    tx_hash: &str,
    poller: &ReceiptPoller,
) -> Result<Option<Value>, TransportError> {
    let mut attempt: u32 = 1;

    loop {
        let delay = match poller.delay_for_attempt(attempt) {
            Some(d) => d,
            None => return Ok(None), // max attempts exceeded
        };

        // Wait before querying (except on the very first attempt).
        if attempt > 1 {
            tokio::time::sleep(delay).await;
        }

        let req = JsonRpcRequest::auto(
            "eth_getTransactionReceipt",
            vec![Value::String(tx_hash.to_string())],
        );
        let resp = transport.send(req).await?;
        let value = resp.into_result().map_err(TransportError::Rpc)?;

        if !value.is_null() {
            return Ok(Some(value));
        }

        attempt += 1;
    }
}

// ---------------------------------------------------------------------------
// send_and_track
// ---------------------------------------------------------------------------

/// Send a raw transaction and automatically track it.
///
/// 1. Sends via `eth_sendRawTransaction`.
/// 2. Extracts the returned transaction hash from the RPC response.
/// 3. Creates a [`TrackedTx`] and registers it with the [`TxTracker`].
/// 4. Returns the transaction hash.
pub async fn send_and_track(
    transport: &dyn RpcTransport,
    tracker: &TxTracker,
    raw_tx: &str,
    from: &str,
    nonce: u64,
) -> Result<String, TransportError> {
    let req = JsonRpcRequest::auto(
        "eth_sendRawTransaction",
        vec![Value::String(raw_tx.to_string())],
    );
    let resp = transport.send(req).await?;
    let result = resp.into_result().map_err(TransportError::Rpc)?;

    let tx_hash = result
        .as_str()
        .ok_or_else(|| {
            TransportError::Other("eth_sendRawTransaction did not return a string hash".into())
        })?
        .to_string();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let tracked = TrackedTx {
        tx_hash: tx_hash.clone(),
        from: from.to_string(),
        nonce,
        submitted_at: now,
        status: TxStatus::Pending,
        gas_price: None,
        max_fee: None,
        max_priority_fee: None,
        last_checked: now,
    };

    tracker.track(tracked);
    Ok(tx_hash)
}

// ---------------------------------------------------------------------------
// refresh_status
// ---------------------------------------------------------------------------

/// Check the status of a tracked transaction against the chain.
///
/// Queries `eth_getTransactionReceipt` and updates the tracker:
/// - If a receipt with a `blockNumber` is found, the status becomes
///   [`TxStatus::Included`].
/// - If the receipt is `null`, the status remains [`TxStatus::Pending`].
///
/// Returns the newly determined status.
pub async fn refresh_status(
    transport: &dyn RpcTransport,
    tracker: &TxTracker,
    tx_hash: &str,
) -> Result<TxStatus, TransportError> {
    let req = JsonRpcRequest::auto(
        "eth_getTransactionReceipt",
        vec![Value::String(tx_hash.to_string())],
    );
    let resp = transport.send(req).await?;
    let value = resp.into_result().map_err(TransportError::Rpc)?;

    let status = if value.is_null() {
        TxStatus::Pending
    } else {
        let block_number = value
            .get("blockNumber")
            .and_then(|v| v.as_str())
            .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);

        let block_hash = value
            .get("blockHash")
            .and_then(|v| v.as_str())
            .unwrap_or("0x0")
            .to_string();

        TxStatus::Included {
            block_number,
            block_hash,
        }
    };

    tracker.update_status(tx_hash, status.clone());
    Ok(status)
}

// ---------------------------------------------------------------------------
// detect_stuck
// ---------------------------------------------------------------------------

/// Detect and return stuck transactions.
///
/// Delegates to [`TxTracker::stuck`] using the supplied `current_time`.
/// For each stuck transaction, queries `eth_getTransactionCount` for the
/// sender to help diagnose nonce-based replacement or dropping (the caller
/// can use this information to decide on remediation).
///
/// Returns the list of stuck [`TrackedTx`] entries.
pub async fn detect_stuck(
    transport: &dyn RpcTransport,
    tracker: &TxTracker,
    current_time: u64,
) -> Vec<TrackedTx> {
    let stuck = tracker.stuck(current_time);

    // For each unique sender, refresh the on-chain nonce in the tracker.
    let mut seen_senders = std::collections::HashSet::new();
    for tx in &stuck {
        if seen_senders.insert(tx.from.clone()) {
            let req = JsonRpcRequest::auto(
                "eth_getTransactionCount",
                vec![
                    Value::String(tx.from.clone()),
                    Value::String("latest".to_string()),
                ],
            );
            if let Ok(resp) = transport.send(req).await {
                if let Ok(val) = resp.into_result() {
                    if let Some(hex) = val.as_str() {
                        if let Ok(nonce) =
                            u64::from_str_radix(hex.trim_start_matches("0x"), 16)
                        {
                            tracker.set_nonce(&tx.from, nonce);
                        }
                    }
                }
            }
        }
    }

    stuck
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcResponse, RpcId};
    use crate::tx::TxTrackerConfig;
    use async_trait::async_trait;
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Mock transport that returns configurable responses per method.
    // -----------------------------------------------------------------------

    struct MockTransport {
        /// Responses to return, keyed by method name.
        responses: Mutex<std::collections::HashMap<String, Value>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                responses: Mutex::new(std::collections::HashMap::new()),
            }
        }

        fn set_response(&self, method: &str, value: Value) {
            let mut map = self.responses.lock().unwrap();
            map.insert(method.to_string(), value);
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(
            &self,
            req: JsonRpcRequest,
        ) -> Result<JsonRpcResponse, TransportError> {
            let map = self.responses.lock().unwrap();
            let result = map.get(&req.method).cloned().unwrap_or(Value::Null);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(result),
                error: None,
            })
        }

        fn url(&self) -> &str {
            "mock://lifecycle"
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn send_and_track_records_tx() {
        let transport = MockTransport::new();
        transport.set_response(
            "eth_sendRawTransaction",
            Value::String("0xdeadbeef".into()),
        );

        let tracker = TxTracker::new(TxTrackerConfig::default());

        let hash = send_and_track(
            &transport,
            &tracker,
            "0xraw_data",
            "0xAlice",
            42,
        )
        .await
        .expect("send_and_track should succeed");

        assert_eq!(hash, "0xdeadbeef");
        assert_eq!(tracker.count(), 1);

        let tracked = tracker.get("0xdeadbeef").expect("tx should be tracked");
        assert_eq!(tracked.from, "0xAlice");
        assert_eq!(tracked.nonce, 42);
        assert_eq!(tracked.status, TxStatus::Pending);
    }

    #[tokio::test]
    async fn detect_stuck_returns_old_txs() {
        let transport = MockTransport::new();
        // Return a nonce as a hex string for eth_getTransactionCount.
        transport.set_response(
            "eth_getTransactionCount",
            Value::String("0x5".into()),
        );

        let config = TxTrackerConfig {
            stuck_timeout_secs: 60,
            ..Default::default()
        };
        let tracker = TxTracker::new(config);

        // Submit a transaction at time 1000.
        let old_tx = TrackedTx {
            tx_hash: "0xold".to_string(),
            from: "0xAlice".to_string(),
            nonce: 3,
            submitted_at: 1000,
            status: TxStatus::Pending,
            gas_price: Some(20_000_000_000),
            max_fee: None,
            max_priority_fee: None,
            last_checked: 1000,
        };
        tracker.track(old_tx);

        // Submit a recent transaction at time 1090.
        let new_tx = TrackedTx {
            tx_hash: "0xnew".to_string(),
            from: "0xAlice".to_string(),
            nonce: 4,
            submitted_at: 1090,
            status: TxStatus::Pending,
            gas_price: Some(20_000_000_000),
            max_fee: None,
            max_priority_fee: None,
            last_checked: 1090,
        };
        tracker.track(new_tx);

        // At time 1061, only 0xold is stuck (61s > 60s timeout),
        // 0xnew is not (only 0s > 60s timeout would be false since 1090-1061 is negative).
        // We use current_time = 1100 so that 0xold (100s) is stuck and 0xnew (10s) is not.
        let stuck = detect_stuck(&transport, &tracker, 1100).await;
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].tx_hash, "0xold");

        // The nonce tracker should have been updated from the transport response.
        assert_eq!(tracker.next_nonce("0xAlice"), Some(6)); // 0x5 = 5, next = 6
    }
}
