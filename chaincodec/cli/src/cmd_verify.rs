//! `chaincodec verify` — verify a schema against real on-chain data.
//!
//! Fetches the transaction receipt via RPC, finds the matching log,
//! decodes it with the named schema, and prints the result.

use anyhow::{bail, Result};

pub async fn run(schema: &str, chain: &str, tx: &str, rpc: Option<&str>) -> Result<()> {
    println!("Verifying schema '{}' on chain '{}' ...", schema, chain);
    println!("  Transaction: {}", tx);

    // TODO Phase 1 (Week 5): implement RPC fetch + decode
    // For now, show the planned flow
    println!();
    println!("  [Phase 1 implementation pending]");
    println!("  Flow:");
    println!("    1. Connect to RPC: {}", rpc.unwrap_or("<CHAINCODEC_RPC_ETHEREUM env>"));
    println!("    2. eth_getTransactionReceipt({})", tx);
    println!("    3. Find log matching schema fingerprint");
    println!("    4. Decode with EvmDecoder");
    println!("    5. Print decoded fields");

    Ok(())
}
