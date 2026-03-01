# chaincodec-stream

Real-time blockchain event streaming for ChainCodec — Tokio-based WebSocket listener with automatic reconnection.

[![crates.io](https://img.shields.io/crates/v/chaincodec-stream)](https://crates.io/crates/chaincodec-stream)
[![docs.rs](https://docs.rs/chaincodec-stream/badge.svg)](https://docs.rs/chaincodec-stream)
[![license](https://img.shields.io/crates/l/chaincodec-stream)](LICENSE)

`chaincodec-stream` connects to an Ethereum JSON-RPC WebSocket endpoint, subscribes to `eth_subscribe("logs", filter)`, and emits `RawEvent` items through an async channel. The WebSocket connection and resubscription are handled automatically on disconnect.

---

## Features

- **Live EVM log streaming** — subscribes to `eth_subscribe` and emits `RawEvent` items
- **Automatic reconnect** — the channel stays open across disconnects; caller never needs to re-subscribe
- **Address filtering** — subscribe to one or many contract addresses
- **Async channels** — events arrive via `futures::channel::mpsc`, composable with any Tokio runtime
- **Connection state** — query `is_connected()` at any time for health checks
- **Removed log filtering** — reorged / removed logs are dropped before reaching your handler

---

## Installation

```toml
[dependencies]
chaincodec-stream = "0.1"
chaincodec-evm    = "0.1"
chaincodec-core   = "0.1"
tokio             = { version = "1", features = ["full"] }
futures           = "0.3"
```

---

## Quick start

```rust
use std::sync::Arc;
use futures::StreamExt;
use chaincodec_stream::{ws_listener::EvmWsListener, listener::BlockListener};
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;
use chaincodec_core::{chain::chains, decoder::ChainDecoder, schema::SchemaRegistry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load schemas
    let mut registry = MemoryRegistry::new();
    registry.load_directory("schemas/")?;
    let decoder = EvmDecoder::new();

    // 2. Create a WebSocket listener for Ethereum mainnet
    let listener = EvmWsListener::new(
        chains::ethereum(),
        "wss://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY",
    )
    .with_address("0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48") // USDC
    .with_address("0xdac17f958d2ee523a2206206994597c13d831ec7"); // USDT

    // 3. Subscribe and decode events
    let mut stream = listener.subscribe().await?;

    println!("Listening for events...");
    while let Some(result) = stream.next().await {
        match result {
            Ok(raw) => {
                let fp = decoder.fingerprint(&raw);
                if let Some(schema) = registry.get_by_fingerprint(&fp) {
                    if let Ok(event) = decoder.decode_event(&raw, &schema) {
                        println!(
                            "[block {}] {} — {:?}",
                            raw.block_number, event.schema_name, event.fields
                        );
                    }
                }
            }
            Err(e) => eprintln!("stream error: {}", e),
        }
    }

    Ok(())
}
```

---

## Filter by multiple addresses

```rust
let listener = EvmWsListener::new(chains::ethereum(), "wss://...")
    .with_addresses([
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT
        "0x6b175474e89094c44da98b954eedeac495271d0f", // DAI
    ]);

// Omit .with_address() entirely to receive ALL logs on the chain
let listener_all = EvmWsListener::new(chains::ethereum(), "wss://...");
```

---

## Monitor connection health

```rust
let listener = Arc::new(EvmWsListener::new(chains::ethereum(), "wss://..."));

let monitor = Arc::clone(&listener);
tokio::spawn(async move {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        if !monitor.is_connected() {
            tracing::warn!("WebSocket disconnected — waiting for reconnect");
        }
    }
});

let mut stream = listener.subscribe().await?;
```

---

## Compact decode pipeline

```rust
while let Some(Ok(raw)) = stream.next().await {
    let fp = decoder.fingerprint(&raw);
    let Some(schema) = registry.get_by_fingerprint(&fp) else { continue };
    let Ok(event) = decoder.decode_event(&raw, &schema) else { continue };

    // Forward to database, message queue, webhook, etc.
    store_event(&event).await?;
}
```

---

## Stream error variants

```rust
use chaincodec_core::error::StreamError;

StreamError::ConnectionFailed { url, reason }  // could not connect
StreamError::Closed                            // connection dropped
StreamError::Timeout { ms }                    // subscription timed out
StreamError::Decode(err)                       // decode pipeline error
StreamError::Other(msg)                        // other runtime error
```

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| [chaincodec-core](https://crates.io/crates/chaincodec-core) | Traits, types, primitives |
| [chaincodec-evm](https://crates.io/crates/chaincodec-evm) | EVM ABI event & call decoder |
| [chaincodec-registry](https://crates.io/crates/chaincodec-registry) | CSDL schema registry |
| [chaincodec-batch](https://crates.io/crates/chaincodec-batch) | Historical batch decode |
| **chaincodec-stream** | Live WebSocket event streaming (this crate) |

---

## License

MIT — see [LICENSE](../../LICENSE)
