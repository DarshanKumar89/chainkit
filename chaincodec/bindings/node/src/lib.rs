//! # chaincodec-node
//!
//! Node.js / TypeScript bindings for ChainCodec.
//! Built with napi-rs — zero-copy Rust ↔ JavaScript bridge.
//!
//! ## Exported classes
//! - `EvmDecoder`       — EVM event decoder
//! - `EvmCallDecoder`   — Function call / constructor decoder
//! - `EvmEncoder`       — ABI encoder
//! - `MemoryRegistry`   — In-memory schema registry
//! - `Eip712Parser`     — EIP-712 typed data parser
//!
//! ## Usage (TypeScript)
//! ```typescript
//! import { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec';
//!
//! const registry = new MemoryRegistry();
//! registry.loadCsdl(`
//! schema ERC20Transfer:
//!   ...
//! `);
//!
//! const decoder = new EvmDecoder();
//! const event = decoder.decodeEvent({
//!   topics: ["0xddf252ad...", ...],
//!   data: "0x...",
//!   chain: "ethereum",
//!   txHash: "0x...",
//!   blockNumber: 19000000,
//!   blockTimestamp: 1700000000,
//!   logIndex: 0,
//!   address: "0x...",
//! }, registry);
//! console.log(event.fields);
//! ```

#![deny(clippy::all)]
#![allow(clippy::unnecessary_wraps)]

use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::collections::HashMap;

use chaincodec_evm::{EvmDecoder as RustEvmDecoder, EvmCallDecoder as RustCallDecoder, EvmEncoder as RustEncoder, Eip712Parser as RustEip712Parser};
use chaincodec_registry::{CsdlParser, MemoryRegistry as RustRegistry};
use chaincodec_core::{
    chain::chains,
    decoder::{ChainDecoder, ErrorMode},
    event::RawEvent,
    schema::SchemaRegistry,
};

// ─── RawLog input ─────────────────────────────────────────────────────────────

#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsRawEvent {
    pub chain: String,
    pub tx_hash: String,
    pub block_number: f64,   // JS numbers are f64
    pub block_timestamp: f64,
    pub log_index: f64,
    pub address: String,
    pub topics: Vec<String>,
    pub data: String,        // hex string with 0x prefix
}

// ─── Decoded event output ─────────────────────────────────────────────────────

#[napi(object)]
pub struct JsDecodedEvent {
    pub schema: String,
    pub schema_version: f64,
    pub chain: String,
    pub tx_hash: String,
    pub block_number: f64,
    pub block_timestamp: f64,
    pub log_index: f64,
    pub address: String,
    pub fields: serde_json::Value,
    pub fingerprint: String,
    pub decode_errors: serde_json::Value,
}

// ─── Decoded call output ─────────────────────────────────────────────────────

#[napi(object)]
pub struct JsDecodedCall {
    pub function_name: String,
    pub selector: Option<String>,
    pub inputs: serde_json::Value,
    pub decode_errors: serde_json::Value,
}

// ─── MemoryRegistry ──────────────────────────────────────────────────────────

#[napi]
pub struct MemoryRegistry {
    inner: RustRegistry,
}

#[napi]
impl MemoryRegistry {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: RustRegistry::new(),
        }
    }

    /// Load schemas from a CSDL YAML string.
    ///
    /// Supports multi-document CSDL files (separated by `---`).
    #[napi]
    pub fn load_csdl(&mut self, csdl: String) -> Result<u32> {
        let schemas = CsdlParser::parse_all(&csdl)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        let count = schemas.len() as u32;
        for schema in schemas {
            self.inner
                .insert(schema)
                .map_err(|e| Error::from_reason(e.to_string()))?;
        }
        Ok(count)
    }

    /// Load schemas from a CSDL file path.
    #[napi]
    pub fn load_file(&mut self, path: String) -> Result<u32> {
        self.inner
            .load_file(std::path::Path::new(&path))
            .map_err(|e| Error::from_reason(e.to_string()))
            .map(|n| n as u32)
    }

    /// Load all CSDL files from a directory.
    #[napi]
    pub fn load_directory(&mut self, path: String) -> Result<u32> {
        self.inner
            .load_directory(std::path::Path::new(&path))
            .map_err(|e| Error::from_reason(e.to_string()))
            .map(|n| n as u32)
    }

    /// Returns the number of schemas currently registered.
    #[napi(getter)]
    pub fn schema_count(&self) -> u32 {
        self.inner.len() as u32
    }

    /// List all schema names.
    #[napi]
    pub fn schema_names(&self) -> Vec<String> {
        self.inner.all_names()
    }
}

// ─── EvmDecoder ───────────────────────────────────────────────────────────────

#[napi]
pub struct EvmDecoder {
    inner: RustEvmDecoder,
}

#[napi]
impl EvmDecoder {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: RustEvmDecoder::new(),
        }
    }

    /// Decode a single EVM event log.
    ///
    /// Returns a `DecodedEvent` object with `fields` as a plain JS object.
    #[napi]
    pub fn decode_event(
        &self,
        raw: JsRawEvent,
        registry: &MemoryRegistry,
    ) -> Result<JsDecodedEvent> {
        let chain = chain_from_str(&raw.chain);
        let data = hex_to_bytes(&raw.data)?;

        let rust_raw = RawEvent {
            chain,
            tx_hash: raw.tx_hash.clone(),
            block_number: raw.block_number as u64,
            block_timestamp: raw.block_timestamp as u64,
            log_index: raw.log_index as u32,
            address: raw.address.clone(),
            topics: raw.topics.clone(),
            data,
            raw_receipt: None,
        };

        let fp = self.inner.fingerprint(&rust_raw);
        let schema = registry
            .inner
            .get_by_fingerprint(&fp)
            .ok_or_else(|| Error::from_reason(format!("schema not found for fingerprint {}", fp.as_hex())))?;

        let decoded = self
            .inner
            .decode_event(&rust_raw, &schema)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        let fields = serde_json::to_value(&decoded.fields)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        let errors = serde_json::to_value(&decoded.decode_errors)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(JsDecodedEvent {
            schema: decoded.schema,
            schema_version: decoded.schema_version as f64,
            chain: raw.chain,
            tx_hash: decoded.tx_hash,
            block_number: decoded.block_number as f64,
            block_timestamp: decoded.block_timestamp as f64,
            log_index: decoded.log_index as f64,
            address: decoded.address,
            fields,
            fingerprint: decoded.fingerprint.as_hex().to_string(),
            decode_errors: errors,
        })
    }

    /// Compute the event fingerprint (keccak256 of topics[0]).
    #[napi]
    pub fn fingerprint(&self, raw: JsRawEvent) -> String {
        let chain = chain_from_str(&raw.chain);
        let rust_raw = RawEvent {
            chain,
            tx_hash: raw.tx_hash,
            block_number: raw.block_number as u64,
            block_timestamp: raw.block_timestamp as u64,
            log_index: raw.log_index as u32,
            address: raw.address,
            topics: raw.topics,
            data: vec![],
            raw_receipt: None,
        };
        self.inner.fingerprint(&rust_raw).as_hex().to_string()
    }

    /// Decode a batch of events in parallel using Rayon.
    ///
    /// Returns `{ events: DecodedEvent[], errors: { index: number, error: string }[] }`.
    #[napi]
    pub fn decode_batch(
        &self,
        raws: Vec<JsRawEvent>,
        registry: &MemoryRegistry,
    ) -> Result<serde_json::Value> {
        let rust_raws: Result<Vec<RawEvent>> = raws
            .iter()
            .map(|raw| {
                let data = hex_to_bytes(&raw.data)?;
                Ok(RawEvent {
                    chain: chain_from_str(&raw.chain),
                    tx_hash: raw.tx_hash.clone(),
                    block_number: raw.block_number as u64,
                    block_timestamp: raw.block_timestamp as u64,
                    log_index: raw.log_index as u32,
                    address: raw.address.clone(),
                    topics: raw.topics.clone(),
                    data,
                    raw_receipt: None,
                })
            })
            .collect();

        let rust_raws = rust_raws?;
        let result = self
            .inner
            .decode_batch(&rust_raws, &registry.inner, ErrorMode::Collect, None)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        let events: Vec<serde_json::Value> = result
            .events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(serde_json::Value::Null))
            .collect();

        let errors: Vec<serde_json::Value> = result
            .errors
            .iter()
            .map(|(idx, err)| {
                serde_json::json!({ "index": idx, "error": err.to_string() })
            })
            .collect();

        Ok(serde_json::json!({ "events": events, "errors": errors }))
    }
}

// ─── EvmCallDecoder ───────────────────────────────────────────────────────────

#[napi]
pub struct EvmCallDecoder {
    inner: RustCallDecoder,
}

#[napi]
impl EvmCallDecoder {
    /// Create a call decoder from a standard Ethereum ABI JSON string.
    #[napi(factory)]
    pub fn from_abi_json(abi_json: String) -> Result<Self> {
        let inner = RustCallDecoder::from_abi_json(&abi_json)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Decode function call calldata.
    ///
    /// `calldata` should be a hex string (with or without 0x prefix).
    #[napi]
    pub fn decode_call(
        &self,
        calldata: String,
        function_name: Option<String>,
    ) -> Result<JsDecodedCall> {
        let bytes = hex_to_bytes(&calldata)?;
        let decoded = self
            .inner
            .decode_call(&bytes, function_name.as_deref())
            .map_err(|e| Error::from_reason(e.to_string()))?;

        let inputs = serde_json::to_value(&decoded.inputs)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        let errors = serde_json::to_value(&decoded.decode_errors)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(JsDecodedCall {
            function_name: decoded.function_name,
            selector: decoded.selector_hex(),
            inputs,
            decode_errors: errors,
        })
    }

    /// Returns function names in this ABI.
    #[napi]
    pub fn function_names(&self) -> Vec<String> {
        self.inner.function_names().into_iter().map(|s| s.to_string()).collect()
    }

    /// Returns the 4-byte selector for a function name.
    #[napi]
    pub fn selector_for(&self, function_name: String) -> Option<String> {
        self.inner
            .selector_for(&function_name)
            .map(|s| format!("0x{}", hex::encode(s)))
    }
}

// ─── EvmEncoder ───────────────────────────────────────────────────────────────

#[napi]
pub struct EvmEncoder {
    inner: RustEncoder,
}

#[napi]
impl EvmEncoder {
    /// Create an encoder from a standard Ethereum ABI JSON string.
    #[napi(factory)]
    pub fn from_abi_json(abi_json: String) -> Result<Self> {
        let inner = RustEncoder::from_abi_json(&abi_json)
            .map_err(|e| Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Encode a function call.
    ///
    /// `args` should be an array of values that will be JSON-serialized and
    /// then converted to the appropriate Solidity types based on the ABI.
    #[napi]
    pub fn encode_call(
        &self,
        function_name: String,
        args_json: String,
    ) -> Result<String> {
        let args: Vec<chaincodec_core::types::NormalizedValue> =
            serde_json::from_str(&args_json)
                .map_err(|e| Error::from_reason(format!("args parse: {e}")))?;

        let calldata = self
            .inner
            .encode_call(&function_name, &args)
            .map_err(|e| Error::from_reason(e.to_string()))?;

        Ok(format!("0x{}", hex::encode(calldata)))
    }
}

// ─── Eip712Parser ─────────────────────────────────────────────────────────────

#[napi]
pub struct Eip712Parser;

#[napi]
impl Eip712Parser {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// Parse an EIP-712 typed data JSON string.
    ///
    /// Returns the parsed typed data as a JavaScript object.
    #[napi]
    pub fn parse(&self, json: String) -> Result<serde_json::Value> {
        let td = RustEip712Parser::parse(&json)
            .map_err(|e| Error::from_reason(e))?;
        serde_json::to_value(&td).map_err(|e| Error::from_reason(e.to_string()))
    }

    /// Compute the domain separator hash.
    #[napi]
    pub fn domain_separator(&self, json: String) -> Result<String> {
        let td = RustEip712Parser::parse(&json)
            .map_err(|e| Error::from_reason(e))?;
        Ok(RustEip712Parser::domain_separator_hex(&td))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_to_bytes(s: &str) -> Result<Vec<u8>> {
    let hex = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(hex).map_err(|e| Error::from_reason(format!("hex decode: {e}")))
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
        _ => {
            // Try to parse as numeric chain ID
            if let Ok(id) = s.parse::<u64>() {
                chaincodec_core::chain::ChainId::from_numeric(id)
                    .unwrap_or_else(|| chains::ethereum())
            } else {
                chains::ethereum()
            }
        }
    }
}
