//! HTTP JSON-RPC client backed by `reqwest`.
//!
//! Features:
//! - Automatic retry with exponential backoff for transient errors
//! - Circuit breaker per provider
//! - Rate limiter (token bucket)
//! - Batch request support (true HTTP batching)

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

use chainrpc_core::error::TransportError;
use chainrpc_core::metrics::ProviderMetrics;
use chainrpc_core::policy::{
    CircuitBreaker, CircuitBreakerConfig, RateLimiter, RateLimiterConfig, RetryConfig, RetryPolicy,
};
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse};
use chainrpc_core::transport::{HealthStatus, RpcTransport};

/// Configuration for `HttpRpcClient`.
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    pub retry: RetryConfig,
    pub circuit_breaker: CircuitBreakerConfig,
    pub rate_limiter: RateLimiterConfig,
    pub request_timeout: Duration,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            retry: RetryConfig::default(),
            circuit_breaker: CircuitBreakerConfig::default(),
            rate_limiter: RateLimiterConfig::default(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// HTTP JSON-RPC client with built-in reliability features.
pub struct HttpRpcClient {
    url: String,
    http: reqwest::Client,
    retry: RetryPolicy,
    circuit: CircuitBreaker,
    rate_limiter: RateLimiter,
    request_timeout: Duration,
    metrics: Option<Arc<ProviderMetrics>>,
    /// Adaptive rate limit state from response headers.
    adaptive_remaining: std::sync::atomic::AtomicU32,
}

impl HttpRpcClient {
    /// Create a new client for the given JSON-RPC endpoint URL.
    pub fn new(url: impl Into<String>, config: HttpClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .build()
            .expect("failed to build reqwest client");

        Self {
            url: url.into(),
            http,
            retry: RetryPolicy::new(config.retry),
            circuit: CircuitBreaker::new(config.circuit_breaker),
            rate_limiter: RateLimiter::new(config.rate_limiter),
            request_timeout: config.request_timeout,
            metrics: None,
            adaptive_remaining: std::sync::atomic::AtomicU32::new(u32::MAX),
        }
    }

    /// Create a new client with metrics recording enabled.
    pub fn with_metrics(
        url: impl Into<String>,
        config: HttpClientConfig,
        metrics: Arc<ProviderMetrics>,
    ) -> Self {
        let mut client = Self::new(url, config);
        client.metrics = Some(metrics);
        client
    }

    /// Create with default configuration.
    pub fn default_for(url: impl Into<String>) -> Self {
        Self::new(url, HttpClientConfig::default())
    }

    async fn send_once(&self, req: &JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        let resp = self
            .http
            .post(&self.url)
            .json(req)
            .send()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))?;

        // Parse rate limit headers from response
        let rl_info = chainrpc_core::rate_limit_headers::RateLimitInfo::from_headers(
            resp.headers().iter().map(|(k, v)| {
                (k.as_str(), v.to_str().unwrap_or(""))
            }),
        );
        if let Some(remaining) = rl_info.remaining {
            self.adaptive_remaining
                .store(remaining, std::sync::atomic::Ordering::Relaxed);
        }

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if let Some(ref m) = self.metrics {
                m.record_rate_limit();
            }
            let _wait = rl_info.retry_after.unwrap_or(std::time::Duration::from_secs(1));
            return Err(TransportError::RateLimited {
                provider: self.url.clone(),
            });
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(TransportError::Http(format!(
                "HTTP {status}: {body}"
            )));
        }

        resp.json::<JsonRpcResponse>()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))
    }
}

#[async_trait]
impl RpcTransport for HttpRpcClient {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Rate limiter check
        if !self.rate_limiter.try_acquire() {
            if let Some(ref m) = self.metrics { m.record_rate_limit(); }
            let wait = self.rate_limiter.wait_time();
            tracing::debug!(wait_ms = wait.as_millis(), "rate limited — backing off");
            tokio::time::sleep(wait).await;
        }

        // Circuit breaker check
        if !self.circuit.is_allowed() {
            if let Some(ref m) = self.metrics { m.record_circuit_open(); }
            return Err(TransportError::CircuitOpen {
                provider: self.url.clone(),
            });
        }

        let start = std::time::Instant::now();

        // Method safety classification — only Safe methods are auto-retried.
        let safety = chainrpc_core::method_safety::classify_method(&req.method);

        // Retry loop
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.send_once(&req).await {
                Ok(resp) => {
                    self.circuit.record_success();
                    if let Some(ref m) = self.metrics { m.record_success(start.elapsed()); }
                    return Ok(resp);
                }
                Err(e) if e.is_retryable() && safety == chainrpc_core::method_safety::MethodSafety::Safe => {
                    self.circuit.record_failure();
                    match self.retry.next_delay(attempt) {
                        Some(delay) => {
                            tracing::warn!(
                                attempt,
                                delay_ms = delay.as_millis(),
                                error = %e,
                                url = %self.url,
                                "retrying request"
                            );
                            tokio::time::sleep(delay).await;
                        }
                        None => {
                            tracing::error!(
                                attempt,
                                error = %e,
                                url = %self.url,
                                "max retries exceeded"
                            );
                            if let Some(ref m) = self.metrics { m.record_failure(); }
                            return Err(e);
                        }
                    }
                }
                Err(e) => {
                    // Non-retryable: either the error itself isn't retryable,
                    // or the method is Idempotent/Unsafe and must not be auto-retried.
                    if let Some(ref m) = self.metrics { m.record_failure(); }
                    return Err(e);
                }
            }
        }
    }

    /// True HTTP batch: send all requests as a JSON array in one HTTP call.
    async fn send_batch(
        &self,
        reqs: Vec<JsonRpcRequest>,
    ) -> Result<Vec<JsonRpcResponse>, TransportError> {
        if reqs.is_empty() {
            return Ok(vec![]);
        }

        let resp = self
            .http
            .post(&self.url)
            .json(&reqs)
            .send()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(TransportError::Http(format!("HTTP {status}: {body}")));
        }

        resp.json::<Vec<JsonRpcResponse>>()
            .await
            .map_err(|e| TransportError::Http(e.to_string()))
    }

    fn health(&self) -> HealthStatus {
        match self.circuit.state() {
            chainrpc_core::policy::CircuitState::Open => HealthStatus::Unhealthy,
            chainrpc_core::policy::CircuitState::HalfOpen => HealthStatus::Degraded,
            chainrpc_core::policy::CircuitState::Closed => HealthStatus::Healthy,
        }
    }

    fn url(&self) -> &str {
        &self.url
    }
}
