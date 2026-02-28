//! # chainerrors-node
//!
//! Node.js / TypeScript bindings for ChainErrors — EVM revert decoder.
//! Built with napi-rs.
//!
//! ## Usage (TypeScript)
//! ```typescript
//! import { EvmErrorDecoder } from '@chainfoundry/chainerrors';
//!
//! const decoder = new EvmErrorDecoder();
//!
//! // Decode a revert string
//! const result = decoder.decode('0x08c379a0' + encodeString('Insufficient balance'));
//! console.log(result.kind);     // "revert_string"
//! console.log(result.message);  // "Insufficient balance"
//!
//! // Decode a Panic
//! const panic = decoder.decode('0x4e487b71' + encodePanic(0x11));
//! console.log(panic.kind);     // "panic"
//! console.log(panic.code);     // 17
//! console.log(panic.meaning);  // "arithmetic overflow or underflow"
//! ```

#![deny(clippy::all)]
#![allow(clippy::unnecessary_wraps)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use chainerrors_evm::EvmErrorDecoder as RustDecoder;
use chainerrors_core::decoder::ErrorDecoder;

// ─── Output types ─────────────────────────────────────────────────────────────

/// Decoded error result returned to JavaScript.
#[napi(object)]
pub struct JsDecodedError {
    /// Error kind: "revert_string" | "custom_error" | "panic" | "raw_revert" | "empty"
    pub kind: String,
    /// For revert_string: the revert message
    pub message: Option<String>,
    /// For custom_error: the error name (e.g. "InsufficientBalance")
    pub error_name: Option<String>,
    /// For custom_error: decoded inputs as JSON object { param: value, ... }
    pub inputs: Option<serde_json::Value>,
    /// For panic: the panic code as a number
    pub panic_code: Option<f64>,
    /// For panic: human-readable explanation of the panic code
    pub panic_meaning: Option<String>,
    /// Raw revert data as hex string (always present)
    pub raw_data: String,
    /// 4-byte selector as hex (null if empty or unknown)
    pub selector: Option<String>,
    /// Human-readable suggestion on how to fix (if known)
    pub suggestion: Option<String>,
    /// Confidence score 0.0-1.0 (1.0 = certain, <1.0 = ambiguous)
    pub confidence: f64,
}

// ─── EvmErrorDecoder ─────────────────────────────────────────────────────────

#[napi]
pub struct EvmErrorDecoder {
    inner: RustDecoder,
}

#[napi]
impl EvmErrorDecoder {
    /// Create a new EVM error decoder with the bundled signature registry.
    ///
    /// The bundled registry includes 500+ known error signatures from:
    /// ERC-20, ERC-721, OpenZeppelin, Uniswap V3, Aave, and more.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: RustDecoder::new(),
        }
    }

    /// Decode raw revert data into a structured error.
    ///
    /// `data` should be a hex string (with or without 0x prefix).
    ///
    /// Returns a `DecodedError` object with a `kind` field indicating the type:
    /// - `"revert_string"` — a `require("message")` revert
    /// - `"custom_error"` — a Solidity custom error (0.8.4+)
    /// - `"panic"` — a Solidity panic (assert failure, overflow, etc.)
    /// - `"raw_revert"` — unrecognized 4-byte selector
    /// - `"empty"` — no revert data (out of gas or plain revert())
    #[napi]
    pub fn decode(&self, data: String) -> Result<JsDecodedError> {
        let bytes = hex_to_bytes(&data)?;
        let result = self
            .inner
            .decode(&bytes, None)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(decoded_error_to_js(result))
    }

    /// Decode with optional transaction context for better error messages.
    ///
    /// `context` can include the contract address, function name, etc.
    #[napi]
    pub fn decode_with_context(
        &self,
        data: String,
        context_json: String,
    ) -> Result<JsDecodedError> {
        let bytes = hex_to_bytes(&data)?;
        let context: chainerrors_core::types::ErrorContext =
            serde_json::from_str(&context_json)
                .map_err(|e| Error::from_reason(format!("context parse: {e}")))?;

        let result = self
            .inner
            .decode(&bytes, Some(&context))
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(decoded_error_to_js(result))
    }

    /// Check if raw revert data is a known error selector.
    #[napi]
    pub fn is_known_error(&self, data: String) -> Result<bool> {
        let bytes = hex_to_bytes(&data)?;
        if bytes.len() < 4 {
            return Ok(false);
        }
        let selector: [u8; 4] = bytes[..4].try_into().unwrap();
        Ok(self.inner.is_known_selector(selector))
    }

    /// Get the human-readable panic description for a panic code.
    #[napi]
    pub fn panic_meaning(code: f64) -> String {
        chainerrors_evm::panic::panic_meaning(code as u64).to_string()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_to_bytes(s: &str) -> Result<Vec<u8>> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    if hex.is_empty() {
        return Ok(vec![]);
    }
    hex::decode(hex).map_err(|e| Error::from_reason(format!("hex decode: {e}")))
}

fn decoded_error_to_js(
    result: chainerrors_core::types::DecodedError,
) -> JsDecodedError {
    use chainerrors_core::types::ErrorKind;

    let raw_hex = format!("0x{}", hex::encode(&result.raw_data));
    let selector_hex = result
        .selector
        .map(|s| format!("0x{}", hex::encode(s)));

    match &result.kind {
        ErrorKind::RevertString { message } => JsDecodedError {
            kind: "revert_string".into(),
            message: Some(message.clone()),
            error_name: None,
            inputs: None,
            panic_code: None,
            panic_meaning: None,
            raw_data: raw_hex,
            selector: selector_hex,
            suggestion: result.suggestion.clone(),
            confidence: result.confidence as f64,
        },
        ErrorKind::CustomError { name, inputs } => {
            let inputs_json = serde_json::to_value(inputs).ok();
            JsDecodedError {
                kind: "custom_error".into(),
                message: None,
                error_name: Some(name.clone()),
                inputs: inputs_json,
                panic_code: None,
                panic_meaning: None,
                raw_data: raw_hex,
                selector: selector_hex,
                suggestion: result.suggestion.clone(),
                confidence: result.confidence as f64,
            }
        }
        ErrorKind::Panic { code, meaning } => JsDecodedError {
            kind: "panic".into(),
            message: None,
            error_name: None,
            inputs: None,
            panic_code: Some(*code as f64),
            panic_meaning: Some(meaning.to_string()),
            raw_data: raw_hex,
            selector: selector_hex,
            suggestion: result.suggestion.clone(),
            confidence: result.confidence as f64,
        },
        ErrorKind::RawRevert { data: _ } => JsDecodedError {
            kind: "raw_revert".into(),
            message: None,
            error_name: None,
            inputs: None,
            panic_code: None,
            panic_meaning: None,
            raw_data: raw_hex,
            selector: selector_hex,
            suggestion: None,
            confidence: result.confidence as f64,
        },
        ErrorKind::Empty => JsDecodedError {
            kind: "empty".into(),
            message: None,
            error_name: None,
            inputs: None,
            panic_code: None,
            panic_meaning: None,
            raw_data: raw_hex,
            selector: None,
            suggestion: Some("No revert data — likely out of gas or plain revert()".into()),
            confidence: 1.0,
        },
    }
}
