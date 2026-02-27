//! # chaincodec-stream
//!
//! Real-time streaming engine for ChainCodec.
//!
//! Connects to blockchain RPC/WebSocket endpoints, listens for new blocks,
//! extracts raw log data, routes each log through the Schema Registry,
//! decodes it, and emits strongly-typed `DecodedEvent` objects.
//!
//! ## Architecture
//! ```text
//! BlockListener (per chain, Tokio task)
//!       │
//!       ▼
//! RawEvent queue
//!       │
//!       ▼
//! SchemaRouter (fingerprint → schema lookup)
//!       │
//!       ▼
//! ChainDecoder::decode_event
//!       │
//!       ▼
//! broadcast::Sender<DecodedEvent>   ← multiple consumers subscribe
//! ```

pub mod config;
pub mod engine;
pub mod listener;
pub mod ws_listener;

pub use config::StreamConfig;
pub use engine::StreamEngine;
pub use ws_listener::EvmWsListener;
