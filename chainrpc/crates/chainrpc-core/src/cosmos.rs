//! Cosmos (Tendermint/CometBFT) RPC support — method safety, CU costs, and transport.
//!
//! Cosmos chains (Cosmos Hub, Osmosis, Sei, Injective, etc.) use Tendermint
//! JSON-RPC on port 26657 by default. This module adds Cosmos-specific
//! semantics on top of the generic [`RpcTransport`] trait:
//!
//! - [`classify_cosmos_method`] — safe / idempotent / unsafe for Cosmos RPC
//! - [`CosmosCuCostTable`] — per-method compute cost table
//! - [`CosmosTransport`] — wrapper that adds chain-specific configuration
//! - [`CosmosChainClient`] — [`ChainClient`] implementation for Cosmos
//! - Known public endpoints for Cosmos Hub and Osmosis

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use crate::chain_client::{ChainBlock, ChainClient};
use crate::error::TransportError;
use crate::method_safety::MethodSafety;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// Cosmos method classification
// ---------------------------------------------------------------------------

/// Classify a Cosmos/Tendermint JSON-RPC method by its safety level.
///
/// - **Safe** — read-only, retryable, cacheable.
/// - **Idempotent** — `broadcast_tx_sync` with the same signed tx always
///   produces the same tx hash.
/// - **Unsafe** — `broadcast_tx_async` fires-and-forgets, must not retry.
///
/// Unknown methods default to `Safe`.
pub fn classify_cosmos_method(method: &str) -> MethodSafety {
    if cosmos_unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if cosmos_idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

/// Returns `true` if the Cosmos method is safe to retry.
pub fn is_cosmos_safe_to_retry(method: &str) -> bool {
    classify_cosmos_method(method) == MethodSafety::Safe
}

/// Returns `true` if concurrent identical requests can be deduplicated.
pub fn is_cosmos_safe_to_dedup(method: &str) -> bool {
    classify_cosmos_method(method) == MethodSafety::Safe
}

/// Returns `true` if the result of this method can be cached.
pub fn is_cosmos_cacheable(method: &str) -> bool {
    classify_cosmos_method(method) == MethodSafety::Safe
}

fn cosmos_unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(|| {
        [
            "broadcast_tx_async",
        ]
        .into_iter()
        .collect()
    })
}

fn cosmos_idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| {
        [
            "broadcast_tx_sync",
            "broadcast_tx_commit",
        ]
        .into_iter()
        .collect()
    })
}

// ---------------------------------------------------------------------------
// CosmosCuCostTable
// ---------------------------------------------------------------------------

/// Per-method compute-unit cost table for Cosmos RPC methods.
#[derive(Debug, Clone)]
pub struct CosmosCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl CosmosCuCostTable {
    /// Create the standard Cosmos cost table with sensible defaults.
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("status", 5),
            ("health", 5),
            ("net_info", 10),
            ("block", 20),
            ("block_results", 30),
            ("blockchain", 25),
            ("commit", 15),
            ("validators", 15),
            ("genesis", 50),
            ("tx", 15),
            ("tx_search", 50),
            ("block_search", 50),
            ("abci_query", 20),
            ("broadcast_tx_sync", 10),
            ("broadcast_tx_async", 10),
            ("broadcast_tx_commit", 50),
            ("unconfirmed_txs", 20),
            ("num_unconfirmed_txs", 5),
            ("consensus_state", 10),
            ("dump_consensus_state", 30),
        ];
        for &(method, cost) in entries {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    /// Create an empty cost table with the given default cost.
    pub fn new(default_cost: u32) -> Self {
        Self {
            costs: HashMap::new(),
            default_cost,
        }
    }

    /// Set (or override) the CU cost for a specific method.
    pub fn set_cost(&mut self, method: &str, cost: u32) {
        self.costs.insert(method.to_string(), cost);
    }

    /// Return the CU cost for a method, falling back to the default.
    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for CosmosCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// Known Cosmos endpoints
// ---------------------------------------------------------------------------

/// Well-known public Cosmos Hub mainnet RPC endpoints.
pub fn cosmos_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "https://rpc.cosmos.network:26657",
        "https://cosmos-rpc.polkachu.com",
        "https://rpc-cosmoshub.blockapsis.com",
    ]
}

/// Well-known public Cosmos Hub testnet (theta) RPC endpoints.
pub fn cosmos_testnet_endpoints() -> &'static [&'static str] {
    &[
        "https://rpc.sentry-01.theta-testnet.polypore.xyz",
        "https://rpc.state-sync-01.theta-testnet.polypore.xyz",
    ]
}

/// Well-known public Osmosis mainnet RPC endpoints.
pub fn osmosis_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "https://rpc.osmosis.zone",
        "https://osmosis-rpc.polkachu.com",
    ]
}

// ---------------------------------------------------------------------------
// CosmosTransport
// ---------------------------------------------------------------------------

/// A wrapper around any [`RpcTransport`] that adds Cosmos-specific behaviour.
///
/// Cosmos/Tendermint RPC uses JSON-RPC 2.0 on port 26657 with methods like
/// `block`, `tx_search`, `broadcast_tx_sync`, etc.
pub struct CosmosTransport {
    inner: Arc<dyn RpcTransport>,
}

impl CosmosTransport {
    /// Wrap an existing transport for Cosmos RPC.
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self { inner }
    }

    /// Get a reference to the inner transport.
    pub fn inner(&self) -> &Arc<dyn RpcTransport> {
        &self.inner
    }
}

#[async_trait]
impl RpcTransport for CosmosTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        self.inner.send(req).await
    }

    async fn send_batch(
        &self,
        reqs: Vec<JsonRpcRequest>,
    ) -> Result<Vec<JsonRpcResponse>, TransportError> {
        self.inner.send_batch(reqs).await
    }

    fn health(&self) -> HealthStatus {
        self.inner.health()
    }

    fn url(&self) -> &str {
        self.inner.url()
    }
}

// ---------------------------------------------------------------------------
// CosmosChainClient
// ---------------------------------------------------------------------------

/// Cosmos implementation of [`ChainClient`].
///
/// Translates `ChainClient` methods into Tendermint JSON-RPC calls:
/// - `get_head_height()` → `status` → `sync_info.latest_block_height`
/// - `get_block_by_height(h)` → `block` with `height=h`
/// - `health_check()` → `health`
pub struct CosmosChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl CosmosChainClient {
    /// Create a Cosmos chain client.
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for CosmosChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::new(1, "status", vec![]);
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        let height_str = result["result"]["sync_info"]["latest_block_height"]
            .as_str()
            .or_else(|| result["sync_info"]["latest_block_height"].as_str())
            .unwrap_or("0");
        height_str.parse::<u64>().map_err(|e| {
            TransportError::Other(format!("invalid cosmos block height: {e}"))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        let req = JsonRpcRequest::new(
            1,
            "block",
            vec![serde_json::json!({ "height": height.to_string() })],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        // Tendermint wraps in "result" for some transports
        let block_data = if result["result"]["block"].is_object() {
            &result["result"]["block"]
        } else if result["block"].is_object() {
            &result["block"]
        } else {
            return Ok(None);
        };

        let header = &block_data["header"];
        let hash = result["result"]["block_id"]["hash"]
            .as_str()
            .or_else(|| result["block_id"]["hash"].as_str())
            .unwrap_or_default()
            .to_string();
        let parent_hash = header["last_block_id"]["hash"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        // Parse RFC3339 timestamp to unix seconds
        let time_str = header["time"].as_str().unwrap_or("");
        let timestamp = parse_rfc3339_to_unix(time_str);

        let tx_count = block_data["data"]["txs"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        Ok(Some(ChainBlock {
            height,
            hash,
            parent_hash,
            timestamp,
            tx_count,
        }))
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    fn chain_family(&self) -> &str {
        "cosmos"
    }

    async fn health_check(&self) -> Result<bool, TransportError> {
        let req = JsonRpcRequest::new(1, "health", vec![]);
        let resp = self.transport.send(req).await?;
        // Tendermint returns empty result {} on success
        let _result = resp.into_result().map_err(TransportError::Rpc)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse an RFC3339 timestamp string to Unix seconds (best-effort).
fn parse_rfc3339_to_unix(time_str: &str) -> i64 {
    // Format: "2024-01-15T12:30:45.123456789Z"
    // Simple parser — just extract date/time components
    if time_str.len() < 19 {
        return 0;
    }
    let parts: Vec<&str> = time_str.split('T').collect();
    if parts.len() != 2 {
        return 0;
    }
    let date_parts: Vec<u32> = parts[0]
        .split('-')
        .filter_map(|s| s.parse().ok())
        .collect();
    let time_part = parts[1].split('.').next().unwrap_or("").split('Z').next().unwrap_or("");
    let time_parts: Vec<u32> = time_part
        .split(':')
        .filter_map(|s| s.parse().ok())
        .collect();

    if date_parts.len() != 3 || time_parts.len() != 3 {
        return 0;
    }

    let (year, month, day) = (date_parts[0], date_parts[1], date_parts[2]);
    let (hour, minute, second) = (time_parts[0], time_parts[1], time_parts[2]);

    // Simple days-since-epoch calculation (no leap second handling)
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    let month_days = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += month_days[m as usize] as i64;
        if m == 2 && is_leap_year(year) {
            days += 1;
        }
    }
    days += (day - 1) as i64;

    days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

fn is_leap_year(y: u32) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use serde_json::Value;
    use std::sync::Mutex;

    struct MockTransport {
        url: String,
        responses: Mutex<Vec<JsonRpcResponse>>,
        recorded: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(responses: Vec<JsonRpcResponse>) -> Self {
            Self {
                url: "mock://cosmos".to_string(),
                responses: Mutex::new(responses),
                recorded: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            self.recorded.lock().unwrap().push(req.method.clone());
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(TransportError::Other("no more mock responses".into()))
            } else {
                Ok(responses.remove(0))
            }
        }

        fn url(&self) -> &str {
            &self.url
        }
    }

    fn ok_response(result: Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: RpcId::Number(1),
            result: Some(result),
            error: None,
        }
    }

    // ── Method classification ───────────────────────────────────────────

    #[test]
    fn classify_safe_methods() {
        assert_eq!(classify_cosmos_method("block"), MethodSafety::Safe);
        assert_eq!(classify_cosmos_method("block_results"), MethodSafety::Safe);
        assert_eq!(classify_cosmos_method("validators"), MethodSafety::Safe);
        assert_eq!(classify_cosmos_method("status"), MethodSafety::Safe);
        assert_eq!(classify_cosmos_method("tx_search"), MethodSafety::Safe);
        assert_eq!(classify_cosmos_method("abci_query"), MethodSafety::Safe);
    }

    #[test]
    fn classify_idempotent_methods() {
        assert_eq!(
            classify_cosmos_method("broadcast_tx_sync"),
            MethodSafety::Idempotent
        );
        assert_eq!(
            classify_cosmos_method("broadcast_tx_commit"),
            MethodSafety::Idempotent
        );
    }

    #[test]
    fn classify_unsafe_methods() {
        assert_eq!(
            classify_cosmos_method("broadcast_tx_async"),
            MethodSafety::Unsafe
        );
    }

    #[test]
    fn unknown_method_defaults_safe() {
        assert_eq!(
            classify_cosmos_method("some_future_method"),
            MethodSafety::Safe
        );
    }

    // ── CU cost table ───────────────────────────────────────────────────

    #[test]
    fn cu_cost_defaults() {
        let table = CosmosCuCostTable::defaults();
        assert_eq!(table.cost_for("status"), 5);
        assert_eq!(table.cost_for("block"), 20);
        assert_eq!(table.cost_for("tx_search"), 50);
        assert_eq!(table.cost_for("unknown_method"), 15); // default
    }

    #[test]
    fn cu_cost_custom() {
        let mut table = CosmosCuCostTable::new(10);
        table.set_cost("block", 100);
        assert_eq!(table.cost_for("block"), 100);
        assert_eq!(table.cost_for("status"), 10);
    }

    // ── Helper booleans ─────────────────────────────────────────────────

    #[test]
    fn retry_dedup_cache_helpers() {
        assert!(is_cosmos_safe_to_retry("block"));
        assert!(!is_cosmos_safe_to_retry("broadcast_tx_async"));
        assert!(is_cosmos_safe_to_dedup("status"));
        assert!(!is_cosmos_safe_to_dedup("broadcast_tx_sync"));
        assert!(is_cosmos_cacheable("tx_search"));
        assert!(!is_cosmos_cacheable("broadcast_tx_commit"));
    }

    // ── Endpoints ───────────────────────────────────────────────────────

    #[test]
    fn endpoints_not_empty() {
        assert!(!cosmos_mainnet_endpoints().is_empty());
        assert!(!cosmos_testnet_endpoints().is_empty());
        assert!(!osmosis_mainnet_endpoints().is_empty());
    }

    // ── RFC3339 parser ──────────────────────────────────────────────────

    #[test]
    fn parse_rfc3339() {
        // 2024-01-01T00:00:00Z = 1704067200
        let ts = parse_rfc3339_to_unix("2024-01-01T00:00:00Z");
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn parse_rfc3339_with_nanos() {
        let ts = parse_rfc3339_to_unix("2024-01-01T00:00:00.123456789Z");
        assert_eq!(ts, 1704067200);
    }

    #[test]
    fn parse_rfc3339_invalid() {
        assert_eq!(parse_rfc3339_to_unix("invalid"), 0);
        assert_eq!(parse_rfc3339_to_unix(""), 0);
    }

    // ── CosmosChainClient ───────────────────────────────────────────────

    #[tokio::test]
    async fn cosmos_get_head_height() {
        let transport = Arc::new(MockTransport::new(vec![ok_response(serde_json::json!({
            "result": {
                "sync_info": {
                    "latest_block_height": "19500000"
                }
            }
        }))]));
        let client = CosmosChainClient::new(transport, "cosmoshub-4");
        let height = client.get_head_height().await.unwrap();
        assert_eq!(height, 19500000);
    }

    #[tokio::test]
    async fn cosmos_get_block() {
        let transport = Arc::new(MockTransport::new(vec![ok_response(serde_json::json!({
            "result": {
                "block_id": {
                    "hash": "ABC123DEF"
                },
                "block": {
                    "header": {
                        "height": "100",
                        "time": "2024-01-01T00:00:00Z",
                        "last_block_id": {
                            "hash": "PARENT_HASH"
                        }
                    },
                    "data": {
                        "txs": ["tx1", "tx2"]
                    }
                }
            }
        }))]));
        let client = CosmosChainClient::new(transport, "cosmoshub-4");
        let block = client.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(block.height, 100);
        assert_eq!(block.hash, "ABC123DEF");
        assert_eq!(block.parent_hash, "PARENT_HASH");
        assert_eq!(block.tx_count, 2);
        assert_eq!(block.timestamp, 1704067200);
    }

    #[tokio::test]
    async fn cosmos_health_check() {
        let transport = Arc::new(MockTransport::new(vec![ok_response(
            serde_json::json!({}),
        )]));
        let client = CosmosChainClient::new(transport, "cosmoshub-4");
        assert!(client.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn cosmos_chain_metadata() {
        let transport = Arc::new(MockTransport::new(vec![]));
        let client = CosmosChainClient::new(transport, "osmosis-1");
        assert_eq!(client.chain_id(), "osmosis-1");
        assert_eq!(client.chain_family(), "cosmos");
    }

    // ── CosmosTransport ─────────────────────────────────────────────────

    #[tokio::test]
    async fn cosmos_transport_delegates() {
        let inner = Arc::new(MockTransport::new(vec![ok_response(
            serde_json::json!("ok"),
        )]));
        let transport = CosmosTransport::new(inner.clone());
        assert_eq!(transport.url(), "mock://cosmos");

        let req = JsonRpcRequest::new(1, "health", vec![]);
        let resp = transport.send(req).await.unwrap();
        assert!(resp.is_ok());
        assert_eq!(inner.recorded.lock().unwrap().len(), 1);
    }
}
