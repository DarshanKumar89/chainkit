//! Indexer cursor — tracks the current position in the chain.

use serde::{Deserialize, Serialize};

/// The indexer's current position in the chain.
///
/// The cursor knows:
/// - Which block was last successfully processed
/// - The confirmation depth (how many blocks behind head we consider "confirmed")
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    /// Last confirmed block number that was processed.
    pub block_number: u64,
    /// Last confirmed block hash.
    pub block_hash: String,
    /// Minimum number of confirmations before processing a block.
    pub confirmation_depth: u64,
}

impl Cursor {
    /// Create a new cursor at the given starting position.
    pub fn new(block_number: u64, block_hash: impl Into<String>, confirmation_depth: u64) -> Self {
        Self {
            block_number,
            block_hash: block_hash.into(),
            confirmation_depth,
        }
    }

    /// Advance the cursor to a new confirmed block.
    pub fn advance(&mut self, block_number: u64, block_hash: impl Into<String>) {
        self.block_number = block_number;
        self.block_hash = block_hash.into();
    }

    /// Returns `true` if `head_number` is far enough ahead for `target` to be confirmed.
    pub fn is_confirmed(&self, target: u64, head_number: u64) -> bool {
        head_number.saturating_sub(target) >= self.confirmation_depth
    }

    /// Returns the next block to process (cursor + 1).
    pub fn next_block(&self) -> u64 {
        self.block_number + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_advance() {
        let mut cursor = Cursor::new(100, "0xaaa", 12);
        cursor.advance(101, "0xbbb");
        assert_eq!(cursor.block_number, 101);
        assert_eq!(cursor.block_hash, "0xbbb");
    }

    #[test]
    fn cursor_confirmation_depth() {
        let cursor = Cursor::new(100, "0xaaa", 12);
        assert!(cursor.is_confirmed(100, 112)); // 112 - 100 = 12 ≥ 12
        assert!(!cursor.is_confirmed(100, 111)); // 111 - 100 = 11 < 12
    }

    #[test]
    fn cursor_next_block() {
        let cursor = Cursor::new(500, "0x123", 6);
        assert_eq!(cursor.next_block(), 501);
    }
}
