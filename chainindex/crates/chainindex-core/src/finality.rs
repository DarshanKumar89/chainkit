//! Chain-specific finality models.
//!
//! Different blockchains have different finality guarantees. This module
//! provides a registry of finality parameters keyed by chain name.

use std::collections::HashMap;
use std::time::Duration;

/// Finality model for a blockchain.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FinalityConfig {
    /// Human-readable chain name (e.g. "ethereum", "polygon", "arbitrum").
    pub chain: String,
    /// Number of confirmations for "safe" status.
    pub safe_confirmations: u64,
    /// Number of confirmations for "finalized" status.
    pub finalized_confirmations: u64,
    /// Expected block/slot time.
    pub block_time: Duration,
    /// Recommended reorg window size (blocks to keep in tracker).
    pub reorg_window: usize,
    /// Whether this chain can have empty/skipped slots (e.g. Solana).
    pub allows_slot_skipping: bool,
    /// Whether the chain has a separate sequencer (L2s).
    pub has_sequencer: bool,
    /// Settlement layer chain name, if this is an L2 (e.g. "ethereum" for Arbitrum).
    pub settlement_chain: Option<String>,
}

impl FinalityConfig {
    /// Time to finality at the "safe" level.
    pub fn safe_time(&self) -> Duration {
        self.block_time * self.safe_confirmations as u32
    }

    /// Time to finality at the "finalized" level.
    pub fn finalized_time(&self) -> Duration {
        self.block_time * self.finalized_confirmations as u32
    }
}

/// Registry of known chain finality configurations.
pub struct FinalityRegistry {
    configs: HashMap<String, FinalityConfig>,
}

impl FinalityRegistry {
    /// Create a registry pre-populated with known chains.
    pub fn new() -> Self {
        let mut configs = HashMap::new();

        // Ethereum (PoS)
        configs.insert(
            "ethereum".into(),
            FinalityConfig {
                chain: "ethereum".into(),
                safe_confirmations: 32,      // ~6.4 min
                finalized_confirmations: 64, // ~12.8 min (2 epochs)
                block_time: Duration::from_secs(12),
                reorg_window: 128,
                allows_slot_skipping: false,
                has_sequencer: false,
                settlement_chain: None,
            },
        );

        // Polygon PoS
        configs.insert(
            "polygon".into(),
            FinalityConfig {
                chain: "polygon".into(),
                safe_confirmations: 128,
                finalized_confirmations: 256,
                block_time: Duration::from_secs(2),
                reorg_window: 512,
                allows_slot_skipping: false,
                has_sequencer: false,
                settlement_chain: Some("ethereum".into()),
            },
        );

        // Arbitrum One
        configs.insert(
            "arbitrum".into(),
            FinalityConfig {
                chain: "arbitrum".into(),
                safe_confirmations: 0, // Sequencer provides instant soft finality
                finalized_confirmations: 1, // After batch posted to L1
                block_time: Duration::from_millis(250),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        // Optimism / Base
        configs.insert(
            "optimism".into(),
            FinalityConfig {
                chain: "optimism".into(),
                safe_confirmations: 0,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(2),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        configs.insert(
            "base".into(),
            FinalityConfig {
                chain: "base".into(),
                safe_confirmations: 0,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(2),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        // BSC (BNB Smart Chain)
        configs.insert(
            "bsc".into(),
            FinalityConfig {
                chain: "bsc".into(),
                safe_confirmations: 15,
                finalized_confirmations: 15,
                block_time: Duration::from_secs(3),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: false,
                settlement_chain: None,
            },
        );

        // Avalanche C-Chain
        configs.insert(
            "avalanche".into(),
            FinalityConfig {
                chain: "avalanche".into(),
                safe_confirmations: 1, // Avalanche has instant finality
                finalized_confirmations: 1,
                block_time: Duration::from_secs(2),
                reorg_window: 32,
                allows_slot_skipping: false,
                has_sequencer: false,
                settlement_chain: None,
            },
        );

        // Solana
        configs.insert(
            "solana".into(),
            FinalityConfig {
                chain: "solana".into(),
                safe_confirmations: 1,       // "confirmed" = 66% voted
                finalized_confirmations: 32, // "finalized" = 31+ confirmations
                block_time: Duration::from_millis(400),
                reorg_window: 256,
                allows_slot_skipping: true,
                has_sequencer: false,
                settlement_chain: None,
            },
        );

        // Fantom
        configs.insert(
            "fantom".into(),
            FinalityConfig {
                chain: "fantom".into(),
                safe_confirmations: 1,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(1),
                reorg_window: 32,
                allows_slot_skipping: false,
                has_sequencer: false,
                settlement_chain: None,
            },
        );

        // Scroll
        configs.insert(
            "scroll".into(),
            FinalityConfig {
                chain: "scroll".into(),
                safe_confirmations: 0,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(3),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        // zkSync Era
        configs.insert(
            "zksync".into(),
            FinalityConfig {
                chain: "zksync".into(),
                safe_confirmations: 0,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(1),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        // Linea
        configs.insert(
            "linea".into(),
            FinalityConfig {
                chain: "linea".into(),
                safe_confirmations: 0,
                finalized_confirmations: 1,
                block_time: Duration::from_secs(12),
                reorg_window: 64,
                allows_slot_skipping: false,
                has_sequencer: true,
                settlement_chain: Some("ethereum".into()),
            },
        );

        Self { configs }
    }

    /// Look up finality config by chain name.
    pub fn get(&self, chain: &str) -> Option<&FinalityConfig> {
        self.configs.get(chain)
    }

    /// Register a custom chain finality config.
    pub fn register(&mut self, config: FinalityConfig) {
        self.configs.insert(config.chain.clone(), config);
    }

    /// Get all known chain names.
    pub fn chains(&self) -> Vec<&str> {
        self.configs.keys().map(|s| s.as_str()).collect()
    }

    /// Get recommended confirmation depth for a chain.
    /// Falls back to 12 if chain is unknown.
    pub fn confirmation_depth(&self, chain: &str) -> u64 {
        self.configs
            .get(chain)
            .map(|c| c.finalized_confirmations)
            .unwrap_or(12)
    }

    /// Get recommended reorg window size for a chain.
    /// Falls back to 128 if chain is unknown.
    pub fn reorg_window(&self, chain: &str) -> usize {
        self.configs
            .get(chain)
            .map(|c| c.reorg_window)
            .unwrap_or(128)
    }

    /// Check if a chain is an L2 (has a settlement layer).
    pub fn is_l2(&self, chain: &str) -> bool {
        self.configs
            .get(chain)
            .map(|c| c.settlement_chain.is_some())
            .unwrap_or(false)
    }
}

impl Default for FinalityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_chains() {
        let reg = FinalityRegistry::new();

        // Ethereum
        let eth = reg.get("ethereum").expect("ethereum must exist");
        assert_eq!(eth.safe_confirmations, 32);
        assert_eq!(eth.finalized_confirmations, 64);
        assert_eq!(eth.block_time, Duration::from_secs(12));
        assert_eq!(eth.reorg_window, 128);
        assert!(!eth.allows_slot_skipping);
        assert!(!eth.has_sequencer);
        assert!(eth.settlement_chain.is_none());

        // Polygon
        let poly = reg.get("polygon").expect("polygon must exist");
        assert_eq!(poly.safe_confirmations, 128);
        assert_eq!(poly.finalized_confirmations, 256);
        assert_eq!(poly.block_time, Duration::from_secs(2));
        assert_eq!(poly.settlement_chain.as_deref(), Some("ethereum"));

        // Arbitrum
        let arb = reg.get("arbitrum").expect("arbitrum must exist");
        assert_eq!(arb.safe_confirmations, 0);
        assert_eq!(arb.finalized_confirmations, 1);
        assert!(arb.has_sequencer);
        assert_eq!(arb.settlement_chain.as_deref(), Some("ethereum"));

        // Solana
        let sol = reg.get("solana").expect("solana must exist");
        assert_eq!(sol.safe_confirmations, 1);
        assert_eq!(sol.finalized_confirmations, 32);
        assert!(sol.allows_slot_skipping);
        assert_eq!(sol.block_time, Duration::from_millis(400));
    }

    #[test]
    fn confirmation_depth_known() {
        let reg = FinalityRegistry::new();
        assert_eq!(reg.confirmation_depth("ethereum"), 64);
    }

    #[test]
    fn confirmation_depth_unknown() {
        let reg = FinalityRegistry::new();
        assert_eq!(reg.confirmation_depth("unknown_chain"), 12);
    }

    #[test]
    fn custom_chain() {
        let mut reg = FinalityRegistry::new();
        reg.register(FinalityConfig {
            chain: "mychain".into(),
            safe_confirmations: 5,
            finalized_confirmations: 10,
            block_time: Duration::from_secs(6),
            reorg_window: 50,
            allows_slot_skipping: false,
            has_sequencer: false,
            settlement_chain: None,
        });
        let cfg = reg.get("mychain").expect("custom chain must exist");
        assert_eq!(cfg.safe_confirmations, 5);
        assert_eq!(cfg.finalized_confirmations, 10);
        assert_eq!(cfg.block_time, Duration::from_secs(6));
        assert_eq!(cfg.reorg_window, 50);
    }

    #[test]
    fn is_l2() {
        let reg = FinalityRegistry::new();
        assert!(reg.is_l2("arbitrum"));
        assert!(reg.is_l2("optimism"));
        assert!(reg.is_l2("base"));
        assert!(!reg.is_l2("ethereum"));
        assert!(!reg.is_l2("solana"));
        assert!(!reg.is_l2("unknown"));
    }

    #[test]
    fn safe_time_calculation() {
        let reg = FinalityRegistry::new();

        // Ethereum: 32 confirmations * 12s = 384s
        let eth = reg.get("ethereum").unwrap();
        assert_eq!(eth.safe_time(), Duration::from_secs(32 * 12));

        // Ethereum finalized: 64 * 12s = 768s
        assert_eq!(eth.finalized_time(), Duration::from_secs(64 * 12));

        // Arbitrum safe: 0 * 250ms = 0
        let arb = reg.get("arbitrum").unwrap();
        assert_eq!(arb.safe_time(), Duration::ZERO);

        // Solana safe: 1 * 400ms = 400ms
        let sol = reg.get("solana").unwrap();
        assert_eq!(sol.safe_time(), Duration::from_millis(400));
    }
}
