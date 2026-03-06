//! chainindex-core — foundation for the reorg-safe, embeddable indexing engine.
//!
//! # Architecture
//!
//! ```text
//! IndexerBuilder → IndexLoop
//!                      ├── BlockTracker     (head tracking, parent hash chain)
//!                      ├── ReorgDetector    (4 reorg scenarios)
//!                      ├── CheckpointManager (crash recovery)
//!                      ├── HandlerRegistry  (user event/block handlers)
//!                      └── Storage backend (memory / SQLite / Postgres)
//! ```

pub mod backfill;
pub mod block_handler;
pub mod checkpoint;
pub mod cursor;
pub mod dlq;
pub mod entity;
pub mod error;
pub mod export;
pub mod factory;
pub mod finality;
pub mod graphql;
pub mod handler;
pub mod hotreload;
pub mod idempotency;
pub mod indexer;
pub mod metrics;
pub mod multichain;
pub mod reorg;
pub mod streaming;
pub mod trace;
pub mod tracker;
pub mod types;

pub use checkpoint::CheckpointManager;
pub use cursor::Cursor;
pub use error::IndexerError;
pub use handler::{EventHandler, HandlerRegistry};
pub use indexer::{IndexerConfig, IndexerState};
pub use reorg::{ReorgDetector, ReorgEvent};
pub use tracker::{BlockInfo, BlockTracker};
pub use types::{BlockSummary, EventFilter, IndexContext};
