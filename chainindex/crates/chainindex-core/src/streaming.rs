//! Cursor-based event streaming — enables downstream consumers to
//! subscribe to indexed events with resumable, at-least-once delivery.
//!
//! Consumers maintain a cursor (position) and can resume from where
//! they left off after crashes or restarts. Each consumer tracks its
//! own independent cursor, so multiple consumers can process the same
//! event stream at different rates.
//!
//! # Architecture
//!
//! ```text
//! Indexer  ──push()──>  EventStream  ──next_batch()──>  Consumer A
//!                           │                           Consumer B
//!                           │                           Consumer C
//!                           └── ring buffer (bounded)
//! ```
//!
//! # Reorg Handling
//!
//! When a chain reorganization is detected, call `invalidate_after(block_number)`
//! to remove all events at or above that block and bump the stream version.
//! Consumers holding cursors with an older version must re-fetch from a
//! known-good position.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::VecDeque;

use crate::error::IndexerError;
use crate::handler::DecodedEvent;

// ─── StreamCursor ────────────────────────────────────────────────────────────

/// An opaque cursor representing a position in the event stream.
///
/// Cursors are comparable and serializable, allowing consumers to persist
/// their position and resume after restarts. The `version` field is bumped
/// on reorgs to invalidate stale cursors.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamCursor {
    /// The block number of the last consumed event.
    pub block_number: u64,
    /// The log index of the last consumed event within its block.
    pub log_index: u32,
    /// Stream version — incremented on reorg to invalidate old cursors.
    pub version: u64,
}

impl StreamCursor {
    /// Create an initial cursor at the beginning of the stream.
    ///
    /// This cursor represents "no events consumed yet" and will return
    /// all available events on the first `next_batch` call.
    pub fn initial() -> Self {
        Self {
            block_number: 0,
            log_index: 0,
            version: 0,
        }
    }

    /// Serialize this cursor to a JSON string for persistence.
    ///
    /// The encoded form can be stored in a database or sent over the wire,
    /// then decoded back with `StreamCursor::decode`.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserialize a cursor from its encoded JSON string.
    ///
    /// Returns an error if the string is not a valid cursor encoding.
    pub fn decode(encoded: &str) -> Result<Self, IndexerError> {
        serde_json::from_str(encoded)
            .map_err(|e| IndexerError::Other(format!("failed to decode cursor: {}", e)))
    }

    /// Returns `true` if this cursor is strictly before the given event
    /// (i.e., the event has not yet been consumed).
    fn is_before(&self, event: &DecodedEvent) -> bool {
        if self.block_number < event.block_number {
            return true;
        }
        if self.block_number == event.block_number && self.log_index < event.log_index {
            return true;
        }
        false
    }
}

// ─── EventBatch ──────────────────────────────────────────────────────────────

/// A batch of events returned to a consumer.
///
/// Contains the events, a cursor pointing to the position after this batch,
/// and a flag indicating whether more events are available.
#[derive(Debug, Clone)]
pub struct EventBatch {
    /// The events in this batch (ordered by block_number, then log_index).
    pub events: Vec<DecodedEvent>,
    /// Cursor pointing to the position AFTER this batch.
    /// Use this cursor in the next `next_batch` call to continue.
    pub cursor: StreamCursor,
    /// `true` if there are more events available beyond this batch.
    pub has_more: bool,
}

// ─── StreamConfig ────────────────────────────────────────────────────────────

/// Configuration for event streaming.
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Maximum number of events held in the buffer (default: 10,000).
    /// When the buffer is full, the oldest events are evicted.
    pub buffer_size: usize,
    /// Maximum number of events returned per batch (default: 100).
    pub batch_size: usize,
    /// Unique identifier for this consumer.
    pub consumer_id: String,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            buffer_size: 10_000,
            batch_size: 100,
            consumer_id: String::new(),
        }
    }
}

// ─── EventStream ─────────────────────────────────────────────────────────────

/// In-memory event stream buffer that supports multiple consumers.
///
/// Events are stored in a bounded ring buffer. Each consumer tracks its
/// own cursor independently. When the buffer is full, the oldest events
/// are evicted (consumers that fall too far behind will miss events).
pub struct EventStream {
    /// Bounded event buffer (ring buffer semantics via VecDeque).
    buffer: VecDeque<DecodedEvent>,
    /// Maximum number of events in the buffer.
    buffer_size: usize,
    /// Consumer cursors, keyed by consumer ID.
    consumers: HashMap<String, StreamCursor>,
    /// Current stream version — bumped on reorg.
    version: u64,
}

impl EventStream {
    /// Create a new event stream with the given buffer capacity.
    ///
    /// # Arguments
    ///
    /// * `buffer_size` — Maximum number of events to retain. When the buffer
    ///   is full, the oldest events are evicted on push.
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(buffer_size.min(1024)),
            buffer_size,
            consumers: HashMap::new(),
            version: 0,
        }
    }

    /// Push a new event into the stream.
    ///
    /// If the buffer is at capacity, the oldest event is evicted first.
    pub fn push(&mut self, event: DecodedEvent) {
        if self.buffer.len() >= self.buffer_size {
            self.buffer.pop_front();
        }
        self.buffer.push_back(event);
    }

    /// Fetch the next batch of events for a consumer, starting after `cursor`.
    ///
    /// Returns events that come after the cursor position, up to `limit` events.
    /// The returned `EventBatch` contains a new cursor pointing to the end of
    /// the batch (use it for the next call).
    ///
    /// If the cursor version does not match the current stream version (due to
    /// a reorg), an error is returned. The consumer should re-register or use
    /// `StreamCursor::initial()`.
    pub fn next_batch(
        &self,
        cursor: &StreamCursor,
        limit: usize,
    ) -> Result<EventBatch, IndexerError> {
        // Check version mismatch (reorg invalidation)
        if cursor.version != self.version && *cursor != StreamCursor::initial() {
            return Err(IndexerError::Other(format!(
                "cursor version {} does not match stream version {} (reorg occurred)",
                cursor.version, self.version
            )));
        }

        let mut events = Vec::new();
        let is_initial = *cursor == StreamCursor::initial();

        for event in &self.buffer {
            if events.len() >= limit {
                break;
            }

            // For the initial cursor, include all events.
            // Otherwise, include events strictly after the cursor position.
            if is_initial || cursor.is_before(event) {
                events.push(event.clone());
            }
        }

        // Determine the new cursor position
        let new_cursor = if let Some(last) = events.last() {
            StreamCursor {
                block_number: last.block_number,
                log_index: last.log_index,
                version: self.version,
            }
        } else {
            StreamCursor {
                version: self.version,
                ..cursor.clone()
            }
        };

        // Check if there are more events beyond what we returned
        let total_after_cursor = self
            .buffer
            .iter()
            .filter(|e| {
                if is_initial {
                    true
                } else {
                    cursor.is_before(e)
                }
            })
            .count();
        let has_more = total_after_cursor > events.len();

        Ok(EventBatch {
            events,
            cursor: new_cursor,
            has_more,
        })
    }

    /// Register a new consumer with the given ID.
    ///
    /// The consumer starts at the initial cursor position (beginning of stream).
    /// If a consumer with this ID already exists, its cursor is reset.
    pub fn register_consumer(&mut self, id: impl Into<String>) {
        let id = id.into();
        let cursor = StreamCursor {
            version: self.version,
            ..StreamCursor::initial()
        };
        self.consumers.insert(id, cursor);
    }

    /// Get the current cursor for a registered consumer.
    ///
    /// Returns `None` if the consumer is not registered.
    pub fn get_consumer_cursor(&self, id: &str) -> Option<&StreamCursor> {
        self.consumers.get(id)
    }

    /// Update a consumer's cursor (e.g., after processing a batch).
    pub fn update_consumer_cursor(&mut self, id: &str, cursor: StreamCursor) {
        if let Some(entry) = self.consumers.get_mut(id) {
            *entry = cursor;
        }
    }

    /// Invalidate all events at or above `block_number` (reorg handling).
    ///
    /// Removes affected events from the buffer and increments the stream
    /// version, which invalidates all outstanding cursors.
    pub fn invalidate_after(&mut self, block_number: u64) {
        self.buffer.retain(|e| e.block_number < block_number);
        self.version += 1;

        // Reset all consumer cursors to account for the version bump
        for cursor in self.consumers.values_mut() {
            // Adjust cursor if it pointed at or beyond the invalidated range
            if cursor.block_number >= block_number {
                cursor.block_number = block_number.saturating_sub(1);
                cursor.log_index = 0;
            }
            cursor.version = self.version;
        }

        tracing::info!(
            block_number,
            version = self.version,
            "invalidated events after reorg"
        );
    }

    /// Returns the current number of events in the buffer.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the current stream version.
    pub fn version(&self) -> u64 {
        self.version
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a DecodedEvent at the given block/log position.
    fn event_at(block: u64, log_index: u32) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "ERC20Transfer".into(),
            address: "0xdead".into(),
            tx_hash: format!("0xtx_{block}_{log_index}"),
            block_number: block,
            log_index,
            fields_json: serde_json::json!({"value": "1000"}),
        }
    }

    // ── Test: push events into stream ────────────────────────────────────────

    #[test]
    fn push_events() {
        let mut stream = EventStream::new(100);
        assert!(stream.is_empty());

        stream.push(event_at(1, 0));
        stream.push(event_at(1, 1));
        stream.push(event_at(2, 0));

        assert_eq!(stream.len(), 3);
    }

    // ── Test: next_batch returns events after cursor ─────────────────────────

    #[test]
    fn next_batch_returns_events_after_cursor() {
        let mut stream = EventStream::new(100);
        stream.push(event_at(1, 0));
        stream.push(event_at(1, 1));
        stream.push(event_at(2, 0));
        stream.push(event_at(2, 1));
        stream.push(event_at(3, 0));

        // Start from initial cursor — should get all events (up to limit)
        let batch = stream.next_batch(&StreamCursor::initial(), 100).unwrap();
        assert_eq!(batch.events.len(), 5);
        assert!(!batch.has_more);

        // Now use the returned cursor to get the next batch — should be empty
        let batch2 = stream.next_batch(&batch.cursor, 100).unwrap();
        assert_eq!(batch2.events.len(), 0);
        assert!(!batch2.has_more);
    }

    // ── Test: empty batch when caught up ─────────────────────────────────────

    #[test]
    fn empty_batch_when_caught_up() {
        let mut stream = EventStream::new(100);
        stream.push(event_at(1, 0));

        let batch = stream.next_batch(&StreamCursor::initial(), 100).unwrap();
        assert_eq!(batch.events.len(), 1);

        // Consumer is caught up — should get empty batch
        let batch2 = stream.next_batch(&batch.cursor, 100).unwrap();
        assert_eq!(batch2.events.len(), 0);
        assert!(!batch2.has_more);
    }

    // ── Test: cursor serialization roundtrip ─────────────────────────────────

    #[test]
    fn cursor_serialization_roundtrip() {
        let cursor = StreamCursor {
            block_number: 12345,
            log_index: 42,
            version: 7,
        };

        let encoded = cursor.encode();
        let decoded = StreamCursor::decode(&encoded).unwrap();

        assert_eq!(cursor, decoded);
    }

    // ── Test: invalid cursor decode ──────────────────────────────────────────

    #[test]
    fn cursor_decode_invalid() {
        let result = StreamCursor::decode("not-valid-json");
        assert!(result.is_err());
    }

    // ── Test: multiple consumers with independent cursors ────────────────────

    #[test]
    fn multiple_consumers_independent_cursors() {
        let mut stream = EventStream::new(100);
        stream.register_consumer("consumer_a");
        stream.register_consumer("consumer_b");

        stream.push(event_at(1, 0));
        stream.push(event_at(2, 0));
        stream.push(event_at(3, 0));

        // Consumer A reads all events
        let cursor_a = stream.get_consumer_cursor("consumer_a").unwrap().clone();
        let batch_a = stream.next_batch(&cursor_a, 100).unwrap();
        assert_eq!(batch_a.events.len(), 3);
        stream.update_consumer_cursor("consumer_a", batch_a.cursor.clone());

        // Consumer B reads only 1 event
        let cursor_b = stream.get_consumer_cursor("consumer_b").unwrap().clone();
        let batch_b = stream.next_batch(&cursor_b, 1).unwrap();
        assert_eq!(batch_b.events.len(), 1);
        assert!(batch_b.has_more);
        stream.update_consumer_cursor("consumer_b", batch_b.cursor.clone());

        // Consumer A is caught up
        let cursor_a2 = stream.get_consumer_cursor("consumer_a").unwrap().clone();
        let batch_a2 = stream.next_batch(&cursor_a2, 100).unwrap();
        assert_eq!(batch_a2.events.len(), 0);

        // Consumer B still has events
        let cursor_b2 = stream.get_consumer_cursor("consumer_b").unwrap().clone();
        let batch_b2 = stream.next_batch(&cursor_b2, 100).unwrap();
        assert_eq!(batch_b2.events.len(), 2);
    }

    // ── Test: reorg invalidation ─────────────────────────────────────────────

    #[test]
    fn reorg_invalidation() {
        let mut stream = EventStream::new(100);
        stream.push(event_at(1, 0));
        stream.push(event_at(2, 0));
        stream.push(event_at(3, 0));
        stream.push(event_at(4, 0));

        assert_eq!(stream.len(), 4);
        assert_eq!(stream.version(), 0);

        // Reorg at block 3 — blocks 3 and 4 are invalidated
        stream.invalidate_after(3);

        assert_eq!(stream.len(), 2); // only blocks 1 and 2 remain
        assert_eq!(stream.version(), 1);

        // Old cursor with version 0 should fail (unless it's the initial cursor)
        let old_cursor = StreamCursor {
            block_number: 1,
            log_index: 0,
            version: 0,
        };
        let result = stream.next_batch(&old_cursor, 100);
        assert!(result.is_err());

        // New cursor with version 1 should work
        let new_cursor = StreamCursor {
            block_number: 0,
            log_index: 0,
            version: 1,
        };
        let batch = stream.next_batch(&new_cursor, 100).unwrap();
        assert_eq!(batch.events.len(), 2);
    }

    // ── Test: buffer overflow evicts oldest ───────────────────────────────────

    #[test]
    fn buffer_overflow_evicts_oldest() {
        let mut stream = EventStream::new(3); // very small buffer

        stream.push(event_at(1, 0));
        stream.push(event_at(2, 0));
        stream.push(event_at(3, 0));
        assert_eq!(stream.len(), 3);

        // Pushing a 4th event should evict the oldest (block 1)
        stream.push(event_at(4, 0));
        assert_eq!(stream.len(), 3);

        let batch = stream.next_batch(&StreamCursor::initial(), 100).unwrap();
        assert_eq!(batch.events.len(), 3);
        // First event should be block 2 (block 1 was evicted)
        assert_eq!(batch.events[0].block_number, 2);
        assert_eq!(batch.events[2].block_number, 4);
    }

    // ── Test: batch size limiting ────────────────────────────────────────────

    #[test]
    fn batch_size_limiting() {
        let mut stream = EventStream::new(100);
        for i in 0..20 {
            stream.push(event_at(i, 0));
        }

        // Request only 5 events
        let batch = stream.next_batch(&StreamCursor::initial(), 5).unwrap();
        assert_eq!(batch.events.len(), 5);
        assert!(batch.has_more);

        // Continue from returned cursor
        let batch2 = stream.next_batch(&batch.cursor, 5).unwrap();
        assert_eq!(batch2.events.len(), 5);
        assert!(batch2.has_more);
    }

    // ── Test: initial cursor values ──────────────────────────────────────────

    #[test]
    fn initial_cursor_values() {
        let cursor = StreamCursor::initial();
        assert_eq!(cursor.block_number, 0);
        assert_eq!(cursor.log_index, 0);
        assert_eq!(cursor.version, 0);
    }

    // ── Test: register consumer creates cursor ───────────────────────────────

    #[test]
    fn register_consumer_creates_cursor() {
        let mut stream = EventStream::new(100);

        assert!(stream.get_consumer_cursor("test").is_none());

        stream.register_consumer("test");

        let cursor = stream.get_consumer_cursor("test").unwrap();
        assert_eq!(cursor.block_number, 0);
        assert_eq!(cursor.log_index, 0);
        assert_eq!(cursor.version, 0);
    }

    // ── Test: reorg updates consumer cursors ─────────────────────────────────

    #[test]
    fn reorg_updates_consumer_cursors() {
        let mut stream = EventStream::new(100);
        stream.register_consumer("c1");

        stream.push(event_at(1, 0));
        stream.push(event_at(2, 0));
        stream.push(event_at(3, 0));

        // Consumer reads up to block 3
        let cursor = stream.get_consumer_cursor("c1").unwrap().clone();
        let batch = stream.next_batch(&cursor, 100).unwrap();
        stream.update_consumer_cursor("c1", batch.cursor);

        // Reorg at block 2
        stream.invalidate_after(2);

        // Consumer cursor should be updated to version 1
        let updated_cursor = stream.get_consumer_cursor("c1").unwrap();
        assert_eq!(updated_cursor.version, 1);
        assert!(updated_cursor.block_number < 2);
    }

    // ── Test: empty stream returns empty batch ───────────────────────────────

    #[test]
    fn empty_stream_returns_empty_batch() {
        let stream = EventStream::new(100);
        let batch = stream.next_batch(&StreamCursor::initial(), 100).unwrap();
        assert!(batch.events.is_empty());
        assert!(!batch.has_more);
    }
}
