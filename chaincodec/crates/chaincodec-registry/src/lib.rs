//! # chaincodec-registry
//!
//! Schema Registry for ChainCodec.
//!
//! ## Levels
//! 1. **In-Memory Registry** — fast, ephemeral; loaded from CSDL files or JSON
//! 2. **File-Backed Registry** — loads CSDL files from a directory at startup
//! 3. **Remote Registry** (future) — HTTP client for registry.chaincodec.io
//!
//! The public-facing API is the `SchemaRegistry` trait from `chaincodec-core`.

pub mod csdl;
pub mod memory;
#[cfg(feature = "remote")]
pub mod remote;
#[cfg(all(feature = "sqlite", not(target_arch = "wasm32")))]
pub mod sqlite;

pub use csdl::CsdlParser;
pub use memory::MemoryRegistry;

#[cfg(feature = "remote")]
pub use remote::AbiFetcher;

#[cfg(all(feature = "sqlite", not(target_arch = "wasm32")))]
pub use sqlite::SqliteRegistry;
