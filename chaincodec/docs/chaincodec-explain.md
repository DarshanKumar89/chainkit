# ChainKit / ChainCodec — Complete Implementation Reference

> What is implemented, why it exists, the logic behind each decision, and real-world use cases.

---

## The Core Problem Being Solved

Every blockchain emits events as raw binary blobs. Without a decoder, they are meaningless bytes:

```
topics[0] = 0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef
topics[1] = 0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045
data      = 0x000000000000000000000000000000000000000000000000000000000000f4240
```

ChainCodec turns that into:

```
schema:  ERC20Transfer v1
chain:   ethereum
from:    0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
to:      0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
value:   1000000
```

Today every indexer, analytics tool, wallet, and dApp solves this themselves:
- Etherscan — solved only for their UI
- The Graph — solved only inside their subgraph runtime
- Dune Analytics — solved only in their SQL engine
- Every team building a custom indexer reimplements it from scratch

**ChainCodec is the standalone, embeddable, multi-chain library that solves this once**, and exposes it via Rust, TypeScript (npm), Python (pip), and WASM.

---

## Repository Layout

```
chainkit/
├── chaincodec/          # Universal ABI decoder — COMPLETE
│   ├── crates/
│   │   ├── chaincodec-core/          # Shared types, traits, error taxonomy
│   │   ├── chaincodec-evm/           # EVM ABI decoder (alloy-core)
│   │   ├── chaincodec-solana/        # Anchor/Borsh event decoder
│   │   ├── chaincodec-cosmos/        # CosmWasm/ABCI event decoder
│   │   ├── chaincodec-registry/      # CSDL parser + 3 storage backends
│   │   ├── chaincodec-stream/        # Real-time WebSocket streaming engine
│   │   ├── chaincodec-batch/         # Rayon parallel bulk decode engine
│   │   └── chaincodec-observability/ # OpenTelemetry metrics + structured logging
│   ├── cli/                          # chaincodec CLI (12 commands)
│   ├── schemas/                      # 24 bundled CSDL protocol schemas
│   ├── fixtures/evm/                 # 20 golden test fixtures
│   ├── bindings/
│   │   ├── node/    # TypeScript / Node.js (napi-rs)
│   │   ├── python/  # Python (PyO3/maturin)
│   │   └── wasm/    # Browser WASM (wasm-bindgen)
│   └── examples/                     # 4 runnable Rust examples
├── chainerrors/         # EVM error decoder — scaffolded
├── chainrpc/            # Resilient RPC client — scaffolded
└── chainindex/          # Reorg-safe indexer — scaffolded
```

---

## Crate-by-Crate Breakdown

### 1. `chaincodec-core` — The Foundation

Every other crate depends on this. It defines the cross-chain type system and the traits that bind the library together.

**Why it exists**: Without a canonical type system, every chain-specific decoder would produce incompatible outputs. A consumer of decoded events (indexer, dashboard, etc.) should never need to know whether the data came from EVM, Solana, or Cosmos.

**Key types:**

| Type | Description |
|------|-------------|
| `CanonicalType` | Cross-chain type enum: `Uint(u16)`, `Int(u16)`, `Bool`, `Address`, `Pubkey`, `Bech32Address`, `Bytes(u8)`, `Array`, `Tuple`, `Hash256`, `Timestamp`, `Decimal` |
| `NormalizedValue` | Decoded output, chain-independent: `Uint(u128)`, `BigUint(String)`, `Address(String)`, `Bool(bool)`, `Null`, etc. |
| `RawEvent` | Decoder input: chain, tx_hash, block_number, topics, data (raw bytes) |
| `DecodedEvent` | Decoder output: schema name, chain, block, address, `HashMap<String, NormalizedValue>` |
| `EventFingerprint` | Newtype around a hash string that identifies an event type (keccak256 for EVM, SHA-256 for Solana/Cosmos) |
| `Schema` + `FieldDef` + `SchemaMeta` | In-memory representation of a parsed schema definition |
| `ChainDecoder` trait | Implemented by `EvmDecoder`, `SolanaDecoder`, `CosmosDecoder` |
| `SchemaRegistry` trait | Implemented by `MemoryRegistry`, `SqliteRegistry` |
| `ErrorMode` | `Skip | Collect | Throw` — controls how batch decoding reacts to individual failures |

---

### 2. `chaincodec-evm` — The Primary Decoder

The workhorse. Decodes Ethereum and any EVM-compatible chain (Arbitrum, Base, Polygon, Optimism, Avalanche, BSC) using **alloy-core** — the modern successor to `ethabi`.

**Why alloy-core**: The legacy `ethabi` crate does not support dynamic ABI types cleanly and has no `DynSolType` for runtime-typed decoding. `alloy-core` provides `DynSolType::decode_single()` and `DynSolValue` for exactly this use case.

#### `decoder.rs` — `EvmDecoder`

The main `ChainDecoder` implementation. Given a `RawEvent` and a `Schema`:

- `topics[0]` — event fingerprint = `keccak256("Transfer(address,address,uint256)")`
- `topics[1..]` — indexed parameters, ABI-decoded per schema field type (one per indexed field)
- `data` — non-indexed parameters, ABI-decoded as a packed tuple in schema field order

Critical subtlety: indexed `address` and `bytesN` types are stored right-aligned in 32-byte topic slots, **not** ABI-encoded the same way as in `data`. The decoder handles both paths separately — a bug here would silently produce wrong addresses.

#### `call_decoder.rs` — `EvmCallDecoder`

Decodes raw transaction `input` data:
- First 4 bytes = function selector (`keccak256(signature)[:4]`)
- Remaining bytes = ABI-encoded arguments
- Constructor calldata: no selector, entire bytes = ABI-encoded constructor args
- Takes standard Ethereum ABI JSON as input (the same JSON exported by Hardhat/Foundry)

#### `encoder.rs` — `EvmEncoder`

The inverse of call decoding. Encodes `NormalizedValue` arguments into calldata bytes. Used for transaction simulation, test fixture generation, and building transactions programmatically.

#### `eip712.rs` — `Eip712Parser`

Parses `eth_signTypedData_v4` JSON payloads — the structured signing format used by MetaMask. Used to inspect Permit signatures, OpenSea Seaport orders, Uniswap permit2 authorizations, and any off-chain typed message.

#### `proxy.rs` — Proxy Detection

Detects whether a contract is a proxy and resolves the implementation address. Supports:

| Pattern | Detection Method |
|---------|-----------------|
| EIP-1967 Logic Proxy | Storage slot `keccak256("eip1967.proxy.implementation") - 1` |
| EIP-1967 Beacon Proxy | Storage slot `keccak256("eip1967.proxy.beacon") - 1` |
| EIP-1822 UUPS | `proxiableUUID()` storage slot |
| OpenZeppelin Transparent Proxy | Admin slot + logic slot (pre-EIP-1967) |
| EIP-1167 Minimal Proxy / Clone | Bytecode prefix match: `0x363d3d37...` |
| Gnosis Safe | `masterCopy()` call pattern |

**Why this matters**: ~60% of all deployed DeFi contracts are behind proxies. USDC, Aave pools, Compound markets — all proxies. If you try to decode a USDC Transfer event using only the proxy address's code (which contains no ABI), you get nothing. You need to resolve to the implementation first.

#### `normalizer.rs`

Converts `alloy-core`'s `DynSolValue` into ChainCodec's `NormalizedValue`. The translation layer between the raw ABI world and the canonical type system. Handles EIP-55 address checksumming, big integer string encoding for values >u128, and reference type (bytes32, address) extraction from topic slots.

#### `batch.rs`

`EvmDecoder` overrides the default `decode_batch()` to use **Rayon** parallel iterators. The batch is split across all available CPU cores. Target: >1M events/sec single-thread, >5M/sec on 8 cores.

---

### 3. `chaincodec-solana` — Anchor/Borsh Decoder

Decodes events from Solana programs built with **Anchor** — the dominant Solana smart contract framework used by Orca, Marinade, Phoenix, and most major protocols.

**Anchor event format:**
- Discriminator = first 8 bytes of `SHA-256("event:<EventName>")`
- Payload = remaining bytes, **Borsh-encoded** in schema field order

The `SolanaDecoder` uses the discriminator (stored in `topics[0]`) as the fingerprint to look up the schema, then decodes each field from the Borsh payload according to the schema's `CanonicalType` definitions.

**Why Borsh matters**: Unlike EVM ABI encoding (which is documented and has mature tooling), Borsh is a custom binary format specific to Solana. There is no equivalent of `alloy-core` — you must decode field-by-field, knowing the exact type and order from the schema. Without the schema, the bytes are uninterpretable.

---

### 4. `chaincodec-cosmos` — CosmWasm/ABCI Decoder

Decodes Cosmos chain events (Cosmos Hub, Osmosis, Neutron, Injective, dYdX, Sei, etc.).

**Cosmos event format** — completely different from EVM:
- Event `type`: string identifier (`"wasm"`, `"transfer"`, `"coin_received"`)
- Attributes: `[{"key": "fieldName", "value": "stringValue"}]` — always strings

The decoder:
1. Parses the JSON attribute list from `raw.data` (handles both array-of-objects and object formats)
2. Strips Cosmos denomination suffixes (e.g. `"1000000uatom"` → `"1000000"` for numeric parsing)
3. Maps string values to `NormalizedValue` per the schema's `CanonicalType`
4. Disambiguates CosmWasm events using `wasm/<action>` fingerprinting (so different actions on the same `wasm` event type get unique schemas)

**Why it's different**: There is no ABI encoding in Cosmos. All values are human-readable strings. The challenge is parsing those strings into typed values, and the denomination stripping is a universal pattern that every Cosmos data consumer must implement independently — until now.

---

### 5. `chaincodec-registry` — Schema Storage

Three backends, all implementing the same `SchemaRegistry` trait. You can swap backends without changing consumer code.

#### CSDL (ChainCodec Schema Definition Language)

A YAML-based DSL for defining schemas. Human-readable, version-controlled, shareable:

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum, arbitrum, base, polygon, optimism]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
```

`CsdlParser` converts CSDL YAML to `Schema` structs. Uses `IndexMap` (not `HashMap`) to preserve field insertion order — **critical** because ABI decoding is positional. Supports multi-document files (multiple schemas separated by `---`).

#### `MemoryRegistry`

Fast, thread-safe (`Arc<RwLock<Inner>>`), ephemeral. Indexed three ways:
1. By fingerprint — `O(1)` HashMap lookup for real-time event matching
2. By `(name, version)` pair — for explicit version-pinned lookups
3. By chain slug — for filtering all schemas applicable to a given chain

Tracks version history, deprecated status, and automatically resolves to the latest non-deprecated version when no version is specified.

#### `SqliteRegistry`

Persistent registry backed by SQLite (`rusqlite` with bundled feature — no system SQLite dependency). Schema stored as JSON blob. WAL mode enabled for concurrent reads. Supports `add()` (reject duplicates) and `upsert()` (replace on conflict). Useful for CLI tools, offline indexers, and embedded deployments where you want schema persistence across restarts.

#### `AbiFetcher` (remote feature)

Fetches ABIs from the internet with automatic fallback:
1. **Sourcify** first — decentralized, no API key, open-source
2. **Etherscan** fallback — requires API key, supports Arbiscan / Polygonscan / Basescan / Snowtrace forks
3. **4byte.directory** — selector-to-signature lookup for unknown calldata

---

### 6. `chaincodec-stream` — Real-Time Streaming

Connects to live blockchain WebSocket endpoints and continuously emits decoded events.

**Architecture:**
```
EvmWsListener (per chain, Tokio task)
      │  eth_subscribe("logs", { address: [...] })
      ▼
RawEvent channel
      │
      ▼
StreamEngine: fingerprint → schema → ChainDecoder::decode_event()
      │
      ▼
broadcast::Sender<DecodedEvent>   ← N consumers subscribe independently
```

`EvmWsListener` maintains a WebSocket connection to any Ethereum JSON-RPC node (`wss://`), handles `eth_subscribe("logs")`, parses incoming subscription messages, responds to server pings, and signals `StreamEngine` to reconnect on disconnect.

`StreamEngine` orchestrates multiple chain listeners simultaneously, routes raw events to the correct decoder by chain slug, filters by subscribed schema names if configured, and broadcasts `DecodedEvent` to all active subscribers. Tracks live metrics: events_decoded, events_skipped, decode_errors, reconnections.

**Reconnect strategy**: Exponential backoff with cap at 64 seconds. The consumer never sees the reconnection — they just keep receiving from the `broadcast::Receiver`.

---

### 7. `chaincodec-batch` — Bulk Historical Processing

For processing historical data — decoding millions of past events from archive nodes or databases.

`BatchEngine`:
- Accepts a `Vec<RawEvent>` of any size
- Splits into chunks (default: 10,000 events/chunk, configurable)
- Calls `EvmDecoder::decode_batch()` (Rayon-parallelized) per chunk
- Three error modes: `Skip` (best-effort analytics), `Collect` (gather all errors for inspection), `Throw` (fail fast on first error)
- Optional progress callback (`|decoded, total|`) for ETAs and progress bars
- Returns `BatchResult { events, errors, total_input }`

**Why chunking**: Processing 200M events in one Rayon call would require holding all decoded results in memory simultaneously. Chunking bounds peak memory usage while still achieving full CPU parallelism within each chunk.

---

### 8. `chaincodec-observability` — Production Monitoring

OpenTelemetry integration for production deployments.

**Metrics (Prometheus-compatible via OTLP):**

| Metric | Type | Tags |
|--------|------|------|
| `chaincodec.events_decoded` | Counter | chain, schema |
| `chaincodec.events_skipped` | Counter | chain, reason |
| `chaincodec.decode_errors` | Counter | chain, error_type |
| `chaincodec.decode_latency_ms` | Histogram | — |
| `chaincodec.batch_size` | Histogram | — |
| `chaincodec.schema_cache_hits` | Counter | — |

**Structured logging**: JSON-formatted, compatible with ELK, Grafana Loki, AWS CloudWatch. Log level configurable per component. OTLP export to any OpenTelemetry collector (Jaeger, Tempo, Datadog, etc.).

**Why it's a separate crate**: Not every user needs observability. Making it opt-in via a separate crate means the core decoder has zero observability overhead for embedded or CLI use cases.

---

### 9. CLI — The `chaincodec` Binary

12 commands covering the full workflow:

| Command | Purpose |
|---------|---------|
| `chaincodec parse --file schema.csdl` | Parse + validate CSDL, show field details |
| `chaincodec decode-log --topics ... --data 0x...` | Decode a raw EVM event log from the terminal |
| `chaincodec decode-call --calldata 0x... --abi abi.json` | Decode transaction calldata |
| `chaincodec encode-call --function transfer --args '[...]'` | Encode a function call to calldata |
| `chaincodec fetch-abi --address 0x... --chain-id 1` | Fetch ABI from Sourcify / Etherscan |
| `chaincodec detect-proxy --address 0x... --rpc wss://...` | Detect proxy pattern |
| `chaincodec verify --schema ERC20Transfer --tx 0x...` | Verify schema against a real on-chain transaction |
| `chaincodec test --fixtures ./fixtures` | Run the golden fixture test suite |
| `chaincodec bench --schema ERC20Transfer --iterations 1000000` | Throughput benchmark |
| `chaincodec schemas list` | List all schemas in a directory |
| `chaincodec schemas search --protocol uniswap` | Search schemas by protocol/category/event |
| `chaincodec schemas validate` | Validate all CSDL files in a directory |
| `chaincodec info` | Show capabilities, supported chains, and binding versions |

---

### 10. Bundled Schemas — 24 Protocols

Organized into five categories:

| Category | Protocols |
|----------|-----------|
| **tokens** | ERC-20, ERC-721, ERC-1155, ERC-4626, WETH |
| **defi** | Uniswap V2, Uniswap V3, Aave V3, Compound V2, Compound V3, Balancer V2, Curve, MakerDAO, Lido, Morpho, Pendle, GMX, EigenLayer |
| **nft** | OpenSea Seaport, Blur |
| **bridge** | Across Protocol, Stargate |
| **governance** | Compound Governor Bravo |

Each schema includes: event fingerprint, field types with indexed flags, protocol metadata, trust level, and chain applicability list.

---

### 11. Golden Test Fixtures — 20 Events

One JSON fixture per major protocol event. Each fixture contains exact `topics` + `data` hex matching real (or synthetically representative) on-chain transactions, plus `expectedFields` as ground truth for test assertions.

Protocols: ERC-20 Transfer, ERC-20 Approval, ERC-1155 TransferSingle, ERC-4626 Deposit, WETH Deposit/Withdrawal, Uniswap V2 (Swap/Mint/Burn/Sync), Uniswap V3 Swap, Aave V3 (Borrow/Repay/Supply), Chainlink (AnswerUpdated/NewRound/OCRTransmitted), Lido Submitted, Balancer V2 Swap, OpenSea OrderFulfilled.

Used by `cargo test` integration tests and `chaincodec test` CLI command.

---

### 12. Language Bindings

| Binding | Package | Framework |
|---------|---------|-----------|
| TypeScript / Node.js | `@chainfoundry/chaincodec` | napi-rs (zero-copy Rust ↔ JS) |
| Python | `chaincodec` | PyO3 / maturin |
| Browser WASM | `@chainfoundry/chaincodec-wasm` | wasm-bindgen |

All three expose: `EvmDecoder`, `EvmCallDecoder`, `EvmEncoder`, `MemoryRegistry`, `Eip712Parser`.

---

### 13. Examples

Four runnable Rust programs in `chaincodec/examples/src/bin/`:

| Binary | Demonstrates |
|--------|-------------|
| `decode_erc20` | Inline CSDL → MemoryRegistry → RawEvent → decode → print fields |
| `batch_decode` | BatchEngine with progress callback and Collect error mode |
| `stream_demo` | StreamEngine + EvmWsListener → real-time USDC events with Ctrl-C shutdown |
| `fetch_and_decode` | AbiFetcher (Sourcify + 4byte.directory) → decode with built-in schema |

---

## The Logic Behind Key Design Decisions

### Why a Schema-First Approach?

The alternative is "ABI-first" — pass a raw Ethereum ABI JSON and decode against it. This is what `ethabi` and `alloy-json-abi` do. The problem:
- ABI JSON is EVM-only
- ABI JSON has no metadata (protocol, category, trust level)
- ABI JSON has no version history
- ABI JSON cannot express cross-chain applicability
- ABI JSON is hard to read and edit manually

CSDL schemas are human-readable, version-controlled, shareable, and chain-agnostic. The schema is the contract between the on-chain code and the consumer.

### Why `IndexMap` for Fields?

EVM ABI decoding is positional — the `data` field is a packed tuple of all non-indexed parameters in the exact order they appear in the event signature. If field ordering is not preserved, the wrong type gets applied to the wrong bytes. `HashMap` does not guarantee insertion order. `IndexMap` does.

### Why Three Registry Backends?

- `MemoryRegistry` — for runtime use, testing, and serverless/embedded contexts where disk persistence is not needed
- `SqliteRegistry` — for CLI tools, offline indexers, and any deployment where schemas should survive restarts without a network call
- `AbiFetcher` (remote) — for ad-hoc decoding of any contract without pre-loading schemas

### Why Separate `chaincodec-stream` and `chaincodec-batch`?

They have opposite performance profiles:
- **Stream**: I/O-bound (WebSocket), low latency, one event at a time, Tokio async
- **Batch**: CPU-bound, maximum throughput, millions of events, Rayon parallel

Merging them into one crate would force every user to take both Tokio and Rayon as dependencies. Keeping them separate keeps dependency footprint minimal.

### Why `NormalizedValue` Instead of Returning Strings?

Returning everything as a string (as Cosmos natively does) is simple but loses type information. A consumer that needs to do arithmetic on a `uint256` transfer value should not have to parse a string. `NormalizedValue` is typed — `Uint(1000000)` is directly usable, not `"1000000"`.

Large integers (>u128) are stored as decimal strings in `BigUint(String)` / `BigInt(String)` because Rust has no native u256, but this is an explicit variant, not a silent downcast.

---

## Real-World Use Cases

### 1. DeFi Analytics Platform
Decode all Uniswap V3 Swap events from a block range to compute volume, fees, and price impact. Use `BatchEngine` with a Rayon thread pool. With ChainCodec: `engine.decode(request)` → structured `Vec<DecodedEvent>`. Without it: write your own ABI decoder, re-implement for each protocol, maintain it as ABIs change.

### 2. Production Real-Time Indexer
Use `StreamEngine` + `EvmWsListener` to subscribe to USDC Transfer events. Write decoded events to Postgres as they arrive. Reconnection, filtering, and decoding are handled — consumer code is just a `while let Ok(event) = rx.recv()` loop.

### 3. Transaction Debugger
`chaincodec decode-call --calldata 0x... --abi usdc.json` shows exactly what a failed transaction was trying to call. `chaincodec detect-proxy --address 0x... --rpc wss://...` reveals whether you need to fetch the implementation's ABI instead.

### 4. Protocol TypeScript SDK
A DeFi protocol ships a TypeScript SDK. They use `@chainfoundry/chaincodec` to decode their own contract events, so consumers don't need to handle raw ABI bytes. The decode logic is in Rust (fast, correct), exposed via napi-rs (zero-copy), published as npm.

### 5. Multi-Chain Data Pipeline
A cross-chain analytics platform covers Ethereum, Arbitrum, Solana, and Osmosis:
- `EvmDecoder` for EVM chains
- `SolanaDecoder` for Orca/Jupiter events (Anchor/Borsh)
- `CosmosDecoder` for Osmosis swaps (ABCI/CosmWasm)

All three produce the same `NormalizedValue` output type. The pipeline downstream never branches on chain family.

### 6. Security Monitoring
A security monitoring tool subscribes to all Aave V3 Borrow/Repay events in real-time. It decodes them, checks the `amount` field against expected thresholds, and triggers alerts on anomalous withdrawals. Schema-based matching means it ignores all other events without deserializing them.

### 7. Smart Contract Test Suite
Use `EvmEncoder` to build calldata for test transactions, then `EvmDecoder` to decode the emitted events and assert field values. A complete encode → call → decode round-trip in Rust tests.

### 8. Schema Registry Server
The `SqliteRegistry` + a thin HTTP wrapper becomes a hosted schema registry for a team. Multiple indexers share the same schema definitions without duplicating CSDL files across repositories.

---

## Performance Targets

| Operation | Target | Method |
|-----------|--------|--------|
| Single-thread event decode | >1M events/sec | Rayon on `decode_batch()` |
| 8-thread batch decode | >5M events/sec | Rayon thread pool |
| Schema fingerprint lookup | O(1) | HashMap keyed by fingerprint hex |
| CSDL parse (per file) | <1ms | serde_yaml + IndexMap |
| WebSocket event latency | <5ms end-to-end | Tokio async, zero-copy channel |

---

## The Broader ChainKit Vision

ChainCodec is the first of four modules. The full stack:

```
chaincodec  ──  decode what happened
chainerrors ──  decode why it failed
chainrpc    ──  talk to nodes reliably
chainindex  ──  track it all over time
```

Each module is an independent Cargo workspace — usable standalone or composed together. A complete blockchain data infrastructure stack in Rust, with first-class TypeScript and Python support.
