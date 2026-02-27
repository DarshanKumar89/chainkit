//! WebSocket subscription management.
//!
//! Tracks active `eth_subscribe` subscriptions and re-subscribes them
//! when the WebSocket connection is re-established after a disconnect.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;

/// A unique subscription ID returned by `eth_subscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubscriptionId(pub String);

impl From<String> for SubscriptionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for SubscriptionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata for a single subscription.
#[derive(Clone)]
struct SubscriptionEntry {
    /// The subscription type (e.g. `"newHeads"`, `"logs"`).
    kind: String,
    /// Parameters for re-subscribing (e.g. filter params).
    params: Vec<Value>,
    /// Channel to forward incoming messages to the caller.
    sender: mpsc::UnboundedSender<Value>,
}

/// Manages active WebSocket subscriptions and supports re-subscription.
#[derive(Clone, Default)]
pub struct SubscriptionManager {
    entries: Arc<Mutex<HashMap<SubscriptionId, SubscriptionEntry>>>,
}

impl SubscriptionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscription.
    pub fn register(
        &self,
        id: SubscriptionId,
        kind: String,
        params: Vec<Value>,
    ) -> mpsc::UnboundedReceiver<Value> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.entries.lock().unwrap().insert(
            id,
            SubscriptionEntry { kind, params, sender: tx },
        );
        rx
    }

    /// Forward an incoming notification to the correct subscription.
    pub fn dispatch(&self, id: &SubscriptionId, message: Value) {
        if let Some(entry) = self.entries.lock().unwrap().get(id) {
            let _ = entry.sender.send(message);
        }
    }

    /// Remove a subscription (e.g. after `eth_unsubscribe`).
    pub fn remove(&self, id: &SubscriptionId) {
        self.entries.lock().unwrap().remove(id);
    }

    /// Return the list of (kind, params) for all active subscriptions.
    /// Used to re-subscribe after reconnect.
    pub fn active_subscriptions(&self) -> Vec<(String, Vec<Value>)> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .map(|e| (e.kind.clone(), e.params.clone()))
            .collect()
    }

    /// Number of active subscriptions.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    /// Returns `true` if there are no active subscriptions.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_dispatch() {
        let mgr = SubscriptionManager::new();
        let id = SubscriptionId("0xdeadbeef".into());
        let mut rx = mgr.register(id.clone(), "newHeads".into(), vec![]);

        mgr.dispatch(&id, serde_json::json!({"number": "0x1"}));

        let msg = rx.try_recv().unwrap();
        assert_eq!(msg["number"], "0x1");
    }

    #[test]
    fn remove_subscription() {
        let mgr = SubscriptionManager::new();
        let id = SubscriptionId("0x1".into());
        let _rx = mgr.register(id.clone(), "logs".into(), vec![]);
        assert_eq!(mgr.len(), 1);
        mgr.remove(&id);
        assert_eq!(mgr.len(), 0);
    }

    #[test]
    fn active_subscriptions_for_resubscribe() {
        let mgr = SubscriptionManager::new();
        mgr.register(SubscriptionId("0xa".into()), "newHeads".into(), vec![]);
        mgr.register(SubscriptionId("0xb".into()), "logs".into(), vec![serde_json::json!({})]);

        let active = mgr.active_subscriptions();
        assert_eq!(active.len(), 2);
    }
}
