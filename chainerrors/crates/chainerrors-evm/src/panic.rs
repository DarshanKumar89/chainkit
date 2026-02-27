//! Decode Solidity `Panic(uint256)` errors (introduced in Solidity 0.8.0).
//!
//! Selector: `0x4e487b71` == `keccak256("Panic(uint256)")[..4]`
//!
//! Full list of panic codes:
//! <https://docs.soliditylang.org/en/latest/control-structures.html#panic-via-assert-and-error-via-require>

use alloy_core::dyn_abi::{DynSolType, DynSolValue};

/// The 4-byte selector for `Panic(uint256)`.
pub const PANIC_SELECTOR: [u8; 4] = [0x4e, 0x48, 0x7b, 0x71];

/// Decode `Panic(uint256)` revert data.
///
/// Returns `Some((code, meaning))` on success, `None` if not a panic revert.
pub fn decode_panic(data: &[u8]) -> Option<(u64, &'static str)> {
    if data.len() < 4 {
        return None;
    }
    if &data[..4] != PANIC_SELECTOR {
        return None;
    }
    let payload = &data[4..];
    let ty = DynSolType::Uint(256);
    match ty.abi_decode(payload) {
        Ok(DynSolValue::Uint(v, _)) => {
            let code = v.to::<u64>();
            Some((code, panic_meaning(code)))
        }
        _ => None,
    }
}

/// Map a Solidity panic code to a human-readable description.
pub fn panic_meaning(code: u64) -> &'static str {
    match code {
        0x00 => "generic compiler-inserted panic",
        0x01 => "assert() called with false condition",
        0x11 => "arithmetic overflow or underflow",
        0x12 => "division or modulo by zero",
        0x21 => "invalid enum value",
        0x22 => "corrupted storage byte array",
        0x31 => ".pop() on empty array",
        0x32 => "out-of-bounds array access",
        0x41 => "too much memory allocated (out of memory)",
        0x51 => "called zero-initialized internal function pointer",
        _ => "unknown panic code",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Panic(0x11)` — arithmetic overflow
    const PANIC_OVERFLOW_HEX: &str =
        "4e487b710000000000000000000000000000000000000000000000000000000000000011";

    /// `Panic(0x12)` — division by zero
    const PANIC_DIV_ZERO_HEX: &str =
        "4e487b710000000000000000000000000000000000000000000000000000000000000012";

    #[test]
    fn decode_panic_overflow() {
        let data = hex::decode(PANIC_OVERFLOW_HEX).unwrap();
        let (code, meaning) = decode_panic(&data).unwrap();
        assert_eq!(code, 0x11);
        assert!(meaning.contains("overflow"));
    }

    #[test]
    fn decode_panic_div_zero() {
        let data = hex::decode(PANIC_DIV_ZERO_HEX).unwrap();
        let (code, meaning) = decode_panic(&data).unwrap();
        assert_eq!(code, 0x12);
        assert!(meaning.contains("division"));
    }

    #[test]
    fn decode_panic_wrong_selector() {
        let data = hex::decode("08c379a000").unwrap();
        assert!(decode_panic(&data).is_none());
    }

    #[test]
    fn panic_meaning_known_codes() {
        assert_eq!(panic_meaning(0x01), "assert() called with false condition");
        assert_eq!(panic_meaning(0x32), "out-of-bounds array access");
        assert_eq!(panic_meaning(0x99), "unknown panic code");
    }
}
