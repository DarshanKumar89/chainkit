//! # chainindex (Node.js)
//!
//! napi-rs Node.js bindings for ChainIndex.
//!
//! ## Usage
//! ```typescript
//! import { IndexerConfig, InMemoryStorage } from '@chainfoundry/chainindex';
//!
//! const config = new IndexerConfig();
//! config.chain = "ethereum";
//! config.fromBlock = BigInt(19_000_000);
//! config.confirmationDepth = BigInt(12);
//!
//! const storage = new InMemoryStorage();
//! const checkpoint = await storage.loadCheckpoint("ethereum", "my-indexer");
//! ```

#![deny(clippy::all)]

use napi::bindgen_prelude::*;
use napi_derive::napi;

use chainindex_core::checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
use chainindex_core::indexer::{IndexerConfig, IndexerState};
use chainindex_core::types::{BlockSummary, EventFilter};

// ─── IndexerConfig ────────────────────────────────────────────────────────────

/// Configuration for an indexer instance.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsIndexerConfig {
    /// Unique name for this indexer (used for checkpoint keys).
    pub id: String,
    /// Chain slug (e.g. "ethereum", "polygon").
    pub chain: String,
    /// First block to index.
    pub from_block: BigInt,
    /// Optional end block for bounded backfill.
    pub to_block: Option<BigInt>,
    /// Blocks behind head before processing (default: 12 for Ethereum).
    pub confirmation_depth: BigInt,
    /// Blocks per eth_getLogs batch (default: 1000).
    pub batch_size: BigInt,
    /// Save checkpoint every N blocks (default: 100).
    pub checkpoint_interval: BigInt,
    /// Live mode polling interval in milliseconds (default: 2000).
    pub poll_interval_ms: BigInt,
}

impl From<IndexerConfig> for JsIndexerConfig {
    fn from(c: IndexerConfig) -> Self {
        Self {
            id: c.id,
            chain: c.chain,
            from_block: BigInt::from(c.from_block),
            to_block: c.to_block.map(BigInt::from),
            confirmation_depth: BigInt::from(c.confirmation_depth),
            batch_size: BigInt::from(c.batch_size),
            checkpoint_interval: BigInt::from(c.checkpoint_interval),
            poll_interval_ms: BigInt::from(c.poll_interval_ms),
        }
    }
}

impl From<JsIndexerConfig> for IndexerConfig {
    fn from(c: JsIndexerConfig) -> Self {
        Self {
            id: c.id,
            chain: c.chain,
            from_block: c.from_block.get_u64().1,
            to_block: c.to_block.map(|b| b.get_u64().1),
            confirmation_depth: c.confirmation_depth.get_u64().1,
            batch_size: c.batch_size.get_u64().1,
            checkpoint_interval: c.checkpoint_interval.get_u64().1,
            poll_interval_ms: c.poll_interval_ms.get_u64().1,
            filter: EventFilter::default(),
        }
    }
}

/// Create a default IndexerConfig.
#[napi]
pub fn default_indexer_config() -> JsIndexerConfig {
    IndexerConfig::default().into()
}

// ─── Checkpoint ───────────────────────────────────────────────────────────────

/// A persisted indexer checkpoint.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsCheckpoint {
    pub chain_id: String,
    pub indexer_id: String,
    pub block_number: BigInt,
    pub block_hash: String,
    pub updated_at: i64,
}

impl From<Checkpoint> for JsCheckpoint {
    fn from(c: Checkpoint) -> Self {
        Self {
            chain_id: c.chain_id,
            indexer_id: c.indexer_id,
            block_number: BigInt::from(c.block_number),
            block_hash: c.block_hash,
            updated_at: c.updated_at,
        }
    }
}

impl From<JsCheckpoint> for Checkpoint {
    fn from(c: JsCheckpoint) -> Self {
        Self {
            chain_id: c.chain_id,
            indexer_id: c.indexer_id,
            block_number: c.block_number.get_u64().1,
            block_hash: c.block_hash,
            updated_at: c.updated_at,
        }
    }
}

// ─── BlockSummary ─────────────────────────────────────────────────────────────

/// A minimal block summary returned by the EVM fetcher.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsBlockSummary {
    pub number: BigInt,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: i64,
    pub tx_count: u32,
}

impl From<BlockSummary> for JsBlockSummary {
    fn from(b: BlockSummary) -> Self {
        Self {
            number: BigInt::from(b.number),
            hash: b.hash,
            parent_hash: b.parent_hash,
            timestamp: b.timestamp,
            tx_count: b.tx_count,
        }
    }
}

// ─── InMemoryStorage ──────────────────────────────────────────────────────────

/// In-memory checkpoint storage for development and testing.
///
/// Data is not persisted across restarts.
#[napi]
pub struct InMemoryStorage {
    inner: MemoryCheckpointStore,
}

#[napi]
impl InMemoryStorage {
    /// Create a new in-memory storage.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: MemoryCheckpointStore::new(),
        }
    }

    /// Load a checkpoint for the given chain + indexer pair.
    #[napi]
    pub async fn load_checkpoint(
        &self,
        chain_id: String,
        indexer_id: String,
    ) -> Result<Option<JsCheckpoint>> {
        self.inner
            .load(&chain_id, &indexer_id)
            .await
            .map(|opt| opt.map(JsCheckpoint::from))
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    /// Save a checkpoint.
    #[napi]
    pub async fn save_checkpoint(&self, checkpoint: JsCheckpoint) -> Result<()> {
        self.inner
            .save(checkpoint.into())
            .await
            .map_err(|e| Error::from_reason(e.to_string()))
    }

    /// Delete a checkpoint.
    #[napi]
    pub async fn delete_checkpoint(&self, chain_id: String, indexer_id: String) -> Result<()> {
        self.inner
            .delete(&chain_id, &indexer_id)
            .await
            .map_err(|e| Error::from_reason(e.to_string()))
    }
}

// ─── EventFilter ──────────────────────────────────────────────────────────────

/// Filter configuration for which events to index.
#[napi]
pub struct JsEventFilter {
    inner: EventFilter,
}

#[napi]
impl JsEventFilter {
    /// Create an empty filter (matches all events).
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            inner: EventFilter::default(),
        }
    }

    /// Create a filter for a specific contract address.
    #[napi(factory)]
    pub fn for_address(address: String) -> Self {
        Self {
            inner: EventFilter::address(address),
        }
    }

    /// Add a topic0 filter (event signature hash, e.g. keccak256("Transfer(address,address,uint256)")).
    #[napi]
    pub fn add_topic0(&mut self, topic: String) {
        self.inner.topic0_values.push(topic);
    }

    /// Add an additional contract address to the filter.
    #[napi]
    pub fn add_address(&mut self, address: String) {
        self.inner.addresses.push(address);
    }

    /// Set the start block for this filter.
    #[napi]
    pub fn set_from_block(&mut self, block: BigInt) {
        self.inner.from_block = Some(block.get_u64().1);
    }

    /// Check if an address matches this filter.
    #[napi]
    pub fn matches_address(&self, address: String) -> bool {
        self.inner.matches_address(&address)
    }

    /// Check if a topic0 matches this filter.
    #[napi]
    pub fn matches_topic0(&self, topic0: String) -> bool {
        self.inner.matches_topic0(&topic0)
    }

    /// Return the filter as a JSON string.
    #[napi]
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(&self.inner).map_err(|e| Error::from_reason(e.to_string()))
    }
}

// ─── IndexerState ─────────────────────────────────────────────────────────────

/// Get a string representation of an indexer state.
///
/// States: "idle" | "backfilling" | "live" | "reorg-recovery" | "stopping" | "stopped" | "error"
#[napi]
pub fn indexer_state_name(state: String) -> String {
    match state.as_str() {
        "idle" => IndexerState::Idle.to_string(),
        "backfilling" => IndexerState::Backfilling.to_string(),
        "live" => IndexerState::Live.to_string(),
        "reorg-recovery" => IndexerState::ReorgRecovery.to_string(),
        "stopping" => IndexerState::Stopping.to_string(),
        "stopped" => IndexerState::Stopped.to_string(),
        _ => "unknown".into(),
    }
}

// ─── Utility functions ────────────────────────────────────────────────────────

/// Check if a block extends (is a direct child of) its parent.
///
/// Returns true if child.number == parent.number + 1 and child.parentHash == parent.hash.
#[napi]
pub fn block_extends_parent(child: JsBlockSummary, parent: JsBlockSummary) -> bool {
    let child_rust = BlockSummary {
        number: child.number.get_u64().1,
        hash: child.hash,
        parent_hash: child.parent_hash,
        timestamp: child.timestamp,
        tx_count: child.tx_count,
    };
    let parent_rust = BlockSummary {
        number: parent.number.get_u64().1,
        hash: parent.hash,
        parent_hash: parent.parent_hash,
        timestamp: parent.timestamp,
        tx_count: parent.tx_count,
    };
    child_rust.extends(&parent_rust)
}

use serde_json;
