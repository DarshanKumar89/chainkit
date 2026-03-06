# ChainCodec — Current Development Status

> Last updated: 2026-03-01 | Version: v0.1.2

---

## Vision & Purpose

`chaincodec` is a **universal blockchain ABI decoder** — the foundational layer that sits between raw on-chain data (topics, calldata, Borsh bytes, Cosmos attributes) and your application. Its job is to take opaque binary data from any chain and return structured, human-readable values so you never write another ABI decoder by hand.

**Core philosophy**: Write once in Rust, use everywhere — native speed in Node.js, browser-safe via WASM, ergonomic in Python.

```
Raw EVM log  ──→  EvmDecoder  ──→  DecodedEvent { fields: { from, to, value } }
Calldata     ──→  EvmCallDecoder ──→  DecodedCall { functionName, inputs: [...] }
ABI + args   ──→  EvmEncoder  ──→  0xa9059cbb...
Borsh bytes  ──→  SolanaDecoder ──→  DecodedEvent (same NormalizedValue shape)
ABCI attrs   ──→  CosmosDecoder ──→  DecodedEvent (same NormalizedValue shape)
```

---

## What's Fully Working (Production-Ready)

### Rust Crates

| Crate | What It Does | Status |
| --- | --- | --- |
| `chaincodec-core` | Traits, types (`NormalizedValue`, `ChainId`, `Schema`), errors | ✅ Complete |
| `chaincodec-evm` | EVM event decode, call decode, ABI encode, EIP-712, proxy detection | ✅ Complete |
| `chaincodec-registry` | CSDL parser, in-memory registry, SQLite registry, remote ABI fetch | ✅ Complete |
| `chaincodec-solana` | Anchor/Borsh IDL decoding, discriminator fingerprinting | ✅ Complete |
| `chaincodec-cosmos` | CosmWasm/ABCI JSON attribute decoding | ✅ Complete |
| `chaincodec-batch` | Rayon parallel batch decode with chunking + error modes | ✅ Complete |
| `chaincodec-stream` | Real-time WebSocket streaming with reconnect + broadcast channels | ✅ Complete |
| `chaincodec-observability` | OpenTelemetry metrics (6 counters/histograms), structured logging | ✅ Complete |

### Language Bindings

| Binding | Package | APIs Exposed | Status |
| --- | --- | --- | --- |
| Node.js (napi-rs) | `@chainfoundry/chaincodec` | `EvmDecoder`, `EvmCallDecoder`, `EvmEncoder`, `MemoryRegistry`, `Eip712Parser` | ✅ Published + tested |
| WASM (browser) | `@chainfoundry/chaincodec-wasm` | Same, JSON-in/JSON-out style | ✅ Published |
| Python (PyO3) | `chaincodec` (PyPI) | Same API, Python dicts | ✅ Published |
| Go (CGo) | `chaincodec-go` | C header + Go wrapper | ⚠️ Code exists, not published |
| Java (JNI) | — | JNI bridge | ⚠️ Code exists, not published |

### CLI (`chaincodec-cli`)

12 commands: `parse`, `decode-log`, `decode-call`, `encode-call`, `encode-constructor`, `fetch-abi`, `detect-proxy`, `verify`, `test`, `bench`, `info`, `schemas`

```bash
cargo install chaincodec-cli

chaincodec decode-log --topics 0xddf252ad... 0x000...from 0x000...to \
  --data 0x000...value --schema-dir ./schemas --chain ethereum

chaincodec decode-call --calldata 0xa9059cbb... --abi ./abis/erc20.json

chaincodec fetch-abi --address 0xA0b86991... --chain-id 1
```

### Schemas & Data

- **53 CSDL schemas** covering: ERC-20/721/1155, Uniswap V2/V3, Aave V2/V3, Compound, Curve, Balancer, Lido, MakerDAO, Chainlink, Wormhole, LayerZero, OpenSea, BAYC, ENS, GMX, dYdX, and more
- **13 example binaries** (batch, streaming, proxy detection, EIP-712, fetch-and-decode, etc.)
- **20 golden test fixtures** for EVM, Solana, Cosmos

### Chain Support

**EVM chains** — full decoder + schema coverage:

| Chain | Chain ID | Notes |
| --- | --- | --- |
| Ethereum | 1 | Full schema coverage |
| Arbitrum One | 42161 | Full schema coverage |
| Base | 8453 | Full schema coverage |
| Polygon | 137 | Full schema coverage |
| Optimism | 10 | Full schema coverage |
| Avalanche | 43114 | Works via `ChainId::evm()`, no shorthand |
| BSC (BNB Chain) | 56 | Works via `ChainId::evm()`, no shorthand |
| Any EVM chain | any | Pass numeric chain ID — ABI encoding is chain-agnostic |

**Non-EVM** — Rust decoder only, no bindings:

| Chain | Status |
| --- | --- |
| Solana | Rust decoder complete, no npm/PyPI package |
| Cosmos / CosmWasm | Rust decoder complete, no npm/PyPI package |
| Sui | Type defined in core, zero decoder code |
| Aptos | Type defined in core, zero decoder code |

---

## What's Partial / Has Known Gaps

| Area | Gap | Impact |
| --- | --- | --- |
| **Proxy detection** (`proxy.rs`) | Detection logic defined but `storage_to_address()` requires a live RPC call to resolve. No built-in HTTP client in the crate — CLI must wire it externally. | Medium — CLI works, library needs RPC wired by caller |
| **Remote ABI fetch** (`remote.rs`) | Sourcify + Etherscan + 4byte clients implemented but **feature-gated** (`remote` feature flag). Not enabled by default, not included in published binary. | Medium — opt-in feature, works when enabled |
| **EIP-712 domain separator** | Hashes raw JSON of `domain` field instead of proper EIP-712 ABI-encoded typed hashing. Works for fingerprinting/comparison, not spec-compliant for wallet signature verification. | Low for indexing, High for wallet use cases |
| **Solana + Cosmos bindings** | No Node.js / WASM / Python bindings for `chaincodec-solana` or `chaincodec-cosmos`. Only the core Rust decoders exist. | High — JS/Python devs cannot decode Solana or Cosmos yet |
| **Go + Java bindings** | Code files exist but no CI/CD publishing pipeline, no tests, no README. | High — not usable without docs/CI |
| **Benchmarks** | `chaincodec-batch` has no `benches/throughput.rs` — the `>1M events/sec` performance claim in docs is unverified by a real `cargo bench`. | Low for correctness, Medium for trust |
| **E2E integration tests** | No tests against a live node (Anvil). All tests use mocked/fixture data. | Medium — correctness not verified end-to-end |
| **Fuzz testing** | No `cargo fuzz` targets for the ABI decoder or CSDL parser. | Medium — security risk for untrusted input |
| **chaincodec-stream WebSocket** | `EvmWsListener` connects via WebSocket, but `eth_subscribe` subscription message not verified against a real node in CI. | Medium — stream works, but not CI-tested |

---

## What's Missing Entirely (Future Phases)

### Phase 2 — `chainerrors` (Weeks 9–11)

Decode EVM revert data. Currently if a transaction reverts, you get raw bytes back. `chainerrors` would decode:

- `0x08c379a0` → `require("message")` revert strings
- `0x4e487b71` → panic codes (overflow, OOB, division by zero)
- Custom Solidity 0.8.4+ errors via ABI + 4byte.directory lookup

```rust
// Future API
let err = EvmErrorDecoder::decode(revert_data)?;
// ErrorKind::Panic { code: 0x11, meaning: "arithmetic overflow" }
// ErrorKind::RevertString("ERC20: transfer amount exceeds balance")
// ErrorKind::CustomError { name: "InsufficientBalance", inputs: [...] }
```

### Phase 3 — `chainrpc` (Weeks 12–18)

A resilient RPC transport layer with:

- Circuit breaker (3-state: closed/open/half-open)
- Token bucket rate limiter per provider
- Exponential backoff retry
- Auto-batching (`eth_getBalance`, `eth_call` combined in one request)
- Provider profiles (Alchemy, Infura, QuickNode, public RPCs)

```rust
// Future API
let provider = ProviderBuilder::new()
    .url("https://eth-mainnet.g.alchemy.com/v2/{key}")
    .rate_limit(300)  // CU/sec
    .circuit_breaker(CircuitBreakerConfig::default())
    .retry(3)
    .build()?;
```

### Phase 4 — `chainindex` (Weeks 19–26)

A reorg-safe blockchain indexer with:

- Backfill + live polling loop
- 4-scenario reorg detection
- SQLite + Postgres checkpoint storage
- Fluent builder API (`IndexerBuilder`)
- Handler registry per event type

```rust
// Future API
let indexer = IndexerBuilder::new()
    .chain(chains::ethereum())
    .from_block(17_000_000)
    .on_event("ERC20Transfer", handle_transfer)
    .storage(SqliteStorage::new("./index.db"))
    .build().await?;

indexer.run().await?;
```

### Production Ops Gaps (All Modules)

| Gap | Status |
| --- | --- |
| Graceful shutdown (cancellation tokens) | Missing |
| `GET /health` endpoint | Missing |
| `GET /metrics` (Prometheus text format) | Missing |
| Unified TOML config file per module | Missing |
| Docker images | Missing |
| E2E tests with Anvil (local EVM node) | Missing |
| Fuzz testing for ABI decoder | Missing |
| Spec-compliant EIP-712 domain separator | Partial |
| Go + Java bindings CI/CD + publishing | Missing |
| Solana + Cosmos language bindings | Missing |
| Benchmark suite (`cargo bench`) | Missing |
| More EVM chain shortcuts (zkSync, Linea, Scroll) | Missing |

---

## What Developers Can Use Right Now

```bash
# JavaScript / TypeScript
npm install @chainfoundry/chaincodec        # Node.js — works, tested, 17/17 smoke tests pass
npm install @chainfoundry/chaincodec-wasm   # Browser / Deno / Cloudflare Workers

# Python
pip install chaincodec                      # Python — works

# Rust
```

```toml
[dependencies]
chaincodec-evm          = "0.1"   # Decode EVM logs + calldata + encode
chaincodec-registry     = "0.1"   # Load CSDL schemas (YAML)
chaincodec-core         = "0.1"   # Traits, NormalizedValue types
chaincodec-solana       = "0.1"   # Decode Anchor/Borsh events
chaincodec-cosmos       = "0.1"   # Decode CosmWasm/ABCI events
chaincodec-batch        = "0.1"   # Rayon parallel batch decode
chaincodec-stream       = "0.1"   # Real-time WebSocket streaming
chaincodec-observability = "0.1"  # OpenTelemetry metrics + tracing
```

The **EVM decode pipeline** (`registry → fingerprint → decode → normalized fields`) is the most production-ready part and works reliably for indexers, analytics dashboards, and dApp frontends today. The Solana and Cosmos decoders are implemented in Rust but have not been tested against live chains.

---

## Publish Status

| Package | Registry | Version | Status |
| --- | --- | --- | --- |
| `chaincodec-core` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-evm` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-registry` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-batch` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-stream` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-observability` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-solana` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-cosmos` | crates.io | 0.1.2 | ✅ Published |
| `chaincodec-cli` | crates.io | 0.1.2 | ✅ Published |
| `@chainfoundry/chaincodec` | npm | 0.1.2 | ✅ Published |
| `@chainfoundry/chaincodec-wasm` | npm | 0.1.2 | ✅ Published |
| `chaincodec` | PyPI | 0.1.2 | ✅ Published |

---

## Roadmap Summary

| Version | Focus | Target |
| --- | --- | --- |
| **v0.1** (current) | chaincodec production release — Rust + npm + Python + WASM | Done |
| **v0.2** | chainerrors — EVM revert/panic/custom error decoder | Weeks 9–11 |
| **v0.3** | chainrpc — resilient RPC transport with provider integrations | Weeks 12–18 |
| **v0.4** | chainindex — reorg-safe indexer with SQLite/Postgres | Weeks 19–26 |
| **v1.0** | Full multi-chain support (Solana, Cosmos bindings), production ops, fuzz testing | Week 44 |
