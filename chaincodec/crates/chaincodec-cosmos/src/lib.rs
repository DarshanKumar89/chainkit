//! # chaincodec-cosmos
//!
//! Cosmos / CosmWasm event decoder for ChainCodec (Phase 2 implementation).
//!
//! ## Status: Scaffolded — implementation in Phase 2
//!
//! Cosmos events are emitted as typed key-value attribute lists in the
//! ABCI event format. CosmWasm contracts emit `wasm` events with a
//! `_contract_address` attribute and typed payload attributes.

use chaincodec_core::{
    chain::ChainFamily,
    decoder::ChainDecoder,
    error::DecodeError,
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::Schema,
};
use sha2::{Digest, Sha256};

/// Cosmos/CosmWasm event decoder.
#[derive(Debug, Default, Clone)]
pub struct CosmosDecoder;

impl CosmosDecoder {
    pub fn new() -> Self {
        Self
    }
}

impl ChainDecoder for CosmosDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Cosmos
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        // Cosmos fingerprint = SHA-256 of the event type string
        let event_type = raw.topics.first().map(|s| s.as_str()).unwrap_or("");
        let hash = Sha256::digest(event_type.as_bytes());
        EventFingerprint::new(format!("0x{}", hex::encode(&hash[..16])))
    }

    fn decode_event(
        &self,
        _raw: &RawEvent,
        _schema: &Schema,
    ) -> Result<DecodedEvent, DecodeError> {
        // TODO Phase 2: implement Cosmos ABCI event decoding
        Err(DecodeError::Other(
            "Cosmos decoder not yet implemented (Phase 2)".into(),
        ))
    }
}
