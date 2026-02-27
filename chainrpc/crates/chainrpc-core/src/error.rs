//! Transport-level error types.

use thiserror::Error;

use crate::request::JsonRpcError;

/// Errors that can occur during an RPC transport operation.
#[derive(Debug, Error)]
pub enum TransportError {
    /// HTTP request failed (connection refused, timeout, etc.).
    #[error("HTTP error: {0}")]
    Http(String),

    /// WebSocket connection/send/receive error.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// JSON-RPC protocol-level error returned by the node.
    #[error("RPC error {}: {}", .0.code, .0.message)]
    Rpc(JsonRpcError),

    /// Rate limit exceeded — caller should back off.
    #[error("Rate limit exceeded (provider: {provider})")]
    RateLimited { provider: String },

    /// Circuit breaker is open — provider is unhealthy.
    #[error("Circuit breaker open for provider: {provider}")]
    CircuitOpen { provider: String },

    /// All providers in the pool are unavailable.
    #[error("All providers unavailable")]
    AllProvidersDown,

    /// Request timed out after the configured duration.
    #[error("Request timed out after {ms}ms")]
    Timeout { ms: u64 },

    /// Response could not be deserialized.
    #[error("Deserialization error: {0}")]
    Deserialization(#[from] serde_json::Error),

    /// An unexpected error.
    #[error("{0}")]
    Other(String),
}

impl TransportError {
    /// Returns `true` if this error is retryable (transient).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Http(_)
                | Self::WebSocket(_)
                | Self::Timeout { .. }
                | Self::RateLimited { .. }
        )
    }

    /// Returns `true` if this is a node-side execution error (not retryable).
    pub fn is_execution_error(&self) -> bool {
        matches!(self, Self::Rpc(_))
    }
}
