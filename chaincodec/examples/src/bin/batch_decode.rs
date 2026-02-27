//! # batch_decode
//!
//! Demonstrates high-throughput batch decoding of multiple ERC-20 Transfer events
//! using `BatchEngine` with progress reporting and error collection.
//!
//! Run with:
//! ```sh
//! cargo run --bin batch_decode
//! ```

use anyhow::Result;
use chaincodec_batch::{BatchEngine, BatchRequest};
use chaincodec_core::{
    chain::chains,
    decoder::ErrorMode,
    event::RawEvent,
};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use std::sync::Arc;

fn main() -> Result<()> {
    // ── 1. Load schemas ───────────────────────────────────────────────────────
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

schema ERC20Approval:
  version: 1
  chains: [ethereum]
  event: Approval
  fingerprint: "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
  fields:
    owner:   { type: address, indexed: true }
    spender: { type: address, indexed: true }
    value:   { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
"#;

    let registry = Arc::new(MemoryRegistry::new());
    for schema in CsdlParser::parse_all(csdl)? {
        registry.add(schema)?;
    }
    println!("✓ Registry loaded ({} schemas)", registry.len());

    // ── 2. Build a batch of raw events ───────────────────────────────────────
    // Simulate 6 real EVM logs: 4 Transfers + 1 Approval + 1 unknown (no schema)
    let usdc = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
    let alice = "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045";
    let bob   = "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b";
    let carol = "0x000000000000000000000000c1912fee45d61c87cc5ea59dae31190ff64f1e39";

    let transfer_fp = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
    let approval_fp = "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925";
    let unknown_fp  = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

    // value = 1_000_000 (1 USDC at 6 decimals) = 0x00000000000000000000000000000000000000000000000000000000000F4240
    let value_1_usdc   = hex::decode("00000000000000000000000000000000000000000000000000000000000F4240")?;
    // value = 5_000_000 (5 USDC)
    let value_5_usdc   = hex::decode("00000000000000000000000000000000000000000000000000000000004C4B40")?;
    // allowance = max uint256
    let max_allowance  = hex::decode("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff")?;

    let logs: Vec<RawEvent> = vec![
        // Transfer: Alice → Bob, 1 USDC
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000001".into(),
            block_number: 19_000_001,
            block_timestamp: 1_700_000_100,
            log_index: 0,
            address: usdc.into(),
            topics: vec![transfer_fp.into(), alice.into(), bob.into()],
            data: value_1_usdc.clone(),
            raw_receipt: None,
        },
        // Transfer: Alice → Carol, 1 USDC
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000002".into(),
            block_number: 19_000_001,
            block_timestamp: 1_700_000_100,
            log_index: 1,
            address: usdc.into(),
            topics: vec![transfer_fp.into(), alice.into(), carol.into()],
            data: value_1_usdc.clone(),
            raw_receipt: None,
        },
        // Transfer: Bob → Carol, 5 USDC
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000003".into(),
            block_number: 19_000_002,
            block_timestamp: 1_700_000_200,
            log_index: 0,
            address: usdc.into(),
            topics: vec![transfer_fp.into(), bob.into(), carol.into()],
            data: value_5_usdc.clone(),
            raw_receipt: None,
        },
        // Transfer: Bob → Alice, 1 USDC (back-transfer)
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000004".into(),
            block_number: 19_000_002,
            block_timestamp: 1_700_000_200,
            log_index: 1,
            address: usdc.into(),
            topics: vec![transfer_fp.into(), bob.into(), alice.into()],
            data: value_1_usdc.clone(),
            raw_receipt: None,
        },
        // Approval: Alice approves Bob for max allowance
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000005".into(),
            block_number: 19_000_003,
            block_timestamp: 1_700_000_300,
            log_index: 0,
            address: usdc.into(),
            topics: vec![approval_fp.into(), alice.into(), bob.into()],
            data: max_allowance,
            raw_receipt: None,
        },
        // Unknown event — no matching schema (will be skipped/collected depending on mode)
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xaaaa000000000000000000000000000000000000000000000000000000000006".into(),
            block_number: 19_000_003,
            block_timestamp: 1_700_000_300,
            log_index: 1,
            address: usdc.into(),
            topics: vec![unknown_fp.into()],
            data: vec![],
            raw_receipt: None,
        },
    ];

    println!("✓ Prepared {} raw events (5 known + 1 unknown)", logs.len());

    // ── 3. Build and run the BatchEngine in Collect mode ─────────────────────
    let mut engine = BatchEngine::new(registry);
    engine.add_decoder("ethereum", Arc::new(EvmDecoder::new()));

    let request = BatchRequest::new("ethereum", logs)
        .chunk_size(100)
        .error_mode(ErrorMode::Collect)
        .on_progress(|done, total| {
            print!("\r  Progress: {done}/{total}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        });

    let result = engine.decode(request)?;
    println!(); // newline after progress

    // ── 4. Print results ──────────────────────────────────────────────────────
    println!(
        "\n─── Batch Result ────────────────────────────────────────"
    );
    println!("  total input:  {}", result.total_input);
    println!("  decoded:      {}", result.events.len());
    println!("  errors:       {}", result.errors.len());

    println!("\n─── Decoded Events ──────────────────────────────────────");
    for (i, event) in result.events.iter().enumerate() {
        let mut field_names: Vec<_> = event.fields.keys().collect();
        field_names.sort();
        let fields: Vec<String> = field_names
            .iter()
            .map(|k| format!("{k}={}", event.fields[*k]))
            .collect();
        println!(
            "  [{i}] {} tx={} | {}",
            event.schema,
            &event.tx_hash[..18],
            fields.join(", ")
        );
    }

    if !result.errors.is_empty() {
        println!("\n─── Skipped / Errors ────────────────────────────────────");
        for (idx, err) in &result.errors {
            println!("  [input #{idx}] {err}");
        }
    }

    println!("\n✓ Batch decode complete");
    Ok(())
}
