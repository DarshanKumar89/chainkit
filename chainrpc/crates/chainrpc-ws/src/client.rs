//! WebSocket JSON-RPC client with auto-reconnect and subscription management.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio::time;
use tokio_tungstenite::tungstenite::Message;

use chainrpc_core::error::TransportError;
use chainrpc_core::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
use chainrpc_core::transport::{HealthStatus, RpcTransport};

use crate::subscriptions::{SubscriptionId, SubscriptionManager};

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<JsonRpcResponse, TransportError>>>>>;

/// Configuration for the WebSocket client.
#[derive(Debug, Clone)]
pub struct WsClientConfig {
    /// Reconnect backoff starting duration.
    pub reconnect_initial: Duration,
    /// Maximum reconnect backoff.
    pub reconnect_max: Duration,
}

impl Default for WsClientConfig {
    fn default() -> Self {
        Self {
            reconnect_initial: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(60),
        }
    }
}

/// Command sent from callers to the background WS task.
enum WsCommand {
    Send {
        req: JsonRpcRequest,
        tx: oneshot::Sender<Result<JsonRpcResponse, TransportError>>,
    },
    Close,
}

/// WebSocket JSON-RPC client.
///
/// Maintains a background task that owns the WebSocket connection and
/// handles reconnect + re-subscribe logic transparently.
pub struct WsRpcClient {
    url: String,
    cmd_tx: mpsc::UnboundedSender<WsCommand>,
    subscriptions: SubscriptionManager,
    _req_id: AtomicU64,
}

impl WsRpcClient {
    /// Connect to `url` and start the background task.
    pub async fn connect(
        url: impl Into<String>,
        config: WsClientConfig,
    ) -> Result<Self, TransportError> {
        let url = url.into();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<WsCommand>();
        let subscriptions = SubscriptionManager::new();
        let subs_clone = subscriptions.clone();
        let url_clone = url.clone();

        tokio::spawn(async move {
            ws_task(url_clone, cmd_rx, subs_clone, config).await;
        });

        Ok(Self {
            url,
            cmd_tx,
            subscriptions,
            _req_id: AtomicU64::new(1),
        })
    }

    /// Subscribe to a WebSocket event stream.
    ///
    /// `kind` is the subscription type (e.g. `"newHeads"`, `"logs"`).
    pub async fn subscribe(
        &self,
        kind: &str,
        params: Vec<Value>,
    ) -> Result<(SubscriptionId, mpsc::UnboundedReceiver<Value>), TransportError> {
        // eth_subscribe returns the subscription ID as the result
        let id_val: String = self
            .call(
                self._req_id.fetch_add(1, Ordering::Relaxed),
                "eth_subscribe",
                std::iter::once(Value::String(kind.to_string()))
                    .chain(params.clone())
                    .collect(),
            )
            .await?;
        let sub_id = SubscriptionId(id_val);
        let rx = self
            .subscriptions
            .register(sub_id.clone(), kind.to_string(), params);
        Ok((sub_id, rx))
    }
}

impl Drop for WsRpcClient {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(WsCommand::Close);
    }
}

#[async_trait]
impl RpcTransport for WsRpcClient {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(WsCommand::Send { req, tx })
            .map_err(|_| TransportError::WebSocket("WS task closed".into()))?;
        rx.await
            .map_err(|_| TransportError::WebSocket("WS response dropped".into()))?
    }

    fn health(&self) -> HealthStatus {
        // Could track reconnect state, for now return Unknown
        HealthStatus::Unknown
    }

    fn url(&self) -> &str {
        &self.url
    }
}

/// Background task that owns the WebSocket connection.
async fn ws_task(
    url: String,
    mut cmd_rx: mpsc::UnboundedReceiver<WsCommand>,
    subscriptions: SubscriptionManager,
    config: WsClientConfig,
) {
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let mut backoff = config.reconnect_initial;

    loop {
        tracing::info!(url = %url, "connecting via WebSocket");

        let conn = tokio_tungstenite::connect_async(&url).await;

        match conn {
            Err(e) => {
                tracing::warn!(error = %e, "WS connect failed, retrying in {backoff:?}");
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(config.reconnect_max);
                continue;
            }
            Ok((ws_stream, _)) => {
                backoff = config.reconnect_initial; // reset on success
                let (mut sink, mut stream) = ws_stream.split();

                // Re-subscribe any active subscriptions
                for (kind, params) in subscriptions.active_subscriptions() {
                    let resubscribe_req = serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "eth_subscribe",
                        "params": std::iter::once(Value::String(kind.clone()))
                            .chain(params)
                            .collect::<Vec<_>>(),
                        "id": 0
                    });
                    if let Ok(msg) = serde_json::to_string(&resubscribe_req) {
                        let _ = sink.send(Message::Text(msg.into())).await;
                    }
                }

                // Main dispatch loop
                loop {
                    tokio::select! {
                        // Incoming commands from callers
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                None | Some(WsCommand::Close) => return,
                                Some(WsCommand::Send { req, tx }) => {
                                    let id = match &req.id { RpcId::Number(n) => *n, _ => 0 };
                                    pending.lock().unwrap().insert(id, tx);
                                    if let Ok(msg) = serde_json::to_string(&req) {
                                        if sink.send(Message::Text(msg.into())).await.is_err() {
                                            // Connection dropped — break to reconnect
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        // Incoming messages from node
                        msg = stream.next() => {
                            match msg {
                                None => break, // stream closed
                                Some(Err(e)) => {
                                    tracing::warn!(error = %e, "WS receive error");
                                    break;
                                }
                                Some(Ok(Message::Text(text))) => {
                                    handle_message(
                                        text.as_str(),
                                        &pending,
                                        &subscriptions,
                                    );
                                }
                                Some(Ok(Message::Close(_))) => break,
                                _ => {}
                            }
                        }
                    }
                }

                tracing::warn!(url = %url, "WS disconnected, reconnecting in {backoff:?}");
                time::sleep(backoff).await;
                backoff = (backoff * 2).min(config.reconnect_max);
            }
        }
    }
}

fn handle_message(text: &str, pending: &PendingMap, subscriptions: &SubscriptionManager) {
    let Ok(val) = serde_json::from_str::<Value>(text) else {
        tracing::debug!("failed to parse WS message as JSON");
        return;
    };

    // Check if this is a subscription notification
    if val.get("method").and_then(|m| m.as_str()) == Some("eth_subscription") {
        if let Some(params) = val.get("params") {
            let sub_id = params["subscription"]
                .as_str()
                .map(|s| SubscriptionId(s.to_string()));
            if let Some(id) = sub_id {
                subscriptions.dispatch(&id, params["result"].clone());
            }
        }
        return;
    }

    // Regular JSON-RPC response
    if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(text) {
        let id = match &resp.id {
            RpcId::Number(n) => *n,
            _ => return,
        };
        if let Some(tx) = pending.lock().unwrap().remove(&id) {
            let _ = tx.send(Ok(resp));
        }
    }
}
