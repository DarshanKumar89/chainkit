//! # chaincodec-cosmos
//!
//! Cosmos / CosmWasm event decoder for ChainCodec.
//!
//! ## Event format
//!
//! Cosmos events follow the ABCI `Event` structure:
//! - `type`: string identifier (e.g. `"wasm"`, `"transfer"`, `"coin_received"`)
//! - `attributes`: list of `{key: String, value: String}` pairs
//!
//! CosmWasm contracts always emit a `wasm` event with a `_contract_address`
//! attribute identifying the emitting contract.
//!
//! ## RawEvent mapping
//! - `topics[0]`: event type string (e.g. `"wasm"`)
//! - `topics[1]`: optional CosmWasm action (e.g. `"transfer"`)
//! - `data`: JSON-encoded attribute list: `[{"key":"fieldName","value":"..."}]`
//! - `address`: contract address (bech32)
//!
//! ## CSDL fingerprint
//! Set to `SHA-256("event:<type>")[..16]` as a 32-hex-char `0x...` string.
//! For CosmWasm actions use `SHA-256("event:wasm/<action>")[..16]` to
//! differentiate events from the same contract type.

use chaincodec_core::{
    chain::ChainFamily,
    decoder::ChainDecoder,
    error::DecodeError,
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::{CanonicalType, Schema},
    types::NormalizedValue,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Cosmos / CosmWasm event decoder.
///
/// Decodes ABCI events from CosmWasm contracts by parsing JSON attribute arrays
/// and mapping string-encoded values to the schema's canonical types.
#[derive(Debug, Default, Clone)]
pub struct CosmosDecoder;

impl CosmosDecoder {
    pub fn new() -> Self {
        Self
    }

    /// Compute the fingerprint for a Cosmos event type string.
    ///
    /// For a bare ABCI type like `"transfer"`:  `SHA-256("event:transfer")[..16]`
    /// For a CosmWasm action like `"wasm/transfer"`: `SHA-256("event:wasm/transfer")[..16]`
    pub fn fingerprint_for(event_type: &str) -> EventFingerprint {
        let preimage = format!("event:{event_type}");
        let hash = Sha256::digest(preimage.as_bytes());
        EventFingerprint::new(format!("0x{}", hex::encode(&hash[..16])))
    }
}

impl ChainDecoder for CosmosDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Cosmos
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        // topics[0] = ABCI event type, topics[1] = optional CosmWasm action
        let event_type = match (
            raw.topics.first().map(|s| s.as_str()),
            raw.topics.get(1).map(|s| s.as_str()),
        ) {
            (Some(ty), Some(action)) if ty == "wasm" => format!("{ty}/{action}"),
            (Some(ty), _) => ty.to_string(),
            (None, _) => "unknown".to_string(),
        };
        Self::fingerprint_for(&event_type)
    }

    fn decode_event(&self, raw: &RawEvent, schema: &Schema) -> Result<DecodedEvent, DecodeError> {
        let attrs = parse_attributes(&raw.data)?;
        let mut fields: HashMap<String, NormalizedValue> = HashMap::new();
        let mut decode_errors: HashMap<String, String> = HashMap::new();

        for (field_name, field_def) in schema.fields.iter() {
            let raw_value = attrs.get(field_name.as_str()).map(|s| s.as_str());
            match decode_cosmos_field(raw_value, &field_def.ty) {
                Ok(val) => {
                    fields.insert(field_name.clone(), val);
                }
                Err(e) => {
                    decode_errors.insert(field_name.clone(), e.to_string());
                    fields.insert(field_name.clone(), NormalizedValue::Null);
                }
            }
        }

        Ok(DecodedEvent {
            chain: raw.chain.clone(),
            schema: schema.name.clone(),
            schema_version: schema.version,
            tx_hash: raw.tx_hash.clone(),
            block_number: raw.block_number,
            block_timestamp: raw.block_timestamp,
            log_index: raw.log_index,
            address: raw.address.clone(),
            fields,
            fingerprint: self.fingerprint(raw),
            decode_errors,
        })
    }

    fn supports_abi_guess(&self) -> bool {
        false
    }
}

// ─── Attribute parsing ────────────────────────────────────────────────────────

/// Parse a JSON attribute list into a name→value map.
///
/// Accepts two formats:
/// 1. Array of `{"key": "...", "value": "..."}` objects (standard ABCI JSON)
/// 2. Object map `{"fieldName": "value"}` (simplified form used in tests/fixtures)
fn parse_attributes(data: &[u8]) -> Result<HashMap<String, String>, DecodeError> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    let v: serde_json::Value = serde_json::from_slice(data)
        .map_err(|e| DecodeError::AbiDecodeFailed(format!("invalid JSON in raw.data: {e}")))?;

    match v {
        serde_json::Value::Array(items) => {
            let mut map = HashMap::new();
            for item in items {
                let key = item
                    .get("key")
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| {
                        DecodeError::AbiDecodeFailed("missing 'key' in attribute".into())
                    })?
                    .to_string();
                let value = match item.get("value") {
                    Some(serde_json::Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => String::new(),
                };
                map.insert(key, value);
            }
            Ok(map)
        }
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                let val = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                map.insert(k, val);
            }
            Ok(map)
        }
        _ => Err(DecodeError::AbiDecodeFailed(
            "raw.data must be a JSON array or object".into(),
        )),
    }
}

// ─── Field decoder ────────────────────────────────────────────────────────────

/// Decode a single Cosmos attribute (string-encoded) to a `NormalizedValue`.
fn decode_cosmos_field(
    raw: Option<&str>,
    ty: &CanonicalType,
) -> Result<NormalizedValue, DecodeError> {
    match ty {
        CanonicalType::Bool => {
            let s = require_attr(raw)?;
            match s.to_lowercase().as_str() {
                "true" | "1" | "yes" => Ok(NormalizedValue::Bool(true)),
                "false" | "0" | "no" => Ok(NormalizedValue::Bool(false)),
                _ => Err(DecodeError::AbiDecodeFailed(format!(
                    "invalid bool: {s:?}"
                ))),
            }
        }

        CanonicalType::Uint(_) => {
            let s = require_attr(raw)?;
            // Strip Cosmos denomination suffix: "1000000uatom" → "1000000"
            let numeric = strip_denom(s);
            if let Some(hex) = numeric.strip_prefix("0x").or_else(|| numeric.strip_prefix("0X")) {
                let v = u128::from_str_radix(hex, 16).map_err(|e| {
                    DecodeError::AbiDecodeFailed(format!("invalid uint hex {numeric:?}: {e}"))
                })?;
                Ok(NormalizedValue::Uint(v))
            } else {
                match numeric.parse::<u128>() {
                    Ok(v) => Ok(NormalizedValue::Uint(v)),
                    Err(_) if numeric.chars().all(|c| c.is_ascii_digit()) => {
                        Ok(NormalizedValue::BigUint(numeric.to_string()))
                    }
                    Err(_) => Err(DecodeError::AbiDecodeFailed(format!(
                        "invalid uint: {s:?}"
                    ))),
                }
            }
        }

        CanonicalType::Int(_) => {
            let s = require_attr(raw)?;
            let numeric = strip_denom(s);
            match numeric.parse::<i128>() {
                Ok(v) => Ok(NormalizedValue::Int(v)),
                Err(_) => {
                    let digits = numeric.trim_start_matches('-');
                    if digits.chars().all(|c| c.is_ascii_digit()) {
                        Ok(NormalizedValue::BigInt(numeric.to_string()))
                    } else {
                        Err(DecodeError::AbiDecodeFailed(format!("invalid int: {s:?}")))
                    }
                }
            }
        }

        CanonicalType::Str => Ok(NormalizedValue::Str(raw.unwrap_or("").to_string())),

        CanonicalType::Bech32Address => {
            let s = require_attr(raw)?;
            Ok(NormalizedValue::Bech32(s.to_string()))
        }

        CanonicalType::Address => {
            let s = require_attr(raw)?;
            let addr = if s.starts_with("0x") || s.starts_with("0X") {
                s.to_string()
            } else {
                format!("0x{s}")
            };
            Ok(NormalizedValue::Address(addr))
        }

        CanonicalType::Pubkey => {
            let s = require_attr(raw)?;
            Ok(NormalizedValue::Pubkey(s.to_string()))
        }

        CanonicalType::Bytes(n) => {
            let s = require_attr(raw)?;
            let hex_str = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
            let bytes = hex::decode(hex_str)
                .map_err(|e| DecodeError::AbiDecodeFailed(format!("invalid hex bytes: {e}")))?;
            if bytes.len() != *n as usize {
                return Err(DecodeError::AbiDecodeFailed(format!(
                    "expected {n} bytes, got {}",
                    bytes.len()
                )));
            }
            Ok(NormalizedValue::Bytes(bytes))
        }

        CanonicalType::BytesVec => {
            let s = raw.unwrap_or("");
            if s.is_empty() {
                return Ok(NormalizedValue::Bytes(vec![]));
            }
            let hex_str = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
            let bytes = hex::decode(hex_str).unwrap_or_else(|_| s.as_bytes().to_vec());
            Ok(NormalizedValue::Bytes(bytes))
        }

        CanonicalType::Hash256 => {
            let s = require_attr(raw)?;
            let hash = if s.starts_with("0x") || s.starts_with("0X") {
                s.to_string()
            } else {
                format!("0x{s}")
            };
            Ok(NormalizedValue::Hash256(hash))
        }

        CanonicalType::Timestamp => {
            let s = require_attr(raw)?;
            let numeric = strip_denom(s);
            let t: i64 = numeric.parse().map_err(|e| {
                DecodeError::AbiDecodeFailed(format!("invalid timestamp {numeric:?}: {e}"))
            })?;
            Ok(NormalizedValue::Timestamp(t))
        }

        CanonicalType::Decimal { .. } => {
            let s = require_attr(raw)?;
            let numeric = strip_denom(s);
            match numeric.parse::<u128>() {
                Ok(v) => Ok(NormalizedValue::Uint(v)),
                Err(_) => {
                    if let Ok(f) = numeric.parse::<f64>() {
                        Ok(NormalizedValue::BigUint(format!("{:.0}", f)))
                    } else {
                        Err(DecodeError::AbiDecodeFailed(format!(
                            "invalid decimal: {s:?}"
                        )))
                    }
                }
            }
        }

        CanonicalType::Array { elem, len } => {
            let items = parse_json_array(raw.unwrap_or("[]"), Some(*len as usize))?;
            let mut result = Vec::with_capacity(*len as usize);
            for item_str in items {
                result.push(decode_cosmos_field(item_str.as_deref(), elem)?);
            }
            Ok(NormalizedValue::Array(result))
        }

        CanonicalType::Vec(elem) => {
            let items = parse_json_array(raw.unwrap_or("[]"), None)?;
            let mut result = Vec::with_capacity(items.len());
            for item_str in items {
                result.push(decode_cosmos_field(item_str.as_deref(), elem)?);
            }
            Ok(NormalizedValue::Array(result))
        }

        CanonicalType::Tuple(tuple_fields) => {
            let s = raw.unwrap_or("{}");
            let json: serde_json::Value = serde_json::from_str(s)
                .map_err(|e| DecodeError::AbiDecodeFailed(format!("invalid tuple JSON: {e}")))?;
            let obj = json.as_object().ok_or_else(|| {
                DecodeError::AbiDecodeFailed("expected JSON object for tuple".into())
            })?;
            let mut result = Vec::with_capacity(tuple_fields.len());
            for (name, field_ty) in tuple_fields {
                let val_str = obj.get(name).map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                result.push((
                    name.clone(),
                    decode_cosmos_field(val_str.as_deref(), field_ty)?,
                ));
            }
            Ok(NormalizedValue::Tuple(result))
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn require_attr(raw: Option<&str>) -> Result<&str, DecodeError> {
    raw.ok_or_else(|| DecodeError::AbiDecodeFailed("missing required attribute".into()))
}

/// Strip trailing Cosmos denomination suffix.
/// e.g. `"1000000uatom"` → `"1000000"`, `"9ibc/ABC"` → `"9"`
fn strip_denom(s: &str) -> &str {
    let end = s
        .find(|c: char| c.is_alphabetic() || c == '/')
        .unwrap_or(s.len());
    &s[..end]
}

/// Parse a JSON-encoded array attribute into a `Vec<Option<String>>`.
fn parse_json_array(
    s: &str,
    expected_len: Option<usize>,
) -> Result<Vec<Option<String>>, DecodeError> {
    let json: serde_json::Value = serde_json::from_str(s)
        .map_err(|e| DecodeError::AbiDecodeFailed(format!("invalid JSON array: {e}")))?;
    let arr = json
        .as_array()
        .ok_or_else(|| DecodeError::AbiDecodeFailed("expected JSON array".into()))?;
    if let Some(len) = expected_len {
        if arr.len() != len {
            return Err(DecodeError::AbiDecodeFailed(format!(
                "expected array length {len}, got {}",
                arr.len()
            )));
        }
    }
    Ok(arr
        .iter()
        .map(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        })
        .collect())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chaincodec_core::chain::ChainFamily;

    #[test]
    fn fingerprint_deterministic() {
        let fp1 = CosmosDecoder::fingerprint_for("wasm/transfer");
        let fp2 = CosmosDecoder::fingerprint_for("wasm/transfer");
        assert_eq!(fp1, fp2);
        assert!(fp1.as_hex().starts_with("0x"));
        assert_eq!(fp1.as_hex().len(), 34); // "0x" + 32 hex chars (16 bytes)
    }

    #[test]
    fn fingerprint_different_types() {
        let fp_transfer = CosmosDecoder::fingerprint_for("transfer");
        let fp_wasm = CosmosDecoder::fingerprint_for("wasm/transfer");
        assert_ne!(fp_transfer, fp_wasm);
    }

    #[test]
    fn chain_family_is_cosmos() {
        assert_eq!(CosmosDecoder::new().chain_family(), ChainFamily::Cosmos);
    }

    #[test]
    fn parse_attributes_array_format() {
        let data =
            br#"[{"key":"sender","value":"cosmos1abc"},{"key":"amount","value":"1000000uatom"}]"#;
        let attrs = parse_attributes(data).unwrap();
        assert_eq!(attrs.get("sender").unwrap(), "cosmos1abc");
        assert_eq!(attrs.get("amount").unwrap(), "1000000uatom");
    }

    #[test]
    fn parse_attributes_object_format() {
        let data = br#"{"sender":"cosmos1abc","amount":"1000000"}"#;
        let attrs = parse_attributes(data).unwrap();
        assert_eq!(attrs.get("sender").unwrap(), "cosmos1abc");
    }

    #[test]
    fn parse_attributes_empty() {
        let attrs = parse_attributes(b"").unwrap();
        assert!(attrs.is_empty());
    }

    #[test]
    fn decode_uint_with_denom() {
        let val = decode_cosmos_field(Some("1000000uatom"), &CanonicalType::Uint(128)).unwrap();
        assert_eq!(val, NormalizedValue::Uint(1_000_000));
    }

    #[test]
    fn decode_uint_plain() {
        let val = decode_cosmos_field(Some("42000"), &CanonicalType::Uint(64)).unwrap();
        assert_eq!(val, NormalizedValue::Uint(42_000));
    }

    #[test]
    fn decode_bool_variants() {
        assert_eq!(
            decode_cosmos_field(Some("true"), &CanonicalType::Bool).unwrap(),
            NormalizedValue::Bool(true)
        );
        assert_eq!(
            decode_cosmos_field(Some("false"), &CanonicalType::Bool).unwrap(),
            NormalizedValue::Bool(false)
        );
        assert_eq!(
            decode_cosmos_field(Some("1"), &CanonicalType::Bool).unwrap(),
            NormalizedValue::Bool(true)
        );
    }

    #[test]
    fn decode_bech32() {
        let val = decode_cosmos_field(
            Some("cosmos1qyqa2zn5c85rjrxnq5fk"),
            &CanonicalType::Bech32Address,
        )
        .unwrap();
        assert_eq!(
            val,
            NormalizedValue::Bech32("cosmos1qyqa2zn5c85rjrxnq5fk".into())
        );
    }

    #[test]
    fn decode_str_field() {
        let val = decode_cosmos_field(Some("hello world"), &CanonicalType::Str).unwrap();
        assert_eq!(val, NormalizedValue::Str("hello world".into()));
    }

    #[test]
    fn decode_timestamp() {
        let val = decode_cosmos_field(Some("1700000000"), &CanonicalType::Timestamp).unwrap();
        assert_eq!(val, NormalizedValue::Timestamp(1_700_000_000));
    }

    #[test]
    fn strip_denom_various() {
        assert_eq!(strip_denom("1000000uatom"), "1000000");
        assert_eq!(strip_denom("500uluna"), "500");
        assert_eq!(strip_denom("100000000"), "100000000");
        assert_eq!(strip_denom("9ibc/ABC"), "9");
    }

    #[test]
    fn decode_event_round_trip() {
        use chaincodec_core::{
            chain::ChainId,
            event::RawEvent,
            schema::{FieldDef, Schema, SchemaMeta, TrustLevel},
        };

        let attrs = serde_json::json!([
            {"key": "sender", "value": "cosmos1abc"},
            {"key": "recipient", "value": "cosmos1def"},
            {"key": "amount", "value": "5000000"}
        ]);
        let data = serde_json::to_vec(&attrs).unwrap();

        let raw = RawEvent {
            chain: ChainId::cosmos("cosmos-hub"),
            tx_hash: "ABC123DEADBEEF".into(),
            block_number: 10_000_000,
            block_timestamp: 1_700_000_000,
            log_index: 0,
            topics: vec!["wasm".into(), "transfer".into()],
            data,
            address: "cosmos1contractaddress".into(),
            raw_receipt: None,
        };

        let schema = Schema {
            name: "CosmosWasmTransfer".into(),
            version: 1,
            chains: vec!["cosmos-hub".into()],
            address: None,
            event: "wasm/transfer".into(),
            fingerprint: CosmosDecoder::fingerprint_for("wasm/transfer"),
            supersedes: None,
            superseded_by: None,
            deprecated: false,
            fields: vec![
                (
                    "sender".into(),
                    FieldDef {
                        ty: CanonicalType::Bech32Address,
                        indexed: false,
                        nullable: false,
                        description: None,
                    },
                ),
                (
                    "recipient".into(),
                    FieldDef {
                        ty: CanonicalType::Bech32Address,
                        indexed: false,
                        nullable: false,
                        description: None,
                    },
                ),
                (
                    "amount".into(),
                    FieldDef {
                        ty: CanonicalType::Uint(128),
                        indexed: false,
                        nullable: false,
                        description: None,
                    },
                ),
            ],
            meta: SchemaMeta {
                protocol: Some("cosmwasm".into()),
                category: Some("transfer".into()),
                verified: false,
                trust_level: TrustLevel::Unverified,
                provenance_sig: None,
            },
        };

        let decoder = CosmosDecoder::new();
        let decoded = decoder.decode_event(&raw, &schema).unwrap();
        assert_eq!(decoded.schema, "CosmosWasmTransfer");
        assert_eq!(
            decoded.fields.get("sender").unwrap(),
            &NormalizedValue::Bech32("cosmos1abc".into())
        );
        assert_eq!(
            decoded.fields.get("amount").unwrap(),
            &NormalizedValue::Uint(5_000_000)
        );
        assert!(decoded.decode_errors.is_empty());
    }
}
