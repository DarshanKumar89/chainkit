# Getting Started with ChainCodec

> Go from raw blockchain bytes to structured, typed data in under 5 minutes.

---

## What is ChainCodec?

Every blockchain emits events as raw binary blobs. Without a decoder they are meaningless bytes:

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

It supports **EVM** (Ethereum, Arbitrum, Base, Polygon, Optimism), **Solana** (Anchor/Borsh), and **Cosmos** (CosmWasm/ABCI), all producing the same typed output.

---

## Core Concepts

Before writing code, understand these four building blocks:

### 1. Schema (CSDL)

A schema describes one event type — its fields, types, and which chains it lives on. Schemas are written in **CSDL** (ChainCodec Schema Definition Language), a human-readable YAML format:

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

Full CSDL syntax reference → [csdl-reference.md](./csdl-reference.md)

### 2. Fingerprint

The fingerprint is the unique identifier of an event type:

| Chain | Fingerprint = |
|-------|--------------|
| EVM | `keccak256("Transfer(address,address,uint256)")` — `topics[0]` |
| Solana/Anchor | `SHA-256("event:Transfer")[..8]` — 8-byte discriminator |
| Cosmos/CosmWasm | `SHA-256("event:wasm/transfer")[..16]` |

The decoder uses the fingerprint to look up the correct schema from the registry — no hard-coding of event types required.

### 3. NormalizedValue

All decoded field values are typed as `NormalizedValue`:

| Variant | Used for |
|---------|---------|
| `Uint(u128)` | uint8 – uint128 |
| `BigUint(String)` | uint256 (decimal string) |
| `Int(i128)` | int8 – int128 |
| `Address(String)` | 20-byte EVM address (EIP-55 checksummed) |
| `Pubkey(String)` | 32-byte Solana pubkey (base58) |
| `Bech32(String)` | Cosmos bech32 address |
| `Bool(bool)` | boolean |
| `Bytes(Vec<u8>)` | bytesN |
| `Str(String)` | string |
| `Hash256(String)` | 32-byte hash |
| `Array(Vec<NormalizedValue>)` | dynamic array |
| `Null` | field missing / decode error |

The same variant is used regardless of whether the event came from EVM, Solana, or Cosmos.

### 4. Decoder + Registry

```
RawEvent  ──►  ChainDecoder.decode_event(raw, schema)  ──►  DecodedEvent
                     ▲
             SchemaRegistry.get_by_fingerprint(fp)
                     ▲
             MemoryRegistry (loaded from CSDL)
```

---

## Installation

### Rust

Add to your `Cargo.toml`:

```toml
[dependencies]
chaincodec-core     = "0.1"
chaincodec-evm      = "0.1"
chaincodec-registry = "0.1"
```

For Solana or Cosmos decoding:
```toml
chaincodec-solana   = "0.1"   # Anchor/Borsh events
chaincodec-cosmos   = "0.1"   # CosmWasm/ABCI events
```

For batch processing:
```toml
chaincodec-batch    = "0.1"   # Rayon parallel bulk decode
```

For real-time streaming:
```toml
chaincodec-stream   = "0.1"   # WebSocket live event stream
```

### TypeScript / Node.js

```bash
npm install @chainkit/chaincodec
```

### Python

```bash
pip install chaincodec
```

### Browser / WASM

```bash
npm install @chainkit/chaincodec-wasm
```

### CLI

```bash
cargo install chaincodec-cli
chaincodec --help
```

---

## 5-Minute Quickstart (Rust)

### Step 1 — Write your schema

Create `my-schema.csdl`:

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
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

ChainCodec ships with [24 bundled schemas](../schemas/) for ERC-20, ERC-721, Uniswap V3, Aave V3, and more — you don't need to write them yourself for common protocols.

### Step 2 — Load the schema and decode

```rust
use chaincodec_core::{chain::chains, decoder::ChainDecoder, event::RawEvent, schema::SchemaRegistry};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};

fn main() -> anyhow::Result<()> {
    // 1. Parse the schema
    let csdl = std::fs::read_to_string("my-schema.csdl")?;
    let registry = MemoryRegistry::new();
    for schema in CsdlParser::parse_all(&csdl)? {
        registry.add(schema)?;
    }

    // 2. Create the decoder
    let decoder = EvmDecoder::new();

    // 3. Build a raw event (in production, this comes from your RPC node)
    let raw = RawEvent {
        chain: chains::ethereum(),
        tx_hash: "0xabc123".into(),
        block_number: 19_000_000,
        block_timestamp: 1_700_000_000,
        log_index: 0,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
        topics: vec![
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
        ],
        data: hex::decode(
            "0000000000000000000000000000000000000000000000000000000000989680"
        )?,
        raw_receipt: None,
    };

    // 4. Fingerprint lookup + decode
    let fp = decoder.fingerprint(&raw);
    let schema = registry.get_by_fingerprint(&fp).expect("unknown event");
    let decoded = decoder.decode_event(&raw, &schema)?;

    // 5. Read the typed fields
    println!("schema: {} v{}", decoded.schema, decoded.schema_version);
    println!("chain:  {}", decoded.chain);
    println!("block:  #{}", decoded.block_number);
    println!("from:   {}", decoded.fields["from"]);
    println!("to:     {}", decoded.fields["to"]);
    println!("value:  {}", decoded.fields["value"]);

    Ok(())
}
```

### Step 3 — Run it

```
schema: ERC20Transfer v1
chain:  ethereum
block:  #19000000
from:   0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
to:     0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
value:  10000000
```

The `value` field is `NormalizedValue::BigUint("10000000")` — a decimal string, safe for arbitrary-precision arithmetic.

---

## 5-Minute Quickstart (TypeScript)

```typescript
import { EvmDecoder, MemoryRegistry, CsdlParser } from '@chainkit/chaincodec';
import { readFileSync } from 'fs';

// Load schema
const registry = new MemoryRegistry();
registry.loadCsdl(readFileSync('my-schema.csdl', 'utf8'));

// Decode
const decoder = new EvmDecoder();
const event = decoder.decodeEvent({
  chain: 'ethereum',
  txHash: '0xabc123',
  blockNumber: 19000000,
  blockTimestamp: 1700000000,
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef',
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045',
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b',
  ],
  data: '0x0000000000000000000000000000000000000000000000000000000000989680',
}, registry);

console.log(event.schema);          // "ERC20Transfer"
console.log(event.fields.from);     // { type: "address", value: "0xd8dA..." }
console.log(event.fields.value);    // { type: "biguint", value: "10000000" }
```

---

## 5-Minute Quickstart (Python)

```python
from chaincodec import EvmDecoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_file("my-schema.csdl")

decoder = EvmDecoder()
event = decoder.decode_event({
    "chain": "ethereum",
    "tx_hash": "0xabc123",
    "block_number": 19_000_000,
    "block_timestamp": 1_700_000_000,
    "log_index": 0,
    "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
    "topics": [
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045",
        "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b",
    ],
    "data": "0x0000000000000000000000000000000000000000000000000000000000989680",
}, registry)

print(event["schema"])           # ERC20Transfer
print(event["fields"]["from"])   # 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
print(event["fields"]["value"])  # 10000000
```

---

## 5-Minute Quickstart (CLI)

```bash
# Decode a raw EVM log directly from the terminal
chaincodec decode-log \
  --topics \
    0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef \
    0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045 \
    0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b \
  --data 0x0000000000000000000000000000000000000000000000000000000000989680 \
  --schema-dir ./schemas \
  --chain ethereum

# Output:
# schema:  ERC20Transfer v1
# from:    0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
# to:      0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
# value:   10000000
```

---

## Common Patterns

### Decode from an RPC node (production pattern)

```rust
// Production: fetch logs from your Ethereum node, then decode
use chaincodec_core::{decoder::ChainDecoder, schema::SchemaRegistry};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;

let registry = MemoryRegistry::new();
registry.load_directory(std::path::Path::new("./schemas"))?;

let decoder = EvmDecoder::new();

// For each log returned by eth_getLogs / eth_getTransactionReceipt:
for raw_log in logs_from_node {
    let fp = decoder.fingerprint(&raw_log);
    if let Some(schema) = registry.get_by_fingerprint(&fp) {
        let decoded = decoder.decode_event(&raw_log, &schema)?;
        // Store decoded.fields in your database
    }
    // Logs with no matching schema are silently skipped
}
```

### Batch decode historical events

```rust
use chaincodec_batch::{BatchEngine, BatchRequest};
use chaincodec_core::decoder::{ChainDecoder, ErrorMode};
use chaincodec_evm::EvmDecoder;
use std::sync::Arc;

let mut engine = BatchEngine::new(Arc::new(registry));
engine.add_decoder("ethereum", Arc::new(EvmDecoder::new()) as Arc<dyn ChainDecoder>);

let result = engine.decode(
    BatchRequest::new("ethereum", all_raw_logs)
        .error_mode(ErrorMode::Collect)
)?;

println!("{} decoded, {} errors", result.events.len(), result.errors.len());
```

### Real-time streaming

```rust
use chaincodec_stream::{StreamConfig, StreamEngine};

let config = StreamConfig {
    rpc_url: "wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY".into(),
    chain: chains::ethereum(),
    schemas: vec!["ERC20Transfer".into()],  // filter to specific events
    ..Default::default()
};

let (engine, mut rx) = StreamEngine::new(config, registry, decoder).await?;
engine.start().await;

while let Ok(event) = rx.recv().await {
    println!("{}: {}", event.schema, event.fields["value"]);
}
```

### Multi-chain (EVM + Solana + Cosmos)

```rust
use chaincodec_evm::EvmDecoder;
use chaincodec_solana::SolanaDecoder;
use chaincodec_cosmos::CosmosDecoder;
use chaincodec_core::chain::ChainFamily;

let decoded = match raw.chain.family {
    ChainFamily::Evm    => EvmDecoder::new().decode_event(&raw, &schema)?,
    ChainFamily::Solana => SolanaDecoder::new().decode_event(&raw, &schema)?,
    ChainFamily::Cosmos => CosmosDecoder::new().decode_event(&raw, &schema)?,
    _ => return Err(anyhow::anyhow!("unsupported chain")),
};

// decoded.fields["amount"] is NormalizedValue regardless of chain
```

---

## Running the Examples

The `chaincodec/examples/` directory contains 13 runnable Rust programs, one for each major feature area. Clone the repo and run any of them:

```bash
git clone https://github.com/DarshanKumar89/chainkit
cd chainkit/chaincodec

# Basic ERC-20 decode
cargo run --bin decode_erc20

# Batch decode with progress
cargo run --bin batch_decode

# Multi-protocol in one batch
cargo run --bin decode_multiprotocol

# Solana/Anchor decode
cargo run --bin decode_solana

# Cosmos/CosmWasm decode
cargo run --bin decode_cosmos

# EIP-712 typed data
cargo run --bin eip712_decode

# Proxy detection
cargo run --bin proxy_detect

# OpenTelemetry metrics + structured logging
cargo run --bin with_observability

# All examples listed:
cargo run --bin decode_erc20
cargo run --bin batch_decode
cargo run --bin stream_demo
cargo run --bin fetch_and_decode
cargo run --bin decode_multiprotocol
cargo run --bin csdl_registry
cargo run --bin decode_call
cargo run --bin encode_call
cargo run --bin proxy_detect
cargo run --bin eip712_decode
cargo run --bin decode_solana
cargo run --bin decode_cosmos
cargo run --bin with_observability
```

Full examples walkthrough → [examples.md](./examples.md)

---

## Using Bundled Schemas

ChainCodec ships 24 production-ready schemas. Load them all at once:

```rust
let registry = MemoryRegistry::new();
registry.load_directory(std::path::Path::new("./schemas"))?;
println!("Loaded {} schemas", registry.len());
// Output: Loaded 24 schemas
```

Or load a specific category:
```rust
registry.load_directory(std::path::Path::new("./schemas/tokens"))?;   // ERC-20, 721, 1155, 4626, WETH
registry.load_directory(std::path::Path::new("./schemas/defi"))?;     // Uniswap, Aave, Compound, ...
registry.load_directory(std::path::Path::new("./schemas/nft"))?;      // OpenSea, Blur
registry.load_directory(std::path::Path::new("./schemas/bridge"))?;   // Across, Stargate
```

---

## Error Handling

```rust
use chaincodec_core::decoder::ErrorMode;

// Three modes for batch decode:

// Skip errors silently (best for analytics — process what you can)
BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Skip)

// Collect all errors without stopping (good for debugging)
BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Collect)
// result.errors: Vec<(usize, DecodeError)>  — index + reason

// Fail fast on first error (good for validation pipelines)
BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Throw)
```

For individual event decode, `decode_event` returns `Result<DecodedEvent, DecodeError>`.

After a successful decode, check for partial field errors:
```rust
let decoded = decoder.decode_event(&raw, &schema)?;
if decoded.has_errors() {
    for (field_name, error_msg) in &decoded.decode_errors {
        eprintln!("field '{}' failed: {}", field_name, error_msg);
    }
}
```

---

## Next Steps

| Goal | Read |
|------|------|
| Write your own CSDL schemas | [csdl-reference.md](./csdl-reference.md) |
| Understand all 13 examples | [examples.md](./examples.md) |
| See real-world use cases | [use-cases.md](./use-cases.md) |
| Deep-dive into the architecture | [architecture.md](./architecture.md) |
| Browse bundled schemas | [../schemas/](../schemas/) |
| CLI reference | `chaincodec --help` |
