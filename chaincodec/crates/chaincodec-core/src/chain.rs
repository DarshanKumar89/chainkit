//! Chain family and identifier types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Top-level blockchain VM family.
/// Determines which decoder is dispatched for a raw event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChainFamily {
    Evm,
    Solana,
    Cosmos,
    Sui,
    Aptos,
    /// Third-party or experimental chains registered via the plugin system.
    Custom(String),
}

impl fmt::Display for ChainFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainFamily::Evm => write!(f, "evm"),
            ChainFamily::Solana => write!(f, "solana"),
            ChainFamily::Cosmos => write!(f, "cosmos"),
            ChainFamily::Sui => write!(f, "sui"),
            ChainFamily::Aptos => write!(f, "aptos"),
            ChainFamily::Custom(s) => write!(f, "{s}"),
        }
    }
}

/// A fully qualified chain identifier, e.g. `ethereum`, `arbitrum`, `solana-mainnet`.
/// Used as the primary key when selecting a decoder and querying schemas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId {
    /// Human-readable slug, e.g. "ethereum", "arbitrum-one", "solana-mainnet"
    pub slug: String,
    /// EVM chain ID integer, if applicable (e.g. 1 for Ethereum mainnet)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evm_chain_id: Option<u64>,
    /// The VM family this chain belongs to
    pub family: ChainFamily,
}

impl ChainId {
    pub fn evm(slug: impl Into<String>, chain_id: u64) -> Self {
        Self {
            slug: slug.into(),
            evm_chain_id: Some(chain_id),
            family: ChainFamily::Evm,
        }
    }

    pub fn solana(slug: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            evm_chain_id: None,
            family: ChainFamily::Solana,
        }
    }

    pub fn cosmos(slug: impl Into<String>) -> Self {
        Self {
            slug: slug.into(),
            evm_chain_id: None,
            family: ChainFamily::Cosmos,
        }
    }

    pub fn custom(slug: impl Into<String>, family_name: impl Into<String>) -> Self {
        let slug = slug.into();
        Self {
            slug,
            evm_chain_id: None,
            family: ChainFamily::Custom(family_name.into()),
        }
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.slug)
    }
}

/// Well-known chain IDs for convenience.
pub mod chains {
    use super::ChainId;

    pub fn ethereum() -> ChainId { ChainId::evm("ethereum", 1) }
    pub fn arbitrum() -> ChainId { ChainId::evm("arbitrum", 42161) }
    pub fn base() -> ChainId { ChainId::evm("base", 8453) }
    pub fn polygon() -> ChainId { ChainId::evm("polygon", 137) }
    pub fn optimism() -> ChainId { ChainId::evm("optimism", 10) }
    pub fn solana_mainnet() -> ChainId { ChainId::solana("solana-mainnet") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_id_display() {
        assert_eq!(chains::ethereum().to_string(), "ethereum");
        assert_eq!(chains::arbitrum().to_string(), "arbitrum");
    }

    #[test]
    fn chain_family_serde() {
        let json = serde_json::to_string(&ChainFamily::Evm).unwrap();
        assert_eq!(json, "\"evm\"");
        let parsed: ChainFamily = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ChainFamily::Evm);
    }
}
