//! Fluent builder API for creating EVM indexers.
//!
//! # Example
//!
//! ```rust,no_run
//! use chainindex_evm::IndexerBuilder;
//! use chainindex_core::types::EventFilter;
//!
//! let config = IndexerBuilder::new()
//!     .chain("ethereum")
//!     .from_block(19_000_000)
//!     .confirmation_depth(12)
//!     .batch_size(500)
//!     .filter(EventFilter::address("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"))
//!     .build_config();
//! ```

use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::EventFilter;

/// Fluent builder for `IndexerConfig`.
#[derive(Default)]
pub struct IndexerBuilder {
    config: IndexerConfig,
}

impl IndexerBuilder {
    pub fn new() -> Self {
        Self {
            config: IndexerConfig::default(),
        }
    }

    /// Set the indexer ID (used for checkpoint keys).
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.config.id = id.into();
        self
    }

    /// Set the chain to index.
    pub fn chain(mut self, chain: impl Into<String>) -> Self {
        self.config.chain = chain.into();
        self
    }

    /// Set the start block.
    pub fn from_block(mut self, block: u64) -> Self {
        self.config.from_block = block;
        self
    }

    /// Set the end block (for bounded backfill).
    pub fn to_block(mut self, block: u64) -> Self {
        self.config.to_block = Some(block);
        self
    }

    /// Set confirmation depth (blocks behind head before processing).
    pub fn confirmation_depth(mut self, depth: u64) -> Self {
        self.config.confirmation_depth = depth;
        self
    }

    /// Set the number of blocks per `eth_getLogs` batch.
    pub fn batch_size(mut self, size: u64) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Set checkpoint save interval (every N blocks).
    pub fn checkpoint_interval(mut self, n: u64) -> Self {
        self.config.checkpoint_interval = n;
        self
    }

    /// Set live mode polling interval in milliseconds.
    pub fn poll_interval_ms(mut self, ms: u64) -> Self {
        self.config.poll_interval_ms = ms;
        self
    }

    /// Set the event/address filter.
    pub fn filter(mut self, filter: EventFilter) -> Self {
        self.config.filter = filter;
        self
    }

    /// Build the `IndexerConfig`.
    pub fn build_config(self) -> IndexerConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let cfg = IndexerBuilder::new().build_config();
        assert_eq!(cfg.chain, "ethereum");
        assert_eq!(cfg.confirmation_depth, 12);
        assert_eq!(cfg.batch_size, 1000);
    }

    #[test]
    fn builder_custom() {
        let cfg = IndexerBuilder::new()
            .id("my-indexer")
            .chain("polygon")
            .from_block(50_000_000)
            .confirmation_depth(32)
            .batch_size(500)
            .build_config();

        assert_eq!(cfg.id, "my-indexer");
        assert_eq!(cfg.chain, "polygon");
        assert_eq!(cfg.from_block, 50_000_000);
        assert_eq!(cfg.confirmation_depth, 32);
        assert_eq!(cfg.batch_size, 500);
    }
}
