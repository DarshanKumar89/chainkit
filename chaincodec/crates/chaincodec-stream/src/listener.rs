//! `BlockListener` trait — abstraction over RPC/WS block polling.
//!
//! Each chain implementation provides a `BlockListener` that produces
//! `RawEvent` items. The stream engine manages one listener per chain.

use async_trait::async_trait;
use chaincodec_core::{error::StreamError, event::RawEvent};
use futures::Stream;
use std::pin::Pin;

/// A stream of raw events from a single chain.
pub type RawEventStream = Pin<Box<dyn Stream<Item = Result<RawEvent, StreamError>> + Send>>;

/// Abstracts over different chain RPC backends.
#[async_trait]
pub trait BlockListener: Send + Sync {
    /// Chain slug this listener covers.
    fn chain_slug(&self) -> &str;

    /// Connect and start streaming raw events.
    /// Returns a pinned async stream of `RawEvent` items.
    async fn subscribe(&self) -> Result<RawEventStream, StreamError>;

    /// Returns `true` if this listener is currently connected.
    fn is_connected(&self) -> bool;
}
