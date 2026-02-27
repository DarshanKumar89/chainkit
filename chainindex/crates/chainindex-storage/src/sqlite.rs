//! SQLite storage backend for ChainIndex.
//!
//! Persists checkpoints, events, and block hashes to a single SQLite file.
//! Uses `sqlx` with WAL mode for concurrent read performance.
//!
//! # Usage
//! ```rust,no_run
//! use chainindex_storage::sqlite::SqliteStorage;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // File-backed (persistent)
//! let store = SqliteStorage::open("./index.db").await?;
//!
//! // In-memory (tests / ephemeral)
//! let store = SqliteStorage::in_memory().await?;
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use tracing::debug;

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;

/// SQLite-backed storage for checkpoints, events, and block hashes.
pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at `path`.
    ///
    /// The path may be a plain file path (`"./index.db"`) or a full
    /// SQLite URL (`"sqlite:./index.db?mode=rwc"`).
    pub async fn open(path: &str) -> Result<Self, IndexerError> {
        let url = if path.starts_with("sqlite:") {
            path.to_string()
        } else {
            format!("sqlite:{path}?mode=rwc")
        };

        let pool = SqlitePool::connect(&url)
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Open an in-memory SQLite database.
    ///
    /// All data is lost when the pool is dropped. Ideal for tests.
    pub async fn in_memory() -> Result<Self, IndexerError> {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Create tables and enable WAL mode.
    async fn init_schema(&self) -> Result<(), IndexerError> {
        // WAL mode — better concurrent read throughput
        sqlx::query("PRAGMA journal_mode=WAL;")
            .execute(&self.pool)
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Checkpoint table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS checkpoints (
                chain_id     TEXT    NOT NULL,
                indexer_id   TEXT    NOT NULL,
                block_number INTEGER NOT NULL,
                block_hash   TEXT    NOT NULL,
                updated_at   INTEGER NOT NULL,
                PRIMARY KEY (chain_id, indexer_id)
            );",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Block-hash table (for reorg detection)
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS block_hashes (
                chain_id     TEXT    NOT NULL,
                block_number INTEGER NOT NULL,
                block_hash   TEXT    NOT NULL,
                PRIMARY KEY (chain_id, block_number)
            );",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Events table
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                schema       TEXT    NOT NULL,
                address      TEXT    NOT NULL,
                tx_hash      TEXT    NOT NULL,
                block_number INTEGER NOT NULL,
                log_index    INTEGER NOT NULL,
                fields_json  TEXT    NOT NULL
            );",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Indexes for common query patterns
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_events_schema ON events (schema);",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_events_block ON events (block_number);",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(())
    }

    // ─── Event storage ──────────────────────────────────────────────────────────

    /// Insert a decoded event into the database.
    pub async fn insert_event(&self, event: &DecodedEvent) -> Result<(), IndexerError> {
        let fields = serde_json::to_string(&event.fields_json)
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        sqlx::query(
            "INSERT INTO events (schema, address, tx_hash, block_number, log_index, fields_json)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&event.schema)
        .bind(&event.address)
        .bind(&event.tx_hash)
        .bind(event.block_number as i64)
        .bind(event.log_index as i64)
        .bind(&fields)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!(schema = %event.schema, block = event.block_number, "event stored");
        Ok(())
    }

    /// Return all indexed events for a given schema name, ordered by block + log index.
    pub async fn events_by_schema(&self, schema: &str) -> Result<Vec<DecodedEvent>, IndexerError> {
        let rows = sqlx::query(
            "SELECT schema, address, tx_hash, block_number, log_index, fields_json
             FROM events WHERE schema = ? ORDER BY block_number, log_index",
        )
        .bind(schema)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let fields_str: String = row.get("fields_json");
            let fields_json: serde_json::Value =
                serde_json::from_str(&fields_str).unwrap_or(serde_json::Value::Null);

            events.push(DecodedEvent {
                schema: row.get("schema"),
                address: row.get("address"),
                tx_hash: row.get("tx_hash"),
                block_number: row.get::<i64, _>("block_number") as u64,
                log_index: row.get::<i64, _>("log_index") as u32,
                fields_json,
            });
        }
        Ok(events)
    }

    /// Total number of indexed events across all schemas.
    pub async fn event_count(&self) -> Result<u64, IndexerError> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM events")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        let cnt: i64 = row.get("cnt");
        Ok(cnt as u64)
    }

    // ─── Block hash storage ──────────────────────────────────────────────────────

    /// Record a block hash for reorg detection.
    pub async fn insert_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
        hash: &str,
    ) -> Result<(), IndexerError> {
        sqlx::query(
            "INSERT OR REPLACE INTO block_hashes (chain_id, block_number, block_hash)
             VALUES (?, ?, ?)",
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .bind(hash)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Look up the canonical hash of a previously indexed block.
    pub async fn get_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
    ) -> Result<Option<String>, IndexerError> {
        let row = sqlx::query(
            "SELECT block_hash FROM block_hashes
             WHERE chain_id = ? AND block_number = ?",
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(row.map(|r| r.get::<String, _>("block_hash")))
    }

    // ─── Reorg recovery ──────────────────────────────────────────────────────────

    /// Delete all events and block hashes at blocks **after** `block_number`.
    ///
    /// Called during reorg recovery to purge invalidated data.
    pub async fn rollback_after(
        &self,
        chain_id: &str,
        block_number: u64,
    ) -> Result<(), IndexerError> {
        sqlx::query("DELETE FROM events WHERE block_number > ?")
            .bind(block_number as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        sqlx::query(
            "DELETE FROM block_hashes WHERE chain_id = ? AND block_number > ?",
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!(chain_id, block_number, "rolled back storage");
        Ok(())
    }
}

// ─── CheckpointStore impl ────────────────────────────────────────────────────

#[async_trait]
impl CheckpointStore for SqliteStorage {
    async fn load(
        &self,
        chain_id: &str,
        indexer_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError> {
        let row = sqlx::query(
            "SELECT chain_id, indexer_id, block_number, block_hash, updated_at
             FROM checkpoints WHERE chain_id = ? AND indexer_id = ?",
        )
        .bind(chain_id)
        .bind(indexer_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(row.map(|r| Checkpoint {
            chain_id: r.get("chain_id"),
            indexer_id: r.get("indexer_id"),
            block_number: r.get::<i64, _>("block_number") as u64,
            block_hash: r.get("block_hash"),
            updated_at: r.get("updated_at"),
        }))
    }

    async fn save(&self, checkpoint: Checkpoint) -> Result<(), IndexerError> {
        sqlx::query(
            "INSERT OR REPLACE INTO checkpoints
             (chain_id, indexer_id, block_number, block_hash, updated_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&checkpoint.chain_id)
        .bind(&checkpoint.indexer_id)
        .bind(checkpoint.block_number as i64)
        .bind(&checkpoint.block_hash)
        .bind(checkpoint.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!(
            chain_id = %checkpoint.chain_id,
            indexer_id = %checkpoint.indexer_id,
            block = checkpoint.block_number,
            "checkpoint saved"
        );
        Ok(())
    }

    async fn delete(&self, chain_id: &str, indexer_id: &str) -> Result<(), IndexerError> {
        sqlx::query(
            "DELETE FROM checkpoints WHERE chain_id = ? AND indexer_id = ?",
        )
        .bind(chain_id)
        .bind(indexer_id)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event(schema: &str, block: u64) -> DecodedEvent {
        DecodedEvent {
            schema: schema.to_string(),
            address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
            tx_hash: format!("0x{block:064x}"),
            block_number: block,
            log_index: 0,
            fields_json: serde_json::json!({
                "from": "0x1111111111111111111111111111111111111111",
                "to":   "0x2222222222222222222222222222222222222222",
                "value": block.to_string()
            }),
        }
    }

    // ── CheckpointStore ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn checkpoint_roundtrip() {
        let store = SqliteStorage::in_memory().await.unwrap();

        let cp = Checkpoint {
            chain_id: "ethereum".into(),
            indexer_id: "test-indexer".into(),
            block_number: 1_000,
            block_hash: "0xabcdef".into(),
            updated_at: 1_700_000_000,
        };

        store.save(cp.clone()).await.unwrap();

        let loaded = store.load("ethereum", "test-indexer").await.unwrap().unwrap();
        assert_eq!(loaded.block_number, 1_000);
        assert_eq!(loaded.block_hash, "0xabcdef");
        assert_eq!(loaded.updated_at, 1_700_000_000);
    }

    #[tokio::test]
    async fn checkpoint_upsert() {
        let store = SqliteStorage::in_memory().await.unwrap();

        let cp1 = Checkpoint {
            chain_id: "ethereum".into(),
            indexer_id: "my-indexer".into(),
            block_number: 100,
            block_hash: "0xold".into(),
            updated_at: 0,
        };
        let cp2 = Checkpoint {
            chain_id: "ethereum".into(),
            indexer_id: "my-indexer".into(),
            block_number: 200,
            block_hash: "0xnew".into(),
            updated_at: 1,
        };

        store.save(cp1).await.unwrap();
        store.save(cp2).await.unwrap();

        // Only one row; second save overwrites the first
        let loaded = store.load("ethereum", "my-indexer").await.unwrap().unwrap();
        assert_eq!(loaded.block_number, 200);
        assert_eq!(loaded.block_hash, "0xnew");
    }

    #[tokio::test]
    async fn checkpoint_missing_returns_none() {
        let store = SqliteStorage::in_memory().await.unwrap();
        let result = store.load("unknown-chain", "unknown-indexer").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn checkpoint_delete() {
        let store = SqliteStorage::in_memory().await.unwrap();

        let cp = Checkpoint {
            chain_id: "ethereum".into(),
            indexer_id: "del-test".into(),
            block_number: 500,
            block_hash: "0xdef".into(),
            updated_at: 0,
        };
        store.save(cp).await.unwrap();
        assert!(store.load("ethereum", "del-test").await.unwrap().is_some());

        store.delete("ethereum", "del-test").await.unwrap();
        assert!(store.load("ethereum", "del-test").await.unwrap().is_none());
    }

    // ── Event storage ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn event_insert_and_query() {
        let store = SqliteStorage::in_memory().await.unwrap();

        store.insert_event(&sample_event("ERC20Transfer", 100)).await.unwrap();
        store.insert_event(&sample_event("ERC20Transfer", 101)).await.unwrap();
        store.insert_event(&sample_event("UniswapV3Swap", 102)).await.unwrap();

        assert_eq!(store.event_count().await.unwrap(), 3);

        let transfers = store.events_by_schema("ERC20Transfer").await.unwrap();
        assert_eq!(transfers.len(), 2);
        assert_eq!(transfers[0].block_number, 100);
        assert_eq!(transfers[1].block_number, 101);

        let swaps = store.events_by_schema("UniswapV3Swap").await.unwrap();
        assert_eq!(swaps.len(), 1);
    }

    #[tokio::test]
    async fn event_fields_json_roundtrip() {
        let store = SqliteStorage::in_memory().await.unwrap();
        let ev = sample_event("ERC20Transfer", 999);
        store.insert_event(&ev).await.unwrap();

        let loaded = store.events_by_schema("ERC20Transfer").await.unwrap();
        assert_eq!(loaded[0].fields_json["value"], "999");
        assert_eq!(loaded[0].fields_json["from"], "0x1111111111111111111111111111111111111111");
    }

    // ── Block hash storage ────────────────────────────────────────────────────

    #[tokio::test]
    async fn block_hash_insert_and_query() {
        let store = SqliteStorage::in_memory().await.unwrap();

        store.insert_block_hash("ethereum", 100, "0xAAA").await.unwrap();
        store.insert_block_hash("ethereum", 101, "0xBBB").await.unwrap();

        let h100 = store.get_block_hash("ethereum", 100).await.unwrap();
        assert_eq!(h100.unwrap(), "0xAAA");

        let h101 = store.get_block_hash("ethereum", 101).await.unwrap();
        assert_eq!(h101.unwrap(), "0xBBB");

        assert!(store.get_block_hash("ethereum", 999).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn block_hash_chain_isolation() {
        let store = SqliteStorage::in_memory().await.unwrap();

        store.insert_block_hash("ethereum", 100, "0xETH").await.unwrap();
        store.insert_block_hash("polygon", 100, "0xPOL").await.unwrap();

        assert_eq!(
            store.get_block_hash("ethereum", 100).await.unwrap().unwrap(),
            "0xETH"
        );
        assert_eq!(
            store.get_block_hash("polygon", 100).await.unwrap().unwrap(),
            "0xPOL"
        );
    }

    // ── Reorg / rollback ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn rollback_removes_future_data() {
        let store = SqliteStorage::in_memory().await.unwrap();

        for i in 100u64..=105 {
            store.insert_event(&sample_event("Transfer", i)).await.unwrap();
            store.insert_block_hash("ethereum", i, &format!("0x{i:064x}")).await.unwrap();
        }
        assert_eq!(store.event_count().await.unwrap(), 6);

        store.rollback_after("ethereum", 102).await.unwrap();

        // 100, 101, 102 remain; 103–105 purged
        assert_eq!(store.event_count().await.unwrap(), 3);
        assert!(store.get_block_hash("ethereum", 103).await.unwrap().is_none());
        assert!(store.get_block_hash("ethereum", 102).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn rollback_does_not_affect_other_chains() {
        let store = SqliteStorage::in_memory().await.unwrap();

        store.insert_block_hash("ethereum", 200, "0xETH200").await.unwrap();
        store.insert_block_hash("polygon", 200, "0xPOL200").await.unwrap();

        // Rollback ethereum at 100 — polygon hashes at 200 must survive
        store.rollback_after("ethereum", 100).await.unwrap();

        assert!(store.get_block_hash("ethereum", 200).await.unwrap().is_none());
        assert_eq!(
            store.get_block_hash("polygon", 200).await.unwrap().unwrap(),
            "0xPOL200"
        );
    }
}
