//! `BatchEngine` — orchestrates chunked, parallel batch decoding.

use crate::request::BatchRequest;
use chaincodec_core::{
    decoder::{ChainDecoder, ErrorMode},
    error::{BatchDecodeError, DecodeError},
    event::DecodedEvent,
    schema::SchemaRegistry,
};
use std::sync::Arc;
use tracing::{info, warn};

/// Result of a batch decode job.
pub struct BatchResult {
    /// Successfully decoded events
    pub events: Vec<DecodedEvent>,
    /// (original_index, error) pairs — only populated in Collect mode
    pub errors: Vec<(usize, DecodeError)>,
    /// Total raw events processed
    pub total_input: usize,
}

/// Batch decode engine.
pub struct BatchEngine {
    registry: Arc<dyn SchemaRegistry>,
    decoders: std::collections::HashMap<String, Arc<dyn ChainDecoder>>,
}

impl BatchEngine {
    pub fn new(registry: Arc<dyn SchemaRegistry>) -> Self {
        Self {
            registry,
            decoders: std::collections::HashMap::new(),
        }
    }

    /// Register a decoder for a given chain slug.
    pub fn add_decoder(
        &mut self,
        chain_slug: impl Into<String>,
        decoder: Arc<dyn ChainDecoder>,
    ) {
        self.decoders.insert(chain_slug.into(), decoder);
    }

    /// Execute a batch decode request.
    pub fn decode(&self, req: BatchRequest) -> Result<BatchResult, BatchDecodeError> {
        let decoder = self.decoders.get(&req.chain).ok_or_else(|| {
            BatchDecodeError::Other(format!("no decoder registered for chain '{}'", req.chain))
        })?;

        let total_input = req.logs.len();
        info!(
            "BatchEngine: decoding {} events for '{}' (chunk_size={})",
            total_input, req.chain, req.chunk_size
        );

        let mut all_events: Vec<DecodedEvent> = Vec::with_capacity(total_input);
        let mut all_errors: Vec<(usize, DecodeError)> = Vec::new();
        let mut global_offset = 0usize;
        let mut decoded_so_far = 0usize;

        for chunk in req.logs.chunks(req.chunk_size) {
            let result = decoder.decode_batch(
                chunk,
                self.registry.as_ref(),
                req.error_mode,
                req.on_progress.as_ref().map(|f| {
                    let cb: &dyn chaincodec_core::decoder::ProgressCallback = f.as_ref();
                    cb
                }),
            )?;

            decoded_so_far += result.events.len();

            if let Some(cb) = &req.on_progress {
                cb(decoded_so_far, total_input);
            }

            all_events.extend(result.events);
            for (local_idx, err) in result.errors {
                all_errors.push((global_offset + local_idx, err));
            }

            global_offset += chunk.len();
        }

        info!(
            "BatchEngine: complete — {} decoded, {} errors",
            all_events.len(),
            all_errors.len()
        );

        Ok(BatchResult {
            events: all_events,
            errors: all_errors,
            total_input,
        })
    }
}
