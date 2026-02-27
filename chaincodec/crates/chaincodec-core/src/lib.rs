//! # chaincodec-core
//!
//! Core traits, types, and primitives shared across all ChainCodec crates.
//! Every chain decoder, registry, and streaming engine is built on top of
//! the interfaces defined here.

pub mod call;
pub mod chain;
pub mod decoder;
pub mod error;
pub mod event;
pub mod schema;
pub mod types;

pub use call::{DecodedCall, DecodedConstructor, HumanReadable};
pub use chain::{ChainFamily, ChainId};
pub use decoder::{BatchDecodeError, ChainDecoder, DecodeError, ProgressCallback};
pub use event::{DecodedEvent, EventFingerprint, RawEvent};
pub use schema::{Schema, SchemaRegistry};
pub use types::{CanonicalType, NormalizedValue};
