//! chainrpc-http — HTTP JSON-RPC transport with reliability features.

use std::sync::Arc;

use chainrpc_core::error::TransportError;
use chainrpc_core::pool::{ProviderPool, ProviderPoolConfig};
use chainrpc_core::transport::RpcTransport;

pub mod batch;
pub mod client;

pub use client::{HttpClientConfig, HttpRpcClient};

/// Create a `ProviderPool` from a list of HTTP endpoint URLs.
///
/// Each URL gets an `HttpRpcClient` with default configuration (retry, circuit breaker, rate limiter).
pub fn pool_from_urls(urls: &[&str]) -> Result<ProviderPool, TransportError> {
    if urls.is_empty() {
        return Err(TransportError::Http("no URLs provided".into()));
    }
    let transports: Vec<Arc<dyn RpcTransport>> = urls
        .iter()
        .map(|url| Arc::new(HttpRpcClient::default_for(*url)) as Arc<dyn RpcTransport>)
        .collect();
    Ok(ProviderPool::new(transports, ProviderPoolConfig::default()))
}
