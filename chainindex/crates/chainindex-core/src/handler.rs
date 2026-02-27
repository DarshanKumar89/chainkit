//! Event and block handler traits + registry.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::IndexerError;
use crate::types::{BlockSummary, IndexContext};

/// A decoded blockchain event (chain-agnostic representation).
///
/// In practice this will be a re-export of chaincodec's `DecodedEvent`, but
/// chainindex-core avoids a direct chaincodec dependency to stay modular.
#[derive(Debug, Clone)]
pub struct DecodedEvent {
    /// The schema/event name (e.g. `"ERC20Transfer"`).
    pub schema: String,
    /// Contract address that emitted the event.
    pub address: String,
    /// Transaction hash.
    pub tx_hash: String,
    /// Block number.
    pub block_number: u64,
    /// Log index within the block.
    pub log_index: u32,
    /// Raw decoded fields as JSON for flexibility.
    pub fields_json: serde_json::Value,
}

/// Trait for user-provided event handlers.
///
/// Implement this to process specific event types during indexing.
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Called for each decoded event that matches the handler's schema.
    async fn handle(&self, event: &DecodedEvent, ctx: &IndexContext) -> Result<(), IndexerError>;

    /// The event schema name this handler processes (e.g. `"ERC20Transfer"`).
    fn schema_name(&self) -> &str;
}

/// Trait for user-provided block handlers.
///
/// Called once per block regardless of events.
#[async_trait]
pub trait BlockHandler: Send + Sync {
    async fn handle_block(&self, block: &BlockSummary, ctx: &IndexContext) -> Result<(), IndexerError>;
}

/// Trait for reorg handlers.
///
/// Called when a chain reorganization is detected.
#[async_trait]
pub trait ReorgHandler: Send + Sync {
    async fn on_reorg(
        &self,
        dropped: &[BlockSummary],
        ctx: &IndexContext,
    ) -> Result<(), IndexerError>;
}

/// Registry of event + block + reorg handlers.
pub struct HandlerRegistry {
    event_handlers: HashMap<String, Vec<Arc<dyn EventHandler>>>,
    block_handlers: Vec<Arc<dyn BlockHandler>>,
    reorg_handlers: Vec<Arc<dyn ReorgHandler>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            event_handlers: HashMap::new(),
            block_handlers: vec![],
            reorg_handlers: vec![],
        }
    }

    /// Register an event handler for a specific schema name.
    pub fn on_event(&mut self, handler: Arc<dyn EventHandler>) {
        self.event_handlers
            .entry(handler.schema_name().to_string())
            .or_default()
            .push(handler);
    }

    /// Register a block handler (called for every block).
    pub fn on_block(&mut self, handler: Arc<dyn BlockHandler>) {
        self.block_handlers.push(handler);
    }

    /// Register a reorg handler.
    pub fn on_reorg(&mut self, handler: Arc<dyn ReorgHandler>) {
        self.reorg_handlers.push(handler);
    }

    /// Dispatch an event to all matching handlers.
    pub async fn dispatch_event(
        &self,
        event: &DecodedEvent,
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        if let Some(handlers) = self.event_handlers.get(&event.schema) {
            for handler in handlers {
                handler.handle(event, ctx).await?;
            }
        }
        Ok(())
    }

    /// Dispatch a block to all block handlers.
    pub async fn dispatch_block(
        &self,
        block: &BlockSummary,
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        for handler in &self.block_handlers {
            handler.handle_block(block, ctx).await?;
        }
        Ok(())
    }

    /// Dispatch a reorg event to all reorg handlers.
    pub async fn dispatch_reorg(
        &self,
        dropped: &[BlockSummary],
        ctx: &IndexContext,
    ) -> Result<(), IndexerError> {
        for handler in &self.reorg_handlers {
            handler.on_reorg(dropped, ctx).await?;
        }
        Ok(())
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Counter(Arc<AtomicU32>, String);

    #[async_trait]
    impl EventHandler for Counter {
        async fn handle(&self, _e: &DecodedEvent, _c: &IndexContext) -> Result<(), IndexerError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn schema_name(&self) -> &str {
            &self.1
        }
    }

    fn dummy_ctx() -> IndexContext {
        IndexContext {
            block: crate::types::BlockSummary {
                number: 1,
                hash: "0xa".into(),
                parent_hash: "0x0".into(),
                timestamp: 0,
                tx_count: 0,
            },
            phase: crate::types::IndexPhase::Backfill,
            chain: "ethereum".into(),
        }
    }

    fn dummy_event(schema: &str) -> DecodedEvent {
        DecodedEvent {
            schema: schema.to_string(),
            address: "0x0".into(),
            tx_hash: "0x0".into(),
            block_number: 1,
            log_index: 0,
            fields_json: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn event_handler_dispatch() {
        let count = Arc::new(AtomicU32::new(0));
        let handler = Arc::new(Counter(count.clone(), "ERC20Transfer".into()));

        let mut registry = HandlerRegistry::new();
        registry.on_event(handler);

        let ctx = dummy_ctx();
        registry.dispatch_event(&dummy_event("ERC20Transfer"), &ctx).await.unwrap();
        registry.dispatch_event(&dummy_event("UniswapSwap"), &ctx).await.unwrap(); // no handler

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
