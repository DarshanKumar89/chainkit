//! # Example 05: Custom Client Configuration
//!
//! Demonstrates how to create an `HttpRpcClient` with fully custom retry,
//! circuit breaker, and rate limiter settings instead of using `default_for()`.
//!
//! ## What this demonstrates
//!
//! - Building `HttpClientConfig` with custom `RetryConfig`
//! - Tuning the `CircuitBreakerConfig` thresholds and timing
//! - Configuring the `RateLimiterConfig` token bucket (capacity + refill rate)
//! - Setting a custom request timeout
//! - Comparing custom config vs. `default_for()` defaults
//! - Using named provider constructors with custom CU rates

use std::time::Duration;

use chainrpc_core::policy::{CircuitBreakerConfig, RateLimiterConfig, RetryConfig};
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use chainrpc_providers::alchemy;

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Option A: Fully custom configuration
    //
    // Use this when you need precise control over every reliability knob.
    // -----------------------------------------------------------------------

    let config = HttpClientConfig {
        // Retry policy: aggressive retries for a mission-critical service.
        retry: RetryConfig {
            max_retries: 5,                              // 5 retries (6 total attempts)
            initial_backoff: Duration::from_millis(500),  // start at 500ms
            max_backoff: Duration::from_secs(30),         // cap at 30s
            multiplier: 2.0,                              // double each time: 500ms, 1s, 2s, 4s, 8s
            jitter_fraction: 0.15,                        // +/- 15% jitter to prevent thundering herd
        },

        // Circuit breaker: open quickly, recover slowly.
        circuit_breaker: CircuitBreakerConfig {
            failure_threshold: 3,                         // open after 3 consecutive failures
            open_duration: Duration::from_secs(10),       // stay open for 10 seconds
            success_threshold: 1,                         // 1 successful probe to close
        },

        // Rate limiter: 500 CU capacity, refilling at 100 CU/s.
        // This means you can burst up to 500 CU, then sustain 100 CU/s.
        // At 10 CU per eth_blockNumber, that's 50 burst / 10 sustained per second.
        rate_limiter: RateLimiterConfig {
            capacity: 500.0,    // maximum tokens in the bucket
            refill_rate: 100.0, // tokens added per second
        },

        // Request timeout: individual request must complete within 15 seconds.
        request_timeout: Duration::from_secs(15),
    };

    // Create the client with the custom config.
    let client = HttpRpcClient::new(
        "https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY",
        config,
    );

    println!("Custom client created:");
    println!("  URL:     {}", client.url());
    println!("  Health:  {}", client.health());

    // Make a test call.
    let block: String = client
        .call(1, "eth_blockNumber", vec![])
        .await
        .expect("eth_blockNumber failed");
    println!("  Block:   {block}");

    // -----------------------------------------------------------------------
    // Option B: default_for() -- zero-config with sensible defaults
    //
    // Defaults:
    //   retry:           3 retries, 100ms initial, 10s max, 2x multiplier, 0.1 jitter
    //   circuit_breaker: 5 failures, 30s open, 1 success to close
    //   rate_limiter:    300 CU capacity, 300 CU/s refill (Alchemy free tier)
    //   timeout:         30s
    // -----------------------------------------------------------------------
    let default_client = HttpRpcClient::default_for("https://rpc.ankr.com/eth");

    let block: String = default_client
        .call(2, "eth_blockNumber", vec![])
        .await
        .expect("default client eth_blockNumber failed");
    println!("\nDefault client block: {block}");

    // -----------------------------------------------------------------------
    // Option C: Named provider with custom CU rate
    //
    // The Alchemy provider profile has a convenience constructor that
    // accepts a custom compute-unit rate for paid tiers.
    // -----------------------------------------------------------------------

    // Free tier (300 CU/s)
    let _free = alchemy::http_client("YOUR_KEY", 1);

    // Growth tier (660 CU/s)
    let growth_client = alchemy::http_client_with_cu("YOUR_KEY", 1, alchemy::GROWTH_TIER_CU_PER_SEC);

    let block: String = growth_client
        .call(3, "eth_blockNumber", vec![])
        .await
        .expect("growth tier eth_blockNumber failed");
    println!("Growth tier client block: {block}");

    // -----------------------------------------------------------------------
    // Configuration comparison: print the effective config for each approach
    // -----------------------------------------------------------------------
    println!("\n--- Configuration Comparison ---\n");

    println!("Custom config:");
    println!("  max_retries:       5");
    println!("  initial_backoff:   500ms");
    println!("  failure_threshold: 3");
    println!("  open_duration:     10s");
    println!("  rate_limit_cap:    500 CU");
    println!("  refill_rate:       100 CU/s");
    println!("  request_timeout:   15s");

    println!("\ndefault_for() config:");
    println!("  max_retries:       3");
    println!("  initial_backoff:   100ms");
    println!("  failure_threshold: 5");
    println!("  open_duration:     30s");
    println!("  rate_limit_cap:    300 CU");
    println!("  refill_rate:       300 CU/s");
    println!("  request_timeout:   30s");

    println!("\nAlchemy Growth tier config:");
    println!("  max_retries:       3");
    println!("  initial_backoff:   200ms");
    println!("  failure_threshold: 5");
    println!("  open_duration:     30s");
    println!("  rate_limit_cap:    660 CU");
    println!("  refill_rate:       660 CU/s");
    println!("  request_timeout:   30s");
}
