# ChainRPC Examples

26 runnable examples covering every module in ChainRPC — from basic RPC calls to production-grade middleware stacks.

---

## Getting Started

```bash
# Run any example (requires a live RPC endpoint)
cd chainrpc
cargo run --example 01_simple_client

# Or run with tracing enabled
RUST_LOG=info cargo run --example 03_provider_pool
```

> Most examples use placeholder URLs (`https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY`). Replace with your actual provider API key.

---

## Examples by Category

### Basics (01-05)

| # | File | What You'll Learn |
|---|------|-------------------|
| 01 | [`01_simple_client.rs`](01_simple_client.rs) | Create an HTTP client, make raw and typed RPC calls. Built-in retry + circuit breaker. |
| 02 | [`02_typed_calls.rs`](02_typed_calls.rs) | Use `client.call::<T>()` for type-safe responses — block numbers, balances, chain IDs. |
| 03 | [`03_provider_pool.rs`](03_provider_pool.rs) | Multi-provider failover with `ProviderPool`. Round-robin, health checks, metrics. |
| 04 | [`04_error_handling.rs`](04_error_handling.rs) | Handle `TransportError` variants — retryable errors, RPC errors, circuit open, overloaded. |
| 05 | [`05_custom_config.rs`](05_custom_config.rs) | Customize `HttpClientConfig` — timeouts, retry policy, rate limiter, circuit breaker. |

### Caching & Dedup (06-08)

| # | File | What You'll Learn |
|---|------|-------------------|
| 06 | [`06_tiered_cache.rs`](06_tiered_cache.rs) | 4-tier response cache (Immutable/SemiStable/Volatile/NeverCache) with reorg invalidation. |
| 07 | [`07_request_dedup.rs`](07_request_dedup.rs) | Deduplicate concurrent identical requests — 3 callers, 1 HTTP call. |
| 08 | [`08_auto_batching.rs`](08_auto_batching.rs) | Auto-batch individual calls into JSON-RPC batch requests within a time window. |

### Rate Limiting & Cost (09-11)

| # | File | What You'll Learn |
|---|------|-------------------|
| 09 | [`09_rate_limiting.rs`](09_rate_limiting.rs) | Token-bucket rate limiter + CU-aware `MethodAwareRateLimiter` (knows `eth_getLogs` = 75 CU). |
| 10 | [`10_cu_budget_tracking.rs`](10_cu_budget_tracking.rs) | Track compute unit consumption against a monthly budget. Alert/throttle near limits. |
| 11 | [`11_circuit_breaker.rs`](11_circuit_breaker.rs) | Three-state circuit breaker (Closed -> Open -> HalfOpen) with configurable thresholds. |

### Multi-Chain & Routing (12-16)

| # | File | What You'll Learn |
|---|------|-------------------|
| 12 | [`12_multi_chain.rs`](12_multi_chain.rs) | `ChainRouter` — route by chain ID, parallel cross-chain queries, health summary. |
| 13 | [`13_request_hedging.rs`](13_request_hedging.rs) | Race primary + backup provider — return whichever responds first. |
| 14 | [`14_backpressure.rs`](14_backpressure.rs) | Concurrency limiting with `BackpressureTransport`. Fail fast when overloaded. |
| 15 | [`15_archive_routing.rs`](15_archive_routing.rs) | Route historical queries to archive nodes, recent queries to full nodes. |
| 16 | [`16_selection_strategies.rs`](16_selection_strategies.rs) | All 5 strategies — RoundRobin, Priority, WeightedRoundRobin, LatencyBased, Sticky. |

### EVM Helpers (17-19)

| # | File | What You'll Learn |
|---|------|-------------------|
| 17 | [`17_gas_estimation.rs`](17_gas_estimation.rs) | EIP-1559 gas recommendations — Slow/Standard/Fast/Urgent with base fee multipliers. |
| 18 | [`18_mev_protection.rs`](18_mev_protection.rs) | Detect MEV-susceptible txs (12 selectors) and route to private relays. |
| 19 | [`19_tx_lifecycle.rs`](19_tx_lifecycle.rs) | Full tx lifecycle — send, track, poll receipt, detect stuck, nonce management. |

### Lifecycle & Observability (20-22)

| # | File | What You'll Learn |
|---|------|-------------------|
| 20 | [`20_graceful_shutdown.rs`](20_graceful_shutdown.rs) | `ShutdownController` + signal handler — drain in-flight requests on SIGTERM. |
| 21 | [`21_cancellation.rs`](21_cancellation.rs) | Cooperative cancellation tokens with parent/child hierarchy. |
| 22 | [`22_prometheus_metrics.rs`](22_prometheus_metrics.rs) | Per-provider metrics + Prometheus text export for monitoring. |

### Solana, Geo Routing, Gas Bumping & Reorgs (23-26)

| # | File | What You'll Learn |
|---|------|-------------------|
| 23 | [`23_solana_rpc.rs`](23_solana_rpc.rs) | Solana RPC with commitment levels (Processed/Confirmed/Finalized), method safety, CU costs. |
| 24 | [`24_geo_routing.rs`](24_geo_routing.rs) | Geographic load balancing — route to closest region, auto-fallback by proximity. |
| 25 | [`25_gas_bumping.rs`](25_gas_bumping.rs) | Speed up or cancel stuck transactions with EIP-1559 compliant gas replacement strategies. |
| 26 | [`26_reorg_detection.rs`](26_reorg_detection.rs) | Chain reorg detection via sliding block hash window with cache invalidation callbacks. |

---

## Production Stack

Example 05 shows individual config. Here's how to compose everything together:

```
DedupTransport
  -> CacheTransport (tiered TTL + reorg invalidation)
    -> BackpressureTransport (max 200 in-flight)
      -> ProviderPool (Alchemy + Infura + Ankr)
          -> HttpRpcClient (retry + circuit breaker + rate limiter)
```

See [USECASES.md](../docs/USECASES.md#composing-layers) for the full code.

---

## Further Reading

| Doc | Description |
|-----|-------------|
| [Architecture](../docs/ARCHITECTURE.md) | System diagram, request flow, design principles |
| [Modules](../docs/MODULES.md) | Detailed reference for all 31 modules |
| [API Reference](../docs/API-REFERENCE.md) | Quick-lookup tables for every type and method |
| [Use Cases](../docs/USECASES.md) | 20 real-world patterns with full code |
| [Implementation](../docs/IMPLEMENTATION.md) | Design decisions, concurrency patterns, internals |
