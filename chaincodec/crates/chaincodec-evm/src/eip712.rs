//! EIP-712 Typed Structured Data decoder.
//!
//! Parses `eth_signTypedData_v4` JSON payloads into strongly-typed Rust structures.
//!
//! EIP-712 defines a way to hash and sign typed structured data in a human-readable way.
//! The JSON format has three top-level keys:
//! - `types` — struct type definitions (including `EIP712Domain`)
//! - `primaryType` — the name of the root type being signed
//! - `domain` — domain separator values
//! - `message` — the actual data being signed
//!
//! # Reference
//! <https://eips.ethereum.org/EIPS/eip-712>

use chaincodec_core::types::NormalizedValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A single field within an EIP-712 type definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Eip712TypeField {
    /// Field name
    pub name: String,
    /// Solidity type string (e.g. "address", "uint256", "MyStruct")
    #[serde(rename = "type")]
    pub ty: String,
}

/// A parsed EIP-712 typed data payload (eth_signTypedData_v4 format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedData {
    /// All type definitions (struct name → field list)
    pub types: HashMap<String, Vec<Eip712TypeField>>,
    /// The root type being signed (must exist in `types`)
    pub primary_type: String,
    /// Domain separator values
    pub domain: HashMap<String, TypedValue>,
    /// The structured data to be signed
    pub message: HashMap<String, TypedValue>,
}

/// A value in typed data — may be primitive or nested struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TypedValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    Str(String),
    Array(Vec<TypedValue>),
    Object(HashMap<String, TypedValue>),
}

impl TypedValue {
    /// Convert to a canonical `NormalizedValue` using a type hint.
    pub fn to_normalized(&self, type_hint: &str) -> NormalizedValue {
        match self {
            TypedValue::Null => NormalizedValue::Null,
            TypedValue::Bool(b) => NormalizedValue::Bool(*b),
            TypedValue::Str(s) => {
                if type_hint == "address" {
                    NormalizedValue::Address(s.clone())
                } else if type_hint.starts_with("bytes") {
                    // Hex-encoded bytes
                    let hex = s.strip_prefix("0x").unwrap_or(s);
                    match hex::decode(hex) {
                        Ok(b) => NormalizedValue::Bytes(b),
                        Err(_) => NormalizedValue::Str(s.clone()),
                    }
                } else {
                    NormalizedValue::Str(s.clone())
                }
            }
            TypedValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    NormalizedValue::Uint(u as u128)
                } else if let Some(i) = n.as_i64() {
                    NormalizedValue::Int(i as i128)
                } else {
                    NormalizedValue::Str(n.to_string())
                }
            }
            TypedValue::Array(elems) => {
                // For arrays: strip trailing "[]" from type hint
                let inner_type = type_hint.strip_suffix("[]").unwrap_or(type_hint);
                NormalizedValue::Array(
                    elems.iter().map(|e| e.to_normalized(inner_type)).collect(),
                )
            }
            TypedValue::Object(fields) => {
                // Struct — fields become a tuple
                let named: Vec<(String, NormalizedValue)> = fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_normalized("unknown")))
                    .collect();
                NormalizedValue::Tuple(named)
            }
        }
    }
}

/// Parser for EIP-712 JSON payloads.
pub struct Eip712Parser;

impl Eip712Parser {
    /// Parse a JSON string conforming to the EIP-712 `eth_signTypedData_v4` format.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed or missing required fields.
    pub fn parse(json: &str) -> Result<TypedData, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;

        let types_raw = v.get("types").ok_or("missing 'types' field")?;
        let types = parse_types(types_raw)?;

        let primary_type = v
            .get("primaryType")
            .and_then(|v| v.as_str())
            .ok_or("missing 'primaryType'")?
            .to_string();

        let domain_raw = v.get("domain").ok_or("missing 'domain'")?;
        let domain = parse_object_values(domain_raw)?;

        let message_raw = v.get("message").ok_or("missing 'message'")?;
        let message = parse_object_values(message_raw)?;

        Ok(TypedData {
            types,
            primary_type,
            domain,
            message,
        })
    }

    /// Returns the type fields for the primary type.
    pub fn primary_type_fields<'a>(td: &'a TypedData) -> Option<&'a Vec<Eip712TypeField>> {
        td.types.get(&td.primary_type)
    }

    /// Compute the EIP-712 domain separator hash.
    ///
    /// Returns the domain separator as a hex string.
    pub fn domain_separator_hex(td: &TypedData) -> String {
        // Domain type hash = keccak256(encode_type("EIP712Domain", types))
        // For simplicity we serialize the domain to JSON and hash it
        // (production impl should use proper ABI encoding)
        let domain_json = serde_json::to_string(&td.domain).unwrap_or_default();
        let hash = tiny_keccak::keccak256(domain_json.as_bytes());
        format!("0x{}", hex::encode(hash))
    }
}

fn parse_types(v: &Value) -> Result<HashMap<String, Vec<Eip712TypeField>>, String> {
    let obj = v.as_object().ok_or("'types' must be an object")?;
    let mut types = HashMap::new();
    for (type_name, fields_val) in obj {
        let fields_arr = fields_val
            .as_array()
            .ok_or_else(|| format!("type '{}' fields must be an array", type_name))?;
        let mut fields = Vec::new();
        for field_val in fields_arr {
            let name = field_val
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("field in '{}' missing 'name'", type_name))?
                .to_string();
            let ty = field_val
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("field '{}' in '{}' missing 'type'", name, type_name))?
                .to_string();
            fields.push(Eip712TypeField { name, ty });
        }
        types.insert(type_name.clone(), fields);
    }
    Ok(types)
}

fn parse_object_values(v: &Value) -> Result<HashMap<String, TypedValue>, String> {
    let obj = v.as_object().ok_or("expected JSON object")?;
    let mut map = HashMap::new();
    for (k, v) in obj {
        map.insert(k.clone(), json_to_typed_value(v));
    }
    Ok(map)
}

fn json_to_typed_value(v: &Value) -> TypedValue {
    match v {
        Value::Null => TypedValue::Null,
        Value::Bool(b) => TypedValue::Bool(*b),
        Value::Number(n) => TypedValue::Number(n.clone()),
        Value::String(s) => TypedValue::Str(s.clone()),
        Value::Array(arr) => TypedValue::Array(arr.iter().map(json_to_typed_value).collect()),
        Value::Object(obj) => {
            let map: HashMap<String, TypedValue> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_typed_value(v)))
                .collect();
            TypedValue::Object(map)
        }
    }
}

// Internal helper - tiny_keccak wrapper
mod tiny_keccak {
    pub fn keccak256(data: &[u8]) -> [u8; 32] {
        use ::tiny_keccak::{Hasher, Keccak};
        let mut k = Keccak::v256();
        k.update(data);
        let mut out = [0u8; 32];
        k.finalize(&mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Example from EIP-712 spec
    const EIP712_EXAMPLE: &str = r#"{
        "types": {
            "EIP712Domain": [
                {"name": "name",              "type": "string"},
                {"name": "version",           "type": "string"},
                {"name": "chainId",           "type": "uint256"},
                {"name": "verifyingContract", "type": "address"}
            ],
            "Mail": [
                {"name": "from",     "type": "Person"},
                {"name": "to",       "type": "Person"},
                {"name": "contents", "type": "string"}
            ],
            "Person": [
                {"name": "name",   "type": "string"},
                {"name": "wallet", "type": "address"}
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
            "from": {
                "name": "Cow",
                "wallet": "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"
            },
            "to": {
                "name": "Bob",
                "wallet": "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB"
            },
            "contents": "Hello, Bob!"
        }
    }"#;

    #[test]
    fn parse_eip712_example() {
        let td = Eip712Parser::parse(EIP712_EXAMPLE).unwrap();
        assert_eq!(td.primary_type, "Mail");
        assert!(td.types.contains_key("EIP712Domain"));
        assert!(td.types.contains_key("Mail"));
        assert!(td.types.contains_key("Person"));
    }

    #[test]
    fn primary_type_fields_count() {
        let td = Eip712Parser::parse(EIP712_EXAMPLE).unwrap();
        let fields = Eip712Parser::primary_type_fields(&td).unwrap();
        assert_eq!(fields.len(), 3); // from, to, contents
    }

    #[test]
    fn domain_has_chain_id() {
        let td = Eip712Parser::parse(EIP712_EXAMPLE).unwrap();
        assert!(td.domain.contains_key("chainId"));
    }

    #[test]
    fn message_contents() {
        let td = Eip712Parser::parse(EIP712_EXAMPLE).unwrap();
        let contents = td.message.get("contents").unwrap();
        assert_eq!(*contents, TypedValue::Str("Hello, Bob!".into()));
    }

    #[test]
    fn missing_fields_return_error() {
        let bad_json = r#"{"types": {}}"#;
        let result = Eip712Parser::parse(bad_json);
        assert!(result.is_err());
    }
}
