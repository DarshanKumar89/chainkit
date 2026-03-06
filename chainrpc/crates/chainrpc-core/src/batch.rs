//! Transport-agnostic auto-batching engine.
//!
//! Coalesces multiple `JsonRpcRequest`s arriving within a time window and
//! flushes them as a single batch call. Each caller gets their response back
//! via a `oneshot` channel.
//!
//! # Usage
//! ```rust,no_run
//! use chainrpc_core::batch::BatchingTransport;
//! use chainrpc_core::transport::RpcTransport;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! fn example(inner: Arc<dyn RpcTransport>) {
//!     let batcher = BatchingTransport::new(inner, Duration::from_millis(5));
//! }
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use tokio::time;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

type ResponseSender = oneshot::Sender<Result<JsonRpcResponse, TransportError>>;

struct BatchItem {
    req: JsonRpcRequest,
    tx: ResponseSender,
}

/// Auto-batching transport wrapper.
///
/// Sends a flush task in the background that groups pending requests into
/// a single batch call every `window` milliseconds.
pub struct BatchingTransport {
    inner: Arc<dyn RpcTransport>,
    tx: mpsc::UnboundedSender<BatchItem>,
    #[allow(dead_code)]
    window: Duration,
}

impl BatchingTransport {
    /// Create a new batching transport wrapping `inner`.
    pub fn new(inner: Arc<dyn RpcTransport>, window: Duration) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel::<BatchItem>();
        let batcher = Arc::new(Self {
            inner: inner.clone(),
            tx,
            window,
        });

        // Spawn background flush task
        let flush_inner = inner;
        let flush_window = window;
        tokio::spawn(async move {
            flush_loop(rx, flush_inner, flush_window).await;
        });

        batcher
    }
}

async fn flush_loop(
    mut rx: mpsc::UnboundedReceiver<BatchItem>,
    transport: Arc<dyn RpcTransport>,
    window: Duration,
) {
    loop {
        // Wait for the first item
        let first = match rx.recv().await {
            Some(item) => item,
            None => break, // channel closed
        };

        let mut batch = vec![first];

        // Collect all items that arrive within the window
        let deadline = time::sleep(window);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => break,
                item = rx.recv() => {
                    match item {
                        Some(i) => batch.push(i),
                        None => break,
                    }
                }
            }
        }

        if batch.len() == 1 {
            // Single item — skip batch overhead
            let item = batch.remove(0);
            let result = transport.send(item.req).await;
            let _ = item.tx.send(result);
        } else {
            // True batch
            let reqs: Vec<JsonRpcRequest> = batch.iter().map(|b| b.req.clone()).collect();
            match transport.send_batch(reqs).await {
                Ok(responses) => {
                    // Match responses to senders by position
                    for (item, resp) in batch.into_iter().zip(responses.into_iter()) {
                        let _ = item.tx.send(Ok(resp));
                    }
                }
                Err(e) => {
                    // Broadcast error to all callers
                    let msg = e.to_string();
                    for item in batch {
                        let _ = item.tx.send(Err(TransportError::Http(msg.clone())));
                    }
                }
            }
        }
    }
}

#[async_trait]
impl RpcTransport for BatchingTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(BatchItem { req, tx })
            .map_err(|_| TransportError::Other("batcher channel closed".into()))?;
        rx.await
            .map_err(|_| TransportError::Other("batcher task dropped".into()))?
    }

    fn health(&self) -> HealthStatus {
        self.inner.health()
    }

    fn url(&self) -> &str {
        self.inner.url()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingTransport {
        send_count: AtomicU64,
        batch_count: AtomicU64,
    }

    impl CountingTransport {
        fn new() -> Self {
            Self {
                send_count: AtomicU64::new(0),
                batch_count: AtomicU64::new(0),
            }
        }
    }

    #[async_trait]
    impl RpcTransport for CountingTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::json!("0x1")),
                error: None,
            })
        }

        async fn send_batch(
            &self,
            reqs: Vec<JsonRpcRequest>,
        ) -> Result<Vec<JsonRpcResponse>, TransportError> {
            self.batch_count.fetch_add(1, Ordering::SeqCst);
            Ok(reqs
                .iter()
                .map(|r| JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: r.id.clone(),
                    result: Some(serde_json::json!("0x1")),
                    error: None,
                })
                .collect())
        }

        fn url(&self) -> &str {
            "mock://counting"
        }
    }

    #[tokio::test]
    async fn single_request_bypasses_batch() {
        let inner = Arc::new(CountingTransport::new());
        let batcher = BatchingTransport::new(inner.clone(), Duration::from_millis(50));

        let req = JsonRpcRequest::new(1, "eth_blockNumber", vec![]);
        let resp = batcher.send(req).await.unwrap();
        assert!(resp.result.is_some());

        // Wait for flush
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(inner.send_count.load(Ordering::SeqCst), 1);
        assert_eq!(inner.batch_count.load(Ordering::SeqCst), 0);
    }
}
