# ChainKit

> **Building blockchain primitives for Rust, TypeScript, Python, Go, Java, and WASM.**

ChainKit is a monorepo of four foundational Rust libraries for building blockchain data infrastructure. Each module is an independent Cargo workspace with language bindings for TypeScript, Python, Go, and Java — use one, use all.

---

## Modules

| Module | Description | Status | Docs | Examples |
|--------|-------------|--------|------|----------|
| [`chaincodec`](./chaincodec/) | Universal ABI decoder — EVM events, calls, EIP-712, proxy detection, 50+ schemas | ✅ v0.1.2 | [docs](./chaincodec/docs/) | [examples](./chaincodec/examples/) |
| [`chainerrors`](./chainerrors/) | EVM revert / panic / custom error decoder with golden fixtures | ✅ Complete | [docs](./chainerrors/docs/) | — |
| [`chainrpc`](./chainrpc/) | Production RPC transport — circuit breaker, rate limiter, caching, pool, MEV, 27 modules | ✅ Complete | [docs](./chainrpc/docs/) | [22 examples](./chainrpc/examples/) |
| [`chainindex`](./chainindex/) | Reorg-safe blockchain indexer with pluggable storage (SQLite/Postgres) | ✅ Complete | — | — |

### Language Bindings

All 4 modules ship with native bindings:

| Language | chaincodec | chainerrors | chainrpc | chainindex |
|----------|-----------|-------------|----------|------------|
| TypeScript (napi-rs) | ✅ | ✅ | ✅ | ✅ |
| Python (PyO3/maturin) | ✅ | ✅ | ✅ | ✅ |
| Go (cgo) | ✅ | ✅ | ✅ | ✅ |
| Java (JNI) | ✅ | ✅ | ✅ | ✅ |
| WASM (wasm-bindgen) | ✅ | — | — | — |

---

## ChainRPC — Production RPC Transport

**27 modules**, **188 tests**, composable middleware stack for EVM RPC.

```
DedupTransport → CacheTransport → BackpressureTransport → ProviderPool → HttpRpcClient
```

Every layer wraps `Arc<dyn RpcTransport>` and itself implements the trait. Stack what you need.

### Highlights

- **Provider Pool** — Multi-provider failover with 5 selection strategies (RoundRobin, Priority, Weighted, LatencyBased, Sticky)
- **Tiered Cache** — 4-tier response cache (Immutable 1h / SemiStable 5m / Volatile 2s / NeverCache) with reorg invalidation
- **CU-Aware Rate Limiting** — Token bucket that knows `eth_getLogs` = 75 CU, `eth_blockNumber` = 10 CU
- **Circuit Breaker** — Three-state (Closed/Open/HalfOpen), automatic failover when providers go down
- **Request Dedup** — N concurrent identical calls = 1 HTTP request, shared response
- **Auto-Batching** — Collects individual calls into JSON-RPC batch requests within a time window
- **MEV Protection** — Detects 12 MEV-susceptible selectors, routes to private relays
- **Gas Estimation** — EIP-1559 recommendations (Slow/Standard/Fast/Urgent)
- **Tx Lifecycle** — Send, track, poll receipt, detect stuck, nonce management
- **Prometheus Metrics** — Per-provider success/failure/latency export

### Quick Start

```rust
use chainrpc_http::{HttpRpcClient, pool_from_urls};
use chainrpc_core::transport::RpcTransport;

// Single client with built-in retry + circuit breaker
let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/KEY");
let block: String = client.call(1, "eth_blockNumber", vec![]).await?;

// Multi-provider failover
let pool = pool_from_urls(&["https://alchemy.com/...", "https://infura.io/..."])?;
let resp = pool.send(JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;
```

### ChainRPC Documentation

| Document | Description |
|----------|-------------|
| [README](./chainrpc/README.md) | Full feature list, quick start, crate structure |
| [Architecture](./chainrpc/docs/ARCHITECTURE.md) | System diagram, request flow, design principles |
| [Modules](./chainrpc/docs/MODULES.md) | All 27 modules with public API signatures |
| [API Reference](./chainrpc/docs/API-REFERENCE.md) | Quick-lookup tables for every type and function |
| [Use Cases](./chainrpc/docs/USECASES.md) | 16 real-world patterns with complete Rust code |
| [Implementation](./chainrpc/docs/IMPLEMENTATION.md) | Design decisions, concurrency, internals |
| [Examples](./chainrpc/examples/README.md) | 22 runnable examples organized by category |

---

## ChainCodec — Universal ABI Decoder

**50+ protocol schemas**, **13 examples**, decode/encode any EVM event, call, or EIP-712 message.

```
EVM log    → EvmDecoder     → DecodedEvent { fields: { from, to, value }, ... }
Calldata   → EvmCallDecoder → DecodedCall { function_name, inputs: [...] }
ABI + args → EvmEncoder     → 0xaabbccdd...
```

### Features

| Feature | Status |
|---------|--------|
| EVM event log decoding | ✅ |
| Function call decoding | ✅ |
| Constructor decoding | ✅ |
| ABI encoding (bidirectional) | ✅ |
| EIP-712 typed data | ✅ |
| Proxy detection (EIP-1967, EIP-1822, EIP-1167) | ✅ |
| Auto ABI fetch (Sourcify + Etherscan) | ✅ |
| CSDL schema format (YAML) | ✅ |
| 50+ bundled protocol schemas | ✅ |
| Parallel batch decode (Rayon) | ✅ |

### Bundled Schemas

**Tokens**: ERC-20, ERC-721, ERC-1155, ERC-4626, WETH
**DEX**: Uniswap V2, Uniswap V3, Curve, Balancer V2, Pendle
**Lending**: Aave V3, Compound V2, Compound V3, Morpho Blue, MakerDAO
**Staking/Restaking**: Lido, EigenLayer
**Perpetuals**: GMX V1
**Oracles**: Chainlink Price Feeds, Chainlink OCR2
**NFT Marketplaces**: OpenSea Seaport, Blur
**Bridges**: Across Protocol, Stargate
**Governance**: Compound Governor Bravo

### Quick Start

```rust
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;

let registry = MemoryRegistry::new();
registry.load_directory("./schemas")?;

let decoder = EvmDecoder::new();
let event = decoder.decode_event(&raw_log, &schema)?;
println!("{}: {:?}", event.schema, event.fields);
```

```bash
npm install @chainfoundry/chaincodec   # TypeScript
pip install chaincodec                  # Python
```

### ChainCodec Documentation

| Document | Description |
|----------|-------------|
| [Getting Started](./chaincodec/docs/getting-started.md) | Install, first decode, quickstarts in Rust / TypeScript / Python / CLI |
| [Examples Walkthrough](./chaincodec/docs/examples.md) | All 13 runnable examples explained with expected output |
| [CSDL Reference](./chaincodec/docs/csdl-reference.md) | Complete schema format — types, fingerprints, versioning |
| [Architecture](./chaincodec/docs/architecture.md) | Every crate explained — design decisions and internals |
| [Use Cases](./chaincodec/docs/use-cases.md) | What to build — indexers, analytics, wallets, security, trading |

---

## ChainErrors — EVM Error Decoder

Decodes Solidity reverts, panics, and custom errors from raw bytes.

```
0x08c379a0... → Error(string): "Insufficient balance"
0x4e487b71... → Panic(uint256): arithmetic overflow (0x11)
0xe450d38c... → ERC20InsufficientBalance(address, uint256, uint256)
```

- Revert string decoding (`Error(string)`)
- Panic code decoding with human-readable descriptions
- Custom error decoding (ERC-20/721, OpenZeppelin, Ownable)
- Golden fixture test suite

---

## ChainIndex — Reorg-Safe Indexer

Pluggable blockchain indexer with automatic reorg detection and recovery.

- **IndexerBuilder** API for configuring chains, handlers, storage
- **ReorgDetector** — 4-scenario detection (simple, deep, to-genesis, chain-switch)
- **BlockTracker** — Sliding window with gap detection
- **Storage backends** — Memory, SQLite (WAL mode), PostgreSQL (JSONB events)
- **Checkpoint system** — Resume from last confirmed block

---

## Architecture

```
chainkit/
├── chaincodec/          # ABI decoder — v0.1.2 published
│   ├── crates/          # 8 crates (core, evm, registry, batch, stream, ...)
│   ├── schemas/         # 50+ bundled CSDL schemas
│   ├── bindings/        # node, python, wasm, go, java
│   └── cli/
├── chainerrors/         # Error decoder
│   ├── crates/          # core, evm
│   └── bindings/        # node, python, go, java
├── chainrpc/            # RPC transport — 27 modules, 188 tests
│   ├── crates/          # core (27 modules), http, ws, providers
│   ├── bindings/        # node, python, go, java
│   ├── examples/        # 22 runnable examples
│   ├── docs/            # 5 documentation files
│   └── cli/
└── chainindex/          # Blockchain indexer
    ├── crates/          # core, evm, storage
    └── bindings/        # node, python, go, java
```

---

## Performance

| Module | Metric |
|--------|--------|
| chaincodec | >1M events/sec single-thread, >5M with Rayon |
| chainrpc | ~1ns middleware overhead per layer (vs 5-100ms network) |
| chainindex | Reorg detection in O(window_size) |

---

## CLI Tools

```bash
# chaincodec — decode events, calls, encode, proxy detection
cargo install chaincodec-cli
chaincodec decode-log --topics 0xddf252ad... --data 0x000...

# chainrpc — test RPC calls, benchmark providers
cargo install chainrpc-cli
chainrpc call --url https://... --method eth_blockNumber

# chainerrors — decode error data
cargo install chainerrors-cli
chainerrors decode --data 0x08c379a0...
```

---

## Built With

The CI/CD pipeline, publishing workflow, build system, and module testing for ChainKit were developed with the assistance of [Claude](https://claude.ai) (Anthropic) — including crates.io publishing, npm/PyPI release automation, cross-platform Rust builds, and language binding generation. Anything wrong open for suggestions and improvement.

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md). Each module has independent CI:

```bash
cd chaincodec && cargo test --workspace
cd chainerrors && cargo test --workspace
cd chainrpc && cargo test --workspace
cd chainindex && cargo test --workspace
```

---

## License

MIT — see [LICENSE](./LICENSE)

---

## Contact

Built by [@darshan_aqua](https://x.com/darshan_aqua) — questions, feedback, and contributions welcome.

---

## Roadmap

- **v0.1** (done): chaincodec production release — Rust + npm + Python + WASM
- **v0.2**: chainerrors + chainrpc publish to crates.io / npm / PyPI
- **v0.3**: chainindex publish with SQLite/Postgres backends
- **v1.0**: Full multi-chain support (Solana, Cosmos), E2E integration tests
