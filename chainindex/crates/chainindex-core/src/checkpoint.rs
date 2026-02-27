//! Checkpoint manager — persists the indexer's position for crash recovery.
//!
//! A checkpoint stores the last successfully processed block number and hash.
//! On restart, the indexer resumes from the last checkpoint rather than
//! re-indexing from scratch.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::IndexerError;

/// A persisted checkpoint for an indexer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Chain slug (e.g. `"ethereum"`).
    pub chain_id: String,
    /// Unique indexer identifier.
    pub indexer_id: String,
    /// Last successfully processed block number.
    pub block_number: u64,
    /// Last successfully processed block hash.
    pub block_hash: String,
    /// Unix timestamp of when this checkpoint was saved.
    pub updated_at: i64,
}

/// Trait for storing and loading checkpoints.
///
/// Implementations include `MemoryCheckpointStore`, `SqliteCheckpointStore`,
/// and `PostgresCheckpointStore`.
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    /// Load the latest checkpoint for a given chain + indexer pair.
    async fn load(
        &self,
        chain_id: &str,
        indexer_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError>;

    /// Save (upsert) a checkpoint.
    async fn save(&self, checkpoint: Checkpoint) -> Result<(), IndexerError>;

    /// Delete a checkpoint (e.g. when resetting an indexer).
    async fn delete(&self, chain_id: &str, indexer_id: &str) -> Result<(), IndexerError>;
}

/// Manages checkpoint reads/writes for an indexer.
pub struct CheckpointManager {
    store: Box<dyn CheckpointStore>,
    chain_id: String,
    indexer_id: String,
    /// How often to save (every N blocks).
    save_interval: u64,
    /// Block counter since last save.
    counter: u64,
}

impl CheckpointManager {
    pub fn new(
        store: Box<dyn CheckpointStore>,
        chain_id: impl Into<String>,
        indexer_id: impl Into<String>,
        save_interval: u64,
    ) -> Self {
        Self {
            store,
            chain_id: chain_id.into(),
            indexer_id: indexer_id.into(),
            save_interval,
            counter: 0,
        }
    }

    /// Load the saved checkpoint (returns `None` if none exists).
    pub async fn load(&self) -> Result<Option<Checkpoint>, IndexerError> {
        self.store.load(&self.chain_id, &self.indexer_id).await
    }

    /// Conditionally save a checkpoint every `save_interval` blocks.
    ///
    /// Call this after each block is successfully processed.
    pub async fn maybe_save(
        &mut self,
        block_number: u64,
        block_hash: &str,
    ) -> Result<(), IndexerError> {
        self.counter += 1;
        if self.counter >= self.save_interval {
            self.force_save(block_number, block_hash).await?;
            self.counter = 0;
        }
        Ok(())
    }

    /// Immediately save a checkpoint (used on shutdown / reorg recovery).
    pub async fn force_save(
        &self,
        block_number: u64,
        block_hash: &str,
    ) -> Result<(), IndexerError> {
        let cp = Checkpoint {
            chain_id: self.chain_id.clone(),
            indexer_id: self.indexer_id.clone(),
            block_number,
            block_hash: block_hash.to_string(),
            updated_at: chrono::Utc::now().timestamp(),
        };
        self.store.save(cp).await
    }
}

// ─── In-memory store (for testing) ────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Mutex;

/// In-memory checkpoint store for tests and ephemeral indexers.
#[derive(Default)]
pub struct MemoryCheckpointStore {
    data: Mutex<HashMap<String, Checkpoint>>,
}

impl MemoryCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(chain_id: &str, indexer_id: &str) -> String {
        format!("{chain_id}:{indexer_id}")
    }
}

#[async_trait]
impl CheckpointStore for MemoryCheckpointStore {
    async fn load(
        &self,
        chain_id: &str,
        indexer_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError> {
        Ok(self.data.lock().unwrap().get(&Self::key(chain_id, indexer_id)).cloned())
    }

    async fn save(&self, checkpoint: Checkpoint) -> Result<(), IndexerError> {
        let key = Self::key(&checkpoint.chain_id, &checkpoint.indexer_id);
        self.data.lock().unwrap().insert(key, checkpoint);
        Ok(())
    }

    async fn delete(&self, chain_id: &str, indexer_id: &str) -> Result<(), IndexerError> {
        self.data.lock().unwrap().remove(&Self::key(chain_id, indexer_id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_roundtrip() {
        let store = Box::new(MemoryCheckpointStore::new());
        let mut mgr = CheckpointManager::new(store, "ethereum", "my-indexer", 10);

        // No checkpoint initially
        assert!(mgr.load().await.unwrap().is_none());

        // Force save
        mgr.force_save(1000, "0xabc").await.unwrap();

        // Load should return the checkpoint
        let cp = mgr.load().await.unwrap().unwrap();
        assert_eq!(cp.block_number, 1000);
        assert_eq!(cp.block_hash, "0xabc");
        assert_eq!(cp.chain_id, "ethereum");
    }

    #[tokio::test]
    async fn checkpoint_save_interval() {
        let store = Box::new(MemoryCheckpointStore::new());
        let mut mgr = CheckpointManager::new(store, "ethereum", "idx", 5);

        // Process 4 blocks — should not save yet
        for i in 1..=4 {
            mgr.maybe_save(i, "0xhash").await.unwrap();
        }
        assert!(mgr.load().await.unwrap().is_none());

        // 5th block — should save
        mgr.maybe_save(5, "0xhash5").await.unwrap();
        let cp = mgr.load().await.unwrap().unwrap();
        assert_eq!(cp.block_number, 5);
    }
}
