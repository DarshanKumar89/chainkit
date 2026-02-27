//! `chaincodec test` — run golden test fixture files.
//!
//! Fixture format (JSON):
//! ```json
//! {
//!   "chain": "ethereum",
//!   "txHash": "0xabc...",
//!   "logIndex": 2,
//!   "expectedSchema": "UniswapV3Swap",
//!   "expectedFields": {
//!     "sender": "0x68b3465833...",
//!     "amount0": "-42000000000000000000"
//!   }
//! }
//! ```

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    chain: String,
    #[serde(rename = "txHash")]
    tx_hash: String,
    #[serde(rename = "logIndex")]
    log_index: u32,
    #[serde(rename = "expectedSchema")]
    expected_schema: String,
    #[serde(rename = "expectedFields")]
    expected_fields: HashMap<String, serde_json::Value>,
}

pub async fn run(fixtures_dir: &str, schema_filter: Option<&str>) -> Result<()> {
    let dir = Path::new(fixtures_dir);
    if !dir.exists() {
        println!("Fixtures directory '{}' not found — skipping", fixtures_dir);
        return Ok(());
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    collect_json_files(dir, &mut |path| {
        let content = std::fs::read_to_string(path)?;
        let fixture: Fixture = serde_json::from_str(&content)?;

        if let Some(filter) = schema_filter {
            if fixture.expected_schema != filter {
                skipped += 1;
                return Ok(());
            }
        }

        // TODO Phase 1 (Week 5): actually run decode and compare fields
        println!(
            "  [TODO] {} — {} ({})",
            fixture.expected_schema,
            fixture.tx_hash,
            fixture.chain
        );
        skipped += 1;
        Ok(())
    })?;

    println!();
    println!("Results: {} passed, {} failed, {} skipped", passed, failed, skipped);
    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn collect_json_files<F>(dir: &Path, f: &mut F) -> Result<()>
where
    F: FnMut(&Path) -> Result<()>,
{
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, f)?;
        } else if path.extension().map(|e| e == "json").unwrap_or(false) {
            f(&path)?;
        }
    }
    Ok(())
}
