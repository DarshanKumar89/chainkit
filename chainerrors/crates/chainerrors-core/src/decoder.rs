//! The `ErrorDecoder` trait — implemented by each chain-specific crate.

use crate::types::{DecodedError, ErrorContext};
use thiserror::Error;

/// Errors that can occur during error decoding.
#[derive(Debug, Error)]
pub enum DecodeErrorError {
    #[error("ABI decode failed: {reason}")]
    AbiDecodeFailed { reason: String },

    #[error("Invalid revert data: {reason}")]
    InvalidData { reason: String },

    #[error("Unsupported chain: {chain}")]
    UnsupportedChain { chain: String },

    #[error("Registry lookup failed: {reason}")]
    RegistryError { reason: String },

    #[error("{0}")]
    Other(String),
}

/// A chain-specific error decoder.
///
/// Each chain family (EVM, Solana, …) provides its own implementation.
/// Implementations must be `Send + Sync` for use in async contexts.
pub trait ErrorDecoder: Send + Sync {
    /// Returns the chain family name this decoder handles (e.g. `"evm"`).
    fn chain_family(&self) -> &'static str;

    /// Decode raw revert/error data from a failed transaction.
    ///
    /// `revert_data` is the raw bytes returned by the node in the
    /// `revert` field (EVM) or equivalent.
    ///
    /// On success returns a `DecodedError` with the highest-confidence
    /// interpretation available. Never returns `Err` for unknown selectors —
    /// instead returns a `RawRevert` or `Empty` kind.
    fn decode(
        &self,
        revert_data: &[u8],
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError>;

    /// Convenience: decode from a hex string (with or without `0x` prefix).
    fn decode_hex(
        &self,
        hex_str: &str,
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError> {
        let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        let bytes = hex::decode(stripped).map_err(|e| DecodeErrorError::InvalidData {
            reason: format!("invalid hex: {e}"),
        })?;
        self.decode(&bytes, ctx)
    }
}
