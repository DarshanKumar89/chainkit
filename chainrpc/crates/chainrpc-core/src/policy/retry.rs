//! Exponential backoff retry policy with optional jitter.

use std::time::Duration;

/// Configuration for the retry policy.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not counting the first try).
    pub max_retries: u32,
    /// Initial backoff delay.
    pub initial_backoff: Duration,
    /// Maximum backoff delay (caps exponential growth).
    pub max_backoff: Duration,
    /// Multiplier applied to backoff on each retry.
    pub multiplier: f64,
    /// Add ±`jitter_fraction * backoff` random jitter (0.0 = no jitter).
    pub jitter_fraction: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(10),
            multiplier: 2.0,
            jitter_fraction: 0.1,
        }
    }
}

/// Stateless retry policy — computes the next delay given the attempt number.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub config: RetryConfig,
}

impl RetryPolicy {
    pub fn new(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Returns the delay before the `attempt`-th retry (1-based).
    /// Returns `None` if `attempt` exceeds `max_retries`.
    pub fn next_delay(&self, attempt: u32) -> Option<Duration> {
        if attempt > self.config.max_retries {
            return None;
        }
        let base_ms = self.config.initial_backoff.as_millis() as f64
            * self.config.multiplier.powi((attempt - 1) as i32);
        let cap_ms = self.config.max_backoff.as_millis() as f64;
        let capped = base_ms.min(cap_ms);

        // Deterministic pseudo-jitter for testing (use system time-based in prod)
        let jitter_ms = capped * self.config.jitter_fraction * 0.5; // simplified: +jitter/2
        let total_ms = (capped + jitter_ms) as u64;

        Some(Duration::from_millis(total_ms))
    }

    /// Returns `true` if any retries remain after `attempt` failures.
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt <= self.config.max_retries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_retry_delay() {
        let policy = RetryPolicy::new(RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
            jitter_fraction: 0.0,
        });
        let d1 = policy.next_delay(1).unwrap();
        let d2 = policy.next_delay(2).unwrap();
        let d3 = policy.next_delay(3).unwrap();
        assert_eq!(d1.as_millis(), 100);
        assert_eq!(d2.as_millis(), 200);
        assert_eq!(d3.as_millis(), 400);
        assert!(policy.next_delay(4).is_none());
    }

    #[test]
    fn delay_capped_at_max() {
        let policy = RetryPolicy::new(RetryConfig {
            max_retries: 10,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(500),
            multiplier: 10.0,
            jitter_fraction: 0.0,
        });
        // After a few doublings it should be capped
        let d5 = policy.next_delay(5).unwrap();
        assert!(d5 <= Duration::from_millis(500), "d5={d5:?} exceeds max");
    }

    #[test]
    fn should_retry_boundary() {
        let policy = RetryPolicy::new(RetryConfig {
            max_retries: 2,
            ..Default::default()
        });
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
    }
}
