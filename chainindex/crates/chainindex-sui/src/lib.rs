//! Sui indexer for ChainIndex.
//!
//! Sui organizes data around checkpoints (finalized blocks) and objects.
//! This module maps Sui checkpoints to [`BlockSummary`] and Sui events
//! to [`DecodedEvent`].
//!
//! # Key Types
//!
//! - [`SuiCheckpoint`] — Checkpoint with digest, epoch, and transaction info
//! - [`SuiEvent`] — Move event emitted during transaction execution
//! - [`SuiObjectChange`] — Object mutation (created, mutated, deleted)
//! - [`SuiRpcClient`] — Trait abstracting Sui JSON-RPC calls
//! - [`SuiIndexerBuilder`] — Fluent builder for Sui indexer configs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter};

// ---------------------------------------------------------------------------
// Checkpoint type
// ---------------------------------------------------------------------------

/// A Sui checkpoint (equivalent to a finalized block).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiCheckpoint {
    /// Checkpoint sequence number.
    pub sequence_number: u64,
    /// Checkpoint digest (hash).
    pub digest: String,
    /// Previous checkpoint digest.
    pub previous_digest: Option<String>,
    /// Timestamp in unix seconds.
    pub timestamp: i64,
    /// Number of transactions in this checkpoint.
    pub tx_count: u32,
    /// Epoch number.
    pub epoch: u64,
    /// Total gas cost for all transactions.
    pub total_gas_cost: u64,
    /// Total computation cost.
    pub total_computation_cost: u64,
}

impl SuiCheckpoint {
    pub fn to_block_summary(&self) -> BlockSummary {
        BlockSummary {
            number: self.sequence_number,
            hash: self.digest.clone(),
            parent_hash: self.previous_digest.clone().unwrap_or_default(),
            timestamp: self.timestamp,
            tx_count: self.tx_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Event type
// ---------------------------------------------------------------------------

/// A Sui Move event emitted during transaction execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiEvent {
    /// Move event type (e.g., "0x2::coin::CoinDeposit<0x2::sui::SUI>").
    pub event_type: String,
    /// Package ID that contains the module.
    pub package_id: String,
    /// Module name.
    pub module_name: String,
    /// Sender address.
    pub sender: String,
    /// Transaction digest.
    pub tx_digest: String,
    /// Checkpoint sequence number.
    pub checkpoint: u64,
    /// Event sequence number within the transaction.
    pub event_seq: u64,
    /// Parsed event data.
    pub parsed_json: serde_json::Value,
    /// BCS-encoded event data (hex).
    pub bcs: Option<String>,
    /// Timestamp in milliseconds.
    pub timestamp_ms: Option<u64>,
}

impl SuiEvent {
    /// Extract the struct name from the event type.
    pub fn struct_name(&self) -> &str {
        // "0x2::coin::CoinDeposit<...>" → "CoinDeposit"
        let base = self.event_type.split('<').next().unwrap_or(&self.event_type);
        base.rsplit("::").next().unwrap_or("unknown")
    }

    pub fn to_decoded_event(&self, chain: &str) -> DecodedEvent {
        let schema = format!("{}::{}", self.module_name, self.struct_name());

        DecodedEvent {
            chain: chain.to_string(),
            schema,
            address: self.package_id.clone(),
            tx_hash: self.tx_digest.clone(),
            block_number: self.checkpoint,
            log_index: self.event_seq as u32,
            fields_json: self.parsed_json.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Object change type
// ---------------------------------------------------------------------------

/// The type of object change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ObjectChangeType {
    Created,
    Mutated,
    Deleted,
    Wrapped,
    Unwrapped,
    Published,
}

/// A Sui object change within a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiObjectChange {
    /// Type of change.
    pub change_type: ObjectChangeType,
    /// Object ID.
    pub object_id: String,
    /// Object type (Move type tag).
    pub object_type: Option<String>,
    /// Object version after the change.
    pub version: u64,
    /// Object digest after the change.
    pub digest: Option<String>,
    /// Owner of the object after the change.
    pub owner: Option<String>,
    /// Transaction digest.
    pub tx_digest: String,
}

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait SuiRpcClient: Send + Sync {
    /// Get the latest checkpoint sequence number.
    async fn get_latest_checkpoint(&self) -> Result<u64, IndexerError>;

    /// Get checkpoint data.
    async fn get_checkpoint(
        &self,
        seq: u64,
    ) -> Result<Option<SuiCheckpoint>, IndexerError>;

    /// Get events for a checkpoint.
    async fn get_events(
        &self,
        tx_digest: &str,
    ) -> Result<Vec<SuiEvent>, IndexerError>;

    /// Query events by type.
    async fn query_events(
        &self,
        event_type: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Vec<SuiEvent>, IndexerError>;
}

// ---------------------------------------------------------------------------
// Event filter
// ---------------------------------------------------------------------------

/// Sui-specific event filter.
#[derive(Debug, Clone, Default)]
pub struct SuiEventFilter {
    /// Filter by package ID.
    pub packages: Vec<String>,
    /// Filter by module name.
    pub modules: Vec<String>,
    /// Filter by event struct name.
    pub event_types: Vec<String>,
    /// Filter by sender address.
    pub senders: Vec<String>,
}

impl SuiEventFilter {
    pub fn matches(&self, event: &SuiEvent) -> bool {
        if !self.packages.is_empty()
            && !self.packages.iter().any(|p| p == &event.package_id)
        {
            return false;
        }

        if !self.modules.is_empty()
            && !self.modules.iter().any(|m| m == &event.module_name)
        {
            return false;
        }

        if !self.event_types.is_empty()
            && !self.event_types.iter().any(|t| event.event_type.contains(t))
        {
            return false;
        }

        if !self.senders.is_empty()
            && !self.senders.iter().any(|s| s == &event.sender)
        {
            return false;
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

/// Parses Sui JSON-RPC responses.
pub struct SuiResponseParser;

impl SuiResponseParser {
    /// Parse a checkpoint from `sui_getCheckpoint` JSON.
    pub fn parse_checkpoint(json: &serde_json::Value) -> Option<SuiCheckpoint> {
        let seq_str = json["sequenceNumber"].as_str()?;
        let sequence_number = seq_str.parse::<u64>().ok()?;

        let digest = json["digest"].as_str().unwrap_or_default().to_string();
        let previous_digest = json["previousDigest"].as_str().map(|s| s.to_string());

        let timestamp_ms = json["timestampMs"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let timestamp = (timestamp_ms / 1000) as i64;

        let tx_count = json["transactions"]
            .as_array()
            .map(|a| a.len() as u32)
            .or_else(|| {
                json["networkTotalTransactions"]
                    .as_str()
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .unwrap_or(0);

        let epoch = json["epoch"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let computation = json["epochRollingGasCostSummary"]["computationCost"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        Some(SuiCheckpoint {
            sequence_number,
            digest,
            previous_digest,
            timestamp,
            tx_count,
            epoch,
            total_gas_cost: 0,
            total_computation_cost: computation,
        })
    }

    /// Parse events from `sui_getEvents` JSON.
    pub fn parse_events(json: &serde_json::Value, checkpoint: u64) -> Vec<SuiEvent> {
        let events_array = json.as_array()
            .or_else(|| json.get("data").and_then(|d| d.as_array()));

        let Some(events) = events_array else {
            return Vec::new();
        };

        events
            .iter()
            .enumerate()
            .filter_map(|(idx, ev)| {
                let event_type = ev["type"].as_str()
                    .or_else(|| ev["eventType"].as_str())?;
                let package_id = ev["packageId"].as_str().unwrap_or_default();
                let module_name = ev["transactionModule"].as_str()
                    .or_else(|| ev["moduleName"].as_str())
                    .unwrap_or_default();
                let sender = ev["sender"].as_str().unwrap_or_default();
                let tx_digest = ev["id"]["txDigest"].as_str()
                    .or_else(|| ev["txDigest"].as_str())
                    .unwrap_or_default();
                let event_seq = ev["id"]["eventSeq"].as_str()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(idx as u64);

                let parsed_json = ev.get("parsedJson")
                    .or_else(|| ev.get("parsedJSON"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                Some(SuiEvent {
                    event_type: event_type.to_string(),
                    package_id: package_id.to_string(),
                    module_name: module_name.to_string(),
                    sender: sender.to_string(),
                    tx_digest: tx_digest.to_string(),
                    checkpoint,
                    event_seq,
                    parsed_json,
                    bcs: ev["bcs"].as_str().map(|s| s.to_string()),
                    timestamp_ms: ev["timestampMs"]
                        .as_str()
                        .and_then(|s| s.parse().ok()),
                })
            })
            .collect()
    }

    /// Parse object changes from a transaction effect.
    pub fn parse_object_changes(
        json: &serde_json::Value,
        tx_digest: &str,
    ) -> Vec<SuiObjectChange> {
        let changes_array = json.as_array()
            .or_else(|| json.get("objectChanges").and_then(|o| o.as_array()));

        let Some(changes) = changes_array else {
            return Vec::new();
        };

        changes
            .iter()
            .filter_map(|change| {
                let change_type_str = change["type"].as_str()?;
                let change_type = match change_type_str {
                    "created" => ObjectChangeType::Created,
                    "mutated" => ObjectChangeType::Mutated,
                    "deleted" => ObjectChangeType::Deleted,
                    "wrapped" => ObjectChangeType::Wrapped,
                    "unwrapped" => ObjectChangeType::Unwrapped,
                    "published" => ObjectChangeType::Published,
                    _ => return None,
                };

                Some(SuiObjectChange {
                    change_type,
                    object_id: change["objectId"].as_str().unwrap_or_default().to_string(),
                    object_type: change["objectType"].as_str().map(|s| s.to_string()),
                    version: change["version"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| change["version"].as_u64())
                        .unwrap_or(0),
                    digest: change["digest"].as_str().map(|s| s.to_string()),
                    owner: change["owner"]["AddressOwner"].as_str().map(|s| s.to_string()),
                    tx_digest: tx_digest.to_string(),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct SuiIndexerBuilder {
    from_checkpoint: u64,
    to_checkpoint: Option<u64>,
    packages: Vec<String>,
    modules: Vec<String>,
    event_types: Vec<String>,
    senders: Vec<String>,
    batch_size: u64,
    poll_interval_ms: u64,
    checkpoint_interval: u64,
    confirmation_depth: u64,
    id: String,
    chain: String,
}

impl SuiIndexerBuilder {
    pub fn new() -> Self {
        Self {
            from_checkpoint: 0,
            to_checkpoint: None,
            packages: Vec::new(),
            modules: Vec::new(),
            event_types: Vec::new(),
            senders: Vec::new(),
            batch_size: 100,
            poll_interval_ms: 500, // ~500ms checkpoint time
            checkpoint_interval: 100,
            confirmation_depth: 1, // BFT instant finality
            id: "sui-indexer".into(),
            chain: "sui".into(),
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

    pub fn from_checkpoint(mut self, seq: u64) -> Self {
        self.from_checkpoint = seq;
        self
    }

    pub fn to_checkpoint(mut self, seq: u64) -> Self {
        self.to_checkpoint = Some(seq);
        self
    }

    pub fn package(mut self, package_id: impl Into<String>) -> Self {
        self.packages.push(package_id.into());
        self
    }

    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.modules.push(module.into());
        self
    }

    pub fn event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_types.push(event_type.into());
        self
    }

    pub fn sender(mut self, sender: impl Into<String>) -> Self {
        self.senders.push(sender.into());
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
            from_block: self.from_checkpoint,
            to_block: self.to_checkpoint,
            confirmation_depth: self.confirmation_depth,
            batch_size: self.batch_size,
            checkpoint_interval: self.checkpoint_interval,
            poll_interval_ms: self.poll_interval_ms,
            filter: EventFilter {
                addresses: self.packages.clone(),
                topic0_values: self.event_types.clone(),
                from_block: Some(self.from_checkpoint),
                to_block: self.to_checkpoint,
            },
        }
    }

    pub fn build_filter(&self) -> SuiEventFilter {
        SuiEventFilter {
            packages: self.packages.clone(),
            modules: self.modules.clone(),
            event_types: self.event_types.clone(),
            senders: self.senders.clone(),
        }
    }
}

impl Default for SuiIndexerBuilder {
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
    fn checkpoint_to_summary() {
        let cp = SuiCheckpoint {
            sequence_number: 50_000_000,
            digest: "checkpoint_digest".into(),
            previous_digest: Some("prev_digest".into()),
            timestamp: 1700000000,
            tx_count: 100,
            epoch: 500,
            total_gas_cost: 1_000_000,
            total_computation_cost: 500_000,
        };
        let summary = cp.to_block_summary();
        assert_eq!(summary.number, 50_000_000);
        assert_eq!(summary.hash, "checkpoint_digest");
        assert_eq!(summary.parent_hash, "prev_digest");
        assert_eq!(summary.tx_count, 100);
    }

    #[test]
    fn event_struct_name() {
        let event = SuiEvent {
            event_type: "0x2::coin::CoinDeposit<0x2::sui::SUI>".into(),
            package_id: "0x2".into(),
            module_name: "coin".into(),
            sender: "0xsender".into(),
            tx_digest: "tx_digest".into(),
            checkpoint: 100,
            event_seq: 0,
            parsed_json: serde_json::json!({}),
            bcs: None,
            timestamp_ms: None,
        };
        assert_eq!(event.struct_name(), "CoinDeposit");
    }

    #[test]
    fn event_to_decoded() {
        let event = SuiEvent {
            event_type: "0x2::coin::CoinDeposit<0x2::sui::SUI>".into(),
            package_id: "0x2".into(),
            module_name: "coin".into(),
            sender: "0xsender".into(),
            tx_digest: "tx_abc".into(),
            checkpoint: 100,
            event_seq: 3,
            parsed_json: serde_json::json!({"amount": "1000"}),
            bcs: None,
            timestamp_ms: None,
        };
        let decoded = event.to_decoded_event("sui");
        assert_eq!(decoded.chain, "sui");
        assert_eq!(decoded.schema, "coin::CoinDeposit");
        assert_eq!(decoded.address, "0x2");
        assert_eq!(decoded.tx_hash, "tx_abc");
        assert_eq!(decoded.log_index, 3);
    }

    #[test]
    fn filter_package() {
        let filter = SuiEventFilter {
            packages: vec!["0x2".into()],
            ..Default::default()
        };
        let event = SuiEvent {
            event_type: "0x2::coin::CoinDeposit".into(),
            package_id: "0x2".into(),
            module_name: "coin".into(),
            sender: "0xsender".into(),
            tx_digest: "".into(),
            checkpoint: 0,
            event_seq: 0,
            parsed_json: serde_json::Value::Null,
            bcs: None,
            timestamp_ms: None,
        };
        assert!(filter.matches(&event));

        let other = SuiEvent {
            event_type: "".into(),
            package_id: "0x3".into(),
            module_name: "".into(),
            sender: "".into(),
            tx_digest: "".into(),
            checkpoint: 0,
            event_seq: 0,
            parsed_json: serde_json::Value::Null,
            bcs: None,
            timestamp_ms: None,
        };
        assert!(!filter.matches(&other));
    }

    #[test]
    fn filter_empty_matches_all() {
        let filter = SuiEventFilter::default();
        let event = SuiEvent {
            event_type: "anything".into(),
            package_id: "any".into(),
            module_name: "any".into(),
            sender: "any".into(),
            tx_digest: "".into(),
            checkpoint: 0,
            event_seq: 0,
            parsed_json: serde_json::Value::Null,
            bcs: None,
            timestamp_ms: None,
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn parse_checkpoint_json() {
        let json = serde_json::json!({
            "sequenceNumber": "50000000",
            "digest": "cp_digest_abc",
            "previousDigest": "cp_digest_prev",
            "timestampMs": "1700000000000",
            "transactions": ["tx1", "tx2", "tx3"],
            "epoch": "500",
            "epochRollingGasCostSummary": {
                "computationCost": "500000"
            }
        });
        let cp = SuiResponseParser::parse_checkpoint(&json).unwrap();
        assert_eq!(cp.sequence_number, 50_000_000);
        assert_eq!(cp.digest, "cp_digest_abc");
        assert_eq!(cp.previous_digest.as_deref(), Some("cp_digest_prev"));
        assert_eq!(cp.timestamp, 1700000000);
        assert_eq!(cp.tx_count, 3);
        assert_eq!(cp.epoch, 500);
    }

    #[test]
    fn parse_events_json() {
        let json = serde_json::json!([
            {
                "type": "0x2::coin::CoinDeposit<0x2::sui::SUI>",
                "packageId": "0x2",
                "transactionModule": "coin",
                "sender": "0xsender1",
                "id": {
                    "txDigest": "tx_digest_1",
                    "eventSeq": "0"
                },
                "parsedJson": { "amount": "1000" }
            }
        ]);
        let events = SuiResponseParser::parse_events(&json, 100);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "0x2::coin::CoinDeposit<0x2::sui::SUI>");
        assert_eq!(events[0].package_id, "0x2");
        assert_eq!(events[0].module_name, "coin");
    }

    #[test]
    fn parse_object_changes_json() {
        let json = serde_json::json!([
            {
                "type": "created",
                "objectId": "0xobj1",
                "objectType": "0x2::coin::Coin<0x2::sui::SUI>",
                "version": "100",
                "digest": "obj_digest",
                "owner": { "AddressOwner": "0xowner1" }
            },
            {
                "type": "mutated",
                "objectId": "0xobj2",
                "objectType": "0x2::coin::Coin<0x2::sui::SUI>",
                "version": "101",
                "digest": "obj_digest2",
                "owner": { "AddressOwner": "0xowner2" }
            }
        ]);
        let changes = SuiResponseParser::parse_object_changes(&json, "tx_digest");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].change_type, ObjectChangeType::Created);
        assert_eq!(changes[0].object_id, "0xobj1");
        assert_eq!(changes[0].owner.as_deref(), Some("0xowner1"));
        assert_eq!(changes[1].change_type, ObjectChangeType::Mutated);
    }

    #[test]
    fn builder_defaults() {
        let config = SuiIndexerBuilder::new().build_config();
        assert_eq!(config.chain, "sui");
        assert_eq!(config.confirmation_depth, 1);
        assert_eq!(config.poll_interval_ms, 500);
    }

    #[test]
    fn builder_custom() {
        let builder = SuiIndexerBuilder::new()
            .id("sui-idx")
            .from_checkpoint(10_000_000)
            .to_checkpoint(20_000_000)
            .package("0x2")
            .module("coin")
            .event_type("CoinDeposit")
            .sender("0xsender")
            .batch_size(50);

        let config = builder.build_config();
        assert_eq!(config.id, "sui-idx");
        assert_eq!(config.from_block, 10_000_000);
        assert_eq!(config.to_block, Some(20_000_000));

        let filter = builder.build_filter();
        assert_eq!(filter.packages, vec!["0x2"]);
        assert_eq!(filter.modules, vec!["coin"]);
        assert_eq!(filter.event_types, vec!["CoinDeposit"]);
        assert_eq!(filter.senders, vec!["0xsender"]);
    }

    #[test]
    fn checkpoint_serializable() {
        let cp = SuiCheckpoint {
            sequence_number: 100,
            digest: "d".into(),
            previous_digest: Some("pd".into()),
            timestamp: 1000,
            tx_count: 5,
            epoch: 10,
            total_gas_cost: 1000,
            total_computation_cost: 500,
        };
        let json = serde_json::to_string(&cp).unwrap();
        let back: SuiCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sequence_number, 100);
    }

    #[test]
    fn event_serializable() {
        let event = SuiEvent {
            event_type: "0x2::coin::CoinDeposit".into(),
            package_id: "0x2".into(),
            module_name: "coin".into(),
            sender: "0x1".into(),
            tx_digest: "tx".into(),
            checkpoint: 100,
            event_seq: 0,
            parsed_json: serde_json::json!({}),
            bcs: None,
            timestamp_ms: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: SuiEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "0x2::coin::CoinDeposit");
    }

    #[test]
    fn object_change_serializable() {
        let change = SuiObjectChange {
            change_type: ObjectChangeType::Created,
            object_id: "0xobj".into(),
            object_type: Some("0x2::coin::Coin".into()),
            version: 1,
            digest: Some("d".into()),
            owner: Some("0xowner".into()),
            tx_digest: "tx".into(),
        };
        let json = serde_json::to_string(&change).unwrap();
        let back: SuiObjectChange = serde_json::from_str(&json).unwrap();
        assert_eq!(back.change_type, ObjectChangeType::Created);
    }
}
