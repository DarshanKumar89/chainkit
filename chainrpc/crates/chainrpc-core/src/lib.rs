//! chainrpc-core — foundation traits and types for ChainRPC.
//!
//! # Overview
//!
//! ChainRPC provides a production-grade, multi-provider RPC transport layer
//! for EVM (and other) blockchains. The core crate defines:
//!
//! - [`RpcTransport`] — the central async trait every transport implements
//! - [`JsonRpcRequest`] / [`JsonRpcResponse`] — wire types
//! - [`TransportError`] — structured error type
//! - [`HealthStatus`] — provider liveness check
//! - [`policy`] module — retry, circuit breaker, rate limiter
//! - [`pool`] module — multi-provider failover pool

pub mod error;
pub mod pool;
pub mod policy;
pub mod request;
pub mod transport;

pub use error::TransportError;
pub use pool::{ProviderPool, ProviderPoolConfig};
pub use request::{JsonRpcRequest, JsonRpcResponse, RpcId, RpcParam};
pub use transport::{HealthStatus, RpcTransport};
