//! # chainrpc (Python)
//!
//! PyO3-based Python bindings for ChainRPC.
//!
//! ## Usage
//! ```python
//! import asyncio
//! from chainrpc import HttpRpcClient, ProviderPool
//!
//! async def main():
//!     # Single provider
//!     client = HttpRpcClient("https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY")
//!     block = await client.call("eth_blockNumber", "[]")
//!     print(block)  # "0x12a05f2"
//!
//!     # Multi-provider pool
//!     pool = ProviderPool([
//!         "https://eth-mainnet.g.alchemy.com/v2/KEY1",
//!         "https://mainnet.infura.io/v3/KEY2",
//!         "https://rpc.ankr.com/eth",
//!     ])
//!     balance = await pool.call("eth_getBalance", '["0x...", "latest"]')
//!     print(balance)
//!
//! asyncio.run(main())
//! ```

use pyo3::prelude::*;
use pyo3_asyncio::tokio::future_into_py;

use chainrpc_http::HttpRpcClient as RustHttpClient;
use chainrpc_core::{
    pool::ProviderPool as RustPool,
    request::JsonRpcRequest,
};

// ─── HttpRpcClient ────────────────────────────────────────────────────────────

#[pyclass(name = "HttpRpcClient")]
pub struct PyHttpRpcClient {
    inner: std::sync::Arc<RustHttpClient>,
}

#[pymethods]
impl PyHttpRpcClient {
    /// Create a new HTTP JSON-RPC client.
    #[new]
    fn new(url: &str) -> PyResult<Self> {
        let inner = RustHttpClient::new(url)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        Ok(Self { inner: std::sync::Arc::new(inner) })
    }

    /// Send a JSON-RPC call (async).
    ///
    /// `params_json` is a JSON string like `'["0x...", "latest"]'`.
    /// Returns the result as a JSON string.
    fn call<'py>(&self, py: Python<'py>, method: String, params_json: String) -> PyResult<&'py PyAny> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let params: Vec<serde_json::Value> = serde_json::from_str(&params_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("params parse: {e}")))?;
            let req = JsonRpcRequest::new(method, params);
            let resp = inner.send(req).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            if let Some(err) = resp.error {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("JSON-RPC {}: {}", err.code, err.message)
                ));
            }
            Ok(resp.result.map(|v| v.to_string()).unwrap_or_else(|| "null".into()))
        })
    }

    /// Send a batch of JSON-RPC calls (async).
    ///
    /// `requests_json` is a JSON array like `[{"method": "eth_blockNumber", "params": []}]`.
    fn batch_call<'py>(&self, py: Python<'py>, requests_json: String) -> PyResult<&'py PyAny> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let reqs: Vec<serde_json::Value> = serde_json::from_str(&requests_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

            let rpc_reqs: Vec<JsonRpcRequest> = reqs.iter().map(|r| {
                let method = r["method"].as_str().unwrap_or("").to_string();
                let params = r["params"].as_array().cloned().unwrap_or_default();
                JsonRpcRequest::new(method, params)
            }).collect();

            let responses = inner.send_batch(rpc_reqs).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let results: Vec<serde_json::Value> = responses.into_iter().map(|resp| {
                if let Some(err) = resp.error {
                    serde_json::json!({ "error": { "code": err.code, "message": err.message } })
                } else {
                    resp.result.unwrap_or(serde_json::Value::Null)
                }
            }).collect();

            serde_json::to_string(&results)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    fn __repr__(&self) -> String {
        "HttpRpcClient()".to_string()
    }
}

// ─── ProviderPool ─────────────────────────────────────────────────────────────

#[pyclass(name = "ProviderPool")]
pub struct PyProviderPool {
    inner: std::sync::Arc<RustPool>,
}

#[pymethods]
impl PyProviderPool {
    /// Create a multi-provider pool with automatic failover and retry.
    #[new]
    fn new(urls: Vec<String>) -> PyResult<Self> {
        let url_refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
        let inner = RustPool::from_urls(&url_refs)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        Ok(Self { inner: std::sync::Arc::new(inner) })
    }

    /// Send a JSON-RPC call through the pool (async).
    fn call<'py>(&self, py: Python<'py>, method: String, params_json: String) -> PyResult<&'py PyAny> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let params: Vec<serde_json::Value> = serde_json::from_str(&params_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
            let req = JsonRpcRequest::new(method, params);
            let resp = inner.send(req).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            if let Some(err) = resp.error {
                return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    format!("JSON-RPC {}: {}", err.code, err.message)
                ));
            }
            Ok(resp.result.map(|v| v.to_string()).unwrap_or_else(|| "null".into()))
        })
    }

    /// Number of currently healthy providers.
    #[getter]
    fn healthy_provider_count(&self) -> usize {
        self.inner.healthy_count()
    }

    fn __repr__(&self) -> String {
        format!("ProviderPool(healthy={})", self.inner.healthy_count())
    }
}

// ─── Module ───────────────────────────────────────────────────────────────────

#[pymodule]
fn chainrpc(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_asyncio::tokio::init_multi_thread_once();
    m.add_class::<PyHttpRpcClient>()?;
    m.add_class::<PyProviderPool>()?;
    Ok(())
}
