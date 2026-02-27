//! chainrpc-ws — WebSocket JSON-RPC transport with auto-reconnect.
//!
//! # Features
//! - Auto-reconnect on disconnect (exponential backoff)
//! - Subscription management (eth_subscribe / eth_unsubscribe)
//! - Auto-resubscribe after reconnect
//! - Request multiplexing over a single connection

pub mod client;
pub mod subscriptions;

pub use client::WsRpcClient;
pub use subscriptions::{SubscriptionId, SubscriptionManager};
