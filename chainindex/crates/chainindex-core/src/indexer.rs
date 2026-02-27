//! Indexer configuration and state types.

use serde::{Deserialize, Serialize};

use crate::types::EventFilter;

/// Configuration for an indexer instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    /// Unique name for this indexer (used for checkpoint keys).
    pub id: String,
    /// Chain to index (e.g. `"ethereum"`).
    pub chain: String,
    /// First block to index.
    pub from_block: u64,
    /// Optional end block (for bounded backfill). `None` = run forever.
    pub to_block: Option<u64>,
    /// Number of blocks to wait before considering a block confirmed.
    /// Typical values: 12 (Ethereum PoS), 64 (Ethereum safe), 1 (fast chains).
    pub confirmation_depth: u64,
    /// How many blocks to batch-fetch per `eth_getLogs` call.
    pub batch_size: u64,
    /// How often to save a checkpoint (every N blocks).
    pub checkpoint_interval: u64,
    /// Block polling interval in live mode (milliseconds).
    pub poll_interval_ms: u64,
    /// Event/address filter.
    pub filter: EventFilter,
}

impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            id: "default".into(),
            chain: "ethereum".into(),
            from_block: 0,
            to_block: None,
            confirmation_depth: 12,
            batch_size: 1000,
            checkpoint_interval: 100,
            poll_interval_ms: 2000,
            filter: EventFilter::default(),
        }
    }
}

/// Runtime state of the indexer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexerState {
    /// Not yet started.
    Idle,
    /// Syncing historical blocks up to the current head.
    Backfilling,
    /// Following the chain tip in real-time.
    Live,
    /// Recovering from a reorg.
    ReorgRecovery,
    /// Shutting down gracefully.
    Stopping,
    /// Terminated.
    Stopped,
    /// Encountered an unrecoverable error.
    Error,
}

impl std::fmt::Display for IndexerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Backfilling => write!(f, "backfilling"),
            Self::Live => write!(f, "live"),
            Self::ReorgRecovery => write!(f, "reorg-recovery"),
            Self::Stopping => write!(f, "stopping"),
            Self::Stopped => write!(f, "stopped"),
            Self::Error => write!(f, "error"),
        }
    }
}
