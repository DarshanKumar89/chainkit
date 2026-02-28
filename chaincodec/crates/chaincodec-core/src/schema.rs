//! Schema types — the in-memory representation of a parsed CSDL schema.

use crate::chain::ChainId;
use crate::event::EventFingerprint;
use crate::types::CanonicalType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trust level assigned to a schema in the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    #[default]
    Unverified,
    CommunityVerified,
    MaintainerVerified,
    ProtocolVerified,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TrustLevel::Unverified => "unverified",
            TrustLevel::CommunityVerified => "community_verified",
            TrustLevel::MaintainerVerified => "maintainer_verified",
            TrustLevel::ProtocolVerified => "protocol_verified",
        };
        write!(f, "{s}")
    }
}

/// Definition of a single field within a schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    /// ChainCodec canonical type
    pub ty: CanonicalType,
    /// EVM: is this an indexed topic?
    pub indexed: bool,
    /// Whether this field can be absent / null
    pub nullable: bool,
    /// Optional human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Metadata block attached to a schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SchemaMeta {
    /// Protocol slug, e.g. "uniswap-v3"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    /// Category, e.g. "dex", "lending", "bridge", "nft"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Whether maintainers have reviewed and verified this schema
    pub verified: bool,
    /// Assigned trust level
    pub trust_level: TrustLevel,
    /// Optional protocol team signature (hex-encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance_sig: Option<String>,
}

/// A parsed, validated schema definition.
/// This is the in-memory representation of a CSDL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    /// PascalCase schema name, e.g. "UniswapV3Swap"
    pub name: String,
    /// Schema version (increments on breaking changes)
    pub version: u32,
    /// Chains this schema applies to (by slug)
    pub chains: Vec<String>,
    /// Contract address(es) — None means "any address"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Vec<String>>,
    /// The blockchain event name, e.g. "Swap"
    pub event: String,
    /// Fingerprint: keccak256 of the event signature (EVM) or SHA-256 (non-EVM)
    pub fingerprint: EventFingerprint,
    /// Optional: the schema this one supersedes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Optional: forward pointer to a successor schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// Whether this schema is deprecated
    pub deprecated: bool,
    /// Ordered field definitions (order matters for ABI decode)
    pub fields: Vec<(String, FieldDef)>,
    /// Metadata
    pub meta: SchemaMeta,
}

impl Schema {
    /// Returns field definitions as a lookup map (name → def).
    pub fn fields_map(&self) -> HashMap<&str, &FieldDef> {
        self.fields.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }

    /// Returns only the indexed fields (EVM topics[1..]).
    pub fn indexed_fields(&self) -> Vec<(&str, &FieldDef)> {
        self.fields
            .iter()
            .filter(|(_, f)| f.indexed)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Returns only the non-indexed fields (EVM data payload).
    pub fn data_fields(&self) -> Vec<(&str, &FieldDef)> {
        self.fields
            .iter()
            .filter(|(_, f)| !f.indexed)
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }
}

/// A thread-safe, read-only view of a schema registry.
/// Concrete implementations live in `chaincodec-registry`.
pub trait SchemaRegistry: Send + Sync {
    /// Look up a schema by its fingerprint.
    fn get_by_fingerprint(&self, fp: &EventFingerprint) -> Option<Schema>;

    /// Look up a schema by name and optional version.
    /// If `version` is None, returns the latest non-deprecated version.
    fn get_by_name(&self, name: &str, version: Option<u32>) -> Option<Schema>;

    /// Returns all schemas applicable to the given chain slug.
    fn list_for_chain(&self, chain_slug: &str) -> Vec<Schema>;

    /// Returns the full evolution chain for a schema: from oldest to newest.
    fn history(&self, name: &str) -> Vec<Schema>;
}
