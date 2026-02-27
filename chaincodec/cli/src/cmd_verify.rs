//! `chaincodec verify` — verify a schema against real on-chain data.
//!
//! Fetches the transaction receipt via JSON-RPC, locates the matching log
//! by fingerprint, decodes it with `EvmDecoder`, and prints the result.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

// ─── JSON-RPC types ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct TransactionReceipt {
    #[serde(rename = "blockNumber")]
    block_number: Option<String>,
    logs: Vec<ReceiptLog>,
}

#[derive(Deserialize, Clone)]
struct ReceiptLog {
    address: String,
    topics: Vec<String>,
    data: String,
    #[serde(rename = "logIndex")]
    log_index: String,
    #[serde(rename = "blockNumber")]
    block_number: Option<String>,
    removed: Option<bool>,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub async fn run(schema: &str, chain: &str, tx: &str, rpc: Option<&str>) -> Result<()> {
    use chaincodec_core::{decoder::ChainDecoder, event::RawEvent};
    use chaincodec_evm::EvmDecoder;
    use chaincodec_registry::MemoryRegistry;

    let rpc_url = resolve_rpc_url(chain, rpc)?;

    println!("Verifying schema '{}' on chain '{}' ...", schema, chain);
    println!("  Transaction: {}", tx);
    println!("  RPC:         {}", rpc_url);
    println!();

    // Load bundled schemas
    let registry = MemoryRegistry::new();
    let loaded = registry
        .load_directory(std::path::Path::new("./schemas"))
        .with_context(|| "loading schemas from './schemas' — run from the chaincodec directory")?;

    if loaded == 0 {
        anyhow::bail!("no schemas found in './schemas'");
    }

    let target = registry
        .get_by_name(schema, None)
        .ok_or_else(|| anyhow!("schema '{}' not found", schema))?;

    println!(
        "  Schema:      {} v{} (fingerprint: {})",
        target.name,
        target.version,
        target.fingerprint.as_hex()
    );

    // Fetch transaction receipt via JSON-RPC
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let receipt = fetch_receipt(&client, &rpc_url, tx).await?;

    // Find the log whose topics[0] matches the schema fingerprint
    let fp_hex = target.fingerprint.as_hex().to_lowercase();
    let log = receipt
        .logs
        .iter()
        .filter(|l| l.removed != Some(true))
        .find(|l| l.topics.first().map(|t| t.to_lowercase() == fp_hex).unwrap_or(false))
        .ok_or_else(|| {
            anyhow!(
                "no log with fingerprint {} found in tx {} ({} logs present)",
                target.fingerprint.as_hex(),
                tx,
                receipt.logs.len()
            )
        })?;

    println!(
        "  Log index:   {} (contract: {})",
        log.log_index, log.address
    );

    // Build RawEvent
    let chain_id = chain_id_from_slug(chain);
    let block_number = log
        .block_number
        .as_deref()
        .or(receipt.block_number.as_deref())
        .and_then(|s| u64::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok())
        .unwrap_or(0);
    let log_idx =
        u32::from_str_radix(log.log_index.strip_prefix("0x").unwrap_or(&log.log_index), 16)
            .unwrap_or(0);
    let data_bytes =
        hex::decode(log.data.strip_prefix("0x").unwrap_or(&log.data)).context("invalid log data hex")?;

    let raw = RawEvent {
        chain: chain_id,
        tx_hash: tx.to_string(),
        block_number,
        block_timestamp: 0,
        log_index: log_idx,
        address: log.address.clone(),
        topics: log.topics.clone(),
        data: data_bytes,
        raw_receipt: None,
    };

    let decoder = EvmDecoder::new();
    let decoded = decoder
        .decode_event(&raw, &target)
        .map_err(|e| anyhow!("decode failed: {e}"))?;

    // Print result
    println!();
    println!("✓ Decoded successfully");
    println!("  Schema:  {} v{}", decoded.schema, decoded.schema_version);
    println!("  Fields:");
    let mut fields: Vec<_> = decoded.fields.iter().collect();
    fields.sort_by_key(|(k, _)| k.as_str());
    for (name, val) in &fields {
        println!("    {:20} = {}", name, val);
    }
    if !decoded.decode_errors.is_empty() {
        println!("  Partial decode errors:");
        for (k, v) in &decoded.decode_errors {
            println!("    {}: {}", k, v);
        }
    }
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async fn fetch_receipt(
    client: &reqwest::Client,
    rpc_url: &str,
    tx_hash: &str,
) -> Result<TransactionReceipt> {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash]
    });
    let resp: JsonRpcResponse<TransactionReceipt> = client
        .post(rpc_url)
        .json(&req)
        .send()
        .await
        .context("RPC request failed")?
        .json()
        .await
        .context("failed to parse RPC JSON")?;
    if let Some(err) = resp.error {
        anyhow::bail!("RPC error: {}", err);
    }
    resp.result
        .ok_or_else(|| anyhow!("RPC returned null for tx {} (not found or pending)", tx_hash))
}

fn resolve_rpc_url(chain: &str, rpc_arg: Option<&str>) -> Result<String> {
    if let Some(url) = rpc_arg {
        return Ok(url.to_string());
    }
    let env_key = format!("CHAINCODEC_RPC_{}", chain.to_uppercase().replace('-', "_"));
    if let Ok(url) = std::env::var(&env_key) {
        return Ok(url);
    }
    anyhow::bail!(
        "no RPC URL for chain '{}'. Set {} or pass --rpc <url>",
        chain,
        env_key
    )
}

fn chain_id_from_slug(chain: &str) -> chaincodec_core::chain::ChainId {
    use chaincodec_core::chain::chains;
    match chain.to_lowercase().as_str() {
        "ethereum" | "eth" | "mainnet" => chains::ethereum(),
        "arbitrum" | "arbitrum-one" => chains::arbitrum(),
        "base" => chains::base(),
        "polygon" => chains::polygon(),
        "optimism" => chains::optimism(),
        "avalanche" | "avax" => chains::avalanche(),
        "bsc" | "bnb" => chains::bsc(),
        _ => chains::ethereum(),
    }
}
