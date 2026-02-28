//! # chainindex (Python)
//!
//! PyO3-based Python bindings for ChainIndex.
//!
//! ## Usage
//! ```python
//! import asyncio
//! from chainindex import IndexerConfig, InMemoryStorage, EventFilter
//!
//! async def main():
//!     # Build indexer config
//!     config = IndexerConfig(
//!         chain="ethereum",
//!         from_block=19_000_000,
//!         confirmation_depth=12,
//!         batch_size=1000,
//!     )
//!
//!     # In-memory storage (dev/testing)
//!     storage = InMemoryStorage()
//!
//!     # Save / load checkpoints
//!     from chainindex import Checkpoint
//!     cp = Checkpoint(
//!         chain_id="ethereum",
//!         indexer_id="my-indexer",
//!         block_number=19_001_000,
//!         block_hash="0xabc...",
//!     )
//!     await storage.save_checkpoint(cp)
//!     loaded = await storage.load_checkpoint("ethereum", "my-indexer")
//!     print(loaded)
//!
//! asyncio.run(main())
//! ```

use pyo3::prelude::*;
use pyo3_asyncio::tokio::future_into_py;
use std::sync::Arc;

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
use chainindex_core::indexer::{IndexerConfig, IndexerState};
use chainindex_core::types::{BlockSummary, EventFilter};

// ─── Checkpoint ───────────────────────────────────────────────────────────────

#[pyclass(name = "Checkpoint")]
#[derive(Clone)]
pub struct PyCheckpoint {
    pub chain_id: String,
    pub indexer_id: String,
    pub block_number: u64,
    pub block_hash: String,
    pub updated_at: i64,
}

#[pymethods]
impl PyCheckpoint {
    #[new]
    #[pyo3(signature = (chain_id, indexer_id, block_number, block_hash, updated_at=None))]
    fn new(
        chain_id: String,
        indexer_id: String,
        block_number: u64,
        block_hash: String,
        updated_at: Option<i64>,
    ) -> Self {
        Self {
            chain_id,
            indexer_id,
            block_number,
            block_hash,
            updated_at: updated_at.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
            }),
        }
    }

    #[getter]
    fn chain_id(&self) -> &str { &self.chain_id }
    #[getter]
    fn indexer_id(&self) -> &str { &self.indexer_id }
    #[getter]
    fn block_number(&self) -> u64 { self.block_number }
    #[getter]
    fn block_hash(&self) -> &str { &self.block_hash }
    #[getter]
    fn updated_at(&self) -> i64 { self.updated_at }

    fn __repr__(&self) -> String {
        format!(
            "Checkpoint(chain_id={:?}, indexer_id={:?}, block_number={}, block_hash={:?})",
            self.chain_id, self.indexer_id, self.block_number, self.block_hash
        )
    }
}

impl From<Checkpoint> for PyCheckpoint {
    fn from(c: Checkpoint) -> Self {
        Self {
            chain_id: c.chain_id,
            indexer_id: c.indexer_id,
            block_number: c.block_number,
            block_hash: c.block_hash,
            updated_at: c.updated_at,
        }
    }
}

impl From<PyCheckpoint> for Checkpoint {
    fn from(c: PyCheckpoint) -> Self {
        Self {
            chain_id: c.chain_id,
            indexer_id: c.indexer_id,
            block_number: c.block_number,
            block_hash: c.block_hash,
            updated_at: c.updated_at,
        }
    }
}

// ─── IndexerConfig ────────────────────────────────────────────────────────────

/// Configuration for an EVM indexer.
#[pyclass(name = "IndexerConfig")]
#[derive(Clone)]
pub struct PyIndexerConfig {
    inner: IndexerConfig,
}

#[pymethods]
impl PyIndexerConfig {
    /// Create a new IndexerConfig with sensible defaults.
    #[new]
    #[pyo3(signature = (
        id = "default".to_string(),
        chain = "ethereum".to_string(),
        from_block = 0,
        to_block = None,
        confirmation_depth = 12,
        batch_size = 1000,
        checkpoint_interval = 100,
        poll_interval_ms = 2000,
    ))]
    fn new(
        id: String,
        chain: String,
        from_block: u64,
        to_block: Option<u64>,
        confirmation_depth: u64,
        batch_size: u64,
        checkpoint_interval: u64,
        poll_interval_ms: u64,
    ) -> Self {
        Self {
            inner: IndexerConfig {
                id,
                chain,
                from_block,
                to_block,
                confirmation_depth,
                batch_size,
                checkpoint_interval,
                poll_interval_ms,
                filter: EventFilter::default(),
            },
        }
    }

    #[getter]
    fn id(&self) -> &str { &self.inner.id }
    #[getter]
    fn chain(&self) -> &str { &self.inner.chain }
    #[getter]
    fn from_block(&self) -> u64 { self.inner.from_block }
    #[getter]
    fn to_block(&self) -> Option<u64> { self.inner.to_block }
    #[getter]
    fn confirmation_depth(&self) -> u64 { self.inner.confirmation_depth }
    #[getter]
    fn batch_size(&self) -> u64 { self.inner.batch_size }
    #[getter]
    fn checkpoint_interval(&self) -> u64 { self.inner.checkpoint_interval }
    #[getter]
    fn poll_interval_ms(&self) -> u64 { self.inner.poll_interval_ms }

    /// Serialize config to JSON.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "IndexerConfig(id={:?}, chain={:?}, from_block={}, confirmation_depth={})",
            self.inner.id, self.inner.chain, self.inner.from_block, self.inner.confirmation_depth
        )
    }
}

// ─── EventFilter ──────────────────────────────────────────────────────────────

/// Filter for which on-chain events to index.
#[pyclass(name = "EventFilter")]
pub struct PyEventFilter {
    inner: EventFilter,
}

#[pymethods]
impl PyEventFilter {
    /// Create an empty filter (matches all events).
    #[new]
    fn new() -> Self {
        Self { inner: EventFilter::default() }
    }

    /// Create a filter for a single contract address.
    #[staticmethod]
    fn for_address(address: String) -> Self {
        Self { inner: EventFilter::address(address) }
    }

    /// Add a topic0 value (keccak256 of the event signature).
    fn add_topic0(&mut self, topic: String) {
        self.inner.topic0_values.push(topic);
    }

    /// Add a contract address to the filter.
    fn add_address(&mut self, address: String) {
        self.inner.addresses.push(address);
    }

    /// Set the start block.
    fn set_from_block(&mut self, block: u64) {
        self.inner.from_block = Some(block);
    }

    /// Check if an address matches.
    fn matches_address(&self, address: &str) -> bool {
        self.inner.matches_address(address)
    }

    /// Check if a topic0 matches.
    fn matches_topic0(&self, topic0: &str) -> bool {
        self.inner.matches_topic0(topic0)
    }

    fn __repr__(&self) -> String {
        format!(
            "EventFilter(addresses={:?}, topic0_values={:?})",
            self.inner.addresses, self.inner.topic0_values
        )
    }
}

// ─── InMemoryStorage ──────────────────────────────────────────────────────────

/// In-memory storage for checkpoints (dev/testing, no persistence).
#[pyclass(name = "InMemoryStorage")]
pub struct PyInMemoryStorage {
    inner: Arc<MemoryCheckpointStore>,
}

#[pymethods]
impl PyInMemoryStorage {
    /// Create a new in-memory storage backend.
    #[new]
    fn new() -> Self {
        Self { inner: Arc::new(MemoryCheckpointStore::new()) }
    }

    /// Load a checkpoint for the given chain and indexer (async).
    fn load_checkpoint<'py>(
        &self,
        py: Python<'py>,
        chain_id: String,
        indexer_id: String,
    ) -> PyResult<&'py PyAny> {
        let store = self.inner.clone();
        future_into_py(py, async move {
            let result = store.load(&chain_id, &indexer_id).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(result.map(PyCheckpoint::from))
        })
    }

    /// Save (upsert) a checkpoint (async).
    fn save_checkpoint<'py>(
        &self,
        py: Python<'py>,
        checkpoint: PyCheckpoint,
    ) -> PyResult<&'py PyAny> {
        let store = self.inner.clone();
        future_into_py(py, async move {
            store.save(checkpoint.into()).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Delete a checkpoint (async).
    fn delete_checkpoint<'py>(
        &self,
        py: Python<'py>,
        chain_id: String,
        indexer_id: String,
    ) -> PyResult<&'py PyAny> {
        let store = self.inner.clone();
        future_into_py(py, async move {
            store.delete(&chain_id, &indexer_id).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    fn __repr__(&self) -> String {
        "InMemoryStorage()".to_string()
    }
}

// ─── BlockSummary ─────────────────────────────────────────────────────────────

/// Summary of an indexed block.
#[pyclass(name = "BlockSummary")]
#[derive(Clone)]
pub struct PyBlockSummary {
    #[pyo3(get)]
    pub number: u64,
    #[pyo3(get)]
    pub hash: String,
    #[pyo3(get)]
    pub parent_hash: String,
    #[pyo3(get)]
    pub timestamp: i64,
    #[pyo3(get)]
    pub tx_count: u32,
}

#[pymethods]
impl PyBlockSummary {
    #[new]
    fn new(number: u64, hash: String, parent_hash: String, timestamp: i64, tx_count: u32) -> Self {
        Self { number, hash, parent_hash, timestamp, tx_count }
    }

    /// Returns True if this block directly extends the given parent block.
    fn extends_parent(&self, parent: &PyBlockSummary) -> bool {
        let self_block = BlockSummary {
            number: self.number,
            hash: self.hash.clone(),
            parent_hash: self.parent_hash.clone(),
            timestamp: self.timestamp,
            tx_count: self.tx_count,
        };
        let parent_block = BlockSummary {
            number: parent.number,
            hash: parent.hash.clone(),
            parent_hash: parent.parent_hash.clone(),
            timestamp: parent.timestamp,
            tx_count: parent.tx_count,
        };
        self_block.extends(&parent_block)
    }

    fn __repr__(&self) -> String {
        format!(
            "BlockSummary(number={}, hash={:?}, tx_count={})",
            self.number, self.hash, self.tx_count
        )
    }
}

// ─── Module ───────────────────────────────────────────────────────────────────

#[pymodule]
fn chainindex(py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_asyncio::tokio::init_multi_thread_once();
    m.add_class::<PyCheckpoint>()?;
    m.add_class::<PyIndexerConfig>()?;
    m.add_class::<PyEventFilter>()?;
    m.add_class::<PyInMemoryStorage>()?;
    m.add_class::<PyBlockSummary>()?;
    Ok(())
}
