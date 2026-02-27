//! chainrpc-providers — Pre-configured provider profiles for major RPC providers.
//!
//! Each provider module knows the URL template, rate limits (compute units),
//! and supported chain IDs for a specific RPC service.
//!
//! # Quick start
//! ```rust,no_run
//! use chainrpc_providers::alchemy;
//! use std::sync::Arc;
//!
//! let client = alchemy::http_client("YOUR_API_KEY", 1); // Ethereum mainnet
//! ```

pub mod alchemy;
pub mod infura;
pub mod public;
pub mod quicknode;

/// Compute unit costs for common Ethereum JSON-RPC methods.
/// Used by provider profiles to configure rate limiters.
pub const ETH_METHOD_COSTS: &[(&str, u32)] = &[
    ("eth_blockNumber", 10),
    ("eth_getBalance", 19),
    ("eth_getTransactionCount", 26),
    ("eth_call", 26),
    ("eth_estimateGas", 87),
    ("eth_sendRawTransaction", 250),
    ("eth_getTransactionReceipt", 15),
    ("eth_getBlockByNumber", 16),
    ("eth_getLogs", 75),
    ("eth_subscribe", 10),
    ("eth_getCode", 19),
    ("eth_getStorageAt", 17),
];
