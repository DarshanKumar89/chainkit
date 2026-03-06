//! Metrics and observability for RPC providers.
//!
//! Provides lock-free, atomic counters for request success/failure rates,
//! latency tracking, rate-limit hits, and circuit-breaker events.
//!
//! # Design
//!
//! - [`ProviderMetrics`] tracks per-provider counters using `AtomicU64`.
//! - [`MetricsSnapshot`] is an immutable, serializable point-in-time snapshot.
//! - [`RpcMetrics`] aggregates metrics across multiple providers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

// ---------------------------------------------------------------------------
// Per-provider metrics
// ---------------------------------------------------------------------------

/// Atomic counters for a single RPC provider endpoint.
pub struct ProviderMetrics {
    /// Provider URL or identifier.
    url: String,
    /// Total requests sent (success + failure).
    total_requests: AtomicU64,
    /// Requests that completed successfully.
    successful_requests: AtomicU64,
    /// Requests that failed (transport error, timeout, etc.).
    failed_requests: AtomicU64,
    /// Cumulative latency in microseconds (for averaging).
    total_latency_us: AtomicU64,
    /// Minimum observed latency in microseconds.
    min_latency_us: AtomicU64,
    /// Maximum observed latency in microseconds.
    max_latency_us: AtomicU64,
    /// Number of times a request was rejected by the rate limiter.
    rate_limit_hits: AtomicU64,
    /// Number of times the circuit breaker opened.
    circuit_open_count: AtomicU64,
}

impl ProviderMetrics {
    /// Create a new metrics instance for the given provider URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
            min_latency_us: AtomicU64::new(u64::MAX),
            max_latency_us: AtomicU64::new(0),
            rate_limit_hits: AtomicU64::new(0),
            circuit_open_count: AtomicU64::new(0),
        }
    }

    /// Record a successful request with the given latency.
    pub fn record_success(&self, latency: Duration) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.successful_requests.fetch_add(1, Ordering::Relaxed);

        let us = latency.as_micros() as u64;
        self.total_latency_us.fetch_add(us, Ordering::Relaxed);
        self.update_min_latency(us);
        self.update_max_latency(us);
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.failed_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rate-limit rejection.
    pub fn record_rate_limit(&self) {
        self.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record the circuit breaker opening.
    pub fn record_circuit_open(&self) {
        self.circuit_open_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Compute the average latency across all successful requests.
    ///
    /// Returns `Duration::ZERO` if no successful requests have been recorded.
    pub fn avg_latency(&self) -> Duration {
        let total = self.total_latency_us.load(Ordering::Relaxed);
        let count = self.successful_requests.load(Ordering::Relaxed);
        if count == 0 {
            return Duration::ZERO;
        }
        Duration::from_micros(total / count)
    }

    /// Compute the success rate as a fraction in `[0.0, 1.0]`.
    ///
    /// Returns `1.0` if no requests have been made.
    pub fn success_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        let successes = self.successful_requests.load(Ordering::Relaxed);
        successes as f64 / total as f64
    }

    /// Return the provider URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Produce an immutable snapshot for reporting / serialization.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let total = self.total_requests.load(Ordering::Relaxed);
        let successful = self.successful_requests.load(Ordering::Relaxed);
        let failed = self.failed_requests.load(Ordering::Relaxed);
        let total_latency = self.total_latency_us.load(Ordering::Relaxed);
        let min_us = self.min_latency_us.load(Ordering::Relaxed);
        let max_us = self.max_latency_us.load(Ordering::Relaxed);

        let avg_latency_ms = if successful > 0 {
            (total_latency as f64 / successful as f64) / 1000.0
        } else {
            0.0
        };

        let min_latency_ms = if min_us == u64::MAX {
            0.0
        } else {
            min_us as f64 / 1000.0
        };

        let max_latency_ms = max_us as f64 / 1000.0;

        let success_rate = if total > 0 {
            successful as f64 / total as f64
        } else {
            1.0
        };

        MetricsSnapshot {
            url: self.url.clone(),
            total_requests: total,
            successful_requests: successful,
            failed_requests: failed,
            avg_latency_ms,
            min_latency_ms,
            max_latency_ms,
            rate_limit_hits: self.rate_limit_hits.load(Ordering::Relaxed),
            circuit_open_count: self.circuit_open_count.load(Ordering::Relaxed),
            success_rate,
        }
    }

    // -- internal helpers ---------------------------------------------------

    /// Atomically update `min_latency_us` if `us` is smaller.
    fn update_min_latency(&self, us: u64) {
        let mut current = self.min_latency_us.load(Ordering::Relaxed);
        while us < current {
            match self.min_latency_us.compare_exchange_weak(
                current,
                us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Atomically update `max_latency_us` if `us` is larger.
    fn update_max_latency(&self, us: u64) {
        let mut current = self.max_latency_us.load(Ordering::Relaxed);
        while us > current {
            match self.max_latency_us.compare_exchange_weak(
                current,
                us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

impl std::fmt::Debug for ProviderMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderMetrics")
            .field("url", &self.url)
            .field(
                "total_requests",
                &self.total_requests.load(Ordering::Relaxed),
            )
            .field("success_rate", &self.success_rate())
            .field("avg_latency", &self.avg_latency())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// An immutable, serializable point-in-time snapshot of provider metrics.
#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    /// Provider URL or identifier.
    pub url: String,
    /// Total requests sent.
    pub total_requests: u64,
    /// Number of successful requests.
    pub successful_requests: u64,
    /// Number of failed requests.
    pub failed_requests: u64,
    /// Average latency in milliseconds.
    pub avg_latency_ms: f64,
    /// Minimum observed latency in milliseconds.
    pub min_latency_ms: f64,
    /// Maximum observed latency in milliseconds.
    pub max_latency_ms: f64,
    /// Number of rate-limit rejections.
    pub rate_limit_hits: u64,
    /// Number of circuit-breaker opens.
    pub circuit_open_count: u64,
    /// Success rate as a fraction in [0.0, 1.0].
    pub success_rate: f64,
}

impl MetricsSnapshot {
    /// Format metrics in Prometheus exposition format.
    ///
    /// Each metric is prefixed with `chainrpc_` and labeled with `provider="<url>"`.
    pub fn to_prometheus(&self) -> String {
        let label = format!("provider=\"{}\"", self.url.replace('"', "\\\""));
        let mut out = String::new();

        out.push_str(&format!(
            "chainrpc_requests_total{{{label}}} {}\n",
            self.total_requests
        ));
        out.push_str(&format!(
            "chainrpc_requests_successful_total{{{label}}} {}\n",
            self.successful_requests
        ));
        out.push_str(&format!(
            "chainrpc_requests_failed_total{{{label}}} {}\n",
            self.failed_requests
        ));
        out.push_str(&format!(
            "chainrpc_latency_avg_ms{{{label}}} {:.3}\n",
            self.avg_latency_ms
        ));
        out.push_str(&format!(
            "chainrpc_latency_min_ms{{{label}}} {:.3}\n",
            self.min_latency_ms
        ));
        out.push_str(&format!(
            "chainrpc_latency_max_ms{{{label}}} {:.3}\n",
            self.max_latency_ms
        ));
        out.push_str(&format!(
            "chainrpc_rate_limit_hits_total{{{label}}} {}\n",
            self.rate_limit_hits
        ));
        out.push_str(&format!(
            "chainrpc_circuit_open_total{{{label}}} {}\n",
            self.circuit_open_count
        ));
        out.push_str(&format!(
            "chainrpc_success_rate{{{label}}} {:.4}\n",
            self.success_rate
        ));

        out
    }
}

// ---------------------------------------------------------------------------
// Aggregated metrics
// ---------------------------------------------------------------------------

/// Aggregated metrics across all RPC providers.
pub struct RpcMetrics {
    providers: Vec<ProviderMetrics>,
}

impl RpcMetrics {
    /// Create a new (empty) metrics aggregator.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a provider and return a reference to its metrics.
    pub fn add_provider(&mut self, url: impl Into<String>) -> &ProviderMetrics {
        self.providers.push(ProviderMetrics::new(url));
        self.providers.last().unwrap()
    }

    /// Produce snapshots for all registered providers.
    pub fn snapshot_all(&self) -> Vec<MetricsSnapshot> {
        self.providers.iter().map(|p| p.snapshot()).collect()
    }

    /// Total requests across all providers.
    pub fn total_requests(&self) -> u64 {
        self.providers
            .iter()
            .map(|p| p.total_requests.load(Ordering::Relaxed))
            .sum()
    }

    /// Number of registered providers.
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Format all provider metrics in Prometheus exposition format.
    pub fn to_prometheus(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("# HELP chainrpc_requests_total Total RPC requests per provider.\n");
        out.push_str("# TYPE chainrpc_requests_total counter\n");
        out.push_str("# HELP chainrpc_latency_avg_ms Average request latency in milliseconds.\n");
        out.push_str("# TYPE chainrpc_latency_avg_ms gauge\n");
        for snap in self.snapshot_all() {
            out.push_str(&snap.to_prometheus());
        }
        out
    }
}

impl Default for RpcMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for RpcMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcMetrics")
            .field("provider_count", &self.providers.len())
            .field("total_requests", &self.total_requests())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_success_updates_counters() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(50));
        m.record_success(Duration::from_millis(150));

        assert_eq!(m.total_requests.load(Ordering::Relaxed), 2);
        assert_eq!(m.successful_requests.load(Ordering::Relaxed), 2);
        assert_eq!(m.failed_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn record_failure_updates_counters() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(10));
        m.record_failure();
        m.record_failure();

        assert_eq!(m.total_requests.load(Ordering::Relaxed), 3);
        assert_eq!(m.successful_requests.load(Ordering::Relaxed), 1);
        assert_eq!(m.failed_requests.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn avg_latency_calculation() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(100));
        m.record_success(Duration::from_millis(200));

        let avg = m.avg_latency();
        // Average should be 150ms.
        assert!(
            avg >= Duration::from_millis(140) && avg <= Duration::from_millis(160),
            "unexpected avg latency: {avg:?}"
        );
    }

    #[test]
    fn avg_latency_zero_when_no_requests() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        assert_eq!(m.avg_latency(), Duration::ZERO);
    }

    #[test]
    fn success_rate_calculation() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(10));
        m.record_success(Duration::from_millis(10));
        m.record_failure();

        let rate = m.success_rate();
        // 2 out of 3 = 0.6667
        assert!(
            (rate - 2.0 / 3.0).abs() < 0.001,
            "unexpected success rate: {rate}"
        );
    }

    #[test]
    fn success_rate_defaults_to_one() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        assert_eq!(m.success_rate(), 1.0);
    }

    #[test]
    fn min_max_latency_tracked() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(50));
        m.record_success(Duration::from_millis(200));
        m.record_success(Duration::from_millis(10));

        let snap = m.snapshot();
        assert!(
            snap.min_latency_ms >= 9.0 && snap.min_latency_ms <= 11.0,
            "unexpected min: {}",
            snap.min_latency_ms
        );
        assert!(
            snap.max_latency_ms >= 199.0 && snap.max_latency_ms <= 201.0,
            "unexpected max: {}",
            snap.max_latency_ms
        );
    }

    #[test]
    fn snapshot_serialization() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(100));
        m.record_failure();
        m.record_rate_limit();
        m.record_circuit_open();

        let snap = m.snapshot();
        let json = serde_json::to_string(&snap).unwrap();

        assert!(json.contains("\"url\":\"https://rpc.example.com\""));
        assert!(json.contains("\"total_requests\":2"));
        assert!(json.contains("\"successful_requests\":1"));
        assert!(json.contains("\"failed_requests\":1"));
        assert!(json.contains("\"rate_limit_hits\":1"));
        assert!(json.contains("\"circuit_open_count\":1"));
        assert!(json.contains("\"success_rate\":0.5"));
    }

    #[test]
    fn rate_limit_and_circuit_open_counts() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_rate_limit();
        m.record_rate_limit();
        m.record_rate_limit();
        m.record_circuit_open();

        assert_eq!(m.rate_limit_hits.load(Ordering::Relaxed), 3);
        assert_eq!(m.circuit_open_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rpc_metrics_aggregated() {
        let mut metrics = RpcMetrics::new();
        let p1 = metrics.add_provider("https://a.com") as *const ProviderMetrics;
        let p2 = metrics.add_provider("https://b.com") as *const ProviderMetrics;

        // Safety: we just created these; they're valid for the lifetime of `metrics`.
        unsafe {
            (*p1).record_success(Duration::from_millis(10));
            (*p1).record_success(Duration::from_millis(20));
            (*p2).record_failure();
        }

        assert_eq!(metrics.total_requests(), 3);
        assert_eq!(metrics.provider_count(), 2);

        let snaps = metrics.snapshot_all();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].url, "https://a.com");
        assert_eq!(snaps[0].successful_requests, 2);
        assert_eq!(snaps[1].url, "https://b.com");
        assert_eq!(snaps[1].failed_requests, 1);
    }

    #[test]
    fn prometheus_export() {
        let m = ProviderMetrics::new("https://rpc.example.com");
        m.record_success(Duration::from_millis(100));
        m.record_failure();
        let snap = m.snapshot();
        let prom = snap.to_prometheus();
        assert!(prom.contains("chainrpc_requests_total{provider=\"https://rpc.example.com\"} 2"));
        assert!(prom.contains("chainrpc_requests_successful_total"));
        assert!(prom.contains("chainrpc_requests_failed_total"));
        assert!(prom.contains("chainrpc_latency_avg_ms"));
        assert!(prom.contains("chainrpc_success_rate"));
    }

    #[test]
    fn rpc_metrics_default() {
        let metrics = RpcMetrics::default();
        assert_eq!(metrics.provider_count(), 0);
        assert_eq!(metrics.total_requests(), 0);
    }
}
