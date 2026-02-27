//! Public / community RPC endpoints.
//!
//! These are free, no-API-key endpoints suitable for development and testing.
//! Rate limits are lower and reliability may vary.

use chainrpc_core::policy::{CircuitBreakerConfig, RateLimiterConfig, RetryConfig};
use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use std::time::Duration;

fn conservative_config() -> HttpClientConfig {
    HttpClientConfig {
        retry: RetryConfig {
            max_retries: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
            jitter_fraction: 0.2,
        },
        circuit_breaker: CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration: Duration::from_secs(60),
            success_threshold: 2,
        },
        rate_limiter: RateLimiterConfig {
            capacity: 5.0,   // conservative: 5 req/s
            refill_rate: 5.0,
        },
        request_timeout: Duration::from_secs(30),
    }
}

/// Cloudflare Ethereum gateway (Ethereum mainnet only).
pub fn cloudflare_mainnet() -> HttpRpcClient {
    HttpRpcClient::new("https://cloudflare-eth.com", conservative_config())
}

/// Ankr public RPC.
pub fn ankr(chain_id: u64) -> HttpRpcClient {
    let url = ankr_url(chain_id);
    HttpRpcClient::new(url, conservative_config())
}

fn ankr_url(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "https://rpc.ankr.com/eth",
        137 => "https://rpc.ankr.com/polygon",
        42161 => "https://rpc.ankr.com/arbitrum",
        10 => "https://rpc.ankr.com/optimism",
        8453 => "https://rpc.ankr.com/base",
        56 => "https://rpc.ankr.com/bsc",
        _ => "https://rpc.ankr.com/eth",
    }
}

/// LlamaNodes public RPC.
pub fn llama_rpc(chain_id: u64) -> HttpRpcClient {
    let url = match chain_id {
        1 => "https://eth.llamarpc.com",
        137 => "https://polygon.llamarpc.com",
        _ => "https://eth.llamarpc.com",
    };
    HttpRpcClient::new(url, conservative_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ankr_url_ethereum() {
        assert_eq!(ankr_url(1), "https://rpc.ankr.com/eth");
    }

    #[test]
    fn ankr_url_polygon() {
        assert_eq!(ankr_url(137), "https://rpc.ankr.com/polygon");
    }
}
