//! `chaincodec test` — run golden test fixture files.
//!
//! Fixture format (JSON):
//! ```json
//! {
//!   "description": "USDC Transfer on Ethereum mainnet",
//!   "chain": "ethereum",
//!   "txHash": "0xabc...",
//!   "logIndex": 4,
//!   "blockNumber": 19000000,
//!   "blockTimestamp": 1700000000,
//!   "contractAddress": "0xa0b86991...",
//!   "topics": ["0xddf252ad...", "0x000...from", "0x000...to"],
//!   "data": "0x000...value",
//!   "expectedSchema": "ERC20Transfer",
//!   "expectedSchemaVersion": 1,
//!   "expectedFields": {
//!     "from": "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
//!     "value": "1000000000"
//!   }
//! }
//! ```

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    description: Option<String>,
    chain: String,
    #[serde(rename = "txHash")]
    tx_hash: String,
    #[serde(rename = "logIndex")]
    log_index: u32,
    #[serde(rename = "blockNumber", default)]
    block_number: u64,
    #[serde(rename = "blockTimestamp", default)]
    block_timestamp: i64,
    #[serde(rename = "contractAddress", default)]
    contract_address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "expectedSchema")]
    expected_schema: String,
    #[serde(rename = "expectedSchemaVersion", default)]
    expected_schema_version: u32,
    #[serde(rename = "expectedFields")]
    expected_fields: HashMap<String, serde_json::Value>,
}

pub async fn run(
    fixtures_dir: &str,
    schema_dir: &str,
    schema_filter: Option<&str>,
    verbose: bool,
) -> Result<()> {
    use chaincodec_core::{chain::chains, decoder::ChainDecoder, event::RawEvent};
    use chaincodec_evm::EvmDecoder;
    use chaincodec_registry::MemoryRegistry;

    let dir = Path::new(fixtures_dir);
    if !dir.exists() {
        println!("Fixtures directory '{}' not found — skipping", fixtures_dir);
        return Ok(());
    }

    // Load schema registry
    let registry = MemoryRegistry::new();
    let n = registry
        .load_directory(Path::new(schema_dir))
        .unwrap_or(0);
    if n == 0 {
        eprintln!(
            "Warning: no schemas loaded from '{}'. Run from the chaincodec directory.",
            schema_dir
        );
    }

    let decoder = EvmDecoder::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    // Collect all fixture files first
    let mut fixture_paths = Vec::new();
    collect_json_files(dir, &mut fixture_paths)?;
    fixture_paths.sort();

    for path in &fixture_paths {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  ✗ {}: read error: {}", path.display(), e);
                failed += 1;
                continue;
            }
        };

        let fixture: Fixture = match serde_json::from_str(&content) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("  ✗ {}: parse error: {}", path.display(), e);
                failed += 1;
                continue;
            }
        };

        if let Some(filter) = schema_filter {
            if fixture.expected_schema != filter {
                skipped += 1;
                continue;
            }
        }

        let desc = fixture
            .description
            .as_deref()
            .unwrap_or(&fixture.expected_schema);

        let chain_id = match fixture.chain.to_lowercase().as_str() {
            "ethereum" | "eth" => chains::ethereum(),
            "arbitrum" | "arbitrum-one" => chains::arbitrum(),
            "base" => chains::base(),
            "polygon" => chains::polygon(),
            "optimism" => chains::optimism(),
            "avalanche" => chains::avalanche(),
            "bsc" | "bnb" => chains::bsc(),
            _ => chains::ethereum(),
        };

        let data_bytes = match hex::decode(
            fixture.data.strip_prefix("0x").unwrap_or(&fixture.data),
        ) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  ✗ {}: invalid data hex: {}", desc, e);
                failed += 1;
                continue;
            }
        };

        let raw = RawEvent {
            chain: chain_id,
            tx_hash: fixture.tx_hash.clone(),
            block_number: fixture.block_number,
            block_timestamp: fixture.block_timestamp,
            log_index: fixture.log_index,
            address: fixture.contract_address.clone(),
            topics: fixture.topics.clone(),
            data: data_bytes,
            raw_receipt: None,
        };

        // Look up schema by fingerprint
        let fp = decoder.fingerprint(&raw);
        let schema = match registry.get_by_fingerprint(&fp) {
            Some(s) => s,
            None => {
                eprintln!(
                    "  ✗ {}: no schema for fingerprint {} ({})",
                    desc,
                    fp.as_hex(),
                    fixture.expected_schema
                );
                failed += 1;
                continue;
            }
        };

        // Decode
        let decoded = match decoder.decode_event(&raw, &schema) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("  ✗ {}: decode error: {}", desc, e);
                failed += 1;
                continue;
            }
        };

        // Check schema name
        let mut errors: Vec<String> = Vec::new();
        if decoded.schema != fixture.expected_schema {
            errors.push(format!(
                "schema mismatch: got '{}', want '{}'",
                decoded.schema, fixture.expected_schema
            ));
        }
        if fixture.expected_schema_version > 0
            && decoded.schema_version != fixture.expected_schema_version
        {
            errors.push(format!(
                "version mismatch: got {}, want {}",
                decoded.schema_version, fixture.expected_schema_version
            ));
        }

        // Check fields
        for (field, expected_json) in &fixture.expected_fields {
            match decoded.fields.get(field) {
                None => {
                    errors.push(format!("missing field '{}'", field));
                }
                Some(actual_val) => {
                    let actual_str = actual_val.to_string();
                    let expected_str = json_value_to_string(expected_json);
                    if actual_str.to_lowercase() != expected_str.to_lowercase() {
                        errors.push(format!(
                            "field '{}': got '{}', want '{}'",
                            field, actual_str, expected_str
                        ));
                    }
                }
            }
        }

        if errors.is_empty() {
            if verbose {
                println!("  ✓ {}", desc);
                for (k, v) in &decoded.fields {
                    println!("      {}: {}", k, v);
                }
            } else {
                println!("  ✓ {}", desc);
            }
            passed += 1;
        } else {
            eprintln!("  ✗ {}", desc);
            for e in &errors {
                eprintln!("      {}", e);
            }
            failed += 1;
        }
    }

    println!();
    println!(
        "Results: {} passed, {} failed, {} skipped  (total fixtures: {})",
        passed,
        failed,
        skipped,
        fixture_paths.len()
    );

    if failed > 0 {
        anyhow::bail!("{} fixture(s) failed", failed);
    }
    Ok(())
}

fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn collect_json_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out)?;
        } else if path.extension().map(|e| e == "json").unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(())
}
