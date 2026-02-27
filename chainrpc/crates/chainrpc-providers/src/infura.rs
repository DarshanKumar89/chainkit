//! Infura provider profile.

use chainrpc_core::policy::{CircuitBreakerConfig, RateLimiterConfig, RetryConfig};
use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use std::time::Duration;

/// Build an Infura HTTP client for the given network and project ID.
pub fn http_client(project_id: &str, chain_id: u64) -> HttpRpcClient {
    let url = http_url(project_id, chain_id);
    let config = HttpClientConfig {
        retry: RetryConfig::default(),
        circuit_breaker: CircuitBreakerConfig::default(),
        rate_limiter: RateLimiterConfig {
            capacity: 10.0,  // 10 req/s free tier
            refill_rate: 10.0,
        },
        request_timeout: Duration::from_secs(30),
    };
    HttpRpcClient::new(url, config)
}

pub fn http_url(project_id: &str, chain_id: u64) -> String {
    let network = chain_id_to_network(chain_id);
    format!("https://{network}.infura.io/v3/{project_id}")
}

fn chain_id_to_network(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "mainnet",
        5 => "goerli",
        11155111 => "sepolia",
        137 => "polygon-mainnet",
        80001 => "polygon-mumbai",
        42161 => "arbitrum-mainnet",
        10 => "optimism-mainnet",
        _ => "mainnet",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infura_mainnet_url() {
        assert_eq!(
            http_url("proj123", 1),
            "https://mainnet.infura.io/v3/proj123"
        );
    }
}
