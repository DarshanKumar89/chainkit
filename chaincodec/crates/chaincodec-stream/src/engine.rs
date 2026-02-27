//! `StreamEngine` — orchestrates multiple chain listeners and decoder workers.

use crate::config::StreamConfig;
use crate::listener::BlockListener;
use chaincodec_core::{
    decoder::ChainDecoder,
    error::StreamError,
    event::DecodedEvent,
    schema::SchemaRegistry,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// Metrics snapshot for the stream engine.
#[derive(Debug, Clone, Default)]
pub struct StreamMetrics {
    pub events_decoded: u64,
    pub events_skipped: u64,
    pub decode_errors: u64,
    pub reconnections: u64,
}

/// The top-level streaming engine.
///
/// # Usage
/// ```no_run
/// # async fn example() {
/// use chaincodec_stream::{StreamEngine, StreamConfig};
/// // ... build engine, call run(), receive from subscriber ...
/// # }
/// ```
pub struct StreamEngine {
    config: StreamConfig,
    listeners: HashMap<String, Arc<dyn BlockListener>>,
    registry: Arc<dyn SchemaRegistry>,
    decoders: HashMap<String, Arc<dyn ChainDecoder>>,
    tx: broadcast::Sender<DecodedEvent>,
    metrics: Arc<std::sync::Mutex<StreamMetrics>>,
}

impl StreamEngine {
    /// Create a new `StreamEngine`.
    pub fn new(
        config: StreamConfig,
        registry: Arc<dyn SchemaRegistry>,
    ) -> (Self, broadcast::Receiver<DecodedEvent>) {
        let (tx, rx) = broadcast::channel(config.channel_capacity);
        let engine = Self {
            config,
            listeners: HashMap::new(),
            registry,
            decoders: HashMap::new(),
            tx,
            metrics: Arc::new(std::sync::Mutex::new(StreamMetrics::default())),
        };
        (engine, rx)
    }

    /// Register a chain listener.
    pub fn add_listener(&mut self, listener: Arc<dyn BlockListener>) {
        self.listeners
            .insert(listener.chain_slug().to_string(), listener);
    }

    /// Register a chain decoder.
    pub fn add_decoder(&mut self, chain_slug: impl Into<String>, decoder: Arc<dyn ChainDecoder>) {
        self.decoders.insert(chain_slug.into(), decoder);
    }

    /// Subscribe to the decoded event stream.
    /// Call before `run()` to avoid missing events.
    pub fn subscribe(&self) -> broadcast::Receiver<DecodedEvent> {
        self.tx.subscribe()
    }

    /// Returns a snapshot of current metrics.
    pub fn metrics(&self) -> StreamMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// Start the engine. Spawns one Tokio task per chain listener.
    /// This method returns immediately; listeners run in the background.
    pub async fn run(self: Arc<Self>) {
        info!("StreamEngine starting with {} chains", self.listeners.len());

        for (chain_slug, listener) in &self.listeners {
            let chain_slug = chain_slug.clone();
            let listener = Arc::clone(listener);
            let engine = Arc::clone(&self);

            tokio::spawn(async move {
                engine.run_listener(chain_slug, listener).await;
            });
        }
    }

    async fn run_listener(
        &self,
        chain_slug: String,
        listener: Arc<dyn BlockListener>,
    ) {
        use futures::StreamExt;

        let mut retry = 0u32;
        loop {
            info!("Connecting listener for chain: {}", chain_slug);
            match listener.subscribe().await {
                Err(e) => {
                    error!("Listener connect error [{chain_slug}]: {e}");
                    retry += 1;
                    self.metrics.lock().unwrap().reconnections += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(retry.min(6)))).await;
                    continue;
                }
                Ok(mut stream) => {
                    retry = 0;
                    while let Some(item) = stream.next().await {
                        match item {
                            Err(e) => {
                                warn!("Stream error [{chain_slug}]: {e}");
                                break; // Reconnect
                            }
                            Ok(raw) => {
                                self.process_raw_event(raw).await;
                            }
                        }
                    }
                    info!("Stream closed for [{chain_slug}], reconnecting...");
                    self.metrics.lock().unwrap().reconnections += 1;
                }
            }
        }
    }

    async fn process_raw_event(&self, raw: chaincodec_core::event::RawEvent) {
        let chain_slug = raw.chain.slug.clone();
        let decoder = match self.decoders.get(&chain_slug) {
            Some(d) => d,
            None => {
                warn!("No decoder registered for chain: {}", chain_slug);
                self.metrics.lock().unwrap().events_skipped += 1;
                return;
            }
        };

        let fp = decoder.fingerprint(&raw);
        let schema = match self.registry.get_by_fingerprint(&fp) {
            Some(s) => s,
            None => {
                if !self.config.skip_unknown {
                    warn!("Unknown schema for fingerprint: {}", fp);
                }
                self.metrics.lock().unwrap().events_skipped += 1;
                return;
            }
        };

        // Filter by subscribed schemas if configured
        if !self.config.schemas.is_empty() && !self.config.schemas.contains(&schema.name) {
            self.metrics.lock().unwrap().events_skipped += 1;
            return;
        }

        match decoder.decode_event(&raw, &schema) {
            Ok(event) => {
                self.metrics.lock().unwrap().events_decoded += 1;
                if let Err(e) = self.tx.send(event) {
                    // Receiver dropped — not a fatal error
                    warn!("No active subscribers: {e}");
                }
            }
            Err(e) => {
                error!("Decode error [{chain_slug}]: {e}");
                self.metrics.lock().unwrap().decode_errors += 1;
            }
        }
    }
}
