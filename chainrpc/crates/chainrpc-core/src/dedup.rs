//! Request deduplication — coalesces identical in-flight RPC requests.
//!
//! When multiple callers issue the same request concurrently, only one actual
//! transport call is made.  All waiters receive a clone of the result.
//!
//! # Key design
//!
//! - A request is identified by `hash(method, params)` (same scheme as cache).
//! - In-flight tracking uses `tokio::sync::watch` channels: the first caller
//!   creates a channel and sends the result; subsequent callers subscribe.
//! - After the result is broadcast, the entry is removed from the pending map.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::RpcTransport;

// ---------------------------------------------------------------------------
// DedupTransport
// ---------------------------------------------------------------------------

/// Deduplicates identical in-flight RPC requests.
///
/// If two tasks call `send()` with the same `(method, params)` at the same
/// time, only one transport call is made.  Both tasks receive a clone of the
/// response (or the same error message).
pub struct DedupTransport {
    inner: Arc<dyn RpcTransport>,
    /// Map from request-key to a watch receiver.
    ///
    /// The channel starts with `None` and is set to `Some(result)` once the
    /// in-flight request completes.
    pending: Mutex<HashMap<u64, watch::Receiver<Option<DedupResult>>>>,
}

/// The result type stored inside the watch channel.
///
/// We cannot clone `TransportError` (it doesn't derive Clone), so we
/// represent errors as a string and re-wrap them on the receiving side.
type DedupResult = Result<JsonRpcResponse, String>;

impl DedupTransport {
    /// Wrap an inner transport with request deduplication.
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self {
            inner,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Send a request, deduplicating identical in-flight requests.
    pub async fn send(
        &self,
        req: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, TransportError> {
        let key = dedup_key(&req.method, &req.params);

        // Fast path: check if there is already an in-flight request.
        // We extract the receiver (if any) while holding the lock, then
        // drop the lock before awaiting.
        let existing_rx = {
            let pending = self.pending.lock().unwrap();
            pending.get(&key).cloned()
        };

        if let Some(mut rx) = existing_rx {
            return self.wait_for_result(&mut rx).await;
        }

        // Slow path: we are the first caller for this key.
        let (tx, rx) = watch::channel(None);

        // Double-check under the write lock: another task may have inserted
        // between our read and this write.
        let coalesce_rx = {
            let mut pending = self.pending.lock().unwrap();
            if let Some(existing) = pending.get(&key) {
                Some(existing.clone())
            } else {
                pending.insert(key, rx);
                None
            }
        };

        if let Some(mut rx) = coalesce_rx {
            return self.wait_for_result(&mut rx).await;
        }

        // Perform the actual request.
        let result = self.inner.send(req).await;

        // Broadcast the result to all waiters.
        let dedup_result: DedupResult = match &result {
            Ok(resp) => Ok(resp.clone()),
            Err(e) => Err(e.to_string()),
        };
        // Ignore send errors (no receivers left).
        let _ = tx.send(Some(dedup_result));

        // Clean up the pending map.
        {
            let mut pending = self.pending.lock().unwrap();
            pending.remove(&key);
        }

        tracing::debug!("dedup: completed request (key={key:#018x})");
        result
    }

    /// Number of currently in-flight deduplicated requests.
    pub fn in_flight_count(&self) -> usize {
        let pending = self.pending.lock().unwrap();
        pending.len()
    }

    // -- internal -----------------------------------------------------------

    async fn wait_for_result(
        &self,
        rx: &mut watch::Receiver<Option<DedupResult>>,
    ) -> Result<JsonRpcResponse, TransportError> {
        // Wait until the value changes from `None` to `Some(...)`.
        loop {
            // Check the current value first.
            {
                let val = rx.borrow();
                if let Some(ref result) = *val {
                    tracing::debug!("dedup: coalesced request");
                    return match result {
                        Ok(resp) => Ok(resp.clone()),
                        Err(msg) => Err(TransportError::Other(msg.clone())),
                    };
                }
            }

            // Wait for the next change.
            if rx.changed().await.is_err() {
                // Sender dropped without sending — should not happen.
                return Err(TransportError::Other(
                    "dedup: sender dropped without result".into(),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hashing helper
// ---------------------------------------------------------------------------

fn dedup_key(method: &str, params: &[serde_json::Value]) -> u64 {
    let mut hasher = DefaultHasher::new();
    method.hash(&mut hasher);
    let params_str = serde_json::to_string(params).unwrap_or_default();
    params_str.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A mock transport that counts calls and has an optional delay.
    struct SlowCountingTransport {
        call_count: AtomicU64,
        delay: std::time::Duration,
    }

    impl SlowCountingTransport {
        fn new(delay: std::time::Duration) -> Self {
            Self {
                call_count: AtomicU64::new(0),
                delay,
            }
        }

        fn calls(&self) -> u64 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RpcTransport for SlowCountingTransport {
        async fn send(
            &self,
            _req: JsonRpcRequest,
        ) -> Result<JsonRpcResponse, TransportError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::Value::String("0x1".into())),
                error: None,
            })
        }

        fn url(&self) -> &str {
            "mock://slow"
        }
    }

    fn make_req(method: &str) -> JsonRpcRequest {
        JsonRpcRequest::new(1, method, vec![])
    }

    #[tokio::test]
    async fn two_concurrent_identical_requests_trigger_one_send() {
        let transport = Arc::new(SlowCountingTransport::new(
            std::time::Duration::from_millis(100),
        ));
        let dedup = Arc::new(DedupTransport::new(transport.clone()));

        let d1 = dedup.clone();
        let d2 = dedup.clone();

        let (r1, r2) = tokio::join!(
            tokio::spawn(async move { d1.send(make_req("eth_chainId")).await }),
            tokio::spawn(async move { d2.send(make_req("eth_chainId")).await }),
        );

        assert!(r1.unwrap().is_ok());
        assert!(r2.unwrap().is_ok());
        // Only one actual call to the inner transport.
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn different_requests_go_through_independently() {
        let transport = Arc::new(SlowCountingTransport::new(
            std::time::Duration::from_millis(50),
        ));
        let dedup = Arc::new(DedupTransport::new(transport.clone()));

        let d1 = dedup.clone();
        let d2 = dedup.clone();

        let (r1, r2) = tokio::join!(
            tokio::spawn(async move { d1.send(make_req("eth_chainId")).await }),
            tokio::spawn(async move { d2.send(make_req("net_version")).await }),
        );

        assert!(r1.unwrap().is_ok());
        assert!(r2.unwrap().is_ok());
        // Two different methods = two transport calls.
        assert_eq!(transport.calls(), 2);
    }

    #[tokio::test]
    async fn cleanup_after_completion() {
        let transport = Arc::new(SlowCountingTransport::new(
            std::time::Duration::from_millis(10),
        ));
        let dedup = DedupTransport::new(transport.clone());

        dedup.send(make_req("eth_chainId")).await.unwrap();
        // After completion the pending map should be empty.
        assert_eq!(dedup.in_flight_count(), 0);
    }

    #[tokio::test]
    async fn sequential_same_requests_both_go_through() {
        let transport = Arc::new(SlowCountingTransport::new(
            std::time::Duration::from_millis(1),
        ));
        let dedup = DedupTransport::new(transport.clone());

        // Sequential (not concurrent) same requests should each hit transport.
        dedup.send(make_req("eth_chainId")).await.unwrap();
        dedup.send(make_req("eth_chainId")).await.unwrap();
        assert_eq!(transport.calls(), 2);
    }

    #[test]
    fn dedup_key_deterministic() {
        let k1 = dedup_key("eth_chainId", &[]);
        let k2 = dedup_key("eth_chainId", &[]);
        assert_eq!(k1, k2);
    }

    #[test]
    fn dedup_key_differs_by_method() {
        let k1 = dedup_key("eth_chainId", &[]);
        let k2 = dedup_key("net_version", &[]);
        assert_ne!(k1, k2);
    }
}
