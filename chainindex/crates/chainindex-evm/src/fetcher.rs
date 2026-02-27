//! EVM block and log fetcher.
//!
//! Uses JSON-RPC `eth_getBlockByNumber` and `eth_getLogs` with range batching
//! to efficiently fetch events during both backfill and live phases.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use chainindex_core::error::IndexerError;
use chainindex_core::types::{BlockSummary, EventFilter};

/// A raw EVM log as returned by `eth_getLogs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawLog {
    pub address: String,
    pub topics: Vec<String>,
    #[serde(rename = "data")]
    pub data: String,
    #[serde(rename = "blockNumber")]
    pub block_number: String,
    #[serde(rename = "blockHash")]
    pub block_hash: String,
    #[serde(rename = "transactionHash")]
    pub tx_hash: String,
    #[serde(rename = "logIndex")]
    pub log_index: String,
    #[serde(rename = "removed")]
    pub removed: Option<bool>,
}

impl RawLog {
    /// Returns the block number as u64.
    pub fn block_number_u64(&self) -> u64 {
        parse_hex_u64(&self.block_number)
    }

    /// Returns the log index as u32.
    pub fn log_index_u32(&self) -> u32 {
        parse_hex_u64(&self.log_index) as u32
    }

    /// Returns `true` if this log was removed by a reorg.
    pub fn is_removed(&self) -> bool {
        self.removed.unwrap_or(false)
    }
}

/// Trait for fetching EVM data from a JSON-RPC provider.
#[async_trait]
pub trait EvmRpcClient: Send + Sync {
    async fn get_block_number(&self) -> Result<u64, IndexerError>;
    async fn get_block(&self, number: u64) -> Result<Option<BlockSummary>, IndexerError>;
    async fn get_logs(
        &self,
        from: u64,
        to: u64,
        filter: &EventFilter,
    ) -> Result<Vec<RawLog>, IndexerError>;
}

/// EVM fetcher that wraps an `EvmRpcClient` and adds batching logic.
pub struct EvmFetcher<C> {
    client: C,
}

impl<C: EvmRpcClient> EvmFetcher<C> {
    pub fn new(client: C) -> Self {
        Self { client }
    }

    /// Fetch the current chain head block number.
    pub async fn head_block_number(&self) -> Result<u64, IndexerError> {
        self.client.get_block_number().await
    }

    /// Fetch a block summary by number.
    pub async fn block(&self, number: u64) -> Result<Option<BlockSummary>, IndexerError> {
        self.client.get_block(number).await
    }

    /// Fetch all logs in `[from, to]` matching the filter.
    /// Automatically splits into smaller ranges if the node rejects large ranges.
    pub async fn logs(
        &self,
        from: u64,
        to: u64,
        filter: &EventFilter,
        max_range: u64,
    ) -> Result<Vec<RawLog>, IndexerError> {
        if to < from {
            return Ok(vec![]);
        }
        if to - from <= max_range {
            return self.client.get_logs(from, to, filter).await;
        }
        // Split into chunks
        let mut all_logs = Vec::new();
        let mut start = from;
        while start <= to {
            let end = (start + max_range).min(to);
            let chunk = self.client.get_logs(start, end, filter).await?;
            all_logs.extend(chunk);
            start = end + 1;
        }
        Ok(all_logs)
    }
}

/// Parse a hex-encoded string (with or without `0x`) to u64.
pub fn parse_hex_u64(s: &str) -> u64 {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u64::from_str_radix(s, 16).unwrap_or(0)
}

/// Convert a `Value` JSON block response to `BlockSummary`.
pub fn block_from_json(v: &Value) -> Option<BlockSummary> {
    Some(BlockSummary {
        number: parse_hex_u64(v["number"].as_str()?),
        hash: v["hash"].as_str()?.to_string(),
        parent_hash: v["parentHash"].as_str()?.to_string(),
        timestamp: parse_hex_u64(v["timestamp"].as_str()?) as i64,
        tx_count: v["transactions"].as_array().map(|a| a.len() as u32).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_u64_basic() {
        assert_eq!(parse_hex_u64("0x1"), 1);
        assert_eq!(parse_hex_u64("0xff"), 255);
        assert_eq!(parse_hex_u64("1234"), 0x1234);
    }

    #[test]
    fn raw_log_block_number() {
        let log = RawLog {
            address: "0x0".into(),
            topics: vec![],
            data: "0x".into(),
            block_number: "0x12a05f200".into(), // 5_000_000_000
            block_hash: "0x0".into(),
            tx_hash: "0x0".into(),
            log_index: "0x5".into(),
            removed: None,
        };
        assert_eq!(log.block_number_u64(), 5_000_000_000);
        assert_eq!(log.log_index_u32(), 5);
    }
}
