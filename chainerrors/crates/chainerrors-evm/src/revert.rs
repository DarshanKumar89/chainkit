//! Decode `Error(string)` revert strings.
//!
//! EVM encodes `require(cond, "message")` as:
//! `0x08c379a0` ++ ABI-encode(string)
//!
//! This selector is `keccak256("Error(string)")[..4]`.

use alloy_core::dyn_abi::{DynSolValue, DynSolType};

/// The 4-byte selector for `Error(string)`.
pub const ERROR_STRING_SELECTOR: [u8; 4] = [0x08, 0xc3, 0x79, 0xa0];

/// Try to decode the revert data as an `Error(string)` payload.
///
/// Returns `Some(message)` on success, `None` if the data doesn't match
/// the expected format.
pub fn decode_error_string(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    if &data[..4] != ERROR_STRING_SELECTOR {
        return None;
    }
    let payload = &data[4..];
    // ABI-decode as a single `string` type
    let ty = DynSolType::String;
    match ty.abi_decode(payload) {
        Ok(DynSolValue::String(s)) => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hex from `require(false, "Not enough tokens to transfer")` on mainnet
    const REVERT_HEX: &str = "08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001e4e6f7420656e6f75676820746f6b656e7320746f207472616e73666572000000";

    #[test]
    fn decode_error_string_basic() {
        let data = hex::decode(REVERT_HEX).unwrap();
        let msg = decode_error_string(&data).unwrap();
        assert_eq!(msg, "Not enough tokens to transfer");
    }

    #[test]
    fn decode_error_string_wrong_selector() {
        let data = hex::decode("4e487b710000000000000000000000000000000000000000000000000000000000000011").unwrap();
        assert!(decode_error_string(&data).is_none());
    }

    #[test]
    fn decode_error_string_too_short() {
        assert!(decode_error_string(&[0x08, 0xc3]).is_none());
    }

    #[test]
    fn decode_error_string_empty_message() {
        // Error("") — ABI-encoded empty string
        let data = hex::decode(
            "08c379a0\
             0000000000000000000000000000000000000000000000000000000000000020\
             0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let msg = decode_error_string(&data).unwrap();
        assert_eq!(msg, "");
    }
}
