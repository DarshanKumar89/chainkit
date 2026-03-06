# ChainRPC — API Reference

Quick-reference for every public type, trait, and function.

---

## `chainrpc-core`

### Traits

| Trait | Methods | Description |
|-------|---------|-------------|
| `RpcTransport` | `send()`, `send_batch()`, `health()`, `url()`, `call<T>()` | Core async transport trait; object-safe via `Arc<dyn RpcTransport>` |

### Error Types

| Type | Variants |
|------|----------|
| `TransportError` | `Http`, `WebSocket`, `Rpc`, `RateLimited`, `CircuitOpen`, `AllProvidersDown`, `Timeout`, `Deserialization`, `Overloaded`, `Cancelled`, `Other` |

| Method | Returns |
|--------|---------|
| `is_retryable()` | `true` for `Http`, `WebSocket`, `Timeout`, `RateLimited` |
| `is_execution_error()` | `true` for `Rpc` |

### Wire Types (`request`)

| Type | Fields |
|------|--------|
| `JsonRpcRequest` | `jsonrpc`, `id: RpcId`, `method`, `params: Vec<Value>` |
| `JsonRpcResponse` | `jsonrpc`, `id: RpcId`, `result: Option<Value>`, `error: Option<JsonRpcError>` |
| `JsonRpcError` | `code: i64`, `message: String`, `data: Option<Value>` |
| `RpcId` | `Number(u64)` or `Str(String)` |

| Constructor | Description |
|-------------|-------------|
| `JsonRpcRequest::new(id, method, params)` | Explicit ID |
| `JsonRpcRequest::auto(method, params)` | Auto-incrementing atomic ID |
| `JsonRpcResponse::is_ok()` | `error` is `None` |
| `JsonRpcResponse::into_result()` | Extracts `result` or `Err(JsonRpcError)` |

### Health

| Enum | Variants |
|------|----------|
| `HealthStatus` | `Healthy`, `Degraded`, `Unhealthy`, `Unknown` |

---

### Policy Engine

#### `RateLimiterConfig`

| Field | Type | Default |
|-------|------|---------|
| `capacity` | `f64` | `300.0` |
| `refill_rate` | `f64` | `300.0` |

#### `TokenBucket`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `(config: RateLimiterConfig) -> Self` | |
| `try_acquire` | `(&self, cost: f64) -> bool` | Consume tokens |
| `wait_time` | `(&self, cost: f64) -> Duration` | Time until tokens available |
| `available` | `(&self) -> f64` | Current token count |

#### `RateLimiter`

| Method | Signature |
|--------|-----------|
| `new` | `(config: RateLimiterConfig) -> Self` |
| `try_acquire` | `(&self) -> bool` |
| `try_acquire_cost` | `(&self, cost: f64) -> bool` |
| `wait_time` | `(&self) -> Duration` |

#### `MethodAwareRateLimiter`

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `(config, cost_table: CuCostTable) -> Self` | |
| `try_acquire_method` | `(&self, method: &str) -> bool` | Uses CU cost for method |
| `wait_time_for_method` | `(&self, method: &str) -> Duration` | |
| `bucket` | `(&self) -> &TokenBucket` | Access underlying bucket |

#### `CircuitBreaker`

| Method | Signature |
|--------|-----------|
| `new` | `(config: CircuitBreakerConfig) -> Self` |
| `is_allowed` | `(&self) -> bool` |
| `record_success` | `(&self)` |
| `record_failure` | `(&self)` |
| `state` | `(&self) -> CircuitState` |

#### `CircuitBreakerConfig`

| Field | Type | Default |
|-------|------|---------|
| `failure_threshold` | `u32` | `5` |
| `reset_timeout` | `Duration` | `30s` |

#### `RetryPolicy`

| Method | Signature |
|--------|-----------|
| `new` | `(config: RetryConfig) -> Self` |
| `should_retry` | `(&self, attempt: u32) -> bool` |
| `delay` | `(&self, attempt: u32) -> Duration` |

---

### Provider Pool

#### `ProviderPool`

| Method | Signature |
|--------|-----------|
| `new` | `(transports: Vec<Arc<dyn RpcTransport>>, config) -> Self` |
| `new_with_metrics` | `(transports, config) -> Self` |
| `len` | `(&self) -> usize` |
| `is_empty` | `(&self) -> bool` |
| `healthy_count` | `(&self) -> usize` |
| `health_summary` | `(&self) -> Vec<(String, HealthStatus, String)>` |
| `health_report` | `(&self) -> Vec<serde_json::Value>` |
| `metrics` | `(&self) -> Vec<MetricsSnapshot>` |

Implements `RpcTransport` — round-robin with automatic failover.

#### `SelectionStrategy`

| Variant | Description |
|---------|-------------|
| `RoundRobin` | Distribute evenly |
| `Priority` | First healthy in registration order |
| `WeightedRoundRobin { weights }` | Proportional by weight |
| `LatencyBased` | EMA-smoothed, picks fastest |
| `Sticky { key }` | Consistent hashing (for nonce mgmt) |

#### `SelectionState`

| Method | Signature |
|--------|-----------|
| `new` | `(provider_count: usize) -> Self` |
| `select` | `(&self, strategy, count, is_allowed) -> Option<usize>` |
| `record_latency` | `(&self, index, latency: Duration)` |

---

### Caching

#### `CacheTransport`

| Method | Signature |
|--------|-----------|
| `new` | `(inner: Arc<dyn RpcTransport>, config: CacheConfig) -> Self` |
| `send` | `async (&self, req) -> Result<JsonRpcResponse, TransportError>` |
| `invalidate` | `(&self)` |
| `invalidate_method` | `(&self, method: &str)` |
| `invalidate_for_reorg` | `(&self, from_block: u64)` |
| `stats` | `(&self) -> CacheStats` |

#### `CacheTier`

| Variant | Default TTL |
|---------|-------------|
| `Immutable` | 1 hour |
| `SemiStable` | 5 minutes |
| `Volatile` | 2 seconds |
| `NeverCache` | `None` |

#### `DedupTransport`

| Method | Signature |
|--------|-----------|
| `new` | `(inner: Arc<dyn RpcTransport>) -> Self` |
| `send` | `async (&self, req) -> Result<JsonRpcResponse, TransportError>` |

---

### Advanced Features

#### `BatchingTransport`

| Method | Signature |
|--------|-----------|
| `new` | `(inner: Arc<dyn RpcTransport>, window: Duration) -> Arc<Self>` |

Implements `RpcTransport`.

#### `BackpressureTransport`

| Method | Signature |
|--------|-----------|
| `new` | `(inner: Arc<dyn RpcTransport>, max_in_flight: usize) -> Self` |
| `send` | `async (&self, req) -> Result<JsonRpcResponse, TransportError>` |
| `in_flight` | `(&self) -> usize` |
| `is_full` | `(&self) -> bool` |

#### `hedged_send`

```rust
pub async fn hedged_send(
    primary: &dyn RpcTransport,
    backup: &dyn RpcTransport,
    req: JsonRpcRequest,
    hedge_delay: Duration,
) -> Result<JsonRpcResponse, TransportError>;
```

#### `ChainRouter`

| Method | Signature |
|--------|-----------|
| `new` | `() -> Self` |
| `add_chain` | `(&mut self, chain_id: u64, transport: Arc<dyn RpcTransport>)` |
| `send_to` | `async (&self, chain_id, req) -> Result<JsonRpcResponse, TransportError>` |
| `parallel` | `async (&self, Vec<(u64, JsonRpcRequest)>) -> Vec<Result<...>>` |
| `chain_ids` | `(&self) -> Vec<u64>` |
| `health_summary` | `(&self) -> Vec<(u64, HealthStatus)>` |

---

### Safety & Cost

#### `classify_method`

```rust
pub fn classify_method(method: &str) -> MethodSafety;  // Safe | Idempotent | Unsafe
pub fn is_safe_to_retry(method: &str) -> bool;
pub fn is_safe_to_dedup(method: &str) -> bool;
pub fn is_cacheable(method: &str) -> bool;
```

#### `CuCostTable`

| Method | Signature |
|--------|-----------|
| `alchemy_defaults` | `() -> Self` |
| `new` | `(default_cost: u32) -> Self` |
| `set_cost` | `(&mut self, method: &str, cost: u32)` |
| `cost_for` | `(&self, method: &str) -> u32` |

#### `CuTracker`

| Method | Signature |
|--------|-----------|
| `new` | `(url, cost_table, budget_config) -> Self` |
| `record` | `(&self, method: &str)` |
| `consumed` | `(&self) -> u64` |
| `remaining` | `(&self) -> u64` |
| `usage_fraction` | `(&self) -> f64` |
| `is_alert` | `(&self) -> bool` |
| `is_exhausted` | `(&self) -> bool` |
| `should_throttle` | `(&self) -> bool` |
| `reset` | `(&self)` |
| `per_method_usage` | `(&self) -> HashMap<String, u64>` |
| `snapshot` | `(&self) -> CuSnapshot` |

#### `RateLimitInfo`

| Method | Signature |
|--------|-----------|
| `from_headers` | `(headers: impl Iterator<Item = (&str, &str)>) -> Self` |
| `should_backoff` | `(&self) -> bool` |
| `suggested_wait` | `(&self) -> Option<Duration>` |

---

### Routing

#### `ProviderCapabilities`

| Field | Type |
|-------|------|
| `archive` | `bool` |
| `trace` | `bool` |
| `max_block_range` | `u64` |
| `max_batch_size` | `usize` |
| `supported_methods` | `HashSet<String>` |

```rust
pub fn analyze_request(method: &str, params: &[Value]) -> RequestRequirements;
pub fn select_capable_provider(providers, requirements) -> Option<usize>;
```

---

### EVM Helpers

#### Gas

```rust
pub fn compute_gas_recommendation(base_fee: u64, samples: &[u64], speed: GasSpeed) -> GasRecommendation;
pub fn apply_gas_margin(value: u64, margin_bps: u64) -> u64;
```

#### MEV

```rust
pub fn is_mev_susceptible(input_data: &str) -> bool;
pub fn should_use_relay(input_data: &str, config: &MevConfig) -> bool;
pub fn relay_urls() -> Vec<&'static str>;
```

---

### Transaction Lifecycle

#### `TxTracker`

| Method | Signature |
|--------|-----------|
| `new` | `(config: TxTrackerConfig) -> Self` |
| `track` | `(&self, tx: TrackedTx)` |
| `untrack` | `(&self, tx_hash: &str)` |
| `update_status` | `(&self, tx_hash: &str, status: TxStatus)` |
| `pending` | `(&self) -> Vec<TrackedTx>` |
| `stuck` | `(&self, current_time: u64) -> Vec<TrackedTx>` |
| `get` | `(&self, tx_hash: &str) -> Option<TrackedTx>` |
| `count` | `(&self) -> usize` |

#### `NonceLedger`

| Method | Signature |
|--------|-----------|
| `next` | `(&self, address: &str) -> u64` |
| `set_confirmed` | `(&self, address: &str, nonce: u64)` |
| `mark_pending` | `(&self, address: &str, nonce: u64)` |
| `confirm` | `(&self, address: &str, nonce: u64)` |
| `gaps` | `(&self, address: &str) -> Vec<u64>` |

#### `tx_lifecycle` functions

```rust
pub async fn poll_receipt(transport, tx_hash, poller) -> Result<Option<Value>, TransportError>;
pub async fn send_and_track(transport, tracker, raw_tx, from, nonce) -> Result<String, TransportError>;
pub async fn refresh_status(transport, tracker, tx_hash) -> Result<TxStatus, TransportError>;
pub async fn detect_stuck(transport, tracker, current_time) -> Vec<TrackedTx>;
```

---

### Lifecycle

#### `ShutdownController`

| Method | Signature |
|--------|-----------|
| `new` | `() -> (Self, ShutdownSignal)` |
| `shutdown` | `(&self)` |
| `signal` | `(&self) -> ShutdownSignal` |

#### `ShutdownSignal`

| Method | Signature |
|--------|-----------|
| `is_shutdown` | `(&self) -> bool` |
| `wait` | `async (&mut self)` |

```rust
pub fn install_signal_handler(controller: Arc<ShutdownController>);
pub async fn shutdown_with_timeout(controller, timeout: Duration) -> bool;
```

#### `CancellationToken`

| Method | Signature |
|--------|-----------|
| `new` | `() -> Self` |
| `cancel` | `(&self)` |
| `is_cancelled` | `(&self) -> bool` |
| `child` | `(&self) -> CancellationChild` |
| `cancelled` | `async (&self)` |

---

### Observability

#### `ProviderMetrics`

| Method | Signature |
|--------|-----------|
| `new` | `(provider: &str) -> Self` |
| `record_success` | `(&self, latency: Duration)` |
| `record_failure` | `(&self)` |
| `record_rate_limit` | `(&self)` |
| `record_circuit_open` | `(&self)` |
| `snapshot` | `(&self) -> MetricsSnapshot` |
| `prometheus_lines` | `(&self) -> String` |

#### `RpcMetrics`

| Method | Signature |
|--------|-----------|
| `new` | `(providers: Vec<Arc<ProviderMetrics>>) -> Self` |
| `prometheus_export` | `(&self) -> String` |

---

## `chainrpc-http`

| Item | Signature |
|------|-----------|
| `HttpRpcClient::new` | `(url, config: HttpClientConfig) -> Self` |
| `HttpRpcClient::default_for` | `(url) -> Self` |
| `HttpRpcClient::with_metrics` | `(url, config, metrics) -> Self` |
| `pool_from_urls` | `(urls: &[&str]) -> Result<ProviderPool, TransportError>` |
| `BatchingTransport` | Re-export from `chainrpc_core::batch` |

---

## `chainrpc-ws`

| Item | Signature |
|------|-----------|
| `WsRpcClient::connect` | `async (url: &str) -> Result<Self, TransportError>` |

---

## `chainrpc-providers`

| Function | Signature |
|----------|-----------|
| `alchemy::http_url` | `(api_key: &str, chain: &str) -> String` |
| `alchemy::ws_url` | `(api_key: &str, chain: &str) -> String` |
| `infura::http_url` | `(api_key: &str, chain: &str) -> String` |
| `public::ankr_url` | `(chain: &str) -> String` |

---

## Solana Support (`solana`)

#### `SolanaCommitment`

| Variant | Description |
|---------|-------------|
| `Processed` | Node has processed, not confirmed |
| `Confirmed` | Supermajority voted (default) |
| `Finalized` | Rooted, cannot roll back |

#### `SolanaTransport`

| Method | Signature |
|--------|-----------|
| `new` | `(inner: Arc<dyn RpcTransport>, commitment: SolanaCommitment) -> Self` |
| `send` | `async (&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>` |
| `commitment` | `(&self) -> SolanaCommitment` |
| `set_commitment` | `(&self, commitment: SolanaCommitment)` |

Implements `RpcTransport` — auto-injects commitment config into Solana RPC params.

#### `SolanaCuCostTable`

| Method | Signature |
|--------|-----------|
| `defaults` | `() -> Self` |
| `cost_for` | `(&self, method: &str) -> u32` |

Default costs: `getProgramAccounts`=100, `getSlot`=5, `getAccountInfo`=10, `getBalance`=5.

#### Solana Safety Functions

```rust
pub fn classify_solana_method(method: &str) -> MethodSafety;
pub fn is_solana_safe_to_retry(method: &str) -> bool;
pub fn is_solana_cacheable(method: &str) -> bool;
pub fn solana_mainnet_endpoints() -> Vec<&'static str>;
pub fn solana_devnet_endpoints() -> Vec<&'static str>;
```

---

## Geographic Routing (`geo_routing`)

#### `Region`

| Variant | Description |
|---------|-------------|
| `UsEast` | US East Coast |
| `UsWest` | US West Coast |
| `EuWest` | Europe West |
| `EuCentral` | Europe Central |
| `AsiaSoutheast` | Southeast Asia |
| `AsiaEast` | East Asia |
| `SouthAmerica` | South America |
| `Oceania` | Australia / Oceania |

#### `GeoRouter`

| Method | Signature |
|--------|-----------|
| `new` | `(local_region: Region, providers: Vec<(Region, Arc<dyn RpcTransport>)>) -> Self` |
| `send` | `async (&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError>` |
| `health_summary` | `(&self) -> Vec<RegionHealthSummary>` |

Implements `RpcTransport` — routes by proximity, falls back on failure.

#### `RegionalEndpoints`

| Method | Signature |
|--------|-----------|
| `alchemy` | `(api_key: &str) -> Vec<(Region, String)>` |
| `ankr` | `() -> Vec<(Region, String)>` |

#### `detect_region_from_env`

```rust
pub fn detect_region_from_env() -> Option<Region>;
// Reads AWS_REGION, FLY_REGION, RENDER_REGION, etc.
```

---

## Gas Bumping (`gas_bumper`)

#### `BumpStrategy`

| Variant | Description |
|---------|-------------|
| `Percentage(u32)` | Increase by basis points (1200 = 12%) |
| `SpeedTier(GasSpeed)` | Use fee history for speed tier |
| `Fixed { max_fee, priority }` | Explicit values (enforces 10% min) |
| `Double` | 2x current gas |
| `Cancel` | Minimum bump for cancellation |

#### `BumpConfig`

| Field | Type | Default |
|-------|------|---------|
| `min_bump_bps` | `u32` | `1000` (10%) |
| `max_gas_price` | `u64` | `500_000_000_000` (500 gwei) |
| `max_bumps` | `u32` | `5` |

#### `BumpResult`

| Field | Type |
|-------|------|
| `new_max_fee` | `u64` |
| `new_priority_fee` | `u64` |
| `bump_number` | `u32` |
| `strategy_used` | `BumpStrategy` |
| `capped` | `bool` |

#### Gas Bump Functions

```rust
pub fn compute_bump(
    current_max_fee: u64,
    current_priority: u64,
    strategy: &BumpStrategy,
    config: &BumpConfig,
) -> Result<BumpResult, TransportError>;

pub async fn bump_and_send(
    transport: &dyn RpcTransport,
    tracker: &TxTracker,
    tx_hash: &str,
    strategy: &BumpStrategy,
    config: &BumpConfig,
    sign: impl Fn(u64, u64) -> Vec<u8>,
) -> Result<String, TransportError>;

pub fn compute_cancel(
    current_max_fee: u64,
    current_priority: u64,
    config: &BumpConfig,
) -> Result<BumpResult, TransportError>;
```

---

## Reorg Detection (`reorg`)

#### `ReorgDetector`

| Method | Signature |
|--------|-----------|
| `new` | `(config: ReorgConfig) -> Self` |
| `check_block` | `(&self, number: u64, hash: &str) -> Option<ReorgEvent>` |
| `poll_and_check` | `async (&self, transport: &dyn RpcTransport) -> Result<Option<ReorgEvent>, TransportError>` |
| `on_reorg` | `(&self, callback: impl Fn(&ReorgEvent) + Send + Sync + 'static)` |
| `safe_block` | `(&self) -> u64` |
| `fetch_finalized_block` | `async (&self, transport: &dyn RpcTransport) -> Result<u64, TransportError>` |
| `window_size` | `(&self) -> usize` |
| `reorg_history` | `(&self) -> Vec<ReorgEvent>` |

#### `ReorgConfig`

| Field | Type | Default |
|-------|------|---------|
| `window_size` | `usize` | `128` |
| `safe_depth` | `u64` | `64` |
| `use_finalized_tag` | `bool` | `false` |

#### `ReorgEvent`

| Field | Type |
|-------|------|
| `fork_block` | `u64` |
| `depth` | `u64` |
| `old_hash` | `String` |
| `new_hash` | `String` |
| `current_tip` | `u64` |
