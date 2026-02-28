# chaincodec-cosmos

Cosmos / CosmWasm event decoder for ChainCodec.

[![crates.io](https://img.shields.io/crates/v/chaincodec-cosmos)](https://crates.io/crates/chaincodec-cosmos)
[![docs.rs](https://docs.rs/chaincodec-cosmos/badge.svg)](https://docs.rs/chaincodec-cosmos)

## Status

Phase 2 — implementation in progress.

Planned features:
- Decode CosmWasm contract events from Cosmos transaction results
- Protobuf deserialization for native Cosmos SDK events
- IBC packet tracking and cross-chain event correlation
- Support for Osmosis, Injective, and other Cosmos ecosystem chains

## Planned Usage

```rust
use chaincodec_cosmos::CosmosDecoder;

let decoder = CosmosDecoder::new();
let event = decoder.decode_tx_event(&tx_result)?;
```

## License

MIT — see [LICENSE](../../LICENSE)
