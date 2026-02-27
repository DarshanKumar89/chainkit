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

pub mod checkpoint;
pub mod cursor;
pub mod error;
pub mod handler;
pub mod indexer;
pub mod reorg;
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
