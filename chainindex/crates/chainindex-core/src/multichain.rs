//! Multi-chain indexer coordinator.
//!
//! Manages multiple [`IndexerConfig`] instances — one per chain — from a single
//! engine. The coordinator tracks runtime state for each chain, provides health
//! reporting, and aggregates events from all chains onto a shared broadcast bus.
//!
//! # Example
//!
//! ```rust,no_run
//! use chainindex_core::multichain::{MultiChainConfig, MultiChainCoordinator, CrossChainEventBus};
//! use chainindex_core::indexer::IndexerConfig;
//! use std::time::Duration;
//!
//! let eth_cfg = IndexerConfig { id: "eth-main".into(), chain: "ethereum".into(), ..Default::default() };
//! let arb_cfg = IndexerConfig { id: "arb-main".into(), chain: "arbitrum".into(), ..Default::default() };
//!
//! let config = MultiChainConfig {
//!     chains: vec![eth_cfg, arb_cfg],
//!     max_concurrent_chains: 4,
//!     health_check_interval: Duration::from_secs(30),
//!     restart_on_error: true,
//!     restart_delay: Duration::from_secs(5),
//! };
//!
//! let coordinator = MultiChainCoordinator::new(config);
//! let bus = CrossChainEventBus::new(1024);
//! let mut rx = bus.subscribe();
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use std::sync::Arc;

use crate::error::IndexerError;
use crate::handler::DecodedEvent;
use crate::indexer::{IndexerConfig, IndexerState};

// ─── ChainInstance ────────────────────────────────────────────────────────────

/// Runtime state for a single chain managed by the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInstance {
    /// The configuration driving this chain.
    pub config: IndexerConfig,
    /// Current runtime state of this chain's indexer.
    pub state: IndexerState,
    /// Latest block number that has been fully processed.
    pub head_block: u64,
    /// Total number of events emitted so far by this chain.
    pub events_processed: u64,
    /// The last error message if `state == IndexerState::Error`.
    pub last_error: Option<String>,
    /// Unix timestamp (seconds) when the chain was started, if it has been.
    pub started_at: Option<i64>,
}

impl ChainInstance {
    /// Construct a new idle instance from a config.
    pub fn new(config: IndexerConfig) -> Self {
        Self {
            config,
            state: IndexerState::Idle,
            head_block: 0,
            events_processed: 0,
            last_error: None,
            started_at: None,
        }
    }

    /// Returns `true` when the chain is actively making progress.
    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            IndexerState::Backfilling | IndexerState::Live | IndexerState::ReorgRecovery
        )
    }

    /// Returns `true` when the chain is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self.state, IndexerState::Error)
    }

    /// Transition to a new state, recording errors if appropriate.
    pub fn transition(&mut self, new_state: IndexerState, error: Option<String>) {
        self.state = new_state;
        if new_state == IndexerState::Error {
            self.last_error = error;
        } else if error.is_none() && !matches!(new_state, IndexerState::Error) {
            // Clear error when transitioning away from error state
            self.last_error = None;
        }
    }
}

// ─── MultiChainConfig ─────────────────────────────────────────────────────────

/// Configuration for the multi-chain coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiChainConfig {
    /// One [`IndexerConfig`] per chain to manage.
    pub chains: Vec<IndexerConfig>,
    /// Maximum number of chains that may run concurrently. `0` means unlimited
    /// (all chains run simultaneously).
    pub max_concurrent_chains: usize,
    /// How often the coordinator evaluates chain health. Default: 30 s.
    pub health_check_interval: Duration,
    /// If `true`, a chain that enters [`IndexerState::Error`] is automatically
    /// restarted after `restart_delay`. Default: `true`.
    pub restart_on_error: bool,
    /// Delay before automatically restarting a failed chain. Default: 5 s.
    pub restart_delay: Duration,
}

impl Default for MultiChainConfig {
    fn default() -> Self {
        Self {
            chains: vec![],
            max_concurrent_chains: 0, // unlimited
            health_check_interval: Duration::from_secs(30),
            restart_on_error: true,
            restart_delay: Duration::from_secs(5),
        }
    }
}

impl MultiChainConfig {
    /// Returns an error string if the configuration is invalid, otherwise `None`.
    pub fn validate(&self) -> Option<String> {
        if self.health_check_interval.is_zero() {
            return Some("health_check_interval must be non-zero".into());
        }
        // Check for duplicate chain IDs.
        let mut seen = std::collections::HashSet::new();
        for cfg in &self.chains {
            if !seen.insert(&cfg.id) {
                return Some(format!("duplicate chain id '{}'", cfg.id));
            }
        }
        None
    }
}

// ─── ChainHealth ─────────────────────────────────────────────────────────────

/// Health snapshot for a single chain instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainHealth {
    /// Chain identifier matching [`IndexerConfig::id`].
    pub chain: String,
    /// Current indexer state.
    pub state: IndexerState,
    /// Latest processed block number.
    pub head_block: u64,
    /// Total events processed since start.
    pub events_processed: u64,
    /// Approximate blocks behind the chain tip (0 when caught up or unknown).
    pub block_lag: u64,
    /// How long the chain has been running.
    pub uptime: Duration,
    /// Last error message, if any.
    pub last_error: Option<String>,
    /// `true` when the chain is actively running without any error.
    pub is_healthy: bool,
}

impl ChainHealth {
    fn from_instance(instance: &ChainInstance, now_secs: i64) -> Self {
        let uptime = instance
            .started_at
            .map(|s| Duration::from_secs(now_secs.saturating_sub(s).max(0) as u64))
            .unwrap_or(Duration::ZERO);

        let is_healthy = instance.is_active() && instance.last_error.is_none();

        Self {
            chain: instance.config.id.clone(),
            state: instance.state,
            head_block: instance.head_block,
            events_processed: instance.events_processed,
            block_lag: 0, // populated externally when tip info is available
            uptime,
            last_error: instance.last_error.clone(),
            is_healthy,
        }
    }
}

// ─── MultiChainCoordinator ───────────────────────────────────────────────────

/// Tracks and coordinates multiple chain indexer instances.
///
/// This is a **state-management** layer: it stores the runtime state of each
/// chain and provides query/mutation primitives. The actual indexer tasks
/// (which require an RPC provider) are started by the caller using the config
/// returned from [`chain_state`].
pub struct MultiChainCoordinator {
    config: MultiChainConfig,
    /// Keyed by [`IndexerConfig::id`].
    instances: RwLock<HashMap<String, ChainInstance>>,
    /// Wall-clock start time, used to compute uptimes.
    started: Instant,
}

impl MultiChainCoordinator {
    /// Create a new coordinator from the given config.
    ///
    /// All chains in `config.chains` are registered in the [`IndexerState::Idle`]
    /// state — they are not started automatically.
    pub fn new(config: MultiChainConfig) -> Self {
        let mut instances = HashMap::new();
        for chain_cfg in &config.chains {
            instances.insert(chain_cfg.id.clone(), ChainInstance::new(chain_cfg.clone()));
        }
        Self {
            config,
            instances: RwLock::new(instances),
            started: Instant::now(),
        }
    }

    // ── Chain lifecycle ────────────────────────────────────────────────────

    /// Register a new chain at runtime.
    ///
    /// Returns an error if a chain with the same `id` already exists.
    pub async fn add_chain(&self, config: IndexerConfig) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        if guard.contains_key(&config.id) {
            return Err(IndexerError::Other(format!(
                "chain '{}' already registered",
                config.id
            )));
        }
        guard.insert(config.id.clone(), ChainInstance::new(config));
        Ok(())
    }

    /// Remove a chain from the coordinator.
    ///
    /// The caller is responsible for ensuring the underlying task is stopped
    /// before calling this method. Returns an error if the chain is not found.
    pub async fn remove_chain(&self, chain_id: &str) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        if guard.remove(chain_id).is_none() {
            return Err(IndexerError::Other(format!(
                "chain '{}' not found",
                chain_id
            )));
        }
        Ok(())
    }

    /// Pause a chain by transitioning it to [`IndexerState::Stopping`].
    ///
    /// Only chains in an active state (Backfilling, Live, ReorgRecovery) can
    /// be paused. Returns an error if the chain is not found or not active.
    pub async fn pause_chain(&self, chain_id: &str) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        let instance = guard.get_mut(chain_id).ok_or_else(|| {
            IndexerError::Other(format!("chain '{}' not found", chain_id))
        })?;
        if !instance.is_active() {
            return Err(IndexerError::Other(format!(
                "chain '{}' is not active (state: {})",
                chain_id, instance.state
            )));
        }
        instance.transition(IndexerState::Stopping, None);
        tracing::info!(chain = %chain_id, "pausing chain");
        Ok(())
    }

    /// Resume a paused or stopped chain by transitioning it back to
    /// [`IndexerState::Backfilling`].
    ///
    /// Returns an error if the chain is not found or is already active.
    pub async fn resume_chain(&self, chain_id: &str) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        let instance = guard.get_mut(chain_id).ok_or_else(|| {
            IndexerError::Other(format!("chain '{}' not found", chain_id))
        })?;
        if instance.is_active() {
            return Err(IndexerError::Other(format!(
                "chain '{}' is already active (state: {})",
                chain_id, instance.state
            )));
        }
        instance.transition(IndexerState::Backfilling, None);
        if instance.started_at.is_none() {
            instance.started_at = Some(chrono::Utc::now().timestamp());
        }
        tracing::info!(chain = %chain_id, "resuming chain");
        Ok(())
    }

    // ── State mutations ────────────────────────────────────────────────────

    /// Update the state of a chain. Called by the underlying indexer task.
    pub async fn update_state(
        &self,
        chain_id: &str,
        new_state: IndexerState,
        error: Option<String>,
    ) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        let instance = guard.get_mut(chain_id).ok_or_else(|| {
            IndexerError::Other(format!("chain '{}' not found", chain_id))
        })?;
        if new_state == IndexerState::Backfilling || new_state == IndexerState::Live {
            if instance.started_at.is_none() {
                instance.started_at = Some(chrono::Utc::now().timestamp());
            }
        }
        instance.transition(new_state, error);
        Ok(())
    }

    /// Record a new block processed by a chain.
    pub async fn record_block(
        &self,
        chain_id: &str,
        block_number: u64,
        events: u64,
    ) -> Result<(), IndexerError> {
        let mut guard = self.instances.write().await;
        let instance = guard.get_mut(chain_id).ok_or_else(|| {
            IndexerError::Other(format!("chain '{}' not found", chain_id))
        })?;
        if block_number > instance.head_block {
            instance.head_block = block_number;
        }
        instance.events_processed += events;
        Ok(())
    }

    // ── Queries ────────────────────────────────────────────────────────────

    /// Returns health snapshots for every registered chain.
    pub async fn health(&self) -> Vec<ChainHealth> {
        let guard = self.instances.read().await;
        let now = chrono::Utc::now().timestamp();
        guard
            .values()
            .map(|inst| ChainHealth::from_instance(inst, now))
            .collect()
    }

    /// Returns health for a specific chain by id.
    pub async fn chain_health(&self, chain_id: &str) -> Option<ChainHealth> {
        let guard = self.instances.read().await;
        let now = chrono::Utc::now().timestamp();
        guard
            .get(chain_id)
            .map(|inst| ChainHealth::from_instance(inst, now))
    }

    /// Returns a clone of the runtime state for a single chain.
    pub async fn chain_state(&self, chain_id: &str) -> Option<ChainInstance> {
        let guard = self.instances.read().await;
        guard.get(chain_id).cloned()
    }

    /// Returns the ids of all registered chains.
    pub async fn chains(&self) -> Vec<String> {
        let guard = self.instances.read().await;
        guard.keys().cloned().collect()
    }

    /// Returns the ids of all chains that are actively indexing.
    pub async fn active_chains(&self) -> Vec<String> {
        let guard = self.instances.read().await;
        guard
            .iter()
            .filter(|(_, inst)| inst.is_active())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Returns `true` when every registered chain is healthy (active, no error).
    pub async fn is_all_healthy(&self) -> bool {
        let guard = self.instances.read().await;
        guard.values().all(|inst| inst.is_active() && inst.last_error.is_none())
    }

    /// Returns `true` when every registered chain has reached at least
    /// `min_block`.
    pub async fn all_past_block(&self, min_block: u64) -> bool {
        let guard = self.instances.read().await;
        guard.values().all(|inst| inst.head_block >= min_block)
    }

    /// Returns the number of registered chains.
    pub async fn chain_count(&self) -> usize {
        self.instances.read().await.len()
    }

    /// Returns the coordinator config.
    pub fn config(&self) -> &MultiChainConfig {
        &self.config
    }

    /// Returns how long the coordinator has been running.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }
}

// ─── CrossChainEvent ─────────────────────────────────────────────────────────

/// An event received from any of the managed chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainEvent {
    /// Chain identifier that produced this event.
    pub chain: String,
    /// The decoded event payload.
    pub event: DecodedEvent,
    /// Unix timestamp (seconds) at which the event was received by the bus.
    pub received_at: i64,
}

/// Type alias for the receive half of a [`CrossChainEventBus`] subscription.
pub type CrossChainReceiver = broadcast::Receiver<CrossChainEvent>;

// ─── CrossChainEventBus ──────────────────────────────────────────────────────

/// Fan-out broadcast bus that aggregates events from all managed chains.
///
/// Multiple subscribers can each receive every event via
/// `tokio::sync::broadcast`. If a subscriber falls behind and the buffer
/// fills, lagged events are dropped for that subscriber (the channel returns
/// [`broadcast::error::RecvError::Lagged`]).
#[derive(Clone)]
pub struct CrossChainEventBus {
    sender: broadcast::Sender<CrossChainEvent>,
}

impl CrossChainEventBus {
    /// Create a new bus with the given channel capacity.
    ///
    /// `capacity` is the maximum number of events buffered per subscriber
    /// before the oldest events are dropped. A value of 1024 is a reasonable
    /// default for most use cases.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Push an event from `chain` onto the bus.
    ///
    /// Returns the number of active subscribers that received the event.
    /// If there are no subscribers, the event is silently discarded.
    pub fn push(&self, chain: &str, event: DecodedEvent) -> usize {
        let cross = CrossChainEvent {
            chain: chain.to_string(),
            event,
            received_at: chrono::Utc::now().timestamp(),
        };
        // `send` only fails when there are no receivers — that is fine.
        self.sender.send(cross).unwrap_or(0)
    }

    /// Subscribe to the event bus.
    ///
    /// Each subscriber receives a clone of every event pushed after the
    /// subscription is created.
    pub fn subscribe(&self) -> CrossChainReceiver {
        self.sender.subscribe()
    }

    /// Returns the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

// ─── ChainSyncStatus ─────────────────────────────────────────────────────────

/// Cross-chain synchronization tracker.
///
/// Stores the current head block for each chain so callers can ask questions
/// like "have all chains passed block N?" or "are all chains within K blocks
/// of their respective tips?"
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChainSyncStatus {
    /// Maps chain id → latest known head block.
    pub chains: HashMap<String, u64>,
    /// Optional tip (chain head) for each chain, used by `all_caught_up`.
    tips: HashMap<String, u64>,
    /// Stores the last known block timestamp per chain (unix seconds).
    timestamps: HashMap<String, i64>,
}

impl ChainSyncStatus {
    /// Create an empty sync status tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the head block for a chain.
    pub fn update(&mut self, chain: &str, head: u64) {
        self.chains.insert(chain.to_string(), head);
    }

    /// Update both the head block and its timestamp for a chain.
    pub fn update_with_timestamp(&mut self, chain: &str, head: u64, timestamp: i64) {
        self.chains.insert(chain.to_string(), head);
        self.timestamps.insert(chain.to_string(), timestamp);
    }

    /// Update the known chain tip (latest block on the network) for a chain.
    pub fn update_tip(&mut self, chain: &str, tip: u64) {
        self.tips.insert(chain.to_string(), tip);
    }

    /// Returns the earliest (minimum) timestamp across all chains with a
    /// recorded timestamp. Returns `None` if no timestamps are recorded.
    pub fn min_timestamp(&self) -> Option<i64> {
        self.timestamps.values().copied().reduce(i64::min)
    }

    /// Returns the latest (maximum) timestamp across all chains.
    pub fn max_timestamp(&self) -> Option<i64> {
        self.timestamps.values().copied().reduce(i64::max)
    }

    /// Returns `true` if every registered chain has processed past `block`.
    ///
    /// Returns `false` if there are no chains registered.
    pub fn all_past_block(&self, _chain: &str, block: u64) -> bool {
        if self.chains.is_empty() {
            return false;
        }
        self.chains.values().all(|&head| head >= block)
    }

    /// Returns `true` if all chains are within `threshold_blocks` of their
    /// recorded tips.
    ///
    /// Returns `false` when no chains have tip information recorded.
    pub fn all_caught_up(&self, threshold_blocks: u64) -> bool {
        if self.tips.is_empty() {
            return false;
        }
        for (chain, &tip) in &self.tips {
            let head = self.chains.get(chain).copied().unwrap_or(0);
            if tip.saturating_sub(head) > threshold_blocks {
                return false;
            }
        }
        true
    }

    /// Returns the head block for a specific chain.
    pub fn head_of(&self, chain: &str) -> Option<u64> {
        self.chains.get(chain).copied()
    }

    /// Returns the lag (tip - head) for a specific chain. `None` if the chain
    /// or its tip is not recorded.
    pub fn lag_of(&self, chain: &str) -> Option<u64> {
        let head = self.chains.get(chain).copied()?;
        let tip = self.tips.get(chain).copied()?;
        Some(tip.saturating_sub(head))
    }

    /// Returns the number of chains being tracked.
    pub fn len(&self) -> usize {
        self.chains.len()
    }

    /// Returns `true` when no chains are tracked.
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }

    /// Build a [`ChainSyncStatus`] snapshot from a [`MultiChainCoordinator`].
    pub async fn from_coordinator(coordinator: &Arc<MultiChainCoordinator>) -> Self {
        let guard = coordinator.instances.read().await;
        let mut status = Self::new();
        for (id, inst) in guard.iter() {
            status.update(id, inst.head_block);
        }
        status
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventFilter;

    // ── helpers ───────────────────────────────────────────────────────────

    fn make_config(id: &str, chain: &str) -> IndexerConfig {
        IndexerConfig {
            id: id.into(),
            chain: chain.into(),
            from_block: 0,
            to_block: None,
            confirmation_depth: 12,
            batch_size: 1000,
            checkpoint_interval: 100,
            poll_interval_ms: 2000,
            filter: EventFilter::default(),
        }
    }

    fn make_coordinator(ids: &[(&str, &str)]) -> MultiChainCoordinator {
        let chains: Vec<IndexerConfig> = ids
            .iter()
            .map(|(id, chain)| make_config(id, chain))
            .collect();
        MultiChainCoordinator::new(MultiChainConfig {
            chains,
            ..Default::default()
        })
    }

    fn dummy_event(chain: &str) -> DecodedEvent {
        DecodedEvent {
            chain: chain.into(),
            schema: "ERC20Transfer".into(),
            address: "0xabc".into(),
            tx_hash: "0xdeadbeef".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({"from": "0x1", "to": "0x2", "value": "1000"}),
        }
    }

    // ── MultiChainConfig ──────────────────────────────────────────────────

    #[test]
    fn multichain_config_defaults() {
        let cfg = MultiChainConfig::default();
        assert!(cfg.chains.is_empty());
        assert_eq!(cfg.max_concurrent_chains, 0);
        assert_eq!(cfg.health_check_interval, Duration::from_secs(30));
        assert!(cfg.restart_on_error);
        assert_eq!(cfg.restart_delay, Duration::from_secs(5));
    }

    #[test]
    fn multichain_config_validate_ok() {
        let cfg = MultiChainConfig {
            chains: vec![
                make_config("eth", "ethereum"),
                make_config("arb", "arbitrum"),
            ],
            ..Default::default()
        };
        assert!(cfg.validate().is_none());
    }

    #[test]
    fn multichain_config_validate_duplicate_id() {
        let cfg = MultiChainConfig {
            chains: vec![
                make_config("eth", "ethereum"),
                make_config("eth", "arbitrum"), // duplicate id
            ],
            ..Default::default()
        };
        let err = cfg.validate().expect("should report duplicate");
        assert!(err.contains("duplicate chain id 'eth'"));
    }

    #[test]
    fn multichain_config_validate_zero_interval() {
        let cfg = MultiChainConfig {
            health_check_interval: Duration::ZERO,
            ..Default::default()
        };
        let err = cfg.validate().expect("should report invalid interval");
        assert!(err.contains("health_check_interval"));
    }

    // ── Coordinator add/remove ────────────────────────────────────────────

    #[tokio::test]
    async fn coordinator_add_chain() {
        let coord = make_coordinator(&[]);
        coord.add_chain(make_config("eth", "ethereum")).await.unwrap();
        assert_eq!(coord.chain_count().await, 1);
    }

    #[tokio::test]
    async fn coordinator_add_duplicate_chain_errors() {
        let coord = make_coordinator(&[("eth", "ethereum")]);
        let err = coord.add_chain(make_config("eth", "ethereum")).await.unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[tokio::test]
    async fn coordinator_remove_chain() {
        let coord = make_coordinator(&[("eth", "ethereum"), ("arb", "arbitrum")]);
        coord.remove_chain("eth").await.unwrap();
        assert_eq!(coord.chain_count().await, 1);
        assert!(coord.chain_state("eth").await.is_none());
    }

    #[tokio::test]
    async fn coordinator_remove_missing_chain_errors() {
        let coord = make_coordinator(&[]);
        let err = coord.remove_chain("unknown").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    // ── Pause / resume ────────────────────────────────────────────────────

    #[tokio::test]
    async fn coordinator_pause_and_resume() {
        let coord = make_coordinator(&[("eth", "ethereum")]);

        // Transition to Live so we can pause
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        assert!(coord.chain_state("eth").await.unwrap().is_active());

        // Pause
        coord.pause_chain("eth").await.unwrap();
        let inst = coord.chain_state("eth").await.unwrap();
        assert_eq!(inst.state, IndexerState::Stopping);

        // Cannot pause again (not active)
        let err = coord.pause_chain("eth").await.unwrap_err();
        assert!(err.to_string().contains("not active"));

        // Resume
        coord.resume_chain("eth").await.unwrap();
        let inst = coord.chain_state("eth").await.unwrap();
        assert_eq!(inst.state, IndexerState::Backfilling);
    }

    #[tokio::test]
    async fn coordinator_resume_already_active_errors() {
        let coord = make_coordinator(&[("eth", "ethereum")]);
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        let err = coord.resume_chain("eth").await.unwrap_err();
        assert!(err.to_string().contains("already active"));
    }

    // ── Health reporting ──────────────────────────────────────────────────

    #[tokio::test]
    async fn health_reflects_state() {
        let coord = make_coordinator(&[("eth", "ethereum"), ("arb", "arbitrum")]);
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        coord
            .update_state("arb", IndexerState::Error, Some("rpc timeout".into()))
            .await
            .unwrap();

        let health = coord.health().await;
        assert_eq!(health.len(), 2);

        let eth_h = health.iter().find(|h| h.chain == "eth").unwrap();
        assert!(eth_h.is_healthy);
        assert_eq!(eth_h.state, IndexerState::Live);

        let arb_h = health.iter().find(|h| h.chain == "arb").unwrap();
        assert!(!arb_h.is_healthy);
        assert_eq!(arb_h.last_error.as_deref(), Some("rpc timeout"));
    }

    #[tokio::test]
    async fn is_all_healthy_false_when_error() {
        let coord = make_coordinator(&[("eth", "ethereum"), ("arb", "arbitrum")]);
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        coord
            .update_state("arb", IndexerState::Error, Some("crash".into()))
            .await
            .unwrap();
        assert!(!coord.is_all_healthy().await);
    }

    #[tokio::test]
    async fn is_all_healthy_true_when_all_live() {
        let coord = make_coordinator(&[("eth", "ethereum"), ("arb", "arbitrum")]);
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        coord
            .update_state("arb", IndexerState::Live, None)
            .await
            .unwrap();
        assert!(coord.is_all_healthy().await);
    }

    // ── Active chains listing ─────────────────────────────────────────────

    #[tokio::test]
    async fn active_chains_filters_correctly() {
        let coord =
            make_coordinator(&[("eth", "ethereum"), ("arb", "arbitrum"), ("sol", "solana")]);
        coord
            .update_state("eth", IndexerState::Live, None)
            .await
            .unwrap();
        coord
            .update_state("arb", IndexerState::Backfilling, None)
            .await
            .unwrap();
        // sol stays Idle

        let active = coord.active_chains().await;
        assert_eq!(active.len(), 2);
        assert!(active.contains(&"eth".to_string()));
        assert!(active.contains(&"arb".to_string()));
        assert!(!active.contains(&"sol".to_string()));
    }

    // ── Error state handling ──────────────────────────────────────────────

    #[tokio::test]
    async fn error_state_records_message() {
        let coord = make_coordinator(&[("eth", "ethereum")]);
        coord
            .update_state(
                "eth",
                IndexerState::Error,
                Some("connection refused".into()),
            )
            .await
            .unwrap();

        let inst = coord.chain_state("eth").await.unwrap();
        assert_eq!(inst.state, IndexerState::Error);
        assert_eq!(inst.last_error.as_deref(), Some("connection refused"));
    }

    #[tokio::test]
    async fn error_cleared_on_resume() {
        let coord = make_coordinator(&[("eth", "ethereum")]);
        // Set error state
        coord
            .update_state("eth", IndexerState::Error, Some("boom".into()))
            .await
            .unwrap();
        // Resume (transitions to Backfilling, clears error)
        coord.resume_chain("eth").await.unwrap();

        let inst = coord.chain_state("eth").await.unwrap();
        assert_eq!(inst.state, IndexerState::Backfilling);
        assert!(inst.last_error.is_none());
    }

    // ── ChainInstance state transitions ───────────────────────────────────

    #[test]
    fn chain_instance_state_transitions() {
        let cfg = make_config("eth", "ethereum");
        let mut inst = ChainInstance::new(cfg);

        assert_eq!(inst.state, IndexerState::Idle);
        assert!(!inst.is_active());
        assert!(!inst.is_error());

        inst.transition(IndexerState::Backfilling, None);
        assert!(inst.is_active());

        inst.transition(IndexerState::Live, None);
        assert!(inst.is_active());

        inst.transition(IndexerState::ReorgRecovery, None);
        assert!(inst.is_active());

        inst.transition(IndexerState::Error, Some("test error".into()));
        assert!(!inst.is_active());
        assert!(inst.is_error());
        assert_eq!(inst.last_error.as_deref(), Some("test error"));

        // Transitioning back to Backfilling clears error
        inst.transition(IndexerState::Backfilling, None);
        assert!(inst.is_active());
        assert!(inst.last_error.is_none());
    }

    // ── Cross-chain event bus ─────────────────────────────────────────────

    #[tokio::test]
    async fn event_bus_push_and_subscribe() {
        let bus = CrossChainEventBus::new(64);
        let mut rx = bus.subscribe();

        let event = dummy_event("ethereum");
        bus.push("ethereum", event.clone());

        let received = rx.recv().await.unwrap();
        assert_eq!(received.chain, "ethereum");
        assert_eq!(received.event.schema, "ERC20Transfer");
        assert_eq!(received.event.tx_hash, "0xdeadbeef");
    }

    #[tokio::test]
    async fn event_bus_multiple_subscribers() {
        let bus = CrossChainEventBus::new(64);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.push("arbitrum", dummy_event("arbitrum"));

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.chain, "arbitrum");
        assert_eq!(e2.chain, "arbitrum");
    }

    #[tokio::test]
    async fn event_bus_no_subscribers_does_not_panic() {
        let bus = CrossChainEventBus::new(16);
        // push with no subscribers — should silently succeed
        let count = bus.push("ethereum", dummy_event("ethereum"));
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn event_bus_received_at_is_populated() {
        let bus = CrossChainEventBus::new(16);
        let mut rx = bus.subscribe();
        bus.push("ethereum", dummy_event("ethereum"));
        let ev = rx.recv().await.unwrap();
        assert!(ev.received_at > 0);
    }

    // ── ChainSyncStatus ───────────────────────────────────────────────────

    #[test]
    fn sync_status_update_and_query() {
        let mut status = ChainSyncStatus::new();
        status.update("ethereum", 1_000_000);
        status.update("arbitrum", 200_000_000);

        assert_eq!(status.head_of("ethereum"), Some(1_000_000));
        assert_eq!(status.head_of("arbitrum"), Some(200_000_000));
        assert_eq!(status.head_of("unknown"), None);
    }

    #[test]
    fn sync_status_all_past_block() {
        let mut status = ChainSyncStatus::new();
        status.update("eth", 1000);
        status.update("arb", 2000);
        status.update("sol", 500);

        // All past 400 → true
        assert!(status.all_past_block("", 400));
        // sol is at 500, not past 600 → false
        assert!(!status.all_past_block("", 600));
    }

    #[test]
    fn sync_status_all_caught_up() {
        let mut status = ChainSyncStatus::new();
        status.update("eth", 990);
        status.update_tip("eth", 1000);
        status.update("arb", 199_990);
        status.update_tip("arb", 200_000);

        // Both within 20 blocks → caught up with threshold 20
        assert!(status.all_caught_up(20));
        // Not within 5 blocks (lag=10 and lag=10 > 5)
        assert!(!status.all_caught_up(5));
    }

    #[test]
    fn sync_status_min_timestamp() {
        let mut status = ChainSyncStatus::new();
        status.update_with_timestamp("eth", 1000, 1_700_000_100);
        status.update_with_timestamp("arb", 2000, 1_700_000_050);
        status.update_with_timestamp("sol", 3000, 1_700_000_200);

        assert_eq!(status.min_timestamp(), Some(1_700_000_050));
    }

    #[test]
    fn sync_status_min_timestamp_none_when_empty() {
        let status = ChainSyncStatus::new();
        assert!(status.min_timestamp().is_none());
    }

    #[test]
    fn sync_status_lag_of() {
        let mut status = ChainSyncStatus::new();
        status.update("eth", 990);
        status.update_tip("eth", 1000);

        assert_eq!(status.lag_of("eth"), Some(10));
        assert_eq!(status.lag_of("unknown"), None);
    }

    #[test]
    fn sync_status_all_caught_up_no_tips_returns_false() {
        let mut status = ChainSyncStatus::new();
        status.update("eth", 1000);
        // No tips recorded
        assert!(!status.all_caught_up(10));
    }

    #[test]
    fn sync_status_is_empty() {
        let status = ChainSyncStatus::new();
        assert!(status.is_empty());
        let mut status = ChainSyncStatus::new();
        status.update("eth", 0);
        assert!(!status.is_empty());
        assert_eq!(status.len(), 1);
    }
}
