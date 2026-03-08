//! Aptos indexer for ChainIndex.
//!
//! Aptos uses Move-based events and resources. This module maps Aptos blocks
//! to [`BlockSummary`] and Move events to [`DecodedEvent`].
//!
//! # Key Types
//!
//! - [`AptosBlock`] — Aptos block with version range and epoch
//! - [`AptosEvent`] — Move event with type tag and data
//! - [`AptosRpcClient`] — Trait abstracting Aptos REST API calls
//! - [`AptosIndexerBuilder`] — Fluent builder for Aptos indexer configs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter};

// ---------------------------------------------------------------------------
// Block type
// ---------------------------------------------------------------------------

/// An Aptos block with chain-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AptosBlock {
    /// Block height.
    pub height: u64,
    /// Block hash.
    pub hash: String,
    /// Timestamp in unix seconds.
    pub timestamp: i64,
    /// First transaction version in this block.
    pub first_version: u64,
    /// Last transaction version in this block.
    pub last_version: u64,
    /// Number of transactions.
    pub tx_count: u32,
    /// Current epoch.
    pub epoch: u64,
    /// Round within the epoch.
    pub round: u64,
}

impl AptosBlock {
    pub fn to_block_summary(&self) -> BlockSummary {
        BlockSummary {
            number: self.height,
            hash: self.hash.clone(),
            parent_hash: format!("version:{}", self.first_version),
            timestamp: self.timestamp,
            tx_count: self.tx_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Event type
// ---------------------------------------------------------------------------

/// An Aptos Move event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AptosEvent {
    /// Move type tag (e.g., "0x1::coin::DepositEvent").
    pub type_tag: String,
    /// Event sequence number.
    pub sequence_number: u64,
    /// Decoded event data as JSON.
    pub data: serde_json::Value,
    /// Transaction version.
    pub version: u64,
    /// Block height.
    pub height: u64,
    /// Transaction hash.
    pub tx_hash: String,
    /// Account address that emitted the event.
    pub account_address: String,
    /// Creation number (identifies the event handle).
    pub creation_number: u64,
}

impl AptosEvent {
    /// Extract the module name from the type tag.
    ///
    /// For `0x1::coin::DepositEvent`, returns `coin`.
    pub fn module_name(&self) -> &str {
        self.type_tag
            .split("::")
            .nth(1)
            .unwrap_or("unknown")
    }

    /// Extract the event name from the type tag.
    ///
    /// For `0x1::coin::DepositEvent`, returns `DepositEvent`.
    pub fn event_name(&self) -> &str {
        self.type_tag
            .split("::")
            .nth(2)
            .unwrap_or("unknown")
    }

    /// Extract the address from the type tag.
    ///
    /// For `0x1::coin::DepositEvent`, returns `0x1`.
    pub fn type_address(&self) -> &str {
        self.type_tag
            .split("::")
            .next()
            .unwrap_or("0x0")
    }

    pub fn to_decoded_event(&self, chain: &str) -> DecodedEvent {
        let schema = format!("{}::{}", self.module_name(), self.event_name());

        DecodedEvent {
            chain: chain.to_string(),
            schema,
            address: self.account_address.clone(),
            tx_hash: self.tx_hash.clone(),
            block_number: self.height,
            log_index: self.sequence_number as u32,
            fields_json: self.data.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait AptosRpcClient: Send + Sync {
    /// Get current ledger info (latest block height).
    async fn get_ledger_info(&self) -> Result<u64, IndexerError>;

    /// Get block by height.
    async fn get_block_by_height(
        &self,
        height: u64,
    ) -> Result<Option<AptosBlock>, IndexerError>;

    /// Get events for a given event handle.
    async fn get_events(
        &self,
        account: &str,
        event_handle: &str,
        field_name: &str,
        start: u64,
        limit: u64,
    ) -> Result<Vec<AptosEvent>, IndexerError>;

    /// Get events emitted in a transaction.
    async fn get_transaction_events(
        &self,
        version: u64,
    ) -> Result<Vec<AptosEvent>, IndexerError>;
}

// ---------------------------------------------------------------------------
// Event filter
// ---------------------------------------------------------------------------

/// Aptos-specific event filter.
#[derive(Debug, Clone, Default)]
pub struct AptosEventFilter {
    /// Filter by Move type tag prefix (e.g., "0x1::coin").
    pub type_prefixes: Vec<String>,
    /// Filter by module name.
    pub modules: Vec<String>,
    /// Filter by account address.
    pub accounts: Vec<String>,
}

impl AptosEventFilter {
    pub fn matches(&self, event: &AptosEvent) -> bool {
        if !self.type_prefixes.is_empty()
            && !self.type_prefixes.iter().any(|p| event.type_tag.starts_with(p))
        {
            return false;
        }

        if !self.modules.is_empty()
            && !self.modules.iter().any(|m| m == event.module_name())
        {
            return false;
        }

        if !self.accounts.is_empty()
            && !self.accounts.iter().any(|a| a == &event.account_address)
        {
            return false;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parses Aptos REST API JSON responses.
pub struct AptosResponseParser;

impl AptosResponseParser {
    /// Parse a block from Aptos REST API `GET /v1/blocks/by_height/{h}`.
    pub fn parse_block(json: &serde_json::Value) -> Option<AptosBlock> {
        let height_str = json["block_height"].as_str()?;
        let height = height_str.parse::<u64>().ok()?;

        let hash = json["block_hash"].as_str().unwrap_or_default().to_string();
        let timestamp_us = json["block_timestamp"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let timestamp = (timestamp_us / 1_000_000) as i64;

        let first_version = json["first_version"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let last_version = json["last_version"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let tx_count = json["transactions"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or_else(|| {
                if last_version >= first_version {
                    (last_version - first_version + 1) as u32
                } else {
                    0
                }
            });

        Some(AptosBlock {
            height,
            hash,
            timestamp,
            first_version,
            last_version,
            tx_count,
            epoch: json["epoch"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
            round: json["round"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }

    /// Parse events from an Aptos REST API events response.
    pub fn parse_events(
        json: &serde_json::Value,
        height: u64,
    ) -> Vec<AptosEvent> {
        let events_array = json.as_array();
        let Some(events) = events_array else {
            return Vec::new();
        };

        events
            .iter()
            .filter_map(|ev| {
                let type_tag = ev["type"].as_str()?.to_string();
                let sequence_number = ev["sequence_number"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let data = ev.get("data").cloned().unwrap_or(serde_json::Value::Null);
                let version = ev["version"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                Some(AptosEvent {
                    type_tag,
                    sequence_number,
                    data,
                    version,
                    height,
                    tx_hash: ev["transaction_hash"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    account_address: ev["guid"]["account_address"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    creation_number: ev["guid"]["creation_number"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct AptosIndexerBuilder {
    from_height: u64,
    to_height: Option<u64>,
    type_prefixes: Vec<String>,
    modules: Vec<String>,
    accounts: Vec<String>,
    batch_size: u64,
    poll_interval_ms: u64,
    checkpoint_interval: u64,
    confirmation_depth: u64,
    id: String,
    chain: String,
}

impl AptosIndexerBuilder {
    pub fn new() -> Self {
        Self {
            from_height: 0,
            to_height: None,
            type_prefixes: Vec::new(),
            modules: Vec::new(),
            accounts: Vec::new(),
            batch_size: 100,
            poll_interval_ms: 4000, // ~4s block time
            checkpoint_interval: 100,
            confirmation_depth: 1, // BFT instant finality
            id: "aptos-indexer".into(),
            chain: "aptos".into(),
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

    pub fn type_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.type_prefixes.push(prefix.into());
        self
    }

    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.modules.push(module.into());
        self
    }

    pub fn account(mut self, account: impl Into<String>) -> Self {
        self.accounts.push(account.into());
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
                addresses: self.accounts.clone(),
                topic0_values: self.type_prefixes.clone(),
                from_block: Some(self.from_height),
                to_block: self.to_height,
            },
        }
    }

    pub fn build_filter(&self) -> AptosEventFilter {
        AptosEventFilter {
            type_prefixes: self.type_prefixes.clone(),
            modules: self.modules.clone(),
            accounts: self.accounts.clone(),
        }
    }
}

impl Default for AptosIndexerBuilder {
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
        let block = AptosBlock {
            height: 150_000_000,
            hash: "0xabc".into(),
            timestamp: 1700000000,
            first_version: 500_000_000,
            last_version: 500_000_050,
            tx_count: 51,
            epoch: 100,
            round: 5,
        };
        let summary = block.to_block_summary();
        assert_eq!(summary.number, 150_000_000);
        assert_eq!(summary.hash, "0xabc");
        assert_eq!(summary.tx_count, 51);
        assert_eq!(summary.parent_hash, "version:500000000");
    }

    #[test]
    fn event_type_parsing() {
        let event = AptosEvent {
            type_tag: "0x1::coin::DepositEvent".into(),
            sequence_number: 42,
            data: serde_json::json!({"amount": "1000"}),
            version: 500_000_000,
            height: 150_000_000,
            tx_hash: "tx_hash_abc".into(),
            account_address: "0xaccount".into(),
            creation_number: 1,
        };
        assert_eq!(event.module_name(), "coin");
        assert_eq!(event.event_name(), "DepositEvent");
        assert_eq!(event.type_address(), "0x1");
    }

    #[test]
    fn event_to_decoded() {
        let event = AptosEvent {
            type_tag: "0x1::coin::DepositEvent".into(),
            sequence_number: 42,
            data: serde_json::json!({"amount": "1000"}),
            version: 500_000_000,
            height: 150_000_000,
            tx_hash: "tx_hash_abc".into(),
            account_address: "0xaccount".into(),
            creation_number: 1,
        };
        let decoded = event.to_decoded_event("aptos");
        assert_eq!(decoded.chain, "aptos");
        assert_eq!(decoded.schema, "coin::DepositEvent");
        assert_eq!(decoded.address, "0xaccount");
        assert_eq!(decoded.tx_hash, "tx_hash_abc");
        assert_eq!(decoded.log_index, 42);
        assert_eq!(decoded.fields_json["amount"], "1000");
    }

    #[test]
    fn filter_type_prefix() {
        let filter = AptosEventFilter {
            type_prefixes: vec!["0x1::coin".into()],
            ..Default::default()
        };
        let event = AptosEvent {
            type_tag: "0x1::coin::DepositEvent".into(),
            sequence_number: 0,
            data: serde_json::Value::Null,
            version: 0,
            height: 0,
            tx_hash: "".into(),
            account_address: "".into(),
            creation_number: 0,
        };
        assert!(filter.matches(&event));

        let other = AptosEvent {
            type_tag: "0x1::staking::StakeEvent".into(),
            sequence_number: 0,
            data: serde_json::Value::Null,
            version: 0,
            height: 0,
            tx_hash: "".into(),
            account_address: "".into(),
            creation_number: 0,
        };
        assert!(!filter.matches(&other));
    }

    #[test]
    fn filter_module() {
        let filter = AptosEventFilter {
            modules: vec!["coin".into()],
            ..Default::default()
        };
        let event = AptosEvent {
            type_tag: "0x1::coin::DepositEvent".into(),
            sequence_number: 0,
            data: serde_json::Value::Null,
            version: 0,
            height: 0,
            tx_hash: "".into(),
            account_address: "".into(),
            creation_number: 0,
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn filter_empty_matches_all() {
        let filter = AptosEventFilter::default();
        let event = AptosEvent {
            type_tag: "anything".into(),
            sequence_number: 0,
            data: serde_json::Value::Null,
            version: 0,
            height: 0,
            tx_hash: "".into(),
            account_address: "".into(),
            creation_number: 0,
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn parse_block_json() {
        let json = serde_json::json!({
            "block_height": "150000000",
            "block_hash": "0xblock_hash_abc",
            "block_timestamp": "1700000000000000",
            "first_version": "500000000",
            "last_version": "500000050",
            "epoch": "100",
            "round": "5",
            "transactions": [{"type": "user"}, {"type": "user"}]
        });
        let block = AptosResponseParser::parse_block(&json).unwrap();
        assert_eq!(block.height, 150_000_000);
        assert_eq!(block.hash, "0xblock_hash_abc");
        assert_eq!(block.timestamp, 1700000000);
        assert_eq!(block.first_version, 500_000_000);
        assert_eq!(block.last_version, 500_000_050);
        assert_eq!(block.tx_count, 2);
        assert_eq!(block.epoch, 100);
    }

    #[test]
    fn parse_events_json() {
        let json = serde_json::json!([
            {
                "type": "0x1::coin::DepositEvent",
                "sequence_number": "42",
                "data": { "amount": "1000" },
                "version": "500000000",
                "transaction_hash": "tx_abc",
                "guid": {
                    "account_address": "0xaccount",
                    "creation_number": "1"
                }
            }
        ]);
        let events = AptosResponseParser::parse_events(&json, 150_000_000);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].type_tag, "0x1::coin::DepositEvent");
        assert_eq!(events[0].sequence_number, 42);
        assert_eq!(events[0].account_address, "0xaccount");
    }

    #[test]
    fn builder_defaults() {
        let config = AptosIndexerBuilder::new().build_config();
        assert_eq!(config.chain, "aptos");
        assert_eq!(config.confirmation_depth, 1);
        assert_eq!(config.poll_interval_ms, 4000);
    }

    #[test]
    fn builder_custom() {
        let builder = AptosIndexerBuilder::new()
            .id("apt-idx")
            .from_height(100_000_000)
            .to_height(200_000_000)
            .type_prefix("0x1::coin")
            .module("coin")
            .account("0xaccount1")
            .batch_size(50);

        let config = builder.build_config();
        assert_eq!(config.id, "apt-idx");
        assert_eq!(config.from_block, 100_000_000);
        assert_eq!(config.to_block, Some(200_000_000));

        let filter = builder.build_filter();
        assert_eq!(filter.type_prefixes, vec!["0x1::coin"]);
        assert_eq!(filter.modules, vec!["coin"]);
        assert_eq!(filter.accounts, vec!["0xaccount1"]);
    }

    #[test]
    fn block_serializable() {
        let block = AptosBlock {
            height: 100,
            hash: "h".into(),
            timestamp: 1000,
            first_version: 500,
            last_version: 550,
            tx_count: 51,
            epoch: 10,
            round: 3,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: AptosBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.height, 100);
        assert_eq!(back.epoch, 10);
    }

    #[test]
    fn event_serializable() {
        let event = AptosEvent {
            type_tag: "0x1::coin::DepositEvent".into(),
            sequence_number: 0,
            data: serde_json::json!({}),
            version: 0,
            height: 0,
            tx_hash: "tx".into(),
            account_address: "0x1".into(),
            creation_number: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: AptosEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.type_tag, "0x1::coin::DepositEvent");
    }
}
