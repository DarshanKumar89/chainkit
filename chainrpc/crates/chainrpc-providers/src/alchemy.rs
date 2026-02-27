//! Alchemy provider profile.
//!
//! Rate limits: 300 CU/s on free tier, 660 CU/s on Growth, unlimited on Enterprise.
//! <https://docs.alchemy.com/reference/throughput>

use chainrpc_core::policy::{CircuitBreakerConfig, RateLimiterConfig, RetryConfig};
use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use std::time::Duration;

/// Alchemy compute unit rates (free tier = 300 CU/s).
pub const FREE_TIER_CU_PER_SEC: f64 = 300.0;
pub const GROWTH_TIER_CU_PER_SEC: f64 = 660.0;

/// URL template for HTTP JSON-RPC endpoint.
pub fn http_url(api_key: &str, chain_id: u64) -> String {
    let network = chain_id_to_network(chain_id);
    format!("https://{network}.g.alchemy.com/v2/{api_key}")
}

/// URL template for WebSocket endpoint.
pub fn ws_url(api_key: &str, chain_id: u64) -> String {
    let network = chain_id_to_network(chain_id);
    format!("wss://{network}.g.alchemy.com/v2/{api_key}")
}

/// Build an `HttpRpcClient` pre-configured for Alchemy free tier.
pub fn http_client(api_key: &str, chain_id: u64) -> HttpRpcClient {
    http_client_with_cu(api_key, chain_id, FREE_TIER_CU_PER_SEC)
}

/// Build an `HttpRpcClient` pre-configured for Alchemy with a custom CU rate.
pub fn http_client_with_cu(api_key: &str, chain_id: u64, cu_per_sec: f64) -> HttpRpcClient {
    let url = http_url(api_key, chain_id);
    let config = HttpClientConfig {
        retry: RetryConfig {
            max_retries: 3,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(5),
            multiplier: 2.0,
            jitter_fraction: 0.1,
        },
        circuit_breaker: CircuitBreakerConfig {
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            success_threshold: 1,
        },
        rate_limiter: RateLimiterConfig {
            capacity: cu_per_sec,
            refill_rate: cu_per_sec,
        },
        request_timeout: Duration::from_secs(30),
    };
    HttpRpcClient::new(url, config)
}

fn chain_id_to_network(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "eth-mainnet",
        5 => "eth-goerli",
        11155111 => "eth-sepolia",
        137 => "polygon-mainnet",
        80001 => "polygon-mumbai",
        42161 => "arb-mainnet",
        421614 => "arb-sepolia",
        10 => "opt-mainnet",
        11155420 => "opt-sepolia",
        8453 => "base-mainnet",
        84532 => "base-sepolia",
        _ => "eth-mainnet",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_mainnet() {
        let url = http_url("test_key", 1);
        assert_eq!(url, "https://eth-mainnet.g.alchemy.com/v2/test_key");
    }

    #[test]
    fn http_url_arbitrum() {
        let url = http_url("key", 42161);
        assert!(url.contains("arb-mainnet"));
    }

    #[test]
    fn ws_url_base() {
        let url = ws_url("key", 8453);
        assert!(url.starts_with("wss://"));
        assert!(url.contains("base-mainnet"));
    }
}
