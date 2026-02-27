//! Error types for the chainindex pipeline.

use thiserror::Error;

/// Errors that can occur during indexing.
#[derive(Debug, Error)]
pub enum IndexerError {
    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Handler error in '{handler}': {reason}")]
    Handler { handler: String, reason: String },

    #[error("Reorg detected at block {block_number}: expected hash {expected}, got {actual}")]
    ReorgDetected {
        block_number: u64,
        expected: String,
        actual: String,
    },

    #[error("Checkpoint mismatch at block {block_number}")]
    CheckpointMismatch { block_number: u64 },

    #[error("Indexer aborted: {reason}")]
    Aborted { reason: String },

    #[error("{0}")]
    Other(String),
}

impl IndexerError {
    /// Returns `true` if the error is a reorg (recoverable).
    pub fn is_reorg(&self) -> bool {
        matches!(self, Self::ReorgDetected { .. })
    }
}
