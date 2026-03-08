//! Unified `ChainClient` trait — a high-level typed abstraction over any
//! blockchain's RPC interface.
//!
//! While [`RpcTransport`] operates at the raw JSON-RPC level (any method, any
//! params), `ChainClient` provides a minimal, chain-agnostic API for common
//! operations like fetching the current block height or retrieving a block by
//! number.
//!
//! Each chain family (EVM, Solana, Cosmos, Substrate, Bitcoin, Aptos, Sui)
//! provides its own implementation wrapping the appropriate transport.

use async_trait::async_trait;
use serde::de::Error as _;
use serde::{Deserialize, Serialize};

use crate::error::TransportError;

// ─── ChainBlock ──────────────────────────────────────────────────────────────

/// A chain-agnostic block summary.
///
/// Contains only the fields that every blockchain provides. Individual chain
/// clients may expose richer block types through chain-specific methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainBlock {
    /// Block height / slot / checkpoint number.
    pub height: u64,
    /// Block hash (hex string, chain-specific format).
    pub hash: String,
    /// Parent block hash.
    pub parent_hash: String,
    /// Block timestamp (Unix seconds).
    pub timestamp: i64,
    /// Number of transactions in the block.
    pub tx_count: u32,
}

// ─── ChainClient trait ──────────────────────────────────────────────────────

/// A high-level, chain-agnostic blockchain client.
///
/// Provides a minimal set of operations that any blockchain supports.
/// Use this trait when writing chain-agnostic infrastructure (indexers,
/// monitors, dashboards) that needs to work across multiple blockchain
/// families.
///
/// For chain-specific operations (e.g. EVM `eth_getLogs`, Solana
/// `getSignaturesForAddress`), use the concrete client types directly.
#[async_trait]
pub trait ChainClient: Send + Sync {
    /// Get the current head height (block number / slot / checkpoint).
    async fn get_head_height(&self) -> Result<u64, TransportError>;

    /// Get a block by its height.
    ///
    /// Returns `None` if the block does not exist (e.g. future block number).
    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError>;

    /// Return the chain identifier (e.g. `"1"` for Ethereum mainnet,
    /// `"mainnet-beta"` for Solana).
    fn chain_id(&self) -> &str;

    /// Return the chain family name (e.g. `"evm"`, `"solana"`, `"cosmos"`).
    fn chain_family(&self) -> &str;

    /// Perform a health check against the underlying transport.
    async fn health_check(&self) -> Result<bool, TransportError>;
}

// ─── EvmChainClient ─────────────────────────────────────────────────────────

use std::sync::Arc;
use crate::transport::RpcTransport;

/// EVM implementation of [`ChainClient`].
///
/// Wraps any `Arc<dyn RpcTransport>` and translates `ChainClient` methods
/// into the appropriate `eth_*` JSON-RPC calls.
pub struct EvmChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl EvmChainClient {
    /// Create an EVM chain client with the given transport and chain ID.
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for EvmChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = crate::request::JsonRpcRequest::new(
            1,
            "eth_blockNumber",
            vec![],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        let hex_str = result
            .as_str()
            .ok_or_else(|| TransportError::Deserialization(
                serde_json::Error::custom("expected hex string for block number"),
            ))?;
        let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
        u64::from_str_radix(stripped, 16).map_err(|e| {
            TransportError::Deserialization(serde_json::Error::custom(format!(
                "invalid block number hex: {e}"
            )))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        let hex_height = format!("0x{height:x}");
        let req = crate::request::JsonRpcRequest::new(
            1,
            "eth_getBlockByNumber",
            vec![
                serde_json::Value::String(hex_height),
                serde_json::Value::Bool(false), // don't include full txs
            ],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        if result.is_null() {
            return Ok(None);
        }

        let hash = result["hash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let parent_hash = result["parentHash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = parse_hex_u64(result["timestamp"].as_str().unwrap_or("0x0"))
            as i64;
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
        "evm"
    }

    async fn health_check(&self) -> Result<bool, TransportError> {
        // Simple: if eth_blockNumber succeeds, we're healthy
        self.get_head_height().await.map(|_| true)
    }
}

// ─── SolanaChainClient ──────────────────────────────────────────────────────

/// Solana implementation of [`ChainClient`].
///
/// Wraps any `Arc<dyn RpcTransport>` and translates `ChainClient` methods
/// into the appropriate Solana JSON-RPC calls.
pub struct SolanaChainClient {
    transport: Arc<dyn RpcTransport>,
    chain_id: String,
}

impl SolanaChainClient {
    /// Create a Solana chain client.
    pub fn new(transport: Arc<dyn RpcTransport>, chain_id: impl Into<String>) -> Self {
        Self {
            transport,
            chain_id: chain_id.into(),
        }
    }
}

#[async_trait]
impl ChainClient for SolanaChainClient {
    async fn get_head_height(&self) -> Result<u64, TransportError> {
        let req = crate::request::JsonRpcRequest::new(
            1,
            "getSlot",
            vec![],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        // Solana returns slot as a JSON number, not hex
        result.as_u64().ok_or_else(|| {
            TransportError::Deserialization(serde_json::Error::custom(
                "expected u64 for slot number",
            ))
        })
    }

    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<ChainBlock>, TransportError> {
        let req = crate::request::JsonRpcRequest::new(
            1,
            "getBlock",
            vec![
                serde_json::Value::Number(serde_json::Number::from(height)),
                serde_json::json!({
                    "encoding": "json",
                    "transactionDetails": "none",
                    "rewards": false,
                }),
            ],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;

        if result.is_null() {
            return Ok(None);
        }

        let hash = result["blockhash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let parent_hash = result["previousBlockhash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = result["blockTime"].as_i64().unwrap_or(0);
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
        "solana"
    }

    async fn health_check(&self) -> Result<bool, TransportError> {
        let req = crate::request::JsonRpcRequest::new(
            1,
            "getHealth",
            vec![],
        );
        let resp = self.transport.send(req).await?;
        let result = resp.into_result().map_err(TransportError::Rpc)?;
        Ok(result.as_str() == Some("ok"))
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn parse_hex_u64(hex_str: &str) -> u64 {
    let stripped = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    u64::from_str_radix(stripped, 16).unwrap_or(0)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
    use std::sync::Mutex;

    /// A mock transport that records requests and returns pre-configured responses.
    struct MockTransport {
        url: String,
        responses: Mutex<Vec<JsonRpcResponse>>,
        recorded_requests: Mutex<Vec<(String, Vec<serde_json::Value>)>>,
    }

    impl MockTransport {
        fn new(responses: Vec<JsonRpcResponse>) -> Self {
            Self {
                url: "mock://test".to_string(),
                responses: Mutex::new(responses),
                recorded_requests: Mutex::new(Vec::new()),
            }
        }

        fn recorded(&self) -> Vec<(String, Vec<serde_json::Value>)> {
            self.recorded_requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            self.recorded_requests.lock().unwrap().push((
                req.method.clone(),
                req.params.clone(),
            ));
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

    fn ok_response(result: serde_json::Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: RpcId::Number(1),
            result: Some(result),
            error: None,
        }
    }

    // ── EVM tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn evm_get_head_height() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::String("0x10".to_string())),
        ]));
        let client = EvmChainClient::new(transport.clone(), "1");

        let height = client.get_head_height().await.unwrap();
        assert_eq!(height, 16);

        let reqs = transport.recorded();
        assert_eq!(reqs[0].0, "eth_blockNumber");
    }

    #[tokio::test]
    async fn evm_get_block_by_height() {
        let block_json = serde_json::json!({
            "hash": "0xabc123",
            "parentHash": "0xdef456",
            "timestamp": "0x60000000",
            "transactions": ["0xtx1", "0xtx2", "0xtx3"]
        });
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(block_json),
        ]));
        let client = EvmChainClient::new(transport.clone(), "1");

        let block = client.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(block.height, 100);
        assert_eq!(block.hash, "0xabc123");
        assert_eq!(block.parent_hash, "0xdef456");
        assert_eq!(block.tx_count, 3);

        let reqs = transport.recorded();
        assert_eq!(reqs[0].0, "eth_getBlockByNumber");
        assert_eq!(reqs[0].1[0], serde_json::Value::String("0x64".to_string()));
    }

    #[tokio::test]
    async fn evm_get_block_null() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::Null),
        ]));
        let client = EvmChainClient::new(transport, "1");

        let block = client.get_block_by_height(99999999).await.unwrap();
        assert!(block.is_none());
    }

    #[tokio::test]
    async fn evm_chain_metadata() {
        let transport = Arc::new(MockTransport::new(vec![]));
        let client = EvmChainClient::new(transport, "137");
        assert_eq!(client.chain_id(), "137");
        assert_eq!(client.chain_family(), "evm");
    }

    #[tokio::test]
    async fn evm_health_check() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::String("0x1".to_string())),
        ]));
        let client = EvmChainClient::new(transport, "1");
        assert!(client.health_check().await.unwrap());
    }

    // ── Solana tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn solana_get_head_height() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::Number(200_000_000u64.into())),
        ]));
        let client = SolanaChainClient::new(transport.clone(), "mainnet-beta");

        let slot = client.get_head_height().await.unwrap();
        assert_eq!(slot, 200_000_000);

        let reqs = transport.recorded();
        assert_eq!(reqs[0].0, "getSlot");
    }

    #[tokio::test]
    async fn solana_get_block_by_height() {
        let block_json = serde_json::json!({
            "blockhash": "5abc123def",
            "previousBlockhash": "4abc123def",
            "blockTime": 1700000000i64,
            "transactions": [{"tx": 1}, {"tx": 2}]
        });
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(block_json),
        ]));
        let client = SolanaChainClient::new(transport.clone(), "mainnet-beta");

        let block = client.get_block_by_height(100).await.unwrap().unwrap();
        assert_eq!(block.height, 100);
        assert_eq!(block.hash, "5abc123def");
        assert_eq!(block.parent_hash, "4abc123def");
        assert_eq!(block.timestamp, 1700000000);
        assert_eq!(block.tx_count, 2);

        let reqs = transport.recorded();
        assert_eq!(reqs[0].0, "getBlock");
    }

    #[tokio::test]
    async fn solana_health_check_ok() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::String("ok".to_string())),
        ]));
        let client = SolanaChainClient::new(transport, "mainnet-beta");
        assert!(client.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn solana_health_check_behind() {
        let transport = Arc::new(MockTransport::new(vec![
            ok_response(serde_json::Value::String("behind".to_string())),
        ]));
        let client = SolanaChainClient::new(transport, "mainnet-beta");
        assert!(!client.health_check().await.unwrap());
    }

    #[tokio::test]
    async fn solana_chain_metadata() {
        let transport = Arc::new(MockTransport::new(vec![]));
        let client = SolanaChainClient::new(transport, "devnet");
        assert_eq!(client.chain_id(), "devnet");
        assert_eq!(client.chain_family(), "solana");
    }

    // ── ChainBlock tests ────────────────────────────────────────────────

    #[test]
    fn chain_block_serde_roundtrip() {
        let block = ChainBlock {
            height: 100,
            hash: "0xabc".to_string(),
            parent_hash: "0xdef".to_string(),
            timestamp: 1700000000,
            tx_count: 42,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: ChainBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.height, 100);
        assert_eq!(back.tx_count, 42);
    }

    #[test]
    fn parse_hex_u64_works() {
        assert_eq!(parse_hex_u64("0x10"), 16);
        assert_eq!(parse_hex_u64("0xff"), 255);
        assert_eq!(parse_hex_u64("10"), 16);
        assert_eq!(parse_hex_u64("0x0"), 0);
        assert_eq!(parse_hex_u64("invalid"), 0);
    }
}
