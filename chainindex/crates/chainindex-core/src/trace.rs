//! Call trace indexing — index internal transactions (CALL, DELEGATECALL, CREATE)
//! from `debug_traceBlockByNumber` or `trace_block` responses.
//!
//! # Overview
//!
//! Ethereum event logs only capture explicitly emitted events. Many important
//! operations (ETH transfers via internal calls, contract creations, delegate
//! calls) are only visible in execution traces. This module provides:
//!
//! - [`CallTrace`] — a structured representation of a single internal call.
//! - [`TraceFilter`] — declarative filtering by address, selector, call type.
//! - [`TraceHandler`] — async trait for user-provided trace processing logic.
//! - [`TraceRegistry`] — handler dispatch with filtering.
//! - [`parse_geth_traces`] — parse Geth `debug_traceBlockByNumber` JSON.
//! - [`parse_parity_traces`] — parse OpenEthereum/Parity `trace_block` JSON.
//!
//! # Example
//!
//! ```rust,no_run
//! use chainindex_core::trace::{CallTrace, TraceFilter, CallType};
//!
//! let filter = TraceFilter::new()
//!     .with_address("0xdead")
//!     .with_call_type(CallType::Call)
//!     .exclude_reverted(true);
//! ```

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::IndexerError;
use crate::types::IndexContext;

// ─── CallType ───────────────────────────────────────────────────────────────

/// Type of EVM call/operation captured in a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CallType {
    /// Standard `CALL` opcode.
    Call,
    /// `DELEGATECALL` — executes callee code in caller's storage context.
    DelegateCall,
    /// `STATICCALL` — read-only call (reverts on state changes).
    StaticCall,
    /// `CREATE` opcode — deploys a new contract.
    Create,
    /// `CREATE2` opcode — deploys with deterministic address.
    Create2,
    /// `SELFDESTRUCT` opcode — destroys the contract.
    SelfDestruct,
}

impl CallType {
    /// Parse a call type string from Geth trace output.
    ///
    /// Geth uses uppercase strings like `"CALL"`, `"DELEGATECALL"`, etc.
    pub fn from_geth(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "CALL" => Some(Self::Call),
            "DELEGATECALL" => Some(Self::DelegateCall),
            "STATICCALL" => Some(Self::StaticCall),
            "CREATE" => Some(Self::Create),
            "CREATE2" => Some(Self::Create2),
            "SELFDESTRUCT" => Some(Self::SelfDestruct),
            _ => None,
        }
    }

    /// Parse a call type string from Parity/OpenEthereum trace output.
    ///
    /// Parity uses lowercase strings and a different naming convention:
    /// `"call"`, `"delegatecall"`, `"staticcall"`, `"create"`, `"suicide"`.
    pub fn from_parity(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "call" => Some(Self::Call),
            "delegatecall" => Some(Self::DelegateCall),
            "staticcall" => Some(Self::StaticCall),
            "create" => Some(Self::Create),
            "create2" => Some(Self::Create2),
            "suicide" | "selfdestruct" => Some(Self::SelfDestruct),
            _ => None,
        }
    }
}

// ─── CallTrace ──────────────────────────────────────────────────────────────

/// A single call trace (internal transaction).
///
/// Represents one node in the call tree of a transaction. Top-level external
/// calls have `depth = 0`; internal calls increment the depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallTrace {
    /// The type of call (CALL, DELEGATECALL, CREATE, etc.).
    pub call_type: CallType,
    /// Sender address (`0x…`).
    pub from: String,
    /// Recipient/target address (`0x…`). For CREATE, this is the new contract.
    pub to: String,
    /// Value transferred in wei (decimal string, e.g. `"1000000000000000000"`).
    pub value: String,
    /// Gas consumed by this call.
    pub gas_used: u64,
    /// Input data (hex-encoded with `0x` prefix).
    pub input: String,
    /// Output/return data (hex-encoded with `0x` prefix).
    pub output: String,
    /// First 4 bytes of input — the function selector (e.g. `"0xa9059cbb"`).
    /// `None` if input is too short (< 4 bytes after `0x` prefix).
    pub function_selector: Option<String>,
    /// Call depth: 0 for top-level, increments for nested calls.
    pub depth: u32,
    /// Block number containing the transaction.
    pub block_number: u64,
    /// Transaction hash.
    pub tx_hash: String,
    /// Transaction index within the block.
    pub tx_index: u32,
    /// Trace index within the transaction (sequential ordering).
    pub trace_index: u32,
    /// Error message if the call failed.
    pub error: Option<String>,
    /// Whether this call (or a parent call) was reverted.
    pub reverted: bool,
}

impl CallTrace {
    /// Extract the function selector (first 4 bytes) from hex-encoded input.
    ///
    /// Returns `None` if input is `"0x"` or shorter than 10 characters
    /// (i.e., `0x` + 8 hex chars = 4 bytes).
    pub fn extract_selector(input: &str) -> Option<String> {
        let hex = input.strip_prefix("0x").unwrap_or(input);
        if hex.len() >= 8 {
            Some(format!("0x{}", &hex[..8].to_lowercase()))
        } else {
            None
        }
    }
}

// ─── TraceFilter ────────────────────────────────────────────────────────────

/// Declarative filter for which traces to process.
///
/// All filter fields are "AND"-ed: a trace must match all non-empty criteria.
/// Empty/default fields match everything.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceFilter {
    /// Only include traces involving these addresses (as `from` or `to`).
    /// Empty = match all addresses.
    pub addresses: HashSet<String>,
    /// Only include traces with these function selectors.
    /// Empty = match all selectors.
    pub selectors: HashSet<String>,
    /// Only include these call types. Empty = match all types.
    pub call_types: HashSet<CallType>,
    /// If `true`, exclude traces that reverted.
    pub exclude_reverted: bool,
    /// Minimum call depth to include (e.g., 1 = skip top-level calls).
    pub min_depth: Option<u32>,
    /// Maximum call depth to include.
    pub max_depth: Option<u32>,
}

impl TraceFilter {
    /// Create a new empty filter (matches everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an address to filter on (as `from` or `to`).
    pub fn with_address(mut self, addr: impl Into<String>) -> Self {
        self.addresses.insert(addr.into().to_lowercase());
        self
    }

    /// Add a function selector to filter on.
    pub fn with_selector(mut self, selector: impl Into<String>) -> Self {
        self.selectors.insert(selector.into().to_lowercase());
        self
    }

    /// Add a call type to filter on.
    pub fn with_call_type(mut self, call_type: CallType) -> Self {
        self.call_types.insert(call_type);
        self
    }

    /// Set whether to exclude reverted traces.
    pub fn exclude_reverted(mut self, exclude: bool) -> Self {
        self.exclude_reverted = exclude;
        self
    }

    /// Set the minimum call depth.
    pub fn min_depth(mut self, depth: u32) -> Self {
        self.min_depth = Some(depth);
        self
    }

    /// Set the maximum call depth.
    pub fn max_depth(mut self, depth: u32) -> Self {
        self.max_depth = Some(depth);
        self
    }

    /// Check whether a trace matches this filter.
    pub fn matches(&self, trace: &CallTrace) -> bool {
        // Check reverted.
        if self.exclude_reverted && trace.reverted {
            return false;
        }

        // Check call type.
        if !self.call_types.is_empty() && !self.call_types.contains(&trace.call_type) {
            return false;
        }

        // Check addresses (from OR to).
        if !self.addresses.is_empty() {
            let from_lower = trace.from.to_lowercase();
            let to_lower = trace.to.to_lowercase();
            if !self.addresses.contains(&from_lower) && !self.addresses.contains(&to_lower) {
                return false;
            }
        }

        // Check function selector.
        if !self.selectors.is_empty() {
            match &trace.function_selector {
                Some(sel) => {
                    if !self.selectors.contains(&sel.to_lowercase()) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        // Check depth bounds.
        if let Some(min) = self.min_depth {
            if trace.depth < min {
                return false;
            }
        }
        if let Some(max) = self.max_depth {
            if trace.depth > max {
                return false;
            }
        }

        true
    }
}

// ─── TraceHandler ───────────────────────────────────────────────────────────

/// Trait for user-provided trace processing logic.
///
/// Implement this to react to call traces during indexing (e.g., track ETH
/// transfers, monitor contract creations, record internal function calls).
#[async_trait]
pub trait TraceHandler: Send + Sync {
    /// Called for each trace that passes the handler's filter.
    async fn handle_trace(
        &self,
        trace: &CallTrace,
        ctx: &IndexContext,
    ) -> Result<(), IndexerError>;

    /// Human-readable name for this handler (used in error messages and logging).
    fn name(&self) -> &str;
}

// ─── TraceRegistry ──────────────────────────────────────────────────────────

/// Entry in the trace registry: a handler paired with its filter.
struct TraceEntry {
    handler: Arc<dyn TraceHandler>,
    filter: TraceFilter,
}

/// Registry of trace handlers with their associated filters.
///
/// Register handlers with filters, then dispatch traces. Only traces that
/// match a handler's filter will be forwarded to that handler.
pub struct TraceRegistry {
    entries: Vec<TraceEntry>,
}

impl TraceRegistry {
    /// Create an empty trace registry.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a trace handler with its filter.
    pub fn register(&mut self, handler: Arc<dyn TraceHandler>, filter: TraceFilter) {
        self.entries.push(TraceEntry { handler, filter });
    }

    /// Dispatch a trace to all handlers whose filter matches.
    pub async fn dispatch(
        &self,
        trace: &CallTrace,
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        for entry in &self.entries {
            if entry.filter.matches(trace) {
                entry.handler.handle_trace(trace, ctx).await.map_err(|e| {
                    IndexerError::Handler {
                        handler: entry.handler.name().to_string(),
                        reason: e.to_string(),
                    }
                })?;
            }
        }
        Ok(())
    }

    /// Dispatch a batch of traces to all matching handlers.
    pub async fn dispatch_batch(
        &self,
        traces: &[CallTrace],
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        for trace in traces {
            self.dispatch(trace, ctx).await?;
        }
        Ok(())
    }

    /// Returns the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for TraceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Geth Trace Parsing ────────────────────────────────────────────────────

/// Parse a Geth `debug_traceBlockByNumber` response into [`CallTrace`] objects.
///
/// Geth's `callTracer` returns a tree of calls per transaction. This function
/// flattens the tree into a linear sequence with depth tracking.
///
/// # Arguments
///
/// * `json` — The JSON response from `debug_traceBlockByNumber` with
///   `{"tracer": "callTracer"}`. Expected format: array of `{"result": {...}}`.
/// * `block_number` — The block number (for populating `CallTrace::block_number`).
///
/// # Errors
///
/// Returns `IndexerError::Rpc` if the JSON structure is unexpected.
pub fn parse_geth_traces(
    json: &serde_json::Value,
    block_number: u64,
) -> Result<Vec<CallTrace>, IndexerError> {
    let results = json
        .as_array()
        .ok_or_else(|| IndexerError::Rpc("expected array of trace results".into()))?;

    let mut traces = Vec::new();

    for (tx_index, entry) in results.iter().enumerate() {
        // Each entry has { "txHash": "0x...", "result": { ... } }
        let tx_hash = entry
            .get("txHash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let result = entry.get("result").unwrap_or(entry);

        let mut trace_index: u32 = 0;
        flatten_geth_call(
            result,
            block_number,
            &tx_hash,
            tx_index as u32,
            0,     // depth
            false, // parent_reverted
            &mut trace_index,
            &mut traces,
        );
    }

    Ok(traces)
}

/// Recursively flatten a Geth callTracer node into the traces vector.
fn flatten_geth_call(
    node: &serde_json::Value,
    block_number: u64,
    tx_hash: &str,
    tx_index: u32,
    depth: u32,
    parent_reverted: bool,
    trace_index: &mut u32,
    out: &mut Vec<CallTrace>,
) {
    let call_type_str = node
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("CALL");

    let call_type = CallType::from_geth(call_type_str).unwrap_or(CallType::Call);

    let from = node
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let to = node
        .get("to")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let value = node
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0")
        .to_string();

    let gas_used = node
        .get("gasUsed")
        .and_then(|v| v.as_str())
        .and_then(|s| u64::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok())
        .unwrap_or(0);

    let input = node
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or("0x")
        .to_string();

    let output = node
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("0x")
        .to_string();

    let error = node.get("error").and_then(|v| v.as_str()).map(String::from);
    let reverted = parent_reverted || error.is_some();

    let function_selector = CallTrace::extract_selector(&input);

    let current_index = *trace_index;
    *trace_index += 1;

    out.push(CallTrace {
        call_type,
        from,
        to,
        value,
        gas_used,
        input,
        output,
        function_selector,
        depth,
        block_number,
        tx_hash: tx_hash.to_string(),
        tx_index,
        trace_index: current_index,
        error,
        reverted,
    });

    // Recurse into child calls.
    if let Some(calls) = node.get("calls").and_then(|v| v.as_array()) {
        for child in calls {
            flatten_geth_call(
                child,
                block_number,
                tx_hash,
                tx_index,
                depth + 1,
                reverted,
                trace_index,
                out,
            );
        }
    }
}

// ─── Parity Trace Parsing ──────────────────────────────────────────────────

/// Parse an OpenEthereum/Parity `trace_block` response into [`CallTrace`] objects.
///
/// Parity traces are flat arrays with a `traceAddress` field indicating depth.
/// Each trace has an `action` object with call details and a `result` object
/// with output.
///
/// # Arguments
///
/// * `json` — The JSON response from `trace_block`. Expected format: flat
///   array of trace objects.
/// * `block_number` — The block number (for populating `CallTrace::block_number`).
///
/// # Errors
///
/// Returns `IndexerError::Rpc` if the JSON structure is unexpected.
pub fn parse_parity_traces(
    json: &serde_json::Value,
    block_number: u64,
) -> Result<Vec<CallTrace>, IndexerError> {
    let traces_arr = json
        .as_array()
        .ok_or_else(|| IndexerError::Rpc("expected array of parity traces".into()))?;

    let mut traces = Vec::new();

    for (i, entry) in traces_arr.iter().enumerate() {
        let action = entry.get("action").unwrap_or(entry);

        let trace_type = entry
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("call");

        let call_type = CallType::from_parity(trace_type).unwrap_or(CallType::Call);

        let from = action
            .get("from")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        let to = action
            .get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        let value = action
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("0x0")
            .to_string();

        let gas_used = entry
            .get("result")
            .and_then(|r| r.get("gasUsed"))
            .and_then(|v| v.as_str())
            .and_then(|s| u64::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok())
            .unwrap_or(0);

        let input = action
            .get("input")
            .and_then(|v| v.as_str())
            .unwrap_or("0x")
            .to_string();

        let output = entry
            .get("result")
            .and_then(|r| r.get("output"))
            .and_then(|v| v.as_str())
            .unwrap_or("0x")
            .to_string();

        let tx_hash = entry
            .get("transactionHash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let tx_index = entry
            .get("transactionPosition")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        // Depth from traceAddress length.
        let depth = entry
            .get("traceAddress")
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        let error_str = entry.get("error").and_then(|v| v.as_str()).map(String::from);
        let reverted = error_str.is_some();

        let function_selector = CallTrace::extract_selector(&input);

        traces.push(CallTrace {
            call_type,
            from,
            to,
            value,
            gas_used,
            input,
            output,
            function_selector,
            depth,
            block_number,
            tx_hash,
            tx_index,
            trace_index: i as u32,
            error: error_str,
            reverted,
        });
    }

    Ok(traces)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn dummy_ctx() -> IndexContext {
        IndexContext {
            block: crate::types::BlockSummary {
                number: 1,
                hash: "0xa".into(),
                parent_hash: "0x0".into(),
                timestamp: 0,
                tx_count: 0,
            },
            phase: crate::types::IndexPhase::Backfill,
            chain: "ethereum".into(),
        }
    }

    fn make_trace(
        call_type: CallType,
        from: &str,
        to: &str,
        selector: Option<&str>,
        depth: u32,
        reverted: bool,
    ) -> CallTrace {
        let input = match selector {
            Some(sel) => format!("{}0000000000000000", sel),
            None => "0x".to_string(),
        };
        let function_selector = CallTrace::extract_selector(&input);
        CallTrace {
            call_type,
            from: from.to_lowercase(),
            to: to.to_lowercase(),
            value: "0x0".into(),
            gas_used: 21000,
            input,
            output: "0x".into(),
            function_selector,
            depth,
            block_number: 100,
            tx_hash: "0xtxhash".into(),
            tx_index: 0,
            trace_index: 0,
            error: if reverted {
                Some("execution reverted".into())
            } else {
                None
            },
            reverted,
        }
    }

    // ── CallType parsing ────────────────────────────────────────────────

    #[test]
    fn call_type_from_geth() {
        assert_eq!(CallType::from_geth("CALL"), Some(CallType::Call));
        assert_eq!(CallType::from_geth("DELEGATECALL"), Some(CallType::DelegateCall));
        assert_eq!(CallType::from_geth("STATICCALL"), Some(CallType::StaticCall));
        assert_eq!(CallType::from_geth("CREATE"), Some(CallType::Create));
        assert_eq!(CallType::from_geth("CREATE2"), Some(CallType::Create2));
        assert_eq!(CallType::from_geth("SELFDESTRUCT"), Some(CallType::SelfDestruct));
        assert_eq!(CallType::from_geth("UNKNOWN"), None);
    }

    #[test]
    fn call_type_from_parity() {
        assert_eq!(CallType::from_parity("call"), Some(CallType::Call));
        assert_eq!(CallType::from_parity("delegatecall"), Some(CallType::DelegateCall));
        assert_eq!(CallType::from_parity("suicide"), Some(CallType::SelfDestruct));
        assert_eq!(CallType::from_parity("selfdestruct"), Some(CallType::SelfDestruct));
        assert_eq!(CallType::from_parity("create"), Some(CallType::Create));
    }

    // ── Function selector extraction ────────────────────────────────────

    #[test]
    fn function_selector_extraction() {
        assert_eq!(
            CallTrace::extract_selector("0xa9059cbb0000000000000000000000001234"),
            Some("0xa9059cbb".into())
        );
        assert_eq!(CallTrace::extract_selector("0x"), None);
        assert_eq!(CallTrace::extract_selector("0xabcd"), None); // too short
        assert_eq!(
            CallTrace::extract_selector("0xA9059CBB"),
            Some("0xa9059cbb".into()) // lowercased
        );
    }

    // ── TraceFilter ─────────────────────────────────────────────────────

    #[test]
    fn filter_matches_all_by_default() {
        let filter = TraceFilter::new();
        let trace = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(filter.matches(&trace));
    }

    #[test]
    fn filter_by_address() {
        let filter = TraceFilter::new().with_address("0xaaa");

        // Matches on `from`.
        let t1 = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(filter.matches(&t1));

        // Matches on `to`.
        let t2 = make_trace(CallType::Call, "0xbbb", "0xaaa", Some("0xa9059cbb"), 0, false);
        assert!(filter.matches(&t2));

        // No match.
        let t3 = make_trace(CallType::Call, "0xbbb", "0xccc", Some("0xa9059cbb"), 0, false);
        assert!(!filter.matches(&t3));
    }

    #[test]
    fn filter_by_selector() {
        let filter = TraceFilter::new().with_selector("0xa9059cbb");

        let t1 = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(filter.matches(&t1));

        let t2 = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0x12345678"), 0, false);
        assert!(!filter.matches(&t2));

        // No selector (short input).
        let t3 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false);
        assert!(!filter.matches(&t3));
    }

    #[test]
    fn filter_by_call_type() {
        let filter = TraceFilter::new().with_call_type(CallType::Create);

        let t1 = make_trace(CallType::Create, "0xaaa", "0xbbb", None, 0, false);
        assert!(filter.matches(&t1));

        let t2 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false);
        assert!(!filter.matches(&t2));
    }

    #[test]
    fn filter_exclude_reverted() {
        let filter = TraceFilter::new().exclude_reverted(true);

        let t1 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false);
        assert!(filter.matches(&t1));

        let t2 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, true);
        assert!(!filter.matches(&t2));
    }

    #[test]
    fn filter_by_depth() {
        let filter = TraceFilter::new().min_depth(1).max_depth(3);

        let t0 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false);
        assert!(!filter.matches(&t0)); // depth 0 < min 1

        let t1 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 1, false);
        assert!(filter.matches(&t1));

        let t3 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 3, false);
        assert!(filter.matches(&t3));

        let t4 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 4, false);
        assert!(!filter.matches(&t4)); // depth 4 > max 3
    }

    // ── TraceHandler dispatch ───────────────────────────────────────────

    struct CountingHandler {
        count: Arc<AtomicU32>,
        handler_name: String,
    }

    #[async_trait]
    impl TraceHandler for CountingHandler {
        async fn handle_trace(
            &self,
            _trace: &CallTrace,
            _ctx: &IndexContext,
        ) -> Result<(), IndexerError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn name(&self) -> &str {
            &self.handler_name
        }
    }

    #[tokio::test]
    async fn dispatch_to_matching_handler() {
        let count = Arc::new(AtomicU32::new(0));
        let handler = Arc::new(CountingHandler {
            count: count.clone(),
            handler_name: "test_handler".into(),
        });

        let mut registry = TraceRegistry::new();
        registry.register(handler, TraceFilter::new().with_call_type(CallType::Create));

        let ctx = dummy_ctx();

        // Should match (Create).
        let t1 = make_trace(CallType::Create, "0xaaa", "0xbbb", None, 0, false);
        registry.dispatch(&t1, &ctx).await.unwrap();

        // Should not match (Call).
        let t2 = make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false);
        registry.dispatch(&t2, &ctx).await.unwrap();

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn dispatch_batch() {
        let count = Arc::new(AtomicU32::new(0));
        let handler = Arc::new(CountingHandler {
            count: count.clone(),
            handler_name: "batch_handler".into(),
        });

        let mut registry = TraceRegistry::new();
        registry.register(handler, TraceFilter::new());

        let ctx = dummy_ctx();
        let traces = vec![
            make_trace(CallType::Call, "0xaaa", "0xbbb", None, 0, false),
            make_trace(CallType::Create, "0xaaa", "0xbbb", None, 0, false),
            make_trace(CallType::DelegateCall, "0xaaa", "0xbbb", None, 0, false),
        ];

        registry.dispatch_batch(&traces, &ctx).await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    // ── Geth trace parsing ──────────────────────────────────────────────

    #[test]
    fn parse_geth_trace_basic() {
        let json = serde_json::json!([
            {
                "txHash": "0xabc123",
                "result": {
                    "type": "CALL",
                    "from": "0xSender",
                    "to": "0xReceiver",
                    "value": "0xde0b6b3a7640000",
                    "gasUsed": "0x5208",
                    "input": "0xa9059cbb0000000000000000000000001234",
                    "output": "0x0000000000000000000000000000000000000001",
                    "calls": [
                        {
                            "type": "DELEGATECALL",
                            "from": "0xReceiver",
                            "to": "0xImpl",
                            "value": "0x0",
                            "gasUsed": "0x1000",
                            "input": "0xa9059cbb0000",
                            "output": "0x01"
                        }
                    ]
                }
            }
        ]);

        let traces = parse_geth_traces(&json, 12345).unwrap();
        assert_eq!(traces.len(), 2);

        // Top-level call.
        assert_eq!(traces[0].call_type, CallType::Call);
        assert_eq!(traces[0].from, "0xsender");
        assert_eq!(traces[0].to, "0xreceiver");
        assert_eq!(traces[0].depth, 0);
        assert_eq!(traces[0].block_number, 12345);
        assert_eq!(traces[0].tx_hash, "0xabc123");
        assert_eq!(traces[0].gas_used, 0x5208);
        assert_eq!(traces[0].function_selector, Some("0xa9059cbb".into()));
        assert!(!traces[0].reverted);
        assert_eq!(traces[0].trace_index, 0);

        // Nested delegate call.
        assert_eq!(traces[1].call_type, CallType::DelegateCall);
        assert_eq!(traces[1].depth, 1);
        assert_eq!(traces[1].trace_index, 1);
    }

    #[test]
    fn parse_geth_trace_with_error() {
        let json = serde_json::json!([
            {
                "txHash": "0xfailed",
                "result": {
                    "type": "CALL",
                    "from": "0xSender",
                    "to": "0xReceiver",
                    "value": "0x0",
                    "gasUsed": "0x5208",
                    "input": "0x",
                    "output": "0x",
                    "error": "execution reverted",
                    "calls": [
                        {
                            "type": "CALL",
                            "from": "0xReceiver",
                            "to": "0xInner",
                            "value": "0x0",
                            "gasUsed": "0x100",
                            "input": "0x",
                            "output": "0x"
                        }
                    ]
                }
            }
        ]);

        let traces = parse_geth_traces(&json, 100).unwrap();
        assert_eq!(traces.len(), 2);

        // Parent is reverted.
        assert!(traces[0].reverted);
        assert_eq!(traces[0].error, Some("execution reverted".into()));

        // Child inherits reverted status from parent.
        assert!(traces[1].reverted);
    }

    // ── Parity trace parsing ────────────────────────────────────────────

    #[test]
    fn parse_parity_trace_basic() {
        let json = serde_json::json!([
            {
                "action": {
                    "from": "0xSender",
                    "to": "0xReceiver",
                    "value": "0xde0b6b3a7640000",
                    "input": "0xa9059cbb0000000000000000000000001234"
                },
                "result": {
                    "gasUsed": "0x5208",
                    "output": "0x0001"
                },
                "transactionHash": "0xparity_tx",
                "transactionPosition": 0,
                "traceAddress": [],
                "type": "call"
            },
            {
                "action": {
                    "from": "0xReceiver",
                    "to": "0xInner",
                    "value": "0x0",
                    "input": "0x12345678aabbccdd"
                },
                "result": {
                    "gasUsed": "0x1000",
                    "output": "0x"
                },
                "transactionHash": "0xparity_tx",
                "transactionPosition": 0,
                "traceAddress": [0],
                "type": "call"
            }
        ]);

        let traces = parse_parity_traces(&json, 999).unwrap();
        assert_eq!(traces.len(), 2);

        // Top-level.
        assert_eq!(traces[0].call_type, CallType::Call);
        assert_eq!(traces[0].from, "0xsender");
        assert_eq!(traces[0].to, "0xreceiver");
        assert_eq!(traces[0].depth, 0);
        assert_eq!(traces[0].block_number, 999);
        assert_eq!(traces[0].tx_hash, "0xparity_tx");
        assert_eq!(traces[0].function_selector, Some("0xa9059cbb".into()));

        // Nested.
        assert_eq!(traces[1].depth, 1);
        assert_eq!(traces[1].function_selector, Some("0x12345678".into()));
    }

    #[test]
    fn parse_parity_trace_create() {
        let json = serde_json::json!([
            {
                "action": {
                    "from": "0xDeployer",
                    "value": "0x0",
                    "init": "0x6080604052"
                },
                "result": {
                    "address": "0xNewContract",
                    "gasUsed": "0x30000",
                    "code": "0x6080"
                },
                "transactionHash": "0xcreate_tx",
                "transactionPosition": 1,
                "traceAddress": [],
                "type": "create"
            }
        ]);

        let traces = parse_parity_traces(&json, 500).unwrap();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].call_type, CallType::Create);
        assert_eq!(traces[0].from, "0xdeployer");
    }

    #[test]
    fn parse_parity_trace_with_error() {
        let json = serde_json::json!([
            {
                "action": {
                    "from": "0xSender",
                    "to": "0xReceiver",
                    "value": "0x0",
                    "input": "0x"
                },
                "transactionHash": "0xfail_tx",
                "transactionPosition": 0,
                "traceAddress": [],
                "type": "call",
                "error": "out of gas"
            }
        ]);

        let traces = parse_parity_traces(&json, 200).unwrap();
        assert_eq!(traces.len(), 1);
        assert!(traces[0].reverted);
        assert_eq!(traces[0].error, Some("out of gas".into()));
    }

    // ── Trace depth tracking ────────────────────────────────────────────

    #[test]
    fn geth_trace_depth_tracking() {
        let json = serde_json::json!([
            {
                "txHash": "0xdeep",
                "result": {
                    "type": "CALL",
                    "from": "0xa",
                    "to": "0xb",
                    "value": "0x0",
                    "gasUsed": "0x100",
                    "input": "0x",
                    "output": "0x",
                    "calls": [
                        {
                            "type": "CALL",
                            "from": "0xb",
                            "to": "0xc",
                            "value": "0x0",
                            "gasUsed": "0x50",
                            "input": "0x",
                            "output": "0x",
                            "calls": [
                                {
                                    "type": "STATICCALL",
                                    "from": "0xc",
                                    "to": "0xd",
                                    "value": "0x0",
                                    "gasUsed": "0x20",
                                    "input": "0x",
                                    "output": "0x"
                                }
                            ]
                        }
                    ]
                }
            }
        ]);

        let traces = parse_geth_traces(&json, 1).unwrap();
        assert_eq!(traces.len(), 3);
        assert_eq!(traces[0].depth, 0);
        assert_eq!(traces[1].depth, 1);
        assert_eq!(traces[2].depth, 2);
        assert_eq!(traces[2].call_type, CallType::StaticCall);
    }

    // ── Combined filter ─────────────────────────────────────────────────

    #[test]
    fn combined_filter_all_criteria() {
        let filter = TraceFilter::new()
            .with_address("0xaaa")
            .with_call_type(CallType::Call)
            .with_selector("0xa9059cbb")
            .exclude_reverted(true);

        // Matches all criteria.
        let t1 = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(filter.matches(&t1));

        // Wrong call type.
        let t2 = make_trace(CallType::Create, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(!filter.matches(&t2));

        // Wrong address.
        let t3 = make_trace(CallType::Call, "0xzzz", "0xbbb", Some("0xa9059cbb"), 0, false);
        assert!(!filter.matches(&t3));

        // Reverted.
        let t4 = make_trace(CallType::Call, "0xaaa", "0xbbb", Some("0xa9059cbb"), 0, true);
        assert!(!filter.matches(&t4));
    }
}
