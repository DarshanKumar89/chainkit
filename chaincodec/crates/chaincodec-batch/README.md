# chaincodec-batch

High-throughput batch decode engine for ChainCodec — Rayon parallel processing.

[![crates.io](https://img.shields.io/crates/v/chaincodec-batch)](https://crates.io/crates/chaincodec-batch)
[![docs.rs](https://docs.rs/chaincodec-batch/badge.svg)](https://docs.rs/chaincodec-batch)

## Features

- Decode millions of historical events per second using Rayon thread pool
- Pluggable storage backend (SQLite, Postgres, custom)
- Streaming output via channels — start processing results before all events decode
- Per-batch error isolation — one malformed log never drops the whole batch

## Usage

```toml
[dependencies]
chaincodec-batch = "0.1"
```

```rust
use chaincodec_batch::BatchDecoder;
use chaincodec_evm::EvmDecoder;

let decoder = BatchDecoder::new(EvmDecoder::new(), registry);

let events: Vec<RawEvent> = fetch_logs(from_block, to_block).await?;
let results = decoder.decode_batch(&events);

for result in results {
    match result {
        Ok(decoded) => store(decoded),
        Err(e) => log_error(e),
    }
}
```

## Performance

| Mode | Throughput |
|------|-----------|
| Single-thread | ~1M events/sec |
| Rayon 8-thread | ~6M events/sec |

Benchmarks run on Apple M3 Pro with ERC-20 Transfer events.

## License

MIT — see [LICENSE](../../LICENSE)
