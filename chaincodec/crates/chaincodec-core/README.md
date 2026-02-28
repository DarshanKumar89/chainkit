# chaincodec-core

Core decoder traits, type normalizer, and shared primitives for ChainCodec.

[![crates.io](https://img.shields.io/crates/v/chaincodec-core)](https://crates.io/crates/chaincodec-core)
[![docs.rs](https://docs.rs/chaincodec-core/badge.svg)](https://docs.rs/chaincodec-core)

## Overview

`chaincodec-core` provides the foundational building blocks used by all other
ChainCodec crates:

- **`ChainId`** — chain identifier (EVM chain ID, Solana, Cosmos)
- **`NormalizedValue`** — universal typed value that all decoders normalize to
- **`RawEvent`** — uninterpreted log/event from any chain
- **`DecodedEvent`** — fully decoded, schema-validated event
- **`SchemaRegistry`** trait — interface for schema lookup by fingerprint or name
- **`ChainDecoder`** trait — interface for per-chain decoder implementations

## Usage

```toml
[dependencies]
chaincodec-core = "0.1"
```

```rust
use chaincodec_core::{
    chain::chains,
    event::RawEvent,
    types::NormalizedValue,
};
```

## Part of ChainCodec

| Crate | Purpose |
|-------|---------|
| **chaincodec-core** | Traits, types, primitives |
| chaincodec-evm | EVM ABI event & call decoder |
| chaincodec-registry | CSDL schema registry |
| chaincodec-batch | Parallel batch decode |
| chaincodec-stream | Live event streaming |
| chaincodec-observability | Metrics & tracing |

## License

MIT — see [LICENSE](../../LICENSE)
