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

pub mod backpressure;
pub mod batch;
pub mod cache;
pub mod cancellation;
pub mod cu_tracker;
pub mod dedup;
pub mod error;
pub mod gas;
pub mod gas_bumper;
pub mod geo_routing;
pub mod health_checker;
pub mod hedging;
pub mod method_safety;
pub mod metrics;
pub mod mev;
pub mod multi_chain;
pub mod pending_pool;
pub mod pool;
pub mod policy;
pub mod rate_limit_headers;
pub mod reorg;
pub mod request;
pub mod routing;
pub mod selection;
pub mod shutdown;
pub mod solana;
pub mod transport;
pub mod tx;
pub mod tx_lifecycle;

pub use error::TransportError;
pub use pool::{ProviderPool, ProviderPoolConfig};
pub use request::{JsonRpcRequest, JsonRpcResponse, RpcId, RpcParam};
pub use transport::{HealthStatus, RpcTransport};
