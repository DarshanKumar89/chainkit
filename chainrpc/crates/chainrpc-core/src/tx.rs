//! Transaction lifecycle management — tracking, confirmation monitoring,
//! nonce management, and stuck transaction detection for EVM-compatible
//! blockchains.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

// ---------------------------------------------------------------------------
// TxStatus
// ---------------------------------------------------------------------------

/// The lifecycle state of an on-chain transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TxStatus {
    /// Transaction submitted but not yet seen in a block.
    Pending,
    /// Transaction included in a block but not yet confirmed.
    Included {
        block_number: u64,
        block_hash: String,
    },
    /// Transaction has enough confirmations to be considered final.
    Confirmed {
        block_number: u64,
        confirmations: u64,
    },
    /// Transaction was dropped from the mempool.
    Dropped,
    /// Transaction was replaced by a higher-gas transaction.
    Replaced { replacement_hash: String },
    /// Transaction failed on-chain.
    Failed { reason: String },
}

impl std::fmt::Display for TxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Included {
                block_number,
                block_hash,
            } => write!(f, "included(block={block_number}, hash={block_hash})"),
            Self::Confirmed {
                block_number,
                confirmations,
            } => write!(
                f,
                "confirmed(block={block_number}, confirmations={confirmations})"
            ),
            Self::Dropped => write!(f, "dropped"),
            Self::Replaced { replacement_hash } => {
                write!(f, "replaced(by={replacement_hash})")
            }
            Self::Failed { reason } => write!(f, "failed({reason})"),
        }
    }
}

// ---------------------------------------------------------------------------
// TrackedTx
// ---------------------------------------------------------------------------

/// A transaction that is being actively monitored.
#[derive(Debug, Clone)]
pub struct TrackedTx {
    /// The transaction hash.
    pub tx_hash: String,
    /// The sender address.
    pub from: String,
    /// The nonce used by this transaction.
    pub nonce: u64,
    /// Unix timestamp when the transaction was submitted.
    pub submitted_at: u64,
    /// Current lifecycle status.
    pub status: TxStatus,
    /// Legacy gas price (Type 0 / Type 1 transactions).
    pub gas_price: Option<u64>,
    /// EIP-1559 max fee per gas.
    pub max_fee: Option<u64>,
    /// EIP-1559 max priority fee per gas.
    pub max_priority_fee: Option<u64>,
    /// Unix timestamp of the last status check.
    pub last_checked: u64,
}

// ---------------------------------------------------------------------------
// TxTracker
// ---------------------------------------------------------------------------

/// Configuration for [`TxTracker`].
pub struct TxTrackerConfig {
    /// How many confirmations needed to consider a transaction confirmed.
    pub confirmation_depth: u64,
    /// Max time in seconds before a pending transaction is considered stuck.
    pub stuck_timeout_secs: u64,
    /// Polling interval for receipt checks (in seconds).
    pub poll_interval_secs: u64,
    /// Maximum number of pending transactions to track.
    pub max_tracked: usize,
}

impl Default for TxTrackerConfig {
    fn default() -> Self {
        Self {
            confirmation_depth: 12,
            stuck_timeout_secs: 300, // 5 minutes
            poll_interval_secs: 3,
            max_tracked: 1000,
        }
    }
}

/// Core transaction tracker that monitors pending transactions.
///
/// Thread-safe via interior `Mutex` — suitable for shared access across
/// Tokio tasks behind an `Arc`.
pub struct TxTracker {
    config: TxTrackerConfig,
    /// tx_hash -> TrackedTx
    transactions: Mutex<HashMap<String, TrackedTx>>,
    /// address -> last known nonce
    nonce_tracker: Mutex<HashMap<String, u64>>,
}

impl TxTracker {
    /// Create a new tracker with the given configuration.
    pub fn new(config: TxTrackerConfig) -> Self {
        Self {
            config,
            transactions: Mutex::new(HashMap::new()),
            nonce_tracker: Mutex::new(HashMap::new()),
        }
    }

    /// Track a new pending transaction.
    ///
    /// If the tracker is already at capacity (`max_tracked`), the transaction
    /// is silently dropped.
    pub fn track(&self, tx: TrackedTx) {
        let mut txs = self.transactions.lock().unwrap();
        if txs.len() >= self.config.max_tracked {
            return;
        }
        txs.insert(tx.tx_hash.clone(), tx);
    }

    /// Remove a transaction from tracking.
    pub fn untrack(&self, tx_hash: &str) {
        let mut txs = self.transactions.lock().unwrap();
        txs.remove(tx_hash);
    }

    /// Update the status of a tracked transaction.
    ///
    /// Does nothing if `tx_hash` is not currently tracked.
    pub fn update_status(&self, tx_hash: &str, status: TxStatus) {
        let mut txs = self.transactions.lock().unwrap();
        if let Some(tx) = txs.get_mut(tx_hash) {
            tx.status = status;
        }
    }

    /// Get all transactions whose status matches `status_match`.
    ///
    /// Comparison uses the discriminant only for variant-carrying statuses;
    /// for simple variants (`Pending`, `Dropped`) it uses `PartialEq`.
    pub fn by_status(&self, status_match: &TxStatus) -> Vec<TrackedTx> {
        let txs = self.transactions.lock().unwrap();
        txs.values()
            .filter(|tx| std::mem::discriminant(&tx.status) == std::mem::discriminant(status_match))
            .cloned()
            .collect()
    }

    /// Get all pending transactions.
    pub fn pending(&self) -> Vec<TrackedTx> {
        self.by_status(&TxStatus::Pending)
    }

    /// Get transactions that appear stuck (pending longer than `stuck_timeout_secs`).
    pub fn stuck(&self, current_time: u64) -> Vec<TrackedTx> {
        let txs = self.transactions.lock().unwrap();
        txs.values()
            .filter(|tx| {
                tx.status == TxStatus::Pending
                    && current_time.saturating_sub(tx.submitted_at) > self.config.stuck_timeout_secs
            })
            .cloned()
            .collect()
    }

    /// Get the next nonce for an address (local tracking).
    ///
    /// Returns the stored nonce + 1, or `None` if the address has never been
    /// registered.
    pub fn next_nonce(&self, address: &str) -> Option<u64> {
        let nonces = self.nonce_tracker.lock().unwrap();
        nonces.get(address).map(|n| n + 1)
    }

    /// Set the nonce for an address (typically from an on-chain query).
    pub fn set_nonce(&self, address: &str, nonce: u64) {
        let mut nonces = self.nonce_tracker.lock().unwrap();
        nonces.insert(address.to_string(), nonce);
    }

    /// Get count of tracked transactions.
    pub fn count(&self) -> usize {
        let txs = self.transactions.lock().unwrap();
        txs.len()
    }

    /// Get a snapshot of a specific transaction.
    pub fn get(&self, tx_hash: &str) -> Option<TrackedTx> {
        let txs = self.transactions.lock().unwrap();
        txs.get(tx_hash).cloned()
    }
}

// ---------------------------------------------------------------------------
// ReceiptPoller
// ---------------------------------------------------------------------------

/// Configuration for [`ReceiptPoller`] exponential-backoff strategy.
pub struct ReceiptPollerConfig {
    /// Initial poll interval.
    pub initial_interval: Duration,
    /// Maximum poll interval (cap).
    pub max_interval: Duration,
    /// Backoff multiplier applied on each successive attempt.
    pub multiplier: f64,
    /// Maximum number of attempts before giving up.
    pub max_attempts: u32,
}

impl Default for ReceiptPollerConfig {
    fn default() -> Self {
        Self {
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(30),
            multiplier: 1.5,
            max_attempts: 60,
        }
    }
}

/// Smart receipt poller with exponential backoff.
///
/// Does not perform I/O itself — it computes delays and decides when to stop
/// polling. The caller drives the actual RPC calls.
pub struct ReceiptPoller {
    config: ReceiptPollerConfig,
}

impl ReceiptPoller {
    /// Create a new poller with the given configuration.
    pub fn new(config: ReceiptPollerConfig) -> Self {
        Self { config }
    }

    /// Calculate the delay before the given attempt (1-indexed).
    ///
    /// Returns `None` when `attempt` exceeds `max_attempts`, signalling that
    /// polling should stop.
    pub fn delay_for_attempt(&self, attempt: u32) -> Option<Duration> {
        if attempt > self.config.max_attempts {
            return None;
        }
        let delay = self.config.initial_interval.as_secs_f64()
            * self.config.multiplier.powi((attempt - 1) as i32);
        let capped = delay.min(self.config.max_interval.as_secs_f64());
        Some(Duration::from_secs_f64(capped))
    }

    /// Check if we should continue polling at the given attempt number.
    pub fn should_continue(&self, attempt: u32) -> bool {
        attempt <= self.config.max_attempts
    }
}

// ---------------------------------------------------------------------------
// NonceLedger
// ---------------------------------------------------------------------------

/// Nonce management for tracking local and on-chain nonces.
///
/// Maintains two nonce counters per address:
/// - **confirmed** — the last nonce known to be mined on-chain.
/// - **pending** — the highest nonce assigned locally but not yet confirmed.
pub struct NonceLedger {
    /// On-chain confirmed nonces per address.
    confirmed: Mutex<HashMap<String, u64>>,
    /// Locally assigned (pending) nonces per address.
    pending: Mutex<HashMap<String, u64>>,
}

impl NonceLedger {
    /// Create a new, empty ledger.
    pub fn new() -> Self {
        Self {
            confirmed: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Set the on-chain confirmed nonce for an address.
    pub fn set_confirmed(&self, address: &str, nonce: u64) {
        let mut confirmed = self.confirmed.lock().unwrap();
        confirmed.insert(address.to_string(), nonce);
    }

    /// Get the next available nonce — the maximum of (confirmed + 1) and
    /// (pending + 1), falling back to 0 when neither is set.
    pub fn next(&self, address: &str) -> u64 {
        let confirmed = self.confirmed.lock().unwrap();
        let pending = self.pending.lock().unwrap();

        let from_confirmed = confirmed.get(address).map(|n| n + 1).unwrap_or(0);
        let from_pending = pending.get(address).map(|n| n + 1).unwrap_or(0);
        from_confirmed.max(from_pending)
    }

    /// Mark a nonce as used (pending).
    pub fn mark_pending(&self, address: &str, nonce: u64) {
        let mut pending = self.pending.lock().unwrap();
        let entry = pending.entry(address.to_string()).or_insert(0);
        if nonce > *entry {
            *entry = nonce;
        }
    }

    /// Confirm a nonce — updates confirmed and clears pending when the
    /// pending nonce is at or below the confirmed nonce.
    pub fn confirm(&self, address: &str, nonce: u64) {
        let mut confirmed = self.confirmed.lock().unwrap();
        confirmed.insert(address.to_string(), nonce);
        drop(confirmed);

        let mut pending = self.pending.lock().unwrap();
        if let Some(p) = pending.get(address) {
            if *p <= nonce {
                pending.remove(address);
            }
        }
    }

    /// Get the current confirmed nonce.
    pub fn confirmed_nonce(&self, address: &str) -> Option<u64> {
        let confirmed = self.confirmed.lock().unwrap();
        confirmed.get(address).copied()
    }

    /// Get the current pending nonce.
    pub fn pending_nonce(&self, address: &str) -> Option<u64> {
        let pending = self.pending.lock().unwrap();
        pending.get(address).copied()
    }

    /// Get gap nonces — nonces between confirmed and pending that have not
    /// been observed.
    ///
    /// For example, if confirmed = 3 and pending = 7, the gaps are `[4, 5, 6]`.
    /// Returns an empty vec if there are no gaps or either value is unset.
    pub fn gaps(&self, address: &str) -> Vec<u64> {
        let confirmed = self.confirmed.lock().unwrap();
        let pending = self.pending.lock().unwrap();

        let c = match confirmed.get(address) {
            Some(n) => *n,
            None => return vec![],
        };
        let p = match pending.get(address) {
            Some(n) => *n,
            None => return vec![],
        };

        if p <= c + 1 {
            return vec![];
        }

        ((c + 1)..p).collect()
    }
}

impl Default for NonceLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // TxTracker tests
    // -----------------------------------------------------------------------

    fn sample_tx(hash: &str, nonce: u64, submitted_at: u64) -> TrackedTx {
        TrackedTx {
            tx_hash: hash.to_string(),
            from: "0xAlice".to_string(),
            nonce,
            submitted_at,
            status: TxStatus::Pending,
            gas_price: Some(20_000_000_000),
            max_fee: None,
            max_priority_fee: None,
            last_checked: submitted_at,
        }
    }

    #[test]
    fn tracker_track_and_get() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        let tx = sample_tx("0xabc", 0, 1000);
        tracker.track(tx);

        let fetched = tracker.get("0xabc").expect("should find tracked tx");
        assert_eq!(fetched.tx_hash, "0xabc");
        assert_eq!(fetched.nonce, 0);
        assert_eq!(fetched.status, TxStatus::Pending);
        assert_eq!(tracker.count(), 1);
    }

    #[test]
    fn tracker_untrack() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        tracker.track(sample_tx("0xabc", 0, 1000));
        assert_eq!(tracker.count(), 1);

        tracker.untrack("0xabc");
        assert_eq!(tracker.count(), 0);
        assert!(tracker.get("0xabc").is_none());
    }

    #[test]
    fn tracker_update_status() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        tracker.track(sample_tx("0xabc", 0, 1000));

        tracker.update_status(
            "0xabc",
            TxStatus::Included {
                block_number: 42,
                block_hash: "0xblock".to_string(),
            },
        );

        let tx = tracker.get("0xabc").unwrap();
        assert_eq!(
            tx.status,
            TxStatus::Included {
                block_number: 42,
                block_hash: "0xblock".to_string(),
            }
        );
    }

    #[test]
    fn tracker_update_status_unknown_hash() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        // should not panic
        tracker.update_status("0xunknown", TxStatus::Dropped);
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn tracker_pending_query() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        tracker.track(sample_tx("0x1", 0, 1000));
        tracker.track(sample_tx("0x2", 1, 1001));
        tracker.track(sample_tx("0x3", 2, 1002));

        // move one to confirmed
        tracker.update_status(
            "0x2",
            TxStatus::Confirmed {
                block_number: 10,
                confirmations: 12,
            },
        );

        let pending = tracker.pending();
        assert_eq!(pending.len(), 2);
        let hashes: Vec<String> = pending.iter().map(|t| t.tx_hash.clone()).collect();
        assert!(hashes.contains(&"0x1".to_string()));
        assert!(hashes.contains(&"0x3".to_string()));
    }

    #[test]
    fn tracker_by_status() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        tracker.track(sample_tx("0x1", 0, 1000));
        tracker.track(sample_tx("0x2", 1, 1001));

        tracker.update_status(
            "0x1",
            TxStatus::Failed {
                reason: "out of gas".into(),
            },
        );

        let failed = tracker.by_status(&TxStatus::Failed {
            reason: String::new(),
        });
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].tx_hash, "0x1");
    }

    #[test]
    fn tracker_stuck_detection() {
        let config = TxTrackerConfig {
            stuck_timeout_secs: 60,
            ..Default::default()
        };
        let tracker = TxTracker::new(config);

        tracker.track(sample_tx("0x_old", 0, 1000));
        tracker.track(sample_tx("0x_new", 1, 1050));

        // at t = 1061, only 0x_old is stuck (61s > 60s)
        let stuck = tracker.stuck(1061);
        assert_eq!(stuck.len(), 1);
        assert_eq!(stuck[0].tx_hash, "0x_old");

        // at t = 1111, both are stuck
        let stuck = tracker.stuck(1111);
        assert_eq!(stuck.len(), 2);
    }

    #[test]
    fn tracker_stuck_ignores_non_pending() {
        let config = TxTrackerConfig {
            stuck_timeout_secs: 10,
            ..Default::default()
        };
        let tracker = TxTracker::new(config);
        tracker.track(sample_tx("0x1", 0, 100));
        tracker.update_status(
            "0x1",
            TxStatus::Confirmed {
                block_number: 5,
                confirmations: 12,
            },
        );

        let stuck = tracker.stuck(9999);
        assert!(stuck.is_empty());
    }

    #[test]
    fn tracker_max_tracked() {
        let config = TxTrackerConfig {
            max_tracked: 2,
            ..Default::default()
        };
        let tracker = TxTracker::new(config);
        tracker.track(sample_tx("0x1", 0, 1000));
        tracker.track(sample_tx("0x2", 1, 1001));
        tracker.track(sample_tx("0x3", 2, 1002)); // should be silently dropped

        assert_eq!(tracker.count(), 2);
        assert!(tracker.get("0x3").is_none());
    }

    #[test]
    fn tracker_nonce_tracking() {
        let tracker = TxTracker::new(TxTrackerConfig::default());
        assert!(tracker.next_nonce("0xAlice").is_none());

        tracker.set_nonce("0xAlice", 5);
        assert_eq!(tracker.next_nonce("0xAlice"), Some(6));

        tracker.set_nonce("0xAlice", 10);
        assert_eq!(tracker.next_nonce("0xAlice"), Some(11));
    }

    // -----------------------------------------------------------------------
    // ReceiptPoller tests
    // -----------------------------------------------------------------------

    #[test]
    fn poller_delay_first_attempt() {
        let poller = ReceiptPoller::new(ReceiptPollerConfig::default());
        let delay = poller.delay_for_attempt(1).unwrap();
        // first attempt: initial_interval * 1.5^0 = 1s
        assert_eq!(delay, Duration::from_secs(1));
    }

    #[test]
    fn poller_delay_backoff_growth() {
        let poller = ReceiptPoller::new(ReceiptPollerConfig {
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(100),
            multiplier: 2.0,
            max_attempts: 10,
        });

        // attempt 1: 1 * 2^0 = 1s
        assert_eq!(poller.delay_for_attempt(1).unwrap(), Duration::from_secs(1));
        // attempt 2: 1 * 2^1 = 2s
        assert_eq!(poller.delay_for_attempt(2).unwrap(), Duration::from_secs(2));
        // attempt 3: 1 * 2^2 = 4s
        assert_eq!(poller.delay_for_attempt(3).unwrap(), Duration::from_secs(4));
        // attempt 4: 1 * 2^3 = 8s
        assert_eq!(poller.delay_for_attempt(4).unwrap(), Duration::from_secs(8));
    }

    #[test]
    fn poller_delay_capped_at_max() {
        let poller = ReceiptPoller::new(ReceiptPollerConfig {
            initial_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(5),
            multiplier: 10.0,
            max_attempts: 10,
        });

        // attempt 2: 1 * 10^1 = 10s, but capped at 5s
        assert_eq!(poller.delay_for_attempt(2).unwrap(), Duration::from_secs(5));
    }

    #[test]
    fn poller_beyond_max_attempts() {
        let poller = ReceiptPoller::new(ReceiptPollerConfig {
            max_attempts: 3,
            ..Default::default()
        });

        assert!(poller.delay_for_attempt(3).is_some());
        assert!(poller.delay_for_attempt(4).is_none());
    }

    #[test]
    fn poller_should_continue() {
        let poller = ReceiptPoller::new(ReceiptPollerConfig {
            max_attempts: 5,
            ..Default::default()
        });

        assert!(poller.should_continue(1));
        assert!(poller.should_continue(5));
        assert!(!poller.should_continue(6));
    }

    // -----------------------------------------------------------------------
    // NonceLedger tests
    // -----------------------------------------------------------------------

    #[test]
    fn ledger_confirmed_pending_tracking() {
        let ledger = NonceLedger::new();

        assert!(ledger.confirmed_nonce("0xAlice").is_none());
        assert!(ledger.pending_nonce("0xAlice").is_none());

        ledger.set_confirmed("0xAlice", 5);
        assert_eq!(ledger.confirmed_nonce("0xAlice"), Some(5));

        ledger.mark_pending("0xAlice", 6);
        assert_eq!(ledger.pending_nonce("0xAlice"), Some(6));
    }

    #[test]
    fn ledger_next_nonce_confirmed_only() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 5);
        // next = max(5+1, 0) = 6
        assert_eq!(ledger.next("0xAlice"), 6);
    }

    #[test]
    fn ledger_next_nonce_pending_only() {
        let ledger = NonceLedger::new();
        ledger.mark_pending("0xAlice", 3);
        // next = max(0, 3+1) = 4
        assert_eq!(ledger.next("0xAlice"), 4);
    }

    #[test]
    fn ledger_next_nonce_both() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 5);
        ledger.mark_pending("0xAlice", 8);
        // next = max(5+1, 8+1) = 9
        assert_eq!(ledger.next("0xAlice"), 9);
    }

    #[test]
    fn ledger_next_nonce_unknown_address() {
        let ledger = NonceLedger::new();
        assert_eq!(ledger.next("0xNobody"), 0);
    }

    #[test]
    fn ledger_mark_pending_keeps_max() {
        let ledger = NonceLedger::new();
        ledger.mark_pending("0xAlice", 5);
        ledger.mark_pending("0xAlice", 3); // lower, should be ignored
        assert_eq!(ledger.pending_nonce("0xAlice"), Some(5));

        ledger.mark_pending("0xAlice", 7); // higher, should update
        assert_eq!(ledger.pending_nonce("0xAlice"), Some(7));
    }

    #[test]
    fn ledger_confirm_clears_pending() {
        let ledger = NonceLedger::new();
        ledger.mark_pending("0xAlice", 5);
        assert_eq!(ledger.pending_nonce("0xAlice"), Some(5));

        ledger.confirm("0xAlice", 5);
        assert_eq!(ledger.confirmed_nonce("0xAlice"), Some(5));
        // pending <= confirmed, so it should be cleared
        assert!(ledger.pending_nonce("0xAlice").is_none());
    }

    #[test]
    fn ledger_confirm_preserves_higher_pending() {
        let ledger = NonceLedger::new();
        ledger.mark_pending("0xAlice", 10);

        ledger.confirm("0xAlice", 5);
        assert_eq!(ledger.confirmed_nonce("0xAlice"), Some(5));
        // pending (10) > confirmed (5), so pending is preserved
        assert_eq!(ledger.pending_nonce("0xAlice"), Some(10));
    }

    #[test]
    fn ledger_gaps_basic() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 3);
        ledger.mark_pending("0xAlice", 7);

        let gaps = ledger.gaps("0xAlice");
        assert_eq!(gaps, vec![4, 5, 6]);
    }

    #[test]
    fn ledger_gaps_no_gap() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 5);
        ledger.mark_pending("0xAlice", 6);

        let gaps = ledger.gaps("0xAlice");
        assert!(gaps.is_empty());
    }

    #[test]
    fn ledger_gaps_no_confirmed() {
        let ledger = NonceLedger::new();
        ledger.mark_pending("0xAlice", 5);
        assert!(ledger.gaps("0xAlice").is_empty());
    }

    #[test]
    fn ledger_gaps_no_pending() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 5);
        assert!(ledger.gaps("0xAlice").is_empty());
    }

    #[test]
    fn ledger_gaps_pending_equals_confirmed() {
        let ledger = NonceLedger::new();
        ledger.set_confirmed("0xAlice", 5);
        ledger.mark_pending("0xAlice", 5);
        assert!(ledger.gaps("0xAlice").is_empty());
    }

    #[test]
    fn ledger_default_trait() {
        let ledger = NonceLedger::default();
        assert_eq!(ledger.next("0xAny"), 0);
    }

    // -----------------------------------------------------------------------
    // TxStatus display / serialize
    // -----------------------------------------------------------------------

    #[test]
    fn tx_status_display() {
        assert_eq!(TxStatus::Pending.to_string(), "pending");
        assert_eq!(TxStatus::Dropped.to_string(), "dropped");
        assert_eq!(
            TxStatus::Replaced {
                replacement_hash: "0xnew".into()
            }
            .to_string(),
            "replaced(by=0xnew)"
        );
    }

    #[test]
    fn tx_status_serialize() {
        let json = serde_json::to_string(&TxStatus::Pending).unwrap();
        assert!(json.contains("Pending"));

        let json = serde_json::to_string(&TxStatus::Included {
            block_number: 42,
            block_hash: "0xblock".into(),
        })
        .unwrap();
        assert!(json.contains("42"));
        assert!(json.contains("0xblock"));
    }
}
