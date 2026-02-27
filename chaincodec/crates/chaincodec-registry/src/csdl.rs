//! CSDL (ChainCodec Schema Definition Language) parser.
//!
//! CSDL is a YAML-based DSL for defining blockchain event schemas.
//! This parser converts raw YAML text into `chaincodec_core::Schema`.
//!
//! A single `.csdl` file may contain multiple schema documents separated
//! by `---`. Use `parse_all()` to get every schema, or `parse()` to get
//! only the first one.

use chaincodec_core::{
    error::RegistryError,
    event::EventFingerprint,
    schema::{FieldDef, Schema, SchemaMeta, TrustLevel},
    types::CanonicalType,
};
use indexmap::IndexMap;
use serde::Deserialize;

// ─── Raw CSDL serde types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CsdlRaw {
    schema: String,
    version: u32,
    #[serde(default)]
    description: Option<String>,
    chains: Vec<String>,
    #[serde(default)]
    address: Option<serde_yaml::Value>,
    event: String,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    supersedes: Option<String>,
    #[serde(default)]
    superseded_by: Option<String>,
    #[serde(default)]
    deprecated: bool,
    // IndexMap preserves YAML insertion order — critical for ABI decode field ordering.
    fields: IndexMap<String, CsdlFieldRaw>,
    #[serde(default)]
    meta: CsdlMetaRaw,
}

#[derive(Debug, Deserialize, Default)]
struct CsdlMetaRaw {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    trust_level: Option<String>,
    #[serde(default)]
    provenance_sig: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CsdlFieldRaw {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    indexed: bool,
    #[serde(default)]
    nullable: bool,
    #[serde(default)]
    description: Option<String>,
}

// ─── Parser ───────────────────────────────────────────────────────────────────

pub struct CsdlParser;

impl CsdlParser {
    /// Parse the first schema document from a CSDL YAML string.
    /// For files with multiple schemas (`---` separator), use `parse_all()`.
    pub fn parse(yaml: &str) -> Result<Schema, RegistryError> {
        let mut schemas = Self::parse_all(yaml)?;
        if schemas.is_empty() {
            return Err(RegistryError::ParseError("empty CSDL file".into()));
        }
        Ok(schemas.remove(0))
    }

    /// Parse all schema documents from a CSDL YAML string.
    ///
    /// `.csdl` files may contain multiple schemas separated by `---`.
    /// Each document is parsed independently and returned in file order.
    pub fn parse_all(yaml: &str) -> Result<Vec<Schema>, RegistryError> {
        use serde::de::Deserialize as _;

        let mut schemas = Vec::new();
        for doc in serde_yaml::Deserializer::from_str(yaml) {
            let value = serde_yaml::Value::deserialize(doc)
                .map_err(|e| RegistryError::ParseError(e.to_string()))?;
            // Skip null/empty documents (e.g. trailing `---`)
            if value.is_null() {
                continue;
            }
            schemas.push(Self::parse_value(value)?);
        }
        Ok(schemas)
    }

    /// Parse a single schema from a `serde_yaml::Value` (one YAML document).
    fn parse_value(value: serde_yaml::Value) -> Result<Schema, RegistryError> {
        // CSDL documents look like:
        //   schema UniswapV3Swap:
        //     version: 2
        //     ...
        // The top-level key is "schema <Name>".
        let mapping = match &value {
            serde_yaml::Value::Mapping(m) => m,
            _ => {
                return Err(RegistryError::ParseError(
                    "CSDL document must be a YAML mapping".into(),
                ))
            }
        };

        let (schema_key, schema_body) = mapping
            .iter()
            .find(|(k, _)| {
                k.as_str()
                    .map(|s| s.starts_with("schema "))
                    .unwrap_or(false)
            })
            .ok_or_else(|| RegistryError::ParseError("missing 'schema <Name>' key".into()))?;

        let schema_name = schema_key
            .as_str()
            .unwrap()
            .strip_prefix("schema ")
            .unwrap()
            .trim()
            .to_string();

        // Re-inject the `schema` field so CsdlRaw can deserialize it
        let body: CsdlRaw = {
            let mut m = serde_yaml::Mapping::new();
            if let serde_yaml::Value::Mapping(map) = schema_body.clone() {
                for (k, v) in map {
                    m.insert(k, v);
                }
            }
            m.insert(
                serde_yaml::Value::String("schema".into()),
                serde_yaml::Value::String(schema_name.clone()),
            );
            serde_yaml::from_value(serde_yaml::Value::Mapping(m))
                .map_err(|e| RegistryError::ParseError(e.to_string()))?
        };

        // Parse fields — IndexMap preserves YAML insertion order
        let mut fields: Vec<(String, FieldDef)> = Vec::with_capacity(body.fields.len());
        for (name, raw_field) in &body.fields {
            let ty = parse_type(&raw_field.ty).map_err(|e| {
                RegistryError::ParseError(format!("field '{}': {}", name, e))
            })?;
            fields.push((
                name.clone(),
                FieldDef {
                    ty,
                    indexed: raw_field.indexed,
                    nullable: raw_field.nullable,
                    description: raw_field.description.clone(),
                },
            ));
        }

        // Parse addresses
        let address = match &body.address {
            None => None,
            Some(serde_yaml::Value::String(s)) => Some(vec![s.clone()]),
            Some(serde_yaml::Value::Sequence(seq)) => Some(
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
            ),
            Some(serde_yaml::Value::Null) | Some(_) => None,
        };

        // Parse meta
        let trust_level = match body.meta.trust_level.as_deref() {
            Some("community_verified") => TrustLevel::CommunityVerified,
            Some("maintainer_verified") => TrustLevel::MaintainerVerified,
            Some("protocol_verified") => TrustLevel::ProtocolVerified,
            _ => TrustLevel::Unverified,
        };
        let meta = SchemaMeta {
            protocol: body.meta.protocol,
            category: body.meta.category,
            verified: body.meta.verified,
            trust_level,
            provenance_sig: body.meta.provenance_sig,
        };

        let fingerprint = body
            .fingerprint
            .map(EventFingerprint::new)
            .unwrap_or_else(|| EventFingerprint::new("0x".to_string()));

        Ok(Schema {
            name: schema_name,
            version: body.version,
            chains: body.chains,
            address,
            event: body.event,
            fingerprint,
            supersedes: body.supersedes,
            superseded_by: body.superseded_by,
            deprecated: body.deprecated,
            fields,
            meta,
        })
    }
}

/// Parse a ChainCodec canonical type string into a `CanonicalType`.
fn parse_type(s: &str) -> Result<CanonicalType, String> {
    let s = s.trim();
    match s {
        "bool" => Ok(CanonicalType::Bool),
        "address" => Ok(CanonicalType::Address),
        "pubkey" => Ok(CanonicalType::Pubkey),
        "bech32" => Ok(CanonicalType::Bech32Address),
        "bytes" => Ok(CanonicalType::BytesVec),
        "string" => Ok(CanonicalType::Str),
        "hash256" => Ok(CanonicalType::Hash256),
        "timestamp" => Ok(CanonicalType::Timestamp),
        _ if s.starts_with("uint") && s[4..].parse::<u16>().is_ok() => {
            Ok(CanonicalType::Uint(s[4..].parse().unwrap()))
        }
        _ if s.starts_with("int") && s[3..].parse::<u16>().is_ok() => {
            Ok(CanonicalType::Int(s[3..].parse().unwrap()))
        }
        _ if s.starts_with("bytes") && s[5..].parse::<u8>().is_ok() => {
            Ok(CanonicalType::Bytes(s[5..].parse().unwrap()))
        }
        _ if s.ends_with("[]") => {
            let inner = parse_type(&s[..s.len() - 2])?;
            Ok(CanonicalType::Vec(Box::new(inner)))
        }
        _ => Err(format!("unknown type: '{s}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum, arbitrum, polygon, base]
  address: null
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:   { type: address, indexed: true  }
    to:     { type: address, indexed: true  }
    value:  { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
"#;

    const MULTI_DOC_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true  }
    to:    { type: address, indexed: true  }
    value: { type: uint256, indexed: false }
  meta: {}
---
schema ERC20Approval:
  version: 1
  chains: [ethereum]
  event: Approval
  fingerprint: "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
  fields:
    owner:   { type: address, indexed: true  }
    spender: { type: address, indexed: true  }
    value:   { type: uint256, indexed: false }
  meta: {}
"#;

    #[test]
    fn parse_erc20_transfer() {
        let schema = CsdlParser::parse(SAMPLE_CSDL).unwrap();
        assert_eq!(schema.name, "ERC20Transfer");
        assert_eq!(schema.version, 1);
        assert_eq!(schema.fields.len(), 3);
        assert_eq!(schema.event, "Transfer");
        assert_eq!(schema.meta.trust_level, TrustLevel::MaintainerVerified);
    }

    #[test]
    fn field_order_preserved() {
        let schema = CsdlParser::parse(SAMPLE_CSDL).unwrap();
        // Fields must be in YAML declaration order: from, to, value
        assert_eq!(schema.fields[0].0, "from");
        assert_eq!(schema.fields[1].0, "to");
        assert_eq!(schema.fields[2].0, "value");
    }

    #[test]
    fn parse_multi_doc_csdl() {
        let schemas = CsdlParser::parse_all(MULTI_DOC_CSDL).unwrap();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].name, "ERC20Transfer");
        assert_eq!(schemas[1].name, "ERC20Approval");
    }

    #[test]
    fn parse_type_uint256() {
        let t = parse_type("uint256").unwrap();
        assert!(matches!(t, CanonicalType::Uint(256)));
    }

    #[test]
    fn parse_type_int24() {
        let t = parse_type("int24").unwrap();
        assert!(matches!(t, CanonicalType::Int(24)));
    }

    #[test]
    fn parse_type_uint160() {
        let t = parse_type("uint160").unwrap();
        assert!(matches!(t, CanonicalType::Uint(160)));
    }

    #[test]
    fn parse_type_address_array() {
        let t = parse_type("address[]").unwrap();
        assert!(matches!(t, CanonicalType::Vec(_)));
    }

    #[test]
    fn parse_type_bytes32() {
        let t = parse_type("bytes32").unwrap();
        assert!(matches!(t, CanonicalType::Bytes(32)));
    }
}
