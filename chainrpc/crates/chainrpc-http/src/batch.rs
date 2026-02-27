//! Auto-batching engine: coalesce multiple requests within a time window.
//!
//! The batcher collects `JsonRpcRequest`s arriving within `batch_window` and
//! flushes them as a single HTTP batch request. Each caller gets their response
//! back via a `oneshot` channel.
//!
//! # Usage
//! ```rust,no_run
//! use chainrpc_http::batch::BatchingTransport;
//! use chainrpc_http::HttpRpcClient;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let client = Arc::new(HttpRpcClient::default_for("https://rpc.example.com"));
//! let batcher = BatchingTransport::new(client, Duration::from_millis(5));
//! ```

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::future;
use tokio::sync::{mpsc, oneshot};
use tokio::time;

use chainrpc_core::error::TransportError;
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
use chainrpc_core::transport::{HealthStatus, RpcTransport};

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
