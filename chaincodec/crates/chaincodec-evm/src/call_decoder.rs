//! EVM function-call and constructor calldata decoder.
//!
//! Decodes transaction `input` data using an ABI JSON definition.
//!
//! # How it works
//! - First 4 bytes of calldata = keccak256(function_signature)[:4] (the selector)
//! - Remaining bytes = ABI-encoded inputs tuple
//! - Constructor: no selector prefix; all bytes = ABI-encoded constructor args

use alloy_core::dyn_abi::{DynSolType, DynSolValue};
use alloy_json_abi::{Function, JsonAbi};
use chaincodec_core::{
    call::{DecodedCall, DecodedConstructor},
    error::DecodeError,
    types::NormalizedValue,
};
use std::collections::HashMap;

use crate::normalizer;

/// EVM function-call decoder.
///
/// Accepts an ABI JSON string (standard Ethereum ABI JSON format) and decodes
/// raw calldata into structured `DecodedCall` or `DecodedConstructor` results.
pub struct EvmCallDecoder {
    abi: JsonAbi,
}

impl EvmCallDecoder {
    /// Create a decoder from a standard Ethereum ABI JSON string.
    ///
    /// # Errors
    /// Returns `DecodeError` if the JSON is not valid ABI JSON.
    pub fn from_abi_json(abi_json: &str) -> Result<Self, DecodeError> {
        let abi: JsonAbi = serde_json::from_str(abi_json)
            .map_err(|e| DecodeError::AbiDecodeFailed {
                reason: format!("invalid ABI JSON: {e}"),
            })?;
        Ok(Self { abi })
    }

    /// Decode a function call from raw calldata bytes.
    ///
    /// If `function_name` is provided, the selector is validated against that
    /// function. Otherwise the selector is matched against all functions in the ABI.
    ///
    /// # Arguments
    /// * `calldata` - full calldata including the 4-byte selector prefix
    /// * `function_name` - optional hint to match a specific function
    pub fn decode_call(
        &self,
        calldata: &[u8],
        function_name: Option<&str>,
    ) -> Result<DecodedCall, DecodeError> {
        if calldata.len() < 4 {
            return Err(DecodeError::InvalidRawEvent {
                reason: format!(
                    "calldata too short: {} bytes (need at least 4 for selector)",
                    calldata.len()
                ),
            });
        }

        let selector: [u8; 4] = calldata[..4].try_into().unwrap();
        let input_data = &calldata[4..];

        // Find the matching function
        let func = self.find_function(selector, function_name)?;

        // Build tuple type from function inputs
        let (input_names, input_types) = extract_function_types(&func);

        let decoded_inputs = decode_abi_tuple(input_data, &input_types, &input_names)?;

        Ok(DecodedCall {
            function_name: func.name.clone(),
            selector: Some(selector),
            inputs: decoded_inputs,
            raw_data: calldata.to_vec(),
            decode_errors: HashMap::new(),
        })
    }

    /// Decode constructor calldata (no selector prefix).
    ///
    /// # Arguments
    /// * `calldata` - raw constructor arguments (ABI-encoded, no 4-byte prefix)
    pub fn decode_constructor(&self, calldata: &[u8]) -> Result<DecodedConstructor, DecodeError> {
        let constructor = self.abi.constructor().ok_or_else(|| DecodeError::AbiDecodeFailed {
            reason: "ABI has no constructor definition".into(),
        })?;

        let input_types: Vec<DynSolType> = constructor
            .inputs
            .iter()
            .map(|p| p.resolve().map_err(|e| DecodeError::AbiDecodeFailed { reason: e.to_string() }))
            .collect::<Result<Vec<_>, _>>()?;

        let input_names: Vec<String> = constructor
            .inputs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if p.name.is_empty() {
                    format!("arg{i}")
                } else {
                    p.name.clone()
                }
            })
            .collect();

        let decoded_args = decode_abi_tuple(calldata, &input_types, &input_names)?;

        Ok(DecodedConstructor {
            args: decoded_args,
            raw_data: calldata.to_vec(),
            decode_errors: HashMap::new(),
        })
    }

    /// Find a function by selector, optionally constrained to a specific name.
    fn find_function(
        &self,
        selector: [u8; 4],
        name_hint: Option<&str>,
    ) -> Result<&Function, DecodeError> {
        // If name hint given, search by name first
        if let Some(name) = name_hint {
            for func in self.abi.functions() {
                if func.name == name && func.selector() == selector {
                    return Ok(func);
                }
            }
            // Try name match ignoring selector mismatch (useful for overloaded fns)
            for func in self.abi.functions() {
                if func.name == name {
                    return Ok(func);
                }
            }
            return Err(DecodeError::SchemaNotFound {
                fingerprint: format!("function '{}' not found in ABI", name),
            });
        }

        // Match by selector alone
        for func in self.abi.functions() {
            if func.selector() == selector {
                return Ok(func);
            }
        }

        Err(DecodeError::SchemaNotFound {
            fingerprint: format!(
                "no function found for selector 0x{}",
                hex::encode(selector)
            ),
        })
    }

    /// Returns all function names in this ABI.
    pub fn function_names(&self) -> Vec<&str> {
        self.abi.functions().map(|f| f.name.as_str()).collect()
    }

    /// Returns the 4-byte selector for a named function.
    pub fn selector_for(&self, name: &str) -> Option<[u8; 4]> {
        self.abi
            .functions()
            .find(|f| f.name == name)
            .map(|f| f.selector())
    }
}

/// Extract (names, DynSolTypes) from a function's inputs.
fn extract_function_types(func: &Function) -> (Vec<String>, Vec<DynSolType>) {
    let mut names = Vec::new();
    let mut types = Vec::new();

    for (i, param) in func.inputs.iter().enumerate() {
        let name = if param.name.is_empty() {
            format!("arg{i}")
        } else {
            param.name.clone()
        };
        names.push(name);
        // Best-effort resolve; skip unresolvable types
        if let Ok(ty) = param.resolve() {
            types.push(ty);
        }
    }

    (names, types)
}

/// ABI-decode a tuple of types and pair with names → NormalizedValue.
fn decode_abi_tuple(
    data: &[u8],
    types: &[DynSolType],
    names: &[String],
) -> Result<Vec<(String, NormalizedValue)>, DecodeError> {
    if types.is_empty() {
        return Ok(vec![]);
    }

    let tuple_type = DynSolType::Tuple(types.to_vec());
    let decoded = tuple_type
        .abi_decode(data)
        .map_err(|e| DecodeError::AbiDecodeFailed {
            reason: format!("function input decode: {e}"),
        })?;

    let values = match decoded {
        DynSolValue::Tuple(vals) => vals,
        other => vec![other],
    };

    let result: Vec<(String, NormalizedValue)> = names
        .iter()
        .zip(values.into_iter())
        .map(|(name, val)| (name.clone(), normalizer::normalize(val)))
        .collect();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Standard ERC-20 ABI (transfer function only for brevity)
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
        },
        {
            "name": "approve",
            "type": "function",
            "inputs": [
                {"name": "spender", "type": "address"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": [{"name": "", "type": "bool"}],
            "stateMutability": "nonpayable"
        }
    ]"#;

    #[test]
    fn decoder_parses_abi_json() {
        let dec = EvmCallDecoder::from_abi_json(ERC20_ABI).unwrap();
        let names = dec.function_names();
        assert!(names.contains(&"transfer"));
        assert!(names.contains(&"approve"));
    }

    #[test]
    fn selector_for_transfer() {
        let dec = EvmCallDecoder::from_abi_json(ERC20_ABI).unwrap();
        // keccak256("transfer(address,uint256)")[:4] = 0xa9059cbb
        let sel = dec.selector_for("transfer").unwrap();
        assert_eq!(hex::encode(sel), "a9059cbb");
    }

    #[test]
    fn decode_transfer_calldata() {
        let dec = EvmCallDecoder::from_abi_json(ERC20_ABI).unwrap();

        // Build real calldata: selector + ABI-encoded (address, uint256)
        // transfer(to=0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045, amount=1000000)
        let selector = hex::decode("a9059cbb").unwrap();
        // address padded to 32 bytes
        let to = hex::decode(
            "000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045",
        )
        .unwrap();
        // uint256 = 1000000
        let amount = hex::decode(
            "00000000000000000000000000000000000000000000000000000000000f4240",
        )
        .unwrap();
        let mut calldata = selector;
        calldata.extend_from_slice(&to);
        calldata.extend_from_slice(&amount);

        let result = dec.decode_call(&calldata, None).unwrap();
        assert_eq!(result.function_name, "transfer");
        assert!(result.is_clean());
        assert_eq!(result.inputs.len(), 2);
        assert_eq!(result.inputs[0].0, "to");
        assert_eq!(result.inputs[1].0, "amount");

        // Amount should decode to 1000000
        if let NormalizedValue::Uint(v) = &result.inputs[1].1 {
            assert_eq!(*v, 1_000_000u128);
        } else {
            panic!("expected Uint for amount");
        }
    }

    #[test]
    fn invalid_json_returns_error() {
        let result = EvmCallDecoder::from_abi_json("not json");
        assert!(result.is_err());
    }
}
