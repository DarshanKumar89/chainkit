//! # MEV Protection
//!
//! Demonstrates MEV detection and private relay routing.  Shows how
//! `is_mev_susceptible()` identifies swap calldata that front-runners target,
//! how `should_use_relay()` decides whether to route through Flashbots Protect,
//! and how `relay_urls()` provides well-known relay endpoints per chain.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use chainrpc_core::mev::{is_mev_susceptible, relay_urls, should_use_relay, MevConfig};

#[tokio::main]
async fn main() {
    println!("=== MEV Protection ===\n");

    // ── 1. Check MEV-susceptible calldata ────────────────────────────────────
    // The function inspects the first 4 bytes (function selector) of the
    // transaction input data against a list of known swap/trade selectors.

    // Uniswap V2: swapExactTokensForTokens (selector 0x38ed1739).
    let uniswap_v2_swap = "0x38ed173900000000000000000000000000000000000000000000000000038d7ea4c68000";
    let is_swap_mev = is_mev_susceptible(uniswap_v2_swap);
    println!("Uniswap V2 swapExactTokensForTokens:");
    println!("  calldata (truncated): {}...", &uniswap_v2_swap[..20]);
    println!("  is_mev_susceptible  : {is_swap_mev}");
    assert!(is_swap_mev, "Uniswap V2 swap should be MEV-susceptible");

    // Uniswap V3: exactInputSingle (selector 0x04e45aaf).
    let uniswap_v3_swap = "0x04e45aaf0000000000000000000000000000000000000000000000000000000000000001";
    println!("\nUniswap V3 exactInputSingle:");
    println!("  calldata (truncated): {}...", &uniswap_v3_swap[..20]);
    println!("  is_mev_susceptible  : {}", is_mev_susceptible(uniswap_v3_swap));
    assert!(is_mev_susceptible(uniswap_v3_swap));

    // Uniswap V3 Router: multicall (selector 0x5ae401dc).
    let multicall = "0x5ae401dc00000000000000000000000000000000000000000000000000000000deadbeef";
    println!("\nUniswap V3 Router multicall:");
    println!("  calldata (truncated): {}...", &multicall[..20]);
    println!("  is_mev_susceptible  : {}", is_mev_susceptible(multicall));
    assert!(is_mev_susceptible(multicall));

    // ── 2. Non-MEV transaction: simple ETH transfer ──────────────────────────
    // A plain ETH transfer has empty calldata ("0x").  This should NOT trigger
    // MEV detection.
    let eth_transfer = "0x";
    let is_transfer_mev = is_mev_susceptible(eth_transfer);
    println!("\nSimple ETH transfer (empty calldata):");
    println!("  calldata            : {eth_transfer}");
    println!("  is_mev_susceptible  : {is_transfer_mev}");
    assert!(!is_transfer_mev, "Plain transfer should NOT be MEV-susceptible");

    // An ERC-20 `transfer()` call (selector 0xa9059cbb) is also not in the
    // MEV-susceptible list — only swaps and trades are targeted.
    let erc20_transfer = "0xa9059cbb000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b";
    let is_erc20_mev = is_mev_susceptible(erc20_transfer);
    println!("\nERC-20 transfer:");
    println!("  calldata (truncated): {}...", &erc20_transfer[..20]);
    println!("  is_mev_susceptible  : {is_erc20_mev}");
    assert!(!is_erc20_mev, "ERC-20 transfer should NOT be MEV-susceptible");

    // ── 3. Relay routing decisions ───────────────────────────────────────────
    // `should_use_relay()` combines the MevConfig (enabled, relay_url,
    // auto_detect) with the calldata check.

    // Config: MEV protection enabled, Flashbots relay URL, auto-detect on.
    let config = MevConfig {
        enabled: true,
        relay_url: Some("https://rpc.flashbots.net".to_string()),
        auto_detect: true,
    };

    // Swap transaction -> should use the relay.
    let route_swap = should_use_relay(&config, uniswap_v2_swap);
    println!("\nRelay routing (auto_detect=true):");
    println!("  Uniswap V2 swap  -> use relay: {route_swap}");
    assert!(route_swap);

    // ERC-20 transfer -> should NOT use the relay (not MEV-susceptible).
    let route_transfer = should_use_relay(&config, erc20_transfer);
    println!("  ERC-20 transfer  -> use relay: {route_transfer}");
    assert!(!route_transfer);

    // Config: auto_detect OFF — always routes through relay when enabled.
    let always_relay_config = MevConfig {
        enabled: true,
        relay_url: Some("https://rpc.flashbots.net".to_string()),
        auto_detect: false,
    };

    let route_always = should_use_relay(&always_relay_config, erc20_transfer);
    println!("\nRelay routing (auto_detect=false):");
    println!("  ERC-20 transfer  -> use relay: {route_always}  (always routes)");
    assert!(route_always);

    // Config: MEV protection disabled — never routes.
    let disabled_config = MevConfig::default(); // enabled = false
    let route_disabled = should_use_relay(&disabled_config, uniswap_v2_swap);
    println!("\nRelay routing (enabled=false):");
    println!("  Uniswap V2 swap  -> use relay: {route_disabled}  (protection off)");
    assert!(!route_disabled);

    // ── 4. Well-known relay URLs ─────────────────────────────────────────────
    // `relay_urls()` returns a map of chain_id -> relay endpoint.
    let relays = relay_urls();
    println!("\nKnown relay endpoints:");
    for (chain_id, url) in &relays {
        println!("  chain {chain_id}: {url}");
    }
    assert!(relays.contains_key(&1), "Should contain Ethereum mainnet");
    println!("\n[OK] Mainnet relay registered");

    // ── 5. Input without 0x prefix ───────────────────────────────────────────
    // The detector handles calldata with or without the "0x" prefix.
    let no_prefix = "38ed173900000000000000000000000000000000";
    assert!(is_mev_susceptible(no_prefix));
    println!("\n[OK] Detection works without 0x prefix");

    println!("\nDone.");
}
