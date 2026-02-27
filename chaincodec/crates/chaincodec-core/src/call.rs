//! Types for decoded function calls and constructor invocations.
//!
//! These are the output types when decoding transaction calldata
//! (as opposed to event logs, which produce `DecodedEvent`).

use crate::types::NormalizedValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of decoding a function call's calldata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedCall {
    /// Function name (e.g. "transfer", "swap")
    pub function_name: String,
    /// First 4 bytes of calldata (keccak256 of signature)
    pub selector: Option<[u8; 4]>,
    /// Decoded input parameters in declaration order
    pub inputs: Vec<(String, NormalizedValue)>,
    /// Raw calldata bytes (including selector)
    pub raw_data: Vec<u8>,
    /// Fields that failed to decode (field_name → error message)
    pub decode_errors: HashMap<String, String>,
}

impl DecodedCall {
    /// Selector as a hex string ("0xaabbccdd")
    pub fn selector_hex(&self) -> Option<String> {
        self.selector
            .map(|s| format!("0x{}", hex::encode(s)))
    }

    /// Look up a decoded input by name
    pub fn input(&self, name: &str) -> Option<&NormalizedValue> {
        self.inputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// Returns true if all inputs decoded without error
    pub fn is_clean(&self) -> bool {
        self.decode_errors.is_empty()
    }
}

/// Result of decoding constructor calldata (no function selector).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedConstructor {
    /// Decoded constructor arguments in declaration order
    pub args: Vec<(String, NormalizedValue)>,
    /// Raw constructor calldata
    pub raw_data: Vec<u8>,
    /// Decode errors
    pub decode_errors: HashMap<String, String>,
}

impl DecodedConstructor {
    /// Look up a decoded arg by name
    pub fn arg(&self, name: &str) -> Option<&NormalizedValue> {
        self.args
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }
}

/// A human-readable representation of a decoded call or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanReadable {
    /// e.g. "ERC20.transfer(to=0x..., amount=1000000)"
    pub summary: String,
    /// e.g. "Transfer 1000000 USDC to 0x..."
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_hex_format() {
        let call = DecodedCall {
            function_name: "transfer".into(),
            selector: Some([0xa9, 0x05, 0x9c, 0xbb]),
            inputs: vec![],
            raw_data: vec![],
            decode_errors: HashMap::new(),
        };
        assert_eq!(call.selector_hex(), Some("0xa9059cbb".to_string()));
    }

    #[test]
    fn input_lookup() {
        let call = DecodedCall {
            function_name: "transfer".into(),
            selector: None,
            inputs: vec![
                ("to".into(), NormalizedValue::Address("0xabc".into())),
                ("amount".into(), NormalizedValue::Uint(1000)),
            ],
            raw_data: vec![],
            decode_errors: HashMap::new(),
        };
        assert!(call.input("to").is_some());
        assert!(call.input("nonexistent").is_none());
    }
}
