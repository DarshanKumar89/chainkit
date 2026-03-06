//! Backpressure transport wrapper — limits concurrent in-flight requests.
//!
//! When the queue is full, returns `TransportError::Overloaded` immediately
//! instead of queueing unboundedly and risking OOM.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Semaphore;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

/// Configuration for backpressure.
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// Maximum number of concurrent in-flight requests.
    pub max_in_flight: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            max_in_flight: 1000,
        }
    }
}

/// A transport wrapper that limits concurrent in-flight requests.
///
/// If `max_in_flight` requests are already pending, new requests
/// immediately fail with `TransportError::Overloaded`.
pub struct BackpressureTransport {
    inner: Arc<dyn RpcTransport>,
    semaphore: Semaphore,
    max_in_flight: usize,
}

impl BackpressureTransport {
    /// Create a new backpressure wrapper.
    pub fn new(inner: Arc<dyn RpcTransport>, config: BackpressureConfig) -> Self {
        Self {
            inner,
            semaphore: Semaphore::new(config.max_in_flight),
            max_in_flight: config.max_in_flight,
        }
    }

    /// Current number of in-flight requests.
    pub fn in_flight(&self) -> usize {
        self.max_in_flight - self.semaphore.available_permits()
    }

    /// Whether the transport is at capacity.
    pub fn is_full(&self) -> bool {
        self.semaphore.available_permits() == 0
    }
}

#[async_trait]
impl RpcTransport for BackpressureTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        let permit = self
            .semaphore
            .try_acquire()
            .map_err(|_| TransportError::Overloaded {
                queue_depth: self.max_in_flight,
            })?;

        let result = self.inner.send(req).await;
        drop(permit);
        result
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

    struct SlowTransport;

    #[async_trait]
    impl RpcTransport for SlowTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::json!("0x1")),
                error: None,
            })
        }
        fn url(&self) -> &str {
            "mock://slow"
        }
    }

    struct InstantTransport;

    #[async_trait]
    impl RpcTransport for InstantTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::json!("0x1")),
                error: None,
            })
        }
        fn url(&self) -> &str {
            "mock://instant"
        }
    }

    #[tokio::test]
    async fn allows_requests_under_limit() {
        let transport = BackpressureTransport::new(
            Arc::new(InstantTransport),
            BackpressureConfig { max_in_flight: 10 },
        );

        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let result = transport.send(req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn rejects_when_full() {
        let transport = Arc::new(BackpressureTransport::new(
            Arc::new(SlowTransport),
            BackpressureConfig { max_in_flight: 2 },
        ));

        // Fill up the slots
        let t1 = transport.clone();
        let t2 = transport.clone();
        let _h1 = tokio::spawn(async move {
            let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
            let _ = t1.send(req).await;
        });
        let _h2 = tokio::spawn(async move {
            let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
            let _ = t2.send(req).await;
        });

        // Give spawned tasks time to acquire permits
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Third request should be rejected
        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let result = transport.send(req).await;
        assert!(matches!(result, Err(TransportError::Overloaded { .. })));
    }

    #[tokio::test]
    async fn in_flight_tracking() {
        let transport = BackpressureTransport::new(
            Arc::new(InstantTransport),
            BackpressureConfig { max_in_flight: 100 },
        );

        assert_eq!(transport.in_flight(), 0);
        assert!(!transport.is_full());
    }
}
