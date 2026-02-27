//! chainerrors-core — foundation types and traits for the ChainErrors library.
//!
//! This crate defines:
//! - [`ErrorKind`] — the taxonomy of EVM error types
//! - [`DecodedError`] — the output of a successful decode
//! - [`ErrorContext`] — chain/tx metadata for a failed call
//! - [`ErrorDecoder`] — the decoder trait every chain implements
//! - [`ErrorSignatureRegistry`] — the trait for looking up error signatures

pub mod decoder;
pub mod registry;
pub mod types;

pub use decoder::ErrorDecoder;
pub use registry::{ErrorSignature, ErrorSignatureRegistry};
pub use types::{DecodedError, ErrorContext, ErrorKind, Severity};
