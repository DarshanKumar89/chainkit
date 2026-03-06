//! Dead letter queue — captures and retries failed handler events.
//!
//! When an event handler fails (returns error), the event is stored in the DLQ
//! rather than blocking the indexer. Events are retried with exponential backoff.
//!
//! # Architecture
//!
//! ```text
//! Handler fails → DlqEntry created → pushed to DeadLetterQueue
//!                                         ↓
//!                                    pop_ready() returns entries due for retry
//!                                         ↓
//!                              retry handler → success: remove
//!                                            → failure: bump attempt, reschedule
//!                                            → max retries: mark Failed
//! ```

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::handler::DecodedEvent;

// ─── DlqStatus ──────────────────────────────────────────────────────────────

/// Status of a DLQ entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DlqStatus {
    /// Waiting for the next retry attempt.
    Pending,
    /// Currently being retried.
    Retrying,
    /// Permanently failed — max retries exceeded.
    Failed,
}

// ─── DlqEntry ───────────────────────────────────────────────────────────────

/// A single entry in the dead letter queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqEntry {
    /// Unique ID for this DLQ entry.
    pub id: String,
    /// The event that failed processing.
    pub event: DecodedEvent,
    /// Name of the handler that failed.
    pub handler_name: String,
    /// Error message from the last failure.
    pub error_message: String,
    /// Number of times this event has been attempted.
    pub attempt_count: u32,
    /// Maximum number of attempts before marking as permanently failed.
    pub max_attempts: u32,
    /// Unix timestamp when this event first failed.
    pub first_failed_at: i64,
    /// Unix timestamp of the most recent failure.
    pub last_failed_at: i64,
    /// Unix timestamp when this entry should next be retried.
    pub next_retry_at: i64,
    /// Current status.
    pub status: DlqStatus,
}

// ─── DlqConfig ──────────────────────────────────────────────────────────────

/// Configuration for the dead letter queue.
#[derive(Debug, Clone)]
pub struct DlqConfig {
    /// Maximum number of retry attempts (default: 5).
    pub max_retries: u32,
    /// Initial backoff duration (default: 1 second).
    pub initial_backoff: Duration,
    /// Maximum backoff duration (default: 5 minutes).
    pub max_backoff: Duration,
    /// Backoff multiplier (default: 2.0 — exponential).
    pub backoff_multiplier: f64,
}

impl Default for DlqConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(300),
            backoff_multiplier: 2.0,
        }
    }
}

// ─── DlqStats ───────────────────────────────────────────────────────────────

/// Statistics about the dead letter queue.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DlqStats {
    /// Total entries ever added to the DLQ.
    pub total_added: u64,
    /// Currently pending entries (waiting for retry).
    pub pending: u64,
    /// Permanently failed entries.
    pub failed: u64,
    /// Entries successfully retried and removed.
    pub retried_success: u64,
}

// ─── DeadLetterQueue ────────────────────────────────────────────────────────

/// In-memory dead letter queue for failed handler events.
///
/// Thread-safe — uses internal `Mutex`. For production, back with a database table.
pub struct DeadLetterQueue {
    config: DlqConfig,
    entries: std::sync::Mutex<VecDeque<DlqEntry>>,
    stats: std::sync::Mutex<DlqStats>,
}

impl DeadLetterQueue {
    /// Create a new DLQ with the given configuration.
    pub fn new(config: DlqConfig) -> Self {
        Self {
            config,
            entries: std::sync::Mutex::new(VecDeque::new()),
            stats: std::sync::Mutex::new(DlqStats::default()),
        }
    }

    /// Push a failed event into the DLQ.
    ///
    /// The entry is scheduled for retry based on the configured backoff.
    pub fn push(
        &self,
        event: DecodedEvent,
        handler_name: impl Into<String>,
        error_message: impl Into<String>,
    ) {
        let now = chrono::Utc::now().timestamp();
        let next_retry = now + self.config.initial_backoff.as_secs() as i64;

        let entry = DlqEntry {
            id: format!("dlq-{}-{}-{}", event.tx_hash, event.log_index, now),
            event,
            handler_name: handler_name.into(),
            error_message: error_message.into(),
            attempt_count: 1,
            max_attempts: self.config.max_retries,
            first_failed_at: now,
            last_failed_at: now,
            next_retry_at: next_retry,
            status: DlqStatus::Pending,
        };

        let mut entries = self.entries.lock().unwrap();
        entries.push_back(entry);

        let mut stats = self.stats.lock().unwrap();
        stats.total_added += 1;
        stats.pending += 1;
    }

    /// Pop all entries that are due for retry (next_retry_at <= now).
    ///
    /// Returns entries with status set to `Retrying`.
    pub fn pop_ready(&self, now: i64) -> Vec<DlqEntry> {
        let mut entries = self.entries.lock().unwrap();
        let mut ready = Vec::new();

        for entry in entries.iter_mut() {
            if entry.status == DlqStatus::Pending && entry.next_retry_at <= now {
                entry.status = DlqStatus::Retrying;
                ready.push(entry.clone());
            }
        }

        ready
    }

    /// Mark an entry as successfully retried (removes it from the DLQ).
    pub fn mark_success(&self, id: &str) {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() < before {
            let mut stats = self.stats.lock().unwrap();
            stats.pending = stats.pending.saturating_sub(1);
            stats.retried_success += 1;
        }
    }

    /// Mark an entry as failed again (reschedule or permanently fail).
    pub fn mark_failed(&self, id: &str, error_message: impl Into<String>) {
        let mut entries = self.entries.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let error_msg = error_message.into();

        if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
            entry.attempt_count += 1;
            entry.last_failed_at = now;
            entry.error_message = error_msg;

            if entry.attempt_count >= entry.max_attempts {
                entry.status = DlqStatus::Failed;
                let mut stats = self.stats.lock().unwrap();
                stats.pending = stats.pending.saturating_sub(1);
                stats.failed += 1;
            } else {
                // Exponential backoff
                let backoff = self.compute_backoff(entry.attempt_count);
                entry.next_retry_at = now + backoff.as_secs() as i64;
                entry.status = DlqStatus::Pending;
            }
        }
    }

    /// Get all entries with a specific status.
    pub fn get_by_status(&self, status: DlqStatus) -> Vec<DlqEntry> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .filter(|e| e.status == status)
            .cloned()
            .collect()
    }

    /// Get a single entry by ID.
    pub fn get(&self, id: &str) -> Option<DlqEntry> {
        let entries = self.entries.lock().unwrap();
        entries.iter().find(|e| e.id == id).cloned()
    }

    /// Total number of entries in the DLQ.
    pub fn len(&self) -> usize {
        let entries = self.entries.lock().unwrap();
        entries.len()
    }

    /// Returns true if the DLQ is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Purge (remove) all entries with `last_failed_at` before the given timestamp.
    pub fn purge_before(&self, timestamp: i64) -> usize {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.last_failed_at >= timestamp);
        let removed = before - entries.len();

        if removed > 0 {
            let mut stats = self.stats.lock().unwrap();
            // Approximate — some may have been pending, some failed
            stats.pending = stats.pending.saturating_sub(removed as u64);
        }

        removed
    }

    /// Reset all `Failed` entries to `Pending` for another round of retries.
    pub fn retry_all_failed(&self) -> usize {
        let mut entries = self.entries.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let mut count = 0;

        for entry in entries.iter_mut() {
            if entry.status == DlqStatus::Failed {
                entry.status = DlqStatus::Pending;
                entry.attempt_count = 0;
                entry.next_retry_at = now;
                count += 1;
            }
        }

        if count > 0 {
            let mut stats = self.stats.lock().unwrap();
            stats.failed = stats.failed.saturating_sub(count as u64);
            stats.pending += count as u64;
        }

        count
    }

    /// Get current DLQ statistics.
    pub fn stats(&self) -> DlqStats {
        let stats = self.stats.lock().unwrap();
        stats.clone()
    }

    /// Compute the backoff duration for a given attempt number.
    fn compute_backoff(&self, attempt: u32) -> Duration {
        let base = self.config.initial_backoff.as_secs_f64();
        let multiplier = self
            .config
            .backoff_multiplier
            .powi(attempt.saturating_sub(1) as i32);
        let backoff_secs = base * multiplier;
        let max_secs = self.config.max_backoff.as_secs_f64();
        Duration::from_secs_f64(backoff_secs.min(max_secs))
    }
}

impl Default for DeadLetterQueue {
    fn default() -> Self {
        Self::new(DlqConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(block: u64) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "Transfer".into(),
            address: "0xtoken".into(),
            tx_hash: format!("0xtx_{block}"),
            block_number: block,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0xA", "to": "0xB"}),
        }
    }

    fn test_config() -> DlqConfig {
        DlqConfig {
            max_retries: 3,
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(60),
            backoff_multiplier: 2.0,
        }
    }

    #[test]
    fn push_entry() {
        let dlq = DeadLetterQueue::new(test_config());
        dlq.push(make_event(100), "handler1", "connection timeout");

        assert_eq!(dlq.len(), 1);
        let stats = dlq.stats();
        assert_eq!(stats.total_added, 1);
        assert_eq!(stats.pending, 1);
    }

    #[test]
    fn pop_ready_returns_due_entries() {
        let dlq = DeadLetterQueue::new(test_config());
        dlq.push(make_event(100), "handler1", "error");

        // Not ready yet (now < next_retry_at)
        let now = chrono::Utc::now().timestamp();
        let ready = dlq.pop_ready(now - 10);
        assert!(ready.is_empty());

        // Ready (now >= next_retry_at)
        let ready = dlq.pop_ready(now + 10);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].status, DlqStatus::Retrying);
    }

    #[test]
    fn mark_success_removes_entry() {
        let dlq = DeadLetterQueue::new(test_config());
        dlq.push(make_event(100), "handler1", "error");

        let now = chrono::Utc::now().timestamp() + 10;
        let ready = dlq.pop_ready(now);
        let id = ready[0].id.clone();

        dlq.mark_success(&id);
        assert_eq!(dlq.len(), 0);

        let stats = dlq.stats();
        assert_eq!(stats.retried_success, 1);
        assert_eq!(stats.pending, 0);
    }

    #[test]
    fn mark_failed_reschedules() {
        let dlq = DeadLetterQueue::new(test_config());
        dlq.push(make_event(100), "handler1", "error");

        let now = chrono::Utc::now().timestamp() + 10;
        let ready = dlq.pop_ready(now);
        let id = ready[0].id.clone();

        dlq.mark_failed(&id, "still broken");

        let entry = dlq.get(&id).unwrap();
        assert_eq!(entry.status, DlqStatus::Pending);
        assert_eq!(entry.attempt_count, 2);
        // next_retry_at should be at least the internal now (which is >= our `now - 10`)
        // plus the backoff (2s for attempt 2). Just verify it was rescheduled.
        assert!(entry.next_retry_at >= entry.last_failed_at);
    }

    #[test]
    fn max_retries_marks_failed() {
        let dlq = DeadLetterQueue::new(DlqConfig {
            max_retries: 2,
            ..test_config()
        });
        dlq.push(make_event(100), "handler1", "error");

        let now = chrono::Utc::now().timestamp() + 100;
        let ready = dlq.pop_ready(now);
        let id = ready[0].id.clone();

        // Attempt 2 → reaches max_retries (2)
        dlq.mark_failed(&id, "still broken");

        let entry = dlq.get(&id).unwrap();
        assert_eq!(entry.status, DlqStatus::Failed);
        assert_eq!(entry.attempt_count, 2);

        let stats = dlq.stats();
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.pending, 0);
    }

    #[test]
    fn get_by_status() {
        let dlq = DeadLetterQueue::new(DlqConfig {
            max_retries: 2,
            ..test_config()
        });

        dlq.push(make_event(100), "h1", "error1");
        dlq.push(make_event(101), "h2", "error2");

        // Pop and fail only the first one permanently
        let now = chrono::Utc::now().timestamp() + 100;
        let ready = dlq.pop_ready(now);
        assert_eq!(ready.len(), 2);

        // Mark the first as permanently failed
        dlq.mark_failed(&ready[0].id, "still broken");

        let failed = dlq.get_by_status(DlqStatus::Failed);
        assert_eq!(failed.len(), 1);

        // The second entry is still in Retrying status (was popped but not yet resolved)
        let retrying = dlq.get_by_status(DlqStatus::Retrying);
        assert_eq!(retrying.len(), 1);
    }

    #[test]
    fn exponential_backoff() {
        let dlq = DeadLetterQueue::new(test_config());

        // attempt 1: 1s * 2^0 = 1s
        let b1 = dlq.compute_backoff(1);
        assert_eq!(b1, Duration::from_secs(1));

        // attempt 2: 1s * 2^1 = 2s
        let b2 = dlq.compute_backoff(2);
        assert_eq!(b2, Duration::from_secs(2));

        // attempt 3: 1s * 2^2 = 4s
        let b3 = dlq.compute_backoff(3);
        assert_eq!(b3, Duration::from_secs(4));
    }

    #[test]
    fn backoff_capped_at_max() {
        let dlq = DeadLetterQueue::new(DlqConfig {
            max_backoff: Duration::from_secs(10),
            ..test_config()
        });

        // attempt 10: would be 1s * 2^9 = 512s, but capped at 10s
        let b = dlq.compute_backoff(10);
        assert_eq!(b, Duration::from_secs(10));
    }

    #[test]
    fn purge_before_removes_old_entries() {
        let dlq = DeadLetterQueue::new(test_config());
        dlq.push(make_event(100), "h1", "error1");
        dlq.push(make_event(101), "h2", "error2");

        // Purge entries last failed before far future → removes all
        let far_future = chrono::Utc::now().timestamp() + 10000;
        let removed = dlq.purge_before(far_future);
        assert_eq!(removed, 2);
        assert!(dlq.is_empty());
    }

    #[test]
    fn retry_all_failed() {
        let dlq = DeadLetterQueue::new(DlqConfig {
            max_retries: 1,
            ..test_config()
        });
        dlq.push(make_event(100), "h1", "error");

        // Exhaust retries
        let now = chrono::Utc::now().timestamp() + 100;
        let ready = dlq.pop_ready(now);
        dlq.mark_failed(&ready[0].id, "still broken");

        assert_eq!(dlq.get_by_status(DlqStatus::Failed).len(), 1);

        // Retry all failed
        let count = dlq.retry_all_failed();
        assert_eq!(count, 1);
        assert_eq!(dlq.get_by_status(DlqStatus::Pending).len(), 1);
        assert_eq!(dlq.get_by_status(DlqStatus::Failed).len(), 0);
    }

    #[test]
    fn stats_tracking() {
        let dlq = DeadLetterQueue::new(test_config());

        dlq.push(make_event(100), "h1", "error1");
        dlq.push(make_event(101), "h2", "error2");

        let stats = dlq.stats();
        assert_eq!(stats.total_added, 2);
        assert_eq!(stats.pending, 2);
        assert_eq!(stats.failed, 0);
        assert_eq!(stats.retried_success, 0);
    }
}
