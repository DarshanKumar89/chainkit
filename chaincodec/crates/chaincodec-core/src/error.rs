//! Error types for the ChainCodec decode pipeline.

use thiserror::Error;

/// Errors that can occur while decoding a single event.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Schema not found for fingerprint {fingerprint}")]
    SchemaNotFound { fingerprint: String },

    #[error("ABI decode failed: {reason}")]
    AbiDecodeFailed { reason: String },

    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },

    #[error("Unsupported chain: {chain}")]
    UnsupportedChain { chain: String },

    #[error("Invalid raw event: {reason}")]
    InvalidRawEvent { reason: String },

    #[error("Missing required field: {field}")]
    MissingField { field: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Unknown error: {0}")]
    Other(String),
}

/// Errors that can occur during batch decoding.
#[derive(Debug, Error)]
pub enum BatchDecodeError {
    #[error("Batch aborted after {count} errors")]
    TooManyErrors { count: usize },

    #[error("Decode error at index {index}: {source}")]
    ItemFailed {
        index: usize,
        #[source]
        source: DecodeError,
    },

    #[error("Memory limit exceeded: tried to allocate {bytes} bytes")]
    MemoryLimitExceeded { bytes: usize },

    #[error("{0}")]
    Other(String),
}

/// Errors from the schema registry.
#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("Schema '{name}' v{version} already exists")]
    AlreadyExists { name: String, version: u32 },

    #[error("Schema '{name}' not found")]
    NotFound { name: String },

    #[error("Schema validation failed: {reason}")]
    ValidationFailed { reason: String },

    #[error("Fingerprint mismatch: claimed {claimed}, computed {computed}")]
    FingerprintMismatch { claimed: String, computed: String },

    #[error("Parse error in CSDL: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),
}

/// Errors from the streaming engine.
#[derive(Debug, Error)]
pub enum StreamError {
    #[error("RPC connection failed: {url}: {reason}")]
    ConnectionFailed { url: String, reason: String },

    #[error("Stream closed unexpectedly")]
    Closed,

    #[error("Subscription timeout after {ms}ms")]
    Timeout { ms: u64 },

    #[error("Decode error in stream: {0}")]
    Decode(#[from] DecodeError),

    #[error("{0}")]
    Other(String),
}
