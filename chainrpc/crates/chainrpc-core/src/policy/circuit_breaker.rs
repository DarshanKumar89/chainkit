//! Three-state circuit breaker: Closed → Open → Half-Open.
//!
//! State transitions:
//! - `Closed` → `Open`:     failure count reaches `failure_threshold`
//! - `Open` → `Half-Open`:  `open_duration` has elapsed
//! - `Half-Open` → `Closed`: probe request succeeds
//! - `Half-Open` → `Open`:   probe request fails

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation.
    Closed,
    /// All requests rejected. Wait for `open_duration` before probing.
    Open,
    /// One probe request allowed to test provider health.
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Configuration for the circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before opening.
    pub failure_threshold: u32,
    /// How long to stay open before transitioning to half-open.
    pub open_duration: Duration,
    /// Number of successful half-open probes before closing.
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 1,
        }
    }
}

struct CircuitInner {
    state: CircuitState,
    failure_count: u32,
    success_count: u32,
    opened_at: Option<Instant>,
}

/// Thread-safe circuit breaker.
#[derive(Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    inner: Arc<Mutex<CircuitInner>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker in `Closed` state.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            inner: Arc::new(Mutex::new(CircuitInner {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                opened_at: None,
            })),
        }
    }

    /// Returns the current state, transitioning Open→HalfOpen if the wait has elapsed.
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().unwrap();
        if inner.state == CircuitState::Open {
            if let Some(opened_at) = inner.opened_at {
                if opened_at.elapsed() >= self.config.open_duration {
                    inner.state = CircuitState::HalfOpen;
                    inner.success_count = 0;
                    tracing::info!("Circuit breaker → half-open");
                }
            }
        }
        inner.state
    }

    /// Returns `true` if the circuit allows the request through.
    pub fn is_allowed(&self) -> bool {
        self.state() != CircuitState::Open
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        match inner.state {
            CircuitState::HalfOpen => {
                inner.success_count += 1;
                if inner.success_count >= self.config.success_threshold {
                    inner.state = CircuitState::Closed;
                    inner.failure_count = 0;
                    inner.success_count = 0;
                    inner.opened_at = None;
                    tracing::info!("Circuit breaker → closed");
                }
            }
            CircuitState::Closed => {
                inner.failure_count = 0; // reset on success
            }
            CircuitState::Open => {} // shouldn't happen
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        match inner.state {
            CircuitState::Closed => {
                inner.failure_count += 1;
                if inner.failure_count >= self.config.failure_threshold {
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(Instant::now());
                    tracing::warn!(
                        failures = inner.failure_count,
                        "Circuit breaker → open"
                    );
                }
            }
            CircuitState::HalfOpen => {
                // Probe failed — go back to open
                inner.state = CircuitState::Open;
                inner.opened_at = Some(Instant::now());
                inner.success_count = 0;
                tracing::warn!("Circuit breaker probe failed → open");
            }
            CircuitState::Open => {} // already open
        }
    }
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CircuitBreaker")
            .field("state", &self.state())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cb(threshold: u32) -> CircuitBreaker {
        CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: threshold,
            open_duration: Duration::from_secs(60),
            success_threshold: 1,
        })
    }

    #[test]
    fn starts_closed() {
        let c = cb(5);
        assert_eq!(c.state(), CircuitState::Closed);
        assert!(c.is_allowed());
    }

    #[test]
    fn opens_after_threshold_failures() {
        let c = cb(3);
        c.record_failure();
        assert_eq!(c.state(), CircuitState::Closed);
        c.record_failure();
        assert_eq!(c.state(), CircuitState::Closed);
        c.record_failure();
        assert_eq!(c.state(), CircuitState::Open);
        assert!(!c.is_allowed());
    }

    #[test]
    fn success_resets_failure_count() {
        let c = cb(3);
        c.record_failure();
        c.record_failure();
        c.record_success(); // reset
        c.record_failure();
        c.record_failure();
        // Only 2 failures since last reset — should still be closed
        assert_eq!(c.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_on_success_closes() {
        let c = cb(1);
        // Force open
        c.record_failure();
        assert_eq!(c.state(), CircuitState::Open);

        // Manually force to half-open (simulate elapsed time)
        {
            let mut inner = c.inner.lock().unwrap();
            inner.state = CircuitState::HalfOpen;
        }

        c.record_success();
        assert_eq!(c.state(), CircuitState::Closed);
    }

    #[test]
    fn half_open_on_failure_reopens() {
        let c = cb(1);
        c.record_failure(); // open

        {
            let mut inner = c.inner.lock().unwrap();
            inner.state = CircuitState::HalfOpen;
        }

        c.record_failure(); // back to open
        assert_eq!(c.state(), CircuitState::Open);
    }
}
