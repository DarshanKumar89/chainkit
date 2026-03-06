//! Multi-chain router — route requests to the correct chain's provider pool.

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

/// A router that maps chain IDs to transport instances.
///
/// Allows a single entry point for multiple chains:
/// ```ignore
/// let router = ChainRouter::new();
/// router.add_chain(1, eth_pool);       // Ethereum mainnet
/// router.add_chain(137, polygon_pool); // Polygon
/// router.add_chain(42161, arb_pool);   // Arbitrum
///
/// let balance = router.chain(1).send(req).await?;
/// ```
pub struct ChainRouter {
    chains: HashMap<u64, Arc<dyn RpcTransport>>,
}

impl ChainRouter {
    /// Create a new empty router.
    pub fn new() -> Self {
        Self {
            chains: HashMap::new(),
        }
    }

    /// Register a transport for a chain ID.
    pub fn add_chain(&mut self, chain_id: u64, transport: Arc<dyn RpcTransport>) {
        self.chains.insert(chain_id, transport);
    }

    /// Get the transport for a specific chain.
    pub fn chain(&self, chain_id: u64) -> Result<&dyn RpcTransport, TransportError> {
        self.chains
            .get(&chain_id)
            .map(|t| t.as_ref())
            .ok_or_else(|| {
                TransportError::Other(format!("no provider configured for chain {chain_id}"))
            })
    }

    /// Send a request to a specific chain.
    pub async fn send_to(
        &self,
        chain_id: u64,
        req: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, TransportError> {
        let transport = self
            .chains
            .get(&chain_id)
            .ok_or_else(|| TransportError::Other(format!("no provider for chain {chain_id}")))?;
        transport.send(req).await
    }

    /// Send requests to multiple chains in parallel and collect results.
    ///
    /// Returns results in the same order as the input. If any request fails,
    /// its slot contains the error.
    pub async fn parallel(
        &self,
        requests: Vec<(u64, JsonRpcRequest)>,
    ) -> Vec<Result<JsonRpcResponse, TransportError>> {
        let mut handles = Vec::with_capacity(requests.len());

        for (chain_id, req) in requests {
            let transport = self.chains.get(&chain_id).cloned();
            handles.push(tokio::spawn(async move {
                match transport {
                    Some(t) => t.send(req).await,
                    None => Err(TransportError::Other(format!(
                        "no provider for chain {chain_id}"
                    ))),
                }
            }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(TransportError::Other(format!("task join error: {e}")))),
            }
        }
        results
    }

    /// List all configured chain IDs.
    pub fn chain_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self.chains.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Number of configured chains.
    pub fn chain_count(&self) -> usize {
        self.chains.len()
    }

    /// Health summary across all chains.
    pub fn health_summary(&self) -> Vec<(u64, HealthStatus)> {
        let mut summary: Vec<_> = self
            .chains
            .iter()
            .map(|(&id, t)| (id, t.health()))
            .collect();
        summary.sort_by_key(|(id, _)| *id);
        summary
    }
}

impl Default for ChainRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use async_trait::async_trait;

    struct MockChainTransport {
        chain_id: u64,
    }

    #[async_trait]
    impl RpcTransport for MockChainTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::json!(format!("chain_{}", self.chain_id))),
                error: None,
            })
        }
        fn url(&self) -> &str {
            "mock://chain"
        }
    }

    fn make_router() -> ChainRouter {
        let mut router = ChainRouter::new();
        router.add_chain(1, Arc::new(MockChainTransport { chain_id: 1 }));
        router.add_chain(137, Arc::new(MockChainTransport { chain_id: 137 }));
        router.add_chain(42161, Arc::new(MockChainTransport { chain_id: 42161 }));
        router
    }

    #[tokio::test]
    async fn send_to_specific_chain() {
        let router = make_router();
        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let resp = router.send_to(1, req).await.unwrap();
        assert_eq!(resp.result.unwrap().as_str().unwrap(), "chain_1");
    }

    #[tokio::test]
    async fn send_to_unknown_chain_fails() {
        let router = make_router();
        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let result = router.send_to(999, req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn parallel_requests() {
        let router = make_router();
        let requests = vec![
            (1, JsonRpcRequest::auto("eth_blockNumber", vec![])),
            (137, JsonRpcRequest::auto("eth_blockNumber", vec![])),
            (42161, JsonRpcRequest::auto("eth_blockNumber", vec![])),
        ];

        let results = router.parallel(requests).await;
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn chain_ids_sorted() {
        let router = make_router();
        assert_eq!(router.chain_ids(), vec![1, 137, 42161]);
    }

    #[test]
    fn chain_count() {
        let router = make_router();
        assert_eq!(router.chain_count(), 3);
    }

    #[test]
    fn health_summary() {
        let router = make_router();
        let summary = router.health_summary();
        assert_eq!(summary.len(), 3);
        // All should be Unknown (default)
        for (_, status) in &summary {
            assert_eq!(*status, HealthStatus::Unknown);
        }
    }
}
