//! # csdl_registry
//!
//! Demonstrates the CSDL parser and `MemoryRegistry` schema management.
//!
//! ## Use-case coverage (chaincodec-usecase.md §3 — Protocol SDK)
//! - Write your event schema once in CSDL YAML → decoders in 4 languages
//! - Multi-document YAML: multiple schemas in one file
//! - Version evolution: load v1 + v2 schemas, query latest
//! - Registry operations: add, lookup by fingerprint, list all
//!
//! Run with:
//! ```sh
//! cargo run --bin csdl_registry
//! ```

use anyhow::Result;
use chaincodec_core::{event::EventFingerprint, schema::SchemaRegistry};
use chaincodec_registry::{CsdlParser, MemoryRegistry};

// Multi-document CSDL: 3 schemas in one YAML string
const MULTI_SCHEMA_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum, polygon, arbitrum, base, optimism]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true,  description: "Sender address" }
    to:    { type: address, indexed: true,  description: "Recipient address" }
    value: { type: uint256, indexed: false, description: "Token amount (in smallest unit)" }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
---
schema ERC20Approval:
  version: 1
  chains: [ethereum, polygon, arbitrum, base, optimism]
  event: Approval
  fingerprint: "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
  fields:
    owner:   { type: address, indexed: true }
    spender: { type: address, indexed: true }
    value:   { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
---
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

// Schema versioning: v1 → v2 evolution
const VERSIONED_CSDL_V1: &str = r#"
schema AaveV3Supply:
  version: 1
  chains: [ethereum]
  event: Supply
  fingerprint: "0x2b627736bca15cd5381dcf80b0bf11fd197d26ee5ad7347de64a0d0fc3a0e9b"
  fields:
    reserve:    { type: address, indexed: true  }
    user:       { type: address, indexed: false }
    onBehalfOf: { type: address, indexed: true  }
    amount:     { type: uint256, indexed: false }
  meta:
    protocol: aave-v3
    category: lending
    verified: true
    trust_level: maintainer_verified
"#;

fn main() -> Result<()> {
    println!("ChainCodec — CSDL Parser + MemoryRegistry");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Parse multi-document CSDL ─────────────────────────────────────────
    let schemas = CsdlParser::parse_all(MULTI_SCHEMA_CSDL)?;
    println!("\n✓ Parsed {} schemas from multi-document YAML:", schemas.len());
    for s in &schemas {
        println!(
            "  • {} v{}  [{}]  fingerprint={}...",
            s.name,
            s.version,
            s.meta.protocol.as_deref().unwrap_or("unknown"),
            &s.fingerprint.as_hex()[..20]
        );
    }

    // ── 2. Load into MemoryRegistry ───────────────────────────────────────────
    let registry = MemoryRegistry::new();
    for schema in schemas {
        registry.add(schema)?;
    }
    println!("\n✓ Registry contains {} schemas", registry.len());

    // ── 3. Look up by fingerprint (how decoders route events) ─────────────────
    let transfer_fp = EventFingerprint::new(
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
    );
    let swap_fp = EventFingerprint::new(
        "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67".into(),
    );
    let unknown_fp = EventFingerprint::new("0xdeadbeefdeadbeef".into());

    println!("\n─── Fingerprint Lookup ──────────────────────────────");
    for (label, fp) in [
        ("ERC20Transfer  ", &transfer_fp),
        ("UniswapV3Swap  ", &swap_fp),
        ("UnknownEvent   ", &unknown_fp),
    ] {
        match registry.get_by_fingerprint(fp) {
            Some(s) => println!(
                "  {} → {} v{}  (protocol: {})",
                label, s.name, s.version, s.meta.protocol.as_deref().unwrap_or("unknown")
            ),
            None => println!("  {label} → NOT FOUND"),
        }
    }

    // ── 4. List all schemas ────────────────────────────────────────────────────
    println!("\n─── All Schemas in Registry ─────────────────────────");
    let all = registry.all_schemas();
    for s in &all {
        let chains: Vec<_> = s.chains.iter().map(|c| c.as_str()).collect();
        println!(
            "  {:20} v{}  chains=[{}]  fields={}",
            s.name,
            s.version,
            chains.join(","),
            s.fields.len()
        );
    }

    // ── 5. Schema versioning ──────────────────────────────────────────────────
    println!("\n─── Schema Version Evolution ────────────────────────");
    let additional = CsdlParser::parse_all(VERSIONED_CSDL_V1)?;
    for s in additional {
        println!("  Adding {} v{}", s.name, s.version);
        registry.add(s)?;
    }
    println!("  Registry now has {} schemas", registry.len());

    // ── 6. Field inspection ───────────────────────────────────────────────────
    println!("\n─── Field Inspection: ERC20Transfer ─────────────────");
    if let Some(s) = registry.get_by_fingerprint(&transfer_fp) {
        println!("  event:     {}", s.event);
        println!("  fields:");
        for (name, field) in &s.fields {
            println!(
                "    {:12} {:10} indexed={}",
                name,
                field.ty.to_string(),
                field.indexed
            );
        }
        let chain_slugs: Vec<_> = s.chains.iter().map(|c| c.as_str()).collect();
        println!("  chains:    [{}]", chain_slugs.join(", "));
        println!("  verified:  {}", s.meta.verified);
        println!("  trust:     {}", s.meta.trust_level);
    }

    // ── 7. Duplicate fingerprint → error ──────────────────────────────────────
    println!("\n─── Duplicate Detection ─────────────────────────────");
    let dup_schemas = CsdlParser::parse_all(MULTI_SCHEMA_CSDL)?;
    for schema in dup_schemas {
        match registry.add(schema) {
            Ok(_)  => {}
            Err(e) => println!("  ✓ duplicate fingerprint rejected: {e}"),
        }
    }

    println!("\n✓ CSDL registry examples complete");
    Ok(())
}
