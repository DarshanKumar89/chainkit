//! Cosmos (Tendermint/CometBFT) indexer for ChainIndex.
//!
//! Provides Cosmos-specific types, RPC client trait, event parsing, and
//! an indexer builder for Cosmos SDK chains (Cosmos Hub, Osmosis, Sei, etc.).
//!
//! # Architecture
//!
//! Cosmos organizes data around blocks and events. Blocks contain transactions,
//! and each transaction emits typed events with key-value attributes. This module
//! maps Cosmos blocks to [`BlockSummary`] and Cosmos events to [`DecodedEvent`].
//!
//! # Key Types
//!
//! - [`CosmosBlock`] — Cosmos block with proposer, evidence, and tx info
//! - [`CosmosEvent`] — Typed event with attributes from a transaction
//! - [`CosmosRpcClient`] — Trait abstracting Tendermint RPC calls
//! - [`CosmosIndexerBuilder`] — Fluent builder for Cosmos indexer configs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter};

// ---------------------------------------------------------------------------
// Cosmos block type
// ---------------------------------------------------------------------------

/// A Cosmos block with chain-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosmosBlock {
    /// Block height.
    pub height: u64,
    /// Block hash (hex, uppercase).
    pub hash: String,
    /// Previous block hash.
    pub parent_hash: String,
    /// Block timestamp (RFC 3339 from Tendermint, stored as unix seconds).
    pub timestamp: i64,
    /// Number of transactions in the block.
    pub tx_count: u32,
    /// Proposer validator address.
    pub proposer: String,
    /// Chain ID (e.g., "cosmoshub-4").
    pub chain_id: String,
}

impl CosmosBlock {
    /// Convert to the chain-agnostic [`BlockSummary`].
    pub fn to_block_summary(&self) -> BlockSummary {
        BlockSummary {
            number: self.height,
            hash: self.hash.clone(),
            parent_hash: self.parent_hash.clone(),
            timestamp: self.timestamp,
            tx_count: self.tx_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Cosmos event type
// ---------------------------------------------------------------------------

/// A single event attribute (key-value pair).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
    /// Whether this attribute was indexed by Tendermint.
    pub index: bool,
}

/// A Cosmos event emitted during transaction execution.
///
/// Events have a type string (e.g., `transfer`, `message`, `ibc_transfer`)
/// and a list of key-value attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosmosEvent {
    /// Event type (e.g., `transfer`, `message`, `coin_spent`).
    pub event_type: String,
    /// Key-value attributes.
    pub attributes: Vec<EventAttribute>,
    /// Transaction hash that emitted this event.
    pub tx_hash: String,
    /// Block height.
    pub height: u64,
    /// Message index within the transaction.
    pub msg_index: u32,
}

impl CosmosEvent {
    /// Get attribute value by key.
    pub fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.key == key)
            .map(|a| a.value.as_str())
    }

    /// Check if this event is an IBC event.
    pub fn is_ibc(&self) -> bool {
        self.event_type.starts_with("ibc_")
            || self.event_type == "send_packet"
            || self.event_type == "recv_packet"
            || self.event_type == "acknowledge_packet"
            || self.event_type == "timeout_packet"
    }

    /// Convert to the chain-agnostic [`DecodedEvent`].
    pub fn to_decoded_event(&self, chain: &str) -> DecodedEvent {
        let mut fields = serde_json::Map::new();
        for attr in &self.attributes {
            fields.insert(attr.key.clone(), serde_json::Value::String(attr.value.clone()));
        }

        DecodedEvent {
            chain: chain.to_string(),
            schema: self.event_type.clone(),
            address: self.attribute("module").unwrap_or("unknown").to_string(),
            tx_hash: self.tx_hash.clone(),
            block_number: self.height,
            log_index: self.msg_index,
            fields_json: serde_json::Value::Object(fields),
        }
    }
}

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

/// Trait abstracting Cosmos/Tendermint RPC calls.
///
/// Implementations talk to a Tendermint JSON-RPC endpoint (port 26657).
#[async_trait]
pub trait CosmosRpcClient: Send + Sync {
    /// Get the latest block height.
    async fn get_latest_height(&self) -> Result<u64, IndexerError>;

    /// Get block at a given height.
    async fn get_block(&self, height: u64) -> Result<Option<CosmosBlock>, IndexerError>;

    /// Get block results (events) at a given height.
    async fn get_block_results(
        &self,
        height: u64,
    ) -> Result<Vec<CosmosEvent>, IndexerError>;

    /// Search for transactions matching a query string.
    async fn tx_search(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<CosmosEvent>, IndexerError>;
}

// ---------------------------------------------------------------------------
// Event filter
// ---------------------------------------------------------------------------

/// Cosmos-specific event filter.
#[derive(Debug, Clone, Default)]
pub struct CosmosEventFilter {
    /// Filter by event type (e.g., `transfer`, `message`).
    pub event_types: Vec<String>,
    /// Filter by module name (from `message.module` attribute).
    pub modules: Vec<String>,
    /// Filter by sender address.
    pub senders: Vec<String>,
    /// Only include IBC events.
    pub ibc_only: bool,
}

impl CosmosEventFilter {
    /// Check if an event matches this filter.
    pub fn matches(&self, event: &CosmosEvent) -> bool {
        if self.ibc_only && !event.is_ibc() {
            return false;
        }

        if !self.event_types.is_empty()
            && !self.event_types.iter().any(|t| t == &event.event_type)
        {
            return false;
        }

        if !self.modules.is_empty() {
            let module = event.attribute("module").unwrap_or("");
            if !self.modules.iter().any(|m| m == module) {
                return false;
            }
        }

        if !self.senders.is_empty() {
            let sender = event.attribute("sender").unwrap_or("");
            if !self.senders.iter().any(|s| s == sender) {
                return false;
            }
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Event parser
// ---------------------------------------------------------------------------

/// Parses raw Tendermint JSON responses into [`CosmosEvent`]s.
pub struct CosmosEventParser;

impl CosmosEventParser {
    /// Parse events from a `block_results` JSON response.
    pub fn parse_block_results(
        json: &serde_json::Value,
        height: u64,
    ) -> Vec<CosmosEvent> {
        let mut events = Vec::new();

        // Parse tx_results events
        if let Some(tx_results) = json["txs_results"].as_array()
            .or_else(|| json["result"]["txs_results"].as_array())
        {
            for (tx_idx, tx_result) in tx_results.iter().enumerate() {
                if let Some(tx_events) = tx_result["events"].as_array() {
                    for event_json in tx_events {
                        if let Some(ev) = Self::parse_event(event_json, height, tx_idx as u32) {
                            events.push(ev);
                        }
                    }
                }
            }
        }

        // Parse begin_block_events
        if let Some(begin_events) = json["begin_block_events"].as_array()
            .or_else(|| json["result"]["begin_block_events"].as_array())
        {
            for event_json in begin_events {
                if let Some(ev) = Self::parse_event(event_json, height, 0) {
                    events.push(ev);
                }
            }
        }

        // Parse end_block_events
        if let Some(end_events) = json["end_block_events"].as_array()
            .or_else(|| json["result"]["end_block_events"].as_array())
        {
            for event_json in end_events {
                if let Some(ev) = Self::parse_event(event_json, height, 0) {
                    events.push(ev);
                }
            }
        }

        events
    }

    /// Parse a single event from JSON.
    fn parse_event(
        json: &serde_json::Value,
        height: u64,
        msg_index: u32,
    ) -> Option<CosmosEvent> {
        let event_type = json["type"].as_str()?;
        let attrs = json["attributes"].as_array()?;

        let attributes: Vec<EventAttribute> = attrs
            .iter()
            .map(|a| {
                let key = a["key"].as_str().unwrap_or_default().to_string();
                let value = a["value"].as_str().unwrap_or_default().to_string();
                let index = a["index"].as_bool().unwrap_or(false);
                EventAttribute { key, value, index }
            })
            .collect();

        Some(CosmosEvent {
            event_type: event_type.to_string(),
            attributes,
            tx_hash: String::new(), // filled by caller
            height,
            msg_index,
        })
    }

    /// Parse a Cosmos block JSON into a [`CosmosBlock`].
    pub fn parse_block(json: &serde_json::Value) -> Option<CosmosBlock> {
        let block = json.get("block").or_else(|| json.get("result").and_then(|r| r.get("block")))?;
        let header = block.get("header")?;

        let height_str = header["height"].as_str().unwrap_or("0");
        let height = height_str.parse::<u64>().unwrap_or(0);
        let hash = json.get("block_id")
            .or_else(|| json.get("result").and_then(|r| r.get("block_id")))
            .and_then(|bid| bid["hash"].as_str())
            .unwrap_or_default()
            .to_string();
        let parent_hash = header["last_block_id"]["hash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let chain_id = header["chain_id"].as_str().unwrap_or_default().to_string();
        let proposer = header["proposer_address"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        let timestamp = parse_rfc3339_to_unix(
            header["time"].as_str().unwrap_or(""),
        );

        let tx_count = block["data"]["txs"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        Some(CosmosBlock {
            height,
            hash,
            parent_hash,
            timestamp,
            tx_count,
            proposer,
            chain_id,
        })
    }
}

/// Parse an RFC 3339 timestamp string to unix seconds.
fn parse_rfc3339_to_unix(s: &str) -> i64 {
    if s.is_empty() {
        return 0;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .or_else(|_| {
            // Tendermint sometimes uses nanosecond precision
            let trimmed = if let Some(dot_pos) = s.rfind('.') {
                if let Some(z_pos) = s.rfind('Z') {
                    format!("{}Z", &s[..dot_pos.min(z_pos)])
                } else {
                    s[..dot_pos].to_string()
                }
            } else {
                s.to_string()
            };
            chrono::DateTime::parse_from_rfc3339(&trimmed)
        })
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Fluent builder for a Cosmos indexer configuration.
pub struct CosmosIndexerBuilder {
    from_height: u64,
    to_height: Option<u64>,
    event_types: Vec<String>,
    modules: Vec<String>,
    ibc_only: bool,
    batch_size: u64,
    poll_interval_ms: u64,
    checkpoint_interval: u64,
    confirmation_depth: u64,
    id: String,
    chain: String,
}

impl CosmosIndexerBuilder {
    pub fn new() -> Self {
        Self {
            from_height: 1,
            to_height: None,
            event_types: Vec::new(),
            modules: Vec::new(),
            ibc_only: false,
            batch_size: 100,
            poll_interval_ms: 6000, // Cosmos ~6s block time
            checkpoint_interval: 100,
            confirmation_depth: 1, // BFT instant finality
            id: "cosmos-indexer".into(),
            chain: "cosmoshub".into(),
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

    pub fn event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_types.push(event_type.into());
        self
    }

    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.modules.push(module.into());
        self
    }

    pub fn ibc_only(mut self, ibc: bool) -> Self {
        self.ibc_only = ibc;
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

    pub fn confirmation_depth(mut self, depth: u64) -> Self {
        self.confirmation_depth = depth;
        self
    }

    /// Build the generic [`IndexerConfig`].
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
                addresses: self.modules.clone(),
                topic0_values: self.event_types.clone(),
                from_block: Some(self.from_height),
                to_block: self.to_height,
            },
        }
    }

    /// Build the Cosmos-specific event filter.
    pub fn build_filter(&self) -> CosmosEventFilter {
        CosmosEventFilter {
            event_types: self.event_types.clone(),
            modules: self.modules.clone(),
            senders: Vec::new(),
            ibc_only: self.ibc_only,
        }
    }
}

impl Default for CosmosIndexerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// IBC helpers
// ---------------------------------------------------------------------------

/// Known IBC event types.
pub fn ibc_event_types() -> &'static [&'static str] {
    &[
        "send_packet",
        "recv_packet",
        "acknowledge_packet",
        "timeout_packet",
        "ibc_transfer",
        "fungible_token_packet",
        "denomination_trace",
        "channel_open_init",
        "channel_open_try",
        "channel_open_ack",
        "channel_open_confirm",
        "channel_close_init",
        "channel_close_confirm",
        "connection_open_init",
        "connection_open_try",
        "connection_open_ack",
        "connection_open_confirm",
    ]
}

/// Extract IBC packet info from a send_packet or recv_packet event.
pub fn extract_ibc_packet(event: &CosmosEvent) -> Option<IbcPacketInfo> {
    if !event.is_ibc() {
        return None;
    }

    Some(IbcPacketInfo {
        source_port: event.attribute("packet_src_port").unwrap_or_default().to_string(),
        source_channel: event.attribute("packet_src_channel").unwrap_or_default().to_string(),
        dest_port: event.attribute("packet_dst_port").unwrap_or_default().to_string(),
        dest_channel: event.attribute("packet_dst_channel").unwrap_or_default().to_string(),
        sequence: event.attribute("packet_sequence")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        data: event.attribute("packet_data").unwrap_or_default().to_string(),
    })
}

/// IBC packet metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IbcPacketInfo {
    pub source_port: String,
    pub source_channel: String,
    pub dest_port: String,
    pub dest_channel: String,
    pub sequence: u64,
    pub data: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosmos_block_to_summary() {
        let block = CosmosBlock {
            height: 18_500_000,
            hash: "ABCDEF1234".into(),
            parent_hash: "FEDCBA4321".into(),
            timestamp: 1700000000,
            tx_count: 15,
            proposer: "cosmosvalcons1abc".into(),
            chain_id: "cosmoshub-4".into(),
        };
        let summary = block.to_block_summary();
        assert_eq!(summary.number, 18_500_000);
        assert_eq!(summary.hash, "ABCDEF1234");
        assert_eq!(summary.parent_hash, "FEDCBA4321");
        assert_eq!(summary.timestamp, 1700000000);
        assert_eq!(summary.tx_count, 15);
    }

    #[test]
    fn event_attribute_lookup() {
        let event = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![
                EventAttribute { key: "sender".into(), value: "cosmos1abc".into(), index: true },
                EventAttribute { key: "recipient".into(), value: "cosmos1def".into(), index: true },
                EventAttribute { key: "amount".into(), value: "1000uatom".into(), index: false },
            ],
            tx_hash: "TX123".into(),
            height: 100,
            msg_index: 0,
        };
        assert_eq!(event.attribute("sender"), Some("cosmos1abc"));
        assert_eq!(event.attribute("recipient"), Some("cosmos1def"));
        assert_eq!(event.attribute("amount"), Some("1000uatom"));
        assert_eq!(event.attribute("nonexistent"), None);
    }

    #[test]
    fn ibc_event_detection() {
        let ibc_event = CosmosEvent {
            event_type: "send_packet".into(),
            attributes: vec![],
            tx_hash: "TX1".into(),
            height: 100,
            msg_index: 0,
        };
        assert!(ibc_event.is_ibc());

        let ibc_event2 = CosmosEvent {
            event_type: "ibc_transfer".into(),
            attributes: vec![],
            tx_hash: "TX2".into(),
            height: 100,
            msg_index: 0,
        };
        assert!(ibc_event2.is_ibc());

        let regular = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![],
            tx_hash: "TX3".into(),
            height: 100,
            msg_index: 0,
        };
        assert!(!regular.is_ibc());
    }

    #[test]
    fn event_to_decoded() {
        let event = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![
                EventAttribute { key: "sender".into(), value: "cosmos1abc".into(), index: true },
                EventAttribute { key: "module".into(), value: "bank".into(), index: false },
            ],
            tx_hash: "TXHASH123".into(),
            height: 500,
            msg_index: 2,
        };
        let decoded = event.to_decoded_event("cosmoshub");
        assert_eq!(decoded.chain, "cosmoshub");
        assert_eq!(decoded.schema, "transfer");
        assert_eq!(decoded.address, "bank");
        assert_eq!(decoded.tx_hash, "TXHASH123");
        assert_eq!(decoded.block_number, 500);
        assert_eq!(decoded.log_index, 2);
        assert_eq!(decoded.fields_json["sender"], "cosmos1abc");
    }

    #[test]
    fn filter_event_type() {
        let filter = CosmosEventFilter {
            event_types: vec!["transfer".into()],
            ..Default::default()
        };
        let event = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(filter.matches(&event));

        let other = CosmosEvent {
            event_type: "message".into(),
            attributes: vec![],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(!filter.matches(&other));
    }

    #[test]
    fn filter_module() {
        let filter = CosmosEventFilter {
            modules: vec!["bank".into()],
            ..Default::default()
        };
        let event = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![
                EventAttribute { key: "module".into(), value: "bank".into(), index: false },
            ],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(filter.matches(&event));

        let other = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![
                EventAttribute { key: "module".into(), value: "staking".into(), index: false },
            ],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(!filter.matches(&other));
    }

    #[test]
    fn filter_ibc_only() {
        let filter = CosmosEventFilter {
            ibc_only: true,
            ..Default::default()
        };
        let ibc = CosmosEvent {
            event_type: "send_packet".into(),
            attributes: vec![],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(filter.matches(&ibc));

        let regular = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(!filter.matches(&regular));
    }

    #[test]
    fn filter_empty_matches_all() {
        let filter = CosmosEventFilter::default();
        let event = CosmosEvent {
            event_type: "anything".into(),
            attributes: vec![],
            tx_hash: "TX".into(),
            height: 1,
            msg_index: 0,
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn builder_defaults() {
        let builder = CosmosIndexerBuilder::new();
        let config = builder.build_config();
        assert_eq!(config.chain, "cosmoshub");
        assert_eq!(config.from_block, 1);
        assert_eq!(config.confirmation_depth, 1);
        assert_eq!(config.poll_interval_ms, 6000);
    }

    #[test]
    fn builder_custom() {
        let builder = CosmosIndexerBuilder::new()
            .id("my-cosmos")
            .chain("osmosis")
            .from_height(5_000_000)
            .to_height(6_000_000)
            .event_type("transfer")
            .module("bank")
            .ibc_only(true)
            .batch_size(50)
            .poll_interval_ms(3000);

        let config = builder.build_config();
        assert_eq!(config.id, "my-cosmos");
        assert_eq!(config.chain, "osmosis");
        assert_eq!(config.from_block, 5_000_000);
        assert_eq!(config.to_block, Some(6_000_000));
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.poll_interval_ms, 3000);

        let filter = builder.build_filter();
        assert!(filter.ibc_only);
        assert_eq!(filter.event_types, vec!["transfer"]);
        assert_eq!(filter.modules, vec!["bank"]);
    }

    #[test]
    fn parse_block_json() {
        let json = serde_json::json!({
            "block_id": { "hash": "BLOCK_HASH_ABC" },
            "block": {
                "header": {
                    "height": "18500000",
                    "chain_id": "cosmoshub-4",
                    "time": "2024-01-15T12:00:00Z",
                    "proposer_address": "PROPOSER_ADDR",
                    "last_block_id": { "hash": "PARENT_HASH_DEF" }
                },
                "data": {
                    "txs": ["tx1", "tx2", "tx3"]
                }
            }
        });
        let block = CosmosEventParser::parse_block(&json).unwrap();
        assert_eq!(block.height, 18500000);
        assert_eq!(block.hash, "BLOCK_HASH_ABC");
        assert_eq!(block.parent_hash, "PARENT_HASH_DEF");
        assert_eq!(block.chain_id, "cosmoshub-4");
        assert_eq!(block.proposer, "PROPOSER_ADDR");
        assert_eq!(block.tx_count, 3);
        assert!(block.timestamp > 0);
    }

    #[test]
    fn parse_block_results_json() {
        let json = serde_json::json!({
            "txs_results": [
                {
                    "events": [
                        {
                            "type": "transfer",
                            "attributes": [
                                { "key": "sender", "value": "cosmos1abc", "index": true },
                                { "key": "recipient", "value": "cosmos1def", "index": true },
                                { "key": "amount", "value": "1000uatom", "index": false }
                            ]
                        },
                        {
                            "type": "message",
                            "attributes": [
                                { "key": "module", "value": "bank", "index": false }
                            ]
                        }
                    ]
                }
            ]
        });
        let events = CosmosEventParser::parse_block_results(&json, 100);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "transfer");
        assert_eq!(events[0].attributes.len(), 3);
        assert_eq!(events[1].event_type, "message");
    }

    #[test]
    fn ibc_packet_extraction() {
        let event = CosmosEvent {
            event_type: "send_packet".into(),
            attributes: vec![
                EventAttribute { key: "packet_src_port".into(), value: "transfer".into(), index: false },
                EventAttribute { key: "packet_src_channel".into(), value: "channel-0".into(), index: false },
                EventAttribute { key: "packet_dst_port".into(), value: "transfer".into(), index: false },
                EventAttribute { key: "packet_dst_channel".into(), value: "channel-141".into(), index: false },
                EventAttribute { key: "packet_sequence".into(), value: "12345".into(), index: false },
                EventAttribute { key: "packet_data".into(), value: "{\"amount\":\"1000\"}".into(), index: false },
            ],
            tx_hash: "TX1".into(),
            height: 100,
            msg_index: 0,
        };
        let packet = extract_ibc_packet(&event).unwrap();
        assert_eq!(packet.source_port, "transfer");
        assert_eq!(packet.source_channel, "channel-0");
        assert_eq!(packet.dest_channel, "channel-141");
        assert_eq!(packet.sequence, 12345);
    }

    #[test]
    fn ibc_event_types_not_empty() {
        assert!(!ibc_event_types().is_empty());
        assert!(ibc_event_types().contains(&"send_packet"));
    }

    #[test]
    fn rfc3339_parsing() {
        assert!(parse_rfc3339_to_unix("2024-01-15T12:00:00Z") > 0);
        assert_eq!(parse_rfc3339_to_unix(""), 0);
    }

    #[test]
    fn cosmos_block_serializable() {
        let block = CosmosBlock {
            height: 100,
            hash: "H".into(),
            parent_hash: "P".into(),
            timestamp: 1000,
            tx_count: 5,
            proposer: "V".into(),
            chain_id: "test".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: CosmosBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.height, 100);
    }

    #[test]
    fn cosmos_event_serializable() {
        let event = CosmosEvent {
            event_type: "transfer".into(),
            attributes: vec![
                EventAttribute { key: "k".into(), value: "v".into(), index: true },
            ],
            tx_hash: "TX".into(),
            height: 50,
            msg_index: 1,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: CosmosEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "transfer");
    }
}
