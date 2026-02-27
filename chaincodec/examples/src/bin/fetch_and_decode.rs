//! # fetch_and_decode
//!
//! Demonstrates fetching a contract ABI from Sourcify (no API key required),
//! then decoding a known ERC-20 Transfer event from that ABI.
//!
//! Workflow:
//!   1. Fetch ABI for USDC from Sourcify (decentralized ABI registry)
//!   2. Look up the Transfer event signature via 4byte.directory
//!   3. Show how to manually construct a `RawEvent` and decode it
//!      using the built-in ERC-20 schema as a fallback
//!
//! Run with:
//! ```sh
//! cargo run --bin fetch_and_decode
//! ```
//!
//! Note: This example requires network access. The Sourcify lookup may return
//! "not found" for some contracts; in that case the example falls back to a
//! built-in schema and still demonstrates the decode path.

use anyhow::Result;
use chaincodec_core::{
    chain::chains,
    event::{EventFingerprint, RawEvent},
    schema::SchemaRegistry,
};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{AbiFetcher, CsdlParser, MemoryRegistry};

#[tokio::main]
async fn main() -> Result<()> {
    // ── 1. Initialize AbiFetcher ──────────────────────────────────────────────
    // Sourcify is used first (no API key), Etherscan as fallback.
    // Set ETHERSCAN_API_KEY env var to enable Etherscan fallback.
    let mut fetcher = AbiFetcher::new();
    if let Ok(key) = std::env::var("ETHERSCAN_API_KEY") {
        fetcher = fetcher.with_etherscan_key(key);
        println!("✓ Etherscan API key set (will use as fallback)");
    }

    // USDC proxy on Ethereum mainnet
    let chain_id: u64 = 1;
    let usdc_address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

    // ── 2. Fetch ABI from Sourcify ────────────────────────────────────────────
    println!("→ Fetching ABI for USDC from Sourcify...");
    match fetcher.fetch_from_sourcify(chain_id, usdc_address).await {
        Ok(abi_json) => {
            // Show a preview of the ABI (first 200 chars)
            let preview = if abi_json.len() > 200 {
                format!("{}...", &abi_json[..200])
            } else {
                abi_json.clone()
            };
            println!("✓ ABI fetched from Sourcify ({} bytes)", abi_json.len());
            println!("  Preview: {preview}");
        }
        Err(e) => {
            println!("  Sourcify lookup: {e} (will use built-in schema)");
        }
    }

    // ── 3. Look up the Transfer event signature on 4byte.directory ────────────
    let transfer_topic0 = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    println!("\n→ Looking up Transfer signature on 4byte.directory...");
    match fetcher.lookup_event_signature(transfer_topic0).await {
        Ok(results) if !results.is_empty() => {
            println!("✓ Found {} match(es):", results.len());
            for r in results.iter().take(3) {
                println!("  [{}] {}", r.id, r.text_signature);
            }
        }
        Ok(_) => {
            println!("  No matches found on 4byte.directory");
        }
        Err(e) => {
            println!("  4byte.directory lookup: {e}");
        }
    }

    // ── 4. Build a registry with the built-in ERC-20 schema ───────────────────
    // In a real application you would parse the fetched ABI into a Schema.
    // Here we use the canonical CSDL schema as a practical demonstration.
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
    println!("\n✓ Registry loaded ({} schemas)", registry.len());

    // ── 5. Construct a RawEvent (mirrors a real on-chain USDC Transfer) ────────
    //   from:  vitalik.eth  (0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045)
    //   to:    0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
    //   value: 50,000,000 = 50 USDC (6 decimals)
    let raw = RawEvent {
        chain: chains::ethereum(),
        tx_hash: "0xf4b5e3a2c1d0e9f8a7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d2e1f0a9b8c7d6e5f4".into(),
        block_number: 19_500_000,
        block_timestamp: 1_715_000_000,
        log_index: 12,
        address: usdc_address.into(),
        topics: vec![
            transfer_topic0.into(),
            // from: vitalik.eth — 32-byte padded
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
            // to: — 32-byte padded
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
        ],
        // value = 50_000_000 = 0x02FAF080
        data: hex::decode(
            "0000000000000000000000000000000000000000000000000000000002FAF080",
        )?,
        raw_receipt: None,
    };

    println!("\n─── Raw Event ───────────────────────────────────────────");
    println!("  address:   {}", raw.address);
    println!("  topics[0]: {}", raw.topics[0]);
    println!("  topics[1]: {}", raw.topics[1]);
    println!("  topics[2]: {}", raw.topics[2]);
    println!("  data:      {} bytes", raw.data.len());

    // ── 6. Decode ─────────────────────────────────────────────────────────────
    let fp = EventFingerprint::new(raw.topics[0].clone());
    let schema = registry
        .get_by_fingerprint(&fp)
        .expect("schema not found — fingerprint mismatch");

    println!("\n✓ Matched schema: {} v{}", schema.name, schema.version);

    let decoder = EvmDecoder::new();
    let decoded = decoder.decode_event(&raw, &schema)?;

    // ── 7. Print results ──────────────────────────────────────────────────────
    println!("\n─── Decoded Event ───────────────────────────────────────");
    println!("  schema:    {} v{}", decoded.schema, schema.version);
    println!("  chain:     {}", decoded.chain);
    println!("  block:     #{}", decoded.block_number);
    println!("  tx:        {}", decoded.tx_hash);
    println!("  address:   {}", decoded.address);
    println!();

    let mut field_names: Vec<_> = decoded.fields.keys().collect();
    field_names.sort();
    for name in field_names {
        println!("  {:12} = {}", name, decoded.fields[name]);
    }

    if decoded.has_errors() {
        println!("\n⚠ Decode errors:");
        for (field, err) in &decoded.decode_errors {
            println!("  {field}: {err}");
        }
    } else {
        println!("\n✓ All fields decoded successfully");
    }

    println!("\n─── Schema Metadata ─────────────────────────────────────");
    println!("  protocol:    {}", schema.meta.protocol.as_deref().unwrap_or("-"));
    println!("  category:    {}", schema.meta.category.as_deref().unwrap_or("-"));
    println!("  verified:    {}", schema.meta.verified);
    println!("  trust_level: {}", schema.meta.trust_level);

    Ok(())
}
