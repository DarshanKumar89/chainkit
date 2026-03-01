# chaincodec-evm

EVM ABI event decoder, function call decoder, and ABI encoder — built on [alloy-rs](https://github.com/alloy-rs).

[![crates.io](https://img.shields.io/crates/v/chaincodec-evm)](https://crates.io/crates/chaincodec-evm)
[![docs.rs](https://docs.rs/chaincodec-evm/badge.svg)](https://docs.rs/chaincodec-evm)
[![license](https://img.shields.io/crates/l/chaincodec-evm)](LICENSE)

Decode `eth_getLogs` entries, function calldata, and EIP-712 typed data payloads into clean, typed Rust structs. All values normalize to `NormalizedValue` — no raw ABI bytes, no manual hex parsing.

---

## Features

- **Event decoding** — decode `eth_getLogs` log entries against a CSDL schema using topic0 fingerprint matching
- **Call decoding** — decode function calldata from any standard Ethereum ABI JSON
- **ABI encoding** — encode function calls for on-chain submission
- **EIP-712 typed data** — parse `eth_signTypedData_v4` payloads
- **Fingerprinting** — compute `keccak256(event_signature)` topic0 selectors
- **Type normalization** — all decoded values become `NormalizedValue` (address, uint256, bytes, string, tuple, …)
- **50+ built-in schemas** — ERC-20/721/1155, Uniswap, Aave, Compound, and more via `chaincodec-registry`

---

## Installation

```toml
[dependencies]
chaincodec-evm      = "0.1"
chaincodec-core     = "0.1"
chaincodec-registry = "0.1"
```

---

## Decode an EVM event log

```rust
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;
use chaincodec_core::{decoder::ChainDecoder, schema::SchemaRegistry, event::RawEvent, chain::chains};

fn main() -> anyhow::Result<()> {
    // 1. Load schemas
    let mut registry = MemoryRegistry::new();
    registry.load_directory("schemas/")?;           // 50+ built-in CSDL schemas

    // 2. Raw log from eth_getLogs
    let raw = RawEvent {
        chain: chains::ethereum(),
        tx_hash: "0xabc123...".to_string(),
        block_number: 19_500_000,
        block_timestamp: 1_710_000_000,
        log_index: 0,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(), // USDC
        topics: vec![
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".to_string(), // Transfer
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".to_string(), // from
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".to_string(), // to
        ],
        data: hex::decode("00000000000000000000000000000000000000000000000000000000000f4240")?,
        raw_receipt: None,
    };

    // 3. Decode
    let decoder = EvmDecoder::new();
    let fp = decoder.fingerprint(&raw);
    let schema = registry.get_by_fingerprint(&fp).expect("schema not found");
    let event = decoder.decode_event(&raw, &schema)?;

    println!("event:  {}", event.schema_name);        // "ERC20Transfer"
    println!("from:   {}", event.fields["from"]);     // "0xd8da6bf26..."
    println!("to:     {}", event.fields["to"]);       // "0xab5801a7d3..."
    println!("value:  {}", event.fields["value"]);    // 1000000 (= 1 USDC)

    Ok(())
}
```

---

## Decode function calldata

```rust
use chaincodec_evm::EvmCallDecoder;

// Load ABI JSON (from Etherscan export, Hardhat artifacts, Foundry out/, etc.)
let abi_json = std::fs::read_to_string("abi/uniswap_v3_router.json")?;
let decoder = EvmCallDecoder::from_abi_json(&abi_json)?;

// Raw calldata bytes from a transaction's `input` field
let calldata = hex::decode("414bf389...")?;
let call = decoder.decode_call(&calldata, None)?;

println!("function: {}", call.function_name);        // "exactInputSingle"
for (name, value) in &call.inputs {
    println!("  {}: {}", name, value);
}
// tokenIn:     0xC02aaA39b...  (WETH)
// tokenOut:    0xA0b8699...   (USDC)
// amountIn:    1000000000000000000
// amountOutMin: 1800000000
```

---

## Encode a function call

```rust
use chaincodec_evm::EvmCallDecoder;
use chaincodec_core::types::NormalizedValue;

let abi_json = std::fs::read_to_string("abi/erc20.json")?;
let decoder = EvmCallDecoder::from_abi_json(&abi_json)?;

let calldata = decoder.encode_call("transfer", &[
    NormalizedValue::Address("0xRecipient...".to_string()),
    NormalizedValue::Uint256(1_000_000),   // 1 USDC
])?;

// Submit calldata to your JSON-RPC provider
println!("calldata: 0x{}", hex::encode(&calldata));
```

---

## Compute topic0 fingerprint

```rust
use chaincodec_evm::fingerprint;

let fp = fingerprint::compute("Transfer(address,address,uint256)");
// "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"

let fp2 = fingerprint::compute("Swap(address,uint256,uint256,uint256,uint256,address)");
// Uniswap V2 Swap event topic0
```

---

## Error handling

```rust
use chaincodec_core::error::DecodeError;

match decoder.decode_event(&raw, &schema) {
    Ok(event) => { /* process event.fields */ }
    Err(DecodeError::SchemaNotFound { fingerprint }) => {
        // No schema registered for this topic0 — log and continue
        eprintln!("unknown event: {}", fingerprint);
    }
    Err(DecodeError::AbiDecodeFailed { reason }) => {
        // Malformed ABI data — happens with proxy contracts or exotic encodings
        eprintln!("ABI decode failed: {}", reason);
    }
    Err(e) => eprintln!("error: {}", e),
}
```

---

## Bundled schemas (50+)

`chaincodec-registry` ships ready-to-use schemas. Load all of them:

```rust
registry.load_directory("path/to/chaincodec/schemas/")?;
```

| Category | Schemas |
|----------|---------|
| Token standards | ERC-20, ERC-721, ERC-1155, ERC-4337, WETH |
| DEXes | Uniswap V2, Uniswap V3, Curve, Balancer, SushiSwap |
| Lending | Aave V2, Aave V3, Compound V2, Compound V3, MakerDAO |
| Oracles | ChainLink Aggregator, ChainLink VRF, ChainLink CCIP |
| Liquid staking | Lido, Rocket Pool, Stader, Frax |
| Bridges | Wormhole, LayerZero V2, Hop, Celer |
| Other | ENS, Safe Multisig, GMX V1, dYdX V4, Bancor V3 |

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| [chaincodec-core](https://crates.io/crates/chaincodec-core) | Traits, types, primitives |
| **chaincodec-evm** | EVM ABI event & call decoder (this crate) |
| [chaincodec-registry](https://crates.io/crates/chaincodec-registry) | CSDL schema registry |
| [chaincodec-batch](https://crates.io/crates/chaincodec-batch) | Rayon parallel batch decode |
| [chaincodec-stream](https://crates.io/crates/chaincodec-stream) | Live WebSocket event streaming |

---

## License

MIT — see [LICENSE](../../LICENSE)
