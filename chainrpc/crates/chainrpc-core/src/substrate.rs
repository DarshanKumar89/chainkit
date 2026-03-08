//! Substrate (Polkadot/Kusama) RPC support — method safety, CU costs, and transport.
//!
//! Substrate chains use JSON-RPC on port 9944. Methods follow the `module_method`
//! naming convention (e.g., `chain_getBlock`, `state_getStorage`).

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

/// Classify a Substrate JSON-RPC method by its safety level.
pub fn classify_substrate_method(method: &str) -> MethodSafety {
    if substrate_unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if substrate_idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

pub fn is_substrate_safe_to_retry(method: &str) -> bool {
    classify_substrate_method(method) == MethodSafety::Safe
}

pub fn is_substrate_safe_to_dedup(method: &str) -> bool {
    classify_substrate_method(method) == MethodSafety::Safe
}

pub fn is_substrate_cacheable(method: &str) -> bool {
    classify_substrate_method(method) == MethodSafety::Safe
}

fn substrate_unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(HashSet::new) // no fire-and-forget methods in Substrate
}

fn substrate_idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| {
        ["author_submitExtrinsic", "author_submitAndWatchExtrinsic"]
            .into_iter()
            .collect()
    })
}

// ---------------------------------------------------------------------------
// CU cost table
// ---------------------------------------------------------------------------

/// Per-method compute-unit cost table for Substrate RPC.
#[derive(Debug, Clone)]
pub struct SubstrateCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl SubstrateCuCostTable {
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("chain_getBlock", 20),
            ("chain_getBlockHash", 5),
            ("chain_getHeader", 10),
            ("chain_getFinalizedHead", 5),
            ("state_getStorage", 15),
            ("state_getMetadata", 50),
            ("state_getRuntimeVersion", 10),
            ("state_queryStorageAt", 30),
            ("system_chain", 5),
            ("system_health", 5),
            ("system_peers", 10),
            ("system_properties", 5),
            ("author_submitExtrinsic", 10),
        ];
        for &(method, cost) in entries {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    pub fn new(default_cost: u32) -> Self {
        Self {
            costs: HashMap::new(),
            default_cost,
        }
    }

    pub fn set_cost(&mut self, method: &str, cost: u32) {
        self.costs.insert(method.to_string(), cost);
    }

    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for SubstrateCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// Known endpoints
// ---------------------------------------------------------------------------

pub fn polkadot_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "wss://rpc.polkadot.io",
        "wss://polkadot.api.onfinality.io/public-ws",
    ]
}

pub fn kusama_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "wss://kusama-rpc.polkadot.io",
        "wss://kusama.api.onfinality.io/public-ws",
    ]
}

// ---------------------------------------------------------------------------
// SubstrateTransport
// ---------------------------------------------------------------------------

/// Substrate RPC transport wrapper.
pub struct SubstrateTransport {
    inner: Arc<dyn RpcTransport>,
}

impl SubstrateTransport {
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn RpcTransport> {
        &self.inner
    }
}

#[async_trait]
impl RpcTransport for SubstrateTransport {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        self.inner.send(req).await
    }

    async fn send_batch(&self, reqs: Vec<JsonRpcRequest>) -> Result<Vec<JsonRpcResponse>, TransportError> {
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
// SubstrateChainClient
// ---------------------------------------------------------------------------

/// Substrate implementation of [`ChainClient`].
pub struct SubstrateChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl SubstrateChainClient {
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for SubstrateChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::new(1, "chain_getHeader", vec![]);
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        let number_hex = result["number"]
            .as_str()
            .unwrap_or("0x0");
        let stripped = number_hex.strip_prefix("0x").unwrap_or(number_hex);
        u64::from_str_radix(stripped, 16).map_err(|e| {
            TransportError::Other(format!("invalid substrate block number: {e}"))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        // First get hash for height
        let hash_req = JsonRpcRequest::new(
            1,
            "chain_getBlockHash",
            vec![serde_json::json!(height)],
        );
        let hash_resp = self.transport.send(hash_req).await?;
        let hash_result = hash_resp.into_result().map_err(TransportError::Rpc)?;

        let block_hash = match hash_result.as_str() {
            Some(h) if !h.is_empty() => h.to_string(),
            _ => return Ok(None),
        };

        // Then get block by hash
        let block_req = JsonRpcRequest::new(
            1,
            "chain_getBlock",
            vec![serde_json::Value::String(block_hash.clone())],
        );
        let block_resp = self.transport.send(block_req).await?;
        let block_result = block_resp.into_result().map_err(TransportError::Rpc)?;

        if block_result.is_null() {
            return Ok(None);
        }

        let header = &block_result["block"]["header"];
        let parent_hash = header["parentHash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let tx_count = block_result["block"]["extrinsics"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        Ok(Some(ChainBlock {
            height,
            hash: block_hash,
            parent_hash,
            timestamp: 0, // Substrate doesn't include timestamp in block header directly
            tx_count,
        }))
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    fn chain_family(&self) -> &str {
        "substrate"
    }

    async fn health_check(&self) -> Result<bool, TransportError> {
        let req = JsonRpcRequest::new(1, "system_health", vec![]);
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        // system_health returns { peers, isSyncing, shouldHavePeers }
        Ok(!result["isSyncing"].as_bool().unwrap_or(true))
    }
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
                url: "mock://substrate".to_string(),
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
                Err(TransportError::Other("no mock responses".into()))
            } else {
                Ok(responses.remove(0))
            }
        }
        fn url(&self) -> &str { &self.url }
    }

    fn ok_response(result: Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: RpcId::Number(1),
            result: Some(result),
            error: None,
        }
    }

    #[test]
    fn classify_methods() {
        assert_eq!(classify_substrate_method("chain_getBlock"), MethodSafety::Safe);
        assert_eq!(classify_substrate_method("state_getStorage"), MethodSafety::Safe);
        assert_eq!(classify_substrate_method("author_submitExtrinsic"), MethodSafety::Idempotent);
        assert_eq!(classify_substrate_method("unknown"), MethodSafety::Safe);
    }

    #[test]
    fn cu_costs() {
        let table = SubstrateCuCostTable::defaults();
        assert_eq!(table.cost_for("chain_getBlock"), 20);
        assert_eq!(table.cost_for("system_health"), 5);
        assert_eq!(table.cost_for("unknown"), 15);
    }

    #[test]
    fn endpoints() {
        assert!(!polkadot_mainnet_endpoints().is_empty());
        assert!(!kusama_mainnet_endpoints().is_empty());
    }

    #[tokio::test]
    async fn substrate_get_head_height() {
        let transport = Arc::new(MockTransport::new(vec![ok_response(serde_json::json!({
            "number": "0x1234",
            "parentHash": "0xabc"
        }))]));
        let client = SubstrateChainClient::new(transport, "polkadot");
        let height = client.get_head_height().await.unwrap();
        assert_eq!(height, 0x1234);
    }

    #[tokio::test]
    async fn substrate_get_block() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::String("0xblock_hash".to_string())),
            ok_response(serde_json::json!({
                "block": {
                    "header": {
                        "number": "0x64",
                        "parentHash": "0xparent"
                    },
                    "extrinsics": ["ext1", "ext2", "ext3"]
                }
            })),
        ]));
        let client = SubstrateChainClient::new(transport, "polkadot");
        let block = client.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(block.height, 100);
        assert_eq!(block.hash, "0xblock_hash");
        assert_eq!(block.parent_hash, "0xparent");
        assert_eq!(block.tx_count, 3);
    }

    #[tokio::test]
    async fn substrate_health_check() {
        let transport = Arc::new(MockTransport::new(vec![ok_response(serde_json::json!({
            "peers": 10,
            "isSyncing": false,
            "shouldHavePeers": true
        }))]));
        let client = SubstrateChainClient::new(transport, "polkadot");
        assert!(client.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn substrate_metadata() {
        let transport = Arc::new(MockTransport::new(vec![]));
        let client = SubstrateChainClient::new(transport, "kusama");
        assert_eq!(client.chain_id(), "kusama");
        assert_eq!(client.chain_family(), "substrate");
    }
}
