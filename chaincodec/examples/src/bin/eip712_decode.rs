//! # eip712_decode
//!
//! Demonstrates parsing EIP-712 typed structured data with `Eip712Parser`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §4 — Security / §5 — Wallet)
//! - Detect phishing via Permit2 / `eth_signTypedData_v4` abuse
//! - Display human-readable "sign request" previews in wallets
//! - Audit off-chain signed messages (governance, meta-transactions)
//!
//! Run with:
//! ```sh
//! cargo run --bin eip712_decode
//! ```

use chaincodec_evm::eip712::{Eip712Parser, TypedValue};

// ─── Example 1: EIP-712 spec "Mail" message ──────────────────────────────────
const MAIL_EIP712: &str = r#"{
    "types": {
        "EIP712Domain": [
            {"name": "name",              "type": "string"},
            {"name": "version",           "type": "string"},
            {"name": "chainId",           "type": "uint256"},
            {"name": "verifyingContract", "type": "address"}
        ],
        "Person": [
            {"name": "name",   "type": "string"},
            {"name": "wallet", "type": "address"}
        ],
        "Mail": [
            {"name": "from",     "type": "Person"},
            {"name": "to",       "type": "Person"},
            {"name": "contents", "type": "string"}
        ]
    },
    "primaryType": "Mail",
    "domain": {
        "name": "Ether Mail",
        "version": "1",
        "chainId": 1,
        "verifyingContract": "0xCcCCccccCCCCcCCCCCCcCcCccCcCCCcCcccccccC"
    },
    "message": {
        "from": {"name": "Cow", "wallet": "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"},
        "to":   {"name": "Bob", "wallet": "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB"},
        "contents": "Hello, Bob!"
    }
}"#;

// ─── Example 2: Permit2 SignatureTransfer (phishing detection) ────────────────
// Permit2 lets anyone request an off-chain signature granting them token access.
// This is increasingly abused in phishing attacks.
const PERMIT2_EIP712: &str = r#"{
    "types": {
        "EIP712Domain": [
            {"name": "name",    "type": "string"},
            {"name": "chainId", "type": "uint256"},
            {"name": "verifyingContract", "type": "address"}
        ],
        "TokenPermissions": [
            {"name": "token",  "type": "address"},
            {"name": "amount", "type": "uint256"}
        ],
        "PermitTransferFrom": [
            {"name": "permitted", "type": "TokenPermissions"},
            {"name": "spender",   "type": "address"},
            {"name": "nonce",     "type": "uint256"},
            {"name": "deadline",  "type": "uint256"}
        ]
    },
    "primaryType": "PermitTransferFrom",
    "domain": {
        "name": "Permit2",
        "chainId": 1,
        "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
    },
    "message": {
        "permitted": {
            "token":  "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
            "amount": "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        },
        "spender":  "0xDeaDBeef00000000000000000000000000000001",
        "nonce":    "12345",
        "deadline": "9999999999"
    }
}"#;

fn main() {
    println!("ChainCodec — EIP-712 Typed Data Parser");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Parse the EIP-712 Mail example ────────────────────────────────────
    let mail = Eip712Parser::parse(MAIL_EIP712).expect("parse mail");

    println!("\n─── EIP-712 Mail Message ────────────────────────────");
    println!("  primaryType: {}", mail.primary_type);
    println!("  types defined: {}", mail.types.len());
    for (name, fields) in &mail.types {
        let field_names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
        println!("    {} → [{}]", name, field_names.join(", "));
    }

    println!("\n  Domain:");
    if let Some(TypedValue::Str(chain_name)) = mail.domain.get("name") {
        println!("    name:    {chain_name}");
    }
    if let Some(TypedValue::Number(chain_id)) = mail.domain.get("chainId") {
        println!("    chainId: {chain_id}");
    }
    if let Some(TypedValue::Str(contract)) = mail.domain.get("verifyingContract") {
        println!("    contract: {contract}");
    }

    println!("\n  Message (primaryType = Mail):");
    if let Some(TypedValue::Object(from)) = mail.message.get("from") {
        if let Some(TypedValue::Str(name)) = from.get("name") {
            println!("    from.name:   {name}");
        }
        if let Some(TypedValue::Str(wallet)) = from.get("wallet") {
            println!("    from.wallet: {wallet}");
        }
    }
    if let Some(TypedValue::Str(contents)) = mail.message.get("contents") {
        println!("    contents:    {contents}");
    }

    println!("\n  Domain separator (keccak256 of domain JSON):");
    println!("    {}", Eip712Parser::domain_separator_hex(&mail));

    // ── 2. Inspect primaryType fields ─────────────────────────────────────────
    if let Some(fields) = Eip712Parser::primary_type_fields(&mail) {
        println!("\n  Mail struct fields:");
        for f in fields {
            println!("    {}: {}", f.name, f.ty);
        }
    }

    // ── 3. Parse Permit2 — detect phishing patterns ───────────────────────────
    let permit = Eip712Parser::parse(PERMIT2_EIP712).expect("parse permit2");

    println!("\n─── Permit2 PermitTransferFrom ──────────────────────");
    println!("  primaryType: {}", permit.primary_type);

    // Check spender
    if let Some(TypedValue::Str(spender)) = permit.message.get("spender") {
        println!("  spender: {spender}");
    }

    // Check deadline
    if let Some(TypedValue::Str(deadline)) = permit.message.get("deadline") {
        let deadline_n: u64 = deadline.parse().unwrap_or(0);
        if deadline_n > 9_999_999_000 {
            println!("  ⚠  SUSPICIOUS: very far deadline ({deadline}) — could be permanent");
        }
    }

    // Check token + amount (phishing: max allowance on USDC)
    if let Some(TypedValue::Object(permitted)) = permit.message.get("permitted") {
        if let Some(TypedValue::Str(token)) = permitted.get("token") {
            println!("  token: {token}");
        }
        if let Some(TypedValue::Str(amount)) = permitted.get("amount") {
            // 2^256 - 1 starts with 115792...
            if amount.starts_with("11579208923731619542") {
                println!("  ⚠  MAX ALLOWANCE DETECTED: signing this grants full token access");
                println!("     This is a common Permit2 phishing pattern — DO NOT SIGN");
            }
        }
    }

    // ── 4. Error case — malformed JSON ───────────────────────────────────────
    println!("\n─── Error handling ──────────────────────────────────");
    let bad = Eip712Parser::parse(r#"{"types": {}}"#);
    match bad {
        Err(e) => println!("  ✓ missing primaryType detected: {e}"),
        Ok(_)  => println!("  unexpected success"),
    }

    println!("\n✓ EIP-712 parsing examples complete");
}
