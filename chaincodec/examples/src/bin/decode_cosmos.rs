//! # decode_cosmos
//!
//! Demonstrates decoding a Cosmos/CosmWasm event with `CosmosDecoder`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §6 — Cross-chain)
//! - Decode ABCI wasm events without chain-specific attribute parsers
//! - Output is the same `NormalizedValue` as EVM and Solana decoders
//! - Enables cross-chain stablecoin tracking (USDC on ETH + axlUSDC on Osmosis)
//!
//! Run with:
//! ```sh
//! cargo run --bin decode_cosmos
//! ```
//!
//! ## Cosmos RawEvent mapping recap
//! - `topics[0]`: ABCI event type (e.g. `"wasm"`)
//! - `topics[1]`: CosmWasm action (e.g. `"transfer"`)
//! - `data`:      JSON-encoded attribute list `[{"key":"...","value":"..."}]`
//! - `address`:   bech32 contract address

use anyhow::Result;
use chaincodec_core::{
    chain::ChainId,
    decoder::ChainDecoder as _,
    event::RawEvent,
    schema::SchemaRegistry,
};
use chaincodec_cosmos::CosmosDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};

// CSDL for a CosmWasm CW-20 transfer event (wasm/transfer action)
// fingerprint = SHA-256("event:wasm/transfer")[..16] — computed at runtime
const CW20_TRANSFER_CSDL_TEMPLATE: &str = r#"
schema Cw20Transfer:
  version: 1
  chains: [osmosis, cosmos, juno, terra]
  event: wasm/transfer
  fingerprint: "{FINGERPRINT}"
  fields:
    amount:    { type: uint128,       indexed: false }
    from:      { type: bech32address, indexed: false }
    to:        { type: bech32address, indexed: false }
    contract:  { type: str,           indexed: false }
  meta:
    protocol: cw20
    category: token
    verified: true
    trust_level: maintainer_verified
"#;

// CSDL for Osmosis poolmanager swap event
const OSMOSIS_SWAP_CSDL_TEMPLATE: &str = r#"
schema OsmosisSwap:
  version: 1
  chains: [osmosis]
  event: wasm/token_swapped
  fingerprint: "{FINGERPRINT}"
  fields:
    sender:       { type: bech32address, indexed: false }
    pool_id:      { type: uint64,        indexed: false }
    tokens_in:    { type: str,           indexed: false }
    tokens_out:   { type: str,           indexed: false }
  meta:
    protocol: osmosis-poolmanager
    category: dex
    verified: true
    trust_level: maintainer_verified
"#;

fn main() -> Result<()> {
    println!("ChainCodec — Cosmos/CosmWasm Decoder");
    println!("═══════════════════════════════════════════════════════");

    let decoder = CosmosDecoder::new();

    // ── 1. Compute fingerprints ───────────────────────────────────────────────
    let cw20_fp   = CosmosDecoder::fingerprint_for("wasm/transfer");
    let osmosis_fp = CosmosDecoder::fingerprint_for("wasm/token_swapped");

    println!("\nFingerprints:");
    println!("  wasm/transfer      → {}", cw20_fp.as_hex());
    println!("  wasm/token_swapped → {}", osmosis_fp.as_hex());

    // ── 2. Load schemas ───────────────────────────────────────────────────────
    let registry = MemoryRegistry::new();

    for csdl in [
        CW20_TRANSFER_CSDL_TEMPLATE.replace("{FINGERPRINT}", cw20_fp.as_hex()),
        OSMOSIS_SWAP_CSDL_TEMPLATE.replace("{FINGERPRINT}", osmosis_fp.as_hex()),
    ] {
        for schema in CsdlParser::parse_all(&csdl)? {
            registry.add(schema)?;
        }
    }
    println!("  ✓ {} schemas registered", registry.len());

    // ── 3. Build a CW-20 Transfer raw event ──────────────────────────────────
    //
    // Cosmos ABCI event attributes come as a JSON array of key/value pairs.
    // CosmosDecoder accepts both array format and object format.
    let cw20_attrs = serde_json::json!([
        {"key": "_contract_address", "value": "osmo1qwerty1234567890abcdef"},
        {"key": "action",            "value": "transfer"},
        {"key": "amount",            "value": "1000000"},  // 1 USDC (6 decimals)
        {"key": "from",              "value": "osmo1aabbccddeeff00112233445566778899aabbcc"},
        {"key": "to",                "value": "osmo1ffeeddccbbaa99887766554433221100ffeedd"},
        {"key": "contract",          "value": "osmo1qwerty1234567890abcdef"}
    ]);

    let osmosis_chain = ChainId::cosmos("osmosis");

    let cw20_raw = RawEvent {
        chain: osmosis_chain.clone(),
        tx_hash: "ABC123DEF456...".into(),
        block_number: 10_500_000,
        block_timestamp: 1_710_000_000,
        log_index: 0,
        address: "osmo1qwerty1234567890abcdef".into(),
        topics: vec![
            "wasm".to_string(),      // topics[0] = ABCI event type
            "transfer".to_string(),  // topics[1] = CosmWasm action
        ],
        data: serde_json::to_vec(&cw20_attrs)?,
        raw_receipt: None,
    };

    // ── 4. Fingerprint routing ────────────────────────────────────────────────
    let detected_fp = decoder.fingerprint(&cw20_raw);
    let schema = registry
        .get_by_fingerprint(&detected_fp)
        .expect("CW-20 schema not found");

    println!("\n─── CW-20 Transfer Event ────────────────────────────");
    println!("  fingerprint: {}", detected_fp.as_hex());
    println!("  schema:      {} v{}", schema.name, schema.version);

    let decoded = decoder.decode_event(&cw20_raw, &schema)?;

    let mut names: Vec<_> = decoded.fields.keys().collect();
    names.sort();
    for name in &names {
        println!("  {:10} = {}", name, decoded.fields[*name]);
    }

    if decoded.has_errors() {
        println!("  ⚠  field errors: {:?}", decoded.decode_errors.keys().collect::<Vec<_>>());
    } else {
        println!("  ✓ clean decode");
    }

    // ── 5. Osmosis swap event ─────────────────────────────────────────────────
    let swap_attrs = serde_json::json!([
        {"key": "sender",     "value": "osmo1aabbccddeeff00112233445566778899aabbcc"},
        {"key": "pool_id",    "value": "1"},
        {"key": "tokens_in",  "value": "1000000uosmo"},
        {"key": "tokens_out", "value": "500000uatom"}
    ]);

    let osmosis_swap = RawEvent {
        chain: osmosis_chain.clone(),
        tx_hash: "DEF789GHI012...".into(),
        block_number: 10_500_001,
        block_timestamp: 1_710_000_012,
        log_index: 1,
        address: "osmo1poolmanager".into(),
        topics: vec![
            "wasm".to_string(),
            "token_swapped".to_string(),
        ],
        data: serde_json::to_vec(&swap_attrs)?,
        raw_receipt: None,
    };

    let swap_schema = registry
        .get_by_fingerprint(&decoder.fingerprint(&osmosis_swap))
        .expect("swap schema not found");

    println!("\n─── Osmosis Swap Event ──────────────────────────────");
    println!("  schema: {} v{}", swap_schema.name, swap_schema.version);
    let decoded_swap = decoder.decode_event(&osmosis_swap, &swap_schema)?;
    let mut swap_names: Vec<_> = decoded_swap.fields.keys().collect();
    swap_names.sort();
    for name in &swap_names {
        println!("  {:12} = {}", name, decoded_swap.fields[*name]);
    }

    // ── 6. Cross-chain type parity note ──────────────────────────────────────
    println!("\n─── Cross-chain output comparison ───────────────────");
    println!("  EVM    ERC-20 Transfer → fields['value']  = NormalizedValue::Uint(1000000)");
    println!("  Cosmos CW-20 Transfer  → fields['amount'] = NormalizedValue::Uint(1000000)");
    println!("  Solana SPL Token xfer  → fields['amount'] = NormalizedValue::Uint(1000000)");
    println!("  ─── same downstream consumer for all three ───────────────────────");

    println!("\n✓ Cosmos/CosmWasm decode complete");
    Ok(())
}
