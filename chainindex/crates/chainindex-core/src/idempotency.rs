//! Idempotency framework — ensures handlers produce correct state
//! even when replayed after chain reorganizations.
//!
//! # Key concepts
//!
//! - **Deterministic entity IDs**: `{tx_hash}-{log_index}` ensures the same
//!   event always produces the same entity ID, regardless of how many times
//!   it is replayed.
//! - **Reorg replay detection**: handlers receive a [`ReplayContext`] that
//!   tells them whether the current execution is a replay of blocks that
//!   were previously processed (after a reorg rollback).
//! - **Side effect guard**: [`SideEffectGuard`] lets handlers skip external
//!   API calls (webhooks, notifications) during replays while still
//!   rebuilding local state correctly.
//!
//! # Example
//!
//! ```rust
//! use chainindex_core::idempotency::{deterministic_id, ReplayContext, SideEffectGuard};
//! use chainindex_core::handler::DecodedEvent;
//!
//! let event = DecodedEvent {
//!     chain: "ethereum".into(),
//!     schema: "ERC20Transfer".into(),
//!     address: "0xdead".into(),
//!     tx_hash: "0xabc123".into(),
//!     block_number: 100,
//!     log_index: 3,
//!     fields_json: serde_json::json!({}),
//! };
//!
//! let id = deterministic_id(&event);
//! assert_eq!(id, "0xabc123-3");
//! ```

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::IndexerError;
use crate::handler::{DecodedEvent, EventHandler};
use crate::types::IndexContext;

// ─── Deterministic ID generation ─────────────────────────────────────────────

/// Generate a deterministic entity ID from an event.
///
/// Format: `{tx_hash}-{log_index}`
///
/// This guarantees that the same on-chain event always maps to the same
/// entity ID, making upsert operations idempotent across replays.
pub fn deterministic_id(event: &DecodedEvent) -> String {
    format!("{}-{}", event.tx_hash, event.log_index)
}

/// Generate a deterministic entity ID with a custom suffix.
///
/// Format: `{tx_hash}-{log_index}-{suffix}`
///
/// Useful when a single event produces multiple entities (e.g., a swap
/// event creates both a "buy" and "sell" entity).
pub fn deterministic_id_with_suffix(event: &DecodedEvent, suffix: &str) -> String {
    format!("{}-{}-{}", event.tx_hash, event.log_index, suffix)
}

// ─── ReplayContext ───────────────────────────────────────────────────────────

/// Context about whether the current execution is a reorg replay.
///
/// Passed to handlers so they can adjust their behavior during replay.
/// For example, handlers should skip sending webhooks or notifications
/// during replay, but still update local entity state.
#[derive(Debug, Clone)]
pub struct ReplayContext {
    /// `true` if the indexer is replaying blocks after a reorg.
    pub is_replay: bool,
    /// The block number where the reorg was detected (fork point).
    /// `None` if this is not a replay.
    pub reorg_from_block: Option<u64>,
    /// The original block hash that was replaced by the reorg.
    /// `None` if this is not a replay.
    pub original_block_hash: Option<String>,
}

impl ReplayContext {
    /// Create a normal (non-replay) context.
    pub fn normal() -> Self {
        Self {
            is_replay: false,
            reorg_from_block: None,
            original_block_hash: None,
        }
    }

    /// Create a replay context for a reorg.
    pub fn replay(reorg_from_block: u64, original_block_hash: Option<String>) -> Self {
        Self {
            is_replay: true,
            reorg_from_block: Some(reorg_from_block),
            original_block_hash,
        }
    }
}

// ─── SideEffectGuard ─────────────────────────────────────────────────────────

/// Guard that tracks whether side effects should be executed.
///
/// During normal indexing, side effects (webhooks, external API calls)
/// are executed. During replay after a reorg, side effects are skipped
/// because they were already executed during the original processing.
///
/// # Example
///
/// ```rust
/// use chainindex_core::idempotency::{ReplayContext, SideEffectGuard};
///
/// let ctx = ReplayContext::normal();
/// let guard = SideEffectGuard::new(&ctx);
/// assert!(guard.should_execute()); // normal mode: execute side effects
///
/// let replay_ctx = ReplayContext::replay(100, None);
/// let replay_guard = SideEffectGuard::new(&replay_ctx);
/// assert!(!replay_guard.should_execute()); // replay mode: skip side effects
/// ```
pub struct SideEffectGuard {
    /// Whether side effects should be executed.
    execute: bool,
}

impl SideEffectGuard {
    /// Create a new guard based on the replay context.
    ///
    /// Side effects are skipped when `replay_ctx.is_replay` is `true`.
    pub fn new(replay_ctx: &ReplayContext) -> Self {
        Self {
            execute: !replay_ctx.is_replay,
        }
    }

    /// Returns `true` if side effects should be executed (not in replay mode).
    pub fn should_execute(&self) -> bool {
        self.execute
    }

    /// Execute a side effect only if not in replay mode.
    ///
    /// Returns `Some(result)` if the side effect was executed, or `None`
    /// if it was skipped (replay mode).
    pub async fn execute<F, Fut, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        if self.execute {
            Some(f().await)
        } else {
            None
        }
    }
}

// ─── IdempotentHandler ───────────────────────────────────────────────────────

/// Wrapper around an [`EventHandler`] that adds idempotency tracking.
///
/// The `IdempotentHandler` wraps an inner handler and:
/// 1. Generates a deterministic event ID before calling the inner handler.
/// 2. Tracks which events have been processed (by their deterministic ID).
/// 3. In normal mode, always calls the inner handler (for upsert semantics).
/// 4. Provides the [`ReplayContext`] for the inner handler to use.
///
/// Note: the inner handler is always called even for already-seen events,
/// because entity upsert is the correct idempotent behavior. The tracking
/// is for observability and metrics, not for skipping events.
pub struct IdempotentHandler {
    /// The wrapped event handler.
    inner: Arc<dyn EventHandler>,
    /// Current replay context.
    replay_ctx: ReplayContext,
    /// Set of processed event IDs (for tracking/metrics).
    processed_ids: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl IdempotentHandler {
    /// Create a new idempotent handler wrapping the given event handler.
    pub fn new(inner: Arc<dyn EventHandler>, replay_ctx: ReplayContext) -> Self {
        Self {
            inner,
            replay_ctx,
            processed_ids: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    /// Returns the current replay context.
    pub fn replay_context(&self) -> &ReplayContext {
        &self.replay_ctx
    }

    /// Returns the number of events processed by this handler.
    pub fn processed_count(&self) -> usize {
        self.processed_ids
            .lock()
            .map(|ids| ids.len())
            .unwrap_or(0)
    }

    /// Returns `true` if an event with the given deterministic ID has been processed.
    pub fn has_processed(&self, event_id: &str) -> bool {
        self.processed_ids
            .lock()
            .map(|ids| ids.contains(event_id))
            .unwrap_or(false)
    }

    /// Create a [`SideEffectGuard`] for this handler's replay context.
    pub fn side_effect_guard(&self) -> SideEffectGuard {
        SideEffectGuard::new(&self.replay_ctx)
    }
}

#[async_trait]
impl EventHandler for IdempotentHandler {
    async fn handle(&self, event: &DecodedEvent, ctx: &IndexContext) -> Result<(), IndexerError> {
        let event_id = deterministic_id(event);

        // Track the event ID.
        if let Ok(mut ids) = self.processed_ids.lock() {
            ids.insert(event_id);
        }

        // Always call the inner handler (upsert semantics).
        self.inner.handle(event, ctx).await
    }

    fn schema_name(&self) -> &str {
        self.inner.schema_name()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn make_event(tx_hash: &str, log_index: u32) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "ERC20Transfer".into(),
            address: "0xdead".into(),
            tx_hash: tx_hash.to_string(),
            block_number: 100,
            log_index,
            fields_json: serde_json::json!({}),
        }
    }

    fn dummy_ctx() -> IndexContext {
        IndexContext {
            block: crate::types::BlockSummary {
                number: 100,
                hash: "0xa".into(),
                parent_hash: "0x0".into(),
                timestamp: 0,
                tx_count: 0,
            },
            phase: crate::types::IndexPhase::Backfill,
            chain: "ethereum".into(),
        }
    }

    // ── deterministic_id ─────────────────────────────────────────────────

    #[test]
    fn deterministic_id_is_stable() {
        let event = make_event("0xabc123", 3);
        let id1 = deterministic_id(&event);
        let id2 = deterministic_id(&event);
        assert_eq!(id1, id2);
        assert_eq!(id1, "0xabc123-3");
    }

    #[test]
    fn different_events_get_different_ids() {
        let e1 = make_event("0xabc", 0);
        let e2 = make_event("0xabc", 1);
        let e3 = make_event("0xdef", 0);

        assert_ne!(deterministic_id(&e1), deterministic_id(&e2));
        assert_ne!(deterministic_id(&e1), deterministic_id(&e3));
    }

    #[test]
    fn deterministic_id_with_suffix_works() {
        let event = make_event("0xabc", 2);
        let id = deterministic_id_with_suffix(&event, "buy");
        assert_eq!(id, "0xabc-2-buy");

        let id2 = deterministic_id_with_suffix(&event, "sell");
        assert_eq!(id2, "0xabc-2-sell");
        assert_ne!(id, id2);
    }

    // ── ReplayContext ────────────────────────────────────────────────────

    #[test]
    fn replay_context_normal() {
        let ctx = ReplayContext::normal();
        assert!(!ctx.is_replay);
        assert!(ctx.reorg_from_block.is_none());
        assert!(ctx.original_block_hash.is_none());
    }

    #[test]
    fn replay_context_replay() {
        let ctx = ReplayContext::replay(100, Some("0xold_hash".to_string()));
        assert!(ctx.is_replay);
        assert_eq!(ctx.reorg_from_block, Some(100));
        assert_eq!(ctx.original_block_hash.as_deref(), Some("0xold_hash"));
    }

    // ── SideEffectGuard ──────────────────────────────────────────────────

    #[test]
    fn side_effect_guard_executes_normally() {
        let ctx = ReplayContext::normal();
        let guard = SideEffectGuard::new(&ctx);
        assert!(guard.should_execute());
    }

    #[test]
    fn side_effect_guard_skips_during_replay() {
        let ctx = ReplayContext::replay(100, None);
        let guard = SideEffectGuard::new(&ctx);
        assert!(!guard.should_execute());
    }

    #[tokio::test]
    async fn side_effect_guard_execute_fn_normal() {
        let ctx = ReplayContext::normal();
        let guard = SideEffectGuard::new(&ctx);

        let result = guard.execute(|| async { 42 }).await;
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn side_effect_guard_execute_fn_replay() {
        let ctx = ReplayContext::replay(100, None);
        let guard = SideEffectGuard::new(&ctx);

        let result = guard.execute(|| async { 42 }).await;
        assert_eq!(result, None);
    }

    // ── IdempotentHandler ────────────────────────────────────────────────

    struct CountingHandler {
        count: Arc<AtomicU32>,
        schema: String,
    }

    #[async_trait]
    impl EventHandler for CountingHandler {
        async fn handle(
            &self,
            _event: &DecodedEvent,
            _ctx: &IndexContext,
        ) -> Result<(), IndexerError> {
            self.count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn schema_name(&self) -> &str {
            &self.schema
        }
    }

    #[tokio::test]
    async fn idempotent_handler_wraps_inner() {
        let count = Arc::new(AtomicU32::new(0));
        let inner = Arc::new(CountingHandler {
            count: count.clone(),
            schema: "ERC20Transfer".into(),
        });

        let handler = IdempotentHandler::new(inner, ReplayContext::normal());
        assert_eq!(handler.schema_name(), "ERC20Transfer");

        let event = make_event("0xabc", 0);
        let ctx = dummy_ctx();

        handler.handle(&event, &ctx).await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(handler.processed_count(), 1);
        assert!(handler.has_processed("0xabc-0"));

        // Calling again with same event still calls inner (upsert semantics).
        handler.handle(&event, &ctx).await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 2);
        // ID is already in the set, count stays at 1 unique.
        assert_eq!(handler.processed_count(), 1);
    }

    #[tokio::test]
    async fn idempotent_handler_tracks_multiple_events() {
        let count = Arc::new(AtomicU32::new(0));
        let inner = Arc::new(CountingHandler {
            count: count.clone(),
            schema: "ERC20Transfer".into(),
        });

        let handler = IdempotentHandler::new(inner, ReplayContext::normal());
        let ctx = dummy_ctx();

        handler.handle(&make_event("0xabc", 0), &ctx).await.unwrap();
        handler.handle(&make_event("0xabc", 1), &ctx).await.unwrap();
        handler.handle(&make_event("0xdef", 0), &ctx).await.unwrap();

        assert_eq!(handler.processed_count(), 3);
        assert!(handler.has_processed("0xabc-0"));
        assert!(handler.has_processed("0xabc-1"));
        assert!(handler.has_processed("0xdef-0"));
        assert!(!handler.has_processed("0xghi-0"));
    }

    #[test]
    fn idempotent_handler_side_effect_guard_normal() {
        let inner = Arc::new(CountingHandler {
            count: Arc::new(AtomicU32::new(0)),
            schema: "Test".into(),
        });
        let handler = IdempotentHandler::new(inner, ReplayContext::normal());
        assert!(handler.side_effect_guard().should_execute());
    }

    #[test]
    fn idempotent_handler_side_effect_guard_replay() {
        let inner = Arc::new(CountingHandler {
            count: Arc::new(AtomicU32::new(0)),
            schema: "Test".into(),
        });
        let handler = IdempotentHandler::new(inner, ReplayContext::replay(100, None));
        assert!(!handler.side_effect_guard().should_execute());
    }
}
