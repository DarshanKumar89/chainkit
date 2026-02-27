//! # chaincodec-wasm
//!
//! WebAssembly (WASM) bindings for ChainCodec.
//! Built with wasm-bindgen — runs in modern browsers and Node.js WASM runtime.
//!
//! ## Usage (browser / Node.js WASM)
//! ```javascript
//! import init, { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec-wasm';
//!
//! await init();
//!
//! const registry = new MemoryRegistry();
//! registry.loadCsdl(`schema ERC20Transfer: ...`);
//!
//! const decoder = new EvmDecoder();
//! const eventJson = decoder.decodeEventJson(JSON.stringify({
//!   chain: "ethereum",
//!   txHash: "0x...",
//!   blockNumber: 19000000,
//!   blockTimestamp: 1700000000,
//!   logIndex: 0,
//!   address: "0x...",
//!   topics: ["0xddf252ad...", ...],
//!   data: "0x..."
//! }), registry);
//! const event = JSON.parse(eventJson);
//! ```

use wasm_bindgen::prelude::*;

use chaincodec_evm::{
    EvmDecoder as RustEvmDecoder,
    EvmCallDecoder as RustCallDecoder,
    EvmEncoder as RustEncoder,
    Eip712Parser as RustEip712Parser,
};
use chaincodec_registry::{CsdlParser, MemoryRegistry as RustRegistry};
use chaincodec_core::{
    chain::chains,
    decoder::{ChainDecoder, ErrorMode},
    event::RawEvent,
    schema::SchemaRegistry,
};
use serde::{Deserialize, Serialize};

// ─── Panic hook setup ─────────────────────────────────────────────────────────

#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// ─── JS-facing raw event type ─────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JsRawEvent {
    chain: String,
    tx_hash: String,
    block_number: u64,
    block_timestamp: i64,
    log_index: u32,
    address: String,
    topics: Vec<String>,
    data: String,  // 0x-prefixed hex
}

// ─── MemoryRegistry ──────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct MemoryRegistry {
    inner: RustRegistry,
}

#[wasm_bindgen]
impl MemoryRegistry {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { inner: RustRegistry::new() }
    }

    /// Load schemas from a CSDL YAML string.
    #[wasm_bindgen(js_name = "loadCsdl")]
    pub fn load_csdl(&mut self, csdl: &str) -> Result<u32, JsError> {
        let schemas = CsdlParser::parse_all(csdl)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let count = schemas.len() as u32;
        for schema in schemas {
            self.inner.insert(schema)
                .map_err(|e| JsError::new(&e.to_string()))?;
        }
        Ok(count)
    }

    /// Returns the number of registered schemas.
    #[wasm_bindgen(getter, js_name = "schemaCount")]
    pub fn schema_count(&self) -> u32 {
        self.inner.len() as u32
    }

    /// Returns all schema names as a JSON array string.
    #[wasm_bindgen(js_name = "schemaNamesJson")]
    pub fn schema_names_json(&self) -> String {
        let names = self.inner.all_names();
        serde_json::to_string(&names).unwrap_or_default()
    }
}

impl Default for MemoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── EvmDecoder ───────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct EvmDecoder {
    inner: RustEvmDecoder,
}

#[wasm_bindgen]
impl EvmDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { inner: RustEvmDecoder::new() }
    }

    /// Decode a single event. Input/output are JSON strings.
    #[wasm_bindgen(js_name = "decodeEventJson")]
    pub fn decode_event_json(
        &self,
        raw_json: &str,
        registry: &MemoryRegistry,
    ) -> Result<String, JsError> {
        let js_raw: JsRawEvent = serde_json::from_str(raw_json)
            .map_err(|e| JsError::new(&format!("parse raw event: {e}")))?;

        let rust_raw = js_raw_to_rust(js_raw)
            .map_err(|e| JsError::new(&e))?;

        let fp = self.inner.fingerprint(&rust_raw);
        let schema = registry.inner.get_by_fingerprint(&fp)
            .ok_or_else(|| JsError::new(&format!("schema not found for {}", fp.as_hex())))?;

        let decoded = self.inner.decode_event(&rust_raw, &schema)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&decoded)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Decode a batch of events. Input/output are JSON strings.
    #[wasm_bindgen(js_name = "decodeBatchJson")]
    pub fn decode_batch_json(
        &self,
        raws_json: &str,
        registry: &MemoryRegistry,
    ) -> Result<String, JsError> {
        let js_raws: Vec<JsRawEvent> = serde_json::from_str(raws_json)
            .map_err(|e| JsError::new(&format!("parse raw events: {e}")))?;

        let rust_raws: Result<Vec<RawEvent>, String> = js_raws
            .into_iter()
            .map(js_raw_to_rust)
            .collect();

        let rust_raws = rust_raws.map_err(|e| JsError::new(&e))?;

        let result = self.inner
            .decode_batch(&rust_raws, &registry.inner, ErrorMode::Collect, None)
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_json::to_string(&result)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Compute event fingerprint from raw event JSON.
    #[wasm_bindgen(js_name = "fingerprintJson")]
    pub fn fingerprint_json(&self, raw_json: &str) -> Result<String, JsError> {
        let js_raw: JsRawEvent = serde_json::from_str(raw_json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let rust_raw = js_raw_to_rust(js_raw)
            .map_err(|e| JsError::new(&e))?;
        Ok(self.inner.fingerprint(&rust_raw).as_hex().to_string())
    }
}

impl Default for EvmDecoder {
    fn default() -> Self {
        Self::new()
    }
}

// ─── EvmCallDecoder ───────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct EvmCallDecoder {
    inner: RustCallDecoder,
}

#[wasm_bindgen]
impl EvmCallDecoder {
    /// Create from Ethereum ABI JSON string.
    #[wasm_bindgen(js_name = "fromAbiJson")]
    pub fn from_abi_json(abi_json: &str) -> Result<EvmCallDecoder, JsError> {
        let inner = RustCallDecoder::from_abi_json(abi_json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Decode calldata. Returns JSON string.
    #[wasm_bindgen(js_name = "decodeCallJson")]
    pub fn decode_call_json(
        &self,
        calldata_hex: &str,
        function_name: Option<String>,
    ) -> Result<String, JsError> {
        let bytes = hex_to_bytes(calldata_hex)
            .map_err(|e| JsError::new(&e))?;
        let decoded = self.inner
            .decode_call(&bytes, function_name.as_deref())
            .map_err(|e| JsError::new(&e.to_string()))?;
        serde_json::to_string(&decoded)
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Returns function names as JSON array.
    #[wasm_bindgen(js_name = "functionNamesJson")]
    pub fn function_names_json(&self) -> String {
        let names: Vec<String> = self.inner.function_names()
            .into_iter().map(|s| s.to_string()).collect();
        serde_json::to_string(&names).unwrap_or_default()
    }

    /// Returns the 4-byte selector for a function.
    #[wasm_bindgen(js_name = "selectorFor")]
    pub fn selector_for(&self, name: &str) -> Option<String> {
        self.inner.selector_for(name).map(|s| format!("0x{}", hex::encode(s)))
    }
}

// ─── EvmEncoder ───────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct EvmEncoder {
    inner: RustEncoder,
}

#[wasm_bindgen]
impl EvmEncoder {
    #[wasm_bindgen(js_name = "fromAbiJson")]
    pub fn from_abi_json(abi_json: &str) -> Result<EvmEncoder, JsError> {
        let inner = RustEncoder::from_abi_json(abi_json)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    /// Encode a function call. `args_json` is a JSON array of NormalizedValues.
    /// Returns 0x-prefixed hex calldata.
    #[wasm_bindgen(js_name = "encodeCall")]
    pub fn encode_call(&self, function_name: &str, args_json: &str) -> Result<String, JsError> {
        let args: Vec<chaincodec_core::types::NormalizedValue> =
            serde_json::from_str(args_json)
                .map_err(|e| JsError::new(&format!("args: {e}")))?;
        let calldata = self.inner.encode_call(function_name, &args)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(format!("0x{}", hex::encode(calldata)))
    }
}

// ─── Eip712Parser ─────────────────────────────────────────────────────────────

#[wasm_bindgen]
pub struct Eip712Parser;

#[wasm_bindgen]
impl Eip712Parser {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Parse EIP-712 typed data JSON. Returns parsed structure as JSON string.
    #[wasm_bindgen(js_name = "parseJson")]
    pub fn parse_json(&self, json: &str) -> Result<String, JsError> {
        let td = RustEip712Parser::parse(json).map_err(|e| JsError::new(&e))?;
        serde_json::to_string(&td).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Compute the domain separator hash. Returns hex string.
    #[wasm_bindgen(js_name = "domainSeparator")]
    pub fn domain_separator(&self, json: &str) -> Result<String, JsError> {
        let td = RustEip712Parser::parse(json).map_err(|e| JsError::new(&e))?;
        Ok(RustEip712Parser::domain_separator_hex(&td))
    }
}

impl Default for Eip712Parser {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(hex).map_err(|e| format!("hex decode: {e}"))
}

fn js_raw_to_rust(raw: JsRawEvent) -> Result<RawEvent, String> {
    let data = hex_to_bytes(&raw.data)?;
    Ok(RawEvent {
        chain: chain_from_str(&raw.chain),
        tx_hash: raw.tx_hash,
        block_number: raw.block_number,
        block_timestamp: raw.block_timestamp,
        log_index: raw.log_index,
        address: raw.address,
        topics: raw.topics,
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
        "avalanche" | "avax" => chains::avalanche(),
        "bsc" | "bnb" => chains::bsc(),
        _ => chains::ethereum(),
    }
}
