//! Token bucket rate limiter.
//!
//! Models a token bucket: tokens accrue at `refill_rate` tokens/second up to
//! `capacity`. Each request consumes `cost` tokens. If insufficient tokens
//! are available, `try_acquire` returns `false` and the caller should back off.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Rate limiter configuration.
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum tokens in the bucket.
    pub capacity: f64,
    /// Token refill rate (tokens per second).
    pub refill_rate: f64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            capacity: 300.0,    // 300 CU capacity (Alchemy default)
            refill_rate: 300.0, // 300 CU/s
        }
    }
}

struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

/// Thread-safe token bucket rate limiter.
pub struct TokenBucket {
    config: RateLimiterConfig,
    state: Mutex<BucketState>,
}

impl TokenBucket {
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            state: Mutex::new(BucketState {
                tokens: config.capacity,
                last_refill: Instant::now(),
            }),
            config,
        }
    }

    /// Try to acquire `cost` tokens.
    ///
    /// Returns `true` if tokens were available and consumed.
    /// Returns `false` if the bucket is empty (rate limit exceeded).
    pub fn try_acquire(&self, cost: f64) -> bool {
        let mut state = self.state.lock().unwrap();
        self.refill(&mut state);

        if state.tokens >= cost {
            state.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Returns the estimated wait time before `cost` tokens are available.
    pub fn wait_time(&self, cost: f64) -> Duration {
        let state = self.state.lock().unwrap();
        let deficit = cost - state.tokens;
        if deficit <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(deficit / self.config.refill_rate)
        }
    }

    /// Returns currently available tokens.
    pub fn available(&self) -> f64 {
        let mut state = self.state.lock().unwrap();
        self.refill(&mut state);
        state.tokens
    }

    fn refill(&self, state: &mut BucketState) {
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill).as_secs_f64();
        let new_tokens = elapsed * self.config.refill_rate;
        state.tokens = (state.tokens + new_tokens).min(self.config.capacity);
        state.last_refill = now;
    }
}

/// A rate limiter wrapping the token bucket.
pub struct RateLimiter {
    bucket: TokenBucket,
    /// Cost per standard request (can be overridden per method).
    pub default_cost: f64,
}

impl RateLimiter {
    pub fn new(config: RateLimiterConfig) -> Self {
        Self {
            bucket: TokenBucket::new(config),
            default_cost: 1.0,
        }
    }

    /// Try to acquire the default cost.
    pub fn try_acquire(&self) -> bool {
        self.bucket.try_acquire(self.default_cost)
    }

    /// Try to acquire a specific cost (for expensive methods like eth_getLogs).
    pub fn try_acquire_cost(&self, cost: f64) -> bool {
        self.bucket.try_acquire(cost)
    }

    /// Wait time before the default cost is available.
    pub fn wait_time(&self) -> Duration {
        self.bucket.wait_time(self.default_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_within_capacity() {
        let rl = RateLimiter::new(RateLimiterConfig {
            capacity: 10.0,
            refill_rate: 1.0,
        });
        for _ in 0..10 {
            assert!(rl.try_acquire(), "should succeed within capacity");
        }
    }

    #[test]
    fn reject_when_empty() {
        let rl = RateLimiter::new(RateLimiterConfig {
            capacity: 3.0,
            refill_rate: 0.0001, // almost no refill
        });
        rl.try_acquire();
        rl.try_acquire();
        rl.try_acquire();
        // Now empty
        assert!(!rl.try_acquire(), "should be rate limited");
    }

    #[test]
    fn wait_time_when_empty() {
        let rl = RateLimiter::new(RateLimiterConfig {
            capacity: 1.0,
            refill_rate: 10.0, // 10 tokens/sec
        });
        rl.try_acquire(); // drain
        let wait = rl.wait_time();
        // Should be ~100ms (1 token / 10 tokens per sec)
        assert!(
            wait.as_millis() >= 50 && wait.as_millis() <= 200,
            "unexpected wait time: {wait:?}"
        );
    }
}
