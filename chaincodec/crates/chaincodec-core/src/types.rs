//! Cross-chain type normalization.
//!
//! Each blockchain VM uses different primitive types. ChainCodec normalizes
//! all decoded values into a single canonical type system so consumers
//! never need to handle chain-specific representations.

use serde::{Deserialize, Serialize};
use std::fmt;

/// ChainCodec's canonical type system.
/// Each variant corresponds to a normalized representation regardless of
/// the originating blockchain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanonicalType {
    // --- Integer types ---
    /// Unsigned integer (u8 .. u256). Width in bits.
    Uint(u16),
    /// Signed integer (i8 .. i256). Width in bits.
    Int(u16),
    /// Boolean
    Bool,

    // --- Byte types ---
    /// Fixed-size byte array (bytes1 .. bytes32). Length in bytes.
    Bytes(u8),
    /// Variable-length byte array
    BytesVec,
    /// UTF-8 string
    Str,

    // --- Address types ---
    /// 20-byte EVM address (hex, checksummed)
    Address,
    /// Solana public key (base58)
    Pubkey,
    /// Cosmos bech32 address
    Bech32Address,

    // --- Composite types ---
    /// Fixed-length array of a type
    Array { elem: Box<CanonicalType>, len: u64 },
    /// Variable-length array of a type
    Vec(Box<CanonicalType>),
    /// Tuple / struct
    Tuple(Vec<(String, CanonicalType)>),

    // --- Special ---
    /// 256-bit hash (tx hash, block hash, etc.)
    Hash256,
    /// Unix timestamp (seconds)
    Timestamp,
    /// U128-scaled decimal with metadata
    Decimal { scale: u8 },
}

impl fmt::Display for CanonicalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalType::Uint(bits) => write!(f, "uint{bits}"),
            CanonicalType::Int(bits) => write!(f, "int{bits}"),
            CanonicalType::Bool => write!(f, "bool"),
            CanonicalType::Bytes(n) => write!(f, "bytes{n}"),
            CanonicalType::BytesVec => write!(f, "bytes"),
            CanonicalType::Str => write!(f, "string"),
            CanonicalType::Address => write!(f, "address"),
            CanonicalType::Pubkey => write!(f, "pubkey"),
            CanonicalType::Bech32Address => write!(f, "bech32"),
            CanonicalType::Array { elem, len } => write!(f, "{elem}[{len}]"),
            CanonicalType::Vec(elem) => write!(f, "{elem}[]"),
            CanonicalType::Tuple(_) => write!(f, "tuple"),
            CanonicalType::Hash256 => write!(f, "hash256"),
            CanonicalType::Timestamp => write!(f, "timestamp"),
            CanonicalType::Decimal { scale } => write!(f, "decimal({scale})"),
        }
    }
}

/// A decoded, normalized value.
/// Consumers always deal with `NormalizedValue` regardless of chain family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum NormalizedValue {
    Uint(u128),
    /// Large uints (> u128) stored as decimal string
    BigUint(String),
    Int(i128),
    /// Large ints (> i128) stored as decimal string
    BigInt(String),
    Bool(bool),
    Bytes(Vec<u8>),
    Str(String),
    /// EVM address — 20 bytes, hex with 0x prefix (EIP-55 checksummed)
    Address(String),
    /// Solana public key — base58
    Pubkey(String),
    /// Cosmos bech32
    Bech32(String),
    Hash256(String),
    Timestamp(i64),
    Array(Vec<NormalizedValue>),
    Tuple(Vec<(String, NormalizedValue)>),
    Null,
}

impl NormalizedValue {
    /// Returns `true` if this value is logically null/absent.
    pub fn is_null(&self) -> bool {
        matches!(self, NormalizedValue::Null)
    }

    /// Returns the inner string if this is an Address value.
    pub fn as_address(&self) -> Option<&str> {
        match self {
            NormalizedValue::Address(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Coerce to a u128 if this is a small Uint.
    pub fn as_u128(&self) -> Option<u128> {
        match self {
            NormalizedValue::Uint(v) => Some(*v),
            _ => None,
        }
    }
}

impl fmt::Display for NormalizedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NormalizedValue::Uint(v) => write!(f, "{v}"),
            NormalizedValue::BigUint(v) => write!(f, "{v}"),
            NormalizedValue::Int(v) => write!(f, "{v}"),
            NormalizedValue::BigInt(v) => write!(f, "{v}"),
            NormalizedValue::Bool(v) => write!(f, "{v}"),
            NormalizedValue::Bytes(b) => write!(f, "0x{}", hex::encode(b)),
            NormalizedValue::Str(s) => write!(f, "{s}"),
            NormalizedValue::Address(a) => write!(f, "{a}"),
            NormalizedValue::Pubkey(p) => write!(f, "{p}"),
            NormalizedValue::Bech32(b) => write!(f, "{b}"),
            NormalizedValue::Hash256(h) => write!(f, "{h}"),
            NormalizedValue::Timestamp(t) => write!(f, "{t}"),
            NormalizedValue::Array(v) => {
                let parts: Vec<_> = v.iter().map(|x| x.to_string()).collect();
                write!(f, "[{}]", parts.join(", "))
            }
            NormalizedValue::Tuple(fields) => {
                let parts: Vec<_> = fields.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                write!(f, "{{{}}}", parts.join(", "))
            }
            NormalizedValue::Null => write!(f, "null"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_type_display() {
        assert_eq!(CanonicalType::Uint(256).to_string(), "uint256");
        assert_eq!(CanonicalType::Address.to_string(), "address");
        assert_eq!(
            CanonicalType::Vec(Box::new(CanonicalType::Address)).to_string(),
            "address[]"
        );
    }

    #[test]
    fn normalized_value_serde_roundtrip() {
        let val = NormalizedValue::Address("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045".into());
        let json = serde_json::to_string(&val).unwrap();
        let back: NormalizedValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, back);
    }
}
