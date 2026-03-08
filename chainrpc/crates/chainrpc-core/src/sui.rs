//! Sui RPC support — method safety, CU costs, and transport.
//!
//! Sui uses JSON-RPC directly. Sui organizes data around checkpoints
//! (finalized) and objects rather than traditional blocks.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use crate::chain_client::{ChainBlock, ChainClient};
use crate::error::TransportError;
use crate::method_safety::MethodSafety;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// Method classification
// ---------------------------------------------------------------------------

pub fn classify_sui_method(method: &str) -> MethodSafety {
    if sui_unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if sui_idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

pub fn is_sui_safe_to_retry(method: &str) -> bool {
    classify_sui_method(method) == MethodSafety::Safe
}

pub fn is_sui_safe_to_dedup(method: &str) -> bool {
    classify_sui_method(method) == MethodSafety::Safe
}

pub fn is_sui_cacheable(method: &str) -> bool {
    classify_sui_method(method) == MethodSafety::Safe
}

fn sui_unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(HashSet::new)
}

fn sui_idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| {
        [
            "sui_executeTransactionBlock",
            "sui_dryRunTransactionBlock",
        ]
        .into_iter()
        .collect()
    })
}

// ---------------------------------------------------------------------------
// CU cost table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SuiCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl SuiCuCostTable {
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("sui_getLatestCheckpointSequenceNumber", 5),
            ("sui_getCheckpoint", 20),
            ("sui_getObject", 10),
            ("sui_multiGetObjects", 30),
            ("sui_getTransactionBlock", 15),
            ("sui_multiGetTransactionBlocks", 30),
            ("sui_getEvents", 20),
            ("sui_getTotalTransactionBlocks", 5),
            ("sui_executeTransactionBlock", 10),
            ("sui_dryRunTransactionBlock", 30),
            ("suix_getOwnedObjects", 20),
            ("suix_getCoins", 15),
            ("suix_getAllBalances", 10),
            ("suix_getReferenceGasPrice", 5),
        ];
        for &(method, cost) in entries {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    pub fn new(default_cost: u32) -> Self {
        Self { costs: HashMap::new(), default_cost }
    }

    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for SuiCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// Known endpoints
// ---------------------------------------------------------------------------

pub fn sui_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.mainnet.sui.io:443",
        "https://sui-mainnet.nodereal.io",
    ]
}

pub fn sui_testnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.testnet.sui.io:443",
    ]
}

pub fn sui_devnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.devnet.sui.io:443",
    ]
}

// ---------------------------------------------------------------------------
// SuiTransport
// ---------------------------------------------------------------------------

pub struct SuiTransport {
    inner: Arc<dyn RpcTransport>,
}

impl SuiTransport {
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn RpcTransport> {
        &self.inner
    }
}

#[async_trait]
impl RpcTransport for SuiTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        self.inner.send(req).await
    }

    async fn send_batch(&self, reqs: Vec<JsonRpcRequest>) -> Result<Vec<JsonRpcResponse>, TransportError> {
        self.inner.send_batch(reqs).await
    }

    fn health(&self) -> HealthStatus { self.inner.health() }
    fn url(&self) -> &str { self.inner.url() }
}

// ---------------------------------------------------------------------------
// SuiChainClient
// ---------------------------------------------------------------------------

/// Sui implementation of [`ChainClient`].
///
/// Maps `height` to Sui checkpoint sequence numbers.
pub struct SuiChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl SuiChainClient {
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for SuiChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::new(
            1,
            "sui_getLatestCheckpointSequenceNumber",
            vec![],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        // Sui returns checkpoint number as a string
        let cp_str = result.as_str().unwrap_or("0");
        cp_str.parse::<u64>().map_err(|e| {
            TransportError::Other(format!("invalid sui checkpoint number: {e}"))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        let req = JsonRpcRequest::new(
            1,
            "sui_getCheckpoint",
            vec![serde_json::Value::String(height.to_string())],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        if result.is_null() {
            return Ok(None);
        }

        let hash = result["digest"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let parent_hash = result["previousDigest"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = result["timestampMs"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|ms| (ms / 1000) as i64)
            .unwrap_or(0);
        let tx_count = result["transactions"]
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
        "sui"
    }

    async fn health_check(&self) -> Result<bool, TransportError> {
        self.get_head_height().await.map(|_| true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use serde_json::Value;
    use std::sync::Mutex;

    struct MockTransport {
        url: String,
        responses: Mutex<Vec<JsonRpcResponse>>,
    }

    impl MockTransport {
        fn new(responses: Vec<JsonRpcResponse>) -> Self {
            Self { url: "mock://sui".into(), responses: Mutex::new(responses) }
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            let mut r = self.responses.lock().unwrap();
            if r.is_empty() { Err(TransportError::Other("no mock".into())) }
            else { Ok(r.remove(0)) }
        }
        fn url(&self) -> &str { &self.url }
    }

    fn ok(result: Value) -> JsonRpcResponse {
        JsonRpcResponse { jsonrpc: "2.0".into(), id: RpcId::Number(1), result: Some(result), error: None }
    }

    #[test]
    fn classify() {
        assert_eq!(classify_sui_method("sui_getCheckpoint"), MethodSafety::Safe);
        assert_eq!(classify_sui_method("sui_getObject"), MethodSafety::Safe);
        assert_eq!(classify_sui_method("sui_executeTransactionBlock"), MethodSafety::Idempotent);
    }

    #[test]
    fn cu_costs() {
        let t = SuiCuCostTable::defaults();
        assert_eq!(t.cost_for("sui_getLatestCheckpointSequenceNumber"), 5);
        assert_eq!(t.cost_for("sui_getCheckpoint"), 20);
    }

    #[test]
    fn endpoints() {
        assert!(!sui_mainnet_endpoints().is_empty());
        assert!(!sui_testnet_endpoints().is_empty());
    }

    #[tokio::test]
    async fn sui_get_head_height() {
        let t = Arc::new(MockTransport::new(vec![ok(serde_json::Value::String("50000000".into()))]));
        let c = SuiChainClient::new(t, "mainnet");
        assert_eq!(c.get_head_height().await.unwrap(), 50000000);
    }

    #[tokio::test]
    async fn sui_get_checkpoint() {
        let t = Arc::new(MockTransport::new(vec![ok(serde_json::json!({
            "digest": "checkpoint_digest_abc",
            "previousDigest": "checkpoint_digest_prev",
            "timestampMs": "1700000000000",
            "transactions": ["tx1", "tx2", "tx3"]
        }))]));
        let c = SuiChainClient::new(t, "mainnet");
        let b = c.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(b.hash, "checkpoint_digest_abc");
        assert_eq!(b.parent_hash, "checkpoint_digest_prev");
        assert_eq!(b.timestamp, 1700000000);
        assert_eq!(b.tx_count, 3);
    }

    #[tokio::test]
    async fn sui_metadata() {
        let t = Arc::new(MockTransport::new(vec![]));
        let c = SuiChainClient::new(t, "testnet");
        assert_eq!(c.chain_family(), "sui");
        assert_eq!(c.chain_id(), "testnet");
    }
}
