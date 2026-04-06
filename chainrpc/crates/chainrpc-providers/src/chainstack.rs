//! Chainstack provider profile.
//!
//! Chainstack is a managed blockchain infrastructure provider supporting 70+ protocols.
//! Endpoints come in two flavors:
//!
//! - **Global (elastic) nodes** use a templated URL:
//!   `https://{chain}-{network}.core.chainstack.com/{auth_token}`
//! - **Dedicated / Trader nodes** use a per-node URL:
//!   `https://nd-NNN-NNN-NNN.p2pify.com/{auth_key}`
//!
//! Authentication is always via a token in the URL path.
//!
//! Rate limits (requests per second) vary by plan:
//! Developer = 25, Growth = 250, Pro = 400, Business = 600, Enterprise = unlimited.
//! <https://docs.chainstack.com/docs/rps-plan-limits>

use chainrpc_core::policy::{CircuitBreakerConfig, RateLimiterConfig, RetryConfig};
use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use std::time::Duration;

/// Requests per second by plan tier.
pub const DEVELOPER_TIER_RPS: f64 = 25.0;
pub const GROWTH_TIER_RPS: f64 = 250.0;
pub const PRO_TIER_RPS: f64 = 400.0;
pub const BUSINESS_TIER_RPS: f64 = 600.0;

// ---------------------------------------------------------------------------
// Global (elastic) node helpers
// ---------------------------------------------------------------------------

/// HTTP URL for a Global Node, built from chain ID and auth token.
///
/// ```
/// # use chainrpc_providers::chainstack;
/// let url = chainstack::http_url("abc123", 1);
/// assert_eq!(url, "https://ethereum-mainnet.core.chainstack.com/abc123");
/// ```
pub fn http_url(auth_token: &str, chain_id: u64) -> String {
    let network = chain_id_to_network(chain_id);
    format!("https://{network}.core.chainstack.com/{auth_token}")
}

/// WebSocket URL for a Global Node.
pub fn ws_url(auth_token: &str, chain_id: u64) -> String {
    let network = chain_id_to_network(chain_id);
    format!("wss://{network}.core.chainstack.com/ws/{auth_token}")
}

/// Build an `HttpRpcClient` for a Chainstack Global Node (Growth tier defaults).
pub fn http_client(auth_token: &str, chain_id: u64) -> HttpRpcClient {
    http_client_with_rps(auth_token, chain_id, GROWTH_TIER_RPS)
}

/// Build an `HttpRpcClient` for a Chainstack Global Node with a custom RPS limit.
pub fn http_client_with_rps(auth_token: &str, chain_id: u64, rps: f64) -> HttpRpcClient {
    let url = http_url(auth_token, chain_id);
    HttpRpcClient::new(url, default_config(rps))
}

// ---------------------------------------------------------------------------
// Dedicated / Trader node helpers (caller provides full URL)
// ---------------------------------------------------------------------------

/// Build an `HttpRpcClient` from a full endpoint URL (Growth tier defaults).
///
/// Use this when you already have the complete endpoint URL from the Chainstack console,
/// e.g. `https://nd-123-456-789.p2pify.com/your_auth_key`.
pub fn http_client_from_url(endpoint_url: impl Into<String>) -> HttpRpcClient {
    http_client_from_url_with_rps(endpoint_url, GROWTH_TIER_RPS)
}

/// Build an `HttpRpcClient` from a full endpoint URL with a custom RPS limit.
pub fn http_client_from_url_with_rps(endpoint_url: impl Into<String>, rps: f64) -> HttpRpcClient {
    HttpRpcClient::new(endpoint_url, default_config(rps))
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn default_config(rps: f64) -> HttpClientConfig {
    HttpClientConfig {
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
            capacity: rps,
            refill_rate: rps,
        },
        request_timeout: Duration::from_secs(30),
    }
}

/// Map an EVM chain ID to the Chainstack Global Node subdomain.
///
/// Chains with non-standard URL suffixes (Hyperliquid `/evm`, TON `/api/v2`,
/// TRON `/wallet`) are intentionally omitted — use [`http_client_from_url`]
/// with the full endpoint URL from the Chainstack console for those.
fn chain_id_to_network(chain_id: u64) -> &'static str {
    match chain_id {
        // Ethereum
        1 => "ethereum-mainnet",
        11155111 => "ethereum-sepolia",
        17000 => "ethereum-holesky",
        // Polygon
        137 => "polygon-mainnet",
        80002 => "polygon-amoy",
        // BNB Smart Chain
        56 => "bsc-mainnet",
        97 => "bsc-testnet",
        // Avalanche C-Chain
        43114 => "avalanche-mainnet",
        43113 => "avalanche-fuji",
        // Arbitrum
        42161 => "arbitrum-mainnet",
        421614 => "arbitrum-sepolia",
        // Optimism
        10 => "optimism-mainnet",
        11155420 => "optimism-sepolia",
        // Base
        8453 => "base-mainnet",
        84532 => "base-sepolia",
        // Sonic
        146 => "sonic-mainnet",
        // Unichain
        130 => "unichain-mainnet",
        // Ronin
        2020 => "ronin-mainnet",
        // Plasma
        9745 => "plasma-mainnet",
        // Tempo
        4217 => "tempo-mainnet",
        // Monad
        10143 => "monad-testnet",
        // Gnosis
        100 => "gnosis-mainnet",
        // Fantom
        250 => "fantom-mainnet",
        // Cronos
        25 => "cronos-mainnet",
        // zkSync Era
        324 => "zksync-mainnet",
        // Polygon zkEVM
        1101 => "polygon-zkevm-mainnet",
        // Linea
        59144 => "linea-mainnet",
        // Scroll
        534352 => "scroll-mainnet",
        // Mantle
        5000 => "mantle-mainnet",
        // Zora
        7777777 => "zora-mainnet",
        // Blast
        81457 => "blast-mainnet",
        // Berachain
        80094 => "berachain-mainnet",
        // opBNB
        204 => "opbnb-mainnet",
        // Kaia
        8217 => "kaia-mainnet",
        // Celo
        42220 => "celo-mainnet",
        // Moonbeam
        1284 => "moonbeam-mainnet",
        // Oasis Sapphire
        23294 => "oasis-sapphire-mainnet",
        // Harmony
        1666600000 => "harmony-mainnet",
        // MegaETH
        4326 => "megaeth-mainnet",
        _ => "ethereum-mainnet",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_http_url_ethereum() {
        assert_eq!(
            http_url("abc123", 1),
            "https://ethereum-mainnet.core.chainstack.com/abc123"
        );
    }

    #[test]
    fn global_http_url_polygon() {
        assert_eq!(
            http_url("key", 137),
            "https://polygon-mainnet.core.chainstack.com/key"
        );
    }

    #[test]
    fn global_ws_url() {
        let url = ws_url("token", 8453);
        assert_eq!(
            url,
            "wss://base-mainnet.core.chainstack.com/ws/token"
        );
    }

    #[test]
    fn global_url_unknown_chain_defaults_to_ethereum() {
        let url = http_url("key", 999999);
        assert!(url.contains("ethereum-mainnet"));
    }

    #[test]
    fn global_http_url_tempo() {
        assert_eq!(
            http_url("key", 4217),
            "https://tempo-mainnet.core.chainstack.com/key"
        );
    }

    #[test]
    fn global_http_url_berachain() {
        assert_eq!(
            http_url("key", 80094),
            "https://berachain-mainnet.core.chainstack.com/key"
        );
    }

    #[test]
    fn dedicated_node_from_url() {
        let url = "https://nd-123-456-789.p2pify.com/mykey";
        let client = http_client_from_url(url);
        assert_eq!(client.url(), url);
    }
}
