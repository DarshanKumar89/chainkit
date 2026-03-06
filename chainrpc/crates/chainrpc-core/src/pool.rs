//! Multi-provider failover pool with round-robin selection and health tracking.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::error::TransportError;
use crate::metrics::ProviderMetrics;
use crate::policy::{CircuitBreaker, CircuitBreakerConfig};
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

/// Configuration for the provider pool.
#[derive(Debug, Clone)]
pub struct ProviderPoolConfig {
    /// Circuit breaker config shared across all providers.
    pub circuit_breaker: CircuitBreakerConfig,
    /// Timeout per individual request.
    pub request_timeout: Duration,
}

impl Default for ProviderPoolConfig {
    fn default() -> Self {
        Self {
            circuit_breaker: CircuitBreakerConfig::default(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

struct ProviderSlot {
    transport: Arc<dyn RpcTransport>,
    circuit: CircuitBreaker,
    metrics: Option<Arc<ProviderMetrics>>,
}

/// Round-robin provider pool with per-provider circuit breakers.
///
/// Automatically skips unhealthy (circuit-open) providers and falls
/// back to the next available one.
pub struct ProviderPool {
    slots: Vec<ProviderSlot>,
    cursor: AtomicUsize,
    config: ProviderPoolConfig,
}

impl ProviderPool {
    /// Build a pool from a list of transports.
    pub fn new(transports: Vec<Arc<dyn RpcTransport>>, config: ProviderPoolConfig) -> Self {
        let slots = transports
            .into_iter()
            .map(|t| ProviderSlot {
                transport: t,
                circuit: CircuitBreaker::new(config.circuit_breaker.clone()),
                metrics: None,
            })
            .collect();
        Self {
            slots,
            cursor: AtomicUsize::new(0),
            config,
        }
    }

    /// Build a pool with per-provider metrics automatically created.
    pub fn new_with_metrics(
        transports: Vec<Arc<dyn RpcTransport>>,
        config: ProviderPoolConfig,
    ) -> Self {
        let slots = transports
            .into_iter()
            .map(|t| {
                let m = Arc::new(ProviderMetrics::new(t.url()));
                ProviderSlot {
                    transport: t,
                    circuit: CircuitBreaker::new(config.circuit_breaker.clone()),
                    metrics: Some(m),
                }
            })
            .collect();
        Self {
            slots,
            cursor: AtomicUsize::new(0),
            config,
        }
    }

    /// Number of providers in the pool.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Returns `true` if the pool has no providers.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Returns summary of each provider's health.
    pub fn health_summary(&self) -> Vec<(String, HealthStatus, String)> {
        self.slots
            .iter()
            .map(|s| {
                let url = s.transport.url().to_string();
                let health = s.transport.health();
                let circuit = s.circuit.state().to_string();
                (url, health, circuit)
            })
            .collect()
    }

    /// Number of providers whose circuit breaker allows requests.
    pub fn healthy_count(&self) -> usize {
        self.slots.iter().filter(|s| s.circuit.is_allowed()).count()
    }

    /// Return metrics snapshots for all providers that have metrics enabled.
    pub fn metrics(&self) -> Vec<crate::metrics::MetricsSnapshot> {
        self.slots
            .iter()
            .filter_map(|s| s.metrics.as_ref().map(|m| m.snapshot()))
            .collect()
    }

    /// Detailed health report for each provider as JSON-serializable values.
    ///
    /// When per-provider metrics are available the report includes
    /// additional fields such as `total_requests`, `success_rate`, and
    /// `avg_latency_ms`.
    pub fn health_report(&self) -> Vec<serde_json::Value> {
        self.slots
            .iter()
            .map(|s| {
                let mut report = serde_json::json!({
                    "url": s.transport.url(),
                    "health": s.transport.health().to_string(),
                    "circuit": s.circuit.state().to_string(),
                });
                if let Some(ref m) = s.metrics {
                    let snap = m.snapshot();
                    let obj = report.as_object_mut().unwrap();
                    obj.insert(
                        "total_requests".into(),
                        serde_json::json!(snap.total_requests),
                    );
                    obj.insert(
                        "successful_requests".into(),
                        serde_json::json!(snap.successful_requests),
                    );
                    obj.insert(
                        "failed_requests".into(),
                        serde_json::json!(snap.failed_requests),
                    );
                    obj.insert("success_rate".into(), serde_json::json!(snap.success_rate));
                    obj.insert(
                        "avg_latency_ms".into(),
                        serde_json::json!(snap.avg_latency_ms),
                    );
                    obj.insert(
                        "rate_limit_hits".into(),
                        serde_json::json!(snap.rate_limit_hits),
                    );
                    obj.insert(
                        "circuit_open_count".into(),
                        serde_json::json!(snap.circuit_open_count),
                    );
                }
                report
            })
            .collect()
    }

    /// Find the next available (circuit-closed/half-open) slot, starting
    /// from the round-robin cursor.
    fn next_slot(&self) -> Option<&ProviderSlot> {
        if self.slots.is_empty() {
            return None;
        }
        let start = self.cursor.fetch_add(1, Ordering::Relaxed) % self.slots.len();
        for i in 0..self.slots.len() {
            let idx = (start + i) % self.slots.len();
            let slot = &self.slots[idx];
            if slot.circuit.is_allowed() {
                return Some(slot);
            }
        }
        None
    }
}

#[async_trait]
impl RpcTransport for ProviderPool {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        let slot = self.next_slot().ok_or(TransportError::AllProvidersDown)?;

        let timeout = self.config.request_timeout;
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(timeout, slot.transport.send(req))
            .await
            .map_err(|_| TransportError::Timeout {
                ms: timeout.as_millis() as u64,
            })?;

        match result {
            Ok(resp) => {
                slot.circuit.record_success();
                if let Some(ref m) = slot.metrics {
                    m.record_success(start.elapsed());
                }
                Ok(resp)
            }
            Err(e) if e.is_retryable() => {
                slot.circuit.record_failure();
                if let Some(ref m) = slot.metrics {
                    m.record_failure();
                }
                Err(e)
            }
            Err(e) => {
                if let Some(ref m) = slot.metrics {
                    m.record_failure();
                }
                Err(e)
            }
        }
    }

    fn health(&self) -> HealthStatus {
        let healthy_count = self.slots.iter().filter(|s| s.circuit.is_allowed()).count();
        match healthy_count {
            0 => HealthStatus::Unhealthy,
            n if n == self.slots.len() => HealthStatus::Healthy,
            _ => HealthStatus::Degraded,
        }
    }

    fn url(&self) -> &str {
        "pool"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;

    struct MockTransport {
        url: String,
        should_fail: bool,
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            if self.should_fail {
                Err(TransportError::Http("mock error".into()))
            } else {
                Ok(JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: RpcId::Number(1),
                    result: Some(serde_json::Value::String("0x1".into())),
                    error: None,
                })
            }
        }
        fn url(&self) -> &str {
            &self.url
        }
    }

    fn mock(url: &str, fail: bool) -> Arc<dyn RpcTransport> {
        Arc::new(MockTransport {
            url: url.to_string(),
            should_fail: fail,
        })
    }

    #[test]
    fn pool_len() {
        let pool = ProviderPool::new(
            vec![mock("https://a.com", false), mock("https://b.com", false)],
            ProviderPoolConfig::default(),
        );
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn health_all_healthy() {
        let pool = ProviderPool::new(
            vec![mock("https://a.com", false)],
            ProviderPoolConfig::default(),
        );
        assert_eq!(pool.health(), HealthStatus::Healthy);
    }

    #[test]
    fn health_all_down() {
        let pool = ProviderPool::new(vec![], ProviderPoolConfig::default());
        // No providers → AllProvidersDown (next_slot returns None)
        assert!(pool.next_slot().is_none());
    }
}
