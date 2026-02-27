//! # decode_call
//!
//! Demonstrates decoding EVM function-call calldata using `EvmCallDecoder`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §4 — Security)
//! - Understand what a suspicious transaction attempted
//! - Detect max-approval phishing patterns
//! - Decode calldata from archive nodes for MEV / post-mortem analysis
//!
//! Run with:
//! ```sh
//! cargo run --bin decode_call
//! ```

use anyhow::Result;
use chaincodec_core::types::NormalizedValue;
use chaincodec_evm::EvmCallDecoder;

// Standard ERC-20 ABI (transfer, approve, transferFrom)
const ERC20_ABI: &str = r#"[
    {
        "name": "transfer",
        "type": "function",
        "inputs": [
            {"name": "to",     "type": "address"},
            {"name": "amount", "type": "uint256"}
        ],
        "outputs": [{"name": "", "type": "bool"}],
        "stateMutability": "nonpayable"
    },
    {
        "name": "approve",
        "type": "function",
        "inputs": [
            {"name": "spender", "type": "address"},
            {"name": "amount",  "type": "uint256"}
        ],
        "outputs": [{"name": "", "type": "bool"}],
        "stateMutability": "nonpayable"
    },
    {
        "name": "transferFrom",
        "type": "function",
        "inputs": [
            {"name": "from",   "type": "address"},
            {"name": "to",     "type": "address"},
            {"name": "amount", "type": "uint256"}
        ],
        "outputs": [{"name": "", "type": "bool"}],
        "stateMutability": "nonpayable"
    }
]"#;

fn main() -> Result<()> {
    let decoder = EvmCallDecoder::from_abi_json(ERC20_ABI)?;

    println!("ChainCodec — EVM Call Decoder");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Print all function selectors ──────────────────────────────────────
    println!("\nFunction selectors in ERC-20 ABI:");
    for name in decoder.function_names() {
        if let Some(sel) = decoder.selector_for(name) {
            println!("  0x{} → {}()", hex::encode(sel), name);
        }
    }

    // ── 2. Decode a transfer() call ───────────────────────────────────────────
    //
    // Calldata breakdown:
    //   [0..4]   0xa9059cbb = keccak256("transfer(address,uint256)")[:4]
    //   [4..36]  `to` address, zero-padded to 32 bytes
    //   [36..68] `amount` = 1,000,000 = 0x0F4240, zero-padded to 32 bytes
    let transfer_calldata = hex::decode(concat!(
        "a9059cbb",                                                          // selector
        "000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045", // to
        "00000000000000000000000000000000000000000000000000000000000f4240", // amount
    ))?;

    let decoded = decoder.decode_call(&transfer_calldata, None)?;

    println!("\n─── transfer() Calldata ─────────────────────────────");
    println!("  function:  {}", decoded.function_name);
    println!(
        "  selector:  0x{}",
        decoded.selector.map(hex::encode).unwrap_or_default()
    );
    println!("  inputs:");
    for (name, value) in &decoded.inputs {
        println!("    {:10} = {}", name, value);
    }
    println!("  clean decode: {}", decoded.is_clean());

    // ── 3. Decode an approve() — detect max-approval phishing ────────────────
    //
    // 0xffffffff...ff as the amount means type(uint256).max — a common
    // phishing pattern that gives unlimited access to the spender.
    let approve_calldata = hex::decode(concat!(
        "095ea7b3",                                                          // selector
        "000000000000000000000000000000000022d473030f116ddee9f6b43ac78ba3", // spender (Permit2)
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", // amount = uint256.max
    ))?;

    let decoded_approve = decoder.decode_call(&approve_calldata, Some("approve"))?;

    println!("\n─── approve() Calldata ──────────────────────────────");
    println!("  function:  {}", decoded_approve.function_name);
    for (name, value) in &decoded_approve.inputs {
        println!("    {:10} = {}", name, value);
    }

    // Detect max-approval: the amount is BigUint (doesn't fit u128) for 2^256-1
    let is_max_approval =
        decoded_approve.inputs.iter().any(|(name, val)| {
            name == "amount" && matches!(val, NormalizedValue::BigUint(_))
        });
    if is_max_approval {
        println!("  ⚠  WARNING: MAX APPROVAL — this grants unlimited token access to spender");
    }

    // ── 4. Decode a transferFrom() call ──────────────────────────────────────
    let transfer_from_calldata = hex::decode(concat!(
        "23b872dd",                                                          // selector
        "000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045", // from
        "000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b", // to
        "0000000000000000000000000000000000000000000000000000000005f5e100", // amount = 100_000_000
    ))?;

    let decoded_from = decoder.decode_call(&transfer_from_calldata, None)?;

    println!("\n─── transferFrom() Calldata ─────────────────────────");
    println!("  function:  {}", decoded_from.function_name);
    for (name, value) in &decoded_from.inputs {
        println!("    {:10} = {}", name, value);
    }

    println!("\n✓ All calls decoded successfully");
    Ok(())
}
