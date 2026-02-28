//! # chaincodec (Python)
//!
//! PyO3-based Python bindings for ChainCodec.
//!
//! ## Usage
//! ```python
//! from chaincodec import EvmDecoder, MemoryRegistry
//!
//! registry = MemoryRegistry()
//! registry.load_csdl("""
//! schema ERC20Transfer:
//!   version: 1
//!   chains: [ethereum]
//!   event: Transfer
//!   fingerprint: "0xddf252ad..."
//!   fields:
//!     from:  { type: address, indexed: true }
//!     to:    { type: address, indexed: true }
//!     value: { type: uint256, indexed: false }
//!   meta: {}
//! """)
//!
//! decoder = EvmDecoder()
//! event = decoder.decode_event({
//!     "chain": "ethereum",
//!     "tx_hash": "0x...",
//!     "block_number": 19000000,
//!     "block_timestamp": 1700000000,
//!     "log_index": 0,
//!     "address": "0x...",
//!     "topics": ["0xddf252ad...", "0x000...from", "0x000...to"],
//!     "data": "0x000...amount",
//! }, registry)
//! print(event["fields"])
//! ```

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use std::collections::HashMap;

use chaincodec_evm::{
    EvmDecoder as RustEvmDecoder, EvmCallDecoder as RustCallDecoder,
    EvmEncoder as RustEncoder, Eip712Parser as RustEip712Parser,
};
use chaincodec_registry::{CsdlParser, MemoryRegistry as RustRegistry};
use chaincodec_core::{
    chain::chains,
    decoder::{ChainDecoder, ErrorMode},
    event::RawEvent,
    schema::SchemaRegistry,
    types::NormalizedValue,
};

// ─── MemoryRegistry ──────────────────────────────────────────────────────────

#[pyclass(name = "MemoryRegistry")]
pub struct PyMemoryRegistry {
    inner: RustRegistry,
}

#[pymethods]
impl PyMemoryRegistry {
    #[new]
    fn new() -> Self {
        Self {
            inner: RustRegistry::new(),
        }
    }

    /// Load schemas from a CSDL YAML string.
    /// Returns the number of schemas loaded.
    fn load_csdl(&mut self, csdl: &str) -> PyResult<usize> {
        let schemas = CsdlParser::parse_all(csdl)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let count = schemas.len();
        for schema in schemas {
            self.inner
                .insert(schema)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        }
        Ok(count)
    }

    /// Load schemas from a .csdl file.
    fn load_file(&mut self, path: &str) -> PyResult<usize> {
        self.inner
            .load_file(std::path::Path::new(path))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))
    }

    /// Load all .csdl files from a directory.
    fn load_directory(&mut self, path: &str) -> PyResult<usize> {
        self.inner
            .load_directory(std::path::Path::new(path))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))
    }

    /// Number of schemas currently registered.
    #[getter]
    fn schema_count(&self) -> usize {
        self.inner.len()
    }

    /// List all schema names.
    fn schema_names(&self) -> Vec<String> {
        self.inner.all_names()
    }

    fn __repr__(&self) -> String {
        format!("MemoryRegistry(schema_count={})", self.inner.len())
    }
}

// ─── EvmDecoder ───────────────────────────────────────────────────────────────

#[pyclass(name = "EvmDecoder")]
pub struct PyEvmDecoder {
    inner: RustEvmDecoder,
}

#[pymethods]
impl PyEvmDecoder {
    #[new]
    fn new() -> Self {
        Self {
            inner: RustEvmDecoder::new(),
        }
    }

    /// Decode a single EVM event log.
    ///
    /// `raw` should be a dict with keys: chain, tx_hash, block_number,
    /// block_timestamp, log_index, address, topics (list), data (hex str).
    ///
    /// Returns a dict with: schema, fields, fingerprint, decode_errors.
    fn decode_event<'py>(
        &self,
        py: Python<'py>,
        raw: &PyDict,
        registry: &PyMemoryRegistry,
    ) -> PyResult<PyObject> {
        let rust_raw = dict_to_raw_event(raw)?;
        let fp = self.inner.fingerprint(&rust_raw);
        let schema = registry
            .inner
            .get_by_fingerprint(&fp)
            .ok_or_else(|| {
                PyErr::new::<pyo3::exceptions::PyKeyError, _>(format!(
                    "schema not found for fingerprint {}",
                    fp.as_hex()
                ))
            })?;

        let decoded = self
            .inner
            .decode_event(&rust_raw, &schema)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        let result = PyDict::new(py);
        result.set_item("schema", &decoded.schema)?;
        result.set_item("schema_version", decoded.schema_version)?;
        result.set_item("tx_hash", &decoded.tx_hash)?;
        result.set_item("block_number", decoded.block_number)?;
        result.set_item("block_timestamp", decoded.block_timestamp)?;
        result.set_item("log_index", decoded.log_index)?;
        result.set_item("address", &decoded.address)?;
        result.set_item("fingerprint", fp.as_hex())?;

        let fields = normalized_map_to_py(py, &decoded.fields)?;
        result.set_item("fields", fields)?;

        let errors = PyDict::new(py);
        for (k, v) in &decoded.decode_errors {
            errors.set_item(k, v)?;
        }
        result.set_item("decode_errors", errors)?;

        Ok(result.into())
    }

    /// Compute the event fingerprint from a raw event dict.
    fn fingerprint(&self, raw: &PyDict) -> PyResult<String> {
        let rust_raw = dict_to_raw_event(raw)?;
        Ok(self.inner.fingerprint(&rust_raw).as_hex().to_string())
    }

    fn __repr__(&self) -> &'static str {
        "EvmDecoder()"
    }
}

// ─── EvmCallDecoder ───────────────────────────────────────────────────────────

#[pyclass(name = "EvmCallDecoder")]
pub struct PyEvmCallDecoder {
    inner: RustCallDecoder,
}

#[pymethods]
impl PyEvmCallDecoder {
    /// Create from standard Ethereum ABI JSON string.
    #[classmethod]
    fn from_abi_json(_cls: &PyAny, abi_json: &str) -> PyResult<Self> {
        let inner = RustCallDecoder::from_abi_json(abi_json)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Decode function call calldata (hex string with 0x prefix).
    fn decode_call<'py>(
        &self,
        py: Python<'py>,
        calldata: &str,
        function_name: Option<&str>,
    ) -> PyResult<PyObject> {
        let bytes = hex_str_to_bytes(calldata)?;
        let decoded = self
            .inner
            .decode_call(&bytes, function_name)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        let result = PyDict::new(py);
        result.set_item("function_name", &decoded.function_name)?;
        result.set_item("selector", decoded.selector_hex())?;

        let inputs = PyList::empty(py);
        for (name, val) in &decoded.inputs {
            let pair = (name.clone(), normalized_to_py(py, val)?);
            inputs.append(pair)?;
        }
        result.set_item("inputs", inputs)?;

        let errors = PyDict::new(py);
        for (k, v) in &decoded.decode_errors {
            errors.set_item(k, v)?;
        }
        result.set_item("decode_errors", errors)?;

        Ok(result.into())
    }

    fn function_names(&self) -> Vec<String> {
        self.inner.function_names().into_iter().map(|s| s.to_string()).collect()
    }

    fn selector_for(&self, function_name: &str) -> Option<String> {
        self.inner
            .selector_for(function_name)
            .map(|s| format!("0x{}", hex::encode(s)))
    }
}

// ─── EvmEncoder ───────────────────────────────────────────────────────────────

#[pyclass(name = "EvmEncoder")]
pub struct PyEvmEncoder {
    inner: RustEncoder,
}

#[pymethods]
impl PyEvmEncoder {
    #[classmethod]
    fn from_abi_json(_cls: &PyAny, abi_json: &str) -> PyResult<Self> {
        let inner = RustEncoder::from_abi_json(abi_json)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Encode a function call. args is a JSON string of NormalizedValues.
    fn encode_call(&self, function_name: &str, args_json: &str) -> PyResult<String> {
        let args: Vec<NormalizedValue> = serde_json::from_str(args_json)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let calldata = self
            .inner
            .encode_call(function_name, &args)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
        Ok(format!("0x{}", hex::encode(calldata)))
    }
}

// ─── Module definition ────────────────────────────────────────────────────────

// Module name must match the last segment of pyproject.toml module-name = "chaincodec._chaincodec"
#[pymodule]
fn _chaincodec(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PyMemoryRegistry>()?;
    m.add_class::<PyEvmDecoder>()?;
    m.add_class::<PyEvmCallDecoder>()?;
    m.add_class::<PyEvmEncoder>()?;
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_str_to_bytes(s: &str) -> PyResult<Vec<u8>> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(hex)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("hex decode: {e}")))
}

fn dict_to_raw_event(raw: &PyDict) -> PyResult<RawEvent> {
    let chain_str: String = raw
        .get_item("chain")?
        .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing 'chain'"))?
        .extract()?;

    let topics: Vec<String> = raw
        .get_item("topics")?
        .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing 'topics'"))?
        .extract()?;

    let data_hex: String = raw
        .get_item("data")?
        .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyKeyError, _>("missing 'data'"))?
        .extract()?;

    let data = hex_str_to_bytes(&data_hex)?;

    Ok(RawEvent {
        chain: chain_from_str(&chain_str),
        tx_hash: raw
            .get_item("tx_hash")?
            .map(|v| v.extract::<String>())
            .unwrap_or(Ok(String::new()))?,
        block_number: raw
            .get_item("block_number")?
            .map(|v| v.extract::<u64>())
            .unwrap_or(Ok(0))?,
        block_timestamp: raw
            .get_item("block_timestamp")?
            .map(|v| v.extract::<i64>())
            .unwrap_or(Ok(0))?,
        log_index: raw
            .get_item("log_index")?
            .map(|v| v.extract::<u32>())
            .unwrap_or(Ok(0))?,
        address: raw
            .get_item("address")?
            .map(|v| v.extract::<String>())
            .unwrap_or(Ok(String::new()))?,
        topics,
        data,
        raw_receipt: None,
    })
}

fn chain_from_str(s: &str) -> chaincodec_core::chain::ChainId {
    match s.to_lowercase().as_str() {
        "ethereum" | "eth" | "mainnet" => chains::ethereum(),
        "arbitrum" | "arb" => chains::arbitrum(),
        "base" => chains::base(),
        "polygon" | "matic" => chains::polygon(),
        "optimism" | "op" => chains::optimism(),
        "avalanche" | "avax" => chaincodec_core::chain::ChainId::evm("avalanche", 43114),
        "bsc" | "bnb" => chaincodec_core::chain::ChainId::evm("bsc", 56),
        _ => chains::ethereum(),
    }
}

fn normalized_to_py<'py>(py: Python<'py>, val: &NormalizedValue) -> PyResult<PyObject> {
    Ok(match val {
        NormalizedValue::Uint(v) => v.to_object(py),
        NormalizedValue::BigUint(s) => s.to_object(py),
        NormalizedValue::Int(v) => v.to_object(py),
        NormalizedValue::BigInt(s) => s.to_object(py),
        NormalizedValue::Bool(b) => b.to_object(py),
        NormalizedValue::Str(s) => s.to_object(py),
        NormalizedValue::Address(a) => a.to_object(py),
        NormalizedValue::Bytes(b) => {
            format!("0x{}", hex::encode(b)).to_object(py)
        }
        NormalizedValue::Hash256(h) => h.to_object(py),
        NormalizedValue::Timestamp(t) => t.to_object(py),
        NormalizedValue::Array(elems) => {
            let list = PyList::empty(py);
            for e in elems {
                list.append(normalized_to_py(py, e)?)?;
            }
            list.into()
        }
        NormalizedValue::Tuple(fields) => {
            let d = PyDict::new(py);
            for (k, v) in fields {
                d.set_item(k, normalized_to_py(py, v)?)?;
            }
            d.into()
        }
        NormalizedValue::Null => py.None(),
        NormalizedValue::Pubkey(s) | NormalizedValue::Bech32(s) => s.to_object(py),
    })
}

fn normalized_map_to_py<'py>(
    py: Python<'py>,
    map: &HashMap<String, NormalizedValue>,
) -> PyResult<PyObject> {
    let d = PyDict::new(py);
    for (k, v) in map {
        d.set_item(k, normalized_to_py(py, v)?)?;
    }
    Ok(d.into())
}
