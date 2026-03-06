//! Token bucket rate limiter.
//!
//! Models a token bucket: tokens accrue at `refill_rate` tokens/second up to
//! `capacity`. Each request consumes `cost` tokens. If insufficient tokens
//! are available, `try_acquire` returns `false` and the caller should back off.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::cu_tracker::CuCostTable;

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

/// A method-aware rate limiter that automatically looks up CU costs per RPC method.
///
/// Wraps a [`TokenBucket`] with a [`CuCostTable`] so callers only need to
/// supply the method name — the correct compute-unit cost is resolved
/// internally.
pub struct MethodAwareRateLimiter {
    bucket: TokenBucket,
    cost_table: CuCostTable,
}

impl MethodAwareRateLimiter {
    /// Create a new method-aware rate limiter.
    pub fn new(config: RateLimiterConfig, cost_table: CuCostTable) -> Self {
        Self {
            bucket: TokenBucket::new(config),
            cost_table,
        }
    }

    /// Acquire tokens for a specific RPC method, using its CU cost.
    ///
    /// Returns `true` if the method's cost was successfully consumed from the
    /// bucket, `false` if the bucket has insufficient tokens (rate limited).
    pub fn try_acquire_method(&self, method: &str) -> bool {
        let cost = self.cost_table.cost_for(method) as f64;
        self.bucket.try_acquire(cost)
    }

    /// Wait time before the given method can be called.
    pub fn wait_time_for_method(&self, method: &str) -> Duration {
        let cost = self.cost_table.cost_for(method) as f64;
        self.bucket.wait_time(cost)
    }

    /// Access the underlying bucket for manual control.
    pub fn bucket(&self) -> &TokenBucket {
        &self.bucket
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

    // ---- MethodAwareRateLimiter tests ----

    #[test]
    fn method_aware_uses_cu_costs() {
        // eth_getLogs = 75 CU, eth_blockNumber = 10 CU.
        // With capacity 150, eth_getLogs can be called 2 times (2*75=150),
        // while eth_blockNumber can be called 15 times (15*10=150).
        let table = CuCostTable::alchemy_defaults();

        // Test expensive method: eth_getLogs (75 CU each)
        let rl_expensive = MethodAwareRateLimiter::new(
            RateLimiterConfig {
                capacity: 150.0,
                refill_rate: 0.0001, // near-zero refill so bucket drains
            },
            table.clone(),
        );
        assert!(rl_expensive.try_acquire_method("eth_getLogs")); // 75 consumed, 75 left
        assert!(rl_expensive.try_acquire_method("eth_getLogs")); // 150 consumed, 0 left
        assert!(
            !rl_expensive.try_acquire_method("eth_getLogs"),
            "should be rate limited after 2 expensive calls"
        );

        // Test cheap method: eth_blockNumber (10 CU each)
        let rl_cheap = MethodAwareRateLimiter::new(
            RateLimiterConfig {
                capacity: 150.0,
                refill_rate: 0.0001,
            },
            CuCostTable::alchemy_defaults(),
        );
        let mut count = 0;
        while rl_cheap.try_acquire_method("eth_blockNumber") {
            count += 1;
            if count > 20 {
                break; // safety valve
            }
        }
        assert_eq!(
            count, 15,
            "cheap method (10 CU) should fit 15 times in 150 capacity"
        );
    }

    #[test]
    fn method_aware_wait_time() {
        // refill_rate = 100 tokens/sec.
        // Drain the bucket, then check wait times scale with method cost.
        let table = CuCostTable::alchemy_defaults();
        let rl = MethodAwareRateLimiter::new(
            RateLimiterConfig {
                capacity: 300.0,
                refill_rate: 100.0, // 100 CU/sec
            },
            table,
        );
        // Drain the bucket completely.
        while rl.bucket().try_acquire(100.0) {}

        // eth_blockNumber = 10 CU → ~100ms wait at 100 CU/sec
        let wait_cheap = rl.wait_time_for_method("eth_blockNumber");
        // eth_getLogs = 75 CU → ~750ms wait at 100 CU/sec
        let wait_expensive = rl.wait_time_for_method("eth_getLogs");

        assert!(
            wait_expensive > wait_cheap,
            "expensive method should have longer wait: expensive={wait_expensive:?}, cheap={wait_cheap:?}"
        );

        // Verify approximate scale: expensive wait should be roughly 7.5x the cheap wait
        let ratio = wait_expensive.as_secs_f64() / wait_cheap.as_secs_f64();
        assert!(
            ratio > 5.0 && ratio < 10.0,
            "wait time ratio should be ~7.5, got {ratio:.2}"
        );
    }

    #[test]
    fn method_aware_unknown_method_uses_default() {
        // Default CU cost for Alchemy table = 50.
        // Capacity 100 → unknown method (50 CU) fits exactly 2 times.
        let table = CuCostTable::alchemy_defaults();
        let rl = MethodAwareRateLimiter::new(
            RateLimiterConfig {
                capacity: 100.0,
                refill_rate: 0.0001,
            },
            table,
        );

        assert!(rl.try_acquire_method("some_unknown_rpc_method")); // 50 consumed
        assert!(rl.try_acquire_method("another_unknown_method")); // 100 consumed
        assert!(
            !rl.try_acquire_method("yet_another_unknown"),
            "unknown method should use default cost (50) and be rate limited"
        );
    }
}
