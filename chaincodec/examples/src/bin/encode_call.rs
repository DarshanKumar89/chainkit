//! # encode_call
//!
//! Demonstrates ABI encoding with `EvmEncoder` and encode → decode roundtrip.
//!
//! ## Use-case coverage (chaincodec-usecase.md §5 — Wallet / Frontend)
//! - Build unsigned transactions programmatically
//! - Test contract interactions locally before signing
//! - Reproduce / replay transactions with different parameters
//!
//! Run with:
//! ```sh
//! cargo run --bin encode_call
//! ```

use anyhow::Result;
use chaincodec_core::types::NormalizedValue;
use chaincodec_evm::{EvmCallDecoder, EvmEncoder};

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
    }
]"#;

fn main() -> Result<()> {
    let encoder = EvmEncoder::from_abi_json(ERC20_ABI)?;
    let decoder = EvmCallDecoder::from_abi_json(ERC20_ABI)?;

    println!("ChainCodec — ABI Encoder + Decode Roundtrip");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Encode a transfer() call ───────────────────────────────────────────
    let recipient = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045";
    let amount: u128 = 1_000_000; // 1 USDC (6 decimals)

    let calldata = encoder.encode_call(
        "transfer",
        &[
            NormalizedValue::Address(recipient.to_string()),
            NormalizedValue::Uint(amount),
        ],
    )?;

    println!("\n─── Encoded transfer() ──────────────────────────────");
    println!("  to:        {recipient}");
    println!("  amount:    {amount} (1 USDC at 6 decimals)");
    println!("  calldata:  0x{}", hex::encode(&calldata));
    println!("  length:    {} bytes  (4 selector + 32 address + 32 uint)", calldata.len());
    println!("  selector:  0x{}", hex::encode(&calldata[..4]));

    // ── 2. Decode it back — proving the roundtrip ─────────────────────────────
    let decoded = decoder.decode_call(&calldata, None)?;

    println!("\n─── Decoded back ────────────────────────────────────");
    println!("  function:  {}", decoded.function_name);
    for (name, value) in &decoded.inputs {
        println!("    {:10} = {}", name, value);
    }

    // Assert correctness
    assert_eq!(decoded.function_name, "transfer");
    assert!(decoded.is_clean());
    if let NormalizedValue::Uint(v) = &decoded.inputs[1].1 {
        assert_eq!(*v, amount);
    }
    println!("  ✓ roundtrip verified: amount matches exactly");

    // ── 3. Encode an approve() with a large (but bounded) allowance ───────────
    let spender = "0x000000000022d473030f116ddee9f6b43ac78ba3"; // Permit2
    let allowance: u128 = 10_000_000_000u128; // 10,000 USDC

    let approve_calldata = encoder.encode_call(
        "approve",
        &[
            NormalizedValue::Address(spender.to_string()),
            NormalizedValue::Uint(allowance),
        ],
    )?;

    println!("\n─── Encoded approve() ───────────────────────────────");
    println!("  spender:   {spender}");
    println!("  allowance: {allowance} (10,000 USDC)");
    println!("  calldata:  0x{}", hex::encode(&approve_calldata));

    let decoded_approve = decoder.decode_call(&approve_calldata, None)?;
    println!("  decoded:   {} inputs ✓", decoded_approve.inputs.len());

    // ── 4. Demonstrate wrong arg count → error ────────────────────────────────
    println!("\n─── Error handling ──────────────────────────────────");
    let bad_result = encoder.encode_call("transfer", &[NormalizedValue::Uint(1)]);
    match bad_result {
        Err(e) => println!("  ✓ wrong arg count detected: {e}"),
        Ok(_)  => println!("  unexpected success"),
    }

    println!("\n✓ Encoding examples complete");
    Ok(())
}
