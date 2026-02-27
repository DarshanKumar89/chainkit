//! Raw and decoded event types.

use crate::chain::ChainId;
use crate::types::NormalizedValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A raw, undecoded event as received from an RPC node or batch loader.
/// This is the input to every decoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    /// The chain this event came from
    pub chain: ChainId,
    /// Transaction hash (hex-encoded with 0x prefix for EVM, base58 for Solana, etc.)
    pub tx_hash: String,
    /// Block number / slot / height
    pub block_number: u64,
    /// Block timestamp (Unix seconds, UTC)
    pub block_timestamp: i64,
    /// Log / event index within the transaction
    pub log_index: u32,
    /// EVM: topics[0] is the event signature hash; additional topics are indexed params.
    /// Solana: discriminator bytes. Cosmos: event type string.
    pub topics: Vec<String>,
    /// ABI-encoded non-indexed parameters (EVM) or raw instruction data (Solana/Cosmos).
    pub data: Vec<u8>,
    /// Contract address that emitted the event
    pub address: String,
    /// Optional raw transaction receipt for additional context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_receipt: Option<serde_json::Value>,
}

impl RawEvent {
    /// EVM convenience: returns topics[0] as the event signature fingerprint, if present.
    pub fn evm_event_signature(&self) -> Option<&str> {
        self.topics.first().map(|s| s.as_str())
    }
}

/// The keccak256 (EVM) or SHA-256 (non-EVM) hash of an event's canonical signature.
/// Used for O(1) schema lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventFingerprint(pub String);

impl EventFingerprint {
    pub fn new(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }

    pub fn as_hex(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EventFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A fully decoded event — the primary output of ChainCodec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedEvent {
    /// The chain this event came from
    pub chain: ChainId,
    /// Matched schema name, e.g. "UniswapV3Swap"
    pub schema: String,
    /// Schema version that was used to decode this event
    pub schema_version: u32,
    /// Transaction hash
    pub tx_hash: String,
    /// Block number
    pub block_number: u64,
    /// Block timestamp (Unix seconds)
    pub block_timestamp: i64,
    /// Log index
    pub log_index: u32,
    /// Contract address
    pub address: String,
    /// Decoded, normalized field values keyed by field name
    pub fields: HashMap<String, NormalizedValue>,
    /// The fingerprint used to match this event
    pub fingerprint: EventFingerprint,
    /// Optional: fields that failed to decode (only present in lenient mode)
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub decode_errors: HashMap<String, String>,
}

impl DecodedEvent {
    /// Get a field value by name.
    pub fn field(&self, name: &str) -> Option<&NormalizedValue> {
        self.fields.get(name)
    }

    /// Returns `true` if any fields failed to decode.
    pub fn has_errors(&self) -> bool {
        !self.decode_errors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::chains;

    fn sample_raw() -> RawEvent {
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xabc123".into(),
            block_number: 19_000_000,
            block_timestamp: 1_700_000_000,
            log_index: 2,
            topics: vec![
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67".into(),
            ],
            data: vec![0u8; 32],
            address: "0xe0554a476a092703abdb3ef35c80e0d76d32939f".into(),
            raw_receipt: None,
        }
    }

    #[test]
    fn raw_event_evm_signature() {
        let e = sample_raw();
        assert!(e.evm_event_signature().unwrap().starts_with("0xc42079"));
    }
}
