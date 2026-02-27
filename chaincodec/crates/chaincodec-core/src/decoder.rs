//! The core `ChainDecoder` trait and associated progress/batch types.
//!
//! Every chain-specific decoder (EVM, Solana, Cosmos, etc.) implements
//! `ChainDecoder`. The trait is object-safe so decoders can be stored as
//! `Arc<dyn ChainDecoder>` in the streaming and batch engines.

use crate::chain::ChainFamily;
use crate::error::{BatchDecodeError, DecodeError};
use crate::event::{DecodedEvent, EventFingerprint, RawEvent};
use crate::schema::SchemaRegistry;

/// Callback invoked by the batch engine during long-running decodes.
/// `decoded` is the number of events successfully decoded so far;
/// `total` is the total count in the current batch.
pub trait ProgressCallback: Send + Sync {
    fn on_progress(&self, decoded: usize, total: usize);
}

/// Blanket impl so closures can be used as progress callbacks.
impl<F: Fn(usize, usize) + Send + Sync> ProgressCallback for F {
    fn on_progress(&self, decoded: usize, total: usize) {
        self(decoded, total)
    }
}

/// Controls how the batch engine reacts to individual decode failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorMode {
    /// Silently skip events that fail to decode. Suitable for best-effort analytics.
    #[default]
    Skip,
    /// Collect decode errors alongside successes and return both at the end.
    Collect,
    /// Abort the entire batch on first error.
    Throw,
}

/// The output of a batch decode: successful events plus any collected errors.
#[derive(Debug)]
pub struct BatchDecodeResult {
    pub events: Vec<DecodedEvent>,
    /// Populated only when `ErrorMode::Collect` is used.
    pub errors: Vec<(usize, DecodeError)>,
}

/// The central trait every chain-specific decoder must implement.
///
/// # Thread Safety
/// Implementations must be `Send + Sync` so they can be shared across
/// Tokio tasks and Rayon threads without additional locking.
pub trait ChainDecoder: Send + Sync {
    /// Returns the chain family this decoder handles.
    fn chain_family(&self) -> ChainFamily;

    /// Compute the event fingerprint from a raw event.
    /// For EVM this is `topics[0]`; for Solana it's a discriminator hash, etc.
    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint;

    /// Decode a single raw event using the provided schema.
    fn decode_event(
        &self,
        raw: &RawEvent,
        schema: &crate::schema::Schema,
    ) -> Result<DecodedEvent, DecodeError>;

    /// Decode a batch of raw events.
    ///
    /// The default implementation calls `decode_event` for each log, but
    /// chain-specific crates can override this for parallelism (Rayon) or
    /// other optimizations.
    fn decode_batch(
        &self,
        logs: &[RawEvent],
        registry: &dyn SchemaRegistry,
        mode: ErrorMode,
        progress: Option<&dyn ProgressCallback>,
    ) -> Result<BatchDecodeResult, BatchDecodeError> {
        let mut events = Vec::with_capacity(logs.len());
        let mut errors = Vec::new();

        for (idx, raw) in logs.iter().enumerate() {
            let fp = self.fingerprint(raw);
            let schema = match registry.get_by_fingerprint(&fp) {
                Some(s) => s,
                None => {
                    let err = DecodeError::SchemaNotFound {
                        fingerprint: fp.to_string(),
                    };
                    match mode {
                        ErrorMode::Skip => {
                            if let Some(cb) = progress {
                                cb.on_progress(events.len(), logs.len());
                            }
                            continue;
                        }
                        ErrorMode::Collect => {
                            errors.push((idx, err));
                            if let Some(cb) = progress {
                                cb.on_progress(events.len(), logs.len());
                            }
                            continue;
                        }
                        ErrorMode::Throw => {
                            return Err(BatchDecodeError::ItemFailed {
                                index: idx,
                                source: err,
                            });
                        }
                    }
                }
            };

            match self.decode_event(raw, &schema) {
                Ok(event) => {
                    events.push(event);
                }
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

            if let Some(cb) = progress {
                cb.on_progress(events.len(), logs.len());
            }
        }

        Ok(BatchDecodeResult { events, errors })
    }

    /// Whether this decoder can attempt to guess/auto-detect a schema
    /// from raw bytes when no schema is found in the registry.
    fn supports_abi_guess(&self) -> bool {
        false
    }
}
