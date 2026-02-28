# chaincodec-solana

Solana / Anchor IDL decoder for ChainCodec.

[![crates.io](https://img.shields.io/crates/v/chaincodec-solana)](https://crates.io/crates/chaincodec-solana)
[![docs.rs](https://docs.rs/chaincodec-solana/badge.svg)](https://docs.rs/chaincodec-solana)

## Status

Phase 2 — implementation in progress.

Planned features:
- Decode Anchor IDL events from Solana transaction logs
- Borsh deserialization for account data
- SPL token transfer normalization
- Program-derived address (PDA) resolution

## Planned Usage

```rust
use chaincodec_solana::SolanaDecoder;

let decoder = SolanaDecoder::from_idl(idl_json)?;
let event = decoder.decode_log(&transaction_log)?;
```

## License

MIT — see [LICENSE](../../LICENSE)
