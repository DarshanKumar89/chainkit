//! Aptos RPC support — REST-to-JSON-RPC adapter, method safety, and transport.
//!
//! Aptos uses a REST API (not JSON-RPC), so the transport internally converts
//! chain client calls into REST operations. For use with the generic
//! `RpcTransport` trait, we provide a thin adapter.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use crate::chain_client::{ChainBlock, ChainClient};
use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// CU cost table
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AptosCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl AptosCuCostTable {
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("get_ledger_info", 5),
            ("get_block_by_height", 20),
            ("get_block_by_version", 20),
            ("get_account", 10),
            ("get_account_resources", 15),
            ("get_account_modules", 15),
            ("get_transactions", 20),
            ("get_transaction_by_hash", 15),
            ("submit_transaction", 10),
            ("simulate_transaction", 30),
            ("get_events_by_event_handle", 20),
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

impl Default for AptosCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// Known endpoints
// ---------------------------------------------------------------------------

pub fn aptos_mainnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.mainnet.aptoslabs.com/v1",
        "https://aptos-mainnet.nodereal.io/v1",
    ]
}

pub fn aptos_testnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.testnet.aptoslabs.com/v1",
    ]
}

pub fn aptos_devnet_endpoints() -> &'static [&'static str] {
    &[
        "https://fullnode.devnet.aptoslabs.com/v1",
    ]
}

// ---------------------------------------------------------------------------
// AptosTransport
// ---------------------------------------------------------------------------

/// Aptos RPC transport wrapper.
///
/// Since Aptos uses REST, this wraps a JSON-RPC transport and maps
/// known method names to REST-like operations internally.
pub struct AptosTransport {
    inner: Arc<dyn RpcTransport>,
}

impl AptosTransport {
    pub fn new(inner: Arc<dyn RpcTransport>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn RpcTransport> {
        &self.inner
    }
}

#[async_trait]
impl RpcTransport for AptosTransport {
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
// AptosChainClient
// ---------------------------------------------------------------------------

/// Aptos implementation of [`ChainClient`].
///
/// Maps chain client methods to Aptos REST API calls routed through the
/// underlying transport as JSON-RPC method names.
pub struct AptosChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl AptosChainClient {
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for AptosChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::new(1, "get_ledger_info", vec![]);
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        // Aptos returns block_height as a string
        let height_str = result["block_height"]
            .as_str()
            .or_else(|| result["result"]["block_height"].as_str())
            .unwrap_or("0");
        height_str.parse::<u64>().map_err(|e| {
            TransportError::Other(format!("invalid aptos block height: {e}"))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        let req = JsonRpcRequest::new(
            1,
            "get_block_by_height",
            vec![serde_json::json!(height.to_string())],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        if result.is_null() {
            return Ok(None);
        }

        let hash = result["block_hash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = result["block_timestamp"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|us| (us / 1_000_000) as i64) // microseconds to seconds
            .unwrap_or(0);
        let tx_count = result["transactions"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        // Aptos doesn't have parent hash in the same way
        let first_version = result["first_version"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        Ok(Some(ChainBlock {
            height,
            hash,
            parent_hash: first_version, // closest analog
            timestamp,
            tx_count,
        }))
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    fn chain_family(&self) -> &str {
        "aptos"
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
            Self { url: "mock://aptos".into(), responses: Mutex::new(responses) }
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
    fn cu_costs() {
        let t = AptosCuCostTable::defaults();
        assert_eq!(t.cost_for("get_ledger_info"), 5);
        assert_eq!(t.cost_for("get_block_by_height"), 20);
    }

    #[test]
    fn endpoints() {
        assert!(!aptos_mainnet_endpoints().is_empty());
        assert!(!aptos_testnet_endpoints().is_empty());
    }

    #[tokio::test]
    async fn aptos_get_head_height() {
        let t = Arc::new(MockTransport::new(vec![ok(serde_json::json!({
            "block_height": "150000000"
        }))]));
        let c = AptosChainClient::new(t, "1");
        assert_eq!(c.get_head_height().await.unwrap(), 150000000);
    }

    #[tokio::test]
    async fn aptos_get_block() {
        let t = Arc::new(MockTransport::new(vec![ok(serde_json::json!({
            "block_hash": "0xabc123",
            "block_timestamp": "1700000000000000",
            "first_version": "100000",
            "transactions": [{"type": "user"}, {"type": "user"}]
        }))]));
        let c = AptosChainClient::new(t, "1");
        let b = c.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(b.hash, "0xabc123");
        assert_eq!(b.timestamp, 1700000000);
        assert_eq!(b.tx_count, 2);
    }

    #[tokio::test]
    async fn aptos_metadata() {
        let t = Arc::new(MockTransport::new(vec![]));
        let c = AptosChainClient::new(t, "2");
        assert_eq!(c.chain_family(), "aptos");
        assert_eq!(c.chain_id(), "2");
    }
}
