//! Rayon-powered batch decode helpers specific to the EVM decoder.
//! The main batch logic lives in `EvmDecoder::decode_batch`, but this module
//! exposes chunk-level utilities for the higher-level batch engine.

use chaincodec_core::{
    decoder::ErrorMode,
    error::DecodeError,
    event::{DecodedEvent, RawEvent},
    schema::SchemaRegistry,
};
use rayon::prelude::*;

use crate::decoder::EvmDecoder;
use chaincodec_core::decoder::ChainDecoder;

/// Decode a slice of EVM raw events in parallel using Rayon.
/// Returns `(successes, errors)`.
pub fn parallel_decode(
    decoder: &EvmDecoder,
    logs: &[RawEvent],
    registry: &dyn SchemaRegistry,
) -> (Vec<DecodedEvent>, Vec<(usize, DecodeError)>) {
    let results: Vec<(usize, Result<DecodedEvent, DecodeError>)> = logs
        .par_iter()
        .enumerate()
        .map(|(idx, raw)| {
            let fp = decoder.fingerprint(raw);
            match registry.get_by_fingerprint(&fp) {
                None => (
                    idx,
                    Err(DecodeError::SchemaNotFound {
                        fingerprint: fp.to_string(),
                    }),
                ),
                Some(schema) => (idx, decoder.decode_event(raw, &schema)),
            }
        })
        .collect();

    let mut events = Vec::new();
    let mut errors = Vec::new();
    for (idx, r) in results {
        match r {
            Ok(e) => events.push(e),
            Err(e) => errors.push((idx, e)),
        }
    }
    (events, errors)
}

/// Chunk `logs` into slices of at most `chunk_size` and decode each chunk
/// in parallel. Returns a flat list of successes and errors.
pub fn chunked_decode(
    decoder: &EvmDecoder,
    logs: &[RawEvent],
    registry: &dyn SchemaRegistry,
    chunk_size: usize,
) -> (Vec<DecodedEvent>, Vec<(usize, DecodeError)>) {
    let mut all_events = Vec::new();
    let mut all_errors = Vec::new();
    let mut offset = 0;

    for chunk in logs.chunks(chunk_size) {
        let (mut evts, errs) = parallel_decode(decoder, chunk, registry);
        all_events.append(&mut evts);
        // Adjust error indices relative to overall slice
        for (idx, err) in errs {
            all_errors.push((offset + idx, err));
        }
        offset += chunk.len();
    }

    (all_events, all_errors)
}
