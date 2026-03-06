# ChainRPC — Implementation Details & Design Decisions

Internal design rationale, concurrency patterns, and trade-offs.

---

## Core Design Decisions

### 1. `Arc<dyn RpcTransport>` as the Universal Composability Primitive

Every middleware (cache, dedup, batch, backpressure, pool) wraps `Arc<dyn RpcTransport>` and itself implements `RpcTransport`. This means layers stack naturally:

```
DedupTransport(CacheTransport(BackpressureTransport(ProviderPool(HttpRpcClient))))
```

Each layer only knows about the trait — zero coupling between modules.

**Trade-off**: Virtual dispatch (dyn trait) adds ~1ns overhead per call. This is negligible compared to network latency (5-100ms). We chose composition over monomorphization because users need runtime flexibility (different provider counts, optional layers).

### 2. `async_trait` for Object Safety

The `RpcTransport` trait uses `#[async_trait]` which boxes the returned future. This is required for object safety (`Arc<dyn RpcTransport>`).

**Alternative considered**: Using `impl Future` return types. Rejected because it breaks object safety and prevents the composable architecture above.

### 3. `OnceLock` Instead of `LazyLock`

Method safety sets and other static data use `std::sync::OnceLock` instead of `LazyLock`. This is because the workspace `rust-version = "1.75"` and `LazyLock` was stabilized in Rust 1.80.

```rust
fn unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(|| { /* ... */ })
}
```

### 4. Standalone Cargo Workspace (Not Root Workspace)

ChainRPC is a standalone Cargo workspace at `chainkit/chainrpc/`, not a member of a root `chainkit/Cargo.toml`. This was an explicit user requirement — each module (chaincodec, chainerrors, chainrpc, chainindex) is independently buildable, testable, and publishable.

---

## Concurrency Patterns

### Atomic Counters for Hot-Path Metrics

`ProviderMetrics` uses `AtomicU64` for all counters to avoid mutex contention on the request hot path:

```rust
pub struct ProviderMetrics {
    total_requests: AtomicU64,
    successful_requests: AtomicU64,
    failed_requests: AtomicU64,
    total_latency_us: AtomicU64,
    min_latency_us: AtomicU64,  // CAS loop for min
    max_latency_us: AtomicU64,  // CAS loop for max
    rate_limit_hits: AtomicU64,
    circuit_open_count: AtomicU64,
}
```

Min/max latency uses compare-and-swap loops:
```rust
loop {
    let current = self.min_latency_us.load(Ordering::Relaxed);
    if latency_us >= current { break; }
    match self.min_latency_us.compare_exchange_weak(
        current, latency_us, Ordering::Relaxed, Ordering::Relaxed
    ) {
        Ok(_) => break,
        Err(_) => continue,
    }
}
```

### Mutex for Non-Hot-Path State

State that's accessed infrequently (nonce ledger, CU per-method breakdown, cache internals) uses `std::sync::Mutex`. We don't use `tokio::sync::Mutex` because these locks are never held across `.await` points.

### Watch Channel for Shutdown/Cancellation

`ShutdownController` and `CancellationToken` both use `tokio::sync::watch<bool>`:
- Single-producer, multi-consumer
- Receivers can poll (`is_shutdown()`) or async wait (`wait()`)
- Zero allocations after creation

### Semaphore for Backpressure

`BackpressureTransport` uses `tokio::sync::Semaphore` for concurrency limiting. `try_acquire()` is non-blocking — returns `TransportError::Overloaded` immediately when full. No queue, no waiting — that's the point of backpressure (fail fast).

---

## Cache Implementation Details

### Tiered TTL Resolution

The cache resolves TTL in this priority:
1. If `tier_resolver` is `Some` → resolver decides tier → tier provides TTL
2. If `tier_resolver` is `None` → flat mode: `cacheable_methods` set + `default_ttl`

This maintains full backward compatibility while adding the tier system.

### Block Param Detection

For `eth_getBlockByNumber`, the resolver inspects the first parameter:
- `"latest"`, `"pending"`, `"earliest"`, `"safe"`, `"finalized"` → `Volatile` (2s TTL)
- `"0x1a2b3c"` (hex string starting with `0x`) → `Immutable` (1h TTL)
- JSON number → `Immutable`

This means the same method gets different TTLs based on parameters — a feature unique to ChainRPC.

### Reorg Invalidation

`CacheEntry` stores an optional `block_ref: Option<u64>` extracted at cache-insert time. On reorg, `invalidate_for_reorg(from_block)` removes entries where `block_ref >= from_block`.

Currently only `eth_getBlockByNumber` and `eth_getTransactionByBlockNumberAndIndex` have block ref extraction. This is intentionally conservative — other methods (like `eth_getLogs` with `fromBlock`/`toBlock`) could be added but the param parsing is more complex and not needed for the common case.

### LRU Eviction

The cache uses a simple "oldest entry" eviction when `max_entries` is exceeded. This is O(n) per eviction (scan all entries for min `inserted_at`). For the typical `max_entries` of 1024-8192, this is fast enough (sub-microsecond). A proper LRU with a doubly-linked list would be O(1) but adds complexity.

---

## Rate Limiter Details

### Token Bucket Algorithm

Standard token bucket with continuous refill:
```
tokens = min(capacity, tokens + elapsed_seconds * refill_rate)
```

Refill happens lazily on each `try_acquire()` call — no background task needed.

### CU-Aware Rate Limiting

`MethodAwareRateLimiter` wraps `TokenBucket` with `CuCostTable`. The table maps 19 EVM methods to their Alchemy CU costs. Unknown methods use the `default_cost` (50 CU).

Key insight: `eth_getLogs` at 75 CU drains the bucket 7.5x faster than `eth_blockNumber` at 10 CU. This prevents a burst of expensive calls from starving cheap ones.

### Adaptive Rate Limiting

The HTTP client parses rate-limit headers from responses:
- `X-RateLimit-Remaining: 0` → stop sending
- `Retry-After: 5` → wait 5 seconds
- `alchemy-cu-remaining: 100` → provider-specific budget

The `adaptive_remaining` field on `HttpRpcClient` is an `AtomicU32` updated from response headers. The retry loop checks this before sending.

---

## Circuit Breaker Details

Three-state machine:

```
        record_failure() × threshold
Closed ──────────────────────────────▶ Open
  ▲                                     │
  │ record_success()                    │ reset_timeout elapsed
  │                                     ▼
  └──────────────────────────────── HalfOpen
                                   (allow 1 probe)
        record_failure() ──────────▶ Open (re-open)
```

**Implementation**: Uses `Mutex<CircuitState>` with a `last_failure: Instant` timestamp. `is_allowed()` checks the state — in `Open`, it checks if `reset_timeout` has elapsed and transitions to `HalfOpen`.

**Thread safety**: The mutex is only held for the duration of a state check/transition (nanoseconds). Never held across await points.

---

## Provider Pool Selection

### Round-Robin with Skip

The default pool selection is round-robin via `AtomicUsize` cursor. When a provider's circuit is open, it's skipped and the next one is tried. If all providers are down, `AllProvidersDown` is returned.

```rust
let start = cursor.fetch_add(1, Relaxed) % len;
for i in 0..len {
    let idx = (start + i) % len;
    if slots[idx].circuit.is_allowed() {
        return Some(&slots[idx]);
    }
}
None // AllProvidersDown
```

### Latency-Based Selection (EMA)

Uses exponential moving average with 30/70 weighting (30% new sample, 70% history):

```rust
new_ema = (new_latency_us * 3 + old_ema * 7) / 10
```

This smooths out spiky latency while still adapting to sustained changes.

### Sticky Selection (Consistent Hashing)

Uses `DefaultHasher` to hash the key (e.g. sender address) to a provider index:

```rust
let hash = DefaultHasher::hash(key);
let preferred = hash % provider_count;
```

If the preferred provider is down, falls back to the next healthy one. This keeps nonce management consistent (same sender always goes to the same provider) while still failing over.

---

## Method Safety Classification

### Why Three Levels?

- **Safe**: Read-only. Retry freely, deduplicate, cache.
- **Idempotent**: `eth_sendRawTransaction`. The raw transaction includes the signature, so re-submitting the same bytes produces the same tx hash. Safe to re-send if you're unsure whether the first attempt succeeded.
- **Unsafe**: `eth_sendTransaction`. The node signs with its own key and may assign a different nonce. NEVER auto-retry — could cause double-spend.

### Wiring into HTTP Client

The HTTP client's retry loop checks `is_safe_to_retry(method)` before retrying:
```rust
for attempt in 0..max_retries {
    match self.send_once(req.clone()).await {
        Err(e) if e.is_retryable() && is_safe_to_retry(&req.method) => {
            sleep(retry.delay(attempt)).await;
            continue;
        }
        result => return result,
    }
}
```

---

## Auto-Batcher

### Why Transport-Agnostic?

The batcher was originally in `chainrpc-http`. We moved it to `chainrpc-core` because batching is useful for any transport that supports `send_batch()` (HTTP, WebSocket, even mock transports in tests).

### Single-Item Optimization

When only one request arrives within the batch window, it's sent via `send()` instead of `send_batch()`. This avoids the overhead of JSON array wrapping for the common case.

### Background Flush Task

The batcher spawns a tokio task that:
1. Waits for the first request (`rx.recv()`)
2. Collects all requests arriving within `window` duration
3. If 1 request → `send()`, if >1 → `send_batch()`
4. Routes responses back via `oneshot` channels

---

## MEV Detection

### Selector-Based Approach

MEV detection checks the first 4 bytes (function selector) of transaction calldata against 12 known MEV-susceptible selectors. This is a conservative approach — it catches common DEX swaps but not every possible MEV opportunity.

Known selectors:
```
0x38ed1739  swapExactTokensForTokens (UniV2)
0x8803dbee  swapTokensForExactTokens (UniV2)
0x7ff36ab5  swapExactETHForTokens (UniV2)
0x18cbafe5  swapExactTokensForETH (UniV2)
0xfb3bdb41  swapETHForExactTokens (UniV2)
0x5c11d795  swapExactTokensForTokensSupportingFeeOnTransferTokens (UniV2)
0x414bf389  exactInputSingle (UniV3)
0xdb3e2198  exactOutputSingle (UniV3)
0xac9650d8  multicall (UniV3)
0x04e45aaf  exactInputSingle (UniV3 newer)
0xd0e30db0  deposit (WETH)
0x2e1a7d4d  withdraw (WETH)
```

**Why not ABI-decode?** We intentionally avoid full ABI decoding because:
1. It requires pulling in the ABI decoder (dependency bloat)
2. Selector matching is sub-microsecond
3. False positives are harmless (sending a non-MEV tx to Flashbots just adds latency)
4. False negatives are the real risk, and the selector list can be extended

---

## Gas Estimation

### Percentile-Based Priority Fee

Given a set of priority fee samples (from `eth_feeHistory`), the recommendation picks a percentile based on speed:

| Speed | Priority Fee Percentile | Base Fee Multiplier |
|-------|------------------------|---------------------|
| Slow | 25th | 1.0x |
| Standard | 50th (median) | 1.125x |
| Fast | 75th | 1.25x |
| Urgent | 95th | 1.5x |

The base fee multiplier accounts for base fee increases over the next few blocks (EIP-1559 can increase base fee by up to 12.5% per block).

---

## Nonce Management

### Dual-Track Nonces

`NonceLedger` maintains two nonce counters per address:
- **confirmed**: Last nonce known to be mined (from `eth_getTransactionCount` or receipt confirmation)
- **pending**: Highest nonce assigned locally but not yet confirmed

`next()` returns `max(confirmed + 1, pending + 1)`, ensuring no nonce reuse.

### Gap Detection

`gaps()` returns nonces between confirmed and pending that haven't been observed:
```
confirmed = 3, pending = 7 → gaps = [4, 5, 6]
```

This helps detect:
- Dropped transactions (gaps that never fill)
- Out-of-order confirmation (gaps that fill later)
- Stuck nonce sequences (long-lived gaps)

---

## Test Strategy

### Mock Transports

Tests use lightweight mock transports that implement `RpcTransport`:

```rust
struct CountingTransport { call_count: AtomicU64 }
struct FailingTransport { error: TransportError }
struct ConfigurableTransport { responses: HashMap<String, Value> }
```

No network calls in any test. All 188 tests run in ~2 seconds.

### Timing-Sensitive Tests

Cache TTL and circuit breaker tests use short durations (50-100ms) with `tokio::time::sleep`. The volatile cache tier test uses the real 2-second TTL with a 2.1-second sleep.

### Golden Fixture Pattern

Not used in chainrpc (chainrpc doesn't decode — that's chaincodec). Instead, chainrpc tests use deterministic mock responses and verify behavior (retry count, circuit state, cache hit/miss) rather than output content.
