//! Golden fixture integration tests.
//!
//! Each test loads a real EVM log from `fixtures/evm/`, decodes it using
//! the corresponding CSDL schema, and asserts the field values match the
//! expected output recorded in the fixture JSON.

use chaincodec_core::{
    chain::{chains, ChainId},
    decoder::ChainDecoder,
    event::RawEvent,
    schema::SchemaRegistry,
    types::NormalizedValue,
};
use chaincodec_evm::decoder::EvmDecoder;
use chaincodec_registry::memory::MemoryRegistry;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Parse hex bytes from a `"0x..."` string.
fn hex_to_bytes(s: &str) -> Vec<u8> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).unwrap_or_else(|e| panic!("bad hex '{s}': {e}"))
}

/// Map a chain slug string to a `ChainId`.
fn chain_id_from_slug(slug: &str) -> ChainId {
    match slug {
        "ethereum" => chains::ethereum(),
        "arbitrum" => chains::arbitrum(),
        "base" => chains::base(),
        "polygon" => chains::polygon(),
        "optimism" => chains::optimism(),
        other => ChainId::custom(other, "evm"),
    }
}

/// Build a `RawEvent` from fixture JSON fields.
fn raw_event_from_fixture(f: &serde_json::Value) -> RawEvent {
    let chain_slug = f["chain"].as_str().unwrap();
    RawEvent {
        chain: chain_id_from_slug(chain_slug),
        tx_hash: f["txHash"].as_str().unwrap().to_string(),
        block_number: f["blockNumber"].as_u64().unwrap(),
        block_timestamp: f["blockTimestamp"].as_i64().unwrap(),
        log_index: f["logIndex"].as_u64().unwrap() as u32,
        topics: f["topics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect(),
        data: hex_to_bytes(f["data"].as_str().unwrap()),
        address: f["contractAddress"].as_str().unwrap().to_string(),
        raw_receipt: None,
    }
}

/// The fixtures live two levels above the crate root.
fn fixture_path(name: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../fixtures/evm");
    p.push(name);
    p
}

/// The schemas live two levels above the crate root.
fn schema_path(rel: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../schemas");
    p.push(rel);
    p
}

// ─── ERC-20 Transfer ──────────────────────────────────────────────────────────

#[test]
fn erc20_transfer_golden() {
    // Load fixture
    let fixture_json =
        std::fs::read_to_string(fixture_path("erc20-transfer.json")).expect("fixture not found");
    let fixture: serde_json::Value = serde_json::from_str(&fixture_json).unwrap();

    // Build registry with ERC-20 schema
    let registry = MemoryRegistry::new();
    registry
        .load_file(&schema_path("tokens/erc20.csdl"))
        .expect("failed to load erc20.csdl");

    // Build RawEvent from fixture
    let raw = raw_event_from_fixture(&fixture);

    // Decode
    let decoder = EvmDecoder::new();
    let fp = decoder.fingerprint(&raw);
    let schema = registry
        .get_by_fingerprint(&fp)
        .expect("schema not found in registry");

    let event = decoder
        .decode_event(&raw, &schema)
        .expect("decode_event failed");

    assert_eq!(event.schema, "ERC20Transfer");
    assert!(!event.has_errors(), "decode errors: {:?}", event.decode_errors);

    // Assert expected fields from fixture
    let expected = &fixture["expectedFields"];

    // `from` — EIP-55 checksummed address (case-insensitive compare)
    let from = event.field("from").expect("missing 'from'");
    if let NormalizedValue::Address(addr) = from {
        assert_eq!(
            addr.to_lowercase(),
            expected["from"].as_str().unwrap().to_lowercase(),
            "from mismatch"
        );
    } else {
        panic!("'from' is not an Address: {from:?}");
    }

    // `to`
    let to = event.field("to").expect("missing 'to'");
    if let NormalizedValue::Address(addr) = to {
        assert_eq!(
            addr.to_lowercase(),
            expected["to"].as_str().unwrap().to_lowercase(),
            "to mismatch"
        );
    } else {
        panic!("'to' is not an Address: {to:?}");
    }

    // `value` — USDC Transfer of 1_000_000_000 (1000 USDC, 6 decimals)
    let value = event.field("value").expect("missing 'value'");
    let expected_value: u128 = expected["value"].as_str().unwrap().parse().unwrap();
    match value {
        NormalizedValue::Uint(v) => assert_eq!(*v, expected_value, "value mismatch"),
        NormalizedValue::BigUint(s) => {
            let parsed: u128 = s.parse().unwrap();
            assert_eq!(parsed, expected_value, "value mismatch (BigUint)");
        }
        other => panic!("'value' is not a Uint: {other:?}"),
    }
}

// ─── UniswapV3 Swap ───────────────────────────────────────────────────────────

#[test]
fn uniswap_v3_swap_golden() {
    let fixture_json =
        std::fs::read_to_string(fixture_path("uniswap-v3-swap.json")).expect("fixture not found");
    let fixture: serde_json::Value = serde_json::from_str(&fixture_json).unwrap();

    let registry = MemoryRegistry::new();
    registry
        .load_file(&schema_path("defi/uniswap-v3.csdl"))
        .expect("failed to load uniswap-v3.csdl");

    let raw = raw_event_from_fixture(&fixture);

    let decoder = EvmDecoder::new();
    let fp = decoder.fingerprint(&raw);
    let schema = registry
        .get_by_fingerprint(&fp)
        .expect("UniswapV3Swap schema not found");

    let event = decoder
        .decode_event(&raw, &schema)
        .expect("decode_event failed");

    assert_eq!(event.schema, "UniswapV3Swap");
    assert_eq!(event.schema_version, 2);
    assert!(!event.has_errors(), "decode errors: {:?}", event.decode_errors);

    let expected = &fixture["expectedFields"];

    // `sender` — indexed address
    let sender = event.field("sender").expect("missing 'sender'");
    if let NormalizedValue::Address(addr) = sender {
        assert_eq!(
            addr.to_lowercase(),
            expected["sender"].as_str().unwrap().to_lowercase(),
            "sender mismatch"
        );
    } else {
        panic!("'sender' is not an Address: {sender:?}");
    }

    // `recipient` — indexed address
    let recipient = event.field("recipient").expect("missing 'recipient'");
    if let NormalizedValue::Address(addr) = recipient {
        assert_eq!(
            addr.to_lowercase(),
            expected["recipient"].as_str().unwrap().to_lowercase(),
            "recipient mismatch"
        );
    } else {
        panic!("'recipient' is not an Address: {recipient:?}");
    }

    // Non-indexed fields are present (amount0, amount1, sqrtPriceX96, liquidity, tick)
    assert!(event.field("amount0").is_some(), "missing amount0");
    assert!(event.field("amount1").is_some(), "missing amount1");
    assert!(event.field("sqrtPriceX96").is_some(), "missing sqrtPriceX96");
    assert!(event.field("liquidity").is_some(), "missing liquidity");
    assert!(event.field("tick").is_some(), "missing tick");
}

// ─── Normalizer unit tests (one per CanonicalType) ────────────────────────────

#[cfg(test)]
mod normalizer_coverage {
    use alloy_core::dyn_abi::DynSolValue;
    use alloy_primitives::{Address, FixedBytes, I256, U256};
    use chaincodec_core::types::NormalizedValue;
    use chaincodec_evm::normalizer::normalize;

    #[test]
    fn bool_true() {
        assert_eq!(normalize(DynSolValue::Bool(true)), NormalizedValue::Bool(true));
    }

    #[test]
    fn bool_false() {
        assert_eq!(normalize(DynSolValue::Bool(false)), NormalizedValue::Bool(false));
    }

    #[test]
    fn uint8_small() {
        let v = normalize(DynSolValue::Uint(U256::from(255u8), 8));
        assert_eq!(v, NormalizedValue::Uint(255));
    }

    #[test]
    fn uint128_boundary() {
        let v = normalize(DynSolValue::Uint(U256::from(u128::MAX), 128));
        assert_eq!(v, NormalizedValue::Uint(u128::MAX));
    }

    #[test]
    fn uint256_large() {
        // 2^128 + 1 — doesn't fit in u128, becomes BigUint
        let big = U256::from(1u128) << 128;
        let v = normalize(DynSolValue::Uint(big + U256::from(1u64), 256));
        assert!(matches!(v, NormalizedValue::BigUint(_)));
    }

    #[test]
    fn int_positive() {
        let v = normalize(DynSolValue::Int(I256::try_from(42i64).unwrap(), 256));
        assert_eq!(v, NormalizedValue::Int(42));
    }

    #[test]
    fn int_negative() {
        let v = normalize(DynSolValue::Int(I256::try_from(-1i64).unwrap(), 256));
        assert_eq!(v, NormalizedValue::Int(-1));
    }

    #[test]
    fn address_roundtrip() {
        let addr: Address = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
            .parse()
            .unwrap();
        let v = normalize(DynSolValue::Address(addr));
        assert!(matches!(v, NormalizedValue::Address(_)));
        if let NormalizedValue::Address(s) = v {
            assert!(s.starts_with("0x"));
        }
    }

    #[test]
    fn fixed_bytes32() {
        let fb: FixedBytes<32> = FixedBytes::from([0xab; 32]);
        let v = normalize(DynSolValue::FixedBytes(fb, 32));
        assert!(matches!(v, NormalizedValue::Bytes(ref b) if b.len() == 32));
    }

    #[test]
    fn bytes_dynamic() {
        let b = vec![1u8, 2, 3, 4];
        let v = normalize(DynSolValue::Bytes(b.clone()));
        assert_eq!(v, NormalizedValue::Bytes(b));
    }

    #[test]
    fn string_value() {
        let v = normalize(DynSolValue::String("hello".into()));
        assert_eq!(v, NormalizedValue::Str("hello".into()));
    }

    #[test]
    fn array_of_uints() {
        let vals = vec![
            DynSolValue::Uint(U256::from(1u64), 256),
            DynSolValue::Uint(U256::from(2u64), 256),
        ];
        let v = normalize(DynSolValue::Array(vals));
        assert!(matches!(v, NormalizedValue::Array(ref a) if a.len() == 2));
    }

    #[test]
    fn tuple_named_by_position() {
        let vals = vec![
            DynSolValue::Bool(true),
            DynSolValue::Uint(U256::from(99u64), 256),
        ];
        let v = normalize(DynSolValue::Tuple(vals));
        if let NormalizedValue::Tuple(fields) = v {
            assert_eq!(fields[0].0, "0");
            assert_eq!(fields[1].0, "1");
        } else {
            panic!("expected Tuple");
        }
    }
}
