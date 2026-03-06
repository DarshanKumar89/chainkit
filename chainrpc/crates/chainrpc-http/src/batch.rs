//! Auto-batching engine — re-exported from `chainrpc-core`.
//!
//! The batching transport is transport-agnostic and now lives in `chainrpc_core::batch`.
//! This module re-exports it for backward compatibility.

pub use chainrpc_core::batch::BatchingTransport;
