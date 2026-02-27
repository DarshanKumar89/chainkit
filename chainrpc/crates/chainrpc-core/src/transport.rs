//! The `RpcTransport` trait — the core abstraction for all RPC providers.

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};

/// Provider health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Provider is responding normally.
    Healthy,
    /// Provider is responding but degraded (high latency, partial errors).
    Degraded,
    /// Provider is not responding (circuit open).
    Unhealthy,
    /// Health status is unknown (not yet checked).
    Unknown,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Degraded => write!(f, "degraded"),
            Self::Unhealthy => write!(f, "unhealthy"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// The central async trait every RPC transport must implement.
///
/// # Thread Safety
/// Implementations must be `Send + Sync` for use across Tokio tasks.
///
/// # Object Safety
/// The trait is object-safe and can be stored as `Arc<dyn RpcTransport>`.
#[async_trait]
pub trait RpcTransport: Send + Sync + 'static {
    /// Send a single JSON-RPC request and return the response.
    async fn send(
        &self,
        req: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, TransportError>;

    /// Send a batch of JSON-RPC requests.
    ///
    /// Default implementation sends them sequentially; override for true batching.
    async fn send_batch(
        &self,
        reqs: Vec<JsonRpcRequest>,
    ) -> Result<Vec<JsonRpcResponse>, TransportError> {
        let mut responses = Vec::with_capacity(reqs.len());
        for req in reqs {
            responses.push(self.send(req).await?);
        }
        Ok(responses)
    }

    /// Return the current health status of this transport.
    fn health(&self) -> HealthStatus {
        HealthStatus::Unknown
    }

    /// Return the transport's identifier (URL or name).
    fn url(&self) -> &str;

    /// Convenience: call a method and deserialize the result.
    async fn call<T: DeserializeOwned>(
        &self,
        id: u64,
        method: &str,
        params: Vec<Value>,
    ) -> Result<T, TransportError> {
        let req = JsonRpcRequest::new(id, method, params);
        let resp = self.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        serde_json::from_value(result).map_err(TransportError::Deserialization)
    }
}
