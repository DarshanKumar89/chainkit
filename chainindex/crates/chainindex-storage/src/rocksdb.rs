//! RocksDB-style storage backend for ChainIndex.
//!
//! This module provides a full storage backend using a RocksDB-style key-value
//! abstraction. Because the `rocksdb` crate requires native C++ compilation, the
//! implementation ships a [`BTreeKvStore`] — an in-process embedded store backed
//! by `BTreeMap` — that precisely mirrors the RocksDB API surface.  When the real
//! `rocksdb` crate is available the `BTreeKvStore` can be replaced by dropping in
//! a `RocksKvStore` that implements the same [`KvStore`] trait.
//!
//! # Feature flag
//! ```toml
//! chainindex-storage = { version = "0.1", features = ["rocksdb"] }
//! ```
//!
//! # Usage
//! ```rust,no_run
//! use chainindex_storage::rocksdb::RocksDbStorage;
//!
//! // In-memory (tests / ephemeral)
//! let store = RocksDbStorage::in_memory();
//!
//! // File-backed path (swap BTreeKvStore for real RocksDB later)
//! // let store = RocksDbStorage::open("./chainindex.db").unwrap();
//! ```
//!
//! # Column families
//! | CF name         | Content                                         |
//! |-----------------|--------------------------------------------------|
//! | `checkpoints`   | `{chain_id}:{indexer_id}` → JSON Checkpoint      |
//! | `events`        | `{block:08x}:{log:08x}:{tx_hash}` → JSON event  |
//! | `block_hashes`  | `{chain_id}:{block:08x}` → hash string           |
//! | `metadata`      | free-form key/value for storage-level bookkeeping|

use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};
use tracing::debug;

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore};
use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;

// ─── Column-family names ────────────────────────────────────────────────────

const CF_CHECKPOINTS: &str = "checkpoints";
const CF_EVENTS: &str = "events";
const CF_BLOCK_HASHES: &str = "block_hashes";
const CF_METADATA: &str = "metadata";

// ─── BatchOp ─────────────────────────────────────────────────────────────────

/// A single operation inside an atomic write batch.
#[derive(Debug, Clone)]
pub enum BatchOp {
    /// Insert or overwrite a key in the given column family.
    Put {
        cf: String,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    /// Delete a key from the given column family.
    Delete { cf: String, key: Vec<u8> },
}

// ─── KvStore trait ───────────────────────────────────────────────────────────

/// Low-level key-value abstraction that mirrors the RocksDB API surface.
///
/// Column families provide logical namespacing within the same store.
/// All operations are synchronous and infallible at the trait level
/// (errors are surfaced as [`IndexerError::Storage`]).
pub trait KvStore: Send + Sync {
    /// Get the value associated with `key` in column family `cf`.
    ///
    /// Returns `None` when the key does not exist.
    fn get(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>, IndexerError>;

    /// Insert or overwrite `key` in column family `cf`.
    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<(), IndexerError>;

    /// Delete `key` from column family `cf`.
    ///
    /// A no-op (not an error) when the key does not exist.
    fn delete(&self, cf: &str, key: &[u8]) -> Result<(), IndexerError>;

    /// Return all key-value pairs whose keys start with `prefix` in `cf`.
    ///
    /// Results are returned in lexicographic key order.
    fn prefix_scan(
        &self,
        cf: &str,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, IndexerError>;

    /// Return all key-value pairs in `cf` where `start <= key < end`.
    ///
    /// Results are returned in lexicographic key order.
    fn range_scan(
        &self,
        cf: &str,
        start: &[u8],
        end: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, IndexerError>;

    /// Apply a sequence of [`BatchOp`]s atomically.
    ///
    /// All operations succeed or none are applied.  For `BTreeKvStore` the
    /// atomicity guarantee holds in the sense that the lock is held for the
    /// entire batch.
    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), IndexerError>;
}

// ─── BTreeKvStore ────────────────────────────────────────────────────────────

/// In-process embedded KV store backed by a `BTreeMap`.
///
/// This is the default implementation used when the native RocksDB library is
/// not available.  It provides the full [`KvStore`] API with `O(log n)` reads
/// and writes, thread-safety via `RwLock`, and natural lexicographic ordering
/// which ensures events are returned in block order without extra sorting.
///
/// Column families are stored as separate `BTreeMap`s inside a shared
/// `HashMap`.  Call [`BTreeKvStore::create_cf`] to register a new family
/// before use; all well-known families are pre-created by
/// [`RocksDbStorage::in_memory`] / [`RocksDbStorage::open`].
#[derive(Default)]
pub struct BTreeKvStore {
    // HashMap<cf_name, BTreeMap<key, value>>
    inner: RwLock<HashMap<String, BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl BTreeKvStore {
    /// Create a new, empty store with no column families.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a column family.  Idempotent — safe to call if the CF already
    /// exists.
    pub fn create_cf(&self, name: &str) {
        let mut guard = self.inner.write().unwrap();
        guard.entry(name.to_string()).or_insert_with(BTreeMap::new);
    }

    /// Flush all pending writes to the underlying store.
    ///
    /// This is a no-op for `BTreeKvStore` (everything is already in-memory)
    /// but exists so the signature matches what a real RocksDB backend would
    /// expose.
    pub fn flush(&self) -> Result<(), IndexerError> {
        Ok(())
    }

    /// Return the total number of keys across all column families.
    ///
    /// Useful for diagnostic / stats collection.
    #[allow(dead_code)]
    pub fn total_keys(&self) -> u64 {
        self.inner
            .read()
            .unwrap()
            .values()
            .map(|cf| cf.len() as u64)
            .sum()
    }
}

impl KvStore for BTreeKvStore {
    fn get(&self, cf: &str, key: &[u8]) -> Result<Option<Vec<u8>>, IndexerError> {
        let guard = self.inner.read().unwrap();
        let cf_map = guard
            .get(cf)
            .ok_or_else(|| IndexerError::Storage(format!("column family not found: {cf}")))?;
        Ok(cf_map.get(key).cloned())
    }

    fn put(&self, cf: &str, key: &[u8], value: &[u8]) -> Result<(), IndexerError> {
        let mut guard = self.inner.write().unwrap();
        let cf_map = guard
            .get_mut(cf)
            .ok_or_else(|| IndexerError::Storage(format!("column family not found: {cf}")))?;
        cf_map.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, cf: &str, key: &[u8]) -> Result<(), IndexerError> {
        let mut guard = self.inner.write().unwrap();
        let cf_map = guard
            .get_mut(cf)
            .ok_or_else(|| IndexerError::Storage(format!("column family not found: {cf}")))?;
        cf_map.remove(key);
        Ok(())
    }

    fn prefix_scan(
        &self,
        cf: &str,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, IndexerError> {
        let guard = self.inner.read().unwrap();
        let cf_map = guard
            .get(cf)
            .ok_or_else(|| IndexerError::Storage(format!("column family not found: {cf}")))?;

        let results = cf_map
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    fn range_scan(
        &self,
        cf: &str,
        start: &[u8],
        end: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, IndexerError> {
        let guard = self.inner.read().unwrap();
        let cf_map = guard
            .get(cf)
            .ok_or_else(|| IndexerError::Storage(format!("column family not found: {cf}")))?;

        let results = cf_map
            .range(start.to_vec()..end.to_vec())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    fn write_batch(&self, ops: Vec<BatchOp>) -> Result<(), IndexerError> {
        // Hold the write lock for the entire batch to ensure atomicity.
        let mut guard = self.inner.write().unwrap();
        for op in ops {
            match op {
                BatchOp::Put { cf, key, value } => {
                    let cf_map = guard.get_mut(&cf).ok_or_else(|| {
                        IndexerError::Storage(format!("column family not found: {cf}"))
                    })?;
                    cf_map.insert(key, value);
                }
                BatchOp::Delete { cf, key } => {
                    let cf_map = guard.get_mut(&cf).ok_or_else(|| {
                        IndexerError::Storage(format!("column family not found: {cf}"))
                    })?;
                    cf_map.remove(&key);
                }
            }
        }
        Ok(())
    }
}

// ─── CompactionConfig ─────────────────────────────────────────────────────────

/// Configuration for RocksDB compaction and memory tuning.
///
/// These values are forwarded to the underlying RocksDB `Options` when the
/// real native backend is in use.  With `BTreeKvStore` they are stored but
/// otherwise ignored.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Maximum number of open file descriptors (default: 256).
    pub max_open_files: i32,
    /// Target write-buffer size per column family in bytes (default: 64 MiB).
    pub write_buffer_size: usize,
    /// Maximum number of write buffers that can be built up in memory
    /// before a flush is forced (default: 3).
    pub max_write_buffer_number: i32,
    /// Target size for SST files at level-1 in bytes (default: 64 MiB).
    pub target_file_size_base: u64,
    /// Block cache size in bytes shared across all column families
    /// (default: 256 MiB).
    pub block_cache_size: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            max_open_files: 256,
            write_buffer_size: 64 * 1024 * 1024,       // 64 MiB
            max_write_buffer_number: 3,
            target_file_size_base: 64 * 1024 * 1024,   // 64 MiB
            block_cache_size: 256 * 1024 * 1024,        // 256 MiB
        }
    }
}

// ─── StorageStats ─────────────────────────────────────────────────────────────

/// Storage-level statistics collected from a [`RocksDbStorage`] instance.
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Total decoded events stored.
    pub total_events: u64,
    /// Total checkpoints stored (one per chain+indexer pair).
    pub total_checkpoints: u64,
    /// Total block-hash entries stored.
    pub total_block_hashes: u64,
    /// Estimated disk usage in bytes (sum of key+value lengths for all CFs).
    pub disk_usage_bytes: u64,
}

impl StorageStats {
    /// Collect current statistics from the given storage instance.
    pub fn collect(storage: &RocksDbStorage) -> Self {
        let kv = &storage.kv;

        let count_cf = |cf: &str| -> u64 {
            kv.prefix_scan(cf, b"").unwrap_or_default().len() as u64
        };

        let byte_usage_cf = |cf: &str| -> u64 {
            kv.prefix_scan(cf, b"")
                .unwrap_or_default()
                .iter()
                .map(|(k, v)| (k.len() + v.len()) as u64)
                .sum()
        };

        let total_events = count_cf(CF_EVENTS);
        let total_checkpoints = count_cf(CF_CHECKPOINTS);
        let total_block_hashes = count_cf(CF_BLOCK_HASHES);
        let disk_usage_bytes = byte_usage_cf(CF_EVENTS)
            + byte_usage_cf(CF_CHECKPOINTS)
            + byte_usage_cf(CF_BLOCK_HASHES)
            + byte_usage_cf(CF_METADATA);

        Self {
            total_events,
            total_checkpoints,
            total_block_hashes,
            disk_usage_bytes,
        }
    }
}

// ─── Key encoding helpers ─────────────────────────────────────────────────────

/// Encode an event key: `{block_number:08x}:{log_index:08x}:{tx_hash}`.
///
/// The zero-padded hex block number and log index ensure that lexicographic
/// ordering of keys matches chronological ordering of events, which makes
/// range scans over block windows efficient.
fn encode_event_key(block_number: u64, log_index: u32, tx_hash: &str) -> Vec<u8> {
    format!("{block_number:08x}:{log_index:08x}:{tx_hash}")
        .into_bytes()
}

/// Encode a block-hash key: `{chain_id}:{block_number:08x}`.
fn encode_block_hash_key(chain_id: &str, block_number: u64) -> Vec<u8> {
    format!("{chain_id}:{block_number:08x}").into_bytes()
}

/// Encode a checkpoint key: `{chain_id}:{indexer_id}`.
fn encode_checkpoint_key(chain_id: &str, indexer_id: &str) -> Vec<u8> {
    format!("{chain_id}:{indexer_id}").into_bytes()
}

/// Encode the exclusive upper-bound block key for range scans.
///
/// Because keys are `{block:08x}:…`, the exclusive end for block `n` is the
/// key of block `n+1`, i.e. `{n+1:08x}:`.
fn encode_block_upper_bound(block_number: u64) -> Vec<u8> {
    format!("{block_number:08x}:").into_bytes()
}

// ─── RocksDbStorage ──────────────────────────────────────────────────────────

/// RocksDB-style storage backend for checkpoints, events, and block hashes.
///
/// Backed by a [`KvStore`] implementation — [`BTreeKvStore`] by default.
/// Swap in a `RocksKvStore` (not included here; requires native libs) to
/// get persistent, production-grade storage without changing any call sites.
pub struct RocksDbStorage {
    kv: Arc<dyn KvStore + Send + Sync>,
    /// Compaction config (stored for inspection / future forwarding).
    pub config: CompactionConfig,
}

impl RocksDbStorage {
    // ── Constructors ─────────────────────────────────────────────────────────

    /// Create a `RocksDbStorage` wrapping an arbitrary [`KvStore`] implementation.
    ///
    /// The caller is responsible for ensuring all required column families
    /// (`checkpoints`, `events`, `block_hashes`, `metadata`) exist on the
    /// provided store before calling this function.  Use [`in_memory`] or
    /// [`from_btree`] to get a fully-configured instance without boilerplate.
    pub fn new(kv: Arc<dyn KvStore + Send + Sync>) -> Self {
        Self::new_with_config(kv, CompactionConfig::default())
    }

    /// Like [`new`] but with a custom [`CompactionConfig`].
    ///
    /// The caller must have pre-created all column families on `kv`.
    pub fn new_with_config(kv: Arc<dyn KvStore + Send + Sync>, config: CompactionConfig) -> Self {
        Self { kv, config }
    }

    /// Create a `RocksDbStorage` from an owned [`BTreeKvStore`].
    ///
    /// All column families are created automatically before the store is
    /// wrapped, so the returned instance is immediately usable.
    pub fn from_btree(btree: BTreeKvStore) -> Self {
        btree.create_cf(CF_CHECKPOINTS);
        btree.create_cf(CF_EVENTS);
        btree.create_cf(CF_BLOCK_HASHES);
        btree.create_cf(CF_METADATA);
        Self {
            kv: Arc::new(btree),
            config: CompactionConfig::default(),
        }
    }

    /// Create an in-memory storage instance using [`BTreeKvStore`].
    ///
    /// Identical to calling `new(Arc::new(BTreeKvStore::new()))`.  Data is
    /// lost when the instance is dropped.
    pub fn in_memory() -> Self {
        let kv = Arc::new(BTreeKvStore::new());
        kv.create_cf(CF_CHECKPOINTS);
        kv.create_cf(CF_EVENTS);
        kv.create_cf(CF_BLOCK_HASHES);
        kv.create_cf(CF_METADATA);
        Self {
            kv,
            config: CompactionConfig::default(),
        }
    }

    /// Simulate opening a file-backed RocksDB database at `path`.
    ///
    /// Currently uses `BTreeKvStore`; a future implementation can open a real
    /// RocksDB instance here once the native library is available.
    pub fn open(_path: &str) -> Result<Self, IndexerError> {
        // In the BTreeKvStore simulation the path is noted but not used.
        // A real implementation would call RocksDB::open_cf(opts, path, cfs).
        Ok(Self::in_memory())
    }

    // ── Event storage ─────────────────────────────────────────────────────────

    /// Insert a single decoded event into the `events` column family.
    ///
    /// Key: `{block_number:08x}:{log_index:08x}:{tx_hash}`
    pub fn insert_event(&self, event: &DecodedEvent) -> Result<(), IndexerError> {
        let key = encode_event_key(event.block_number, event.log_index, &event.tx_hash);
        let value = serde_json::to_vec(event)
            .map_err(|e| IndexerError::Storage(format!("serialize event: {e}")))?;

        self.kv.put(CF_EVENTS, &key, &value)?;
        debug!(
            schema = %event.schema,
            block  = event.block_number,
            "rocksdb: event stored"
        );
        Ok(())
    }

    /// Insert multiple decoded events atomically using a write batch.
    ///
    /// All events are written in a single batch; if any serialisation fails
    /// the entire batch is aborted.
    pub fn insert_events_batch(&self, events: &[DecodedEvent]) -> Result<(), IndexerError> {
        if events.is_empty() {
            return Ok(());
        }

        let mut ops = Vec::with_capacity(events.len());
        for event in events {
            let key = encode_event_key(event.block_number, event.log_index, &event.tx_hash);
            let value = serde_json::to_vec(event)
                .map_err(|e| IndexerError::Storage(format!("serialize event: {e}")))?;
            ops.push(BatchOp::Put {
                cf: CF_EVENTS.to_string(),
                key,
                value,
            });
        }

        self.kv.write_batch(ops)?;
        debug!(count = events.len(), "rocksdb: batch events stored");
        Ok(())
    }

    /// Return all events that match the given schema name, ordered by block
    /// number and log index.
    pub fn events_by_schema(&self, schema: &str) -> Result<Vec<DecodedEvent>, IndexerError> {
        // Full scan of the events CF then filter by schema.
        // A real RocksDB backend would maintain a secondary index CF keyed by
        // `{schema}:{block:08x}:{log:08x}:{tx_hash}` for O(log n) lookups.
        let pairs = self.kv.prefix_scan(CF_EVENTS, b"")?;
        self.decode_and_filter(pairs, |e| e.schema == schema)
    }

    /// Return all events emitted by the given contract address.
    pub fn events_by_address(&self, address: &str) -> Result<Vec<DecodedEvent>, IndexerError> {
        let pairs = self.kv.prefix_scan(CF_EVENTS, b"")?;
        let addr_lower = address.to_lowercase();
        self.decode_and_filter(pairs, |e| e.address.to_lowercase() == addr_lower)
    }

    /// Return all events in the inclusive block range `[from, to]`.
    pub fn events_in_block_range(
        &self,
        from: u64,
        to: u64,
    ) -> Result<Vec<DecodedEvent>, IndexerError> {
        // Range scan: start = `{from:08x}:`, end = `{to+1:08x}:`
        let start = encode_block_upper_bound(from);
        // Adjust start to be inclusive: use `{from:08x}:` but we need to
        // include keys that start with exactly `{from:08x}:`.
        // BTreeMap::range lower bound is inclusive, so `{from:08x}:` works
        // since event keys are `{from:08x}:{log:08x}:{tx}` which is >= `{from:08x}:`.
        let end = encode_block_upper_bound(to.saturating_add(1));

        let pairs = self.kv.range_scan(CF_EVENTS, &start, &end)?;
        self.decode_and_filter(pairs, |_| true)
    }

    /// Delete all events at blocks **strictly after** `block_number`.
    ///
    /// Returns the number of deleted entries.  Used during reorg recovery.
    pub fn rollback_after(&self, block_number: u64) -> Result<u64, IndexerError> {
        // The exclusive start of the deletion range is `{block_number+1:08x}:`.
        let start = encode_block_upper_bound(block_number.saturating_add(1));
        // Use a maximum possible key as the end bound.
        let end = b"ffffffff:".to_vec();

        let to_delete = self.kv.range_scan(CF_EVENTS, &start, &end)?;
        let count = to_delete.len() as u64;

        if count == 0 {
            return Ok(0);
        }

        let ops: Vec<BatchOp> = to_delete
            .into_iter()
            .map(|(key, _)| BatchOp::Delete {
                cf: CF_EVENTS.to_string(),
                key,
            })
            .collect();

        self.kv.write_batch(ops)?;
        debug!(
            block_number,
            deleted = count,
            "rocksdb: rolled back events after block"
        );
        Ok(count)
    }

    // ── Block hash storage ────────────────────────────────────────────────────

    /// Store the canonical hash of a block for reorg detection.
    ///
    /// Key: `{chain_id}:{block_number:08x}`
    pub fn insert_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
        hash: &str,
    ) -> Result<(), IndexerError> {
        let key = encode_block_hash_key(chain_id, block_number);
        self.kv.put(CF_BLOCK_HASHES, &key, hash.as_bytes())?;
        Ok(())
    }

    /// Look up the hash stored for a given chain and block number.
    pub fn get_block_hash(
        &self,
        chain_id: &str,
        block_number: u64,
    ) -> Result<Option<String>, IndexerError> {
        let key = encode_block_hash_key(chain_id, block_number);
        match self.kv.get(CF_BLOCK_HASHES, &key)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| IndexerError::Storage(format!("decode block hash: {e}")))?;
                Ok(Some(s))
            }
            None => Ok(None),
        }
    }

    /// Prune block hashes for `chain_id`, keeping only the `keep_last` most
    /// recent entries.
    ///
    /// Returns the number of pruned entries.
    pub fn prune_block_hashes(
        &self,
        chain_id: &str,
        keep_last: u64,
    ) -> Result<u64, IndexerError> {
        // Collect all entries for this chain, ordered by key (= by block number
        // because of zero-padded hex encoding).
        let prefix = format!("{chain_id}:");
        let pairs = self.kv.prefix_scan(CF_BLOCK_HASHES, prefix.as_bytes())?;

        let total = pairs.len() as u64;
        if total <= keep_last {
            return Ok(0);
        }

        // `pairs` is already in lexicographic order → the first (total - keep_last)
        // entries are the oldest ones to prune.
        let prune_count = total - keep_last;
        let ops: Vec<BatchOp> = pairs
            .into_iter()
            .take(prune_count as usize)
            .map(|(key, _)| BatchOp::Delete {
                cf: CF_BLOCK_HASHES.to_string(),
                key,
            })
            .collect();

        self.kv.write_batch(ops)?;
        debug!(chain_id, pruned = prune_count, "rocksdb: pruned block hashes");
        Ok(prune_count)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn decode_and_filter<F>(
        &self,
        pairs: Vec<(Vec<u8>, Vec<u8>)>,
        predicate: F,
    ) -> Result<Vec<DecodedEvent>, IndexerError>
    where
        F: Fn(&DecodedEvent) -> bool,
    {
        let mut events = Vec::new();
        for (_key, value) in pairs {
            let event: DecodedEvent = serde_json::from_slice(&value)
                .map_err(|e| IndexerError::Storage(format!("deserialize event: {e}")))?;
            if predicate(&event) {
                events.push(event);
            }
        }
        Ok(events)
    }
}

// ─── CheckpointStore impl ─────────────────────────────────────────────────────

#[async_trait]
impl CheckpointStore for RocksDbStorage {
    async fn load(
        &self,
        chain_id: &str,
        indexer_id: &str,
    ) -> Result<Option<Checkpoint>, IndexerError> {
        let key = encode_checkpoint_key(chain_id, indexer_id);
        match self.kv.get(CF_CHECKPOINTS, &key)? {
            Some(bytes) => {
                let cp: Checkpoint = serde_json::from_slice(&bytes).map_err(|e| {
                    IndexerError::Storage(format!("deserialize checkpoint: {e}"))
                })?;
                Ok(Some(cp))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, checkpoint: Checkpoint) -> Result<(), IndexerError> {
        let key = encode_checkpoint_key(&checkpoint.chain_id, &checkpoint.indexer_id);
        let value = serde_json::to_vec(&checkpoint)
            .map_err(|e| IndexerError::Storage(format!("serialize checkpoint: {e}")))?;

        self.kv.put(CF_CHECKPOINTS, &key, &value)?;
        debug!(
            chain_id  = %checkpoint.chain_id,
            indexer_id = %checkpoint.indexer_id,
            block     = checkpoint.block_number,
            "rocksdb: checkpoint saved"
        );
        Ok(())
    }

    async fn delete(&self, chain_id: &str, indexer_id: &str) -> Result<(), IndexerError> {
        let key = encode_checkpoint_key(chain_id, indexer_id);
        self.kv.delete(CF_CHECKPOINTS, &key)?;
        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn sample_event(schema: &str, address: &str, block: u64, log_index: u32) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: schema.to_string(),
            address: address.to_string(),
            tx_hash: format!("0x{block:064x}"),
            block_number: block,
            log_index,
            fields_json: serde_json::json!({
                "from":  "0x1111111111111111111111111111111111111111",
                "to":    "0x2222222222222222222222222222222222222222",
                "value": block.to_string(),
            }),
        }
    }

    fn sample_checkpoint(chain: &str, indexer: &str, block: u64) -> Checkpoint {
        Checkpoint {
            chain_id: chain.into(),
            indexer_id: indexer.into(),
            block_number: block,
            block_hash: format!("0x{block:064x}"),
            updated_at: 1_700_000_000,
        }
    }

    // ── 1. BTreeKvStore — basic CRUD ─────────────────────────────────────────

    #[test]
    fn btree_kv_basic_crud() {
        let kv = BTreeKvStore::new();
        kv.create_cf("test");

        // get on non-existent key returns None
        assert!(kv.get("test", b"missing").unwrap().is_none());

        // put then get
        kv.put("test", b"key1", b"value1").unwrap();
        assert_eq!(kv.get("test", b"key1").unwrap().unwrap(), b"value1");

        // overwrite
        kv.put("test", b"key1", b"value2").unwrap();
        assert_eq!(kv.get("test", b"key1").unwrap().unwrap(), b"value2");

        // delete
        kv.delete("test", b"key1").unwrap();
        assert!(kv.get("test", b"key1").unwrap().is_none());

        // delete non-existent key is a no-op
        kv.delete("test", b"ghost").unwrap();
    }

    // ── 2. Column family isolation ────────────────────────────────────────────

    #[test]
    fn btree_kv_cf_isolation() {
        let kv = BTreeKvStore::new();
        kv.create_cf("cf_a");
        kv.create_cf("cf_b");

        kv.put("cf_a", b"shared_key", b"value_a").unwrap();
        kv.put("cf_b", b"shared_key", b"value_b").unwrap();

        assert_eq!(
            kv.get("cf_a", b"shared_key").unwrap().unwrap(),
            b"value_a"
        );
        assert_eq!(
            kv.get("cf_b", b"shared_key").unwrap().unwrap(),
            b"value_b"
        );
    }

    // ── 3. Prefix scan ────────────────────────────────────────────────────────

    #[test]
    fn btree_kv_prefix_scan() {
        let kv = BTreeKvStore::new();
        kv.create_cf("cf");

        kv.put("cf", b"foo:1", b"v1").unwrap();
        kv.put("cf", b"foo:2", b"v2").unwrap();
        kv.put("cf", b"bar:3", b"v3").unwrap();

        let results = kv.prefix_scan("cf", b"foo:").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(k, _)| k.starts_with(b"foo:")));

        let empty = kv.prefix_scan("cf", b"xyz:").unwrap();
        assert!(empty.is_empty());
    }

    // ── 4. Range scan ─────────────────────────────────────────────────────────

    #[test]
    fn btree_kv_range_scan() {
        let kv = BTreeKvStore::new();
        kv.create_cf("cf");

        for i in 0u8..10 {
            kv.put("cf", &[i], &[i * 10]).unwrap();
        }

        // Range [3, 7) — should return keys 3, 4, 5, 6
        let results = kv.range_scan("cf", &[3], &[7]).unwrap();
        assert_eq!(results.len(), 4);
        assert_eq!(results[0].0, vec![3]);
        assert_eq!(results[3].0, vec![6]);
    }

    // ── 5. Write batch (atomic) ───────────────────────────────────────────────

    #[test]
    fn btree_kv_write_batch_atomic() {
        let kv = BTreeKvStore::new();
        kv.create_cf("cf");

        kv.put("cf", b"existing", b"old").unwrap();

        let ops = vec![
            BatchOp::Put {
                cf: "cf".into(),
                key: b"new_key".to_vec(),
                value: b"new_val".to_vec(),
            },
            BatchOp::Delete {
                cf: "cf".into(),
                key: b"existing".to_vec(),
            },
            BatchOp::Put {
                cf: "cf".into(),
                key: b"another".to_vec(),
                value: b"another_val".to_vec(),
            },
        ];

        kv.write_batch(ops).unwrap();

        assert_eq!(
            kv.get("cf", b"new_key").unwrap().unwrap(),
            b"new_val"
        );
        assert!(kv.get("cf", b"existing").unwrap().is_none());
        assert_eq!(
            kv.get("cf", b"another").unwrap().unwrap(),
            b"another_val"
        );
    }

    // ── 6. In-memory constructor ──────────────────────────────────────────────

    #[test]
    fn rocksdb_in_memory_constructor() {
        let store = RocksDbStorage::in_memory();
        // Confirm all CFs are reachable by performing a prefix scan on each.
        for cf in &[CF_CHECKPOINTS, CF_EVENTS, CF_BLOCK_HASHES, CF_METADATA] {
            let result = store.kv.prefix_scan(cf, b"");
            assert!(result.is_ok(), "CF '{cf}' should be accessible");
        }
    }

    // ── 7. CheckpointStore — roundtrip ────────────────────────────────────────

    #[tokio::test]
    async fn checkpoint_roundtrip() {
        let store = RocksDbStorage::in_memory();
        let cp = sample_checkpoint("ethereum", "test-indexer", 1_000);

        store.save(cp.clone()).await.unwrap();

        let loaded = store
            .load("ethereum", "test-indexer")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.block_number, 1_000);
        assert_eq!(loaded.block_hash, cp.block_hash);
        assert_eq!(loaded.updated_at, 1_700_000_000);
    }

    // ── 8. CheckpointStore — upsert ───────────────────────────────────────────

    #[tokio::test]
    async fn checkpoint_upsert() {
        let store = RocksDbStorage::in_memory();

        store
            .save(sample_checkpoint("ethereum", "idx", 100))
            .await
            .unwrap();
        store
            .save(sample_checkpoint("ethereum", "idx", 200))
            .await
            .unwrap();

        let loaded = store.load("ethereum", "idx").await.unwrap().unwrap();
        assert_eq!(loaded.block_number, 200);
    }

    // ── 9. CheckpointStore — missing returns None ─────────────────────────────

    #[tokio::test]
    async fn checkpoint_missing_returns_none() {
        let store = RocksDbStorage::in_memory();
        assert!(store
            .load("unknown", "unknown")
            .await
            .unwrap()
            .is_none());
    }

    // ── 10. CheckpointStore — delete ─────────────────────────────────────────

    #[tokio::test]
    async fn checkpoint_delete() {
        let store = RocksDbStorage::in_memory();
        store
            .save(sample_checkpoint("ethereum", "del-test", 500))
            .await
            .unwrap();

        assert!(store
            .load("ethereum", "del-test")
            .await
            .unwrap()
            .is_some());

        store.delete("ethereum", "del-test").await.unwrap();

        assert!(store
            .load("ethereum", "del-test")
            .await
            .unwrap()
            .is_none());
    }

    // ── 11. Event insert and query by schema ──────────────────────────────────

    #[test]
    fn event_insert_and_query_by_schema() {
        let store = RocksDbStorage::in_memory();

        store
            .insert_event(&sample_event("ERC20Transfer", "0xA", 100, 0))
            .unwrap();
        store
            .insert_event(&sample_event("ERC20Transfer", "0xA", 101, 0))
            .unwrap();
        store
            .insert_event(&sample_event("UniswapV3Swap", "0xB", 102, 0))
            .unwrap();

        let transfers = store.events_by_schema("ERC20Transfer").unwrap();
        assert_eq!(transfers.len(), 2);
        let swaps = store.events_by_schema("UniswapV3Swap").unwrap();
        assert_eq!(swaps.len(), 1);
    }

    // ── 12. Event query by address ────────────────────────────────────────────

    #[test]
    fn event_query_by_address() {
        let store = RocksDbStorage::in_memory();

        let addr_a = "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let addr_b = "0xBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

        store
            .insert_event(&sample_event("Transfer", addr_a, 100, 0))
            .unwrap();
        store
            .insert_event(&sample_event("Transfer", addr_a, 101, 0))
            .unwrap();
        store
            .insert_event(&sample_event("Transfer", addr_b, 102, 0))
            .unwrap();

        let events_a = store.events_by_address(addr_a).unwrap();
        assert_eq!(events_a.len(), 2);

        let events_b = store.events_by_address(addr_b).unwrap();
        assert_eq!(events_b.len(), 1);
    }

    // ── 13. Event query by block range ────────────────────────────────────────

    #[test]
    fn event_query_by_block_range() {
        let store = RocksDbStorage::in_memory();

        for block in 100u64..=110 {
            store
                .insert_event(&sample_event("Transfer", "0xA", block, 0))
                .unwrap();
        }

        let mid = store.events_in_block_range(103, 107).unwrap();
        assert_eq!(mid.len(), 5);
        assert_eq!(mid.first().unwrap().block_number, 103);
        assert_eq!(mid.last().unwrap().block_number, 107);
    }

    // ── 14. Event batch insert ────────────────────────────────────────────────

    #[test]
    fn event_batch_insert() {
        let store = RocksDbStorage::in_memory();

        let events: Vec<DecodedEvent> = (200u64..210)
            .map(|b| sample_event("BatchSchema", "0xC", b, 0))
            .collect();

        store.insert_events_batch(&events).unwrap();

        let stored = store.events_by_schema("BatchSchema").unwrap();
        assert_eq!(stored.len(), 10);
    }

    // ── 15. Event batch insert — empty slice is no-op ─────────────────────────

    #[test]
    fn event_batch_insert_empty_is_noop() {
        let store = RocksDbStorage::in_memory();
        store.insert_events_batch(&[]).unwrap();
        let events = store.events_by_schema("Any").unwrap();
        assert!(events.is_empty());
    }

    // ── 16. Rollback after block ──────────────────────────────────────────────

    #[test]
    fn rollback_after_block() {
        let store = RocksDbStorage::in_memory();

        for block in 100u64..=110 {
            store
                .insert_event(&sample_event("Transfer", "0xD", block, 0))
                .unwrap();
        }

        let deleted = store.rollback_after(105).unwrap();
        assert_eq!(deleted, 5); // blocks 106–110 removed

        let remaining = store.events_in_block_range(100, 110).unwrap();
        assert_eq!(remaining.len(), 6); // 100–105 survive
        assert!(remaining
            .iter()
            .all(|e| e.block_number <= 105));
    }

    // ── 17. Block hash CRUD ───────────────────────────────────────────────────

    #[test]
    fn block_hash_crud() {
        let store = RocksDbStorage::in_memory();

        store
            .insert_block_hash("ethereum", 100, "0xAAA")
            .unwrap();
        store
            .insert_block_hash("ethereum", 101, "0xBBB")
            .unwrap();

        assert_eq!(
            store.get_block_hash("ethereum", 100).unwrap().unwrap(),
            "0xAAA"
        );
        assert_eq!(
            store.get_block_hash("ethereum", 101).unwrap().unwrap(),
            "0xBBB"
        );
        assert!(store.get_block_hash("ethereum", 999).unwrap().is_none());
    }

    // ── 18. Block hash — chain isolation ─────────────────────────────────────

    #[test]
    fn block_hash_chain_isolation() {
        let store = RocksDbStorage::in_memory();

        store
            .insert_block_hash("ethereum", 100, "0xETH")
            .unwrap();
        store
            .insert_block_hash("polygon", 100, "0xPOL")
            .unwrap();

        assert_eq!(
            store.get_block_hash("ethereum", 100).unwrap().unwrap(),
            "0xETH"
        );
        assert_eq!(
            store.get_block_hash("polygon", 100).unwrap().unwrap(),
            "0xPOL"
        );
    }

    // ── 19. Block hash pruning ────────────────────────────────────────────────

    #[test]
    fn block_hash_pruning() {
        let store = RocksDbStorage::in_memory();

        for b in 0u64..20 {
            store
                .insert_block_hash("ethereum", b, &format!("0x{b:064x}"))
                .unwrap();
        }

        // Keep only the last 5 entries (blocks 15–19).
        let pruned = store.prune_block_hashes("ethereum", 5).unwrap();
        assert_eq!(pruned, 15);

        // Blocks 0–14 should be gone.
        for b in 0u64..15 {
            assert!(
                store.get_block_hash("ethereum", b).unwrap().is_none(),
                "block {b} should have been pruned"
            );
        }
        // Blocks 15–19 should survive.
        for b in 15u64..20 {
            assert!(
                store.get_block_hash("ethereum", b).unwrap().is_some(),
                "block {b} should survive pruning"
            );
        }
    }

    // ── 20. Block hash pruning — keep_last >= total is no-op ─────────────────

    #[test]
    fn block_hash_prune_noop_when_fewer_than_keep_last() {
        let store = RocksDbStorage::in_memory();

        store.insert_block_hash("ethereum", 1, "0x1").unwrap();
        store.insert_block_hash("ethereum", 2, "0x2").unwrap();

        let pruned = store.prune_block_hashes("ethereum", 100).unwrap();
        assert_eq!(pruned, 0);
    }

    // ── 21. CompactionConfig defaults ────────────────────────────────────────

    #[test]
    fn compaction_config_defaults() {
        let cfg = CompactionConfig::default();
        assert_eq!(cfg.max_open_files, 256);
        assert_eq!(cfg.write_buffer_size, 64 * 1024 * 1024);
        assert_eq!(cfg.max_write_buffer_number, 3);
        assert_eq!(cfg.target_file_size_base, 64 * 1024 * 1024);
        assert_eq!(cfg.block_cache_size, 256 * 1024 * 1024);
    }

    // ── 22. StorageStats collection ───────────────────────────────────────────

    #[tokio::test]
    async fn storage_stats_collection() {
        let store = RocksDbStorage::in_memory();

        // Initially everything is zero.
        let stats = StorageStats::collect(&store);
        assert_eq!(stats.total_events, 0);
        assert_eq!(stats.total_checkpoints, 0);
        assert_eq!(stats.total_block_hashes, 0);
        assert_eq!(stats.disk_usage_bytes, 0);

        // Add some data.
        store
            .insert_event(&sample_event("ERC20Transfer", "0xE", 100, 0))
            .unwrap();
        store
            .insert_event(&sample_event("ERC20Transfer", "0xE", 101, 0))
            .unwrap();
        store
            .save(sample_checkpoint("ethereum", "idx", 100))
            .await
            .unwrap();
        store.insert_block_hash("ethereum", 100, "0xHASH").unwrap();

        let stats = StorageStats::collect(&store);
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.total_checkpoints, 1);
        assert_eq!(stats.total_block_hashes, 1);
        assert!(stats.disk_usage_bytes > 0);
    }

    // ── 23. Key encoding order (events sorted by block number) ────────────────

    #[test]
    fn event_key_encoding_lexicographic_order() {
        let store = RocksDbStorage::in_memory();

        // Insert in reverse order to confirm retrieval is always sorted.
        for block in (100u64..=110).rev() {
            store
                .insert_event(&sample_event("Transfer", "0xF", block, 0))
                .unwrap();
        }

        let events = store.events_in_block_range(100, 110).unwrap();
        assert_eq!(events.len(), 11);

        let block_numbers: Vec<u64> = events.iter().map(|e| e.block_number).collect();
        let mut sorted = block_numbers.clone();
        sorted.sort();
        assert_eq!(block_numbers, sorted, "events must be in ascending block order");
    }

    // ── 24. Empty queries return empty results ────────────────────────────────

    #[test]
    fn empty_queries_return_empty_results() {
        let store = RocksDbStorage::in_memory();

        assert!(store.events_by_schema("NonExistent").unwrap().is_empty());
        assert!(store.events_by_address("0x0000").unwrap().is_empty());
        assert!(store.events_in_block_range(0, 1_000_000).unwrap().is_empty());
    }

    // ── 25. Multiple log indices in same block are all stored ─────────────────

    #[test]
    fn multiple_log_indices_same_block() {
        let store = RocksDbStorage::in_memory();

        for log_index in 0u32..5 {
            store
                .insert_event(&sample_event("Transfer", "0xG", 200, log_index))
                .unwrap();
        }

        let events = store.events_in_block_range(200, 200).unwrap();
        assert_eq!(events.len(), 5);

        let log_indices: Vec<u32> = events.iter().map(|e| e.log_index).collect();
        let mut sorted = log_indices.clone();
        sorted.sort();
        assert_eq!(log_indices, sorted, "log indices must be in ascending order");
    }

    // ── 26. BTreeKvStore flush is a no-op ─────────────────────────────────────

    #[test]
    fn btree_kv_flush_noop() {
        let kv = BTreeKvStore::new();
        kv.create_cf("cf");
        kv.put("cf", b"k", b"v").unwrap();
        kv.flush().unwrap(); // must not error
        assert_eq!(kv.get("cf", b"k").unwrap().unwrap(), b"v");
    }
}
