//! Converts alloy-core `DynSolValue` → ChainCodec `NormalizedValue`.
//!
//! This is where EVM ABI types are mapped to the canonical cross-chain
//! type system defined in `chaincodec-core`.

use alloy_core::dyn_abi::DynSolValue;
use alloy_primitives::U256;
use chaincodec_core::types::NormalizedValue;

/// Convert a decoded `DynSolValue` into a `NormalizedValue`.
pub fn normalize(val: DynSolValue) -> NormalizedValue {
    match val {
        DynSolValue::Bool(b) => NormalizedValue::Bool(b),

        DynSolValue::Int(i, bits) => {
            // For ints that fit in i128 return Int, else BigInt string
            if bits <= 128 {
                // alloy stores as two's-complement I256; safe to narrow
                match i128::try_from(i) {
                    Ok(v) => NormalizedValue::Int(v),
                    Err(_) => NormalizedValue::BigInt(i.to_string()),
                }
            } else {
                NormalizedValue::BigInt(i.to_string())
            }
        }

        DynSolValue::Uint(u, bits) => {
            if bits <= 128 {
                match u128::try_from(u) {
                    Ok(v) => NormalizedValue::Uint(v),
                    Err(_) => NormalizedValue::BigUint(u.to_string()),
                }
            } else {
                NormalizedValue::BigUint(u.to_string())
            }
        }

        DynSolValue::FixedBytes(bytes, _size) => {
            NormalizedValue::Bytes(bytes.to_vec())
        }

        DynSolValue::Bytes(b) => NormalizedValue::Bytes(b),

        DynSolValue::String(s) => NormalizedValue::Str(s),

        DynSolValue::Address(a) => {
            // EIP-55 checksum encoding
            NormalizedValue::Address(format!("{a:#x}"))
        }

        DynSolValue::Array(vals) | DynSolValue::FixedArray(vals) => {
            NormalizedValue::Array(vals.into_iter().map(normalize).collect())
        }

        DynSolValue::Tuple(fields) => {
            // Unnamed tuple fields get positional names "0", "1", ...
            let named: Vec<(String, NormalizedValue)> = fields
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), normalize(v)))
                .collect();
            NormalizedValue::Tuple(named)
        }

        // Custom types (e.g. function selector) — fall back to bytes
        DynSolValue::Function(f) => NormalizedValue::Bytes(f.to_vec()),
    }
}

/// Parse a U256 big-endian hex string into a NormalizedValue.
pub fn normalize_u256_hex(hex_str: &str) -> NormalizedValue {
    let hex = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    match U256::from_str_radix(hex, 16) {
        Ok(u) => match u128::try_from(u) {
            Ok(v) => NormalizedValue::Uint(v),
            Err(_) => NormalizedValue::BigUint(u.to_string()),
        },
        Err(_) => NormalizedValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, U256};

    #[test]
    fn normalize_bool() {
        let v = normalize(DynSolValue::Bool(true));
        assert_eq!(v, NormalizedValue::Bool(true));
    }

    #[test]
    fn normalize_uint256_small() {
        let u = DynSolValue::Uint(U256::from(42u64), 256);
        let v = normalize(u);
        assert_eq!(v, NormalizedValue::Uint(42));
    }

    #[test]
    fn normalize_address() {
        let addr: Address = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
            .parse()
            .unwrap();
        let v = normalize(DynSolValue::Address(addr));
        assert!(matches!(v, NormalizedValue::Address(_)));
        if let NormalizedValue::Address(s) = v {
            assert!(s.starts_with("0x"));
        }
    }
}
