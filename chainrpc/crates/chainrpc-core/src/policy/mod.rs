//! Policy engine — composable middleware for RPC reliability.
//!
//! The policy stack (applied in order):
//! ```text
//! Request → [RateLimiter] → [CircuitBreaker] → [RetryPolicy] → [Transport]
//! ```

pub mod circuit_breaker;
pub mod rate_limiter;
pub mod retry;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use rate_limiter::{RateLimiter, RateLimiterConfig, TokenBucket};
pub use retry::{RetryConfig, RetryPolicy};
