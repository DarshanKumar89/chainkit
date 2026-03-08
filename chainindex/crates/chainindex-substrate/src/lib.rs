//! Substrate (Polkadot/Kusama) indexer for ChainIndex.
//!
//! Substrate chains use SCALE-encoded extrinsics and events. This module maps
//! Substrate blocks to [`BlockSummary`] and pallet events to [`DecodedEvent`].
//!
//! # Key Types
//!
//! - [`SubstrateBlock`] — Block with extrinsics and validator info
//! - [`SubstrateEvent`] — Pallet event with decoded fields
//! - [`SubstrateRpcClient`] — Trait abstracting Substrate JSON-RPC calls
//! - [`SubstrateIndexerBuilder`] — Fluent builder for Substrate indexer configs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter};

// ---------------------------------------------------------------------------
// Block type
// ---------------------------------------------------------------------------

/// A Substrate block with chain-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstrateBlock {
    pub height: u64,
    /// Block hash (hex with 0x prefix).
    pub hash: String,
    pub parent_hash: String,
    /// State root hash.
    pub state_root: String,
    /// Extrinsics root hash.
    pub extrinsics_root: String,
    pub timestamp: i64,
    /// Number of extrinsics (transactions) in the block.
    pub extrinsic_count: u32,
    /// Validator who authored the block (if known).
    pub author: Option<String>,
    /// Spec version of the runtime.
    pub spec_version: Option<u32>,
}

impl SubstrateBlock {
    pub fn to_block_summary(&self) -> BlockSummary {
        BlockSummary {
            number: self.height,
            hash: self.hash.clone(),
            parent_hash: self.parent_hash.clone(),
            timestamp: self.timestamp,
            tx_count: self.extrinsic_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Event type
// ---------------------------------------------------------------------------

/// A Substrate pallet event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubstrateEvent {
    /// Pallet name (e.g., "Balances", "System", "Staking").
    pub pallet: String,
    /// Event variant name (e.g., "Transfer", "ExtrinsicSuccess").
    pub variant: String,
    /// Decoded event fields as JSON.
    pub fields: serde_json::Value,
    /// Block height.
    pub height: u64,
    /// Event index within the block.
    pub event_index: u32,
    /// Extrinsic index that generated this event (if applicable).
    pub extrinsic_index: Option<u32>,
    /// Event phase (ApplyExtrinsic, Initialization, Finalization).
    pub phase: String,
}

impl SubstrateEvent {
    /// Full event name in `Pallet.Variant` format.
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.pallet, self.variant)
    }

    pub fn to_decoded_event(&self, chain: &str) -> DecodedEvent {
        DecodedEvent {
            chain: chain.to_string(),
            schema: self.full_name(),
            address: self.pallet.clone(),
            tx_hash: format!("extrinsic:{}", self.extrinsic_index.unwrap_or(0)),
            block_number: self.height,
            log_index: self.event_index,
            fields_json: self.fields.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SubstrateRpcClient: Send + Sync {
    /// Get the finalized head block hash.
    async fn get_finalized_head(&self) -> Result<String, IndexerError>;

    /// Get block header at hash, returns block number.
    async fn get_header(&self, hash: Option<&str>) -> Result<u64, IndexerError>;

    /// Get full block at a given height.
    async fn get_block(&self, height: u64) -> Result<Option<SubstrateBlock>, IndexerError>;

    /// Get events for a block at a given height.
    async fn get_events(&self, height: u64) -> Result<Vec<SubstrateEvent>, IndexerError>;
}

// ---------------------------------------------------------------------------
// Event filter
// ---------------------------------------------------------------------------

/// Substrate-specific event filter.
#[derive(Debug, Clone, Default)]
pub struct SubstrateEventFilter {
    /// Filter by pallet name (e.g., "Balances", "Staking").
    pub pallets: Vec<String>,
    /// Filter by event variant (e.g., "Transfer").
    pub variants: Vec<String>,
    /// Exclude system events (ExtrinsicSuccess, ExtrinsicFailed).
    pub exclude_system: bool,
}

impl SubstrateEventFilter {
    pub fn matches(&self, event: &SubstrateEvent) -> bool {
        if self.exclude_system && event.pallet == "System" {
            return false;
        }

        if !self.pallets.is_empty()
            && !self.pallets.iter().any(|p| p == &event.pallet)
        {
            return false;
        }

        if !self.variants.is_empty()
            && !self.variants.iter().any(|v| v == &event.variant)
        {
            return false;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Event parser
// ---------------------------------------------------------------------------

/// Parses Substrate JSON-RPC responses into typed events and blocks.
pub struct SubstrateEventParser;

impl SubstrateEventParser {
    /// Parse a block from `chain_getBlock` JSON response.
    pub fn parse_block(json: &serde_json::Value, height: u64) -> Option<SubstrateBlock> {
        let block = json.get("block").or_else(|| json.get("result").and_then(|r| r.get("block")))?;
        let header = block.get("header")?;

        let hash = header.get("hash")
            .or_else(|| json.get("hash"))
            .and_then(|h| h.as_str())
            .unwrap_or_default()
            .to_string();
        let parent_hash = header["parentHash"].as_str().unwrap_or_default().to_string();
        let state_root = header["stateRoot"].as_str().unwrap_or_default().to_string();
        let extrinsics_root = header["extrinsicsRoot"].as_str().unwrap_or_default().to_string();

        let extrinsic_count = block["extrinsics"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        Some(SubstrateBlock {
            height,
            hash,
            parent_hash,
            state_root,
            extrinsics_root,
            timestamp: 0, // Substrate doesn't include timestamp in header
            extrinsic_count,
            author: None,
            spec_version: None,
        })
    }

    /// Parse events from `state_getStorage` decoded events JSON.
    pub fn parse_events(json: &serde_json::Value, height: u64) -> Vec<SubstrateEvent> {
        let events_array = json.as_array()
            .or_else(|| json.get("result").and_then(|r| r.as_array()));

        let Some(events) = events_array else {
            return Vec::new();
        };

        events
            .iter()
            .enumerate()
            .filter_map(|(idx, ev)| {
                let pallet = ev["pallet"].as_str()
                    .or_else(|| ev["section"].as_str())?;
                let variant = ev["method"].as_str()
                    .or_else(|| ev["variant"].as_str())?;
                let phase = ev["phase"].as_str().unwrap_or("ApplyExtrinsic");
                let extrinsic_index = ev["extrinsicIndex"].as_u64().map(|n| n as u32);

                let fields = ev.get("data")
                    .or_else(|| ev.get("fields"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                Some(SubstrateEvent {
                    pallet: pallet.to_string(),
                    variant: variant.to_string(),
                    fields,
                    height,
                    event_index: idx as u32,
                    extrinsic_index,
                    phase: phase.to_string(),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct SubstrateIndexerBuilder {
    from_height: u64,
    to_height: Option<u64>,
    pallets: Vec<String>,
    variants: Vec<String>,
    exclude_system: bool,
    batch_size: u64,
    poll_interval_ms: u64,
    checkpoint_interval: u64,
    confirmation_depth: u64,
    id: String,
    chain: String,
}

impl SubstrateIndexerBuilder {
    pub fn new() -> Self {
        Self {
            from_height: 1,
            to_height: None,
            pallets: Vec::new(),
            variants: Vec::new(),
            exclude_system: false,
            batch_size: 100,
            poll_interval_ms: 6000,
            checkpoint_interval: 100,
            confirmation_depth: 1, // GRANDPA finality
            id: "substrate-indexer".into(),
            chain: "polkadot".into(),
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn chain(mut self, chain: impl Into<String>) -> Self {
        self.chain = chain.into();
        self
    }

    pub fn from_height(mut self, height: u64) -> Self {
        self.from_height = height;
        self
    }

    pub fn to_height(mut self, height: u64) -> Self {
        self.to_height = Some(height);
        self
    }

    pub fn pallet(mut self, pallet: impl Into<String>) -> Self {
        self.pallets.push(pallet.into());
        self
    }

    pub fn variant(mut self, variant: impl Into<String>) -> Self {
        self.variants.push(variant.into());
        self
    }

    pub fn exclude_system(mut self, exclude: bool) -> Self {
        self.exclude_system = exclude;
        self
    }

    pub fn batch_size(mut self, size: u64) -> Self {
        self.batch_size = size;
        self
    }

    pub fn poll_interval_ms(mut self, ms: u64) -> Self {
        self.poll_interval_ms = ms;
        self
    }

    pub fn build_config(&self) -> IndexerConfig {
        IndexerConfig {
            id: self.id.clone(),
            chain: self.chain.clone(),
            from_block: self.from_height,
            to_block: self.to_height,
            confirmation_depth: self.confirmation_depth,
            batch_size: self.batch_size,
            checkpoint_interval: self.checkpoint_interval,
            poll_interval_ms: self.poll_interval_ms,
            filter: EventFilter {
                addresses: self.pallets.clone(),
                topic0_values: self.variants.clone(),
                from_block: Some(self.from_height),
                to_block: self.to_height,
            },
        }
    }

    pub fn build_filter(&self) -> SubstrateEventFilter {
        SubstrateEventFilter {
            pallets: self.pallets.clone(),
            variants: self.variants.clone(),
            exclude_system: self.exclude_system,
        }
    }
}

impl Default for SubstrateIndexerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_to_summary() {
        let block = SubstrateBlock {
            height: 20_000_000,
            hash: "0xblock".into(),
            parent_hash: "0xparent".into(),
            state_root: "0xstate".into(),
            extrinsics_root: "0xext".into(),
            timestamp: 1700000000,
            extrinsic_count: 5,
            author: Some("validator1".into()),
            spec_version: Some(1001000),
        };
        let summary = block.to_block_summary();
        assert_eq!(summary.number, 20_000_000);
        assert_eq!(summary.hash, "0xblock");
        assert_eq!(summary.tx_count, 5);
    }

    #[test]
    fn event_full_name() {
        let event = SubstrateEvent {
            pallet: "Balances".into(),
            variant: "Transfer".into(),
            fields: serde_json::json!({"from": "alice", "to": "bob", "amount": 1000}),
            height: 100,
            event_index: 0,
            extrinsic_index: Some(1),
            phase: "ApplyExtrinsic".into(),
        };
        assert_eq!(event.full_name(), "Balances.Transfer");
    }

    #[test]
    fn event_to_decoded() {
        let event = SubstrateEvent {
            pallet: "Balances".into(),
            variant: "Transfer".into(),
            fields: serde_json::json!({"from": "alice", "to": "bob", "amount": 1000}),
            height: 100,
            event_index: 3,
            extrinsic_index: Some(1),
            phase: "ApplyExtrinsic".into(),
        };
        let decoded = event.to_decoded_event("polkadot");
        assert_eq!(decoded.chain, "polkadot");
        assert_eq!(decoded.schema, "Balances.Transfer");
        assert_eq!(decoded.address, "Balances");
        assert_eq!(decoded.block_number, 100);
        assert_eq!(decoded.log_index, 3);
    }

    #[test]
    fn filter_pallet() {
        let filter = SubstrateEventFilter {
            pallets: vec!["Balances".into()],
            ..Default::default()
        };
        let event = SubstrateEvent {
            pallet: "Balances".into(),
            variant: "Transfer".into(),
            fields: serde_json::Value::Null,
            height: 1,
            event_index: 0,
            extrinsic_index: None,
            phase: "ApplyExtrinsic".into(),
        };
        assert!(filter.matches(&event));

        let other = SubstrateEvent {
            pallet: "Staking".into(),
            variant: "Reward".into(),
            fields: serde_json::Value::Null,
            height: 1,
            event_index: 0,
            extrinsic_index: None,
            phase: "ApplyExtrinsic".into(),
        };
        assert!(!filter.matches(&other));
    }

    #[test]
    fn filter_exclude_system() {
        let filter = SubstrateEventFilter {
            exclude_system: true,
            ..Default::default()
        };
        let system_event = SubstrateEvent {
            pallet: "System".into(),
            variant: "ExtrinsicSuccess".into(),
            fields: serde_json::Value::Null,
            height: 1,
            event_index: 0,
            extrinsic_index: None,
            phase: "ApplyExtrinsic".into(),
        };
        assert!(!filter.matches(&system_event));

        let balances = SubstrateEvent {
            pallet: "Balances".into(),
            variant: "Transfer".into(),
            fields: serde_json::Value::Null,
            height: 1,
            event_index: 0,
            extrinsic_index: None,
            phase: "ApplyExtrinsic".into(),
        };
        assert!(filter.matches(&balances));
    }

    #[test]
    fn filter_empty_matches_all() {
        let filter = SubstrateEventFilter::default();
        let event = SubstrateEvent {
            pallet: "Anything".into(),
            variant: "Whatever".into(),
            fields: serde_json::Value::Null,
            height: 1,
            event_index: 0,
            extrinsic_index: None,
            phase: "ApplyExtrinsic".into(),
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn builder_defaults() {
        let config = SubstrateIndexerBuilder::new().build_config();
        assert_eq!(config.chain, "polkadot");
        assert_eq!(config.from_block, 1);
        assert_eq!(config.confirmation_depth, 1);
    }

    #[test]
    fn builder_custom() {
        let builder = SubstrateIndexerBuilder::new()
            .id("dot-idx")
            .chain("kusama")
            .from_height(1_000_000)
            .to_height(2_000_000)
            .pallet("Balances")
            .variant("Transfer")
            .exclude_system(true)
            .batch_size(50);

        let config = builder.build_config();
        assert_eq!(config.id, "dot-idx");
        assert_eq!(config.chain, "kusama");
        assert_eq!(config.from_block, 1_000_000);
        assert_eq!(config.to_block, Some(2_000_000));

        let filter = builder.build_filter();
        assert!(filter.exclude_system);
        assert_eq!(filter.pallets, vec!["Balances"]);
        assert_eq!(filter.variants, vec!["Transfer"]);
    }

    #[test]
    fn parse_events_json() {
        let json = serde_json::json!([
            {
                "pallet": "Balances",
                "method": "Transfer",
                "phase": "ApplyExtrinsic",
                "extrinsicIndex": 1,
                "data": { "from": "alice", "to": "bob", "amount": "1000000000000" }
            },
            {
                "pallet": "System",
                "method": "ExtrinsicSuccess",
                "phase": "ApplyExtrinsic",
                "extrinsicIndex": 1,
                "data": { "dispatchInfo": { "weight": 123 } }
            }
        ]);
        let events = SubstrateEventParser::parse_events(&json, 500);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].pallet, "Balances");
        assert_eq!(events[0].variant, "Transfer");
        assert_eq!(events[0].extrinsic_index, Some(1));
        assert_eq!(events[1].pallet, "System");
    }

    #[test]
    fn parse_block_json() {
        let json = serde_json::json!({
            "block": {
                "header": {
                    "parentHash": "0xparent_hash",
                    "stateRoot": "0xstate_root",
                    "extrinsicsRoot": "0xext_root",
                    "number": "0x1312D00"
                },
                "extrinsics": ["ext1", "ext2", "ext3", "ext4"]
            }
        });
        let block = SubstrateEventParser::parse_block(&json, 20_000_000).unwrap();
        assert_eq!(block.height, 20_000_000);
        assert_eq!(block.parent_hash, "0xparent_hash");
        assert_eq!(block.extrinsic_count, 4);
    }

    #[test]
    fn block_serializable() {
        let block = SubstrateBlock {
            height: 100,
            hash: "0xh".into(),
            parent_hash: "0xp".into(),
            state_root: "0xs".into(),
            extrinsics_root: "0xe".into(),
            timestamp: 1000,
            extrinsic_count: 3,
            author: None,
            spec_version: Some(1001000),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: SubstrateBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.height, 100);
        assert_eq!(back.spec_version, Some(1001000));
    }

    #[test]
    fn event_serializable() {
        let event = SubstrateEvent {
            pallet: "Balances".into(),
            variant: "Transfer".into(),
            fields: serde_json::json!({}),
            height: 100,
            event_index: 0,
            extrinsic_index: Some(1),
            phase: "ApplyExtrinsic".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: SubstrateEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.full_name(), "Balances.Transfer");
    }
}
