//! Bitcoin indexer for ChainIndex.
//!
//! Bitcoin uses a UTXO model rather than account-based events. This module
//! provides UTXO-aware indexing with input/output tracking and address monitoring.
//!
//! # Key Types
//!
//! - [`BitcoinBlock`] — Bitcoin block with merkle root and difficulty
//! - [`BitcoinInput`] / [`BitcoinOutput`] — UTXO model types
//! - [`UtxoHandler`] — Trait for handling UTXO inputs and outputs
//! - [`BitcoinRpcClient`] — Trait abstracting Bitcoin Core JSON-RPC
//! - [`BitcoinIndexerBuilder`] — Fluent builder for Bitcoin indexer configs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use chainindex_core::error::IndexerError;
use chainindex_core::handler::DecodedEvent;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::{BlockSummary, EventFilter};

// ---------------------------------------------------------------------------
// Block type
// ---------------------------------------------------------------------------

/// A Bitcoin block with chain-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinBlock {
    pub height: u64,
    pub hash: String,
    pub parent_hash: String,
    pub timestamp: i64,
    pub tx_count: u32,
    /// Merkle root of transactions.
    pub merkle_root: String,
    /// Block difficulty target (bits field).
    pub bits: String,
    /// Nonce used in mining.
    pub nonce: u64,
    /// Block size in bytes.
    pub size: u64,
    /// Block weight (for SegWit).
    pub weight: u64,
}

impl BitcoinBlock {
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
// UTXO types
// ---------------------------------------------------------------------------

/// A Bitcoin transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinTransaction {
    /// Transaction ID (hash).
    pub txid: String,
    /// Block height containing this transaction.
    pub block_height: u64,
    /// Transaction version.
    pub version: u32,
    /// Inputs (spending UTXOs).
    pub inputs: Vec<BitcoinInput>,
    /// Outputs (creating UTXOs).
    pub outputs: Vec<BitcoinOutput>,
    /// Lock time.
    pub locktime: u64,
    /// Whether this is a coinbase transaction.
    pub is_coinbase: bool,
    /// Total input value in satoshis.
    pub total_input: u64,
    /// Total output value in satoshis.
    pub total_output: u64,
    /// Fee in satoshis (input - output).
    pub fee: u64,
}

/// A transaction input (spending a previous UTXO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinInput {
    /// Previous transaction ID being spent.
    pub prev_txid: String,
    /// Output index in the previous transaction.
    pub prev_vout: u32,
    /// ScriptSig (hex).
    pub script_sig: String,
    /// Witness data (for SegWit).
    pub witness: Vec<String>,
    /// Sequence number.
    pub sequence: u64,
    /// Resolved address (if determinable from scriptPubKey of spent output).
    pub address: Option<String>,
    /// Value being spent in satoshis (from the referenced UTXO).
    pub value: Option<u64>,
}

/// A transaction output (creating a new UTXO).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitcoinOutput {
    /// Output index within the transaction.
    pub vout: u32,
    /// Value in satoshis.
    pub value: u64,
    /// ScriptPubKey (hex).
    pub script_pubkey: String,
    /// Script type (e.g., "pubkeyhash", "scripthash", "witness_v0_keyhash").
    pub script_type: String,
    /// Resolved address (if standard script type).
    pub address: Option<String>,
}

// ---------------------------------------------------------------------------
// UTXO handler trait
// ---------------------------------------------------------------------------

/// Trait for handling Bitcoin UTXO inputs and outputs.
///
/// This is the Bitcoin equivalent of `EventHandler` — instead of events,
/// Bitcoin has inputs (spending UTXOs) and outputs (creating UTXOs).
#[async_trait]
pub trait UtxoHandler: Send + Sync {
    /// Called for each transaction input.
    async fn handle_input(
        &self,
        input: &BitcoinInput,
        tx: &BitcoinTransaction,
    ) -> Result<(), IndexerError>;

    /// Called for each transaction output.
    async fn handle_output(
        &self,
        output: &BitcoinOutput,
        tx: &BitcoinTransaction,
    ) -> Result<(), IndexerError>;
}

// ---------------------------------------------------------------------------
// RPC client trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait BitcoinRpcClient: Send + Sync {
    /// Get the current block count (height of the tip).
    async fn get_block_count(&self) -> Result<u64, IndexerError>;

    /// Get the block hash at a given height.
    async fn get_block_hash(&self, height: u64) -> Result<String, IndexerError>;

    /// Get full block data at a given height.
    async fn get_block(&self, height: u64) -> Result<Option<BitcoinBlock>, IndexerError>;

    /// Get transactions for a block at a given height.
    async fn get_block_transactions(
        &self,
        height: u64,
    ) -> Result<Vec<BitcoinTransaction>, IndexerError>;

    /// Get a raw transaction by txid.
    async fn get_raw_transaction(
        &self,
        txid: &str,
    ) -> Result<Option<BitcoinTransaction>, IndexerError>;
}

// ---------------------------------------------------------------------------
// Address filter
// ---------------------------------------------------------------------------

/// Bitcoin-specific address filter for UTXO monitoring.
#[derive(Debug, Clone, Default)]
pub struct AddressFilter {
    /// Addresses to monitor (both input and output).
    pub addresses: Vec<String>,
    /// Minimum output value in satoshis.
    pub min_value: Option<u64>,
    /// Include coinbase transactions.
    pub include_coinbase: bool,
    /// Script types to include (empty = all).
    pub script_types: Vec<String>,
}

impl AddressFilter {
    /// Check if a transaction matches this filter.
    pub fn matches_transaction(&self, tx: &BitcoinTransaction) -> bool {
        if !self.include_coinbase && tx.is_coinbase {
            return false;
        }

        if self.addresses.is_empty() {
            return true;
        }

        // Check outputs
        for output in &tx.outputs {
            if let Some(ref addr) = output.address {
                if self.addresses.iter().any(|a| a == addr) {
                    if let Some(min) = self.min_value {
                        if output.value < min {
                            continue;
                        }
                    }
                    return true;
                }
            }
        }

        // Check inputs
        for input in &tx.inputs {
            if let Some(ref addr) = input.address {
                if self.addresses.iter().any(|a| a == addr) {
                    return true;
                }
            }
        }

        false
    }

    /// Check if an output matches this filter.
    pub fn matches_output(&self, output: &BitcoinOutput) -> bool {
        if let Some(min) = self.min_value {
            if output.value < min {
                return false;
            }
        }

        if !self.script_types.is_empty()
            && !self.script_types.iter().any(|t| t == &output.script_type)
        {
            return false;
        }

        if self.addresses.is_empty() {
            return true;
        }

        output
            .address
            .as_ref()
            .map(|a| self.addresses.iter().any(|f| f == a))
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Transaction decoder (to DecodedEvent)
// ---------------------------------------------------------------------------

/// Converts Bitcoin transactions to the chain-agnostic [`DecodedEvent`] format.
pub struct BitcoinEventDecoder;

impl BitcoinEventDecoder {
    /// Convert a transaction to a decoded event representing a transfer.
    pub fn tx_to_decoded_event(
        tx: &BitcoinTransaction,
        chain: &str,
    ) -> DecodedEvent {
        let fields = serde_json::json!({
            "txid": tx.txid,
            "version": tx.version,
            "is_coinbase": tx.is_coinbase,
            "input_count": tx.inputs.len(),
            "output_count": tx.outputs.len(),
            "total_input": tx.total_input,
            "total_output": tx.total_output,
            "fee": tx.fee,
        });

        DecodedEvent {
            chain: chain.to_string(),
            schema: if tx.is_coinbase { "coinbase" } else { "transfer" }.to_string(),
            address: "bitcoin".to_string(),
            tx_hash: tx.txid.clone(),
            block_number: tx.block_height,
            log_index: 0,
            fields_json: fields,
        }
    }

    /// Convert an output to a decoded event.
    pub fn output_to_decoded_event(
        output: &BitcoinOutput,
        tx: &BitcoinTransaction,
        chain: &str,
    ) -> DecodedEvent {
        let fields = serde_json::json!({
            "vout": output.vout,
            "value": output.value,
            "script_type": output.script_type,
            "address": output.address,
        });

        DecodedEvent {
            chain: chain.to_string(),
            schema: "utxo_created".to_string(),
            address: output.address.clone().unwrap_or_default(),
            tx_hash: tx.txid.clone(),
            block_number: tx.block_height,
            log_index: output.vout,
            fields_json: fields,
        }
    }
}

// ---------------------------------------------------------------------------
// Block parser
// ---------------------------------------------------------------------------

/// Parses Bitcoin JSON-RPC responses.
pub struct BitcoinBlockParser;

impl BitcoinBlockParser {
    /// Parse a block from `getblock` JSON response (verbosity=1).
    pub fn parse_block(json: &serde_json::Value) -> Option<BitcoinBlock> {
        let height = json["height"].as_u64()?;
        let hash = json["hash"].as_str().unwrap_or_default().to_string();
        let parent_hash = json["previousblockhash"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let timestamp = json["time"].as_i64().unwrap_or(0);
        let tx_count = json["tx"]
            .as_array()
            .map(|a| a.len() as u32)
            .unwrap_or_else(|| json["nTx"].as_u64().unwrap_or(0) as u32);
        let merkle_root = json["merkleroot"].as_str().unwrap_or_default().to_string();
        let bits = json["bits"].as_str().unwrap_or_default().to_string();
        let nonce = json["nonce"].as_u64().unwrap_or(0);
        let size = json["size"].as_u64().unwrap_or(0);
        let weight = json["weight"].as_u64().unwrap_or(0);

        Some(BitcoinBlock {
            height,
            hash,
            parent_hash,
            timestamp,
            tx_count,
            merkle_root,
            bits,
            nonce,
            size,
            weight,
        })
    }

    /// Parse a transaction from `getrawtransaction` JSON response (verbose=true).
    pub fn parse_transaction(
        json: &serde_json::Value,
        block_height: u64,
    ) -> Option<BitcoinTransaction> {
        let txid = json["txid"].as_str()?.to_string();
        let version = json["version"].as_u64().unwrap_or(2) as u32;
        let locktime = json["locktime"].as_u64().unwrap_or(0);

        let inputs: Vec<BitcoinInput> = json["vin"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|vin| {
                let is_coinbase = vin.get("coinbase").is_some();
                BitcoinInput {
                    prev_txid: if is_coinbase {
                        "0000000000000000000000000000000000000000000000000000000000000000".into()
                    } else {
                        vin["txid"].as_str().unwrap_or_default().to_string()
                    },
                    prev_vout: vin["vout"].as_u64().unwrap_or(0) as u32,
                    script_sig: vin["scriptSig"]["hex"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    witness: vin["txinwitness"]
                        .as_array()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .filter_map(|w| w.as_str().map(|s| s.to_string()))
                        .collect(),
                    sequence: vin["sequence"].as_u64().unwrap_or(0xFFFFFFFF),
                    address: None,
                    value: None,
                }
            })
            .collect();

        let is_coinbase = inputs.first().map(|i| &i.prev_txid == "0000000000000000000000000000000000000000000000000000000000000000").unwrap_or(false);

        let outputs: Vec<BitcoinOutput> = json["vout"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|vout| {
                let value_btc = vout["value"].as_f64().unwrap_or(0.0);
                let value_sat = (value_btc * 100_000_000.0) as u64;
                BitcoinOutput {
                    vout: vout["n"].as_u64().unwrap_or(0) as u32,
                    value: value_sat,
                    script_pubkey: vout["scriptPubKey"]["hex"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    script_type: vout["scriptPubKey"]["type"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    address: vout["scriptPubKey"]["address"]
                        .as_str()
                        .map(|s| s.to_string()),
                }
            })
            .collect();

        let total_output: u64 = outputs.iter().map(|o| o.value).sum();

        Some(BitcoinTransaction {
            txid,
            block_height,
            version,
            inputs,
            outputs,
            locktime,
            is_coinbase,
            total_input: 0, // requires looking up previous outputs
            total_output,
            fee: 0, // requires looking up previous outputs
        })
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

pub struct BitcoinIndexerBuilder {
    from_height: u64,
    to_height: Option<u64>,
    addresses: Vec<String>,
    min_value: Option<u64>,
    include_coinbase: bool,
    batch_size: u64,
    poll_interval_ms: u64,
    checkpoint_interval: u64,
    confirmation_depth: u64,
    id: String,
    chain: String,
}

impl BitcoinIndexerBuilder {
    pub fn new() -> Self {
        Self {
            from_height: 0,
            to_height: None,
            addresses: Vec::new(),
            min_value: None,
            include_coinbase: true,
            batch_size: 10, // Bitcoin blocks are large
            poll_interval_ms: 60_000, // ~10min block time
            checkpoint_interval: 10,
            confirmation_depth: 6, // 6 confirmations standard
            id: "bitcoin-indexer".into(),
            chain: "bitcoin".into(),
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

    pub fn address(mut self, address: impl Into<String>) -> Self {
        self.addresses.push(address.into());
        self
    }

    pub fn min_value(mut self, satoshis: u64) -> Self {
        self.min_value = Some(satoshis);
        self
    }

    pub fn include_coinbase(mut self, include: bool) -> Self {
        self.include_coinbase = include;
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
                addresses: self.addresses.clone(),
                topic0_values: Vec::new(),
                from_block: Some(self.from_height),
                to_block: self.to_height,
            },
        }
    }

    pub fn build_filter(&self) -> AddressFilter {
        AddressFilter {
            addresses: self.addresses.clone(),
            min_value: self.min_value,
            include_coinbase: self.include_coinbase,
            script_types: Vec::new(),
        }
    }
}

impl Default for BitcoinIndexerBuilder {
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

    fn sample_tx() -> BitcoinTransaction {
        BitcoinTransaction {
            txid: "abc123".into(),
            block_height: 830000,
            version: 2,
            inputs: vec![BitcoinInput {
                prev_txid: "prev_tx".into(),
                prev_vout: 0,
                script_sig: "".into(),
                witness: vec!["w1".into()],
                sequence: 0xFFFFFFFF,
                address: Some("bc1qsender".into()),
                value: Some(100_000),
            }],
            outputs: vec![
                BitcoinOutput {
                    vout: 0,
                    value: 50_000,
                    script_pubkey: "0014abc".into(),
                    script_type: "witness_v0_keyhash".into(),
                    address: Some("bc1qrecipient".into()),
                },
                BitcoinOutput {
                    vout: 1,
                    value: 49_000,
                    script_pubkey: "0014def".into(),
                    script_type: "witness_v0_keyhash".into(),
                    address: Some("bc1qchange".into()),
                },
            ],
            locktime: 0,
            is_coinbase: false,
            total_input: 100_000,
            total_output: 99_000,
            fee: 1_000,
        }
    }

    #[test]
    fn block_to_summary() {
        let block = BitcoinBlock {
            height: 830000,
            hash: "00000000abc".into(),
            parent_hash: "00000000def".into(),
            timestamp: 1700000000,
            tx_count: 2500,
            merkle_root: "merkle123".into(),
            bits: "17034219".into(),
            nonce: 12345,
            size: 1_500_000,
            weight: 3_993_456,
        };
        let summary = block.to_block_summary();
        assert_eq!(summary.number, 830000);
        assert_eq!(summary.hash, "00000000abc");
        assert_eq!(summary.tx_count, 2500);
    }

    #[test]
    fn address_filter_matches_output() {
        let filter = AddressFilter {
            addresses: vec!["bc1qrecipient".into()],
            ..Default::default()
        };
        let tx = sample_tx();
        assert!(filter.matches_transaction(&tx));
    }

    #[test]
    fn address_filter_matches_input() {
        let filter = AddressFilter {
            addresses: vec!["bc1qsender".into()],
            ..Default::default()
        };
        let tx = sample_tx();
        assert!(filter.matches_transaction(&tx));
    }

    #[test]
    fn address_filter_no_match() {
        let filter = AddressFilter {
            addresses: vec!["bc1qother".into()],
            ..Default::default()
        };
        let tx = sample_tx();
        assert!(!filter.matches_transaction(&tx));
    }

    #[test]
    fn address_filter_empty_matches_all() {
        let filter = AddressFilter::default();
        let tx = sample_tx();
        assert!(filter.matches_transaction(&tx));
    }

    #[test]
    fn address_filter_exclude_coinbase() {
        let filter = AddressFilter {
            include_coinbase: false,
            ..Default::default()
        };
        let mut tx = sample_tx();
        tx.is_coinbase = true;
        assert!(!filter.matches_transaction(&tx));
    }

    #[test]
    fn address_filter_min_value() {
        let filter = AddressFilter {
            addresses: vec!["bc1qrecipient".into()],
            min_value: Some(60_000),
            ..Default::default()
        };
        let tx = sample_tx();
        // bc1qrecipient output is 50_000 which is < 60_000, but input still matches
        // Actually, min_value only applies to outputs in matches_transaction
        assert!(!filter.matches_transaction(&tx)); // output too small, input doesn't check value
    }

    #[test]
    fn output_filter() {
        let filter = AddressFilter {
            min_value: Some(45_000),
            ..Default::default()
        };
        let tx = sample_tx();
        assert!(filter.matches_output(&tx.outputs[0])); // 50_000 >= 45_000
        assert!(filter.matches_output(&tx.outputs[1])); // 49_000 >= 45_000
    }

    #[test]
    fn output_filter_too_small() {
        let filter = AddressFilter {
            min_value: Some(55_000),
            ..Default::default()
        };
        let tx = sample_tx();
        assert!(!filter.matches_output(&tx.outputs[0])); // 50_000 < 55_000
    }

    #[test]
    fn tx_to_decoded_event() {
        let tx = sample_tx();
        let decoded = BitcoinEventDecoder::tx_to_decoded_event(&tx, "bitcoin");
        assert_eq!(decoded.chain, "bitcoin");
        assert_eq!(decoded.schema, "transfer");
        assert_eq!(decoded.tx_hash, "abc123");
        assert_eq!(decoded.block_number, 830000);
        assert_eq!(decoded.fields_json["fee"], 1_000);
    }

    #[test]
    fn coinbase_tx_decoded_event() {
        let mut tx = sample_tx();
        tx.is_coinbase = true;
        let decoded = BitcoinEventDecoder::tx_to_decoded_event(&tx, "bitcoin");
        assert_eq!(decoded.schema, "coinbase");
    }

    #[test]
    fn output_to_decoded_event() {
        let tx = sample_tx();
        let decoded = BitcoinEventDecoder::output_to_decoded_event(
            &tx.outputs[0],
            &tx,
            "bitcoin",
        );
        assert_eq!(decoded.schema, "utxo_created");
        assert_eq!(decoded.fields_json["value"], 50_000);
        assert_eq!(decoded.fields_json["address"], "bc1qrecipient");
    }

    #[test]
    fn parse_block_json() {
        let json = serde_json::json!({
            "height": 830000,
            "hash": "00000000000000000002abc",
            "previousblockhash": "00000000000000000001def",
            "time": 1700000000,
            "nTx": 2500,
            "merkleroot": "merkle_root_hash",
            "bits": "17034219",
            "nonce": 12345,
            "size": 1500000,
            "weight": 3993456
        });
        let block = BitcoinBlockParser::parse_block(&json).unwrap();
        assert_eq!(block.height, 830000);
        assert_eq!(block.hash, "00000000000000000002abc");
        assert_eq!(block.tx_count, 2500);
        assert_eq!(block.nonce, 12345);
    }

    #[test]
    fn parse_transaction_json() {
        let json = serde_json::json!({
            "txid": "txhash123",
            "version": 2,
            "locktime": 0,
            "vin": [
                {
                    "txid": "prev_tx_hash",
                    "vout": 1,
                    "scriptSig": { "hex": "4830450221..." },
                    "sequence": 4294967295u64
                }
            ],
            "vout": [
                {
                    "value": 0.5,
                    "n": 0,
                    "scriptPubKey": {
                        "hex": "0014abc",
                        "type": "witness_v0_keyhash",
                        "address": "bc1qrecipient"
                    }
                }
            ]
        });
        let tx = BitcoinBlockParser::parse_transaction(&json, 830000).unwrap();
        assert_eq!(tx.txid, "txhash123");
        assert_eq!(tx.outputs[0].value, 50_000_000); // 0.5 BTC
        assert_eq!(tx.outputs[0].address.as_deref(), Some("bc1qrecipient"));
        assert!(!tx.is_coinbase);
    }

    #[test]
    fn parse_coinbase_transaction() {
        let json = serde_json::json!({
            "txid": "coinbase_tx",
            "version": 2,
            "locktime": 0,
            "vin": [
                {
                    "coinbase": "03e4800804...",
                    "sequence": 4294967295u64
                }
            ],
            "vout": [
                {
                    "value": 6.25,
                    "n": 0,
                    "scriptPubKey": {
                        "hex": "0014miner",
                        "type": "witness_v0_keyhash",
                        "address": "bc1qminer"
                    }
                }
            ]
        });
        let tx = BitcoinBlockParser::parse_transaction(&json, 830000).unwrap();
        assert!(tx.is_coinbase);
        assert_eq!(tx.outputs[0].value, 625_000_000);
    }

    #[test]
    fn builder_defaults() {
        let config = BitcoinIndexerBuilder::new().build_config();
        assert_eq!(config.chain, "bitcoin");
        assert_eq!(config.confirmation_depth, 6);
        assert_eq!(config.poll_interval_ms, 60_000);
    }

    #[test]
    fn builder_custom() {
        let builder = BitcoinIndexerBuilder::new()
            .id("btc-idx")
            .from_height(830_000)
            .to_height(840_000)
            .address("bc1qaddr1")
            .min_value(10_000)
            .include_coinbase(false)
            .batch_size(5);

        let config = builder.build_config();
        assert_eq!(config.id, "btc-idx");
        assert_eq!(config.from_block, 830_000);
        assert_eq!(config.to_block, Some(840_000));

        let filter = builder.build_filter();
        assert!(!filter.include_coinbase);
        assert_eq!(filter.min_value, Some(10_000));
        assert_eq!(filter.addresses, vec!["bc1qaddr1"]);
    }

    #[test]
    fn block_serializable() {
        let block = BitcoinBlock {
            height: 830000,
            hash: "h".into(),
            parent_hash: "p".into(),
            timestamp: 1700000000,
            tx_count: 100,
            merkle_root: "m".into(),
            bits: "b".into(),
            nonce: 42,
            size: 1000,
            weight: 4000,
        };
        let json = serde_json::to_string(&block).unwrap();
        let back: BitcoinBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back.height, 830000);
    }

    #[test]
    fn transaction_serializable() {
        let tx = sample_tx();
        let json = serde_json::to_string(&tx).unwrap();
        let back: BitcoinTransaction = serde_json::from_str(&json).unwrap();
        assert_eq!(back.txid, "abc123");
        assert_eq!(back.outputs.len(), 2);
    }
}
