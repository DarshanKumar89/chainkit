# chaincodec-core

Core traits, shared types, and primitives for the ChainCodec ecosystem.

[![crates.io](https://img.shields.io/crates/v/chaincodec-core)](https://crates.io/crates/chaincodec-core)
[![docs.rs](https://docs.rs/chaincodec-core/badge.svg)](https://docs.rs/chaincodec-core)
[![license](https://img.shields.io/crates/l/chaincodec-core)](LICENSE)

`chaincodec-core` is the shared foundation for all ChainCodec crates. It defines the traits every chain-specific decoder must implement and the universal value types that all decoded output normalizes to — so your application code never depends on chain-specific representations.

---

## What's in this crate

| Item | Description |
|------|-------------|
| `ChainDecoder` | Trait every chain decoder (EVM, Solana, Cosmos) must implement |
| `SchemaRegistry` | Trait for O(1) schema lookup by fingerprint or name |
| `ProgressCallback` | Blanket-impl trait for batch progress callbacks |
| `NormalizedValue` | Universal typed value — address, uint256, bytes, string, bool, array, tuple |
| `RawEvent` | Uninterpreted log/event from any chain |
| `DecodedEvent` | Fully decoded, schema-validated event with named fields |
| `ChainId` | Chain identifier — EVM chain IDs, Solana, Cosmos |
| `EventFingerprint` | Topic0 hash or equivalent for schema matching |
| `DecodeError` | Typed error variants for single-event decode failures |
| `BatchDecodeError` | Typed error variants for batch decode failures |
| `ErrorMode` | Controls batch error handling: `Skip` / `Collect` / `Throw` |

---

## Installation

```toml
[dependencies]
chaincodec-core = "0.1"
```

---

## Quick start

```rust
use chaincodec_core::{
    chain::{ChainId, chains},
    event::RawEvent,
    types::NormalizedValue,
};

// Build a raw EVM log (normally from eth_getLogs)
let raw = RawEvent {
    chain: chains::ethereum(),                         // ChainId for Ethereum mainnet
    tx_hash: "0xabc123...".to_string(),
    block_number: 19_500_000,
    block_timestamp: 1_710_000_000,
    log_index: 0,
    address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
    topics: vec![
        // keccak256("Transfer(address,address,uint256)")
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".to_string(),
    ],
    data: vec![0u8; 32],
    raw_receipt: None,
};

// NormalizedValue is the universal output type
let addr  = NormalizedValue::Address("0xabc...".to_string());
let value = NormalizedValue::Uint256(1_000_000);
let flag  = NormalizedValue::Bool(true);
```

---

## Implementing `ChainDecoder`

Add support for a new chain by implementing the `ChainDecoder` trait:

```rust
use chaincodec_core::{
    chain::ChainFamily,
    decoder::ChainDecoder,
    error::DecodeError,
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::Schema,
};

pub struct MyChainDecoder;

impl ChainDecoder for MyChainDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Evm
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        // EVM: topic0 is keccak256 of the event signature
        raw.topics.first().cloned().unwrap_or_default().into()
    }

    fn decode_event(&self, raw: &RawEvent, schema: &Schema) -> Result<DecodedEvent, DecodeError> {
        // Decode raw ABI data into named NormalizedValue fields
        todo!()
    }

    // decode_batch() has a default implementation that loops decode_event().
    // Override it to use Rayon or other parallelism.
}
```

---

## NormalizedValue variants

```rust
pub enum NormalizedValue {
    Address(String),            // EVM address: "0xabc..."
    Uint256(u128),              // For full 256-bit precision, BigUint is also supported
    Int256(i128),
    Bool(bool),
    Bytes(Vec<u8>),             // Fixed-size: bytes1 .. bytes32
    BytesDynamic(Vec<u8>),      // Dynamic: bytes
    String(String),
    Array(Vec<NormalizedValue>),
    Tuple(Vec<NormalizedValue>),
}
```

All chain decoders produce `NormalizedValue` so downstream storage and analytics code stays chain-agnostic.

---

## Built-in chain IDs

```rust
use chaincodec_core::chain::chains;

let eth  = chains::ethereum();   // evm_id: 1
let arb  = chains::arbitrum();   // evm_id: 42161
let base = chains::base();       // evm_id: 8453
let poly = chains::polygon();    // evm_id: 137
let op   = chains::optimism();   // evm_id: 10
let avax = chains::avalanche();  // evm_id: 43114
let bnb  = chains::bsc();        // evm_id: 56

// Custom chain (local Anvil / Hardhat)
let local = ChainId::evm(31337);
```

---

## Error modes for batch decoding

```rust
use chaincodec_core::decoder::ErrorMode;

// Skip silently — best for analytics where a few bad logs are acceptable
let mode = ErrorMode::Skip;

// Collect errors alongside successes — inspect failures after decoding
let mode = ErrorMode::Collect;

// Abort immediately on first failure — best for critical data pipelines
let mode = ErrorMode::Throw;
```

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| **chaincodec-core** | Traits, types, primitives (this crate) |
| [chaincodec-evm](https://crates.io/crates/chaincodec-evm) | EVM ABI event & call decoder |
| [chaincodec-registry](https://crates.io/crates/chaincodec-registry) | CSDL schema registry |
| [chaincodec-batch](https://crates.io/crates/chaincodec-batch) | Rayon parallel batch decode |
| [chaincodec-stream](https://crates.io/crates/chaincodec-stream) | Live WebSocket event streaming |
| [chaincodec-observability](https://crates.io/crates/chaincodec-observability) | OpenTelemetry metrics & tracing |

---

## License

MIT — see [LICENSE](../../LICENSE)
