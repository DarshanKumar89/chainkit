//! Response caching layer for RPC transports.
//!
//! `CacheTransport` wraps any `Arc<dyn RpcTransport>` and caches successful
//! responses for configurable methods. Cache keys are computed as a hash of
//! `(method, params)` so identical requests share a single cache entry.
//!
//! # Tiered Caching
//!
//! When a [`CacheTierResolver`] is configured, the cache classifies each
//! method+params pair into one of four [`CacheTier`]s, each with its own TTL:
//!
//! - **Immutable** — results that never change (1 hour TTL).
//! - **SemiStable** — results that change infrequently (5 minutes TTL).
//! - **Volatile** — results that change frequently (2 seconds TTL).
//! - **NeverCache** — write methods and subscriptions (never cached).
//!
//! Without a tier resolver, the cache falls back to the flat `default_ttl`
//! behavior for all methods listed in `cacheable_methods`.
//!
//! # Finality Awareness
//!
//! The [`CacheTransport::invalidate_for_reorg`] method removes cached entries
//! that reference blocks at or above a given block number, enabling safe
//! cache invalidation during chain reorganizations.
//!
//! # Design
//!
//! - Only explicitly cacheable methods are cached (opt-in via `CacheConfig`).
//! - Expired entries are evicted lazily on access.
//! - When the cache exceeds `max_entries`, the oldest entry is evicted (LRU-ish).
//! - Thread-safe: the cache is behind a `Mutex` and the struct is `Send + Sync`.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::RpcTransport;

// ---------------------------------------------------------------------------
// CacheTier
// ---------------------------------------------------------------------------

/// Classification of an RPC method's caching behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTier {
    /// Results that never change (e.g., finalized block data, confirmed tx
    /// receipts). Very long TTL (1 hour).
    Immutable,
    /// Results that change infrequently (e.g., `eth_chainId`, `eth_getCode`).
    /// Medium TTL (5 minutes).
    SemiStable,
    /// Results that change frequently (e.g., `eth_blockNumber`, `eth_gasPrice`).
    /// Short TTL (2 seconds).
    Volatile,
    /// Write methods and subscription calls. Never cached.
    NeverCache,
}

impl CacheTier {
    /// Return the default TTL associated with this tier.
    pub fn default_ttl(&self) -> Option<Duration> {
        match self {
            CacheTier::Immutable => Some(Duration::from_secs(3600)),
            CacheTier::SemiStable => Some(Duration::from_secs(300)),
            CacheTier::Volatile => Some(Duration::from_secs(2)),
            CacheTier::NeverCache => None,
        }
    }
}

// ---------------------------------------------------------------------------
// CacheTierResolver
// ---------------------------------------------------------------------------

/// Determines the [`CacheTier`] for a given RPC request.
///
/// The resolver inspects the method name and, for certain methods, the
/// parameters to classify the caching behavior. For example,
/// `eth_getBlockByNumber` with a specific hex block number is `Immutable`,
/// while the same method with `"latest"` or `"pending"` is `Volatile`.
#[derive(Debug, Clone)]
pub struct CacheTierResolver {
    _private: (),
}

impl CacheTierResolver {
    /// Create a new resolver with default classification rules.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Determine the cache tier for the given method and params.
    pub fn tier_for(&self, method: &str, params: &[serde_json::Value]) -> CacheTier {
        match method {
            // -- Immutable (confirmed/finalized data) -------------------------
            "eth_getTransactionByHash" | "eth_getTransactionReceipt" => CacheTier::Immutable,

            // eth_getBlockByNumber: immutable only if the first param is a
            // concrete hex block number (not a tag like "latest"/"pending").
            "eth_getBlockByNumber" => {
                if let Some(block_param) = params.first() {
                    if is_concrete_block_number(block_param) {
                        CacheTier::Immutable
                    } else {
                        CacheTier::Volatile
                    }
                } else {
                    CacheTier::Volatile
                }
            }

            "eth_getBlockByHash" => CacheTier::Immutable,

            // -- SemiStable ---------------------------------------------------
            "eth_chainId"
            | "net_version"
            | "eth_getCode"
            | "net_listening"
            | "web3_clientVersion"
            | "eth_protocolVersion"
            | "eth_accounts" => CacheTier::SemiStable,

            // -- Volatile (frequently changing) -------------------------------
            "eth_blockNumber"
            | "eth_gasPrice"
            | "eth_estimateGas"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_call"
            | "eth_feeHistory"
            | "eth_maxPriorityFeePerGas"
            | "eth_getStorageAt" => CacheTier::Volatile,

            // -- NeverCache (writes, subscriptions) ---------------------------
            "eth_sendRawTransaction"
            | "eth_sendTransaction"
            | "eth_subscribe"
            | "eth_unsubscribe"
            | "eth_newFilter"
            | "eth_newBlockFilter"
            | "eth_newPendingTransactionFilter"
            | "eth_uninstallFilter"
            | "eth_getFilterChanges"
            | "eth_getFilterLogs"
            | "personal_sign"
            | "eth_sign"
            | "eth_signTransaction"
            | "eth_signTypedData_v4" => CacheTier::NeverCache,

            // Unknown methods — default to NeverCache (safe default).
            _ => CacheTier::NeverCache,
        }
    }
}

impl Default for CacheTierResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for the response cache.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Default time-to-live for cached entries.
    ///
    /// When `tier_resolver` is `None`, this TTL is used for all entries in
    /// `cacheable_methods`. When a tier resolver is present, this is used as
    /// a fallback only if a tier's default TTL is somehow `None`.
    pub default_ttl: Duration,
    /// Maximum number of entries to store.
    pub max_entries: usize,
    /// Set of RPC method names that are eligible for caching.
    ///
    /// When `tier_resolver` is `None`, only methods in this set are cached.
    /// When `tier_resolver` is `Some`, this set is ignored and the resolver
    /// decides cacheability (any tier except `NeverCache`).
    pub cacheable_methods: HashSet<String>,
    /// Optional tier resolver for tiered caching.
    ///
    /// When `Some`, the resolver classifies each request into a [`CacheTier`]
    /// and applies tier-specific TTLs. When `None`, the cache uses the flat
    /// `default_ttl` + `cacheable_methods` behavior.
    pub tier_resolver: Option<CacheTierResolver>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        let cacheable: HashSet<String> = [
            "eth_chainId",
            "eth_getBlockByNumber",
            "eth_getCode",
            "net_version",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        Self {
            default_ttl: Duration::from_secs(60),
            max_entries: 1024,
            cacheable_methods: cacheable,
            tier_resolver: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

struct CacheEntry {
    method: String,
    response: JsonRpcResponse,
    inserted_at: Instant,
    /// The cache tier this entry was classified under.
    /// Stored for diagnostics and potential future tier-based eviction policies.
    #[allow(dead_code)]
    tier: CacheTier,
    /// The block number this entry references, if detectable from params.
    /// Used for reorg-based invalidation.
    block_ref: Option<u64>,
    /// The TTL for this specific entry (set at insertion time).
    ttl: Duration,
}

/// Aggregate cache statistics.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Total cache hits since creation.
    pub hits: u64,
    /// Total cache misses since creation.
    pub misses: u64,
    /// Current number of (non-expired) entries.
    pub size: usize,
}

struct CacheInner {
    entries: HashMap<u64, CacheEntry>,
    stats: CacheStats,
}

// ---------------------------------------------------------------------------
// CacheTransport
// ---------------------------------------------------------------------------

/// A caching wrapper around an RPC transport.
///
/// Only methods listed in `CacheConfig::cacheable_methods` are cached.
/// All other requests pass straight through to the inner transport.
pub struct CacheTransport {
    inner: Arc<dyn RpcTransport>,
    cache: Mutex<CacheInner>,
    config: CacheConfig,
}

impl CacheTransport {
    /// Create a new caching wrapper.
    pub fn new(inner: Arc<dyn RpcTransport>, config: CacheConfig) -> Self {
        Self {
            inner,
            cache: Mutex::new(CacheInner {
                entries: HashMap::new(),
                stats: CacheStats::default(),
            }),
            config,
        }
    }

    /// Send a request, returning a cached response when available.
    pub async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        // Determine cacheability and TTL.
        let (is_cacheable, tier, ttl) = self.resolve_cacheability(&req);

        // Non-cacheable — pass through immediately.
        if !is_cacheable {
            return self.inner.send(req).await;
        }

        let key = cache_key(&req.method, &req.params);

        // Check cache (under lock).
        {
            let mut inner = self.cache.lock().unwrap();

            // Evict expired entries lazily.
            self.evict_expired(&mut inner);

            // Check for a valid (non-expired) cached entry.
            let cached = inner.entries.get(&key).and_then(|entry| {
                if entry.inserted_at.elapsed() < entry.ttl {
                    Some(entry.response.clone())
                } else {
                    None
                }
            });

            if let Some(response) = cached {
                inner.stats.hits += 1;
                tracing::debug!(method = %req.method, "cache hit");
                return Ok(response);
            }

            // Remove expired entry if present.
            inner.entries.remove(&key);

            inner.stats.misses += 1;
        }

        // Cache miss — delegate to the inner transport.
        let response = self.inner.send(req.clone()).await?;

        // Only cache successful responses.
        if response.is_ok() {
            let block_ref = extract_block_ref(&req.method, &req.params);

            let mut inner = self.cache.lock().unwrap();

            // Evict LRU if over capacity.
            while inner.entries.len() >= self.config.max_entries {
                self.evict_oldest(&mut inner);
            }

            inner.entries.insert(
                key,
                CacheEntry {
                    method: req.method.clone(),
                    response: response.clone(),
                    inserted_at: Instant::now(),
                    tier,
                    block_ref,
                    ttl,
                },
            );
            tracing::debug!(method = %req.method, ?tier, "cached response");
        }

        Ok(response)
    }

    /// Clear all cached entries.
    pub fn invalidate(&self) {
        let mut inner = self.cache.lock().unwrap();
        inner.entries.clear();
        tracing::info!("cache invalidated (all entries)");
    }

    /// Clear cached entries for the given method name.
    ///
    /// Each `CacheEntry` stores its method name, so we can filter precisely
    /// and only remove entries belonging to the targeted method.
    pub fn invalidate_method(&self, method: &str) {
        let mut inner = self.cache.lock().unwrap();
        inner.entries.retain(|_, entry| entry.method != method);
    }

    /// Invalidate all cached entries that reference blocks at or above
    /// `from_block`.
    ///
    /// This is used during chain reorganizations: when a reorg is detected
    /// starting at `from_block`, all cached data that might reference the
    /// now-invalid chain segment must be removed.
    ///
    /// Entries without a detectable `block_ref` (i.e., `block_ref` is `None`)
    /// are **not** removed — they are either block-agnostic (like `eth_chainId`)
    /// or their block association could not be determined.
    pub fn invalidate_for_reorg(&self, from_block: u64) {
        let mut inner = self.cache.lock().unwrap();
        let before = inner.entries.len();
        inner.entries.retain(|_, entry| {
            match entry.block_ref {
                Some(block) => block < from_block,
                None => true, // keep entries without a block ref
            }
        });
        let removed = before - inner.entries.len();
        tracing::info!(from_block, removed, "cache invalidated for reorg");
    }

    /// Return a snapshot of cache statistics.
    pub fn stats(&self) -> CacheStats {
        let inner = self.cache.lock().unwrap();
        CacheStats {
            hits: inner.stats.hits,
            misses: inner.stats.misses,
            size: inner.entries.len(),
        }
    }

    // -- internal helpers ---------------------------------------------------

    /// Determine whether a request is cacheable, its tier, and TTL.
    ///
    /// When a `tier_resolver` is configured, the resolver decides.
    /// Otherwise, fall back to the flat `cacheable_methods` + `default_ttl`.
    fn resolve_cacheability(&self, req: &JsonRpcRequest) -> (bool, CacheTier, Duration) {
        if let Some(ref resolver) = self.config.tier_resolver {
            let tier = resolver.tier_for(&req.method, &req.params);
            match tier {
                CacheTier::NeverCache => (false, tier, Duration::ZERO),
                _ => {
                    let ttl = tier.default_ttl().unwrap_or(self.config.default_ttl);
                    (true, tier, ttl)
                }
            }
        } else {
            // Legacy flat mode.
            let is_cacheable = self.config.cacheable_methods.contains(&req.method);
            (
                is_cacheable,
                CacheTier::SemiStable, // default tier for legacy mode
                self.config.default_ttl,
            )
        }
    }

    fn evict_expired(&self, inner: &mut CacheInner) {
        inner
            .entries
            .retain(|_, entry| entry.inserted_at.elapsed() < entry.ttl);
    }

    fn evict_oldest(&self, inner: &mut CacheInner) {
        if inner.entries.is_empty() {
            return;
        }
        // Find the key with the oldest `inserted_at`.
        let oldest_key = inner
            .entries
            .iter()
            .min_by_key(|(_, e)| e.inserted_at)
            .map(|(k, _)| *k);
        if let Some(key) = oldest_key {
            inner.entries.remove(&key);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute a deterministic cache key from method + params.
fn cache_key(method: &str, params: &[serde_json::Value]) -> u64 {
    let mut hasher = DefaultHasher::new();
    method.hash(&mut hasher);
    // Serialize params to a canonical JSON string for hashing.
    let params_str = serde_json::to_string(params).unwrap_or_default();
    params_str.hash(&mut hasher);
    hasher.finish()
}

/// Check whether a JSON value represents a concrete (hex) block number
/// rather than a block tag like `"latest"`, `"pending"`, `"earliest"`,
/// `"safe"`, or `"finalized"`.
fn is_concrete_block_number(value: &serde_json::Value) -> bool {
    match value.as_str() {
        Some(s) => {
            // Tags are never concrete.
            let tags = ["latest", "pending", "earliest", "safe", "finalized"];
            if tags.contains(&s) {
                return false;
            }
            // Accept hex-encoded block numbers (e.g., "0x10d4f").
            s.starts_with("0x") || s.starts_with("0X")
        }
        None => {
            // Could be a JSON number — treat as concrete.
            value.is_number()
        }
    }
}

/// Try to extract a block number from the request params, for use in
/// reorg-based cache invalidation.
///
/// This is a best-effort extraction: it covers common patterns like
/// `eth_getBlockByNumber("0x1a2b3c", ...)` but does not attempt to
/// parse every possible method's params.
fn extract_block_ref(method: &str, params: &[serde_json::Value]) -> Option<u64> {
    match method {
        "eth_getBlockByNumber" => params.first().and_then(parse_hex_block),
        "eth_getTransactionByBlockNumberAndIndex" => params.first().and_then(parse_hex_block),
        _ => None,
    }
}

/// Parse a hex-encoded block number string like `"0x1a2b3c"` into a `u64`.
fn parse_hex_block(value: &serde_json::Value) -> Option<u64> {
    let s = value.as_str()?;
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    u64::from_str_radix(hex, 16).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcRequest, JsonRpcResponse, RpcId};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A mock transport that counts how many times `send` is called.
    struct CountingTransport {
        call_count: AtomicU64,
    }

    impl CountingTransport {
        fn new() -> Self {
            Self {
                call_count: AtomicU64::new(0),
            }
        }

        fn calls(&self) -> u64 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RpcTransport for CountingTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::Value::String("0x1".into())),
                error: None,
            })
        }

        fn url(&self) -> &str {
            "mock://counting"
        }
    }

    fn default_config() -> CacheConfig {
        CacheConfig {
            default_ttl: Duration::from_secs(60),
            max_entries: 128,
            cacheable_methods: ["eth_chainId"].iter().map(|s| s.to_string()).collect(),
            tier_resolver: None,
        }
    }

    fn tiered_config() -> CacheConfig {
        CacheConfig {
            default_ttl: Duration::from_secs(60),
            max_entries: 128,
            cacheable_methods: HashSet::new(), // ignored when tier_resolver is Some
            tier_resolver: Some(CacheTierResolver::new()),
        }
    }

    fn make_req(method: &str) -> JsonRpcRequest {
        JsonRpcRequest::new(1, method, vec![])
    }

    fn make_req_with_params(method: &str, params: Vec<serde_json::Value>) -> JsonRpcRequest {
        JsonRpcRequest::new(1, method, params)
    }

    // -----------------------------------------------------------------------
    // Original tests (backward compatibility with flat mode)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn cache_hit_returns_same_response() {
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), default_config());

        let req = make_req("eth_chainId");
        let r1 = cache.send(req.clone()).await.unwrap();
        let r2 = cache.send(req).await.unwrap();

        // Both responses should be identical.
        assert_eq!(r1.result, r2.result);
        // Only one actual call to the inner transport.
        assert_eq!(transport.calls(), 1);
    }

    #[tokio::test]
    async fn cache_miss_delegates_to_inner() {
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), default_config());

        // First call is always a miss.
        let _r = cache.send(make_req("eth_chainId")).await.unwrap();
        assert_eq!(transport.calls(), 1);

        let stats = cache.stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.size, 1);
    }

    #[tokio::test]
    async fn ttl_expiry_works() {
        let transport = Arc::new(CountingTransport::new());
        let config = CacheConfig {
            default_ttl: Duration::from_millis(50), // very short TTL
            max_entries: 128,
            cacheable_methods: ["eth_chainId"].iter().map(|s| s.to_string()).collect(),
            tier_resolver: None,
        };
        let cache = CacheTransport::new(transport.clone(), config);

        let req = make_req("eth_chainId");
        cache.send(req.clone()).await.unwrap();
        assert_eq!(transport.calls(), 1);

        // Wait for TTL to expire.
        tokio::time::sleep(Duration::from_millis(100)).await;

        cache.send(req).await.unwrap();
        // Should have hit the inner transport again.
        assert_eq!(transport.calls(), 2);
    }

    #[tokio::test]
    async fn non_cacheable_methods_bypass_cache() {
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), default_config());

        // eth_blockNumber is NOT in cacheable_methods.
        let req = make_req("eth_blockNumber");
        cache.send(req.clone()).await.unwrap();
        cache.send(req).await.unwrap();

        // Both calls should have hit the inner transport.
        assert_eq!(transport.calls(), 2);
        // Cache should be empty.
        assert_eq!(cache.stats().size, 0);
    }

    #[tokio::test]
    async fn invalidate_clears_cache() {
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), default_config());

        cache.send(make_req("eth_chainId")).await.unwrap();
        assert_eq!(cache.stats().size, 1);

        cache.invalidate();
        assert_eq!(cache.stats().size, 0);

        // Next send should be a miss.
        cache.send(make_req("eth_chainId")).await.unwrap();
        assert_eq!(transport.calls(), 2);
    }

    #[tokio::test]
    async fn max_entries_evicts_oldest() {
        let transport = Arc::new(CountingTransport::new());
        let config = CacheConfig {
            default_ttl: Duration::from_secs(60),
            max_entries: 2,
            cacheable_methods: ["eth_chainId", "eth_getCode"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            tier_resolver: None,
        };
        let cache = CacheTransport::new(transport.clone(), config);

        // Fill cache to max.
        cache
            .send(JsonRpcRequest::new(
                1,
                "eth_chainId",
                vec![serde_json::Value::String("a".into())],
            ))
            .await
            .unwrap();
        cache
            .send(JsonRpcRequest::new(
                2,
                "eth_chainId",
                vec![serde_json::Value::String("b".into())],
            ))
            .await
            .unwrap();
        assert_eq!(cache.stats().size, 2);

        // One more should evict the oldest.
        cache
            .send(JsonRpcRequest::new(
                3,
                "eth_getCode",
                vec![serde_json::Value::String("c".into())],
            ))
            .await
            .unwrap();
        assert_eq!(cache.stats().size, 2);
    }

    #[tokio::test]
    async fn invalidate_method_is_targeted() {
        let transport = Arc::new(CountingTransport::new());
        let config = CacheConfig {
            default_ttl: Duration::from_secs(60),
            max_entries: 128,
            cacheable_methods: ["eth_chainId", "eth_getCode"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            tier_resolver: None,
        };
        let cache = CacheTransport::new(transport.clone(), config);

        cache.send(make_req("eth_chainId")).await.unwrap();
        cache.send(make_req("eth_getCode")).await.unwrap();
        assert_eq!(cache.stats().size, 2);

        cache.invalidate_method("eth_chainId");
        assert_eq!(cache.stats().size, 1); // only eth_getCode remains

        // eth_chainId should be a miss now
        cache.send(make_req("eth_chainId")).await.unwrap();
        assert_eq!(transport.calls(), 3); // 2 original + 1 new
    }

    #[test]
    fn cache_key_deterministic() {
        let k1 = cache_key("eth_chainId", &[]);
        let k2 = cache_key("eth_chainId", &[]);
        assert_eq!(k1, k2);

        let k3 = cache_key("eth_blockNumber", &[]);
        assert_ne!(k1, k3);
    }

    #[test]
    fn cache_key_differs_by_params() {
        let k1 = cache_key("eth_getCode", &[serde_json::Value::String("0xabc".into())]);
        let k2 = cache_key("eth_getCode", &[serde_json::Value::String("0xdef".into())]);
        assert_ne!(k1, k2);
    }

    // -----------------------------------------------------------------------
    // New tiered caching tests
    // -----------------------------------------------------------------------

    #[test]
    fn tier_default_ttls() {
        assert_eq!(
            CacheTier::Immutable.default_ttl(),
            Some(Duration::from_secs(3600))
        );
        assert_eq!(
            CacheTier::SemiStable.default_ttl(),
            Some(Duration::from_secs(300))
        );
        assert_eq!(
            CacheTier::Volatile.default_ttl(),
            Some(Duration::from_secs(2))
        );
        assert_eq!(CacheTier::NeverCache.default_ttl(), None);
    }

    #[test]
    fn resolver_classifies_methods() {
        let resolver = CacheTierResolver::new();

        assert_eq!(
            resolver.tier_for("eth_getTransactionReceipt", &[]),
            CacheTier::Immutable
        );
        assert_eq!(
            resolver.tier_for("eth_getTransactionByHash", &[]),
            CacheTier::Immutable
        );
        assert_eq!(resolver.tier_for("eth_chainId", &[]), CacheTier::SemiStable);
        assert_eq!(resolver.tier_for("net_version", &[]), CacheTier::SemiStable);
        assert_eq!(resolver.tier_for("eth_getCode", &[]), CacheTier::SemiStable);
        assert_eq!(
            resolver.tier_for("eth_blockNumber", &[]),
            CacheTier::Volatile
        );
        assert_eq!(resolver.tier_for("eth_gasPrice", &[]), CacheTier::Volatile);
        assert_eq!(
            resolver.tier_for("eth_sendRawTransaction", &[]),
            CacheTier::NeverCache
        );
        assert_eq!(
            resolver.tier_for("eth_subscribe", &[]),
            CacheTier::NeverCache
        );
    }

    #[tokio::test]
    async fn tier_immutable_long_ttl() {
        // Immutable entries (like tx receipts) survive well past the
        // default_ttl because they get 1 hour TTL from the tier.
        let transport = Arc::new(CountingTransport::new());
        let config = CacheConfig {
            default_ttl: Duration::from_millis(50), // very short fallback
            max_entries: 128,
            cacheable_methods: HashSet::new(),
            tier_resolver: Some(CacheTierResolver::new()),
        };
        let cache = CacheTransport::new(transport.clone(), config);

        let req = make_req_with_params(
            "eth_getTransactionReceipt",
            vec![serde_json::Value::String("0xabc123def456".into())],
        );
        cache.send(req.clone()).await.unwrap();
        assert_eq!(transport.calls(), 1);

        // Sleep past the default_ttl (50ms) but well under 1 hour.
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should still be a cache hit because immutable TTL is 1 hour.
        cache.send(req).await.unwrap();
        assert_eq!(transport.calls(), 1); // still 1 — cache hit
    }

    #[tokio::test]
    async fn tier_volatile_short_ttl() {
        // Volatile entries expire after 2 seconds.
        let transport = Arc::new(CountingTransport::new());
        let config = CacheConfig {
            default_ttl: Duration::from_secs(60), // long fallback
            max_entries: 128,
            cacheable_methods: HashSet::new(),
            // Use a custom-like config: we want volatile to be very short
            // for testing. We'll use the real resolver but override nothing;
            // eth_blockNumber is Volatile with 2s TTL. We'll sleep 50ms in
            // a tight test by using a more controllable approach:
            // Instead, we use eth_gasPrice which is also volatile.
            tier_resolver: Some(CacheTierResolver::new()),
        };
        let cache = CacheTransport::new(transport.clone(), config);

        let req = make_req("eth_gasPrice");
        cache.send(req.clone()).await.unwrap();
        assert_eq!(transport.calls(), 1);

        // The entry is cached with 2s TTL. Wait past it.
        tokio::time::sleep(Duration::from_millis(2100)).await;

        cache.send(req).await.unwrap();
        // Should have been a miss — inner transport called again.
        assert_eq!(transport.calls(), 2);
    }

    #[tokio::test]
    async fn tier_never_cache_bypasses() {
        // NeverCache methods are never stored in the cache.
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), tiered_config());

        let req = make_req("eth_sendRawTransaction");
        cache.send(req.clone()).await.unwrap();
        cache.send(req).await.unwrap();

        // Both calls hit the inner transport; nothing cached.
        assert_eq!(transport.calls(), 2);
        assert_eq!(cache.stats().size, 0);
    }

    #[tokio::test]
    async fn reorg_invalidation_removes_affected() {
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), tiered_config());

        // Cache blocks 100, 200, 300.
        for block in [100u64, 200, 300] {
            let req = make_req_with_params(
                "eth_getBlockByNumber",
                vec![
                    serde_json::Value::String(format!("0x{:x}", block)),
                    serde_json::Value::Bool(true),
                ],
            );
            cache.send(req).await.unwrap();
        }
        assert_eq!(cache.stats().size, 3);

        // Reorg at block 200: blocks 200 and 300 should be invalidated.
        cache.invalidate_for_reorg(200);

        // Only block 100 should remain.
        assert_eq!(cache.stats().size, 1);

        // Fetching block 200 again should be a cache miss.
        let req200 = make_req_with_params(
            "eth_getBlockByNumber",
            vec![
                serde_json::Value::String("0xc8".into()),
                serde_json::Value::Bool(true),
            ],
        );
        cache.send(req200).await.unwrap();
        // 3 original + 1 new = 4
        assert_eq!(transport.calls(), 4);
    }

    #[tokio::test]
    async fn block_param_latest_is_volatile() {
        let resolver = CacheTierResolver::new();

        // "latest" should be volatile.
        let tier_latest = resolver.tier_for(
            "eth_getBlockByNumber",
            &[
                serde_json::Value::String("latest".into()),
                serde_json::Value::Bool(true),
            ],
        );
        assert_eq!(tier_latest, CacheTier::Volatile);

        // "pending" should be volatile.
        let tier_pending = resolver.tier_for(
            "eth_getBlockByNumber",
            &[
                serde_json::Value::String("pending".into()),
                serde_json::Value::Bool(true),
            ],
        );
        assert_eq!(tier_pending, CacheTier::Volatile);

        // A concrete hex block number should be immutable.
        let tier_concrete = resolver.tier_for(
            "eth_getBlockByNumber",
            &[
                serde_json::Value::String("0x10d4f".into()),
                serde_json::Value::Bool(true),
            ],
        );
        assert_eq!(tier_concrete, CacheTier::Immutable);
    }

    #[test]
    fn is_concrete_block_number_checks() {
        // Tags are not concrete.
        assert!(!is_concrete_block_number(&serde_json::Value::String(
            "latest".into()
        )));
        assert!(!is_concrete_block_number(&serde_json::Value::String(
            "pending".into()
        )));
        assert!(!is_concrete_block_number(&serde_json::Value::String(
            "earliest".into()
        )));
        assert!(!is_concrete_block_number(&serde_json::Value::String(
            "safe".into()
        )));
        assert!(!is_concrete_block_number(&serde_json::Value::String(
            "finalized".into()
        )));

        // Hex numbers are concrete.
        assert!(is_concrete_block_number(&serde_json::Value::String(
            "0x10d4f".into()
        )));
        assert!(is_concrete_block_number(&serde_json::Value::String(
            "0X1A".into()
        )));

        // JSON numbers are concrete.
        assert!(is_concrete_block_number(&serde_json::json!(42)));
    }

    #[test]
    fn parse_hex_block_works() {
        assert_eq!(
            parse_hex_block(&serde_json::Value::String("0x64".into())),
            Some(100)
        );
        assert_eq!(
            parse_hex_block(&serde_json::Value::String("0xc8".into())),
            Some(200)
        );
        assert_eq!(
            parse_hex_block(&serde_json::Value::String("latest".into())),
            None
        );
        assert_eq!(parse_hex_block(&serde_json::json!(42)), None);
    }

    #[test]
    fn extract_block_ref_for_get_block() {
        assert_eq!(
            extract_block_ref(
                "eth_getBlockByNumber",
                &[serde_json::Value::String("0x64".into())]
            ),
            Some(100)
        );
        assert_eq!(
            extract_block_ref(
                "eth_getBlockByNumber",
                &[serde_json::Value::String("latest".into())]
            ),
            None
        );
        assert_eq!(extract_block_ref("eth_chainId", &[]), None);
    }

    #[tokio::test]
    async fn tiered_mode_caches_semi_stable() {
        // eth_chainId with tiered config should be cached as SemiStable.
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), tiered_config());

        let req = make_req("eth_chainId");
        cache.send(req.clone()).await.unwrap();
        cache.send(req).await.unwrap();

        assert_eq!(transport.calls(), 1); // second was a cache hit
        assert_eq!(cache.stats().hits, 1);
        assert_eq!(cache.stats().misses, 1);
    }

    #[tokio::test]
    async fn reorg_keeps_unrelated_entries() {
        // Entries without a block ref (e.g., eth_chainId) should survive a reorg.
        let transport = Arc::new(CountingTransport::new());
        let cache = CacheTransport::new(transport.clone(), tiered_config());

        cache.send(make_req("eth_chainId")).await.unwrap();
        let block_req = make_req_with_params(
            "eth_getBlockByNumber",
            vec![
                serde_json::Value::String("0x64".into()),
                serde_json::Value::Bool(true),
            ],
        );
        cache.send(block_req).await.unwrap();
        assert_eq!(cache.stats().size, 2);

        // Reorg at block 50: block 100 (0x64) should be removed.
        cache.invalidate_for_reorg(50);

        // eth_chainId has no block_ref, so it survives.
        assert_eq!(cache.stats().size, 1);
    }
}
