# ChainCodec Development Roadmap

> **Chain-Agnostic Event & ABI Decoder with Universal Schema Registry**
>
> The universal open-source primitive for reading any blockchain.

---

## How to Use This Roadmap with Claude Code

This file is designed as the **single source of truth** for developing ChainCodec using Claude Code in VSCode. Each task includes context, acceptance criteria, and the exact Claude Code prompt you can run.

**Workflow:**
1. Open this file in VSCode alongside your terminal
2. Copy the Claude Code prompt for your current task
3. Run it with `claude` in your terminal
4. Verify the acceptance criteria before marking complete
5. Check off the task `[x]` and commit

**Convention:** All prompts assume you're in the `chaincodec/` monorepo root.

---

## Repository Structure (Target)

```
chaincodec/
├── crates/
│   ├── chaincodec-core/          # Decoder trait + type normalizer + error types
│   ├── chaincodec-evm/           # EVM ABI decoder (alloy-rs)
│   ├── chaincodec-solana/        # Anchor IDL decoder
│   ├── chaincodec-cosmos/        # CosmWasm schema decoder
│   ├── chaincodec-registry/      # Local schema registry + CSDL parser
│   ├── chaincodec-stream/        # Streaming engine (Tokio)
│   ├── chaincodec-batch/         # Batch decode engine (Rayon)
│   └── chaincodec-observability/ # OpenTelemetry integration
├── bindings/
│   ├── node/                     # napi-rs TypeScript binding
│   ├── wasm/                     # wasm-bindgen browser binding
│   ├── python/                   # PyO3 Python binding
│   ├── java/                     # JNI Java binding
│   └── go/                       # cgo Go binding
├── registry-server/              # axum HTTP server for public/private registry
│   ├── Dockerfile
│   ├── docker-compose.yml
│   └── migrations/
├── schemas/                      # Built-in schema library (CSDL files)
│   ├── defi/
│   └── tokens/
├── fixtures/                     # Golden test fixtures
│   ├── evm/
│   ├── solana/
│   └── cosmos/
├── plugins/                      # Example WASM plugins
├── cli/                          # chaincodec CLI tool
├── examples/
├── docs/
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE                       # MIT
├── ROADMAP.md                    # This file
└── .github/workflows/
```

---

## Phase 0: Project Scaffolding (Week 1)

### 0.1 — Initialize Rust Workspace Monorepo

- [ ] **Create the Cargo workspace with all crate stubs**

```
claude "Initialize a Rust workspace monorepo for ChainCodec. Create a root Cargo.toml
with workspace members for these crates: chaincodec-core, chaincodec-evm,
chaincodec-registry, chaincodec-stream, chaincodec-batch, chaincodec-observability.
Each crate should have a minimal lib.rs with a doc comment explaining its purpose.
Set edition = 2021, rust-version = 1.75. Add workspace-level dependencies for:
serde (1.x, features derive), serde_json, thiserror, tokio (features full),
tracing, alloy-primitives, alloy-sol-types. Create a .rustfmt.toml with
max_width = 100 and a clippy.toml. Add a root .gitignore for Rust projects."
```

**Acceptance criteria:**
- `cargo check --workspace` passes
- All 6 crates exist with lib.rs files
- Workspace dependencies are shared correctly

### 0.2 — Core Type Definitions

- [ ] **Define all shared types in chaincodec-core**

```
claude "In crates/chaincodec-core/src/lib.rs, define the foundational types for
ChainCodec:

1. ChainFamily enum: Evm, Solana, Cosmos, Sui, Aptos, Custom(String)
2. RawEvent struct: chain (ChainFamily), tx_hash (Vec<u8>), log_index (u32),
   block_number (u64), topics (Vec<Vec<u8>>), data (Vec<u8>), timestamp (Option<u64>)
3. EventFingerprint struct: wrapping [u8; 32]
4. DecodedField enum: Address(String), Uint256(String), Int256(String),
   Bytes(Vec<u8>), String(String), Bool(bool), Timestamp(u64), Array(Vec<DecodedField>)
5. DecodedEvent struct: schema_name (String), chain (ChainFamily), tx_hash (Vec<u8>),
   block_number (u64), log_index (u32), fields (IndexMap<String, DecodedField>),
   fingerprint (EventFingerprint), decode_status (DecodeStatus), timestamp (Option<u64>)
6. DecodeStatus enum: Success, PartialDecode(String), UnknownSchema, Failed(String)
7. DecodeError enum using thiserror: SchemaNotFound, AbiMismatch, InvalidData(String),
   RegistryUnavailable, ChainNotSupported(String)
8. BatchDecodeError struct with: errors (Vec<(usize, DecodeError)>), decoded_count (usize)
9. ProgressCallback trait: fn on_progress(&self, decoded: usize, total: usize)

All types should derive Debug, Clone, Serialize, Deserialize where appropriate.
Use indexmap::IndexMap for field ordering. Add indexmap to workspace deps."
```

**Acceptance criteria:**
- `cargo check -p chaincodec-core` passes
- All types are public and documented
- Serde round-trip test passes for DecodedEvent

### 0.3 — ChainDecoder Trait

- [ ] **Define the core decoder trait**

```
claude "In crates/chaincodec-core/src/decoder.rs, define the ChainDecoder trait:

pub trait ChainDecoder: Send + Sync {
    fn chain_id(&self) -> ChainFamily;
    fn decode_event(&self, raw: &RawEvent, schema: &Schema) -> Result<DecodedEvent, DecodeError>;
    fn decode_batch(
        &self,
        logs: &[RawEvent],
        schemas: &dyn SchemaLookup,
        progress: Option<&dyn ProgressCallback>,
    ) -> Result<Vec<DecodedEvent>, BatchDecodeError>;
    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint;
    fn supports_abi_guess(&self) -> bool;
}

Also define the SchemaLookup trait:
pub trait SchemaLookup: Send + Sync {
    fn lookup_by_fingerprint(&self, fp: &EventFingerprint) -> Option<&Schema>;
    fn lookup_by_name(&self, name: &str) -> Option<&Schema>;
}

And the Schema struct with fields: name, version, chains, event_name, fingerprint,
fields (Vec<SchemaField>), meta (SchemaMeta). SchemaField has: name, field_type
(CanonicalType enum), indexed, nullable, description. SchemaMeta has: protocol,
category, verified, trust_level (TrustLevel enum: Unverified, CommunityVerified,
MaintainerVerified, ProtocolVerified), supersedes, superseded_by, deprecated.

Export everything from lib.rs. Run cargo check."
```

**Acceptance criteria:**
- Trait compiles with all method signatures
- Schema struct supports all CSDL fields from the architecture doc
- `cargo test -p chaincodec-core` passes

### 0.4 — Project Config Files

- [ ] **Create CI, license, and community files**

```
claude "Create the following project files for the ChainCodec monorepo:

1. LICENSE - MIT license, copyright 2026 AI2Innovate / Darsh
2. CONTRIBUTING.md - Standard OSS contribution guide with sections:
   getting started, building from source (cargo build --workspace),
   running tests (cargo test --workspace), submitting schemas,
   adding chain decoders, code style (rustfmt + clippy), PR process
3. CHANGELOG.md - Initial entry for v0.1.0-alpha with 'Initial scaffolding'
4. .github/workflows/ci.yml - GitHub Actions workflow that:
   - Triggers on push/PR to main
   - Matrix: ubuntu-latest, macos-latest, windows-latest
   - Steps: checkout, install rust stable, cargo fmt --check,
     cargo clippy --workspace -- -D warnings, cargo test --workspace
   - Cache: uses Swatinem/rust-cache@v2
5. .github/workflows/bench.yml - Benchmark workflow on main push only,
   runs cargo criterion (placeholder, actual benchmarks added later)"
```

**Acceptance criteria:**
- CI workflow is valid YAML
- CONTRIBUTING.md has clear build instructions
- LICENSE is valid MIT

---

## Phase 1: EVM Decoder (Weeks 1–3)

### 1.1 — CSDL Parser

- [ ] **Implement CSDL YAML parser in chaincodec-registry**

```
claude "In crates/chaincodec-registry/, implement a CSDL (ChainCodec Schema Definition
Language) parser. CSDL is YAML-based. Add serde_yaml to workspace deps.

Create src/csdl.rs with:
1. A CsdlFile struct that maps to this YAML format:
   schema: SchemaName (PascalCase)
   version: integer
   description: optional string
   chains: [chain-id, ...]
   address: hex string or null
   event: string (event name)
   fingerprint: hex string (auto-computed if missing)
   supersedes: optional SchemaName
   superseded_by: optional SchemaName
   deprecated: optional bool (default false)
   deprecated_at: optional string
   fields: map of fieldName -> { type, indexed, nullable, description }
   meta: { protocol, category, verified, trust_level, provenance_sig }

2. A parse_csdl(yaml_str: &str) -> Result<Schema, CsdlParseError> function
   that converts CsdlFile into the Schema type from chaincodec-core
3. A parse_csdl_file(path: &Path) -> Result<Schema, CsdlParseError> function
4. A validate_schema(schema: &Schema) -> Vec<ValidationWarning> function that checks:
   - Name is PascalCase
   - At least one chain specified
   - At least one field defined
   - Fingerprint matches computed value (if provided)

Add unit tests parsing this example:
schema UniswapV3Swap:
  version: 2
  chains: [ethereum, arbitrum, polygon, base, optimism]
  event: Swap
  fields:
    sender: { type: address, indexed: true }
    recipient: { type: address, indexed: true }
    amount0: { type: int256, indexed: false }
    amount1: { type: int256, indexed: false }
    sqrtPriceX96: { type: uint160, indexed: false }
    liquidity: { type: uint128, indexed: false }
    tick: { type: int24, indexed: false }
  meta:
    protocol: uniswap-v3
    category: dex
    verified: true"
```

**Acceptance criteria:**
- Parses the example CSDL correctly
- Validates PascalCase naming
- Round-trips: parse → serialize → parse produces identical Schema
- `cargo test -p chaincodec-registry` passes with 5+ tests

### 1.2 — Local Schema Registry

- [ ] **Implement in-memory and SQLite-backed local registry**

```
claude "In crates/chaincodec-registry/src/registry.rs, implement the local schema
registry with two backends:

1. MemoryRegistry - HashMap-based, implements SchemaLookup trait from core
   - add_schema(schema: Schema) -> Result<(), RegistryError>
   - lookup_by_fingerprint(fp: &EventFingerprint) -> Option<&Schema>
   - lookup_by_name(name: &str) -> Option<&Schema>
   - list_schemas() -> Vec<&Schema>
   - load_directory(path: &Path) -> Result<usize, RegistryError> (loads all .csdl files)

2. SqliteRegistry - backed by rusqlite (add to workspace deps)
   - Same interface as MemoryRegistry
   - Schema table: id, name, version, fingerprint (BLOB, indexed), chains (JSON),
     event_name, fields (JSON), meta (JSON), created_at, deprecated
   - Lookup by fingerprint should be O(1) via index
   - Support for schema versioning: multiple versions of same name,
     latest() returns highest version

Both should implement SchemaLookup trait.

Add a RegistryBuilder that lets you chain:
  RegistryBuilder::new()
    .with_directory('./schemas/')
    .with_sqlite('./cache.db')
    .build()

Tests:
- Add schema, lookup by fingerprint, lookup by name
- Load directory of .csdl files
- SQLite persistence: add, close, reopen, verify data survives
- Version lookup: add v1 and v2, latest() returns v2"
```

**Acceptance criteria:**
- Both backends pass identical test suites
- SQLite schema survives process restart
- Fingerprint lookup is indexed
- `cargo test -p chaincodec-registry` passes with 10+ tests

### 1.3 — EVM ABI Decoder

- [ ] **Implement EVM event decoder using alloy-rs**

```
claude "In crates/chaincodec-evm/src/lib.rs, implement the EVM ABI decoder.

Add workspace deps: alloy-primitives, alloy-sol-types, alloy-json-abi, hex.

Create EvmDecoder struct implementing ChainDecoder trait:

1. chain_id() returns ChainFamily::Evm
2. fingerprint(raw) extracts topics[0] as the event signature hash
3. decode_event(raw, schema):
   - Parse the schema fields to determine ABI types
   - Split raw data: indexed params come from topics[1..], non-indexed from data
   - Decode each field using alloy's ABI decoding
   - Map EVM types to ChainCodec canonical types:
     address -> DecodedField::Address(checksummed string)
     uint256/uint128/etc -> DecodedField::Uint256(decimal string)
     int256/int24/etc -> DecodedField::Int256(decimal string)
     bytes/bytes32 -> DecodedField::Bytes(vec)
     string -> DecodedField::String
     bool -> DecodedField::Bool
   - Return DecodedEvent with all fields populated
4. decode_batch: iterate logs, collect results, respect error handling mode
5. supports_abi_guess() returns true

Also implement a compute_fingerprint(event_signature: &str) -> EventFingerprint
function that computes keccak256 of the event signature string
(e.g., 'Transfer(address,address,uint256)').

Write tests for:
- ERC20 Transfer(address indexed from, address indexed to, uint256 value)
  Use real mainnet data: known tx hash with expected decoded values
- ERC721 Transfer
- Uniswap V3 Swap (7 fields, mix of indexed/non-indexed)
- Approval event
- Edge case: empty data field
- Edge case: more topics than expected"
```

**Acceptance criteria:**
- Decodes ERC20 Transfer correctly with real mainnet test data
- Decodes Uniswap V3 Swap with all 7 fields
- Fingerprint computation matches known keccak256 values
- `cargo test -p chaincodec-evm` passes with 8+ tests

### 1.4 — Golden Test Fixtures

- [ ] **Create golden test fixture system**

```
claude "Create a golden test fixture system for ChainCodec.

1. In fixtures/evm/, create JSON fixture files for these protocols:
   - erc20-transfer.json: 3 known Ethereum mainnet Transfer events with
     tx_hash, log_index, raw topics, raw data, and expected decoded output
   - erc20-approval.json: 2 Approval events
   - uniswap-v3-swap.json: 2 Swap events with all 7 fields
   - aave-v3-supply.json: 2 Supply events
   - erc721-transfer.json: 2 NFT Transfer events

   Fixture format:
   {
     'chain': 'ethereum',
     'txHash': '0x...',
     'logIndex': 2,
     'rawTopics': ['0x...', '0x...'],
     'rawData': '0x...',
     'expectedSchema': 'ERC20Transfer',
     'expectedFields': {
       'from': '0x68b3465833...',
       'to': '0x7a250d5630...',
       'value': '42000000000000000000'
     }
   }

2. In crates/chaincodec-evm/tests/golden.rs, create an integration test that:
   - Loads all fixture files from fixtures/evm/
   - For each fixture, constructs a RawEvent from the raw data
   - Loads the appropriate schema from schemas/
   - Runs EvmDecoder.decode_event()
   - Asserts every expectedField matches the decoded output exactly
   - Reports which fixtures pass/fail with clear error messages

3. Create corresponding .csdl schema files in schemas/tokens/ and schemas/defi/
   for each protocol used in fixtures.

NOTE: For the raw data in fixtures, use realistic hex-encoded values that
would actually appear on-chain. The topics and data must be valid ABI-encoded
values that our decoder can parse. If you don't have real mainnet data, construct
synthetic but valid ABI-encoded test data."
```

**Acceptance criteria:**
- At least 10 golden test fixtures exist
- All fixtures have matching .csdl schemas
- `cargo test -p chaincodec-evm -- golden` passes 100%
- Fixture format is documented

### 1.5 — CLI Scaffold + Verify Command

- [ ] **Create the chaincodec CLI with verify command**

```
claude "Create a CLI tool in cli/ directory as a Rust binary crate. Add clap (features
derive) to workspace deps.

Implement these subcommands:

1. chaincodec verify --schema <name> --fixture <path>
   - Loads schema from schemas/ directory
   - Loads fixture from the given path
   - Runs the decoder and compares output
   - Prints: ✓ Schema matched, ✓ All N fields decoded correctly, ✓ Fingerprint verified
   - Exit code 0 on success, 1 on failure

2. chaincodec test --fixtures <dir>
   - Runs all fixtures in a directory
   - Prints summary: X/Y passed, lists failures

3. chaincodec parse --schema <path.csdl>
   - Parses a CSDL file and prints the parsed Schema as JSON
   - Useful for validating CSDL syntax

4. chaincodec bench --schema <name> --iterations <N> (placeholder for now)

Add the cli crate to workspace members. Use colored output (add colored crate)."
```

**Acceptance criteria:**
- `cargo run -p chaincodec-cli -- verify --schema ERC20Transfer --fixture fixtures/evm/erc20-transfer.json` passes
- `cargo run -p chaincodec-cli -- test --fixtures fixtures/evm/` runs all fixtures
- `cargo run -p chaincodec-cli -- parse --schema schemas/tokens/erc20.csdl` outputs valid JSON

---

## Phase 2: Registry + Batch Engine (Weeks 3–5)

### 2.1 — Schema Fingerprint Auto-Discovery

- [ ] **Implement fingerprint-based schema auto-resolution**

```
claude "In chaincodec-registry, add auto-discovery capability:

1. Add a method to MemoryRegistry and SqliteRegistry:
   auto_resolve(raw: &RawEvent) -> Option<&Schema>
   - Extracts fingerprint from the raw event (topics[0] for EVM)
   - Looks up in the registry
   - Returns the schema if found

2. Add a SchemaResolver struct that chains multiple lookup strategies:
   SchemaResolver::new()
     .with_local(local_registry)
     .with_fallback(|fp| { /* HTTP fetch from remote registry */ })
     .build()

3. Implement the HTTP client for remote registry lookups:
   GET /schemas/fingerprint/{hex_fingerprint}
   Returns JSON schema or 404
   Cache results locally after first fetch

Add tests for:
- Auto-resolve finds correct schema for known fingerprint
- Auto-resolve returns None for unknown fingerprint
- Resolver tries local first, then remote
- Cache prevents duplicate HTTP calls"
```

**Acceptance criteria:**
- Auto-resolve works for all golden test fixture fingerprints
- Resolver chain works local → remote with caching
- Tests pass

### 2.2 — Batch Decode Engine

- [ ] **Implement parallel batch decoder with Rayon**

```
claude "In crates/chaincodec-batch/, implement the batch decode engine.

Add rayon to workspace deps.

Create BatchDecoder struct:
1. new(decoder: Arc<dyn ChainDecoder>, registry: Arc<dyn SchemaLookup>) -> Self
2. decode(
     logs: &[RawEvent],
     config: BatchConfig,
   ) -> BatchResult

BatchConfig:
- concurrency: usize (default: num_cpus)
- chunk_size: usize (default: 10_000)
- error_mode: ErrorMode (Skip, Collect, Throw)
- progress: Option<Arc<dyn ProgressCallback>>

BatchResult:
- events: Vec<DecodedEvent>
- errors: Vec<(usize, DecodeError)> (only populated in Collect mode)
- stats: BatchStats { total: usize, decoded: usize, failed: usize, duration: Duration }

Implementation:
- Split logs into chunks of chunk_size
- Use rayon::par_iter to process chunks in parallel
- Each chunk calls decoder.decode_event() for each log
- Merge results preserving original order
- Call progress callback after each chunk completes
- Handle error modes: Skip drops failures, Collect keeps them, Throw returns on first error

Write benchmarks using criterion:
- Decode 100k synthetic ERC20 Transfer events
- Compare single-threaded vs parallel (expect 3-6x speedup on 8 cores)
- Memory usage should stay bounded regardless of input size"
```

**Acceptance criteria:**
- Batch decode of 100k events completes in < 2 seconds on modern hardware
- Parallel is measurably faster than sequential
- All three error modes work correctly
- Progress callback fires at expected intervals
- `cargo test -p chaincodec-batch` passes
- `cargo bench -p chaincodec-batch` produces benchmark results

### 2.3 — Schema Versioning & Evolution

- [ ] **Implement schema version management in registry**

```
claude "In chaincodec-registry, add schema versioning and evolution support:

1. Version tracking:
   - Registry stores multiple versions of the same schema name
   - latest(name) returns highest version number
   - get_version(name, version) returns specific version
   - history(name) returns all versions ordered

2. Evolution chain:
   - When adding a schema with 'supersedes' field, validate the referenced schema exists
   - When adding a schema that supersedes another, auto-set 'superseded_by' on the old one
   - evolution_chain(name) returns the full chain: V1 -> V2 -> V3...

3. Deprecation:
   - deprecate(name, version) marks a schema as deprecated
   - is_deprecated(name) -> bool
   - When resolving a deprecated schema, return it but include a warning

4. Lock file support:
   - generate_lockfile(schemas: &[&str]) -> LockFile
   - LockFile struct: map of schema_name -> { version, fingerprint, sha256 }
   - validate_lockfile(lockfile: &LockFile) -> Vec<LockfileViolation>
   - Serializes to/from chaincodec.lock.yaml

Tests:
- Add v1, add v2, latest() returns v2
- Add v2 with supersedes v1, verify chain
- Deprecate v1, resolve returns warning
- Generate and validate lockfile"
```

**Acceptance criteria:**
- Full version history traversal works
- Evolution chain auto-links supersedes/superseded_by
- Lockfile generation and validation works
- Tests pass

---

## Phase 3: TypeScript SDK + Streaming (Weeks 5–8)

### 3.1 — napi-rs TypeScript Binding

- [ ] **Create Node.js binding with napi-rs**

```
claude "In bindings/node/, set up a napi-rs project for the TypeScript SDK.

1. Initialize with: napi new --path bindings/node
   Package name: @chaincodec/sdk
   Configure for: linux-x64-gnu, linux-arm64-gnu, darwin-x64, darwin-arm64, win32-x64

2. Expose these functions to JS:
   - class ChainCodec:
     constructor(config: ChainCodecConfig)
     decode(params: DecodeParams): Promise<DecodedEvent>
     decodeBatch(params: BatchDecodeParams): Promise<BatchResult>
     stream(params: StreamParams): AsyncIterableIterator<DecodedEvent>
   - class SchemaRegistry:
     add(schema: Schema): void
     loadDirectory(path: string): number
     lookupByFingerprint(fp: string): Schema | null
     lookupByName(name: string): Schema | null
   - function parseSchema(yamlStr: string): Schema
   - function computeFingerprint(eventSignature: string): string

3. TypeScript type definitions in index.d.ts:
   interface ChainCodecConfig {
     chains: Record<string, { rpc: string }>;
     registry?: string | 'local';
     cache?: boolean;
     errorMode?: 'lenient' | 'strict';
     trustPolicy?: TrustPolicy;
   }
   // ... full type definitions for all public types

4. Create a basic test in __tests__/decode.test.ts:
   - Create ChainCodec instance
   - Load ERC20 schema
   - Decode a known event
   - Assert fields match expected values"
```

**Acceptance criteria:**
- `npm run build` produces native bindings
- TypeScript types are complete and accurate
- Basic decode test passes
- Package.json has correct metadata for @chaincodec/sdk

### 3.2 — Streaming Engine

- [ ] **Implement Tokio-based streaming engine**

```
claude "In crates/chaincodec-stream/, implement the real-time streaming engine.

Add tokio-tungstenite and futures-util to workspace deps.

1. BlockListener trait:
   #[async_trait]
   pub trait BlockListener: Send + Sync {
     async fn connect(&mut self, rpc_url: &str) -> Result<(), StreamError>;
     async fn next_block(&mut self) -> Option<Block>;
     async fn subscribe_logs(&mut self, filter: LogFilter) -> Result<LogStream, StreamError>;
   }

2. EvmBlockListener: implements BlockListener for EVM chains via WebSocket
   - Connects to eth_subscribe('logs', filter)
   - Parses incoming JSON-RPC notifications into RawEvent structs
   - Handles reconnection with exponential backoff (3 retries, 100ms/500ms/2s)

3. StreamEngine struct:
   - listeners: HashMap<ChainId, Box<dyn BlockListener>>
   - registry: Arc<dyn SchemaLookup>
   - decoder_registry: HashMap<ChainFamily, Arc<dyn ChainDecoder>>
   - metrics: StreamMetrics (decode_count, error_count, lag_blocks)

   Methods:
   - add_chain(chain_id, rpc_url, chain_family)
   - stream(config: StreamConfig) -> impl Stream<Item = DecodedEvent>
     StreamConfig: schemas (optional filter), chains (optional filter), from_block

   Internal:
   - Each chain listener runs in its own tokio::spawn task
   - Logs are sent through a tokio::sync::broadcast channel
   - Decoder picks up logs, resolves schema, decodes, emits to output stream
   - Fan-in pattern: multiple producers (chain listeners) -> single consumer stream

4. StreamMetrics: atomic counters for decode_count, error_count, events_per_sec

Tests (with mock listener):
- Create mock BlockListener that emits synthetic blocks
- Verify stream emits decoded events
- Verify reconnection logic
- Verify multi-chain fan-in (2 mock chains -> 1 stream)"
```

**Acceptance criteria:**
- Mock stream test passes
- Reconnection logic handles 3 failures then succeeds
- Multi-chain fan-in produces correctly tagged events
- Metrics counters increment correctly

### 3.3 — WASM Browser Build

- [ ] **Create wasm-bindgen browser binding**

```
claude "In bindings/wasm/, set up a wasm-bindgen project for browser-side decoding.

1. Cargo.toml for the wasm crate:
   - crate-type: ['cdylib']
   - Dependencies: wasm-bindgen, serde-wasm-bindgen, chaincodec-core, chaincodec-evm,
     chaincodec-registry (with no SQLite feature - memory only)
   - Profile release: opt-level = 's', lto = true

2. Expose to JS via wasm-bindgen:
   - async fn init() -> ChainCodecWasm (one-time WASM init)
   - fn decode(chain: &str, topics: Vec<String>, data: &str) -> JsValue (DecodedEvent)
   - fn parse_schema(yaml: &str) -> JsValue (Schema)
   - fn compute_fingerprint(sig: &str) -> String
   - fn add_schema(yaml: &str) -> Result<(), JsError>

3. Build script in package.json:
   - wasm-pack build --target web --out-dir pkg
   - Post-build: check bundle size with wc -c pkg/chaincodec_wasm_bg.wasm

4. Create a minimal HTML test page in bindings/wasm/test.html:
   - Loads the WASM module
   - Decodes a hardcoded ERC20 Transfer event
   - Displays decoded fields in the page

Target: < 200KB gzipped for full EVM decoder"
```

**Acceptance criteria:**
- `wasm-pack build` succeeds
- WASM bundle < 200KB gzipped
- test.html works in Chrome/Firefox
- Decode produces same results as native

### 3.4 — Error Handling Framework

- [ ] **Implement configurable error handling across all components**

```
claude "Implement the error handling and fallback system across ChainCodec.

1. In chaincodec-core, add ErrorConfig:
   pub struct ErrorConfig {
     pub error_mode: ErrorMode,           // Lenient or Strict
     pub on_unknown_schema: UnknownSchemaAction, // Passthrough, Skip, Error
     pub retry_policy: RetryPolicy,       // max_retries, backoff_ms
     pub on_decode_error: Option<Arc<dyn Fn(&DecodeError, &RawEvent) + Send + Sync>>,
   }

2. ErrorMode::Lenient behavior:
   - Unknown schema: emit RawEvent with DecodeStatus::UnknownSchema
   - ABI mismatch: attempt best-effort decode, tag as PartialDecode
   - Registry unreachable: use cached schemas, emit warning log

3. ErrorMode::Strict behavior:
   - Unknown schema: return DecodeError::SchemaNotFound
   - ABI mismatch: return DecodeError::AbiMismatch
   - Registry unreachable: return DecodeError::RegistryUnavailable

4. RetryPolicy implementation:
   - Used by streaming engine and remote registry client
   - Exponential backoff with jitter
   - Default: 3 retries, [100ms, 500ms, 2000ms]

5. Update EvmDecoder, BatchDecoder, and StreamEngine to respect ErrorConfig

Tests:
- Lenient mode: unknown schema produces passthrough event
- Strict mode: unknown schema produces error
- Retry: mock HTTP client fails twice then succeeds
- Callback: verify on_decode_error is called with correct args"
```

**Acceptance criteria:**
- Both error modes produce documented behavior
- Retry policy works with exponential backoff
- Error callback fires correctly
- All existing tests still pass

---

## Phase 4: Python SDK + Public Launch (Weeks 8–11)

### 4.1 — PyO3 Python Binding

- [ ] **Create Python SDK with PyO3**

```
claude "In bindings/python/, set up a PyO3 project for the Python SDK.

1. Use maturin for build tooling. Create pyproject.toml:
   - Package name: chaincodec
   - Requires-python >= 3.9
   - maturin backend

2. Expose to Python:
   - class ChainCodec:
     def __init__(self, chains, registry='local', cache=True)
     def decode(self, chain, tx_hash=None, log_index=None, topics=None, data=None) -> dict
     def decode_batch(self, chain, logs, concurrency=8, chunk_size=10000,
                      error_mode='skip', on_progress=None) -> BatchResult
     async def stream(self, schemas=None, chains=None) -> AsyncGenerator
     def decode_batch_to_dataframe(self, chain, logs, output='polars') -> DataFrame
   - class SchemaRegistry:
     def add(self, schema_yaml: str)
     def load_directory(self, path: str) -> int
     def lookup(self, fingerprint=None, name=None) -> Optional[dict]
   - def parse_schema(yaml_str: str) -> dict
   - def compute_fingerprint(event_signature: str) -> str

3. DataFrame integration:
   - decode_batch_to_dataframe returns polars.DataFrame or pandas.DataFrame
   - Column names from schema field names
   - Types mapped: Address->Utf8, Uint256->Utf8, Int256->Utf8, Bool->Boolean, etc.

4. Tests in tests/test_decode.py:
   - Basic decode of ERC20 Transfer
   - Batch decode of 100 synthetic events
   - DataFrame export produces correct column names and types
   - Schema loading from .csdl file

5. Jupyter notebook example in examples/python/quickstart.ipynb"
```

**Acceptance criteria:**
- `maturin develop` builds successfully
- `pytest tests/` passes all tests
- DataFrame export works with both polars and pandas
- Jupyter notebook runs end-to-end

### 4.2 — OpenTelemetry Observability

- [ ] **Add built-in metrics, logging, and tracing**

```
claude "In crates/chaincodec-observability/, implement the observability layer.

Add workspace deps: opentelemetry (0.22+), opentelemetry-otlp, tracing-opentelemetry,
tracing-subscriber, metrics.

1. Metrics (all prefixed with 'chaincodec.'):
   - decode.count (counter, labels: chain, schema, status)
   - decode.errors (counter, labels: chain, error_type)
   - decode.latency_us (histogram, labels: chain)
   - schema.cache_hits / cache_misses (counter)
   - stream.lag_blocks (gauge, labels: chain)
   - stream.events_per_sec (gauge)
   - registry.fetch_latency_ms (histogram)
   - batch.progress_pct (gauge)

2. Structured logging:
   - ObservabilityConfig { level, component_levels, format (json/pretty) }
   - Initialize tracing-subscriber with per-component filtering
   - All decode operations emit structured span events

3. Distributed tracing:
   - Inject/extract W3C Trace Context headers
   - Each decode_event creates a span with: chain, schema, fingerprint, tx_hash

4. Integration: add observe() method or macro that wraps any ChainDecoder
   to automatically emit metrics and traces

Tests:
- Verify metrics counters increment after decode
- Verify structured log output contains expected fields
- Verify spans are created with correct attributes"
```

**Acceptance criteria:**
- Metrics increment correctly
- JSON log output is parseable
- Spans contain chain/schema/tx_hash attributes
- Zero overhead when observability is disabled (feature flag)

### 4.3 — Documentation Site

- [ ] **Create docs site with quickstart guides**

```
claude "In docs/, create a documentation site using mdBook (Rust documentation tool).
Add mdbook as a dev dependency.

Structure:
docs/
├── book.toml
└── src/
    ├── SUMMARY.md
    ├── introduction.md
    ├── quickstart/
    │   ├── installation.md     # npm/pip/cargo install instructions
    │   ├── five-minute-guide.md # Decode your first event in 5 minutes
    │   └── concepts.md         # Schema, fingerprint, registry explained
    ├── guides/
    │   ├── typescript.md       # Full TS SDK guide with examples
    │   ├── python.md           # Full Python SDK guide with DataFrame examples
    │   ├── java.md             # Java SDK guide
    │   ├── browser.md          # WASM browser usage
    │   ├── batch-decode.md     # Historical data processing guide
    │   ├── streaming.md        # Real-time event streaming guide
    │   └── schemas.md          # Writing and submitting CSDL schemas
    ├── reference/
    │   ├── api.md              # Full API reference
    │   ├── csdl-spec.md        # CSDL specification
    │   ├── type-system.md      # Cross-chain type normalization table
    │   └── cli.md              # CLI commands reference
    ├── architecture/
    │   ├── overview.md         # System architecture diagram
    │   ├── security.md         # Trust model and verification
    │   └── plugins.md          # Writing chain decoder plugins
    └── changelog.md

The five-minute-guide.md should show:
1. npm install @chaincodec/sdk (3 lines)
2. Decode an ERC20 Transfer (5 lines)
3. Stream Uniswap swaps (8 lines)

This must be the fastest possible path from zero to decoded event."
```

**Acceptance criteria:**
- `mdbook build docs/` produces a static site
- Five-minute guide is actually completable in 5 minutes
- All code examples are tested/testable
- No broken internal links

### 4.4 — Public GitHub Launch

- [ ] **Prepare repository for public launch**

```
claude "Prepare the ChainCodec repository for public GitHub launch:

1. README.md - the most important file. Structure:
   - One-liner: what ChainCodec is
   - 3-line code example showing decode in TypeScript
   - Badges: CI status, npm version, crates.io version, PyPI version, license
   - 'Why ChainCodec?' section (3 bullet points, not a wall of text)
   - Quick install for all languages (npm/pip/cargo)
   - Feature table: chains supported, languages, batch/stream, registry
   - 'Supported Protocols' showing the pre-loaded schema count
   - Links to docs, contributing, changelog
   - 'Built by AI2Innovate' footer with link

2. Pre-load schemas/ with 50 DeFi protocol schemas:
   - Top 20 EVM DeFi protocols (Uniswap V2/V3, Aave V3, Compound V3,
     Curve, Balancer, Maker, Lido, Rocket Pool, etc.)
   - ERC20, ERC721, ERC1155 token standards
   - Major bridge contracts (Across, Stargate)
   - Governance events (Governor Bravo, Timelock)

3. examples/ directory:
   - typescript/decode-transfer.ts
   - typescript/stream-uniswap.ts
   - python/batch-analysis.py
   - python/jupyter-quickstart.ipynb

4. GitHub repo settings checklist (as a comment in .github/SETUP.md):
   - Description, topics (blockchain, web3, abi, decoder, rust)
   - Social preview image dimensions
   - Branch protection: require CI pass on main
   - Issue templates: bug report, feature request, schema submission"
```

**Acceptance criteria:**
- README renders beautifully on GitHub
- Code examples in README actually work
- 50 schemas exist in schemas/ directory
- All example files run without errors

---

## Phase 5: Solana + Enterprise (Weeks 11–16)

### 5.1 — Solana Anchor IDL Decoder

- [ ] **Implement Solana program event decoder**

```
claude "Create crates/chaincodec-solana/ implementing the ChainDecoder trait for Solana.

1. Parse Anchor IDL JSON format to extract event definitions
2. Decode Solana program logs (base64-encoded event data after discriminator)
3. Map Solana types to ChainCodec canonical types:
   - Pubkey -> Address (base58 string)
   - u64/u128 -> Uint256
   - i64/i128 -> Int256
   - Vec<u8> -> Bytes
   - String -> String
   - bool -> Bool
4. Fingerprint: SHA-256 of event discriminator (first 8 bytes)
5. Support for SPL Token events (Transfer, Approve, MintTo)

Add anchor-lang-idl (or manual IDL parsing with serde) to deps.

Golden test fixtures for Solana in fixtures/solana/:
- SPL Token Transfer
- Raydium Swap
- Jupiter aggregator route event

CSDL schemas for Solana use 'solana' as chain identifier."
```

**Acceptance criteria:**
- Decodes SPL Token Transfer correctly
- Anchor IDL parsing works for real program IDLs
- Golden tests pass for all Solana fixtures
- Cross-chain decode works: same ChainCodec instance handles both EVM and Solana

### 5.2 — Registry Server (Docker)

- [ ] **Build the hosted/private registry HTTP server**

```
claude "In registry-server/, build the axum-based registry HTTP server.

API endpoints:
  POST   /schemas                    - Submit a new schema (JSON body)
  GET    /schemas/:name              - Get latest version of a schema
  GET    /schemas/:name/versions     - List all versions
  GET    /schemas/:name/history      - Full evolution chain
  GET    /schemas/fingerprint/:hex   - Lookup by fingerprint
  GET    /schemas?chain=X&category=Y - Search/filter schemas
  POST   /schemas/:name/verify      - Submit verification (requires auth)
  POST   /schemas/:name/flag        - Flag schema as incorrect
  GET    /health                     - Health check

Storage: PostgreSQL via sqlx (add to deps).
Migrations in registry-server/migrations/.

Schema submission flow:
1. Accept CSDL or JSON schema
2. Recompute fingerprint, verify it matches
3. Run golden test validation if fixtures provided
4. Store with trust_level: 'unverified'
5. Return schema ID and registry URL

Docker setup:
- Dockerfile: multi-stage Rust build -> alpine runtime
- docker-compose.yml: registry-server + postgres
- One-command start: docker-compose up

Add rate limiting, CORS, and basic API key auth."
```

**Acceptance criteria:**
- `docker-compose up` starts the server
- All API endpoints work via curl
- Schema submission validates fingerprint
- PostgreSQL stores and retrieves schemas correctly
- Health check returns 200

### 5.3 — Offline Bundles & Schema Pinning

- [ ] **Implement air-gapped deployment support**

```
claude "Add offline operation support to ChainCodec:

1. CLI command: chaincodec bundle create
   - Takes: list of schema names or 'all'
   - Outputs: single binary file containing all schemas (MessagePack serialized)
   - Include metadata: bundle version, creation date, schema count, checksums

2. CLI command: chaincodec bundle inspect
   - Shows: schema count, names, versions, total size

3. Bundle loading in registry:
   RegistryBuilder::new()
     .with_bundle('schemas.bundle')  // loads all schemas from bundle
     .offline(true)                   // disables all network calls
     .build()

4. Schema pinning:
   - CLI command: chaincodec lock generate
     Reads current schemas, writes chaincodec.lock.yaml with version + fingerprint + sha256
   - CLI command: chaincodec lock verify
     Validates current schemas match lockfile exactly
   - Registry mode: when lockfile present, refuse schemas that don't match pins

5. TypeScript/Python integration:
   - ChainCodec({ registry: 'file://schemas.bundle', offline: true })
   - Throws error if any network call attempted in offline mode"
```

**Acceptance criteria:**
- Bundle create/inspect works via CLI
- Bundle loading produces identical decode results to directory loading
- Lockfile generation and verification works
- Offline mode blocks all network calls
- Bundle file < 1MB for 50 schemas

---

## Phase 6: Ecosystem Maturity (Months 4–12)

### 6.1 — Java SDK (JNI)
- [ ] Implement JNI binding in bindings/java/
- [ ] Maven/Gradle publish to Maven Central
- [ ] Reactive Streams compatible stream() method
- [ ] Integration tests with JUnit 5

### 6.2 — Cosmos/CosmWasm Decoder
- [ ] Create crates/chaincodec-cosmos/
- [ ] Parse CosmWasm contract schemas
- [ ] Map Cosmos types to canonical types
- [ ] Golden test fixtures for Osmosis, Injective

### 6.3 — SUI/Aptos Move Decoders
- [ ] Create crates/chaincodec-sui/ and chaincodec-aptos/
- [ ] Parse Move ABI format
- [ ] Golden test fixtures for major Move protocols

### 6.4 — WASM Plugin System
- [ ] Define WASM plugin interface (wit-bindgen)
- [ ] Runtime plugin loading via wasmtime
- [ ] Sandbox security: no filesystem, no network
- [ ] Example plugin for a custom chain

### 6.5 — Go SDK
- [ ] Create bindings/go/ with cgo
- [ ] Publish to pkg.go.dev
- [ ] Go-idiomatic API (channels for streaming)

### 6.6 — Transport Adapters
- [ ] Kafka consumer adapter (rdkafka)
- [ ] Redis Streams adapter
- [ ] Custom transport trait for user-defined sources

### 6.7 — v1.0 Stable Release
- [ ] All golden tests pass across EVM + Solana + Cosmos
- [ ] All SDKs (TS, Python, Java, Go, WASM) published
- [ ] Public registry at 2000+ schemas
- [ ] LTS commitment: 18-month security patches
- [ ] Comprehensive docs site live
- [ ] Conference talk delivered (EthCC or ETH Prague)

---

## Performance Targets

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Single EVM event decode | < 1 μs | `cargo criterion -p chaincodec-evm` |
| Batch decode throughput | > 1M events/sec | 10M ERC20 events on 8-core |
| Schema lookup (cached) | < 100 ns | RocksDB/HashMap point lookup |
| Schema lookup (remote) | < 50 ms p99 | HTTP GET to hosted registry |
| Memory per chain listener | < 10 MB | tokio task + WS buffer |
| WASM decode (browser) | < 5 μs/event | Chrome V8, single-threaded |
| npm package size | < 5 MB | napi-rs binary + JS |
| WASM bundle (gzip) | < 200 KB | wasm-opt -O3 + gzip |

---

## Version Milestones

| Version | Status | Key Feature |
|---------|--------|-------------|
| v0.1.0 | MVP | EVM decode + local registry + TypeScript SDK |
| v0.2.0 | Beta | Python SDK + WASM + batch decode + CLI |
| v0.3.0 | Beta | Solana support + hosted registry |
| v0.5.0 | RC | Cosmos + Java SDK + enterprise registry |
| v0.7.0 | RC | SUI/Aptos + WASM plugins |
| v1.0.0 | Stable | All chains + full plugin system + LTS |

---

## Quick Reference: Daily Development Commands

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Run golden fixture tests only
cargo test -p chaincodec-evm -- golden

# Run benchmarks
cargo criterion -p chaincodec-evm

# Check formatting + linting
cargo fmt --check && cargo clippy --workspace -- -D warnings

# Build TypeScript SDK
cd bindings/node && npm run build

# Build WASM
cd bindings/wasm && wasm-pack build --target web

# Build Python SDK
cd bindings/python && maturin develop

# Run Python tests
cd bindings/python && pytest tests/

# CLI verify
cargo run -p chaincodec-cli -- test --fixtures fixtures/evm/

# Start registry server locally
cd registry-server && docker-compose up

# Build docs
mdbook build docs/
```
