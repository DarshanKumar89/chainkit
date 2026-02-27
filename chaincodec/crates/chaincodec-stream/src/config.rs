//! Stream engine configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a single chain's RPC connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRpcConfig {
    /// WebSocket or HTTP RPC endpoint, e.g. "wss://mainnet.infura.io/ws/v3/..."
    pub rpc_url: String,
    /// Maximum retry attempts on connection failure
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Initial backoff in milliseconds
    #[serde(default = "default_backoff_ms")]
    pub backoff_ms: u64,
    /// Optional contract addresses to filter (empty = all contracts)
    #[serde(default)]
    pub filter_addresses: Vec<String>,
}

fn default_max_retries() -> u32 { 5 }
fn default_backoff_ms() -> u64 { 500 }

/// Top-level streaming configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// chain slug → RPC config
    pub chains: HashMap<String, ChainRpcConfig>,
    /// Schema names to subscribe to (empty = all schemas in registry)
    #[serde(default)]
    pub schemas: Vec<String>,
    /// Broadcast channel capacity
    #[serde(default = "default_channel_capacity")]
    pub channel_capacity: usize,
    /// Whether to skip events with no matching schema
    #[serde(default = "bool_true")]
    pub skip_unknown: bool,
}

fn default_channel_capacity() -> usize { 1_024 }
fn bool_true() -> bool { true }

impl StreamConfig {
    /// Create a simple config for a single chain.
    pub fn single_chain(chain: impl Into<String>, rpc_url: impl Into<String>) -> Self {
        let mut chains = HashMap::new();
        chains.insert(
            chain.into(),
            ChainRpcConfig {
                rpc_url: rpc_url.into(),
                max_retries: 5,
                backoff_ms: 500,
                filter_addresses: vec![],
            },
        );
        Self {
            chains,
            schemas: vec![],
            channel_capacity: 1_024,
            skip_unknown: true,
        }
    }
}
