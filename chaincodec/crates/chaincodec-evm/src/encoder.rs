//! ABI encoder — the inverse of the ABI decoder.
//!
//! Converts `NormalizedValue` inputs into EVM ABI-encoded calldata.
//! Supports function calls (with 4-byte selector prefix) and raw tuple encoding.
//!
//! # Usage
//! ```ignore
//! let encoder = EvmEncoder::from_abi_json(ABI_JSON)?;
//! let calldata = encoder.encode_call("transfer", &[
//!     NormalizedValue::Address("0xd8dA...".into()),
//!     NormalizedValue::Uint(1_000_000),
//! ])?;
//! ```

use alloy_dyn_abi::Specifier;
use alloy_core::dyn_abi::{DynSolType, DynSolValue};
use alloy_json_abi::JsonAbi;
use alloy_primitives::{Address, FixedBytes, I256, U256};
use chaincodec_core::{error::DecodeError, types::NormalizedValue};
use std::str::FromStr;

/// ABI encoder for EVM function calls.
pub struct EvmEncoder {
    abi: JsonAbi,
}

impl EvmEncoder {
    /// Create an encoder from a standard Ethereum ABI JSON string.
    pub fn from_abi_json(abi_json: &str) -> Result<Self, DecodeError> {
        let abi: JsonAbi = serde_json::from_str(abi_json)
            .map_err(|e| DecodeError::AbiDecodeFailed {
                reason: format!("invalid ABI JSON: {e}"),
            })?;
        Ok(Self { abi })
    }

    /// Encode a function call to calldata bytes.
    ///
    /// Returns `selector ++ abi_encode_packed(args...)` — the standard
    /// EVM calldata format suitable for `eth_sendRawTransaction`.
    ///
    /// # Arguments
    /// * `function_name` - the Solidity function name
    /// * `args` - values in declaration order (must match ABI parameter count & types)
    pub fn encode_call(
        &self,
        function_name: &str,
        args: &[NormalizedValue],
    ) -> Result<Vec<u8>, DecodeError> {
        let func = self
            .abi
            .functions()
            .find(|f| f.name == function_name)
            .ok_or_else(|| DecodeError::SchemaNotFound {
                fingerprint: format!("function '{function_name}' not found in ABI"),
            })?;

        if args.len() != func.inputs.len() {
            return Err(DecodeError::AbiDecodeFailed {
                reason: format!(
                    "argument count mismatch: ABI has {}, got {}",
                    func.inputs.len(),
                    args.len()
                ),
            });
        }

        let mut dyn_values = Vec::with_capacity(args.len());
        for (i, (param, arg)) in func.inputs.iter().zip(args.iter()).enumerate() {
            let sol_type = param.resolve().map_err(|e| DecodeError::AbiDecodeFailed {
                reason: format!("param {i}: {e}"),
            })?;
            let dyn_val = normalized_to_dyn_value(arg, &sol_type).map_err(|e| {
                DecodeError::AbiDecodeFailed {
                    reason: format!("param '{}': {e}", param.name),
                }
            })?;
            dyn_values.push(dyn_val);
        }

        // Selector: 4-byte function selector
        let selector = func.selector();

        // ABI-encode the tuple of arguments
        let encoded = DynSolValue::Tuple(dyn_values).abi_encode();

        let mut calldata = selector.to_vec();
        calldata.extend_from_slice(&encoded);
        Ok(calldata)
    }

    /// Encode raw ABI tuple without a function selector.
    ///
    /// Useful for encoding constructor arguments.
    pub fn encode_tuple(
        &self,
        type_strings: &[&str],
        args: &[NormalizedValue],
    ) -> Result<Vec<u8>, DecodeError> {
        if type_strings.len() != args.len() {
            return Err(DecodeError::AbiDecodeFailed {
                reason: "type_strings and args length mismatch".into(),
            });
        }

        let mut dyn_values = Vec::new();
        for (ty_str, arg) in type_strings.iter().zip(args.iter()) {
            let sol_type: DynSolType = ty_str.parse().map_err(|e: alloy_core::dyn_abi::Error| {
                DecodeError::AbiDecodeFailed {
                    reason: format!("type parse '{ty_str}': {e}"),
                }
            })?;
            let dyn_val = normalized_to_dyn_value(arg, &sol_type)?;
            dyn_values.push(dyn_val);
        }

        Ok(DynSolValue::Tuple(dyn_values).abi_encode())
    }
}

/// Convert a `NormalizedValue` to the alloy `DynSolValue` for the given expected type.
pub fn normalized_to_dyn_value(
    val: &NormalizedValue,
    expected: &DynSolType,
) -> Result<DynSolValue, String> {
    match (val, expected) {
        (NormalizedValue::Bool(b), DynSolType::Bool) => Ok(DynSolValue::Bool(*b)),

        (NormalizedValue::Uint(u), DynSolType::Uint(bits)) => {
            Ok(DynSolValue::Uint(U256::from(*u), *bits))
        }
        (NormalizedValue::BigUint(s), DynSolType::Uint(bits)) => {
            let u = U256::from_str(s).map_err(|e| format!("BigUint parse: {e}"))?;
            Ok(DynSolValue::Uint(u, *bits))
        }

        (NormalizedValue::Int(i), DynSolType::Int(bits)) => {
            Ok(DynSolValue::Int(I256::try_from(*i).map_err(|e| e.to_string())?, *bits))
        }
        (NormalizedValue::BigInt(s), DynSolType::Int(bits)) => {
            let i = I256::from_str(s).map_err(|e| format!("BigInt parse: {e}"))?;
            Ok(DynSolValue::Int(i, *bits))
        }

        (NormalizedValue::Address(s), DynSolType::Address) => {
            let addr = Address::from_str(s).map_err(|e| format!("address parse: {e}"))?;
            Ok(DynSolValue::Address(addr))
        }

        (NormalizedValue::Bytes(b), DynSolType::Bytes) => Ok(DynSolValue::Bytes(b.clone())),

        (NormalizedValue::Bytes(b), DynSolType::FixedBytes(n)) => {
            if b.len() > *n {
                return Err(format!("bytes{n}: got {} bytes", b.len()));
            }
            let mut arr = [0u8; 32];
            arr[..*n.min(&b.len())].copy_from_slice(&b[..*n.min(&b.len())]);
            Ok(DynSolValue::FixedBytes(FixedBytes::from_slice(&arr[..*n]), *n))
        }

        (NormalizedValue::Str(s), DynSolType::String) => Ok(DynSolValue::String(s.clone())),

        (NormalizedValue::Array(elems), DynSolType::Array(inner)) => {
            let dyn_elems: Result<Vec<_>, _> =
                elems.iter().map(|e| normalized_to_dyn_value(e, inner)).collect();
            Ok(DynSolValue::Array(dyn_elems?))
        }

        (NormalizedValue::Array(elems), DynSolType::FixedArray(inner, len)) => {
            if elems.len() != *len {
                return Err(format!("fixed array length mismatch: expected {len}, got {}", elems.len()));
            }
            let dyn_elems: Result<Vec<_>, _> =
                elems.iter().map(|e| normalized_to_dyn_value(e, inner)).collect();
            Ok(DynSolValue::FixedArray(dyn_elems?))
        }

        (NormalizedValue::Tuple(fields), DynSolType::Tuple(types)) => {
            let dyn_elems: Result<Vec<_>, _> = fields
                .iter()
                .zip(types.iter())
                .map(|((_, v), t)| normalized_to_dyn_value(v, t))
                .collect();
            Ok(DynSolValue::Tuple(dyn_elems?))
        }

        // Uint can represent Timestamp/Decimal too
        (NormalizedValue::Uint(u), DynSolType::Uint(bits)) => {
            Ok(DynSolValue::Uint(U256::from(*u), *bits))
        }

        _ => Err(format!(
            "cannot convert {:?} to {:?}",
            std::mem::discriminant(val),
            expected
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERC20_ABI: &str = r#"[
        {
            "name": "transfer",
            "type": "function",
            "inputs": [
                {"name": "to", "type": "address"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": [{"name": "", "type": "bool"}],
            "stateMutability": "nonpayable"
        }
    ]"#;

    #[test]
    fn encode_transfer() {
        let encoder = EvmEncoder::from_abi_json(ERC20_ABI).unwrap();
        let calldata = encoder
            .encode_call(
                "transfer",
                &[
                    NormalizedValue::Address(
                        "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045".into(),
                    ),
                    NormalizedValue::Uint(1_000_000),
                ],
            )
            .unwrap();

        // First 4 bytes = selector for transfer(address,uint256) = 0xa9059cbb
        assert_eq!(&calldata[..4], hex::decode("a9059cbb").unwrap().as_slice());
        // Total length = 4 + 32 + 32 = 68 bytes
        assert_eq!(calldata.len(), 68);
    }

    #[test]
    fn wrong_arg_count_returns_error() {
        let encoder = EvmEncoder::from_abi_json(ERC20_ABI).unwrap();
        let result = encoder.encode_call("transfer", &[NormalizedValue::Uint(1)]);
        assert!(result.is_err());
    }

    #[test]
    fn roundtrip_encode_decode() {
        use crate::call_decoder::EvmCallDecoder;

        let encoder = EvmEncoder::from_abi_json(ERC20_ABI).unwrap();
        let decoder = EvmCallDecoder::from_abi_json(ERC20_ABI).unwrap();

        let original_to = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
        let original_amount: u128 = 999_888;

        let calldata = encoder
            .encode_call(
                "transfer",
                &[
                    NormalizedValue::Address(original_to.to_lowercase()),
                    NormalizedValue::Uint(original_amount),
                ],
            )
            .unwrap();

        let decoded = decoder.decode_call(&calldata, None).unwrap();
        assert_eq!(decoded.function_name, "transfer");

        if let NormalizedValue::Uint(amount) = &decoded.inputs[1].1 {
            assert_eq!(*amount, original_amount);
        } else {
            panic!("expected Uint for amount");
        }
    }
}
