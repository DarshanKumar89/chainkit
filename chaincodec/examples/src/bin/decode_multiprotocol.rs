//! # decode_multiprotocol
//!
//! Demonstrates decoding events from multiple protocols (ERC-20, Uniswap V3,
//! Aave V3) in a single `BatchEngine` call.
//!
//! ## Use-case coverage (chaincodec-usecase.md §1 — Indexer / §2 — DeFi Analytics)
//! - One BatchEngine call → events from 3 different protocols, all decoded
//! - No per-protocol branching: schema fingerprint handles routing automatically
//! - Summarise decoded events by protocol/category for analytics dashboards
//!
//! Run with:
//! ```sh
//! cargo run --bin decode_multiprotocol
//! ```

use anyhow::Result;
use chaincodec_batch::{BatchEngine, BatchRequest};
use chaincodec_core::{
    chain::chains,
    decoder::{ChainDecoder, ErrorMode},
    event::RawEvent,
    schema::SchemaRegistry,
};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use std::sync::Arc;

// ── CSDL schemas (inline for self-contained example) ─────────────────────────

const ERC20_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
"#;

// fingerprint = keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)")
const UNISWAP_V3_CSDL: &str = r#"
schema UniswapV3Swap:
  version: 2
  chains: [ethereum, arbitrum, polygon, base, optimism]
  event: Swap
  fingerprint: "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"
  fields:
    sender:       { type: address, indexed: true  }
    recipient:    { type: address, indexed: true  }
    amount0:      { type: int256,  indexed: false }
    amount1:      { type: int256,  indexed: false }
    sqrtPriceX96: { type: uint160, indexed: false }
    liquidity:    { type: uint128, indexed: false }
    tick:         { type: int24,   indexed: false }
  meta:
    protocol: uniswap-v3
    category: dex
    verified: true
    trust_level: maintainer_verified
"#;

// fingerprint = keccak256("Borrow(address,address,address,uint256,uint8,uint256,uint16)")
const AAVE_V3_CSDL: &str = r#"
schema AaveV3Borrow:
  version: 1
  chains: [ethereum, arbitrum, polygon]
  event: Borrow
  fingerprint: "0xb3d084820fb1a9decffb176436bd02b1b4a5d2b7a2b6eca9e6d6dda0da1f89"
  fields:
    reserve:          { type: address, indexed: true  }
    user:             { type: address, indexed: false }
    onBehalfOf:       { type: address, indexed: true  }
    amount:           { type: uint256, indexed: false }
    interestRateMode: { type: uint256, indexed: false }
    borrowRate:       { type: uint256, indexed: false }
    referralCode:     { type: uint16,  indexed: true  }
  meta:
    protocol: aave-v3
    category: lending
    verified: true
    trust_level: maintainer_verified
"#;

fn main() -> Result<()> {
    println!("ChainCodec — Multi-Protocol Batch Decode");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Load all three schemas into one registry ───────────────────────────
    let registry = Arc::new(MemoryRegistry::new());
    for csdl in [ERC20_CSDL, UNISWAP_V3_CSDL, AAVE_V3_CSDL] {
        for schema in CsdlParser::parse_all(csdl)? {
            registry.add(schema)?;
        }
    }
    println!("\n✓ Registry loaded: {} schemas", registry.len());
    for schema in registry.all_schemas() {
        println!(
            "  • {} v{} ({})",
            schema.name,
            schema.version,
            schema.meta.protocol.as_deref().unwrap_or("unknown"),
        );
    }

    // ── 2. Create raw events from three different protocols ───────────────────
    let eth = chains::ethereum();

    // Event A: ERC-20 Transfer (USDC, from Binance to Vitalik)
    let erc20_transfer = RawEvent {
        chain: eth.clone(),
        tx_hash: "0xaaa111".into(),
        block_number: 19_500_000,
        block_timestamp: 1_710_000_000,
        log_index: 0,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(), // USDC
        topics: vec![
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
            "0x00000000000000000000000028c6c06298d514db089934071355e5743bf21d60".into(), // Binance 14
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(), // Vitalik
        ],
        // value = 100_000_000_000 (100,000 USDC)
        data: hex::decode(
            "000000000000000000000000000000000000000000000000000000174876e800",
        )?,
        raw_receipt: None,
    };

    // Event B: Uniswap V3 Swap (ETH/USDC pool)
    // Non-indexed: amount0 (int256), amount1 (int256), sqrtPriceX96 (uint160), liquidity (uint128), tick (int24)
    let uniswap_swap = RawEvent {
        chain: eth.clone(),
        tx_hash: "0xbbb222".into(),
        block_number: 19_500_001,
        block_timestamp: 1_710_000_012,
        log_index: 3,
        address: "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640".into(), // USDC/ETH 0.05% pool
        topics: vec![
            "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67".into(),
            "0x000000000000000000000000e592427a0aece92de3edee1f18e0157c05861564".into(), // sender (router)
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(), // recipient
        ],
        data: hex::decode(concat!(
            // amount0 = -1 (int256 negative, sold 1 unit token0)
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            // amount1 = 1 (int256, received 1 unit token1)
            "0000000000000000000000000000000000000000000000000000000000000001",
            // sqrtPriceX96 = 2^96 (price = 1.0 in Q64.96)
            "0000000000000000000000000000000000000001000000000000000000000000",
            // liquidity = 1_000_000_000_000 (uint128)
            "000000000000000000000000000000000000000000000000000000e8d4a51000",
            // tick = 100 (int24)
            "0000000000000000000000000000000000000000000000000000000000000064",
        ))?,
        raw_receipt: None,
    };

    // Event C: Aave V3 Borrow (borrow USDC)
    // Non-indexed: user (address), amount (uint256), interestRateMode (uint256), borrowRate (uint256)
    let aave_borrow = RawEvent {
        chain: eth.clone(),
        tx_hash: "0xccc333".into(),
        block_number: 19_500_002,
        block_timestamp: 1_710_000_024,
        log_index: 7,
        address: "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2".into(), // Aave V3 Pool
        topics: vec![
            "0xb3d084820fb1a9decffb176436bd02b1b4a5d2b7a2b6eca9e6d6dda0da1f89".into(),
            "0x000000000000000000000000a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(), // reserve = USDC
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(), // onBehalfOf
            "0x0000000000000000000000000000000000000000000000000000000000000000".into(), // referralCode = 0
        ],
        data: hex::decode(concat!(
            // user = 0xd8da6bf26964af9d7eed9e03e53415d37aa96045
            "000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045",
            // amount = 10_000_000 (10 USDC)
            "0000000000000000000000000000000000000000000000000000000000989680",
            // interestRateMode = 2 (variable)
            "0000000000000000000000000000000000000000000000000000000000000002",
            // borrowRate = 1000 (placeholder)
            "00000000000000000000000000000000000000000000000000000000000003e8",
        ))?,
        raw_receipt: None,
    };

    // ── 3. Route events to schemas (fingerprint lookup) ───────────────────────
    println!("\nFingerprint → Schema routing:");
    let ev_decoder = EvmDecoder::new();
    for (label, raw) in [
        ("ERC-20 Transfer", &erc20_transfer),
        ("Uniswap V3 Swap", &uniswap_swap),
        ("Aave V3 Borrow",  &aave_borrow),
    ] {
        let fp = ev_decoder.fingerprint(raw);
        let schema = registry.get_by_fingerprint(&fp);
        println!(
            "  {:<18} fp={}... → {}",
            label,
            &fp.as_hex()[..18],
            schema.as_ref().map(|s| s.name.as_str()).unwrap_or("NOT FOUND")
        );
    }

    // ── 4. Batch decode all three events ──────────────────────────────────────
    // BatchEngine::new() takes just the registry; register a decoder per chain slug.
    let mut batch_engine = BatchEngine::new(registry.clone());
    batch_engine.add_decoder(
        "ethereum",
        Arc::new(EvmDecoder::new()) as Arc<dyn ChainDecoder>,
    );

    // BatchRequest::new(chain_slug, logs) — all events are on Ethereum here.
    let request = BatchRequest::new(
        "ethereum",
        vec![erc20_transfer, uniswap_swap, aave_borrow],
    )
    .error_mode(ErrorMode::Collect); // collect field errors instead of aborting

    let result = batch_engine.decode(request)?;

    println!("\n─── Batch Decode Results ────────────────────────────");
    let skipped = result.total_input - result.events.len() - result.errors.len();
    println!(
        "  decoded: {}  errors: {}  skipped: {}",
        result.events.len(),
        result.errors.len(),
        skipped,
    );

    // ── 5. Group by protocol for analytics ───────────────────────────────────
    println!("\n─── By Protocol ─────────────────────────────────────");
    let mut by_protocol: std::collections::HashMap<String, Vec<_>> = Default::default();

    for event in &result.events {
        // event.fingerprint is already an EventFingerprint — pass it directly
        if let Some(schema) = registry.get_by_fingerprint(&event.fingerprint) {
            let protocol = schema.meta.protocol.as_deref().unwrap_or("unknown").to_string();
            by_protocol
                .entry(protocol)
                .or_default()
                .push(event);
        }
    }

    let mut protocol_names: Vec<_> = by_protocol.keys().collect();
    protocol_names.sort();
    for protocol in protocol_names {
        println!("  {protocol}:");
        for ev in &by_protocol[protocol] {
            println!("    tx={} block=#{}", &ev.tx_hash[..8], ev.block_number);
            let mut names: Vec<_> = ev.fields.keys().collect();
            names.sort();
            for n in names {
                println!("      {:16} = {}", n, ev.fields[n]);
            }
        }
    }

    if !result.errors.is_empty() {
        println!("\n─── Field Decode Errors (collected) ─────────────────");
        // errors is Vec<(usize, DecodeError)> — index of the raw event + error
        for (event_index, err) in &result.errors {
            println!("  event[{}]: {}", event_index, err);
        }
    }

    println!("\n✓ Multi-protocol batch decode complete");
    Ok(())
}
