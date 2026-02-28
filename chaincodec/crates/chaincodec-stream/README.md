# chaincodec-stream

Real-time blockchain event streaming engine for ChainCodec — Tokio-based WebSocket listener.

[![crates.io](https://img.shields.io/crates/v/chaincodec-stream)](https://crates.io/crates/chaincodec-stream)
[![docs.rs](https://docs.rs/chaincodec-stream/badge.svg)](https://docs.rs/chaincodec-stream)

## Features

- Subscribe to live EVM events via `eth_subscribe` (WebSocket)
- Automatic reconnect with exponential backoff
- Decoded events streamed through async `tokio::sync::mpsc` channels
- Supports multiple concurrent subscriptions across chains

## Usage

```toml
[dependencies]
chaincodec-stream = "0.1"
```

```rust
use chaincodec_stream::EventListener;

let listener = EventListener::builder()
    .ws_url("wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY")
    .registry(registry)
    .decoder(EvmDecoder::new())
    .build()
    .await?;

let mut rx = listener.subscribe().await?;
while let Some(event) = rx.recv().await {
    println!("{:?}", event.fields);
}
```

## License

MIT — see [LICENSE](../../LICENSE)
