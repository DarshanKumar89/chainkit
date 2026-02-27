//! # decode_erc20
//!
//! Demonstrates decoding a real ERC-20 Transfer event from raw log data.
//!
//! Run with:
//! ```sh
//! cargo run --bin decode_erc20
//! ```

use anyhow::Result;
use chaincodec_core::{
    chain::chains,
    decoder::ChainDecoder,
    event::{EventFingerprint, RawEvent},
};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_core::schema::SchemaRegistry;

fn main() -> Result<()> {
    // ── 1. Define the CSDL schema inline ─────────────────────────────────────
    let csdl = r#"
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

    let registry = MemoryRegistry::new();
    for schema in CsdlParser::parse_all(csdl)? {
        registry.add(schema)?;
    }
    println!("✓ Registry loaded ({} schemas)", registry.len());

    // ── 2. Construct a raw ERC-20 Transfer log ────────────────────────────────
    // This matches the well-known USDC Transfer event structure:
    //   from:  0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
    //   to:    0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
    //   value: 1,000,000,000 (1000 USDC at 6 decimals)
    let raw = RawEvent {
        chain: chains::ethereum(),
        tx_hash: "0xa9d1e08c7793af67e9d92fe308d5697fb81d3e43ce35d8ba6dd0ebc4e3b7f3e2".into(),
        block_number: 19_000_000,
        block_timestamp: 1_700_000_000,
        log_index: 4,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
        topics: vec![
            // topics[0] = keccak256("Transfer(address,address,uint256)")
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
            // topics[1] = from address (indexed, 32-byte padded)
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
            // topics[2] = to address (indexed, 32-byte padded)
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
        ],
        // data = ABI-encoded value: 1,000,000,000 = 0x3B9ACA00
        data: hex::decode(
            "000000000000000000000000000000000000000000000000000000003b9aca00",
        )?,
        raw_receipt: None,
    };

    // ── 3. Look up the schema by fingerprint ──────────────────────────────────
    let fp = EventFingerprint::new(raw.topics[0].clone());
    let schema = registry
        .get_by_fingerprint(&fp)
        .expect("schema not found — check fingerprint");
    println!("✓ Matched schema: {} v{}", schema.name, schema.version);

    // ── 4. Decode with EvmDecoder ─────────────────────────────────────────────
    let decoder = EvmDecoder::new();
    let decoded = decoder.decode_event(&raw, &schema)?;

    // ── 5. Print results ──────────────────────────────────────────────────────
    println!("\n─── Decoded Event ───────────────────────────────");
    println!("  schema:    {}", decoded.schema);
    println!("  chain:     {}", decoded.chain);
    println!("  tx_hash:   {}", decoded.tx_hash);
    println!("  block:     #{}", decoded.block_number);
    println!("  address:   {}", decoded.address);
    println!();

    let mut field_names: Vec<_> = decoded.fields.keys().collect();
    field_names.sort();
    for name in field_names {
        println!("  {:12} = {}", name, decoded.fields[name]);
    }

    if decoded.has_errors() {
        println!("\n⚠️  Decode errors:");
        for (field, err) in &decoded.decode_errors {
            println!("  {field}: {err}");
        }
    } else {
        println!("\n✓ All fields decoded successfully");
    }

    Ok(())
}
