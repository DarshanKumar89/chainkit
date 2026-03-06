//! Chain reorganization detection at the RPC transport layer.
//!
//! Monitors block hashes via a sliding window and detects when the chain
//! has reorganized (a previously-seen block hash changes for the same height).
//!
//! When a reorg is detected, registered callbacks are invoked with the
//! fork point (the lowest block number that changed).
//!
//! # Architecture
//!
//! The reorg detector maintains a fixed-size window of `(block_number, block_hash)`
//! pairs. On each new block:
//!
//! 1. Fetch block by number via `eth_getBlockByNumber`
//! 2. Compare hash against stored hash for that height
//! 3. If mismatch -> walk backward to find the fork point
//! 4. Invoke all registered `on_reorg` callbacks
//! 5. Invalidate affected entries in the window

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;

use crate::error::TransportError;
use crate::request::JsonRpcRequest;
use crate::transport::RpcTransport;

// ---------------------------------------------------------------------------
// ReorgConfig
// ---------------------------------------------------------------------------

/// Configuration for the reorg detector.
#[derive(Debug, Clone)]
pub struct ReorgConfig {
    /// Number of blocks to keep in the sliding window (default: 128).
    pub window_size: usize,
    /// Minimum block depth before considering a block "safe" (default: 64).
    pub safe_depth: u64,
    /// Whether to use "finalized" block tag for safe block (default: true).
    pub use_finalized_tag: bool,
}

impl Default for ReorgConfig {
    fn default() -> Self {
        Self {
            window_size: 128,
            safe_depth: 64,
            use_finalized_tag: true,
        }
    }
}

// ---------------------------------------------------------------------------
// ReorgEvent
// ---------------------------------------------------------------------------

/// Information about a detected reorganization.
#[derive(Debug, Clone, Serialize)]
pub struct ReorgEvent {
    /// The lowest block number that changed (fork point).
    pub fork_block: u64,
    /// The depth of the reorg (how many blocks were replaced).
    pub depth: u64,
    /// The old block hash at the fork point.
    pub old_hash: String,
    /// The new block hash at the fork point.
    pub new_hash: String,
    /// The current chain tip block number.
    pub current_tip: u64,
}

// ---------------------------------------------------------------------------
// ReorgDetector
// ---------------------------------------------------------------------------

/// Chain reorganization detector.
///
/// Maintains a sliding window of block hashes and detects when the chain
/// reorganizes by comparing new block hashes against stored ones.
///
/// Thread-safe via interior `Mutex`es — suitable for shared access across
/// Tokio tasks behind an `Arc`.
pub struct ReorgDetector {
    config: ReorgConfig,
    /// Sliding window: block_number -> block_hash.
    window: Mutex<HashMap<u64, String>>,
    /// Last known tip.
    last_tip: Mutex<Option<u64>>,
    /// Registered callbacks -- called with ReorgEvent when reorg detected.
    #[allow(clippy::type_complexity)]
    callbacks: Mutex<Vec<Box<dyn Fn(&ReorgEvent) + Send + Sync>>>,
    /// History of detected reorgs.
    reorg_history: Mutex<Vec<ReorgEvent>>,
}

impl ReorgDetector {
    /// Create a new reorg detector with the given configuration.
    pub fn new(config: ReorgConfig) -> Self {
        Self {
            config,
            window: Mutex::new(HashMap::new()),
            last_tip: Mutex::new(None),
            callbacks: Mutex::new(Vec::new()),
            reorg_history: Mutex::new(Vec::new()),
        }
    }

    /// Register a callback that fires on reorg detection.
    pub fn on_reorg<F>(&self, callback: F)
    where
        F: Fn(&ReorgEvent) + Send + Sync + 'static,
    {
        let mut callbacks = self.callbacks.lock().unwrap();
        callbacks.push(Box::new(callback));
    }

    /// Check a block against the window. Returns `Some(ReorgEvent)` if reorg detected.
    ///
    /// Call this with each new block as it arrives.
    pub fn check_block(&self, block_number: u64, block_hash: &str) -> Option<ReorgEvent> {
        let mut window = self.window.lock().unwrap();
        let mut last_tip = self.last_tip.lock().unwrap();

        // Check if we have a stored hash for this block number
        if let Some(stored_hash) = window.get(&block_number) {
            if stored_hash != block_hash {
                // REORG DETECTED -- find fork point
                let fork_block = block_number;
                let depth = last_tip.unwrap_or(block_number) - fork_block + 1;

                let event = ReorgEvent {
                    fork_block,
                    depth,
                    old_hash: stored_hash.clone(),
                    new_hash: block_hash.to_string(),
                    current_tip: block_number,
                };

                // Invalidate affected blocks in window
                let affected: Vec<u64> = window
                    .keys()
                    .filter(|&&n| n >= fork_block)
                    .copied()
                    .collect();
                for n in affected {
                    window.remove(&n);
                }

                // Store new block
                window.insert(block_number, block_hash.to_string());
                *last_tip = Some(block_number);

                // Trim window
                Self::trim_window_inner(&self.config, &mut window, block_number);

                // Fire callbacks
                let callbacks = self.callbacks.lock().unwrap();
                for cb in callbacks.iter() {
                    cb(&event);
                }

                // Store in history
                self.reorg_history.lock().unwrap().push(event.clone());

                return Some(event);
            }
        }

        // No reorg -- store block and advance tip
        window.insert(block_number, block_hash.to_string());
        *last_tip = Some(block_number);
        Self::trim_window_inner(&self.config, &mut window, block_number);

        None
    }

    /// Trim blocks that fall outside the window.
    fn trim_window_inner(
        config: &ReorgConfig,
        window: &mut HashMap<u64, String>,
        current_tip: u64,
    ) {
        if current_tip >= config.window_size as u64 {
            let cutoff = current_tip - config.window_size as u64;
            window.retain(|&n, _| n > cutoff);
        }
    }

    /// Query the transport for a block hash at a given height.
    pub async fn fetch_block_hash(
        transport: &dyn RpcTransport,
        block_number: u64,
    ) -> Result<Option<String>, TransportError> {
        let hex_block = format!("0x{:x}", block_number);
        let req = JsonRpcRequest::auto(
            "eth_getBlockByNumber",
            vec![Value::String(hex_block), Value::Bool(false)],
        );
        let resp = transport.send(req).await?;
        let value = resp.into_result().map_err(TransportError::Rpc)?;

        Ok(value
            .get("hash")
            .and_then(|h| h.as_str())
            .map(|s| s.to_string()))
    }

    /// Poll the chain for new blocks and check for reorgs.
    ///
    /// 1. Gets the current block number
    /// 2. Fetches the block hash
    /// 3. Calls check_block
    ///
    /// Returns any detected ReorgEvent.
    pub async fn poll_and_check(
        &self,
        transport: &dyn RpcTransport,
    ) -> Result<Option<ReorgEvent>, TransportError> {
        // Get current block number
        let req = JsonRpcRequest::auto("eth_blockNumber", vec![]);
        let resp = transport.send(req).await?;
        let value = resp.into_result().map_err(TransportError::Rpc)?;

        let block_number = value
            .as_str()
            .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| TransportError::Other("invalid eth_blockNumber response".into()))?;

        // Get block hash
        let hash = Self::fetch_block_hash(transport, block_number)
            .await?
            .ok_or_else(|| TransportError::Other("block not found".into()))?;

        Ok(self.check_block(block_number, &hash))
    }

    /// Get the safe block number (current tip - safe_depth).
    pub fn safe_block(&self) -> Option<u64> {
        let tip = self.last_tip.lock().unwrap();
        tip.and_then(|t| t.checked_sub(self.config.safe_depth))
    }

    /// Get the finalized block from the chain.
    pub async fn fetch_finalized_block(
        transport: &dyn RpcTransport,
    ) -> Result<u64, TransportError> {
        let req = JsonRpcRequest::auto(
            "eth_getBlockByNumber",
            vec![Value::String("finalized".into()), Value::Bool(false)],
        );
        let resp = transport.send(req).await?;
        let value = resp.into_result().map_err(TransportError::Rpc)?;

        value
            .get("number")
            .and_then(|n| n.as_str())
            .and_then(|hex| u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| TransportError::Other("invalid finalized block response".into()))
    }

    /// Get reorg history.
    pub fn reorg_history(&self) -> Vec<ReorgEvent> {
        self.reorg_history.lock().unwrap().clone()
    }

    /// Number of blocks currently in the window.
    pub fn window_size(&self) -> usize {
        self.window.lock().unwrap().len()
    }

    /// Check if a block number is in the safe zone (below safe_depth).
    pub fn is_block_safe(&self, block_number: u64) -> bool {
        self.safe_block().is_some_and(|safe| block_number <= safe)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcResponse, RpcId};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Mock transport
    // -----------------------------------------------------------------------

    struct MockTransport {
        responses: Mutex<HashMap<String, Value>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
            }
        }

        fn set_response(&self, method: &str, value: Value) {
            let mut map = self.responses.lock().unwrap();
            map.insert(method.to_string(), value);
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            let map = self.responses.lock().unwrap();
            let result = map.get(&req.method).cloned().unwrap_or(Value::Null);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(result),
                error: None,
            })
        }

        fn url(&self) -> &str {
            "mock://reorg"
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn no_reorg_sequential_blocks() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        // Add sequential blocks with consistent hashes
        for i in 100..110 {
            let hash = format!("0xhash_{i}");
            let result = detector.check_block(i, &hash);
            assert!(result.is_none(), "block {i} should not trigger reorg");
        }

        assert_eq!(detector.window_size(), 10);
        assert!(detector.reorg_history().is_empty());
    }

    #[test]
    fn detect_simple_reorg() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        // Add block 100 with hash A
        assert!(detector.check_block(100, "0xhash_A").is_none());

        // Same block 100 with different hash B -> REORG
        let event = detector
            .check_block(100, "0xhash_B")
            .expect("should detect reorg");

        assert_eq!(event.fork_block, 100);
        assert_eq!(event.old_hash, "0xhash_A");
        assert_eq!(event.new_hash, "0xhash_B");
    }

    #[test]
    fn reorg_event_has_correct_fields() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        // Build a chain: blocks 100, 101, 102
        detector.check_block(100, "0xA100");
        detector.check_block(101, "0xA101");
        detector.check_block(102, "0xA102");

        // Reorg at block 101: tip was 102, fork at 101, depth = 102 - 101 + 1 = 2
        let event = detector
            .check_block(101, "0xB101")
            .expect("should detect reorg");

        assert_eq!(event.fork_block, 101);
        assert_eq!(event.depth, 2);
        assert_eq!(event.old_hash, "0xA101");
        assert_eq!(event.new_hash, "0xB101");
        assert_eq!(event.current_tip, 101);
    }

    #[test]
    fn window_trims_old_blocks() {
        let config = ReorgConfig {
            window_size: 5,
            ..Default::default()
        };
        let detector = ReorgDetector::new(config);

        // Add blocks 1 through 10
        for i in 1..=10 {
            detector.check_block(i, &format!("0xhash_{i}"));
        }

        // Window size is 5, tip is 10, cutoff is 10-5=5.
        // Only blocks > 5 are retained: 6, 7, 8, 9, 10.
        assert_eq!(detector.window_size(), 5);

        // Blocks 1-5 should be gone (no reorg if we re-add block 3
        // with a different hash, because it's been trimmed)
        assert!(detector.check_block(3, "0xdifferent").is_none());
    }

    #[test]
    fn callback_fires_on_reorg() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        let call_count = Arc::new(AtomicU32::new(0));
        let count_clone = call_count.clone();

        detector.on_reorg(move |_event| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Add block, then reorg it
        detector.check_block(100, "0xhash_A");
        detector.check_block(100, "0xhash_B");

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn multiple_callbacks() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        let count1 = Arc::new(AtomicU32::new(0));
        let count2 = Arc::new(AtomicU32::new(0));
        let c1 = count1.clone();
        let c2 = count2.clone();

        detector.on_reorg(move |_| {
            c1.fetch_add(1, Ordering::SeqCst);
        });
        detector.on_reorg(move |_| {
            c2.fetch_add(1, Ordering::SeqCst);
        });

        detector.check_block(100, "0xA");
        detector.check_block(100, "0xB");

        assert_eq!(count1.load(Ordering::SeqCst), 1);
        assert_eq!(count2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn reorg_history_recorded() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        assert!(detector.reorg_history().is_empty());

        // First reorg
        detector.check_block(100, "0xA");
        detector.check_block(100, "0xB");

        // Second reorg
        detector.check_block(200, "0xC");
        detector.check_block(200, "0xD");

        let history = detector.reorg_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].fork_block, 100);
        assert_eq!(history[1].fork_block, 200);
    }

    #[test]
    fn safe_block_calculation() {
        let config = ReorgConfig {
            safe_depth: 10,
            ..Default::default()
        };
        let detector = ReorgDetector::new(config);

        // No blocks yet
        assert!(detector.safe_block().is_none());

        // Tip = 100, safe_depth = 10 -> safe_block = 90
        detector.check_block(100, "0xhash");
        assert_eq!(detector.safe_block(), Some(90));

        // Advance tip to 150 -> safe_block = 140
        detector.check_block(150, "0xhash_150");
        assert_eq!(detector.safe_block(), Some(140));
    }

    #[test]
    fn safe_block_returns_none_when_tip_below_depth() {
        let config = ReorgConfig {
            safe_depth: 100,
            ..Default::default()
        };
        let detector = ReorgDetector::new(config);

        // Tip = 50, safe_depth = 100 -> 50 - 100 would underflow
        detector.check_block(50, "0xhash");
        assert!(detector.safe_block().is_none());
    }

    #[test]
    fn is_block_safe_checks_depth() {
        let config = ReorgConfig {
            safe_depth: 10,
            ..Default::default()
        };
        let detector = ReorgDetector::new(config);

        detector.check_block(100, "0xhash");
        // safe_block = 90

        assert!(detector.is_block_safe(80)); // below 90
        assert!(detector.is_block_safe(90)); // exactly 90
        assert!(!detector.is_block_safe(91)); // above 90
        assert!(!detector.is_block_safe(100)); // at tip
    }

    #[test]
    fn is_block_safe_false_without_tip() {
        let detector = ReorgDetector::new(ReorgConfig::default());
        assert!(!detector.is_block_safe(0));
        assert!(!detector.is_block_safe(100));
    }

    #[test]
    fn reorg_clears_affected_blocks() {
        let detector = ReorgDetector::new(ReorgConfig::default());

        // Build chain: 100, 101, 102, 103
        detector.check_block(100, "0xA100");
        detector.check_block(101, "0xA101");
        detector.check_block(102, "0xA102");
        detector.check_block(103, "0xA103");
        assert_eq!(detector.window_size(), 4);

        // Reorg at block 101 — blocks 101, 102, 103 should be removed,
        // then 101 is re-added with the new hash.
        let event = detector
            .check_block(101, "0xB101")
            .expect("should detect reorg");
        assert_eq!(event.fork_block, 101);

        // Window should contain block 100 (unchanged) and 101 (new hash).
        // Blocks 102 and 103 were removed.
        assert_eq!(detector.window_size(), 2);

        // Re-adding 102 with a new hash should NOT trigger reorg
        // because 102 was removed from the window.
        assert!(detector.check_block(102, "0xB102").is_none());
        assert_eq!(detector.window_size(), 3);
    }

    #[tokio::test]
    async fn poll_and_check_works() {
        let transport = MockTransport::new();

        // Set up mock responses
        transport.set_response(
            "eth_blockNumber",
            Value::String("0x64".into()), // block 100
        );
        transport.set_response(
            "eth_getBlockByNumber",
            serde_json::json!({
                "number": "0x64",
                "hash": "0xblock_hash_100"
            }),
        );

        let detector = ReorgDetector::new(ReorgConfig::default());

        // First poll — no reorg (fresh window)
        let result = detector.poll_and_check(&transport).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
        assert_eq!(detector.window_size(), 1);

        // Poll again with same block/hash — no reorg
        let result = detector.poll_and_check(&transport).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());

        // Change the hash response to simulate reorg
        transport.set_response(
            "eth_getBlockByNumber",
            serde_json::json!({
                "number": "0x64",
                "hash": "0xreorged_hash_100"
            }),
        );

        // Poll again — should detect reorg
        let result = detector.poll_and_check(&transport).await;
        assert!(result.is_ok());
        let event = result.unwrap().expect("should detect reorg");
        assert_eq!(event.fork_block, 100);
        assert_eq!(event.old_hash, "0xblock_hash_100");
        assert_eq!(event.new_hash, "0xreorged_hash_100");
    }

    #[tokio::test]
    async fn fetch_block_hash_works() {
        let transport = MockTransport::new();
        transport.set_response(
            "eth_getBlockByNumber",
            serde_json::json!({
                "number": "0xc8",
                "hash": "0xblock_hash_200"
            }),
        );

        let hash = ReorgDetector::fetch_block_hash(&transport, 200).await;
        assert!(hash.is_ok());
        assert_eq!(hash.unwrap(), Some("0xblock_hash_200".to_string()));
    }

    #[tokio::test]
    async fn fetch_block_hash_returns_none_for_null_hash() {
        let transport = MockTransport::new();
        transport.set_response(
            "eth_getBlockByNumber",
            serde_json::json!({
                "number": "0xc8"
                // no "hash" field
            }),
        );

        let hash = ReorgDetector::fetch_block_hash(&transport, 200).await;
        assert!(hash.is_ok());
        assert!(hash.unwrap().is_none());
    }

    #[tokio::test]
    async fn fetch_finalized_block_works() {
        let transport = MockTransport::new();
        transport.set_response(
            "eth_getBlockByNumber",
            serde_json::json!({
                "number": "0x1f4",
                "hash": "0xfinalized_hash"
            }),
        );

        let block = ReorgDetector::fetch_finalized_block(&transport).await;
        assert!(block.is_ok());
        assert_eq!(block.unwrap(), 500); // 0x1f4 = 500
    }

    #[test]
    fn reorg_event_serializable() {
        let event = ReorgEvent {
            fork_block: 100,
            depth: 3,
            old_hash: "0xold".into(),
            new_hash: "0xnew".into(),
            current_tip: 102,
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("fork_block"));
        assert!(json.contains("100"));
        assert!(json.contains("0xold"));
        assert!(json.contains("0xnew"));
    }

    #[test]
    fn default_config_values() {
        let config = ReorgConfig::default();
        assert_eq!(config.window_size, 128);
        assert_eq!(config.safe_depth, 64);
        assert!(config.use_finalized_tag);
    }
}
