# ChainRPC

Production-grade, multi-provider RPC transport layer for EVM blockchains.

[![crates.io](https://img.shields.io/crates/v/chainrpc-core)](https://crates.io/crates/chainrpc-core)
[![docs.rs](https://docs.rs/chainrpc-core/badge.svg)](https://docs.rs/chainrpc-core)
[![npm](https://img.shields.io/npm/v/@chainfoundry/chainrpc)](https://www.npmjs.com/package/@chainfoundry/chainrpc)
[![PyPI](https://img.shields.io/pypi/v/chainrpc)](https://pypi.org/project/chainrpc/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Built-in retry, circuit breaker, rate limiting, tiered caching, request deduplication, auto-batching, multi-chain routing, MEV protection, and Prometheus metrics — all composable via a single `Arc<dyn RpcTransport>` trait.

```
DedupTransport(CacheTransport(BackpressureTransport(ProviderPool(HttpRpcClient))))
```

Every layer wraps `Arc<dyn RpcTransport>` and itself implements `RpcTransport`. Stack what you need, skip what you don't.

---

## Quick Start

### Rust

```toml
# Cargo.toml
[dependencies]
chainrpc-core = "0.1"
chainrpc-http = "0.1"
```

```rust
use chainrpc_http::HttpRpcClient;
use chainrpc_core::transport::RpcTransport;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY");

    // Typed call — auto-deserializes the response
    let block: String = client.call(1, "eth_blockNumber", vec![]).await?;
    println!("Latest block: {block}");

    Ok(())
}
```

### Multi-Provider Failover

```rust
use chainrpc_http::pool_from_urls;

let pool = pool_from_urls(&[
    "https://eth-mainnet.g.alchemy.com/v2/KEY1",
    "https://mainnet.infura.io/v3/KEY2",
    "https://rpc.ankr.com/eth",
])?;

// Round-robin with automatic failover when a provider's circuit breaker opens
let block = pool.send(JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;
println!("Healthy: {}/{}", pool.healthy_count(), pool.len());
```

### Tiered Caching + Dedup

```rust
use chainrpc_core::cache::{CacheTransport, CacheConfig, CacheTierResolver};
use chainrpc_core::dedup::DedupTransport;

// Cache with smart tier resolution:
//   eth_getTransactionReceipt → Immutable (1h TTL)
//   eth_blockNumber → Volatile (2s TTL)
//   eth_sendRawTransaction → NeverCache
let cached = CacheTransport::new(Arc::new(client), CacheConfig {
    tier_resolver: Some(CacheTierResolver::new()),
    max_entries: 4096,
    ..Default::default()
});

// Deduplicate concurrent identical requests
let dedup = DedupTransport::new(Arc::new(cached));
```

---

## Features

### Policy Engine
- **Rate Limiter** — Token-bucket with CU-aware mode (knows `eth_getLogs` = 75 CU, `eth_blockNumber` = 10 CU)
- **Circuit Breaker** — Three-state (Closed/Open/HalfOpen), configurable failure threshold and reset timeout
- **Retry Policy** — Exponential backoff with jitter, respects method safety (never retries `eth_sendTransaction`)
- **Method Safety** — Classifies 40+ EVM methods as Safe / Idempotent / Unsafe

### Provider Pool
- 5 selection strategies: RoundRobin, Priority, WeightedRoundRobin, LatencyBased, Sticky
- Per-provider circuit breaker + automatic failover
- Atomic metrics (success/failure counts, latency, rate-limit hits)
- Background health checking

### Caching & Dedup
- 4-tier cache: Immutable (1h), SemiStable (5m), Volatile (2s), NeverCache
- Smart tier resolution based on method + parameters
- Reorg-aware invalidation (`invalidate_for_reorg(block)`)
- Request deduplication (N concurrent callers = 1 HTTP call)

### Advanced
- **Auto-Batching** — Collects individual calls into JSON-RPC batch requests within a time window
- **Multi-Chain Router** — Route by chain ID, parallel cross-chain queries
- **Request Hedging** — Race primary + backup, return first response
- **Backpressure** — Concurrency limiting, fail-fast when overloaded
- **Archive Routing** — Route historical queries to archive nodes
- **MEV Protection** — Detect 12 MEV-susceptible selectors, route to private relays
- **Gas Estimation** — EIP-1559 recommendations (Slow/Standard/Fast/Urgent)
- **Tx Lifecycle** — Send, track, poll receipt, detect stuck, nonce management, gas bumping
- **CU Budget Tracking** — Monitor compute unit consumption against monthly budgets
- **Solana Support** — Commitment levels (Processed/Confirmed/Finalized), 50+ method safety classification, CU costs
- **Geo-Aware Routing** — Route to geographically closest provider with automatic proximity-based fallback
- **Gas Bumping** — Speed up or cancel stuck transactions with EIP-1559 compliant replacement (Percentage/Double/SpeedTier/Cancel)
- **Reorg Detection** — Sliding block hash window, configurable safe depth, reorg callbacks for cache invalidation

### Lifecycle & Observability
- Graceful shutdown with signal handling and drain timeout
- Cooperative cancellation tokens (parent/child hierarchy)
- Per-provider Prometheus metrics export
- Structured tracing integration

---

## Crate Structure

```
chainrpc/
  crates/
    chainrpc-core/       # 31 modules — trait, types, all middleware
    chainrpc-http/       # HTTP transport (reqwest) with retry loop
    chainrpc-ws/         # WebSocket transport (tokio-tungstenite)
    chainrpc-providers/  # Pre-configured Alchemy, Infura, QuickNode, Chainstack, public profiles
  bindings/
    node/                # TypeScript (napi-rs)
    python/              # Python (PyO3 + maturin)
    go/                  # Go (cgo)
    java/                # Java (JNI)
  cli/                   # CLI tool (call, pool, bench)
  examples/              # 26 runnable examples
  docs/                  # Architecture, modules, API reference
```

---

## Examples

26 examples covering every module. See [examples/README.md](examples/README.md) for the full index.

| Category | Examples |
|----------|----------|
| **Basics** | [Simple Client](examples/01_simple_client.rs) | [Typed Calls](examples/02_typed_calls.rs) | [Provider Pool](examples/03_provider_pool.rs) | [Error Handling](examples/04_error_handling.rs) | [Custom Config](examples/05_custom_config.rs) |
| **Caching** | [Tiered Cache](examples/06_tiered_cache.rs) | [Dedup](examples/07_request_dedup.rs) | [Auto-Batch](examples/08_auto_batching.rs) |
| **Cost Control** | [Rate Limiting](examples/09_rate_limiting.rs) | [CU Budget](examples/10_cu_budget_tracking.rs) | [Circuit Breaker](examples/11_circuit_breaker.rs) |
| **Routing** | [Multi-Chain](examples/12_multi_chain.rs) | [Hedging](examples/13_request_hedging.rs) | [Backpressure](examples/14_backpressure.rs) | [Archive](examples/15_archive_routing.rs) | [Selection](examples/16_selection_strategies.rs) |
| **EVM** | [Gas](examples/17_gas_estimation.rs) | [MEV](examples/18_mev_protection.rs) | [Tx Lifecycle](examples/19_tx_lifecycle.rs) |
| **Lifecycle** | [Shutdown](examples/20_graceful_shutdown.rs) | [Cancellation](examples/21_cancellation.rs) | [Prometheus](examples/22_prometheus_metrics.rs) |
| **New Modules** | [Solana RPC](examples/23_solana_rpc.rs) | [Geo Routing](examples/24_geo_routing.rs) | [Gas Bumping](examples/25_gas_bumping.rs) | [Reorg Detection](examples/26_reorg_detection.rs) |

---

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](docs/ARCHITECTURE.md) | System diagram, crate structure, request flow, design principles |
| [Modules](docs/MODULES.md) | Detailed reference for all 31 modules with public API |
| [API Reference](docs/API-REFERENCE.md) | Quick-lookup tables for every type, trait, and function |
| [Use Cases](docs/USECASES.md) | 20 real-world patterns with complete Rust code |
| [Implementation](docs/IMPLEMENTATION.md) | Design decisions, concurrency patterns, internals |

---

## Language Bindings

| Language | Package | Install |
|----------|---------|---------|
| TypeScript | `@chainfoundry/chainrpc` | `npm install @chainfoundry/chainrpc` |
| Python | `chainrpc` | `pip install chainrpc` |
| Go | `chainrpc` | `go get github.com/DarshanKumar89/chainfoundry/chainrpc` |
| Java | `chainrpc` | Maven / Gradle |

---

## Test Coverage

```
262 tests, 0 failures

chainrpc-core:      250 tests (all 31 modules)
chainrpc-providers:   6 tests (URL construction)
chainrpc-ws:          3 tests (subscription management)
Doc-tests:            3 tests
```

```bash
cd chainrpc && cargo test --workspace
```

---

## License

MIT
