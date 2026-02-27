//! Shared types for the indexing pipeline.

use serde::{Deserialize, Serialize};

// ─── BlockSummary ─────────────────────────────────────────────────────────────

/// A minimal summary of a block — enough for the index loop to track progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockSummary {
    /// Block number.
    pub number: u64,
    /// Block hash (`0x…`).
    pub hash: String,
    /// Parent block hash (`0x…`).
    pub parent_hash: String,
    /// Unix timestamp of the block (seconds since epoch).
    pub timestamp: i64,
    /// Number of transactions in the block.
    pub tx_count: u32,
}

impl BlockSummary {
    /// Returns `true` if `parent` is the direct parent of `self`.
    pub fn extends(&self, parent: &BlockSummary) -> bool {
        self.number == parent.number + 1 && self.parent_hash == parent.hash
    }
}

// ─── EventFilter ─────────────────────────────────────────────────────────────

/// Filter for which events/logs to index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventFilter {
    /// Only index logs from these contract addresses (empty = all addresses).
    pub addresses: Vec<String>,
    /// Only index logs with this topic[0] value (empty = all events).
    pub topic0_values: Vec<String>,
    /// Start block (inclusive).
    pub from_block: Option<u64>,
    /// End block (inclusive); `None` = live (keep running).
    pub to_block: Option<u64>,
}

impl EventFilter {
    /// Create a filter for a single contract address.
    pub fn address(addr: impl Into<String>) -> Self {
        Self {
            addresses: vec![addr.into()],
            ..Default::default()
        }
    }

    /// Add a topic0 filter (event signature hash).
    pub fn topic0(mut self, topic: impl Into<String>) -> Self {
        self.topic0_values.push(topic.into());
        self
    }

    /// Set the start block.
    pub fn from_block(mut self, block: u64) -> Self {
        self.from_block = Some(block);
        self
    }

    /// Returns `true` if `address` matches this filter.
    pub fn matches_address(&self, address: &str) -> bool {
        self.addresses.is_empty()
            || self.addresses.iter().any(|a| a.eq_ignore_ascii_case(address))
    }

    /// Returns `true` if `topic0` matches this filter.
    pub fn matches_topic0(&self, topic0: &str) -> bool {
        self.topic0_values.is_empty()
            || self.topic0_values.iter().any(|t| t.eq_ignore_ascii_case(topic0))
    }
}

// ─── IndexContext ─────────────────────────────────────────────────────────────

/// Context passed to event/block handlers during indexing.
#[derive(Debug, Clone)]
pub struct IndexContext {
    /// The block being processed.
    pub block: BlockSummary,
    /// Current index phase.
    pub phase: IndexPhase,
    /// The indexer's chain slug (e.g. `"ethereum"`).
    pub chain: String,
}

/// The current phase of the index loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexPhase {
    /// Catching up to the chain head (processing historical blocks).
    Backfill,
    /// Following the chain tip in real-time.
    Live,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_extends_parent() {
        let parent = BlockSummary {
            number: 100,
            hash: "0xaaa".into(),
            parent_hash: "0x000".into(),
            timestamp: 1000,
            tx_count: 5,
        };
        let child = BlockSummary {
            number: 101,
            hash: "0xbbb".into(),
            parent_hash: "0xaaa".into(),
            timestamp: 1012,
            tx_count: 3,
        };
        assert!(child.extends(&parent));
        assert!(!parent.extends(&child));
    }

    #[test]
    fn block_extends_false_on_gap() {
        let a = BlockSummary {
            number: 100,
            hash: "0xaaa".into(),
            parent_hash: "0x000".into(),
            timestamp: 1000,
            tx_count: 0,
        };
        let b = BlockSummary {
            number: 102, // gap
            hash: "0xccc".into(),
            parent_hash: "0xaaa".into(),
            timestamp: 1024,
            tx_count: 0,
        };
        assert!(!b.extends(&a));
    }

    #[test]
    fn event_filter_matches_address() {
        let f = EventFilter::address("0xAbCdEf");
        assert!(f.matches_address("0xabcdef")); // case-insensitive
        assert!(!f.matches_address("0x111111"));
    }

    #[test]
    fn event_filter_empty_matches_all() {
        let f = EventFilter::default();
        assert!(f.matches_address("0xanything"));
        assert!(f.matches_topic0("0xanything"));
    }
}
