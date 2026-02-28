//! # chainrpc-node
//!
//! Node.js / TypeScript bindings for ChainRPC — production-grade RPC transport.
//! Built with napi-rs.
//!
//! ## Usage (TypeScript)
//! ```typescript
//! import { HttpRpcClient, ProviderPool } from '@chainfoundry/chainrpc';
//!
//! // Single provider
//! const client = HttpRpcClient.create('https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY');
//! const blockNumber = await client.call('eth_blockNumber', []);
//! console.log(blockNumber); // "0x12a05f2"
//!
//! // Multi-provider pool with auto-failover
//! const pool = ProviderPool.create([
//!   'https://eth-mainnet.g.alchemy.com/v2/KEY1',
//!   'https://mainnet.infura.io/v3/KEY2',
//!   'https://rpc.ankr.com/eth',
//! ]);
//! const result = await pool.call('eth_getBalance', ['0x...', 'latest']);
//! ```

#![deny(clippy::all)]
#![allow(clippy::unnecessary_wraps)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use chainrpc_http::HttpRpcClient as RustHttpClient;
use chainrpc_core::{
    pool::ProviderPool as RustPool,
    request::JsonRpcRequest,
};

// ─── HttpRpcClient ────────────────────────────────────────────────────────────

#[napi]
pub struct HttpRpcClient {
    inner: RustHttpClient,
}

#[napi]
impl HttpRpcClient {
    /// Create a new HTTP JSON-RPC client.
    ///
    /// `url` should be the full RPC endpoint URL including API key if required.
    #[napi(factory)]
    pub fn create(url: String) -> Result<Self> {
        let inner = RustHttpClient::new(&url)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Send a JSON-RPC call and return the result.
    ///
    /// Returns the raw JSON result value as a string.
    /// Throws on JSON-RPC errors (code/message returned by the node).
    #[napi]
    pub async fn call(&self, method: String, params_json: String) -> Result<String> {
        let params: Vec<serde_json::Value> = serde_json::from_str(&params_json)
            .map_err(|e| Error::from_reason(format!("params parse: {e}")))?;

        let req = JsonRpcRequest::new(method, params);
        let resp = self.inner.send(req).await
            .map_err(|e| Error::from_reason(e.to_string()))?;

        if let Some(err) = resp.error {
            return Err(Error::from_reason(format!(
                "JSON-RPC error {}: {}",
                err.code, err.message
            )));
        }

        Ok(resp.result
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into()))
    }

    /// Send multiple JSON-RPC calls as a batch (single HTTP request).
    ///
    /// `requests_json` should be a JSON array of `{ method, params }` objects.
    /// Returns a JSON array of results in the same order.
    #[napi]
    pub async fn batch_call(&self, requests_json: String) -> Result<String> {
        let reqs: Vec<serde_json::Value> = serde_json::from_str(&requests_json)
            .map_err(|e| Error::from_reason(format!("requests parse: {e}")))?;

        let rpc_reqs: Vec<JsonRpcRequest> = reqs.iter().map(|r| {
            let method = r["method"].as_str().unwrap_or("").to_string();
            let params = r["params"].as_array().cloned().unwrap_or_default();
            JsonRpcRequest::new(method, params)
        }).collect();

        let responses = self.inner.send_batch(rpc_reqs).await
            .map_err(|e| Error::from_reason(e.to_string()))?;

        let results: Vec<serde_json::Value> = responses.into_iter().map(|resp| {
            if let Some(err) = resp.error {
                serde_json::json!({ "error": { "code": err.code, "message": err.message } })
            } else {
                resp.result.unwrap_or(serde_json::Value::Null)
            }
        }).collect();

        serde_json::to_string(&results)
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    /// Get current provider health status.
    #[napi]
    pub fn health(&self) -> String {
        format!("{:?}", self.inner.health())
    }
}

// ─── ProviderPool ─────────────────────────────────────────────────────────────

#[napi]
pub struct ProviderPool {
    inner: RustPool,
}

#[napi]
impl ProviderPool {
    /// Create a multi-provider pool with automatic failover.
    ///
    /// Providers are tried in order; failed providers are circuit-broken
    /// until they recover. Requests are automatically retried on failure.
    #[napi(factory)]
    pub fn create(urls: Vec<String>) -> Result<Self> {
        let inner = RustPool::from_urls(&urls)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Send a JSON-RPC call through the pool.
    ///
    /// Automatically retries on failed providers and uses circuit breaking.
    #[napi]
    pub async fn call(&self, method: String, params_json: String) -> Result<String> {
        let params: Vec<serde_json::Value> = serde_json::from_str(&params_json)
            .map_err(|e| Error::from_reason(format!("params parse: {e}")))?;

        let req = JsonRpcRequest::new(method, params);
        let resp = self.inner.send(req).await
            .map_err(|e| Error::from_reason(e.to_string()))?;

        if let Some(err) = resp.error {
            return Err(Error::from_reason(format!(
                "JSON-RPC error {}: {}",
                err.code, err.message
            )));
        }

        Ok(resp.result
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".into()))
    }

    /// Get the number of healthy providers in the pool.
    #[napi(getter)]
    pub fn healthy_provider_count(&self) -> u32 {
        self.inner.healthy_count() as u32
    }

    /// Get health status for all providers as a JSON array.
    #[napi]
    pub fn provider_statuses(&self) -> String {
        serde_json::to_string(&self.inner.health_report()).unwrap_or_else(|_| "[]".into())
    }
}
