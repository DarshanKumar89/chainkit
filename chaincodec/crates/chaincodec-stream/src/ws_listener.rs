//! `EvmWsListener` — concrete `BlockListener` implementation for EVM chains
//! using an Ethereum JSON-RPC WebSocket subscription (`eth_subscribe("logs", ...)`).
//!
//! # Usage
//! ```no_run
//! use chaincodec_stream::ws_listener::EvmWsListener;
//! use chaincodec_core::chain::chains;
//! use std::sync::Arc;
//!
//! let listener = Arc::new(EvmWsListener::new(
//!     chains::ethereum(),
//!     "wss://mainnet.infura.io/ws/v3/YOUR_KEY",
//! ));
//! ```

use crate::listener::{BlockListener, RawEventStream};
use async_trait::async_trait;
use chaincodec_core::{chain::ChainId, error::StreamError, event::RawEvent};
use futures::{channel::mpsc, SinkExt, StreamExt};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// EVM WebSocket log listener.
///
/// Subscribes to `eth_subscribe("logs", filter)` and emits `RawEvent` items.
/// Reconnection is handled by the `StreamEngine` which calls `subscribe()`
/// again on stream completion.
pub struct EvmWsListener {
    chain: ChainId,
    rpc_url: String,
    /// Optional list of contract addresses to filter (empty = all)
    filter_addresses: Vec<String>,
    connected: Arc<AtomicBool>,
}

impl EvmWsListener {
    /// Create a new listener that connects to the given WebSocket URL.
    ///
    /// # Arguments
    /// * `chain` — chain descriptor (used to populate `RawEvent.chain`)
    /// * `rpc_url` — WebSocket RPC URL (`ws://` or `wss://`)
    pub fn new(chain: ChainId, rpc_url: impl Into<String>) -> Self {
        Self {
            chain,
            rpc_url: rpc_url.into(),
            filter_addresses: vec![],
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Add a contract address filter (can be called multiple times).
    pub fn with_address(mut self, addr: impl Into<String>) -> Self {
        self.filter_addresses.push(addr.into());
        self
    }

    /// Add multiple contract address filters.
    pub fn with_addresses(mut self, addrs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.filter_addresses.extend(addrs.into_iter().map(Into::into));
        self
    }
}

#[async_trait]
impl BlockListener for EvmWsListener {
    fn chain_slug(&self) -> &str {
        &self.chain.slug
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    async fn subscribe(&self) -> Result<RawEventStream, StreamError> {
        let (tx, rx) = mpsc::channel::<Result<RawEvent, StreamError>>(512);

        let rpc_url = self.rpc_url.clone();
        let chain_id = self.chain.clone();
        let filter_addresses = self.filter_addresses.clone();
        let connected = Arc::clone(&self.connected);

        tokio::spawn(async move {
            run_ws_subscription(rpc_url, chain_id, filter_addresses, connected, tx).await;
        });

        Ok(Box::pin(rx))
    }
}

// ─── Internal WebSocket loop ──────────────────────────────────────────────────

async fn run_ws_subscription(
    rpc_url: String,
    chain_id: ChainId,
    filter_addresses: Vec<String>,
    connected: Arc<AtomicBool>,
    mut tx: mpsc::Sender<Result<RawEvent, StreamError>>,
) {
    info!("Connecting to WebSocket: {}", rpc_url);

    let ws_stream = match connect_async(&rpc_url).await {
        Ok((ws, _)) => {
            connected.store(true, Ordering::Relaxed);
            info!("WebSocket connected: {}", rpc_url);
            ws
        }
        Err(e) => {
            connected.store(false, Ordering::Relaxed);
            error!("WebSocket connect failed: {}", e);
            let _ = tx
                .send(Err(StreamError::ConnectionFailed {
                    url: rpc_url.clone(),
                    reason: e.to_string(),
                }))
                .await;
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();

    // Build and send the eth_subscribe("logs", filter) message
    let filter = build_log_filter(&filter_addresses);
    let sub_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_subscribe",
        "params": ["logs", filter]
    });

    if let Err(e) = write.send(Message::Text(sub_msg.to_string())).await {
        error!("Failed to send eth_subscribe: {}", e);
        connected.store(false, Ordering::Relaxed);
        let _ = tx.send(Err(StreamError::Closed)).await;
        return;
    }

    // Process incoming messages
    while let Some(msg_result) = read.next().await {
        match msg_result {
            Err(e) => {
                warn!("WebSocket error: {}", e);
                connected.store(false, Ordering::Relaxed);
                let _ = tx.send(Err(StreamError::Closed)).await;
                break;
            }
            Ok(Message::Text(text)) => {
                debug!("WS message: {}", &text[..text.len().min(120)]);
                if let Some(raw_result) = parse_eth_subscription_log(&text, &chain_id) {
                    if tx.send(raw_result).await.is_err() {
                        // Receiver dropped
                        break;
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket closed by server");
                connected.store(false, Ordering::Relaxed);
                let _ = tx.send(Err(StreamError::Closed)).await;
                break;
            }
            Ok(Message::Ping(data)) => {
                // Respond to server pings to keep the connection alive
                let _ = write.send(Message::Pong(data)).await;
            }
            Ok(_) => {} // binary / pong — ignore
        }
    }

    connected.store(false, Ordering::Relaxed);
    info!("WebSocket subscription loop ended");
}

// ─── Message parsing ─────────────────────────────────────────────────────────

/// Parse an `eth_subscription` log message into a `RawEvent`.
/// Returns `None` for subscription confirmations, removed logs, or parse errors.
fn parse_eth_subscription_log(
    text: &str,
    chain_id: &ChainId,
) -> Option<Result<RawEvent, StreamError>> {
    let v: Value = serde_json::from_str(text).ok()?;

    // Only handle `eth_subscription` events, not the subscription ID confirmation
    if v.get("method")?.as_str()? != "eth_subscription" {
        return None;
    }

    let result = v.get("params")?.get("result")?;

    // Skip reorged/removed logs
    if result
        .get("removed")
        .and_then(|r| r.as_bool())
        .unwrap_or(false)
    {
        return None;
    }

    let address = result.get("address")?.as_str()?.to_string();

    let topics: Vec<String> = result
        .get("topics")?
        .as_array()?
        .iter()
        .filter_map(|t| t.as_str().map(String::from))
        .collect();

    if topics.is_empty() {
        return None; // No topics → no fingerprint
    }

    let data_hex = result
        .get("data")
        .and_then(|d| d.as_str())
        .unwrap_or("0x");
    let data =
        hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex)).unwrap_or_default();

    let block_number = hex_str_to_u64(result.get("blockNumber").and_then(|b| b.as_str()));
    let log_index =
        hex_str_to_u64(result.get("logIndex").and_then(|l| l.as_str())) as u32;
    let tx_hash = result
        .get("transactionHash")
        .and_then(|t| t.as_str())
        .unwrap_or("0x0")
        .to_string();

    Some(Ok(RawEvent {
        chain: chain_id.clone(),
        tx_hash,
        block_number,
        block_timestamp: 0, // not in eth_subscribe logs
        log_index,
        address,
        topics,
        data,
        raw_receipt: None,
    }))
}

fn build_log_filter(addresses: &[String]) -> Value {
    if addresses.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::json!({ "address": addresses })
    }
}

fn hex_str_to_u64(s: Option<&str>) -> u64 {
    s.and_then(|h| {
        u64::from_str_radix(h.strip_prefix("0x").unwrap_or(h), 16).ok()
    })
    .unwrap_or(0)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chaincodec_core::chain::chains;

    #[test]
    fn parse_subscription_log() {
        let chain = chains::ethereum();
        let msg = r#"{
            "jsonrpc":"2.0","method":"eth_subscription",
            "params":{
                "subscription":"0xabc",
                "result":{
                    "address":"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                    "topics":["0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"],
                    "data":"0x0000000000000000000000000000000000000000000000000000000000000001",
                    "blockNumber":"0x1234","logIndex":"0x0",
                    "transactionHash":"0xdeadbeef",
                    "removed":false
                }
            }
        }"#;
        let result = parse_eth_subscription_log(msg, &chain);
        assert!(result.is_some());
        let raw = result.unwrap().unwrap();
        assert_eq!(raw.block_number, 0x1234);
        assert_eq!(raw.topics.len(), 1);
    }

    #[test]
    fn skip_subscription_confirmation() {
        let chain = chains::ethereum();
        let msg = r#"{"jsonrpc":"2.0","id":1,"result":"0xsubid"}"#;
        let result = parse_eth_subscription_log(msg, &chain);
        assert!(result.is_none());
    }

    #[test]
    fn skip_removed_log() {
        let chain = chains::ethereum();
        let msg = r#"{
            "jsonrpc":"2.0","method":"eth_subscription",
            "params":{"subscription":"0x1","result":{
                "address":"0x1","topics":["0x1"],"data":"0x","removed":true,
                "blockNumber":"0x1","logIndex":"0x0","transactionHash":"0x1"
            }}
        }"#;
        let result = parse_eth_subscription_log(msg, &chain);
        assert!(result.is_none());
    }
}
