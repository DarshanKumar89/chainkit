//! Pending transaction pool monitoring.
//!
//! [`PendingPoolMonitor`] watches a set of transaction hashes and can query
//! an [`RpcTransport`] to determine whether each transaction is still pending,
//! has been included in a block, or has disappeared from the mempool.

use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::Value;

use crate::error::TransportError;
use crate::request::JsonRpcRequest;
use crate::transport::RpcTransport;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for pending pool monitoring.
#[derive(Debug, Clone)]
pub struct PendingPoolConfig {
    /// How often to poll for status changes (ms).
    pub poll_interval_ms: u64,
    /// Max number of transactions to monitor simultaneously.
    pub max_monitored: usize,
}

impl Default for PendingPoolConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2000,
            max_monitored: 256,
        }
    }
}

// ---------------------------------------------------------------------------
// PendingTxStatus
// ---------------------------------------------------------------------------

/// The observed status of a pending transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingTxStatus {
    /// Still in mempool, no receipt yet.
    Pending,
    /// Included in a block.
    Included { block_number: u64 },
    /// Transaction not found (possibly dropped).
    NotFound,
}

impl std::fmt::Display for PendingTxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Included { block_number } => write!(f, "included(block={block_number})"),
            Self::NotFound => write!(f, "not_found"),
        }
    }
}

// ---------------------------------------------------------------------------
// PendingPoolMonitor
// ---------------------------------------------------------------------------

/// Monitors pending transactions and reports status changes.
///
/// The monitor maintains a thread-safe set of transaction hashes and
/// provides a static [`check_status`](PendingPoolMonitor::check_status)
/// method to query a transport for the current state of a transaction.
pub struct PendingPoolMonitor {
    config: PendingPoolConfig,
    watched: Mutex<HashSet<String>>,
}

impl PendingPoolMonitor {
    /// Create a new monitor with the given configuration.
    pub fn new(config: PendingPoolConfig) -> Self {
        Self {
            config,
            watched: Mutex::new(HashSet::new()),
        }
    }

    /// Add a transaction hash to monitor.
    ///
    /// Returns `true` if the hash was added, `false` if already present or
    /// if the monitor is at maximum capacity.
    pub fn watch(&self, tx_hash: String) -> bool {
        let mut watched = self.watched.lock().unwrap();
        if watched.len() >= self.config.max_monitored {
            return false;
        }
        watched.insert(tx_hash)
    }

    /// Remove a transaction from monitoring.
    pub fn unwatch(&self, tx_hash: &str) {
        let mut watched = self.watched.lock().unwrap();
        watched.remove(tx_hash);
    }

    /// Get all currently watched transaction hashes.
    pub fn watched(&self) -> Vec<String> {
        let watched = self.watched.lock().unwrap();
        watched.iter().cloned().collect()
    }

    /// Number of transactions being monitored.
    pub fn count(&self) -> usize {
        let watched = self.watched.lock().unwrap();
        watched.len()
    }

    /// Get the poll interval from the config.
    pub fn poll_interval_ms(&self) -> u64 {
        self.config.poll_interval_ms
    }

    /// Check the status of a single tx by querying the transport.
    ///
    /// Calls `eth_getTransactionReceipt` on the transport:
    /// - If the receipt exists and contains a `blockNumber`, the tx is
    ///   [`Included`](PendingTxStatus::Included).
    /// - If the receipt is `null` we fall back to `eth_getTransactionByHash`:
    ///   - If the tx object is present the tx is still [`Pending`](PendingTxStatus::Pending).
    ///   - Otherwise it is [`NotFound`](PendingTxStatus::NotFound).
    pub async fn check_status(
        transport: &dyn RpcTransport,
        tx_hash: &str,
    ) -> Result<PendingTxStatus, TransportError> {
        // 1. Try eth_getTransactionReceipt.
        let receipt_req = JsonRpcRequest::auto(
            "eth_getTransactionReceipt",
            vec![Value::String(tx_hash.to_string())],
        );
        let receipt_resp = transport.send(receipt_req).await?;
        let receipt_value = receipt_resp
            .into_result()
            .map_err(TransportError::Rpc)?;

        if !receipt_value.is_null() {
            // Extract blockNumber from the receipt.
            if let Some(block_hex) = receipt_value.get("blockNumber").and_then(|v| v.as_str()) {
                let block_number =
                    u64::from_str_radix(block_hex.trim_start_matches("0x"), 16).unwrap_or(0);
                return Ok(PendingTxStatus::Included { block_number });
            }
            // Receipt exists but no blockNumber — treat as included at 0.
            return Ok(PendingTxStatus::Included { block_number: 0 });
        }

        // 2. Receipt is null — check if the tx itself exists.
        let tx_req = JsonRpcRequest::auto(
            "eth_getTransactionByHash",
            vec![Value::String(tx_hash.to_string())],
        );
        let tx_resp = transport.send(tx_req).await?;
        let tx_value = tx_resp.into_result().map_err(TransportError::Rpc)?;

        if tx_value.is_null() {
            Ok(PendingTxStatus::NotFound)
        } else {
            Ok(PendingTxStatus::Pending)
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_watch_unwatch() {
        let monitor = PendingPoolMonitor::new(PendingPoolConfig::default());

        // Watch a new hash.
        assert!(monitor.watch("0xabc".to_string()));
        assert_eq!(monitor.count(), 1);

        // Watching the same hash again returns false (already present).
        assert!(!monitor.watch("0xabc".to_string()));
        assert_eq!(monitor.count(), 1);

        // Watch a second hash.
        assert!(monitor.watch("0xdef".to_string()));
        assert_eq!(monitor.count(), 2);

        // Unwatch.
        monitor.unwatch("0xabc");
        assert_eq!(monitor.count(), 1);

        // The watched list should only contain 0xdef.
        let list = monitor.watched();
        assert_eq!(list.len(), 1);
        assert!(list.contains(&"0xdef".to_string()));
    }

    #[test]
    fn monitor_max_capacity() {
        let config = PendingPoolConfig {
            poll_interval_ms: 1000,
            max_monitored: 2,
        };
        let monitor = PendingPoolMonitor::new(config);

        assert!(monitor.watch("0x1".to_string()));
        assert!(monitor.watch("0x2".to_string()));
        // At capacity — should return false.
        assert!(!monitor.watch("0x3".to_string()));
        assert_eq!(monitor.count(), 2);

        // After unwatching one, we can add again.
        monitor.unwatch("0x1");
        assert!(monitor.watch("0x3".to_string()));
        assert_eq!(monitor.count(), 2);
    }

    #[test]
    fn pending_status_enum() {
        let pending = PendingTxStatus::Pending;
        assert_eq!(pending.to_string(), "pending");

        let included = PendingTxStatus::Included { block_number: 42 };
        assert_eq!(included.to_string(), "included(block=42)");

        let not_found = PendingTxStatus::NotFound;
        assert_eq!(not_found.to_string(), "not_found");

        // PartialEq
        assert_eq!(PendingTxStatus::Pending, PendingTxStatus::Pending);
        assert_ne!(PendingTxStatus::Pending, PendingTxStatus::NotFound);
        assert_eq!(
            PendingTxStatus::Included { block_number: 10 },
            PendingTxStatus::Included { block_number: 10 },
        );
        assert_ne!(
            PendingTxStatus::Included { block_number: 10 },
            PendingTxStatus::Included { block_number: 20 },
        );
    }
}
