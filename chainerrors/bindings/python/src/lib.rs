//! # chainerrors (Python)
//!
//! PyO3-based Python bindings for ChainErrors — EVM revert decoder.
//!
//! ## Usage
//! ```python
//! from chainerrors import EvmErrorDecoder
//!
//! decoder = EvmErrorDecoder()
//!
//! # Decode a revert string
//! result = decoder.decode("0x08c379a0" + encode_abi_string("Insufficient balance"))
//! print(result["kind"])     # "revert_string"
//! print(result["message"])  # "Insufficient balance"
//!
//! # Decode a Panic(0x11) — arithmetic overflow
//! result = decoder.decode("0x4e487b71" + encode_abi_uint256(0x11))
//! print(result["kind"])        # "panic"
//! print(result["panic_code"])  # 17
//! print(result["panic_meaning"])  # "arithmetic overflow or underflow"
//!
//! # Decode a known custom error
//! result = decoder.decode("0xe450d38c...")
//! print(result["kind"])        # "custom_error"
//! print(result["error_name"])  # "ERC20InsufficientBalance"
//! print(result["inputs"])      # {"sender": "0x...", "balance": 0, "needed": 1000}
//! ```

use pyo3::prelude::*;
use pyo3::types::PyDict;

use chainerrors_evm::EvmErrorDecoder as RustDecoder;
use chainerrors_core::decoder::ErrorDecoder;
use chainerrors_core::types::ErrorKind;

// ─── EvmErrorDecoder ─────────────────────────────────────────────────────────

#[pyclass(name = "EvmErrorDecoder")]
pub struct PyEvmErrorDecoder {
    inner: RustDecoder,
}

#[pymethods]
impl PyEvmErrorDecoder {
    /// Create a new EVM error decoder with the bundled signature registry.
    ///
    /// The registry includes 500+ known error signatures from ERC-20, ERC-721,
    /// OpenZeppelin, Uniswap, Aave, and more.
    #[new]
    fn new() -> Self {
        Self {
            inner: RustDecoder::new(),
        }
    }

    /// Decode raw revert data (hex string, with or without 0x prefix).
    ///
    /// Returns a dict with:
    /// - `kind`: "revert_string" | "custom_error" | "panic" | "raw_revert" | "empty"
    /// - `message` (str): present for "revert_string"
    /// - `error_name` (str): present for "custom_error"
    /// - `inputs` (dict): present for "custom_error" with decoded parameters
    /// - `panic_code` (int): present for "panic"
    /// - `panic_meaning` (str): present for "panic"
    /// - `raw_data` (str): always present as 0x-prefixed hex
    /// - `selector` (str | None): 4-byte selector as 0x hex
    /// - `suggestion` (str | None): human-readable hint to fix the error
    /// - `confidence` (float): 0.0-1.0 decode confidence
    fn decode<'py>(&self, py: Python<'py>, data: &str) -> PyResult<PyObject> {
        let bytes = hex_str_to_bytes(data)?;
        let result = self
            .inner
            .decode(&bytes, None)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        decoded_to_py(py, result)
    }

    /// Decode with context dict (optional contract_address, function_name, etc.)
    fn decode_with_context<'py>(
        &self,
        py: Python<'py>,
        data: &str,
        context: &PyDict,
    ) -> PyResult<PyObject> {
        let bytes = hex_str_to_bytes(data)?;
        let context_json = dict_to_json(context)?;
        let context: chainerrors_core::types::ErrorContext =
            serde_json::from_str(&context_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

        let result = self
            .inner
            .decode(&bytes, Some(&context))
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        decoded_to_py(py, result)
    }

    /// Returns True if this data matches a known error selector in the registry.
    fn is_known_error(&self, data: &str) -> PyResult<bool> {
        let bytes = hex_str_to_bytes(data)?;
        if bytes.len() < 4 {
            return Ok(false);
        }
        let selector: [u8; 4] = bytes[..4].try_into().unwrap();
        Ok(self.inner.is_known_selector(selector))
    }

    fn __repr__(&self) -> &'static str {
        "EvmErrorDecoder()"
    }
}

// ─── Module ───────────────────────────────────────────────────────────────────

#[pymodule]
fn chainerrors(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<PyEvmErrorDecoder>()?;

    // Convenience function
    m.add_function(wrap_pyfunction!(decode_error, m)?)?;
    m.add_function(wrap_pyfunction!(panic_meaning, m)?)?;
    Ok(())
}

/// Convenience: decode revert data without instantiating a decoder object.
#[pyfunction]
fn decode_error(py: Python<'_>, data: &str) -> PyResult<PyObject> {
    let decoder = RustDecoder::new();
    let bytes = hex_str_to_bytes(data)?;
    let result = decoder
        .decode(&bytes, None)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
    decoded_to_py(py, result)
}

/// Get the human-readable description for a Solidity panic code.
#[pyfunction]
fn panic_meaning(code: u64) -> String {
    chainerrors_evm::panic::panic_meaning(code).to_string()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_str_to_bytes(s: &str) -> PyResult<Vec<u8>> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    if hex.is_empty() {
        return Ok(vec![]);
    }
    hex::decode(hex)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("hex decode: {e}")))
}

fn dict_to_json(d: &PyDict) -> PyResult<String> {
    let mut map = serde_json::Map::new();
    for (k, v) in d.iter() {
        let key: String = k.extract()?;
        let val: String = v.extract().unwrap_or_else(|_| v.to_string());
        map.insert(key, serde_json::Value::String(val));
    }
    Ok(serde_json::Value::Object(map).to_string())
}

fn decoded_to_py(
    py: Python<'_>,
    result: chainerrors_core::types::DecodedError,
) -> PyResult<PyObject> {
    let d = PyDict::new(py);
    let raw_hex = format!("0x{}", hex::encode(&result.raw_data));
    let selector_hex = result
        .selector
        .map(|s| format!("0x{}", hex::encode(s)));

    d.set_item("raw_data", &raw_hex)?;
    d.set_item("selector", selector_hex)?;
    d.set_item("suggestion", result.suggestion.as_deref())?;
    d.set_item("confidence", result.confidence)?;

    match &result.kind {
        ErrorKind::RevertString { message } => {
            d.set_item("kind", "revert_string")?;
            d.set_item("message", message)?;
            d.set_item("error_name", py.None())?;
            d.set_item("inputs", py.None())?;
            d.set_item("panic_code", py.None())?;
            d.set_item("panic_meaning", py.None())?;
        }
        ErrorKind::CustomError { name, inputs } => {
            d.set_item("kind", "custom_error")?;
            d.set_item("message", py.None())?;
            d.set_item("error_name", name)?;
            let inputs_json = serde_json::to_string(inputs).unwrap_or_else(|_| "{}".into());
            d.set_item("inputs", inputs_json)?;
            d.set_item("panic_code", py.None())?;
            d.set_item("panic_meaning", py.None())?;
        }
        ErrorKind::Panic { code, meaning } => {
            d.set_item("kind", "panic")?;
            d.set_item("message", py.None())?;
            d.set_item("error_name", py.None())?;
            d.set_item("inputs", py.None())?;
            d.set_item("panic_code", *code)?;
            d.set_item("panic_meaning", meaning.to_string())?;
        }
        ErrorKind::RawRevert { data: _ } => {
            d.set_item("kind", "raw_revert")?;
            d.set_item("message", py.None())?;
            d.set_item("error_name", py.None())?;
            d.set_item("inputs", py.None())?;
            d.set_item("panic_code", py.None())?;
            d.set_item("panic_meaning", py.None())?;
        }
        ErrorKind::Empty => {
            d.set_item("kind", "empty")?;
            d.set_item("message", py.None())?;
            d.set_item("error_name", py.None())?;
            d.set_item("inputs", py.None())?;
            d.set_item("panic_code", py.None())?;
            d.set_item("panic_meaning", py.None())?;
        }
    }

    Ok(d.into())
}
