//! MEV protection — route transactions through private relays.
//!
//! Supports Flashbots Protect and other MEV-protection RPC endpoints.

use std::collections::HashMap;

/// MEV protection configuration.
#[derive(Debug, Clone)]
pub struct MevConfig {
    /// Whether MEV protection is enabled.
    pub enabled: bool,
    /// Private relay URL (e.g. Flashbots Protect).
    pub relay_url: Option<String>,
    /// Auto-detect MEV-susceptible transactions (swaps, liquidations).
    pub auto_detect: bool,
}

impl Default for MevConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            relay_url: None,
            auto_detect: true,
        }
    }
}

/// Well-known MEV protection relay endpoints.
pub fn relay_urls() -> HashMap<u64, &'static str> {
    let mut urls = HashMap::new();
    urls.insert(1, "https://rpc.flashbots.net"); // Ethereum mainnet
    urls.insert(5, "https://rpc-goerli.flashbots.net"); // Goerli testnet
    urls
}

/// Known function selectors that are MEV-susceptible.
///
/// These are common swap/trade selectors that front-runners target.
const MEV_SUSCEPTIBLE_SELECTORS: &[&str] = &[
    "0x38ed1739", // swapExactTokensForTokens (Uniswap V2)
    "0x8803dbee", // swapTokensForExactTokens (Uniswap V2)
    "0x7ff36ab5", // swapExactETHForTokens (Uniswap V2)
    "0x18cbafe5", // swapExactTokensForETH (Uniswap V2)
    "0x5ae401dc", // multicall (Uniswap V3 Router)
    "0xac9650d8", // multicall (Uniswap V3 Router)
    "0x04e45aaf", // exactInputSingle (Uniswap V3)
    "0xb858183f", // exactInput (Uniswap V3)
    "0x414bf389", // exactInputSingle (old Uniswap V3)
    "0xc04b8d59", // exactInput (old Uniswap V3)
    "0x2e1a7d4d", // withdraw (WETH — unwrap)
    "0xd0e30db0", // deposit (WETH — wrap, used in sandwiches)
];

/// Check if transaction calldata appears MEV-susceptible.
///
/// `input` is the hex-encoded transaction input data (with or without 0x prefix).
pub fn is_mev_susceptible(input: &str) -> bool {
    let input = input.strip_prefix("0x").unwrap_or(input);
    if input.len() < 8 {
        return false;
    }
    let selector = format!("0x{}", &input[..8]);
    MEV_SUSCEPTIBLE_SELECTORS.contains(&selector.as_str())
}

/// Determine if a transaction should be routed through the MEV relay.
pub fn should_use_relay(config: &MevConfig, input: &str) -> bool {
    if !config.enabled {
        return false;
    }
    if config.relay_url.is_none() {
        return false;
    }
    if config.auto_detect {
        is_mev_susceptible(input)
    } else {
        // When auto_detect is off but relay is enabled, always use relay
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_uniswap_v2_swap() {
        // swapExactTokensForTokens selector
        assert!(is_mev_susceptible("0x38ed1739000000000000000000000000"));
    }

    #[test]
    fn detect_uniswap_v3_multicall() {
        assert!(is_mev_susceptible("0x5ae401dc000000000000000000000000"));
    }

    #[test]
    fn non_mev_transaction() {
        // ERC20 transfer selector
        assert!(!is_mev_susceptible("0xa9059cbb000000000000000000000000"));
    }

    #[test]
    fn short_input() {
        assert!(!is_mev_susceptible("0x"));
        assert!(!is_mev_susceptible(""));
        assert!(!is_mev_susceptible("0x1234"));
    }

    #[test]
    fn should_use_relay_disabled() {
        let config = MevConfig::default(); // enabled = false
        assert!(!should_use_relay(&config, "0x38ed1739"));
    }

    #[test]
    fn should_use_relay_enabled_auto() {
        let config = MevConfig {
            enabled: true,
            relay_url: Some("https://rpc.flashbots.net".into()),
            auto_detect: true,
        };
        assert!(should_use_relay(&config, "0x38ed1739")); // swap
        assert!(!should_use_relay(&config, "0xa9059cbb")); // transfer
    }

    #[test]
    fn should_use_relay_always_when_no_autodetect() {
        let config = MevConfig {
            enabled: true,
            relay_url: Some("https://rpc.flashbots.net".into()),
            auto_detect: false,
        };
        assert!(should_use_relay(&config, "0xa9059cbb")); // even transfer
    }

    #[test]
    fn relay_urls_has_mainnet() {
        let urls = relay_urls();
        assert!(urls.contains_key(&1));
    }

    #[test]
    fn without_0x_prefix() {
        assert!(is_mev_susceptible("38ed1739000000000000000000000000"));
    }
}
