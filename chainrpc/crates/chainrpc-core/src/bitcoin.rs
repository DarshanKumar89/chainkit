//! Bitcoin RPC support — method safety, CU costs, and transport.
//!
//! Bitcoin Core uses JSON-RPC on port 8332 with HTTP Basic Auth.
//! This module adds Bitcoin-specific semantics.

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

/// Classify a Bitcoin JSON-RPC method by its safety level.
pub fn classify_bitcoin_method(method: &str) -> MethodSafety {
    if bitcoin_unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if bitcoin_idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

pub fn is_bitcoin_safe_to_retry(method: &str) -> bool {
    classify_bitcoin_method(method) == MethodSafety::Safe
}

pub fn is_bitcoin_safe_to_dedup(method: &str) -> bool {
    classify_bitcoin_method(method) == MethodSafety::Safe
}

pub fn is_bitcoin_cacheable(method: &str) -> bool {
    classify_bitcoin_method(method) == MethodSafety::Safe
}

fn bitcoin_unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(|| {
        [
            "walletpassphrase",
            "encryptwallet",
            "backupwallet",
            "importprivkey",
        ]
        .into_iter()
        .collect()
    })
}

fn bitcoin_idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| {
        ["sendrawtransaction"].into_iter().collect()
    })
}

// ---------------------------------------------------------------------------
// CU cost table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BitcoinCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl BitcoinCuCostTable {
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("getblockcount", 5),
            ("getbestblockhash", 5),
            ("getblockhash", 5),
            ("getblock", 20),
            ("getblockheader", 10),
            ("getrawtransaction", 15),
            ("gettxout", 10),
            ("getmempoolinfo", 5),
            ("getrawmempool", 20),
            ("getnetworkinfo", 5),
            ("getblockchaininfo", 10),
            ("estimatesmartfee", 10),
            ("sendrawtransaction", 10),
            ("decoderawtransaction", 10),
        ];
        for &(method, cost) in entries {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    pub fn new(default_cost: u32) -> Self {
        Self { costs: HashMap::new(), default_cost }
    }

    pub fn set_cost(&mut self, method: &str, cost: u32) {
        self.costs.insert(method.to_string(), cost);
    }

    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for BitcoinCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// Known endpoints
// ---------------------------------------------------------------------------

pub fn bitcoin_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "https://btc.getblock.io/mainnet/",
        "https://bitcoin-mainnet.public.blastapi.io",
    ]
}

pub fn bitcoin_testnet_endpoints() -> &'static [&'static str] {
    &[
        "https://btc.getblock.io/testnet/",
    ]
}

// ---------------------------------------------------------------------------
// BitcoinTransport
// ---------------------------------------------------------------------------

/// Bitcoin RPC transport wrapper.
pub struct BitcoinTransport {
    inner: Arc<dyn RpcTransport>,
}

impl BitcoinTransport {
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn RpcTransport> {
        &self.inner
    }
}

#[async_trait]
impl RpcTransport for BitcoinTransport {
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
// BitcoinChainClient
// ---------------------------------------------------------------------------

/// Bitcoin implementation of [`ChainClient`].
pub struct BitcoinChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl BitcoinChainClient {
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for BitcoinChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::new(1, "getblockcount", vec![]);
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        result.as_u64().ok_or_else(|| {
            TransportError::Other("expected u64 for block count".into())
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        // Get block hash
        let hash_req = JsonRpcRequest::new(
            1,
            "getblockhash",
            vec![serde_json::json!(height)],
        );
        let hash_resp = self.transport.send(hash_req).await?;
        let hash_result = hash_resp.into_result().map_err(TransportError::Rpc)?;
        let block_hash = match hash_result.as_str() {
            Some(h) => h.to_string(),
            None => return Ok(None),
        };

        // Get block with verbosity=1 (includes tx IDs)
        let block_req = JsonRpcRequest::new(
            1,
            "getblock",
            vec![
                serde_json::Value::String(block_hash.clone()),
                serde_json::json!(1), // verbosity
            ],
        );
        let block_resp = self.transport.send(block_req).await?;
        let result = block_resp.into_result().map_err(TransportError::Rpc)?;

        if result.is_null() {
            return Ok(None);
        }

        let parent_hash = result["previousblockhash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = result["time"].as_i64().unwrap_or(0);
        let tx_count = result["tx"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        Ok(Some(ChainBlock {
            height,
            hash: block_hash,
            parent_hash,
            timestamp,
            tx_count,
        }))
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    fn chain_family(&self) -> &str {
        "bitcoin"
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
            Self { url: "mock://btc".to_string(), responses: Mutex::new(responses) }
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
        assert_eq!(classify_bitcoin_method("getblock"), MethodSafety::Safe);
        assert_eq!(classify_bitcoin_method("sendrawtransaction"), MethodSafety::Idempotent);
        assert_eq!(classify_bitcoin_method("walletpassphrase"), MethodSafety::Unsafe);
    }

    #[test]
    fn cu_costs() {
        let t = BitcoinCuCostTable::defaults();
        assert_eq!(t.cost_for("getblockcount"), 5);
        assert_eq!(t.cost_for("getblock"), 20);
    }

    #[tokio::test]
    async fn btc_get_head_height() {
        let t = Arc::new(MockTransport::new(vec![ok(serde_json::json!(830000u64))]));
        let c = BitcoinChainClient::new(t, "mainnet");
        assert_eq!(c.get_head_height().await.unwrap(), 830000);
    }

    #[tokio::test]
    async fn btc_get_block() {
        let t = Arc::new(MockTransport::new(vec![
            ok(serde_json::Value::String("00000000abc".to_string())),
            ok(serde_json::json!({
                "previousblockhash": "00000000def",
                "time": 1700000000,
                "tx": ["tx1", "tx2"]
            })),
        ]));
        let c = BitcoinChainClient::new(t, "mainnet");
        let b = c.get_block_by_height(830000).await.unwrap().unwrap();
        assert_eq!(b.hash, "00000000abc");
        assert_eq!(b.parent_hash, "00000000def");
        assert_eq!(b.tx_count, 2);
    }

    #[tokio::test]
    async fn btc_metadata() {
        let t = Arc::new(MockTransport::new(vec![]));
        let c = BitcoinChainClient::new(t, "mainnet");
        assert_eq!(c.chain_family(), "bitcoin");
    }
}
