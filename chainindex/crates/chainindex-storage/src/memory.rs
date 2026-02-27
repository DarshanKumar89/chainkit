//! In-memory storage backend.
//!
//! Stores indexed events, block hashes, and checkpoints in RAM.
//! Useful for testing and short-lived indexers that don't need persistence.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::types::BlockSummary;

/// In-memory indexer storage.
///
/// All data is lost when the process exits.
#[derive(Default)]
pub struct InMemoryStorage {
    checkpoints: Mutex<HashMap<String, Checkpoint>>,
    events: Mutex<Vec<DecodedEvent>>,
    block_hashes: Mutex<HashMap<u64, String>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a decoded event.
    pub fn insert_event(&self, event: DecodedEvent) {
        self.events.lock().unwrap().push(event);
    }

    /// Record a block hash for reorg detection.
    pub fn insert_block_hash(&self, block_number: u64, hash: String) {
        self.block_hashes.lock().unwrap().insert(block_number, hash);
    }

    /// Look up the hash of a previously indexed block.
    pub fn get_block_hash(&self, block_number: u64) -> Option<String> {
        self.block_hashes.lock().unwrap().get(&block_number).cloned()
    }

    /// Return all indexed events for a schema (e.g. `"ERC20Transfer"`).
    pub fn events_by_schema(&self, schema: &str) -> Vec<DecodedEvent> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.schema == schema)
            .cloned()
            .collect()
    }

    /// Total number of indexed events.
    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// Rollback (delete) events at blocks after `block_number` (reorg recovery).
    pub fn rollback_after(&self, block_number: u64) {
        let mut events = self.events.lock().unwrap();
        events.retain(|e| e.block_number <= block_number);
        let mut hashes = self.block_hashes.lock().unwrap();
        hashes.retain(|num, _| *num <= block_number);
    }
}

#[async_trait]
impl CheckpointStore for InMemoryStorage {
    async fn load(
        &self,
        chain_id: &str,
        indexer_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError> {
        let key = format!("{chain_id}:{indexer_id}");
        Ok(self.checkpoints.lock().unwrap().get(&key).cloned())
    }

    async fn save(&self, checkpoint: Checkpoint) -> Result<(), IndexerError> {
        let key = format!("{}:{}", checkpoint.chain_id, checkpoint.indexer_id);
        self.checkpoints.lock().unwrap().insert(key, checkpoint);
        Ok(())
    }

    async fn delete(&self, chain_id: &str, indexer_id: &str) -> Result<(), IndexerError> {
        let key = format!("{chain_id}:{indexer_id}");
        self.checkpoints.lock().unwrap().remove(&key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(schema: &str, block: u64) -> DecodedEvent {
        DecodedEvent {
            schema: schema.to_string(),
            address: "0x0".into(),
            tx_hash: "0x0".into(),
            block_number: block,
            log_index: 0,
            fields_json: serde_json::Value::Null,
        }
    }

    #[test]
    fn insert_and_query_events() {
        let store = InMemoryStorage::new();
        store.insert_event(ev("ERC20Transfer", 100));
        store.insert_event(ev("ERC20Transfer", 101));
        store.insert_event(ev("UniswapSwap", 102));

        let transfers = store.events_by_schema("ERC20Transfer");
        assert_eq!(transfers.len(), 2);

        let swaps = store.events_by_schema("UniswapSwap");
        assert_eq!(swaps.len(), 1);
    }

    #[test]
    fn rollback_clears_future_events() {
        let store = InMemoryStorage::new();
        for i in 100..=105 {
            store.insert_event(ev("Transfer", i));
            store.insert_block_hash(i, format!("0x{i}"));
        }
        assert_eq!(store.event_count(), 6);

        store.rollback_after(102);

        assert_eq!(store.event_count(), 3); // 100, 101, 102 remain
        assert!(store.get_block_hash(103).is_none());
        assert!(store.get_block_hash(102).is_some());
    }

    #[tokio::test]
    async fn checkpoint_roundtrip() {
        let store = InMemoryStorage::new();
        let cp = Checkpoint {
            chain_id: "ethereum".into(),
            indexer_id: "test".into(),
            block_number: 1000,
            block_hash: "0xabc".into(),
            updated_at: 0,
        };
        store.save(cp).await.unwrap();
        let loaded = store.load("ethereum", "test").await.unwrap().unwrap();
        assert_eq!(loaded.block_number, 1000);
    }
}
