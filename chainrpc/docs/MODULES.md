# ChainRPC — Module Reference

Every module in `chainrpc-core` is documented here with its purpose, public API, and how it connects to the rest of the system.

---

## Core Types

### `transport.rs` — RpcTransport Trait

The central abstraction. Every transport (HTTP, WebSocket, pool, cache wrapper, etc.) implements this trait.

```rust
#[async_trait]
pub trait RpcTransport: Send + Sync + 'static {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>;
    async fn send_batch(&self, reqs: Vec<JsonRpcRequest>) -> Result<Vec<JsonRpcResponse>, TransportError>;
    fn health(&self) -> HealthStatus;
    fn url(&self) -> &str;
    async fn call<T: DeserializeOwned>(&self, id: u64, method: &str, params: Vec<Value>) -> Result<T, TransportError>;
}
```

Object-safe (`Arc<dyn RpcTransport>`). Default `send_batch` sends sequentially; `HttpRpcClient` and `BatchingTransport` override with true batch.

### `request.rs` — Wire Types

```rust
pub struct JsonRpcRequest {
    pub jsonrpc: String,   // always "2.0"
    pub id: RpcId,         // Number(u64) or Str(String)
    pub method: String,
    pub params: Vec<Value>,
}

pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RpcId,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}
```

Key methods:
- `JsonRpcRequest::new(id, method, params)` — explicit ID
- `JsonRpcRequest::auto(method, params)` — auto-incrementing atomic ID
- `JsonRpcResponse::is_ok()` — true if `error` is None
- `JsonRpcResponse::into_result()` — extracts `result` or returns `JsonRpcError`

### `error.rs` — TransportError

```rust
pub enum TransportError {
    Http(String),              // Connection/network error
    WebSocket(String),         // WS-specific error
    Rpc(JsonRpcError),         // Node returned an error response
    RateLimited { provider },  // Token bucket empty
    CircuitOpen { provider },  // Provider circuit breaker tripped
    AllProvidersDown,          // No healthy providers in pool
    Timeout { ms },            // Request exceeded deadline
    Deserialization(Error),    // Response JSON parse failure
    Overloaded { queue_depth },// Backpressure limit reached
    Cancelled,                 // Operation cancelled via token
    Other(String),             // Catch-all
}
```

`is_retryable()` returns true for `Http`, `WebSocket`, `Timeout`, `RateLimited`.
`is_execution_error()` returns true for `Rpc` (node-side errors, don't retry).

---

## Policy Engine (`policy/`)

### `rate_limiter.rs` — Token Bucket + CU-Aware Rate Limiter

**TokenBucket** — Core rate limiting primitive.
```rust
pub fn try_acquire(&self, cost: f64) -> bool;   // consume tokens
pub fn wait_time(&self, cost: f64) -> Duration;  // time until tokens available
pub fn available(&self) -> f64;                   // current tokens
```

**RateLimiter** — Simple wrapper with `default_cost`.
```rust
pub fn try_acquire(&self) -> bool;
pub fn try_acquire_cost(&self, cost: f64) -> bool;
```

**MethodAwareRateLimiter** — Uses `CuCostTable` to automatically look up per-method costs.
```rust
pub fn try_acquire_method(&self, method: &str) -> bool;   // e.g. eth_getLogs = 75 CU
pub fn wait_time_for_method(&self, method: &str) -> Duration;
```

### `circuit_breaker.rs` — Circuit Breaker

Three states: `Closed` (normal) → `Open` (blocked, cooldown) → `HalfOpen` (probe).

```rust
pub fn is_allowed(&self) -> bool;     // can we send a request?
pub fn record_success(&self);         // move toward Closed
pub fn record_failure(&self);         // increment failure count, may Open
pub fn state(&self) -> CircuitState;
```

Config: `failure_threshold` (default 5), `reset_timeout` (default 30s).

### `retry.rs` — Exponential Backoff with Jitter

```rust
pub fn should_retry(&self, attempt: u32) -> bool;
pub fn delay(&self, attempt: u32) -> Duration;  // exponential + time-based jitter
```

Config: `max_retries` (default 3), `initial_delay` (200ms), `max_delay` (10s), `multiplier` (2.0).

---

## Provider Pool

### `pool.rs` — ProviderPool

Multi-provider failover with per-provider circuit breakers and metrics.

```rust
pub fn new(transports: Vec<Arc<dyn RpcTransport>>, config: ProviderPoolConfig) -> Self;
pub fn new_with_metrics(transports, config) -> Self;  // auto-creates ProviderMetrics
pub fn healthy_count(&self) -> usize;
pub fn health_summary(&self) -> Vec<(String, HealthStatus, String)>;
pub fn health_report(&self) -> Vec<serde_json::Value>;  // JSON-serializable
pub fn metrics(&self) -> Vec<MetricsSnapshot>;
```

Implements `RpcTransport` — sends to the next healthy provider via round-robin. Records success/failure on the circuit breaker and metrics.

### `selection.rs` — Provider Selection Strategies

5 strategies for `select()`:

| Strategy | How it works |
|----------|-------------|
| `RoundRobin` | Atomic counter, wraps around, skips unhealthy |
| `Priority` | First provider that `is_allowed`, in registration order |
| `WeightedRoundRobin` | Counter modulo total weight, maps to provider |
| `LatencyBased` | EMA-smoothed latency (30% new / 70% old), picks lowest |
| `Sticky` | Consistent hashing on a key (e.g. sender address), fallback if down |

```rust
pub fn select(
    &self,
    strategy: &SelectionStrategy,
    provider_count: usize,
    is_allowed: impl Fn(usize) -> bool,
) -> Option<usize>;
```

### `health_checker.rs` — Background Health Probing

Spawns a tokio task that periodically calls a lightweight RPC method on each provider.

```rust
pub fn start_health_checker(
    providers: Vec<Arc<dyn RpcTransport>>,
    config: HealthCheckConfig,         // interval, method, timeout
    on_result: impl Fn(ProbeResult),   // callback per probe
) -> JoinHandle<()>;
```

---

## Caching & Dedup

### `cache.rs` — Tiered Response Cache

4-tier system with per-entry TTL:

| Tier | TTL | Examples |
|------|-----|----------|
| `Immutable` | 1 hour | `eth_getTransactionReceipt`, `eth_getBlockByNumber("0x1a2b")` |
| `SemiStable` | 5 min | `eth_chainId`, `net_version`, `eth_getCode` |
| `Volatile` | 2 sec | `eth_blockNumber`, `eth_gasPrice`, `eth_getBalance` |
| `NeverCache` | — | `eth_sendRawTransaction`, `eth_subscribe`, filter methods |

Smart block param detection: `eth_getBlockByNumber("latest")` = Volatile, `eth_getBlockByNumber("0x64")` = Immutable.

```rust
pub fn new(inner: Arc<dyn RpcTransport>, config: CacheConfig) -> Self;
pub async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>;
pub fn invalidate(&self);                    // clear all
pub fn invalidate_method(&self, method: &str);  // clear specific method
pub fn invalidate_for_reorg(&self, from_block: u64);  // reorg safety
pub fn stats(&self) -> CacheStats;           // hits, misses, size
```

Backward-compatible: without `tier_resolver`, uses flat `cacheable_methods` + `default_ttl`.

### `dedup.rs` — Request Deduplication

Coalesces concurrent identical requests (same method + params) into a single transport call.

```rust
pub async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>;
```

Uses `DashMap`-free approach: `Mutex<HashMap<key, Vec<Sender>>>`. First request fires, others wait on broadcast.

---

## Advanced Features

### `batch.rs` — Auto-Batching Transport

Collects requests within a time window, flushes as one `send_batch()`. Implements `RpcTransport`.

```rust
pub fn new(inner: Arc<dyn RpcTransport>, window: Duration) -> Arc<Self>;
```

Single-item batches skip overhead and call `send()` directly.

### `backpressure.rs` — Concurrency Limiting

Wraps any transport with a Semaphore-based concurrency limit.

```rust
pub fn new(inner: Arc<dyn RpcTransport>, max_in_flight: usize) -> Self;
pub async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>;
pub fn in_flight(&self) -> usize;
pub fn is_full(&self) -> bool;
```

Returns `TransportError::Overloaded { queue_depth }` when limit is reached.

### `hedging.rs` — Request Hedging

Races primary vs backup provider for latency-sensitive reads.

```rust
pub async fn hedged_send(
    primary: &dyn RpcTransport,
    backup: &dyn RpcTransport,
    req: JsonRpcRequest,
    hedge_delay: Duration,
) -> Result<JsonRpcResponse, TransportError>;
```

Only hedges safe methods (checks `method_safety::is_safe_to_retry()`). Write methods go to primary only.

### `multi_chain.rs` — Multi-Chain Router

Single entry point for multiple chains.

```rust
pub fn new() -> Self;
pub fn add_chain(&mut self, chain_id: u64, transport: Arc<dyn RpcTransport>);
pub async fn send_to(&self, chain_id: u64, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>;
pub async fn parallel(&self, requests: Vec<(u64, JsonRpcRequest)>) -> Vec<Result<JsonRpcResponse, TransportError>>;
pub fn chain_ids(&self) -> Vec<u64>;
pub fn health_summary(&self) -> Vec<(u64, HealthStatus)>;
```

### `routing.rs` — Archive vs Full Node Routing

Capability-based provider selection.

```rust
pub struct ProviderCapabilities {
    pub archive: bool,          // has full historical state
    pub trace: bool,            // supports debug_*/trace_* methods
    pub max_block_range: u64,   // max eth_getLogs range
    pub max_batch_size: usize,
    pub supported_methods: HashSet<String>,
}

pub fn analyze_request(method: &str, params: &[Value]) -> RequestRequirements;
pub fn select_capable_provider(providers: &[(usize, &ProviderCapabilities)], req: &RequestRequirements) -> Option<usize>;
```

Historical blocks (>256 from head) and trace methods require archive nodes.

---

## Safety & Cost

### `method_safety.rs` — Method Safety Classification

```rust
pub enum MethodSafety { Safe, Idempotent, Unsafe }

pub fn classify_method(method: &str) -> MethodSafety;
pub fn is_safe_to_retry(method: &str) -> bool;
pub fn is_safe_to_dedup(method: &str) -> bool;
pub fn is_cacheable(method: &str) -> bool;
```

**Safe**: All read methods (`eth_getBalance`, `eth_call`, `eth_getLogs`, etc.)
**Idempotent**: `eth_sendRawTransaction` (same raw tx = same hash, safe to re-send)
**Unsafe**: `eth_sendTransaction`, `personal_sendTransaction` (node-signed, may get different nonce)

### `cu_tracker.rs` — Compute Unit Budget Tracking

Per-provider CU consumption tracking with Alchemy-style cost table (19 methods).

```rust
pub struct CuCostTable { /* method -> CU cost */ }
pub struct CuTracker { /* per-provider consumption */ }

// CuCostTable
pub fn alchemy_defaults() -> Self;        // 19 methods pre-configured
pub fn cost_for(&self, method: &str) -> u32;

// CuTracker
pub fn record(&self, method: &str);       // track consumption
pub fn consumed(&self) -> u64;            // total CU used
pub fn remaining(&self) -> u64;           // budget left (u64::MAX if unlimited)
pub fn is_alert(&self) -> bool;           // past threshold (default 80%)
pub fn should_throttle(&self) -> bool;    // alert + throttle_near_limit enabled
pub fn snapshot(&self) -> CuSnapshot;     // serializable report
```

Default CU costs: `eth_blockNumber`=10, `eth_call`=26, `eth_getLogs`=75, `eth_sendRawTransaction`=250, `debug_traceTransaction`=309, `trace_block`=500.

### `rate_limit_headers.rs` — HTTP Rate-Limit Header Parsing

Parses standard and provider-specific headers from HTTP responses.

```rust
pub struct RateLimitInfo {
    pub limit: Option<u32>,         // X-RateLimit-Limit
    pub remaining: Option<u32>,     // X-RateLimit-Remaining
    pub reset_secs: Option<u32>,    // X-RateLimit-Reset
    pub retry_after_secs: Option<u32>, // Retry-After
    pub alchemy_cu_limit: Option<u32>,
    pub alchemy_cu_remaining: Option<u32>,
}

pub fn from_headers(headers: impl Iterator<Item = (&str, &str)>) -> RateLimitInfo;
pub fn should_backoff(&self) -> bool;              // remaining == 0 or retry_after set
pub fn suggested_wait(&self) -> Option<Duration>;  // best delay to respect limits
```

---

## EVM Helpers

### `gas.rs` — EIP-1559 Gas Estimation

```rust
pub enum GasSpeed { Slow, Standard, Fast, Urgent }

pub fn compute_gas_recommendation(
    base_fee: u64,
    priority_fee_samples: &[u64],
    speed: GasSpeed,
) -> GasRecommendation;

pub struct GasRecommendation {
    pub max_fee_per_gas: u64,
    pub max_priority_fee_per_gas: u64,
    pub speed: GasSpeed,
}
```

Base fee multipliers: Slow=1.0x, Standard=1.125x, Fast=1.25x, Urgent=1.5x.
Priority fee: percentile of samples (25th, 50th, 75th, 95th by speed tier).

### `mev.rs` — MEV Protection

Detects MEV-susceptible transactions and routes to private relays.

```rust
pub fn is_mev_susceptible(input_data: &str) -> bool;  // checks 12 known selectors
pub fn should_use_relay(input_data: &str, config: &MevConfig) -> bool;
pub fn relay_urls() -> Vec<&'static str>;  // Flashbots, etc.
```

Known MEV selectors: Uniswap V2 (`swapExactTokensForTokens`, `swapTokensForExactTokens`, etc.), Uniswap V3 (`exactInputSingle`, `exactOutputSingle`, `multicall`), WETH (`deposit`, `withdraw`).

---

## Transaction Lifecycle

### `tx.rs` — Data Structures

```rust
pub enum TxStatus { Pending, Included{..}, Confirmed{..}, Dropped, Replaced{..}, Failed{..} }
pub struct TrackedTx { tx_hash, from, nonce, status, gas_price, max_fee, ... }
pub struct TxTracker { /* thread-safe HashMap of tracked txs */ }
pub struct ReceiptPoller { /* exponential backoff schedule */ }
pub struct NonceLedger { /* confirmed vs pending nonce tracking with gap detection */ }
```

### `tx_lifecycle.rs` — RpcTransport Integration

Async functions that compose `TxTracker` with live RPC calls:

```rust
pub async fn poll_receipt(transport, tx_hash, poller) -> Result<Option<Value>, TransportError>;
pub async fn send_and_track(transport, tracker, raw_tx, from, nonce) -> Result<String, TransportError>;
pub async fn refresh_status(transport, tracker, tx_hash) -> Result<TxStatus, TransportError>;
pub async fn detect_stuck(transport, tracker, current_time) -> Vec<TrackedTx>;
```

### `pending_pool.rs` — Pending Tx Monitoring

```rust
pub struct PendingPoolMonitor { /* watches tx hashes */ }

pub fn watch(&self, tx_hash: String) -> bool;
pub fn unwatch(&self, tx_hash: &str);
pub async fn check_status(transport, tx_hash) -> Result<PendingTxStatus, TransportError>;

pub enum PendingTxStatus { Pending, Included { block_number }, NotFound }
```

---

## Lifecycle & Coordination

### `shutdown.rs` — Graceful Shutdown

```rust
pub struct ShutdownController { /* owns the signal */ }
pub struct ShutdownSignal { /* check/wait for shutdown */ }

let (ctrl, signal) = ShutdownController::new();
// In your loop:
if signal.is_shutdown() { break; }
// Or async:
signal.wait().await;
// Trigger:
ctrl.shutdown();

// Signal handler (SIGTERM/SIGINT):
pub fn install_signal_handler(controller: Arc<ShutdownController>);
// Drain with timeout:
pub async fn shutdown_with_timeout(controller, timeout) -> bool;
```

### `cancellation.rs` — Cancellation Tokens

Cooperative cancellation with parent/child hierarchy.

```rust
pub struct CancellationToken { /* cancel(), is_cancelled(), cancelled() */ }
pub struct CancellationChild { /* inherits parent cancellation */ }

let token = CancellationToken::new();
let child = token.child();

token.cancel();            // cancels token AND all children
child.is_cancelled();      // true (inherited from parent)

// Child-only cancellation doesn't propagate up:
let token2 = CancellationToken::new();
let child2 = token2.child();
child2.cancel();           // only child2 is cancelled
token2.is_cancelled();     // false
```

---

## Observability

### `metrics.rs` — Per-Provider Metrics + Prometheus

```rust
pub struct ProviderMetrics { /* atomic counters per provider */ }

pub fn record_success(&self, latency: Duration);
pub fn record_failure(&self);
pub fn record_rate_limit(&self);
pub fn record_circuit_open(&self);
pub fn snapshot(&self) -> MetricsSnapshot;
pub fn prometheus_lines(&self) -> String;

pub struct MetricsSnapshot {
    pub provider: String,
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub success_rate: f64,
    pub avg_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
    pub rate_limit_hits: u64,
    pub circuit_open_count: u64,
}

// Aggregate across providers:
pub struct RpcMetrics { /* Vec<Arc<ProviderMetrics>> */ }
pub fn prometheus_export(&self) -> String;  // all providers in one text blob
```

---

## HTTP Transport (`chainrpc-http`)

### `client.rs` — HttpRpcClient

```rust
pub fn new(url: impl Into<String>, config: HttpClientConfig) -> Self;
pub fn default_for(url: impl Into<String>) -> Self;
pub fn with_metrics(url, config, metrics: Arc<ProviderMetrics>) -> Self;
```

Built-in reliability:
- Retry loop with exponential backoff (only for safe methods)
- Circuit breaker check before each attempt
- Rate limiter (token bucket)
- Rate-limit header parsing from responses (adaptive)
- Metrics recording on success/failure

### `pool_from_urls()` — Quick Pool Setup

```rust
pub fn pool_from_urls(urls: &[&str]) -> Result<ProviderPool, TransportError>;
```

Creates an `HttpRpcClient` with default config for each URL, wraps in `ProviderPool`.

---

## WebSocket Transport (`chainrpc-ws`)

### `WsRpcClient` — WebSocket with Auto-Reconnect

```rust
pub async fn connect(url: &str) -> Result<Self, TransportError>;
```

Features: auto-reconnect on disconnect, subscription management (`eth_subscribe`/`eth_unsubscribe`), auto-resubscribe after reconnect.

---

## `solana` — Solana RPC Support

Extends ChainRPC to support Solana's JSON-RPC protocol with commitment levels, method safety, and CU costs.

### Public API

| Item | Description |
|------|-------------|
| `SolanaCommitment` | Enum: `Processed` / `Confirmed` / `Finalized` |
| `SolanaTransport` | Wraps `Arc<dyn RpcTransport>`, auto-injects commitment config |
| `SolanaCuCostTable` | Solana method CU costs (getProgramAccounts=100, getSlot=5) |
| `classify_solana_method()` | Safe / Idempotent / Unsafe for 50+ Solana methods |
| `is_solana_safe_to_retry()` | Retry safety check |
| `is_solana_cacheable()` | Cache safety check |
| `solana_mainnet_endpoints()` | Known mainnet RPC URLs |
| `solana_devnet_endpoints()` | Known devnet RPC URLs |

### Commitment Levels

| Level | Description | Safe for Indexing? |
|-------|-------------|-------------------|
| `Processed` | Node has processed, not confirmed | No |
| `Confirmed` | Supermajority voted (default) | No |
| `Finalized` | Rooted, cannot roll back | Yes |

### Method Safety (Solana)

| Classification | Methods |
|---------------|---------|
| Safe | getBalance, getSlot, getAccountInfo, getProgramAccounts, ... (50+) |
| Idempotent | sendTransaction |
| Unsafe | requestAirdrop |

**Tests**: 16

---

## `geo_routing` — Geographic Routing

Routes requests to the geographically closest provider with automatic fallback.

### Public API

| Item | Description |
|------|-------------|
| `Region` | Enum: UsEast, UsWest, EuWest, EuCentral, AsiaSoutheast, AsiaEast, SouthAmerica, Oceania |
| `GeoRouter` | Implements `RpcTransport` — routes by proximity, falls back on failure |
| `RegionHealthSummary` | Per-region health stats (latency, success/failure counts) |
| `RegionalEndpoints` | Pre-configured URLs for Alchemy, Ankr by region |
| `detect_region_from_env()` | Auto-detect from AWS_REGION, FLY_REGION, etc. |

### Routing Behavior

1. Try local region first
2. On retryable failure → next-closest region (by proximity)
3. Non-retryable errors propagate immediately
4. All regions exhausted → `AllProvidersDown`

### Proximity Table

| From | Nearest → Farthest |
|------|-------------------|
| UsEast | UsWest → EuWest → SouthAmerica → EuCentral → ... |
| AsiaEast | AsiaSoutheast → Oceania → UsWest → ... |
| EuWest | EuCentral → UsEast → SouthAmerica → ... |

**Tests**: 25

---

## `gas_bumper` — Transaction Gas Bumping

Speed up or cancel stuck transactions with EIP-1559 compliant gas replacement.

### Public API

| Item | Description |
|------|-------------|
| `BumpStrategy` | Percentage(bps), SpeedTier(GasSpeed), Fixed{max_fee, priority}, Double, Cancel |
| `BumpConfig` | min_bump_bps (10%), max_gas_price (500 gwei), max_bumps (5) |
| `BumpResult` | Computed new gas params + metadata |
| `compute_bump()` | Pure function — computes bumped gas parameters |
| `bump_and_send()` | Async — computes, signs (via closure), sends, updates tracker |
| `compute_cancel()` | Convenience — minimum-bump cancellation |

### EIP-1559 Replacement Rules

- Same nonce as stuck tx
- `maxPriorityFeePerGas` at least 10% higher
- `maxFeePerGas` at least 10% higher
- Capped at configurable maximum (default 500 gwei)

### Bump Strategies

| Strategy | Description |
|----------|-------------|
| `Percentage(1200)` | 12% increase (default) |
| `Double` | 2x current gas |
| `SpeedTier(GasSpeed::Urgent)` | Use fee history for urgent tier |
| `Fixed { max_fee, priority }` | Explicit values (enforces 10% minimum) |
| `Cancel` | Minimum bump for cancellation (0-value self-transfer) |

**Tests**: 15

---

## `reorg` — Chain Reorganization Detection

Detects chain reorgs at the RPC layer via a sliding block hash window.

### Public API

| Item | Description |
|------|-------------|
| `ReorgDetector` | Main detector with sliding window |
| `ReorgConfig` | window_size (128), safe_depth (64), use_finalized_tag |
| `ReorgEvent` | fork_block, depth, old_hash, new_hash, current_tip |
| `check_block()` | Synchronous check — returns `Option<ReorgEvent>` |
| `poll_and_check()` | Async — fetches block + checks in one call |
| `on_reorg()` | Register callback for reorg events |
| `safe_block()` | Returns tip - safe_depth |
| `fetch_finalized_block()` | Queries eth_getBlockByNumber("finalized") |

### Detection Flow

1. New block arrives → compare hash against stored hash
2. Hash mismatch → reorg detected at that height
3. Affected blocks removed from window
4. All registered callbacks fire with `ReorgEvent`
5. Event stored in reorg history

**Tests**: 18

---

## Provider Profiles (`chainrpc-providers`)

Pre-configured URL builders and CU cost tables.

```rust
// Alchemy
pub fn http_url(api_key: &str, chain: &str) -> String;
pub fn ws_url(api_key: &str, chain: &str) -> String;

// Infura
pub fn http_url(api_key: &str, chain: &str) -> String;

// Public (no API key)
pub fn ankr_url(chain: &str) -> String;
```
