# chaincodec-evm

EVM ABI event & function call decoder for ChainCodec — built on [alloy-rs](https://github.com/alloy-rs).

[![crates.io](https://img.shields.io/crates/v/chaincodec-evm)](https://crates.io/crates/chaincodec-evm)
[![docs.rs](https://docs.rs/chaincodec-evm/badge.svg)](https://docs.rs/chaincodec-evm)

## Features

- **Event decoding** — decode `eth_getLogs` entries against a CSDL schema
- **Call decoding** — decode function calldata from any standard Ethereum ABI JSON
- **ABI encoding** — encode function calls for on-chain submission
- **EIP-712 typed data** — parse `eth_signTypedData_v4` payloads
- **Fingerprinting** — compute `keccak256(topic0)` selectors for schema matching
- **Type normalization** — all decoded values become `NormalizedValue`

## Usage

```toml
[dependencies]
chaincodec-evm      = "0.1"
chaincodec-core     = "0.1"
chaincodec-registry = "0.1"
```

```rust
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_core::{decoder::ChainDecoder, schema::SchemaRegistry};

let mut registry = MemoryRegistry::new();
registry.load_file("schemas/erc20.csdl")?;

let decoder = EvmDecoder::new();
let event = decoder.decode_event(&raw_log, &schema)?;
println!("{:?}", event.fields);
```

## Decode call data

```rust
use chaincodec_evm::EvmCallDecoder;

let abi_json = std::fs::read_to_string("abi/uniswap_v3.json")?;
let decoder = EvmCallDecoder::from_abi_json(&abi_json)?;
let call = decoder.decode_call(&calldata, None)?;
println!("function: {}", call.function_name);
```

## License

MIT — see [LICENSE](../../LICENSE)
