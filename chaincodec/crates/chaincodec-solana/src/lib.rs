//! # chaincodec-solana
//!
//! Solana Anchor IDL decoder for ChainCodec (Phase 2 implementation).
//!
//! ## Status: Scaffolded — implementation in Phase 2
//!
//! Solana events are structured differently from EVM:
//! - Programs emit "event" CPI logs, discriminated by the first 8 bytes
//!   (SHA-256 of "event:<EventName>", first 8 bytes)
//! - Anchor IDL defines event schemas in JSON format
//! - Data is Borsh-encoded, not ABI-encoded

use chaincodec_core::{
    chain::ChainFamily,
    decoder::{BatchDecodeResult, ChainDecoder, ErrorMode, ProgressCallback},
    error::{BatchDecodeError, DecodeError},
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::{Schema, SchemaRegistry},
};

/// Solana/Anchor event decoder.
/// Implements the `ChainDecoder` trait for Solana programs.
#[derive(Debug, Default, Clone)]
pub struct SolanaDecoder;

impl SolanaDecoder {
    pub fn new() -> Self {
        Self
    }
}

impl ChainDecoder for SolanaDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Solana
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        // Anchor discriminator: first 8 bytes of SHA-256("event:<EventName>")
        // For now return topics[0] if present, else compute from data prefix
        raw.topics
            .first()
            .map(|t| EventFingerprint::new(t.clone()))
            .unwrap_or_else(|| EventFingerprint::new("0x00000000"))
    }

    fn decode_event(
        &self,
        _raw: &RawEvent,
        _schema: &Schema,
    ) -> Result<DecodedEvent, DecodeError> {
        // TODO Phase 2: implement Borsh decoding for Anchor events
        Err(DecodeError::Other(
            "Solana decoder not yet implemented (Phase 2)".into(),
        ))
    }

    fn supports_abi_guess(&self) -> bool {
        false
    }
}
