//! Decode Solidity 0.8.4+ custom errors.
//!
//! Custom errors are ABI-encoded as:
//! `selector(4 bytes)` ++ `ABI-encoded arguments`
//!
//! Where `selector = keccak256("ErrorName(type1,type2,...)")[:4]`

use alloy_core::dyn_abi::{DynSolType, DynSolValue};
use chainerrors_core::registry::{ErrorSignature, ErrorSignatureRegistry};
use chainerrors_core::types::{ErrorFieldValue, ErrorKind};

/// Try to decode custom error data against a known registry.
///
/// Returns `Some(ErrorKind::CustomError { .. })` if the selector matches a
/// registry entry and the ABI decode succeeds. Returns `None` otherwise.
pub fn decode_custom_error(
    data: &[u8],
    registry: &dyn ErrorSignatureRegistry,
) -> Option<ErrorKind> {
    if data.len() < 4 {
        return None;
    }
    let selector: [u8; 4] = data[..4].try_into().ok()?;
    let sigs = registry.get_by_selector(selector);
    if sigs.is_empty() {
        return None;
    }
    let payload = &data[4..];
    // Try each matching signature (handle rare selector collisions)
    for sig in &sigs {
        if let Some(kind) = try_decode_with_signature(sig, payload) {
            return Some(kind);
        }
    }
    None
}

fn try_decode_with_signature(sig: &ErrorSignature, payload: &[u8]) -> Option<ErrorKind> {
    if sig.inputs.is_empty() {
        // No arguments — just return the name
        return Some(ErrorKind::CustomError {
            name: sig.name.clone(),
            inputs: vec![],
        });
    }

    // Build alloy DynSolType for each input
    let types: Vec<DynSolType> = sig
        .inputs
        .iter()
        .map(|p| p.ty.parse::<DynSolType>().ok())
        .collect::<Option<Vec<_>>>()?;

    // Decode as a tuple
    let tuple_type = DynSolType::Tuple(types);
    let decoded = tuple_type.abi_decode(payload).ok()?;

    let values = match decoded {
        DynSolValue::Tuple(vals) => vals,
        single => vec![single],
    };

    let inputs: Vec<(String, ErrorFieldValue)> = sig
        .inputs
        .iter()
        .zip(values.iter())
        .map(|(param, val)| (param.name.clone(), dyn_sol_to_field_value(val)))
        .collect();

    Some(ErrorKind::CustomError {
        name: sig.name.clone(),
        inputs,
    })
}

fn dyn_sol_to_field_value(val: &DynSolValue) -> ErrorFieldValue {
    use alloy_primitives::{I256, U256};
    match val {
        DynSolValue::Uint(v, _) => {
            if let Ok(small) = v.try_into::<u128>() {
                ErrorFieldValue::Uint(small)
            } else {
                ErrorFieldValue::BigUint(v.to_string())
            }
        }
        DynSolValue::Int(v, _) => {
            // I256 → i128 if fits
            let as_i128: Option<i128> = i128::try_from(*v).ok();
            if let Some(i) = as_i128 {
                ErrorFieldValue::Int(i)
            } else {
                ErrorFieldValue::BigInt(v.to_string())
            }
        }
        DynSolValue::Bool(b) => ErrorFieldValue::Bool(*b),
        DynSolValue::Address(a) => ErrorFieldValue::Address(format!("{a:#x}")),
        DynSolValue::String(s) => ErrorFieldValue::Str(s.clone()),
        DynSolValue::Bytes(b) => ErrorFieldValue::Bytes(b.clone()),
        DynSolValue::FixedBytes(fb, _) => ErrorFieldValue::Bytes(fb.to_vec()),
        _ => ErrorFieldValue::Bytes(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chainerrors_core::registry::{ErrorParam, ErrorSignature, MemoryErrorRegistry};
    use tiny_keccak::{Hasher, Keccak};

    fn selector_of(sig: &str) -> [u8; 4] {
        let mut k = Keccak::v256();
        k.update(sig.as_bytes());
        let mut out = [0u8; 32];
        k.finalize(&mut out);
        [out[0], out[1], out[2], out[3]]
    }

    fn make_reg(name: &str, sig_str: &str, inputs: Vec<ErrorParam>) -> MemoryErrorRegistry {
        let reg = MemoryErrorRegistry::new();
        reg.register(ErrorSignature {
            name: name.to_string(),
            signature: sig_str.to_string(),
            selector: selector_of(sig_str),
            inputs,
            source: "test".to_string(),
            suggestion: None,
        });
        reg
    }

    #[test]
    fn decode_oz_ownable_unauthorized() {
        // error OwnableUnauthorizedAccount(address account)
        // selector = keccak256("OwnableUnauthorizedAccount(address)")[..4]
        let sig_str = "OwnableUnauthorizedAccount(address)";
        let reg = make_reg(
            "OwnableUnauthorizedAccount",
            sig_str,
            vec![ErrorParam { name: "account".into(), ty: "address".into() }],
        );
        let sel = selector_of(sig_str);

        // Encode: selector ++ address (20 bytes, zero-padded to 32)
        let addr_bytes = hex::decode("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045").unwrap();
        let mut data = sel.to_vec();
        data.extend_from_slice(&[0u8; 12]); // left-pad address to 32 bytes
        data.extend_from_slice(&addr_bytes);

        let kind = decode_custom_error(&data, &reg).unwrap();
        match kind {
            ErrorKind::CustomError { name, inputs } => {
                assert_eq!(name, "OwnableUnauthorizedAccount");
                assert_eq!(inputs.len(), 1);
                assert_eq!(inputs[0].0, "account");
            }
            _ => panic!("unexpected kind: {kind:?}"),
        }
    }

    #[test]
    fn decode_custom_error_no_args() {
        // error T() — Uniswap V3 style zero-arg custom error
        let sig_str = "T()";
        let reg = make_reg("T", sig_str, vec![]);
        let sel = selector_of(sig_str);

        let kind = decode_custom_error(&sel, &reg).unwrap();
        assert!(matches!(kind, ErrorKind::CustomError { ref name, .. } if name == "T"));
    }

    #[test]
    fn decode_unknown_selector_returns_none() {
        let reg = MemoryErrorRegistry::new();
        let data = [0xde, 0xad, 0xbe, 0xef, 0x00, 0x00];
        assert!(decode_custom_error(&data, &reg).is_none());
    }
}
