//! # decode_solana
//!
//! Demonstrates decoding a Solana/Anchor event with `SolanaDecoder`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §6 — Cross-chain)
//! - Decode Anchor program events without chain-specific parsers
//! - Output is the same `NormalizedValue` type as EVM — one consumer for all chains
//! - Suitable for cross-chain DEX aggregation, bridge monitoring, stablecoin tracking
//!
//! Run with:
//! ```sh
//! cargo run --bin decode_solana
//! ```
//!
//! ## Solana/Anchor event format recap
//! - `topics[0]`: hex Anchor discriminator = SHA-256("event:<EventName>")[..8]
//! - `data`: Borsh-encoded payload (fields in schema order, NO discriminator prefix)

use anyhow::Result;
use chaincodec_core::{
    chain::chains,
    decoder::ChainDecoder,
    event::{EventFingerprint, RawEvent},
    schema::SchemaRegistry,
};
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_solana::SolanaDecoder;

// Inline CSDL for a simple Anchor token transfer event
// The fingerprint MUST match SolanaDecoder::fingerprint_for("AnchorTransfer")
// which is SHA-256("event:AnchorTransfer")[..8] as hex.
// We compute it at runtime in main() and print it for reference.
const ANCHOR_TRANSFER_CSDL_TEMPLATE: &str = r#"
schema AnchorTransfer:
  version: 1
  chains: [solana]
  event: AnchorTransfer
  fingerprint: "{FINGERPRINT}"
  fields:
    from:   { type: pubkey,  indexed: false }
    to:     { type: pubkey,  indexed: false }
    amount: { type: uint64,  indexed: false }
  meta:
    protocol: spl-token
    category: token
    verified: true
    trust_level: maintainer_verified
"#;

fn main() -> Result<()> {
    println!("ChainCodec — Solana/Anchor Decoder");
    println!("═══════════════════════════════════════════════════════");

    let decoder = SolanaDecoder::new();

    // ── 1. Compute the fingerprint for our event ──────────────────────────────
    let fp = SolanaDecoder::fingerprint_for("AnchorTransfer");
    println!("\nAnchor discriminator for 'AnchorTransfer':");
    println!("  SHA-256('event:AnchorTransfer')[..8] = {}", fp.as_hex());

    // ── 2. Load the CSDL schema with the computed fingerprint ─────────────────
    let csdl = ANCHOR_TRANSFER_CSDL_TEMPLATE.replace("{FINGERPRINT}", fp.as_hex());
    let registry = MemoryRegistry::new();
    for schema in CsdlParser::parse_all(&csdl)? {
        registry.add(schema)?;
    }
    println!("  schema registered: AnchorTransfer v1");

    // ── 3. Build Borsh-encoded payload ────────────────────────────────────────
    //
    // Field order: from (pubkey = 32 bytes), to (pubkey = 32 bytes), amount (u64 LE = 8 bytes)
    // Borsh encoding:
    //   - Pubkey: 32 raw bytes
    //   - u64:    8 bytes, little-endian
    let from_pubkey  = [0x11u8; 32]; // placeholder pubkey (32 bytes all 0x11)
    let to_pubkey    = [0x22u8; 32]; // placeholder pubkey (32 bytes all 0x22)
    let amount: u64  = 5_000_000;   // 5 SOL in lamports (1 SOL = 10^9 lamports)
    let amount_bytes = amount.to_le_bytes();

    let mut payload = Vec::new();
    payload.extend_from_slice(&from_pubkey);
    payload.extend_from_slice(&to_pubkey);
    payload.extend_from_slice(&amount_bytes);

    println!("\n  Borsh payload: {} bytes (32 + 32 + 8)", payload.len());
    println!("  from (pubkey):  {}", hex::encode(&from_pubkey[..8]));
    println!("  to   (pubkey):  {}", hex::encode(&to_pubkey[..8]));
    println!("  amount (u64 LE): {amount} lamports");

    // ── 4. Build the RawEvent (Solana format) ─────────────────────────────────
    let raw = RawEvent {
        chain: chains::solana_mainnet(),
        tx_hash: "5KTr4jk9FXiEQqEWd3fZvCmF7xk1MrJgNhfCXpGa7BZxxxxxxxxxxxxxxxxxxxxxxxxxxx".into(),
        block_number: 250_000_000,
        block_timestamp: 1_710_000_000,
        log_index: 0,
        address: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".into(), // SPL Token program
        topics: vec![fp.as_hex().to_string()], // topics[0] = discriminator hex
        data: payload,
        raw_receipt: None,
    };

    // ── 5. Fingerprint routing (same as EVM) ──────────────────────────────────
    let detected_fp = decoder.fingerprint(&raw);
    println!("\n  Fingerprint from raw event: {}", detected_fp.as_hex());
    assert_eq!(detected_fp.as_hex(), fp.as_hex(), "fingerprint mismatch");

    let schema = registry
        .get_by_fingerprint(&detected_fp)
        .expect("schema not found");
    println!("  Matched schema: {} v{}", schema.name, schema.version);

    // ── 6. Decode ─────────────────────────────────────────────────────────────
    let decoded = decoder.decode_event(&raw, &schema)?;

    println!("\n─── Decoded Solana Event ────────────────────────────");
    println!("  schema:  {}", decoded.schema);
    println!("  chain:   {}", decoded.chain);
    println!("  block:   #{}", decoded.block_number);
    println!("  program: {}", decoded.address);
    println!();

    let mut names: Vec<_> = decoded.fields.keys().collect();
    names.sort();
    for name in &names {
        println!("  {:8} = {}", name, decoded.fields[*name]);
    }

    if decoded.has_errors() {
        println!("\n⚠  Field errors:");
        for (f, e) in &decoded.decode_errors {
            println!("  {f}: {e}");
        }
    } else {
        println!("\n✓ All fields decoded (NormalizedValue — same type as EVM output)");
    }

    // ── 7. Cross-chain comparison ─────────────────────────────────────────────
    println!("\n─── Cross-chain type parity ─────────────────────────");
    println!("  Solana Pubkey  → NormalizedValue::Pubkey(base58_string)");
    println!("  Solana u64     → NormalizedValue::Uint(u128)");
    println!("  EVM address    → NormalizedValue::Address(hex_checksummed)");
    println!("  EVM uint256    → NormalizedValue::BigUint(decimal_string)");
    println!("  ─── same consumer handles both ───────────────────────");
    println!("  decoded.fields['amount'] works identically for both chains");

    println!("\n✓ Solana decode complete");
    Ok(())
}
