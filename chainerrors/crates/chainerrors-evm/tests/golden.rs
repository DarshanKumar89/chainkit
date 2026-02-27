//! Golden fixture integration tests for chainerrors-evm.
//!
//! Each test loads a fixture JSON from `fixtures/evm/`, decodes the
//! `revertData` field using `EvmErrorDecoder`, and asserts the decoded
//! output matches the expected values in the fixture.

use chainerrors_core::{ErrorDecoder, ErrorKind};
use chainerrors_evm::EvmErrorDecoder;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn fixture_path(name: &str) -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../fixtures/evm");
    p.push(name);
    p
}

fn load_fixture(name: &str) -> serde_json::Value {
    let content =
        std::fs::read_to_string(fixture_path(name)).expect("fixture not found");
    serde_json::from_str(&content).expect("invalid fixture JSON")
}

fn decode_fixture(fixture: &serde_json::Value) -> chainerrors_core::types::DecodedError {
    let decoder = EvmErrorDecoder::new();
    let hex_str = fixture["revertData"].as_str().expect("missing revertData");
    decoder.decode_hex(hex_str, None).expect("decode_hex failed")
}

// ─── Revert string tests ───────────────────────────────────────────────────────

#[test]
fn golden_revert_string_insufficient_balance() {
    let f = load_fixture("revert-string-insufficient-balance.json");
    let decoded = decode_fixture(&f);

    assert_eq!(f["expectedKind"].as_str().unwrap(), "RevertString");
    match &decoded.kind {
        ErrorKind::RevertString { message } => {
            assert_eq!(
                message,
                f["expectedMessage"].as_str().unwrap(),
                "message mismatch"
            );
        }
        _ => panic!("expected RevertString, got {:?}", decoded.kind),
    }
    let expected_confidence: f32 = f["expectedConfidence"].as_f64().unwrap() as f32;
    assert_eq!(decoded.confidence, expected_confidence);
}

// ─── Panic tests ───────────────────────────────────────────────────────────────

#[test]
fn golden_panic_arithmetic_overflow() {
    let f = load_fixture("panic-arithmetic-overflow.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::Panic { code, meaning } => {
            let expected_code = f["expectedPanicCode"].as_u64().unwrap();
            assert_eq!(*code, expected_code, "panic code mismatch");
            let expected_meaning = f["expectedMeaning"].as_str().unwrap();
            assert!(
                meaning.contains("overflow"),
                "meaning '{meaning}' does not contain '{expected_meaning}'"
            );
        }
        _ => panic!("expected Panic, got {:?}", decoded.kind),
    }
    assert_eq!(decoded.confidence, 1.0);
}

#[test]
fn golden_panic_division_by_zero() {
    let f = load_fixture("panic-division-by-zero.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::Panic { code, meaning } => {
            assert_eq!(*code, 0x12u64);
            assert!(meaning.contains("division"), "meaning: {meaning}");
        }
        _ => panic!("expected Panic, got {:?}", decoded.kind),
    }
}

#[test]
fn golden_panic_array_oob() {
    let f = load_fixture("panic-array-oob.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::Panic { code, .. } => assert_eq!(*code, 0x32u64),
        _ => panic!("expected Panic, got {:?}", decoded.kind),
    }
}

#[test]
fn golden_panic_assert_false() {
    let f = load_fixture("panic-assert-false.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::Panic { code, meaning } => {
            assert_eq!(*code, 0x01u64);
            assert!(meaning.contains("assert"), "meaning: {meaning}");
        }
        _ => panic!("expected Panic, got {:?}", decoded.kind),
    }
}

// ─── Empty revert ──────────────────────────────────────────────────────────────

#[test]
fn golden_empty_revert() {
    let f = load_fixture("empty-revert.json");
    let decoded = decode_fixture(&f);

    assert!(
        matches!(decoded.kind, ErrorKind::Empty),
        "expected Empty, got {:?}", decoded.kind
    );
    assert_eq!(decoded.selector, None);
    assert_eq!(decoded.confidence, 0.5);
}

// ─── Custom errors ─────────────────────────────────────────────────────────────

#[test]
fn golden_custom_error_oz_ownable() {
    let f = load_fixture("custom-error-oz-ownable.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::CustomError { name, inputs } => {
            assert_eq!(name, "OwnableUnauthorizedAccount");
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0].0, "account");
            // Address should match (case-insensitive)
            let expected_addr = f["expectedFields"]["account"]
                .as_str()
                .unwrap()
                .to_lowercase();
            assert_eq!(
                inputs[0].1.to_string().to_lowercase(),
                expected_addr,
                "account address mismatch"
            );
        }
        _ => panic!("expected CustomError, got {:?}", decoded.kind),
    }
    assert!(decoded.suggestion.is_some(), "should have a suggestion");
    assert_eq!(decoded.confidence, 0.95);
}

#[test]
fn golden_custom_error_erc20_insufficient_balance() {
    let f = load_fixture("custom-error-erc20-insufficient-balance.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::CustomError { name, inputs } => {
            assert_eq!(name, "ERC20InsufficientBalance");
            assert_eq!(inputs.len(), 3);

            let field_names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
            assert!(field_names.contains(&"sender"), "missing sender");
            assert!(field_names.contains(&"balance"), "missing balance");
            assert!(field_names.contains(&"needed"), "missing needed");
        }
        _ => panic!("expected CustomError, got {:?}", decoded.kind),
    }
    assert!(decoded.suggestion.is_some());
}

// ─── Raw revert ────────────────────────────────────────────────────────────────

#[test]
fn golden_raw_revert_unknown() {
    let f = load_fixture("raw-revert-unknown.json");
    let decoded = decode_fixture(&f);

    match &decoded.kind {
        ErrorKind::RawRevert { selector, .. } => {
            let expected_sel = f["expectedSelector"].as_str().unwrap();
            assert_eq!(selector, expected_sel, "selector mismatch");
        }
        _ => panic!("expected RawRevert, got {:?}", decoded.kind),
    }
    assert_eq!(decoded.confidence, 0.0);
    assert!(decoded.suggestion.is_some(), "should have a 4byte.directory hint");
}

// ─── Selector correctness ──────────────────────────────────────────────────────

#[test]
fn golden_selectors_match_expected() {
    let cases = &[
        ("revert-string-insufficient-balance.json", "08c379a0"),
        ("panic-arithmetic-overflow.json", "4e487b71"),
        ("custom-error-oz-ownable.json", "118cdaa7"),
        ("empty-revert.json", /* no selector */  ""),
    ];

    let decoder = EvmErrorDecoder::new();
    for (fixture_name, expected_sel) in cases {
        let f = load_fixture(fixture_name);
        let hex_str = f["revertData"].as_str().unwrap();
        let decoded = decoder.decode_hex(hex_str, None).unwrap();

        if expected_sel.is_empty() {
            assert_eq!(decoded.selector, None, "fixture {fixture_name}: expected no selector");
        } else {
            let sel = decoded.selector.expect("expected a selector");
            let actual = hex::encode(sel);
            assert_eq!(
                &actual, expected_sel,
                "fixture {fixture_name}: selector mismatch"
            );
        }
    }
}
