//! `EvmDecoder` — the ChainDecoder implementation for all EVM chains.

use alloy_core::dyn_abi::{DynSolEvent, DynSolType, DynSolValue};
use alloy_primitives::B256;
use chaincodec_core::{
    chain::ChainFamily,
    decoder::{BatchDecodeResult, ChainDecoder, ErrorMode, ProgressCallback},
    error::{BatchDecodeError, DecodeError},
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::{Schema, SchemaRegistry},
    types::{CanonicalType, NormalizedValue},
};
use rayon::prelude::*;
use std::collections::HashMap;

use crate::{fingerprint, normalizer};

/// The EVM chain decoder.
/// Thread-safe, cheap to clone (no heap state).
#[derive(Debug, Default, Clone)]
pub struct EvmDecoder;

impl EvmDecoder {
    pub fn new() -> Self {
        Self
    }

    /// Build alloy `DynSolType` from a ChainCodec `CanonicalType`.
    fn canonical_to_dyn(ty: &CanonicalType) -> Result<DynSolType, DecodeError> {
        match ty {
            CanonicalType::Uint(bits) => Ok(DynSolType::Uint(*bits as usize)),
            CanonicalType::Int(bits) => Ok(DynSolType::Int(*bits as usize)),
            CanonicalType::Bool => Ok(DynSolType::Bool),
            CanonicalType::Bytes(n) => Ok(DynSolType::FixedBytes(*n as usize)),
            CanonicalType::BytesVec => Ok(DynSolType::Bytes),
            CanonicalType::Str => Ok(DynSolType::String),
            CanonicalType::Address => Ok(DynSolType::Address),
            CanonicalType::Array { elem, len } => {
                let inner = Self::canonical_to_dyn(elem)?;
                Ok(DynSolType::FixedArray(Box::new(inner), *len as usize))
            }
            CanonicalType::Vec(elem) => {
                let inner = Self::canonical_to_dyn(elem)?;
                Ok(DynSolType::Array(Box::new(inner)))
            }
            CanonicalType::Tuple(fields) => {
                let types: Result<Vec<DynSolType>, _> =
                    fields.iter().map(|(_, t)| Self::canonical_to_dyn(t)).collect();
                Ok(DynSolType::Tuple(types?))
            }
            CanonicalType::Hash256 => Ok(DynSolType::FixedBytes(32)),
            CanonicalType::Timestamp => Ok(DynSolType::Uint(256)),
            // Pubkey / Bech32 don't exist in EVM — treat as bytes
            CanonicalType::Pubkey | CanonicalType::Bech32Address => Ok(DynSolType::Bytes),
            CanonicalType::Decimal { .. } => Ok(DynSolType::Uint(256)),
        }
    }

    /// Decode the EVM log data (non-indexed params) as an ABI-encoded tuple.
    fn decode_data(
        &self,
        raw_data: &[u8],
        data_fields: &[(&str, &chaincodec_core::schema::FieldDef)],
    ) -> Result<HashMap<String, NormalizedValue>, DecodeError> {
        if data_fields.is_empty() {
            return Ok(HashMap::new());
        }

        let tuple_types: Result<Vec<DynSolType>, _> = data_fields
            .iter()
            .map(|(_, f)| Self::canonical_to_dyn(&f.ty))
            .collect();
        let tuple_types = tuple_types?;

        let tuple_type = DynSolType::Tuple(tuple_types);
        let decoded = tuple_type
            .abi_decode(raw_data)
            .map_err(|e| DecodeError::AbiDecodeFailed {
                reason: e.to_string(),
            })?;

        let values = match decoded {
            DynSolValue::Tuple(vals) => vals,
            other => vec![other],
        };

        let mut out = HashMap::new();
        for ((name, _), val) in data_fields.iter().zip(values.into_iter()) {
            out.insert(name.to_string(), normalizer::normalize(val));
        }
        Ok(out)
    }

    /// Decode a single indexed topic (always 32 bytes, ABI-encoded).
    ///
    /// # EVM ABI indexed-parameter encoding rules
    /// - **Value types** (uint, int, bool, address, bytes1–bytes32): padded to
    ///   32 bytes, stored directly — we can ABI-decode and recover the value.
    /// - **Reference types** (string, bytes, arrays, tuples): stored as the
    ///   `keccak256` of their ABI-encoded form — the original value is
    ///   **unrecoverable**. We return the raw 32-byte hash as `Bytes`.
    fn decode_topic(
        &self,
        topic_hex: &str,
        ty: &CanonicalType,
    ) -> Result<NormalizedValue, DecodeError> {
        let hex = topic_hex.strip_prefix("0x").unwrap_or(topic_hex);
        let bytes = hex::decode(hex).map_err(|e| DecodeError::InvalidRawEvent {
            reason: format!("invalid topic hex: {e}"),
        })?;

        // Reference types are hashed in indexed position — return raw bytes.
        match ty {
            CanonicalType::Str
            | CanonicalType::BytesVec
            | CanonicalType::Vec(_)
            | CanonicalType::Array { .. }
            | CanonicalType::Tuple(_) => {
                return Ok(NormalizedValue::Bytes(bytes));
            }
            _ => {}
        }

        let dyn_type = Self::canonical_to_dyn(ty)?;
        // Value types: ABI-encoded into exactly 32 bytes
        match dyn_type.abi_decode(&bytes) {
            Ok(val) => Ok(normalizer::normalize(val)),
            Err(e) => Err(DecodeError::AbiDecodeFailed {
                reason: format!("topic decode: {e}"),
            }),
        }
    }
}

impl ChainDecoder for EvmDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Evm
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        fingerprint::from_topics(&raw.topics)
            .unwrap_or_else(|| EventFingerprint::new("0x".repeat(32)))
    }

    fn decode_event(
        &self,
        raw: &RawEvent,
        schema: &Schema,
    ) -> Result<DecodedEvent, DecodeError> {
        let mut fields: HashMap<String, NormalizedValue> = HashMap::new();
        let mut decode_errors: HashMap<String, String> = HashMap::new();

        // Indexed fields → topics[1..]
        let indexed_fields = schema.indexed_fields();
        for (i, (name, field_def)) in indexed_fields.iter().enumerate() {
            let topic_idx = i + 1; // topics[0] is the event sig
            match raw.topics.get(topic_idx) {
                Some(topic) => match self.decode_topic(topic, &field_def.ty) {
                    Ok(val) => { fields.insert(name.to_string(), val); }
                    Err(e) => { decode_errors.insert(name.to_string(), e.to_string()); }
                },
                None => {
                    if !field_def.nullable {
                        return Err(DecodeError::MissingField { field: name.to_string() });
                    }
                    fields.insert(name.to_string(), NormalizedValue::Null);
                }
            }
        }

        // Non-indexed fields → data payload
        let data_fields = schema.data_fields();
        match self.decode_data(&raw.data, &data_fields) {
            Ok(decoded) => fields.extend(decoded),
            Err(e) => {
                // Record error but still return partial result
                decode_errors.insert("__data__".into(), e.to_string());
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

    /// Override default batch with Rayon parallel decode.
    fn decode_batch(
        &self,
        logs: &[RawEvent],
        registry: &dyn SchemaRegistry,
        mode: ErrorMode,
        progress: Option<&dyn ProgressCallback>,
    ) -> Result<BatchDecodeResult, BatchDecodeError> {
        // Parallel decode using Rayon; fall back to sequential for progress callbacks
        // because Rayon threads can't share the callback cleanly.
        if progress.is_some() {
            // Use the default sequential implementation when progress tracking is needed
            return chaincodec_core::decoder::ChainDecoder::decode_batch(
                self, logs, registry, mode, progress,
            );
        }

        let results: Vec<(usize, Result<DecodedEvent, DecodeError>)> = logs
            .par_iter()
            .enumerate()
            .map(|(idx, raw)| {
                let fp = self.fingerprint(raw);
                let schema = registry.get_by_fingerprint(&fp);
                match schema {
                    None => (
                        idx,
                        Err(DecodeError::SchemaNotFound {
                            fingerprint: fp.to_string(),
                        }),
                    ),
                    Some(s) => (idx, self.decode_event(raw, &s)),
                }
            })
            .collect();

        let mut events = Vec::with_capacity(logs.len());
        let mut errors = Vec::new();

        for (idx, result) in results {
            match result {
                Ok(event) => events.push(event),
                Err(err) => match mode {
                    ErrorMode::Skip => {}
                    ErrorMode::Collect => errors.push((idx, err)),
                    ErrorMode::Throw => {
                        return Err(BatchDecodeError::ItemFailed {
                            index: idx,
                            source: err,
                        });
                    }
                },
            }
        }

        Ok(BatchDecodeResult { events, errors })
    }

    fn supports_abi_guess(&self) -> bool {
        // Future: use 4byte.directory / samczsun's ABI lookup
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaincodec_core::chain::chains;

    fn erc20_transfer_raw() -> RawEvent {
        // Real ERC-20 Transfer event log data (truncated/simplified for test)
        RawEvent {
            chain: chains::ethereum(),
            tx_hash: "0xabc123".into(),
            block_number: 19_000_000,
            block_timestamp: 1_700_000_000,
            log_index: 0,
            topics: vec![
                // Transfer(address,address,uint256)
                "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
                // from (padded to 32 bytes)
                "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
                // to (padded to 32 bytes)
                "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
            ],
            // value: 1000000000000000000 (1 ETH in wei) — uint256, 32 bytes big-endian
            data: {
                let mut d = vec![0u8; 32];
                d[24..].copy_from_slice(&1_000_000_000_000_000_000u64.to_be_bytes());
                d
            },
            address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
            raw_receipt: None,
        }
    }

    #[test]
    fn evm_decoder_fingerprint() {
        let dec = EvmDecoder::new();
        let raw = erc20_transfer_raw();
        let fp = dec.fingerprint(&raw);
        assert_eq!(
            fp.as_hex(),
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }
}
