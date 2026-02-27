//! Block tracker — maintains a sliding window of recent block headers
//! for parent-hash chain verification and reorg detection.

use std::collections::VecDeque;

use crate::types::BlockSummary;

/// Information about a single tracked block.
pub type BlockInfo = BlockSummary;

/// Tracks the last N block headers to enable reorg detection.
///
/// When a new block arrives, the tracker checks whether its `parent_hash`
/// matches the hash of the previous block. A mismatch means a reorg occurred.
pub struct BlockTracker {
    /// Sliding window of recent blocks (oldest first).
    window: VecDeque<BlockInfo>,
    /// Maximum number of blocks to retain.
    window_size: usize,
}

impl BlockTracker {
    /// Create a new tracker with the given window size.
    /// A window of 128 covers deep reorgs for all major EVM chains.
    pub fn new(window_size: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(window_size),
            window_size,
        }
    }

    /// Add a new block to the tracker.
    ///
    /// Returns `Ok(())` if the block extends the current head.
    /// Returns `Err(reorg_depth)` if a reorg is detected (parent hash mismatch).
    pub fn push(&mut self, block: BlockInfo) -> Result<(), u64> {
        if let Some(head) = self.window.back() {
            if !block.extends(head) {
                // Reorg — find how deep by walking back
                let depth = self.find_reorg_depth(&block);
                return Err(depth);
            }
        }
        if self.window.len() >= self.window_size {
            self.window.pop_front();
        }
        self.window.push_back(block);
        Ok(())
    }

    /// Returns the current chain head (most recently added block).
    pub fn head(&self) -> Option<&BlockInfo> {
        self.window.back()
    }

    /// Returns a block by number if it's in the window.
    pub fn get(&self, number: u64) -> Option<&BlockInfo> {
        self.window.iter().find(|b| b.number == number)
    }

    /// Number of blocks in the window.
    pub fn len(&self) -> usize {
        self.window.len()
    }

    /// Returns `true` if the window is empty.
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    /// Rewind the tracker to a given block number (discard everything after it).
    pub fn rewind_to(&mut self, block_number: u64) {
        while let Some(back) = self.window.back() {
            if back.number > block_number {
                self.window.pop_back();
            } else {
                break;
            }
        }
    }

    /// Find how deep the reorg is by scanning the window.
    fn find_reorg_depth(&self, new_block: &BlockInfo) -> u64 {
        // Walk from newest to oldest looking for a common ancestor
        for (i, tracked) in self.window.iter().enumerate().rev() {
            if tracked.hash == new_block.parent_hash {
                // Found the fork point; depth = number of blocks we need to drop
                return (self.window.len() - 1 - i) as u64;
            }
        }
        // Common ancestor not in window — assume deep reorg
        self.window.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(number: u64, hash: &str, parent: &str) -> BlockInfo {
        BlockSummary {
            number,
            hash: hash.into(),
            parent_hash: parent.into(),
            timestamp: (number * 12) as i64,
            tx_count: 0,
        }
    }

    #[test]
    fn push_normal_chain() {
        let mut tracker = BlockTracker::new(10);
        tracker.push(block(100, "0xa", "0x0")).unwrap();
        tracker.push(block(101, "0xb", "0xa")).unwrap();
        tracker.push(block(102, "0xc", "0xb")).unwrap();
        assert_eq!(tracker.head().unwrap().number, 102);
        assert_eq!(tracker.len(), 3);
    }

    #[test]
    fn push_detects_reorg() {
        let mut tracker = BlockTracker::new(10);
        tracker.push(block(100, "0xa", "0x0")).unwrap();
        tracker.push(block(101, "0xb", "0xa")).unwrap();
        // Reorg: block 102 has a parent that is NOT 0xb
        let result = tracker.push(block(102, "0xc2", "0xb-different"));
        assert!(result.is_err(), "should detect reorg");
    }

    #[test]
    fn rewind_to() {
        let mut tracker = BlockTracker::new(10);
        for i in 100..=110 {
            let prev = if i == 100 { "0x0".to_string() } else { format!("0x{}", i - 1) };
            tracker.push(block(i, &format!("0x{i}"), &prev)).unwrap();
        }
        assert_eq!(tracker.head().unwrap().number, 110);
        tracker.rewind_to(105);
        assert_eq!(tracker.head().unwrap().number, 105);
    }

    #[test]
    fn window_size_enforced() {
        let mut tracker = BlockTracker::new(5);
        for i in 0..10 {
            let prev = if i == 0 { "0x0".to_string() } else { format!("0x{}", i - 1) };
            tracker.push(block(i, &format!("0x{i}"), &prev)).unwrap();
        }
        assert_eq!(tracker.len(), 5); // oldest blocks evicted
    }
}
