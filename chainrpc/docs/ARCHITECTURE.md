# ChainRPC — Architecture Overview

> Production-grade, multi-provider RPC transport layer for EVM blockchains.

## System Diagram

```
┌──────────────────────────────────────────────────────────────────────────┐
│                          YOUR APPLICATION                                │
│   Indexers  │  Bots  │  Wallets  │  Analytics  │  dApps  │  Backends    │
├──────────────────────────────────────────────────────────────────────────┤
│                      LANGUAGE BINDINGS                                   │
│   [ TypeScript (napi-rs) ]  [ Python (PyO3) ]  [ Go (cgo) ]  [ Java ]  │
├──────────────────────────────────────────────────────────────────────────┤
│                      CHAINRPC LAYERS                                     │
│                                                                          │
│  ┌── Advanced Features ──────────────────────────────────────────────┐  │
│  │  Multi-Chain Router  │  Request Hedging  │  Auto-Batcher          │  │
│  │  MEV Protection      │  Gas Estimation   │  Tx Lifecycle          │  │
│  │  Archive Routing     │  Backpressure     │  CU Budget Tracking    │  │
│  │  Solana Support      │  Geo Routing      │  Gas Bumping           │  │
│  │  Reorg Detection     │                   │                        │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Provider Pool ──────────────────────────────────────────────────┐  │
│  │  Selection Strategies  │  Health Checker  │  Per-Provider Metrics  │  │
│  │  (RoundRobin, Priority, Latency, Weighted, Sticky)                │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Policy Engine ──────────────────────────────────────────────────┐  │
│  │  Rate Limiter (TokenBucket + CU-aware)                            │  │
│  │  Circuit Breaker (Closed → Open → Half-Open)                      │  │
│  │  Retry Policy (exponential backoff + jitter)                      │  │
│  │  Method Safety (Safe / Idempotent / Unsafe)                       │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Caching & Dedup ────────────────────────────────────────────────┐  │
│  │  Tiered Cache (Immutable 1h / SemiStable 5m / Volatile 2s)        │  │
│  │  Reorg Invalidation  │  Request Deduplication                     │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Transports ─────────────────────────────────────────────────────┐  │
│  │  HTTP (reqwest)        │  WebSocket (tokio-tungstenite)            │  │
│  │  Rate-limit headers    │  Auto-reconnect + resubscribe            │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Lifecycle ──────────────────────────────────────────────────────┐  │
│  │  Graceful Shutdown  │  Cancellation Tokens  │  Signal Handling     │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌── Observability ──────────────────────────────────────────────────┐  │
│  │  Per-Provider Metrics  │  Prometheus Export  │  Structured Tracing  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
├──────────────────────────────────────────────────────────────────────────┤
│                         CORE TYPES                                       │
│  RpcTransport trait  │  JsonRpcRequest/Response  │  TransportError       │
│  HealthStatus        │  RpcId / RpcParam                                 │
└──────────────────────────────────────────────────────────────────────────┘
```

## Crate Structure

```
chainrpc/
├── crates/
│   ├── chainrpc-core/         # Foundation: trait, types, policy, all 31 modules
│   │   └── src/
│   │       ├── transport.rs        # RpcTransport trait
│   │       ├── request.rs          # JsonRpcRequest, JsonRpcResponse
│   │       ├── error.rs            # TransportError enum
│   │       ├── pool.rs             # ProviderPool (multi-provider failover)
│   │       ├── policy/             # Rate limiter, circuit breaker, retry
│   │       ├── cache.rs            # Tiered response caching
│   │       ├── dedup.rs            # Request deduplication
│   │       ├── batch.rs            # Auto-batching engine
│   │       ├── metrics.rs          # Per-provider metrics + Prometheus
│   │       ├── method_safety.rs    # Safe/Idempotent/Unsafe classification
│   │       ├── cu_tracker.rs       # Compute unit budget tracking
│   │       ├── rate_limit_headers.rs # HTTP rate-limit header parsing
│   │       ├── selection.rs        # 5 provider selection strategies
│   │       ├── health_checker.rs   # Background health probing
│   │       ├── routing.rs          # Archive vs full node routing
│   │       ├── backpressure.rs     # Concurrency limiting
│   │       ├── multi_chain.rs      # Multi-chain router
│   │       ├── hedging.rs          # Request hedging (race primary + backup)
│   │       ├── gas.rs              # EIP-1559 gas estimation
│   │       ├── mev.rs              # MEV protection + relay routing
│   │       ├── tx.rs               # Tx lifecycle data structures
│   │       ├── tx_lifecycle.rs     # Tx lifecycle + RpcTransport integration
│   │       ├── pending_pool.rs     # Pending tx monitoring
│   │       ├── cancellation.rs     # Cooperative cancellation tokens
│   │       ├── shutdown.rs         # Graceful shutdown coordination
│   │       ├── solana.rs           # Solana RPC support (commitment, method safety, CU costs)
│   │       ├── geo_routing.rs      # Geographic routing (region proximity, fallback)
│   │       ├── gas_bumper.rs       # Tx gas bumping (EIP-1559 replacement)
│   │       └── reorg.rs            # Chain reorg detection (sliding hash window)
│   │
│   ├── chainrpc-http/         # HTTP JSON-RPC transport (reqwest)
│   │   └── src/
│   │       ├── client.rs           # HttpRpcClient with retry loop
│   │       ├── batch.rs            # Re-exports core BatchingTransport
│   │       └── lib.rs              # pool_from_urls() helper
│   │
│   ├── chainrpc-ws/           # WebSocket transport (tokio-tungstenite)
│   │   └── src/
│   │       ├── client.rs           # WsRpcClient with auto-reconnect
│   │       └── subscriptions.rs    # Subscription management
│   │
│   └── chainrpc-providers/    # Pre-configured provider profiles
│       └── src/
│           ├── alchemy.rs          # Alchemy URL builder + CU costs
│           ├── infura.rs           # Infura URL builder
│           └── public.rs           # Ankr + other public RPCs
│
├── bindings/
│   ├── node/                  # TypeScript binding (napi-rs)
│   ├── python/                # Python binding (PyO3 + maturin)
│   ├── go/                    # Go binding (cgo)
│   └── java/                  # Java binding (JNI)
│
└── cli/                       # CLI tool (test, call, bench, pool)
```

## Request Flow

```
Application
    │
    ▼
┌─────────────┐     ┌───────────────┐
│ Method       │────▶│ Is method     │──── Unsafe ──▶ No retry, no dedup, no cache
│ Safety Check │     │ Safe/Idempotent/Unsafe?       │
└─────────────┘     └───────────────┘
    │ Safe
    ▼
┌─────────────┐
│ Dedup Check  │──── Duplicate in-flight? ──▶ Share response
└─────────────┘
    │ New request
    ▼
┌─────────────┐
│ Cache Check  │──── Cached + valid? ──▶ Return cached
└─────────────┘
    │ Cache miss
    ▼
┌─────────────┐
│ Backpressure │──── Queue full? ──▶ TransportError::Overloaded
└─────────────┘
    │ Admitted
    ▼
┌─────────────┐
│ Rate Limiter │──── Tokens available? ──▶ Wait or reject
│ (CU-aware)   │
└─────────────┘
    │ Acquired
    ▼
┌─────────────┐
│ Circuit      │──── Open? ──▶ Try next provider / AllProvidersDown
│ Breaker      │
└─────────────┘
    │ Closed/Half-Open
    ▼
┌─────────────┐
│ Provider     │──── Select by strategy (RR, Priority, Latency, etc.)
│ Selection    │
└─────────────┘
    │
    ▼
┌─────────────┐
│ Transport    │──── HTTP POST / WebSocket send
│ (send)       │
└─────────────┘
    │
    ▼
┌─────────────┐
│ On Success   │──── Record metrics, close circuit, cache response
│ On Failure   │──── Record failure, open circuit if threshold hit
│ On Retryable │──── Exponential backoff + jitter → retry (if safe)
└─────────────┘
```

## Design Principles

1. **Composable** — Every module works independently. You can use just the cache, just the pool, or stack everything together.

2. **Safe by default** — Write methods (`eth_sendRawTransaction`) are never retried. Circuit breakers prevent hammering dead providers. Backpressure protects against overload.

3. **Observable** — Every provider gets atomic metrics (success/failure counts, latency histograms, rate-limit hits). Prometheus export built-in.

4. **Transport-agnostic** — The `RpcTransport` trait abstracts over HTTP, WebSocket, or any future transport. All middleware (cache, dedup, batch, backpressure) wraps `Arc<dyn RpcTransport>`.

5. **Chain-aware** — Multi-chain router, archive node routing, MEV detection, EIP-1559 gas helpers — all EVM-specific logic is modular and opt-in.

## Test Coverage

| Crate | Tests | Key Coverage |
|-------|-------|-------------|
| chainrpc-core | 250 | All 31 modules, policy engine, cache tiers, reorg invalidation, Solana, geo routing, gas bumping, reorg detection |
| chainrpc-providers | 6 | URL construction for Alchemy, Infura, Ankr |
| chainrpc-ws | 3 | Subscription register/dispatch/resubscribe |
| Doc-tests | 3 | API usage examples |
| **Total** | **250** | |
