//! Background health checker — periodically probes providers.
//!
//! Unlike reactive health tracking (from actual requests), this actively
//! polls providers to detect recovery from outages.

use std::sync::Arc;
use std::time::Duration;

use crate::request::JsonRpcRequest;
use crate::transport::RpcTransport;

/// Configuration for the background health checker.
#[derive(Debug, Clone)]
pub struct HealthCheckerConfig {
    /// How often to check each provider.
    pub interval: Duration,
    /// Which RPC method to use as a health probe.
    pub probe_method: String,
    /// Timeout for the health probe.
    pub probe_timeout: Duration,
}

impl Default for HealthCheckerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            probe_method: "eth_blockNumber".to_string(),
            probe_timeout: Duration::from_secs(5),
        }
    }
}

/// Result of a health probe.
#[derive(Debug, Clone)]
pub struct ProbeResult {
    /// Provider URL.
    pub url: String,
    /// Whether the probe succeeded.
    pub success: bool,
    /// Probe latency (None if failed).
    pub latency: Option<Duration>,
    /// Error message if probe failed.
    pub error: Option<String>,
}

/// Run a single health probe against a transport.
pub async fn probe_provider(
    transport: &dyn RpcTransport,
    method: &str,
    timeout: Duration,
) -> ProbeResult {
    let req = JsonRpcRequest::auto(method, vec![]);
    let start = std::time::Instant::now();

    let result = tokio::time::timeout(timeout, transport.send(req)).await;

    match result {
        Ok(Ok(resp)) => ProbeResult {
            url: transport.url().to_string(),
            success: resp.is_ok(),
            latency: Some(start.elapsed()),
            error: resp.error.map(|e| e.message),
        },
        Ok(Err(e)) => ProbeResult {
            url: transport.url().to_string(),
            success: false,
            latency: Some(start.elapsed()),
            error: Some(e.to_string()),
        },
        Err(_) => ProbeResult {
            url: transport.url().to_string(),
            success: false,
            latency: None,
            error: Some(format!("probe timed out after {}ms", timeout.as_millis())),
        },
    }
}

/// Callback type for probe results.
pub type ProbeCallback = Box<dyn Fn(ProbeResult) + Send + Sync + 'static>;

/// Start a background health check loop for multiple providers.
///
/// Returns a `JoinHandle` that can be aborted to stop health checking.
/// The `on_result` callback is called after each probe with the result.
pub fn start_health_checker(
    providers: Vec<Arc<dyn RpcTransport>>,
    config: HealthCheckerConfig,
    on_result: Option<ProbeCallback>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(config.interval);
        loop {
            interval.tick().await;

            for provider in &providers {
                let result = probe_provider(
                    provider.as_ref(),
                    &config.probe_method,
                    config.probe_timeout,
                )
                .await;

                tracing::debug!(
                    url = %result.url,
                    success = result.success,
                    latency_ms = result.latency.map(|d| d.as_millis() as u64),
                    "health probe"
                );

                if let Some(ref cb) = on_result {
                    cb(result);
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TransportError;
    use crate::request::{JsonRpcResponse, RpcId};
    use async_trait::async_trait;

    struct OkTransport;

    #[async_trait]
    impl RpcTransport for OkTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::Value::String("0x1".into())),
                error: None,
            })
        }
        fn url(&self) -> &str {
            "mock://ok"
        }
    }

    struct FailTransport;

    #[async_trait]
    impl RpcTransport for FailTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Err(TransportError::Http("connection refused".into()))
        }
        fn url(&self) -> &str {
            "mock://fail"
        }
    }

    struct SlowTransport;

    #[async_trait]
    impl RpcTransport for SlowTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            tokio::time::sleep(Duration::from_secs(10)).await;
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

    #[tokio::test]
    async fn probe_healthy_provider() {
        let transport = OkTransport;
        let result = probe_provider(&transport, "eth_blockNumber", Duration::from_secs(5)).await;

        assert!(result.success);
        assert!(result.latency.is_some());
        assert!(result.error.is_none());
        assert_eq!(result.url, "mock://ok");
    }

    #[tokio::test]
    async fn probe_failed_provider() {
        let transport = FailTransport;
        let result = probe_provider(&transport, "eth_blockNumber", Duration::from_secs(5)).await;

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("connection refused"));
    }

    #[tokio::test]
    async fn probe_timeout() {
        let transport = SlowTransport;
        let result = probe_provider(
            &transport,
            "eth_blockNumber",
            Duration::from_millis(50), // very short timeout
        )
        .await;

        assert!(!result.success);
        assert!(result.latency.is_none());
        assert!(result.error.unwrap().contains("timed out"));
    }
}
