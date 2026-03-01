# chaincodec-batch

High-throughput batch decode engine for ChainCodec — parallel processing with Rayon.

[![crates.io](https://img.shields.io/crates/v/chaincodec-batch)](https://crates.io/crates/chaincodec-batch)
[![docs.rs](https://docs.rs/chaincodec-batch/badge.svg)](https://docs.rs/chaincodec-batch)
[![license](https://img.shields.io/crates/l/chaincodec-batch)](LICENSE)

`chaincodec-batch` turns a `Vec<RawEvent>` into decoded results as fast as your hardware allows. It chunks the input, fans out across a Rayon thread pool, and collects results with per-item error isolation — one malformed log never drops the whole batch.

---

## Features

- **Rayon parallel processing** — saturate all CPU cores for historical backfills
- **Chunked execution** — configurable chunk size caps per-chunk memory
- **Three error modes** — `Skip` (analytics), `Collect` (audit), `Throw` (strict pipelines)
- **Progress callbacks** — report decode progress to a UI or logging system
- **Chain-agnostic** — register any `ChainDecoder` implementation (EVM, Solana, Cosmos)
- **Zero unsafe code** — pure safe Rust

---

## Installation

```toml
[dependencies]
chaincodec-batch    = "0.1"
chaincodec-evm      = "0.1"
chaincodec-registry = "0.1"
chaincodec-core     = "0.1"
```

---

## Quick start

```rust
use std::sync::Arc;
use chaincodec_batch::{BatchEngine, BatchRequest};
use chaincodec_core::decoder::ErrorMode;
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;

fn main() -> anyhow::Result<()> {
    // 1. Set up registry + decoder
    let mut registry = MemoryRegistry::new();
    registry.load_directory("schemas/")?;
    let registry = Arc::new(registry);

    let mut engine = BatchEngine::new(Arc::clone(&registry));
    engine.add_decoder("ethereum", Arc::new(EvmDecoder::new()));

    // 2. Fetch raw logs (from your RPC or database)
    let logs = fetch_logs(from_block, to_block)?;  // Vec<RawEvent>

    // 3. Build and execute the batch request
    let req = BatchRequest::new("ethereum", logs)
        .chunk_size(10_000)                        // process 10k events per chunk
        .error_mode(ErrorMode::Collect)            // gather errors instead of aborting
        .on_progress(|decoded, total| {
            println!("progress: {}/{}", decoded, total);
        });

    let result = engine.decode(req)?;

    println!("decoded: {}", result.events.len());
    println!("errors:  {}", result.errors.len());

    for event in &result.events {
        println!("{}: {:?}", event.schema_name, event.fields);
    }

    Ok(())
}
```

---

## Error modes

```rust
use chaincodec_core::decoder::ErrorMode;

// Skip bad logs silently — best for analytics / data pipelines
let req = BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Skip);

// Collect errors alongside successful results — inspect failures after the run
let req = BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Collect);
let result = engine.decode(req)?;
for (idx, err) in &result.errors {
    eprintln!("log[{}] failed: {}", idx, err);
}

// Abort on first failure — best for critical financial data
let req = BatchRequest::new("ethereum", logs).error_mode(ErrorMode::Throw);
```

---

## Progress reporting

```rust
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

let counter = Arc::new(AtomicUsize::new(0));
let counter_clone = Arc::clone(&counter);

let req = BatchRequest::new("ethereum", logs)
    .on_progress(move |decoded, total| {
        counter_clone.store(decoded, Ordering::Relaxed);
        // Could also send to a channel, update a progress bar, etc.
        if decoded % 100_000 == 0 {
            println!("{:.1}%", decoded as f64 / total as f64 * 100.0);
        }
    });
```

---

## Register multiple chains

```rust
let mut engine = BatchEngine::new(Arc::clone(&registry));
engine.add_decoder("ethereum",  Arc::new(EvmDecoder::new()));
engine.add_decoder("arbitrum",  Arc::new(EvmDecoder::new()));
engine.add_decoder("base",      Arc::new(EvmDecoder::new()));
engine.add_decoder("polygon",   Arc::new(EvmDecoder::new()));

// BatchRequest::chain matches the slug used in add_decoder
let req_eth = BatchRequest::new("ethereum", eth_logs);
let req_arb = BatchRequest::new("arbitrum", arb_logs);
```

---

## Performance

Benchmarks on Apple M3 Pro, ERC-20 Transfer events:

| Mode | Events/sec |
|------|-----------|
| Single-thread | ~1M |
| Rayon 8-thread | ~6M |
| Rayon 12-thread | ~8M |

Throughput scales with core count and schema complexity. Simple events (ERC-20 Transfer) decode faster than complex events (Uniswap V3 Swap with tuples).

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| [chaincodec-core](https://crates.io/crates/chaincodec-core) | Traits, types, primitives |
| [chaincodec-evm](https://crates.io/crates/chaincodec-evm) | EVM ABI event & call decoder |
| [chaincodec-registry](https://crates.io/crates/chaincodec-registry) | CSDL schema registry |
| **chaincodec-batch** | Rayon parallel batch decode (this crate) |
| [chaincodec-stream](https://crates.io/crates/chaincodec-stream) | Live WebSocket event streaming |

---

## License

MIT — see [LICENSE](../../LICENSE)
