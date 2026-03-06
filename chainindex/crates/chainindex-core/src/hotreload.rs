//! Hot-reload configuration system for the chainindex engine.
//!
//! Allows indexer configs and handler registrations to be updated at runtime
//! without restarting the indexer process.
//!
//! # Overview
//!
//! ```text
//! HotReloadManager
//!   ├── register_config(id, config)  → Arc<RwLock<ReloadableConfig>>
//!   ├── update_config(id, new)       → ReloadResult { diffs, warnings, version }
//!   ├── subscribe(id)                → watch::Receiver<u64>   (version bump)
//!   └── history(id)                  → Vec<ReloadRecord>
//!
//! ConfigWatcher  — polls a source on a fixed interval, fires callbacks on change
//! FilterReloader — fine-grained add/remove for EventFilter addresses & topic0s
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn};

use crate::error::IndexerError;
use crate::indexer::IndexerConfig;
use crate::types::EventFilter;

// ─── ConfigSource ─────────────────────────────────────────────────────────────

/// Where a configuration value originated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigSource {
    /// Built-in defaults — no explicit configuration was provided.
    Default,
    /// Loaded from a file at the given path.
    File(String),
    /// Derived from environment variables.
    Environment,
    /// Pushed via an API call (e.g. HTTP control plane).
    Api,
    /// Set directly in code or via a test.
    Manual,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::File(p) => write!(f, "file:{p}"),
            Self::Environment => write!(f, "environment"),
            Self::Api => write!(f, "api"),
            Self::Manual => write!(f, "manual"),
        }
    }
}

// ─── ReloadableConfig ─────────────────────────────────────────────────────────

/// Wraps any config `T` with version tracking and provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadableConfig<T> {
    /// The actual configuration value.
    pub inner: T,
    /// Monotonically increasing version number.  Starts at `1`.
    pub version: u64,
    /// Unix timestamp (seconds) of the last update.
    pub updated_at: i64,
    /// Where this configuration version came from.
    pub source: ConfigSource,
}

impl<T: Clone + Serialize> ReloadableConfig<T> {
    /// Create a new `ReloadableConfig` at version `1`.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            version: 1,
            updated_at: chrono::Utc::now().timestamp(),
            source: ConfigSource::Default,
        }
    }

    /// Create a new `ReloadableConfig` at version `1` with an explicit source.
    pub fn with_source(inner: T, source: ConfigSource) -> Self {
        Self {
            inner,
            version: 1,
            updated_at: chrono::Utc::now().timestamp(),
            source,
        }
    }

    /// Replace the inner config, increment the version, and return the new version.
    pub fn update(&mut self, inner: T) -> u64 {
        self.inner = inner;
        self.version += 1;
        self.updated_at = chrono::Utc::now().timestamp();
        self.version
    }

    /// Replace the inner config with an explicit source, increment the version,
    /// and return the new version.
    pub fn update_with_source(&mut self, inner: T, source: ConfigSource) -> u64 {
        self.inner = inner;
        self.version += 1;
        self.updated_at = chrono::Utc::now().timestamp();
        self.source = source;
        self.version
    }
}

// ─── ConfigDiff ───────────────────────────────────────────────────────────────

/// Describes a single field-level change between two `IndexerConfig` values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiff {
    /// The name of the field that changed.
    pub field: String,
    /// The previous value (serialized as JSON).
    pub old_value: serde_json::Value,
    /// The new value (serialized as JSON).
    pub new_value: serde_json::Value,
}

impl ConfigDiff {
    fn new(
        field: impl Into<String>,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
    ) -> Self {
        Self {
            field: field.into(),
            old_value,
            new_value,
        }
    }
}

/// Compare two `IndexerConfig` values and return a list of field-level diffs.
///
/// Returns an empty `Vec` when the two configs are identical.
pub fn diff_configs(old: &IndexerConfig, new: &IndexerConfig) -> Vec<ConfigDiff> {
    let mut diffs = Vec::new();

    macro_rules! check {
        ($field:ident) => {
            if old.$field != new.$field {
                diffs.push(ConfigDiff::new(
                    stringify!($field),
                    serde_json::to_value(&old.$field).unwrap_or(serde_json::Value::Null),
                    serde_json::to_value(&new.$field).unwrap_or(serde_json::Value::Null),
                ));
            }
        };
    }

    check!(id);
    check!(chain);
    check!(from_block);
    check!(to_block);
    check!(confirmation_depth);
    check!(batch_size);
    check!(checkpoint_interval);
    check!(poll_interval_ms);

    // EventFilter sub-fields
    if old.filter.addresses != new.filter.addresses {
        diffs.push(ConfigDiff::new(
            "filter.addresses",
            serde_json::to_value(&old.filter.addresses).unwrap_or(serde_json::Value::Null),
            serde_json::to_value(&new.filter.addresses).unwrap_or(serde_json::Value::Null),
        ));
    }
    if old.filter.topic0_values != new.filter.topic0_values {
        diffs.push(ConfigDiff::new(
            "filter.topic0_values",
            serde_json::to_value(&old.filter.topic0_values).unwrap_or(serde_json::Value::Null),
            serde_json::to_value(&new.filter.topic0_values).unwrap_or(serde_json::Value::Null),
        ));
    }
    if old.filter.from_block != new.filter.from_block {
        diffs.push(ConfigDiff::new(
            "filter.from_block",
            serde_json::to_value(&old.filter.from_block).unwrap_or(serde_json::Value::Null),
            serde_json::to_value(&new.filter.from_block).unwrap_or(serde_json::Value::Null),
        ));
    }
    if old.filter.to_block != new.filter.to_block {
        diffs.push(ConfigDiff::new(
            "filter.to_block",
            serde_json::to_value(&old.filter.to_block).unwrap_or(serde_json::Value::Null),
            serde_json::to_value(&new.filter.to_block).unwrap_or(serde_json::Value::Null),
        ));
    }

    diffs
}

// ─── WarningSeverity ──────────────────────────────────────────────────────────

/// Severity level of a configuration warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningSeverity {
    /// Informational — no action required.
    Info,
    /// Potential issue — the change may have unintended consequences.
    Warning,
    /// The change is highly risky and may break the indexer.
    Critical,
}

impl std::fmt::Display for WarningSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARNING"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

// ─── ConfigWarning ────────────────────────────────────────────────────────────

/// A non-fatal advisory raised by `ConfigValidator`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWarning {
    /// The field or aspect that triggered the warning.
    pub field: String,
    /// Human-readable explanation.
    pub message: String,
    /// How serious this warning is.
    pub severity: WarningSeverity,
}

impl ConfigWarning {
    fn new(
        field: impl Into<String>,
        message: impl Into<String>,
        severity: WarningSeverity,
    ) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            severity,
        }
    }
}

// ─── ConfigValidator ──────────────────────────────────────────────────────────

/// Validates that a proposed `IndexerConfig` update is safe to apply.
pub struct ConfigValidator;

impl ConfigValidator {
    /// Validate the transition from `old` to `new`.
    ///
    /// Returns `Ok(warnings)` when the change is permitted (possibly with
    /// advisory warnings), or `Err(IndexerError)` when the change is rejected
    /// outright (breaking change).
    pub fn validate(
        old: &IndexerConfig,
        new: &IndexerConfig,
    ) -> Result<Vec<ConfigWarning>, IndexerError> {
        let mut warnings = Vec::new();

        // --- Hard rejections ---

        if old.chain != new.chain {
            return Err(IndexerError::Other(format!(
                "hot-reload: cannot change chain from '{}' to '{}' — stop and reconfigure the indexer",
                old.chain, new.chain
            )));
        }

        if old.from_block != new.from_block {
            return Err(IndexerError::Other(format!(
                "hot-reload: cannot change from_block from {} to {} — use a checkpoint to rewind instead",
                old.from_block, new.from_block
            )));
        }

        // --- Advisory warnings ---

        if new.confirmation_depth < old.confirmation_depth {
            warnings.push(ConfigWarning::new(
                "confirmation_depth",
                format!(
                    "Decreasing confirmation_depth from {} to {} may cause premature finality and missed reorgs",
                    old.confirmation_depth, new.confirmation_depth
                ),
                WarningSeverity::Warning,
            ));
        }

        if new.batch_size > old.batch_size * 10 {
            warnings.push(ConfigWarning::new(
                "batch_size",
                format!(
                    "batch_size increased more than 10x (from {} to {}); RPC node may reject large eth_getLogs ranges",
                    old.batch_size, new.batch_size
                ),
                WarningSeverity::Warning,
            ));
        }

        if new.poll_interval_ms < 500 {
            warnings.push(ConfigWarning::new(
                "poll_interval_ms",
                format!(
                    "poll_interval_ms={} is very aggressive; may overwhelm the RPC endpoint",
                    new.poll_interval_ms
                ),
                WarningSeverity::Warning,
            ));
        }

        if new.checkpoint_interval == 0 {
            warnings.push(ConfigWarning::new(
                "checkpoint_interval",
                "checkpoint_interval=0 disables checkpointing; crash recovery will be impaired",
                WarningSeverity::Critical,
            ));
        }

        if old.id != new.id {
            warnings.push(ConfigWarning::new(
                "id",
                format!(
                    "Changing indexer id from '{}' to '{}' will break checkpoint continuity",
                    old.id, new.id
                ),
                WarningSeverity::Critical,
            ));
        }

        Ok(warnings)
    }

    /// Returns `true` when the transition from `old` to `new` contains no
    /// breaking changes (i.e. `validate` would succeed with no `Critical`
    /// warnings).
    pub fn is_safe_reload(old: &IndexerConfig, new: &IndexerConfig) -> bool {
        match Self::validate(old, new) {
            Err(_) => false,
            Ok(warnings) => !warnings
                .iter()
                .any(|w| w.severity == WarningSeverity::Critical),
        }
    }
}

// ─── ReloadResult ─────────────────────────────────────────────────────────────

/// The outcome of a successful hot-reload operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadResult {
    /// The new version number after the update.
    pub version: u64,
    /// Field-level diffs between the old and new config.
    pub diffs: Vec<ConfigDiff>,
    /// Advisory warnings raised by the validator.
    pub warnings: Vec<ConfigWarning>,
    /// Unix timestamp (seconds) when the reload was applied.
    pub applied_at: i64,
}

// ─── ReloadRecord ─────────────────────────────────────────────────────────────

/// An immutable record of a single hot-reload event kept in the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadRecord {
    /// The version number assigned to this reload.
    pub version: u64,
    /// Field-level diffs at the time of this reload.
    pub diffs: Vec<ConfigDiff>,
    /// Unix timestamp (seconds) when this reload was applied.
    pub applied_at: i64,
    /// Source that triggered this reload.
    pub source: ConfigSource,
}

// ─── Internal per-config state ────────────────────────────────────────────────

struct ManagedConfig {
    config: Arc<RwLock<ReloadableConfig<IndexerConfig>>>,
    sender: watch::Sender<u64>,
    history: Vec<ReloadRecord>,
}

// ─── HotReloadManager ─────────────────────────────────────────────────────────

/// Central coordinator for all hot-reload operations.
///
/// Holds one `ReloadableConfig<IndexerConfig>` per registered indexer ID,
/// and a `watch` channel per config so subscribers are notified on every
/// version bump.
pub struct HotReloadManager {
    configs: RwLock<HashMap<String, ManagedConfig>>,
}

impl HotReloadManager {
    /// Create a new, empty `HotReloadManager`.
    pub fn new() -> Self {
        Self {
            configs: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new indexer config under `id`.
    ///
    /// Returns a shared handle to the `ReloadableConfig` so the caller can
    /// read the current value cheaply without going through the manager.
    pub async fn register_config(
        &self,
        id: &str,
        config: IndexerConfig,
    ) -> Arc<RwLock<ReloadableConfig<IndexerConfig>>> {
        let reloadable = ReloadableConfig::new(config);
        let version = reloadable.version;
        let arc = Arc::new(RwLock::new(reloadable));
        let (tx, _rx) = watch::channel(version);

        let managed = ManagedConfig {
            config: Arc::clone(&arc),
            sender: tx,
            history: Vec::new(),
        };

        self.configs.write().await.insert(id.to_string(), managed);
        info!("hot-reload: registered config '{id}' at version {version}");
        arc
    }

    /// Apply `new_config` to the indexer identified by `id`.
    ///
    /// The update is validated first; if validation fails the existing config
    /// is left unchanged and an `Err` is returned.
    pub async fn update_config(
        &self,
        id: &str,
        new_config: IndexerConfig,
    ) -> Result<ReloadResult, IndexerError> {
        let mut guard = self.configs.write().await;
        let managed = guard.get_mut(id).ok_or_else(|| {
            IndexerError::Other(format!("hot-reload: no config registered for id '{id}'"))
        })?;

        let old_config = {
            let r = managed.config.read().await;
            r.inner.clone()
        };

        let warnings = ConfigValidator::validate(&old_config, &new_config)?;
        let diffs = diff_configs(&old_config, &new_config);

        let new_version = {
            let mut w = managed.config.write().await;
            w.update_with_source(new_config, ConfigSource::Manual)
        };

        let applied_at = chrono::Utc::now().timestamp();

        managed.history.push(ReloadRecord {
            version: new_version,
            diffs: diffs.clone(),
            applied_at,
            source: ConfigSource::Manual,
        });

        // Notify subscribers.
        let _ = managed.sender.send(new_version);

        for w in &warnings {
            warn!(
                "hot-reload[{id}] v{new_version} {} [{}]: {}",
                w.field, w.severity, w.message
            );
        }
        debug!("hot-reload[{id}] bumped to v{new_version} ({} diffs)", diffs.len());

        Ok(ReloadResult {
            version: new_version,
            diffs,
            warnings,
            applied_at,
        })
    }

    /// Return a clone of the current `IndexerConfig` for `id`, or `None` if
    /// `id` is not registered.
    pub async fn get_config(&self, id: &str) -> Option<IndexerConfig> {
        let guard = self.configs.read().await;
        let managed = guard.get(id)?;
        let r = managed.config.read().await;
        Some(r.inner.clone())
    }

    /// Return the current version number for `id`, or `None` if not registered.
    pub async fn get_version(&self, id: &str) -> Option<u64> {
        let guard = self.configs.read().await;
        let managed = guard.get(id)?;
        let r = managed.config.read().await;
        Some(r.version)
    }

    /// Return a `watch::Receiver` that yields the new version on every reload.
    ///
    /// Returns `None` if `id` is not registered.
    pub async fn subscribe(&self, id: &str) -> Option<watch::Receiver<u64>> {
        let guard = self.configs.read().await;
        let managed = guard.get(id)?;
        Some(managed.sender.subscribe())
    }

    /// List all registered config IDs.
    pub async fn configs(&self) -> Vec<String> {
        let guard = self.configs.read().await;
        guard.keys().cloned().collect()
    }

    /// Return the full reload history for `id`.
    pub async fn history(&self, id: &str) -> Vec<ReloadRecord> {
        let guard = self.configs.read().await;
        match guard.get(id) {
            Some(m) => m.history.clone(),
            None => Vec::new(),
        }
    }
}

impl Default for HotReloadManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── ConfigWatcher ────────────────────────────────────────────────────────────

type ChangeCallback = Box<dyn Fn(Vec<ConfigDiff>) + Send + Sync>;

/// Watches a `ReloadableConfig<IndexerConfig>` on a fixed polling interval and
/// fires registered callbacks whenever the config changes.
pub struct ConfigWatcher {
    interval: Duration,
    callbacks: Arc<RwLock<Vec<ChangeCallback>>>,
    stop_tx: watch::Sender<bool>,
}

impl ConfigWatcher {
    /// Create a new `ConfigWatcher` that polls every `interval`.
    pub fn new(interval: Duration) -> Self {
        let (stop_tx, _) = watch::channel(false);
        Self {
            interval,
            callbacks: Arc::new(RwLock::new(Vec::new())),
            stop_tx,
        }
    }

    /// Start watching `config` for changes originating from `source`.
    ///
    /// Spawns a background `tokio` task that compares version numbers and
    /// invokes all registered callbacks with the diffs when a change is
    /// detected.
    pub fn watch(
        &self,
        config: Arc<RwLock<ReloadableConfig<IndexerConfig>>>,
        _source: ConfigSource,
    ) {
        let interval = self.interval;
        let callbacks = Arc::clone(&self.callbacks);
        let mut stop_rx = self.stop_tx.subscribe();

        tokio::spawn(async move {
            let mut last_version = {
                let r = config.read().await;
                r.version
            };
            let mut last_inner = {
                let r = config.read().await;
                r.inner.clone()
            };

            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // consume the immediate first tick

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let (cur_version, cur_inner) = {
                            let r = config.read().await;
                            (r.version, r.inner.clone())
                        };

                        if cur_version != last_version {
                            let diffs = diff_configs(&last_inner, &cur_inner);
                            debug!("config-watcher: version {} → {}, {} diffs", last_version, cur_version, diffs.len());

                            let cbs = callbacks.read().await;
                            for cb in cbs.iter() {
                                cb(diffs.clone());
                            }

                            last_version = cur_version;
                            last_inner = cur_inner;
                        }
                    }
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            debug!("config-watcher: stopped");
                            break;
                        }
                    }
                }
            }
        });
    }

    /// Register a callback to be invoked with the diffs on every detected change.
    pub async fn on_change(&self, callback: ChangeCallback) {
        self.callbacks.write().await.push(callback);
    }

    /// Stop the background watcher task.
    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }
}

// ─── FilterReloader ───────────────────────────────────────────────────────────

/// Fine-grained hot-reload helper for `EventFilter`.
///
/// Wraps the filter in a `RwLock` so individual addresses and topic0 values
/// can be added/removed without replacing the whole config.
pub struct FilterReloader {
    filter: Arc<RwLock<EventFilter>>,
}

impl FilterReloader {
    /// Create a new `FilterReloader` from an initial `EventFilter`.
    pub fn new(filter: EventFilter) -> Self {
        Self {
            filter: Arc::new(RwLock::new(filter)),
        }
    }

    /// Replace the entire filter and return the field-level diffs.
    pub async fn update(&self, new_filter: EventFilter) -> Vec<ConfigDiff> {
        let old_filter = self.filter.read().await.clone();

        // Build a pair of stub configs so we can reuse diff_configs.
        let old_cfg = stub_config_with_filter(old_filter);
        let new_cfg = stub_config_with_filter(new_filter.clone());
        let diffs = diff_configs(&old_cfg, &new_cfg);

        *self.filter.write().await = new_filter;
        diffs
    }

    /// Return a snapshot of the current `EventFilter`.
    pub async fn current(&self) -> EventFilter {
        self.filter.read().await.clone()
    }

    /// Add `addr` to the filter's address list (if not already present).
    pub async fn add_address(&self, addr: &str) {
        let mut f = self.filter.write().await;
        let addr = addr.to_string();
        if !f.addresses.contains(&addr) {
            f.addresses.push(addr);
        }
    }

    /// Remove `addr` from the filter's address list (case-sensitive).
    pub async fn remove_address(&self, addr: &str) {
        let mut f = self.filter.write().await;
        f.addresses.retain(|a| a != addr);
    }

    /// Add `topic` to the filter's topic0 list (if not already present).
    pub async fn add_topic0(&self, topic: &str) {
        let mut f = self.filter.write().await;
        let topic = topic.to_string();
        if !f.topic0_values.contains(&topic) {
            f.topic0_values.push(topic);
        }
    }

    /// Remove `topic` from the filter's topic0 list (case-sensitive).
    pub async fn remove_topic0(&self, topic: &str) {
        let mut f = self.filter.write().await;
        f.topic0_values.retain(|t| t != topic);
    }
}

// Helper: build a minimal IndexerConfig that only differs in its filter.
fn stub_config_with_filter(filter: EventFilter) -> IndexerConfig {
    IndexerConfig {
        id: "stub".into(),
        chain: "ethereum".into(),
        from_block: 0,
        to_block: None,
        confirmation_depth: 12,
        batch_size: 1000,
        checkpoint_interval: 100,
        poll_interval_ms: 2000,
        filter,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::IndexerConfig;
    use crate::types::EventFilter;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn base_config() -> IndexerConfig {
        IndexerConfig {
            id: "my-indexer".into(),
            chain: "ethereum".into(),
            from_block: 1_000_000,
            to_block: None,
            confirmation_depth: 12,
            batch_size: 500,
            checkpoint_interval: 100,
            poll_interval_ms: 2_000,
            filter: EventFilter::default(),
        }
    }

    // ── ReloadableConfig versioning ───────────────────────────────────────────

    #[test]
    fn reloadable_config_starts_at_version_1() {
        let cfg: ReloadableConfig<IndexerConfig> = ReloadableConfig::new(base_config());
        assert_eq!(cfg.version, 1);
    }

    #[test]
    fn reloadable_config_update_increments_version() {
        let mut cfg: ReloadableConfig<IndexerConfig> = ReloadableConfig::new(base_config());
        let v2 = cfg.update(base_config());
        assert_eq!(v2, 2);
        assert_eq!(cfg.version, 2);

        let v3 = cfg.update(base_config());
        assert_eq!(v3, 3);
        assert_eq!(cfg.version, 3);
    }

    #[test]
    fn reloadable_config_update_replaces_inner() {
        let mut cfg: ReloadableConfig<IndexerConfig> = ReloadableConfig::new(base_config());
        let mut new_inner = base_config();
        new_inner.batch_size = 9_999;
        cfg.update(new_inner);
        assert_eq!(cfg.inner.batch_size, 9_999);
    }

    #[test]
    fn reloadable_config_updated_at_is_set() {
        let cfg: ReloadableConfig<IndexerConfig> = ReloadableConfig::new(base_config());
        assert!(cfg.updated_at > 0);
    }

    // ── ConfigSource variants ─────────────────────────────────────────────────

    #[test]
    fn config_source_display() {
        assert_eq!(ConfigSource::Default.to_string(), "default");
        assert_eq!(ConfigSource::Environment.to_string(), "environment");
        assert_eq!(ConfigSource::Api.to_string(), "api");
        assert_eq!(ConfigSource::Manual.to_string(), "manual");
        assert_eq!(
            ConfigSource::File("/etc/chainindex.yaml".into()).to_string(),
            "file:/etc/chainindex.yaml"
        );
    }

    #[test]
    fn config_source_equality() {
        assert_eq!(ConfigSource::Manual, ConfigSource::Manual);
        assert_ne!(ConfigSource::Api, ConfigSource::Manual);
        assert_eq!(
            ConfigSource::File("a.yaml".into()),
            ConfigSource::File("a.yaml".into())
        );
        assert_ne!(
            ConfigSource::File("a.yaml".into()),
            ConfigSource::File("b.yaml".into())
        );
    }

    // ── ConfigDiff ────────────────────────────────────────────────────────────

    #[test]
    fn diff_configs_empty_when_identical() {
        let cfg = base_config();
        let diffs = diff_configs(&cfg, &cfg);
        assert!(diffs.is_empty(), "identical configs should produce no diffs");
    }

    #[test]
    fn diff_configs_detects_batch_size_change() {
        let old = base_config();
        let mut new = base_config();
        new.batch_size = 2_000;

        let diffs = diff_configs(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "batch_size");
        assert_eq!(diffs[0].old_value, serde_json::json!(500u64));
        assert_eq!(diffs[0].new_value, serde_json::json!(2_000u64));
    }

    #[test]
    fn diff_configs_detects_poll_interval_change() {
        let old = base_config();
        let mut new = base_config();
        new.poll_interval_ms = 500;

        let diffs = diff_configs(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "poll_interval_ms");
    }

    #[test]
    fn diff_configs_detects_multiple_changes() {
        let old = base_config();
        let mut new = base_config();
        new.batch_size = 10;
        new.checkpoint_interval = 50;
        new.poll_interval_ms = 1_000;

        let diffs = diff_configs(&old, &new);
        assert_eq!(diffs.len(), 3);
        let fields: Vec<_> = diffs.iter().map(|d| d.field.as_str()).collect();
        assert!(fields.contains(&"batch_size"));
        assert!(fields.contains(&"checkpoint_interval"));
        assert!(fields.contains(&"poll_interval_ms"));
    }

    #[test]
    fn diff_configs_detects_filter_address_change() {
        let old = base_config();
        let mut new = base_config();
        new.filter.addresses.push("0xDEAD".into());

        let diffs = diff_configs(&old, &new);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "filter.addresses");
    }

    // ── ConfigValidator ───────────────────────────────────────────────────────

    #[test]
    fn validator_rejects_chain_change() {
        let old = base_config();
        let mut new = base_config();
        new.chain = "polygon".into();

        let result = ConfigValidator::validate(&old, &new);
        assert!(result.is_err(), "chain change must be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("chain"), "error message should mention 'chain'");
    }

    #[test]
    fn validator_rejects_from_block_change() {
        let old = base_config();
        let mut new = base_config();
        new.from_block = 999_999;

        let result = ConfigValidator::validate(&old, &new);
        assert!(result.is_err(), "from_block change must be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("from_block"));
    }

    #[test]
    fn validator_allows_safe_reload() {
        let old = base_config();
        let mut new = base_config();
        new.batch_size = 1_000;
        new.poll_interval_ms = 3_000;

        let result = ConfigValidator::validate(&old, &new);
        assert!(result.is_ok());
        let warnings = result.unwrap();
        assert!(warnings.is_empty());
    }

    #[test]
    fn validator_warns_on_confirmation_depth_decrease() {
        let old = base_config();
        let mut new = base_config();
        new.confirmation_depth = 3; // was 12

        let result = ConfigValidator::validate(&old, &new);
        assert!(result.is_ok());
        let warnings = result.unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "confirmation_depth");
        assert_eq!(warnings[0].severity, WarningSeverity::Warning);
    }

    #[test]
    fn validator_is_safe_reload_false_for_chain_change() {
        let old = base_config();
        let mut new = base_config();
        new.chain = "arbitrum".into();

        assert!(!ConfigValidator::is_safe_reload(&old, &new));
    }

    #[test]
    fn validator_is_safe_reload_true_for_batch_size_change() {
        let old = base_config();
        let mut new = base_config();
        new.batch_size = 250;

        assert!(ConfigValidator::is_safe_reload(&old, &new));
    }

    // ── WarningSeverity ───────────────────────────────────────────────────────

    #[test]
    fn warning_severity_display() {
        assert_eq!(WarningSeverity::Info.to_string(), "INFO");
        assert_eq!(WarningSeverity::Warning.to_string(), "WARNING");
        assert_eq!(WarningSeverity::Critical.to_string(), "CRITICAL");
    }

    #[test]
    fn config_warning_checkpoint_interval_zero_is_critical() {
        let old = base_config();
        let mut new = base_config();
        new.checkpoint_interval = 0;

        let warnings = ConfigValidator::validate(&old, &new).unwrap();
        let critical: Vec<_> = warnings
            .iter()
            .filter(|w| w.severity == WarningSeverity::Critical)
            .collect();
        assert!(
            !critical.is_empty(),
            "checkpoint_interval=0 should raise Critical"
        );
    }

    // ── HotReloadManager ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn manager_register_and_get() {
        let mgr = HotReloadManager::new();
        mgr.register_config("idx-1", base_config()).await;

        let cfg = mgr.get_config("idx-1").await;
        assert!(cfg.is_some());
        assert_eq!(cfg.unwrap().id, "my-indexer");
    }

    #[tokio::test]
    async fn manager_update_config_bumps_version() {
        let mgr = HotReloadManager::new();
        mgr.register_config("idx-1", base_config()).await;

        let mut new_cfg = base_config();
        new_cfg.batch_size = 777;

        let result = mgr.update_config("idx-1", new_cfg).await.unwrap();
        assert_eq!(result.version, 2);
        assert_eq!(result.diffs.len(), 1);
        assert_eq!(result.diffs[0].field, "batch_size");
    }

    #[tokio::test]
    async fn manager_subscribe_receives_version_bump() {
        let mgr = HotReloadManager::new();
        mgr.register_config("idx-1", base_config()).await;

        let mut rx = mgr.subscribe("idx-1").await.unwrap();
        assert_eq!(*rx.borrow(), 1);

        let mut new_cfg = base_config();
        new_cfg.poll_interval_ms = 500;
        mgr.update_config("idx-1", new_cfg).await.unwrap();

        // The receiver should now observe version 2.
        rx.changed().await.unwrap();
        assert_eq!(*rx.borrow(), 2);
    }

    #[tokio::test]
    async fn manager_history_tracks_reloads() {
        let mgr = HotReloadManager::new();
        mgr.register_config("idx-1", base_config()).await;

        // Two successive updates.
        let mut c1 = base_config();
        c1.batch_size = 100;
        mgr.update_config("idx-1", c1).await.unwrap();

        let mut c2 = base_config();
        c2.batch_size = 200;
        mgr.update_config("idx-1", c2).await.unwrap();

        let history = mgr.history("idx-1").await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].version, 2);
        assert_eq!(history[1].version, 3);
    }

    #[tokio::test]
    async fn manager_unknown_config_returns_none() {
        let mgr = HotReloadManager::new();
        assert!(mgr.get_config("does-not-exist").await.is_none());
        assert!(mgr.get_version("does-not-exist").await.is_none());
        assert!(mgr.subscribe("does-not-exist").await.is_none());
    }

    #[tokio::test]
    async fn manager_update_rejects_chain_change() {
        let mgr = HotReloadManager::new();
        mgr.register_config("idx-1", base_config()).await;

        let mut bad = base_config();
        bad.chain = "solana".into();

        let result = mgr.update_config("idx-1", bad).await;
        assert!(result.is_err());
        // Version must remain at 1.
        assert_eq!(mgr.get_version("idx-1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn manager_multiple_registrations() {
        let mgr = HotReloadManager::new();

        let mut cfg_a = base_config();
        cfg_a.id = "a".into();
        let mut cfg_b = base_config();
        cfg_b.id = "b".into();
        let mut cfg_c = base_config();
        cfg_c.id = "c".into();

        mgr.register_config("a", cfg_a).await;
        mgr.register_config("b", cfg_b).await;
        mgr.register_config("c", cfg_c).await;

        let mut ids = mgr.configs().await;
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn manager_get_version_initial() {
        let mgr = HotReloadManager::new();
        mgr.register_config("v-test", base_config()).await;
        assert_eq!(mgr.get_version("v-test").await.unwrap(), 1);
    }

    // ── ReloadResult fields ───────────────────────────────────────────────────

    #[tokio::test]
    async fn reload_result_fields_populated() {
        let mgr = HotReloadManager::new();
        mgr.register_config("r", base_config()).await;

        let mut new_cfg = base_config();
        new_cfg.checkpoint_interval = 50;
        new_cfg.poll_interval_ms = 1_000;

        let result = mgr.update_config("r", new_cfg).await.unwrap();

        assert_eq!(result.version, 2);
        assert_eq!(result.diffs.len(), 2);
        assert!(result.applied_at > 0);
        // No warnings for these safe changes.
        assert!(result.warnings.is_empty());
    }

    // ── FilterReloader ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn filter_reloader_add_address() {
        let fr = FilterReloader::new(EventFilter::default());
        fr.add_address("0xABCD").await;
        fr.add_address("0x1234").await;

        let f = fr.current().await;
        assert_eq!(f.addresses.len(), 2);
        assert!(f.addresses.contains(&"0xABCD".to_string()));
        assert!(f.addresses.contains(&"0x1234".to_string()));
    }

    #[tokio::test]
    async fn filter_reloader_add_address_no_duplicates() {
        let fr = FilterReloader::new(EventFilter::default());
        fr.add_address("0xABCD").await;
        fr.add_address("0xABCD").await; // duplicate — should be ignored
        let f = fr.current().await;
        assert_eq!(f.addresses.len(), 1);
    }

    #[tokio::test]
    async fn filter_reloader_remove_address() {
        let fr = FilterReloader::new(EventFilter {
            addresses: vec!["0xAAAA".into(), "0xBBBB".into()],
            ..Default::default()
        });
        fr.remove_address("0xAAAA").await;

        let f = fr.current().await;
        assert_eq!(f.addresses, vec!["0xBBBB".to_string()]);
    }

    #[tokio::test]
    async fn filter_reloader_add_topic0() {
        let fr = FilterReloader::new(EventFilter::default());
        fr.add_topic0("0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")
            .await;

        let f = fr.current().await;
        assert_eq!(f.topic0_values.len(), 1);
    }

    #[tokio::test]
    async fn filter_reloader_current_returns_latest() {
        let fr = FilterReloader::new(EventFilter::default());

        let new_filter = EventFilter {
            addresses: vec!["0xCafe".into()],
            topic0_values: vec!["0xdead".into()],
            from_block: Some(100),
            to_block: None,
        };
        fr.update(new_filter).await;

        let current = fr.current().await;
        assert_eq!(current.addresses, vec!["0xCafe".to_string()]);
        assert_eq!(current.topic0_values, vec!["0xdead".to_string()]);
        assert_eq!(current.from_block, Some(100));
    }

    #[tokio::test]
    async fn filter_reloader_update_returns_diffs() {
        let fr = FilterReloader::new(EventFilter::default());

        let new_filter = EventFilter {
            addresses: vec!["0xFeed".into()],
            ..Default::default()
        };
        let diffs = fr.update(new_filter).await;

        // Should detect the change in filter.addresses.
        assert!(
            diffs.iter().any(|d| d.field == "filter.addresses"),
            "expected filter.addresses diff"
        );
    }
}
