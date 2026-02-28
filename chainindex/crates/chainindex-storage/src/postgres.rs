//! PostgreSQL storage backend for ChainIndex.
//!
//! Persists checkpoints, events, and block hashes to a PostgreSQL database.
//! Uses `sqlx` with connection pooling for high-throughput production deployments.
//!
//! # Feature Flag
//! Requires the `postgres` feature:
//! ```toml
//! chainindex-storage = { version = "0.1", features = ["postgres"] }
//! ```
//!
//! # Usage
//! ```rust,no_run
//! use chainindex_storage::postgres::PostgresStorage;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = PostgresStorage::connect(
//!     "postgresql://user:password@localhost:5432/chainindex"
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Schema
//! The storage creates these tables automatically on first connect:
//! - `checkpoints` — indexer progress (chain_id + indexer_id → block number)
//! - `block_hashes` — sliding window of recent block hashes for reorg detection
//! - `events` — decoded event JSON store (optional, for query layer)

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use tracing::{debug, info};

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;

// ─── Connection options ────────────────────────────────────────────────────────

/// Connection options for the Postgres storage backend.
#[derive(Debug, Clone)]
pub struct PostgresOptions {
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: u32,
    /// Minimum number of idle connections to keep open (default: 1)
    pub min_connections: u32,
    /// Connection timeout in seconds (default: 30)
    pub connect_timeout_secs: u64,
    /// Statement cache size per connection (default: 100)
    pub statement_cache_capacity: usize,
}

impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            connect_timeout_secs: 30,
            statement_cache_capacity: 100,
        }
    }
}

// ─── PostgresStorage ─────────────────────────────────────────────────────────

/// PostgreSQL-backed storage for checkpoints, events, and block hashes.
///
/// Thread-safe and cheaply cloneable — wraps a connection pool internally.
#[derive(Clone)]
pub struct PostgresStorage {
    pool: PgPool,
}

impl PostgresStorage {
    /// Connect to a PostgreSQL database and initialize the schema.
    ///
    /// The URL format follows libpq convention:
    /// `postgresql://[user[:password]@][host][:port][/dbname]`
    pub async fn connect(database_url: &str) -> Result<Self, IndexerError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| IndexerError::Storage(format!("postgres connect: {e}")))?;

        let storage = Self { pool };
        storage.init_schema().await?;
        info!("PostgresStorage connected and schema initialized");
        Ok(storage)
    }

    /// Connect with custom pool options.
    pub async fn connect_with_options(
        database_url: &str,
        opts: PostgresOptions,
    ) -> Result<Self, IndexerError> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(opts.max_connections)
            .min_connections(opts.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(opts.connect_timeout_secs))
            .connect(database_url)
            .await
            .map_err(|e| IndexerError::Storage(format!("postgres connect: {e}")))?;

        let storage = Self { pool };
        storage.init_schema().await?;
        Ok(storage)
    }

    /// Create tables and indexes if they don't already exist.
    async fn init_schema(&self) -> Result<(), IndexerError> {
        // Checkpoints table: one row per (chain_id, indexer_id) pair
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chainindex_checkpoints (
                chain_id     TEXT    NOT NULL,
                indexer_id   TEXT    NOT NULL,
                block_number BIGINT  NOT NULL,
                block_hash   TEXT    NOT NULL,
                updated_at   BIGINT  NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,
                PRIMARY KEY (chain_id, indexer_id)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Block hashes table: sliding window for reorg detection
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chainindex_block_hashes (
                chain_id     TEXT   NOT NULL,
                block_number BIGINT NOT NULL,
                block_hash   TEXT   NOT NULL,
                indexed_at   BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,
                PRIMARY KEY (chain_id, block_number)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Events table: decoded event storage for query layer
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chainindex_events (
                id           BIGSERIAL PRIMARY KEY,
                chain_id     TEXT      NOT NULL,
                indexer_id   TEXT      NOT NULL,
                schema_name  TEXT      NOT NULL,
                tx_hash      TEXT      NOT NULL,
                block_number BIGINT    NOT NULL,
                log_index    INTEGER   NOT NULL,
                address      TEXT      NOT NULL,
                event_data   JSONB     NOT NULL,
                indexed_at   BIGINT    NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,
                UNIQUE (chain_id, tx_hash, log_index)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Indexes for common query patterns
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chainindex_events_chain_block
             ON chainindex_events(chain_id, block_number DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chainindex_events_address
             ON chainindex_events(chain_id, address, block_number DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_chainindex_events_schema
             ON chainindex_events(schema_name, block_number DESC)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!("PostgresStorage schema initialized");
        Ok(())
    }

    /// Run arbitrary SQL migrations (for schema upgrades).
    ///
    /// Executes each SQL statement in order; stops on first error.
    pub async fn run_migrations(&self, sql: &[&str]) -> Result<(), IndexerError> {
        for stmt in sql {
            sqlx::query(stmt)
                .execute(&self.pool)
                .await
                .map_err(|e| IndexerError::Storage(format!("migration failed: {e}\nSQL: {stmt}")))?;
        }
        Ok(())
    }

    /// Store a decoded event for the query layer.
    ///
    /// On conflict (same chain/tx_hash/log_index), updates the event data.
    pub async fn store_event(
        &self,
        indexer_id: &str,
        event: &DecodedEvent,
    ) -> Result<(), IndexerError> {
        let event_json = serde_json::to_value(event)
            .map_err(|e| IndexerError::Storage(format!("serialize event: {e}")))?;

        sqlx::query(
            "INSERT INTO chainindex_events
                (chain_id, indexer_id, schema_name, tx_hash, block_number, log_index, address, event_data)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (chain_id, tx_hash, log_index)
             DO UPDATE SET event_data = EXCLUDED.event_data, indexed_at = EXTRACT(EPOCH FROM NOW())::BIGINT",
        )
        .bind(event.chain.as_str())
        .bind(indexer_id)
        .bind(&event.schema)
        .bind(&event.tx_hash)
        .bind(event.block_number as i64)
        .bind(event.log_index as i32)
        .bind(&event.address)
        .bind(event_json)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Store multiple events in a single transaction (higher throughput).
    pub async fn store_events_batch(
        &self,
        indexer_id: &str,
        events: &[DecodedEvent],
    ) -> Result<(), IndexerError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        for event in events {
            let event_json = serde_json::to_value(event)
                .map_err(|e| IndexerError::Storage(format!("serialize event: {e}")))?;

            sqlx::query(
                "INSERT INTO chainindex_events
                    (chain_id, indexer_id, schema_name, tx_hash, block_number, log_index, address, event_data)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (chain_id, tx_hash, log_index) DO NOTHING",
            )
            .bind(event.chain.as_str())
            .bind(indexer_id)
            .bind(&event.schema)
            .bind(&event.tx_hash)
            .bind(event.block_number as i64)
            .bind(event.log_index as i32)
            .bind(&event.address)
            .bind(event_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| IndexerError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| IndexerError::Storage(format!("commit batch: {e}")))?;

        Ok(())
    }

    /// Remove all block hashes older than `keep_blocks` blocks from the tip.
    ///
    /// Call periodically to prevent unbounded growth of the block_hashes table.
    pub async fn prune_block_hashes(
        &self,
        chain_id: &str,
        tip_block: u64,
        keep_blocks: u64,
    ) -> Result<u64, IndexerError> {
        let cutoff = tip_block.saturating_sub(keep_blocks) as i64;
        let result = sqlx::query(
            "DELETE FROM chainindex_block_hashes
             WHERE chain_id = $1 AND block_number < $2",
        )
        .bind(chain_id)
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(result.rows_affected())
    }

    /// Roll back all events after `from_block` for a given chain (reorg handling).
    ///
    /// Called by the reorg detector when a chain reorganization is detected.
    pub async fn rollback_after(
        &self,
        chain_id: &str,
        from_block: u64,
    ) -> Result<u64, IndexerError> {
        let result = sqlx::query(
            "DELETE FROM chainindex_events
             WHERE chain_id = $1 AND block_number >= $2",
        )
        .bind(chain_id)
        .bind(from_block as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        // Also remove stale block hashes
        sqlx::query(
            "DELETE FROM chainindex_block_hashes
             WHERE chain_id = $1 AND block_number >= $2",
        )
        .bind(chain_id)
        .bind(from_block as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!("rollback_after: removed {} events for chain={} from block={}", result.rows_affected(), chain_id, from_block);
        Ok(result.rows_affected())
    }

    /// Query events for a specific chain and schema.
    pub async fn query_events(
        &self,
        chain_id: &str,
        schema_name: &str,
        from_block: u64,
        to_block: u64,
        limit: u32,
    ) -> Result<Vec<serde_json::Value>, IndexerError> {
        let rows = sqlx::query(
            "SELECT event_data FROM chainindex_events
             WHERE chain_id = $1
               AND schema_name = $2
               AND block_number >= $3
               AND block_number <= $4
             ORDER BY block_number ASC, log_index ASC
             LIMIT $5",
        )
        .bind(chain_id)
        .bind(schema_name)
        .bind(from_block as i64)
        .bind(to_block as i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        let events = rows
            .iter()
            .map(|row| row.try_get::<serde_json::Value, _>("event_data"))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(events)
    }

    /// Get the underlying connection pool (for custom queries).
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

// ─── CheckpointStore impl ─────────────────────────────────────────────────────

#[async_trait]
impl CheckpointStore for PostgresStorage {
    async fn save_checkpoint(
        &self,
        indexer_id: &str,
        checkpoint: &Checkpoint,
    ) -> Result<(), IndexerError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO chainindex_checkpoints
                (chain_id, indexer_id, block_number, block_hash, updated_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (chain_id, indexer_id)
             DO UPDATE SET
                block_number = EXCLUDED.block_number,
                block_hash   = EXCLUDED.block_hash,
                updated_at   = EXCLUDED.updated_at",
        )
        .bind(&checkpoint.chain_id)
        .bind(indexer_id)
        .bind(checkpoint.block_number as i64)
        .bind(&checkpoint.block_hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        debug!(
            "checkpoint saved: chain={} indexer={} block={}",
            checkpoint.chain_id, indexer_id, checkpoint.block_number
        );
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        indexer_id: &str,
        chain_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError> {
        let row = sqlx::query(
            "SELECT chain_id, block_number, block_hash
             FROM chainindex_checkpoints
             WHERE chain_id = $1 AND indexer_id = $2",
        )
        .bind(chain_id)
        .bind(indexer_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(row.map(|r| Checkpoint {
            chain_id: r.get::<String, _>("chain_id"),
            block_number: r.get::<i64, _>("block_number") as u64,
            block_hash: r.get::<String, _>("block_hash"),
        }))
    }

    async fn save_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
        block_hash: &str,
    ) -> Result<(), IndexerError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        sqlx::query(
            "INSERT INTO chainindex_block_hashes (chain_id, block_number, block_hash, indexed_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (chain_id, block_number) DO NOTHING",
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .bind(block_hash)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn get_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
    ) -> Result<Option<String>, IndexerError> {
        let row = sqlx::query(
            "SELECT block_hash FROM chainindex_block_hashes
             WHERE chain_id = $1 AND block_number = $2",
        )
        .bind(chain_id)
        .bind(block_number as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;

        Ok(row.map(|r| r.get::<String, _>("block_hash")))
    }

    async fn delete_checkpoint(
        &self,
        indexer_id: &str,
        chain_id: &str,
    ) -> Result<(), IndexerError> {
        sqlx::query(
            "DELETE FROM chainindex_checkpoints
             WHERE chain_id = $1 AND indexer_id = $2",
        )
        .bind(chain_id)
        .bind(indexer_id)
        .execute(&self.pool)
        .await
        .map_err(|e| IndexerError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Integration tests require a running PostgreSQL instance.
    // Set DATABASE_URL environment variable to enable.
    // Example: DATABASE_URL=postgresql://localhost/chainindex_test cargo test

    #[tokio::test]
    #[ignore = "requires PostgreSQL (set DATABASE_URL to enable)"]
    async fn test_postgres_checkpoint_roundtrip() {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for integration tests");
        let store = super::PostgresStorage::connect(&url).await.unwrap();

        let checkpoint = chainindex_core::checkpoint::Checkpoint {
            chain_id: "ethereum".to_string(),
            block_number: 19_000_000,
            block_hash: "0xabc123def456".to_string(),
        };

        store.save_checkpoint("test-indexer", &checkpoint).await.unwrap();

        let loaded = store
            .load_checkpoint("test-indexer", "ethereum")
            .await
            .unwrap()
            .expect("checkpoint not found");

        assert_eq!(loaded.block_number, 19_000_000);
        assert_eq!(loaded.block_hash, "0xabc123def456");

        // Clean up
        store
            .delete_checkpoint("test-indexer", "ethereum")
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires PostgreSQL (set DATABASE_URL to enable)"]
    async fn test_postgres_block_hash_and_rollback() {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set for integration tests");
        let store = super::PostgresStorage::connect(&url).await.unwrap();

        // Store some block hashes
        for i in 0u64..10 {
            store
                .save_block_hash("ethereum", 19_000_000 + i, &format!("0xhash{i}"))
                .await
                .unwrap();
        }

        // Verify retrieval
        let hash = store
            .get_block_hash("ethereum", 19_000_005)
            .await
            .unwrap()
            .expect("hash not found");
        assert_eq!(hash, "0xhash5");

        // Rollback
        let deleted = store.rollback_after("ethereum", 19_000_005).await.unwrap();
        assert_eq!(deleted, 0); // no events, only block hashes

        // Verify blocks after rollback point are gone
        let gone = store.get_block_hash("ethereum", 19_000_006).await.unwrap();
        assert!(gone.is_none());
    }
}
