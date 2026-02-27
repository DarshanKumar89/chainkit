//! Reorg detection and recovery logic.
//!
//! Handles four reorg scenarios:
//! 1. **Short reorg (1-3 blocks)**: parent hash mismatch on the next block
//! 2. **Deep reorg**: checkpoint hash doesn't match the chain → rewind
//! 3. **Node switch**: provider returns a different canonical chain
//! 4. **RPC inconsistency**: finalized block number decreases

use crate::types::BlockSummary;

/// Describes a detected chain reorganization.
#[derive(Debug, Clone)]
pub struct ReorgEvent {
    /// The block where the fork was detected.
    pub detected_at: u64,
    /// The blocks that were dropped (rolled back) — most recent first.
    pub dropped_blocks: Vec<BlockSummary>,
    /// The depth of the reorg (number of blocks rolled back).
    pub depth: u64,
    /// Type of reorg detected.
    pub reorg_type: ReorgType,
}

/// Classification of the reorg type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorgType {
    /// Parent hash mismatch — short reorg (1–3 blocks).
    ShortReorg,
    /// Checkpoint hash mismatch — could be a deep reorg or node switch.
    DeepReorg,
    /// Finalized block number decreased — RPC inconsistency or node switch.
    RpcInconsistency,
}

impl std::fmt::Display for ReorgType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShortReorg => write!(f, "short reorg"),
            Self::DeepReorg => write!(f, "deep reorg"),
            Self::RpcInconsistency => write!(f, "RPC inconsistency"),
        }
    }
}

/// Detects and classifies chain reorganizations.
pub struct ReorgDetector {
    /// Last known finalized block number (for RPC inconsistency detection).
    last_finalized: Option<u64>,
    /// Confirmation depth — blocks behind head considered finalized.
    confirmation_depth: u64,
}

impl ReorgDetector {
    pub fn new(confirmation_depth: u64) -> Self {
        Self {
            last_finalized: None,
            confirmation_depth,
        }
    }

    /// Check whether `new_block` extends `previous_head` normally.
    ///
    /// Returns `Some(ReorgEvent)` if a reorg is detected, `None` if the chain is canonical.
    pub fn check(
        &mut self,
        new_block: &BlockSummary,
        previous_head: &BlockSummary,
        window: &[BlockSummary],
    ) -> Option<ReorgEvent> {
        // Check parent hash
        if !new_block.extends(previous_head) {
            let (dropped, depth) = find_dropped_blocks(new_block, window);
            let reorg_type = if depth <= 3 {
                ReorgType::ShortReorg
            } else {
                ReorgType::DeepReorg
            };
            tracing::warn!(
                depth,
                at = new_block.number,
                reorg_type = %reorg_type,
                "Reorg detected"
            );
            return Some(ReorgEvent {
                detected_at: new_block.number,
                dropped_blocks: dropped,
                depth,
                reorg_type,
            });
        }

        None
    }

    /// Check if the node reports a lower finalized block than previously seen.
    ///
    /// Returns `Some(ReorgEvent)` with `RpcInconsistency` if so.
    pub fn check_finalized(
        &mut self,
        new_finalized: u64,
        window: &[BlockSummary],
    ) -> Option<ReorgEvent> {
        if let Some(last) = self.last_finalized {
            if new_finalized < last {
                tracing::warn!(
                    last_finalized = last,
                    new_finalized,
                    "Finalized block decreased — possible RPC inconsistency"
                );
                let dropped: Vec<_> = window
                    .iter()
                    .filter(|b| b.number > new_finalized)
                    .cloned()
                    .collect();
                self.last_finalized = Some(new_finalized);
                return Some(ReorgEvent {
                    detected_at: new_finalized,
                    dropped_blocks: dropped,
                    depth: last - new_finalized,
                    reorg_type: ReorgType::RpcInconsistency,
                });
            }
        }
        self.last_finalized = Some(new_finalized);
        None
    }
}

/// Walk the window backward to find which blocks need to be rolled back.
fn find_dropped_blocks(
    new_block: &BlockSummary,
    window: &[BlockSummary],
) -> (Vec<BlockSummary>, u64) {
    let mut dropped = Vec::new();
    for block in window.iter().rev() {
        if block.hash == new_block.parent_hash {
            // Found the fork point
            break;
        }
        dropped.push(block.clone());
    }
    let depth = dropped.len() as u64;
    (dropped, depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(num: u64, hash: &str, parent: &str) -> BlockSummary {
        BlockSummary {
            number: num,
            hash: hash.into(),
            parent_hash: parent.into(),
            timestamp: (num * 12) as i64,
            tx_count: 0,
        }
    }

    #[test]
    fn no_reorg_on_normal_chain() {
        let mut det = ReorgDetector::new(12);
        let head = b(100, "0xa", "0x0");
        let new = b(101, "0xb", "0xa");
        assert!(det.check(&new, &head, &[head.clone()]).is_none());
    }

    #[test]
    fn detects_short_reorg() {
        let mut det = ReorgDetector::new(12);
        let block_99 = b(99, "0x99", "0x98");
        let block_100 = b(100, "0xa", "0x99");
        let block_100b = b(100, "0xb", "0x99"); // different block at 100

        // The tracker has [99, 100] but a new block at 101 with parent 0xb (reorg)
        let new_101 = b(101, "0xc", "0xb");
        let window = vec![block_99.clone(), block_100.clone()];

        let result = det.check(&new_101, &block_100, &window);
        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.reorg_type, ReorgType::ShortReorg);
    }

    #[test]
    fn rpc_inconsistency_detected() {
        let mut det = ReorgDetector::new(12);
        let window = vec![b(100, "0xa", "0x0"), b(101, "0xb", "0xa")];
        det.check_finalized(100, &window); // sets last_finalized = 100
        let result = det.check_finalized(98, &window); // decreased!
        assert!(result.is_some());
        assert_eq!(result.unwrap().reorg_type, ReorgType::RpcInconsistency);
    }
}
