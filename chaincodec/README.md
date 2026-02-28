# chaincodec

Universal blockchain ABI decoder — production-grade EVM event log and function call
decoding for Rust, TypeScript/Node.js, Python, and WebAssembly.

[![crates.io](https://img.shields.io/crates/v/chaincodec-core)](https://crates.io/crates/chaincodec-core)
[![docs.rs](https://docs.rs/chaincodec-core/badge.svg)](https://docs.rs/chaincodec-core)
[![npm](https://img.shields.io/npm/v/@chainfoundry/chaincodec)](https://www.npmjs.com/package/@chainfoundry/chaincodec)
[![PyPI](https://img.shields.io/pypi/v/chaincodec)](https://pypi.org/project/chaincodec/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Features

- **Event decoding** — Decode EVM `eth_getLogs` output using CSDL schemas with full type normalization
- **Function call decoding** — Decode transaction `input` data (calldata) to named arguments
- **ABI encoding** — Encode typed arguments into calldata for any function
- **Constructor decoding** — Decode deploy transaction arguments
- **EIP-712 typed data** — Parse `eth_signTypedData_v4` JSON payloads
- **Auto ABI fetch** — Pull ABIs from Sourcify, Etherscan, and 4byte.directory automatically
- **53 built-in schemas** — ERC-20/721/1155, Uniswap, Aave, Compound, Lido, Curve, and 40+ more
- **Batch decode** — >1M events/sec single-thread via Rayon parallel processing
- **Cross-chain** — EVM, Cosmos/CosmWasm, Solana/Anchor IDL (Phase 2)
- **Zero unsafe** — 100% safe Rust with `#![deny(unsafe_code)]`

## Install

**Rust**
```toml
[dependencies]
chaincodec-core     = "0.1"
chaincodec-evm      = "0.1"
chaincodec-registry = "0.1"
```

**Node.js / TypeScript**
```bash
npm install @chainfoundry/chaincodec
```

**Python**
```bash
pip install chaincodec
```

**Browser (WASM)**
```bash
npm install @chainfoundry/chaincodec-wasm
```

## Rust Quick Start

```rust
use chaincodec_registry::memory::InMemoryRegistry;
use chaincodec_evm::decoder::EvmDecoder;
use chaincodec_core::types::RawLog;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load schemas from directory
    let mut registry = InMemoryRegistry::new();
    registry.load_directory("schemas/")?;

    let decoder = EvmDecoder::new();

    // Decode an ERC-20 Transfer
    let log = RawLog {
        address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
        topics: vec![
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
        ],
        data: hex::decode("000000000000000000000000000000000000000000000000000000003b9aca00")?,
    };

    let fingerprint = &log.topics[0];
    if let Some(schema) = registry.get_by_fingerprint(fingerprint) {
        let event = decoder.decode_event(&log, schema)?;
        println!("{} {:?}", event.schema_name, event.fields);
    }

    Ok(())
}
```

## TypeScript Quick Start

```typescript
import { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec';

const registry = new MemoryRegistry();
await registry.loadFile('./schemas/erc20.csdl');

const decoder = new EvmDecoder();
const event = decoder.decodeEvent(rawLog, registry.getByFingerprint(fp));
console.log(event);
// { schemaName: 'ERC20Transfer', fields: { from: '0xd8dA...', to: '0xAb58...', value: 1000000000n } }
```

## Python Quick Start

```python
from chaincodec import EvmDecoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_file("schemas/erc20.csdl")

decoder = EvmDecoder()
event = decoder.decode_event(raw_log, registry.get_by_fingerprint(fingerprint))
print(event)
# DecodedEvent(schema='ERC20Transfer', fields={'from': '0xd8dA...', 'to': '0xAb58...', 'value': 1000000000})
```

## Browser WASM Quick Start

```typescript
import init, { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec-wasm';

await init();  // load the .wasm file

const registry = new MemoryRegistry();
registry.loadSchemaJson(erc20SchemaJson);

const decoder = new EvmDecoder();
const event = decoder.decodeEvent(rawLog, registry.getByFingerprint(fp));
```

## Built-in Schemas (53 protocols)

| Category | Protocols |
|----------|-----------|
| **Tokens** | ERC-20, ERC-721, ERC-1155, WETH, ERC-4626, ENS, ERC-4337 EntryPoint |
| **DEX** | Uniswap V2/V3, SushiSwap, Curve, Balancer V2, Camelot, DODO, Bancor V3 |
| **Lending** | Aave V2/V3, Compound V2/V3, Morpho Blue, Spark |
| **Derivatives** | GMX, GMX V1, dYdX V4, Perpetual Protocol, Synthetix V3, Ribbon Finance |
| **LSD/Staking** | Lido, Rocket Pool, Frax Ether, Stader, EigenLayer, Convex, Yearn |
| **Bridges** | Wormhole, LayerZero V2, Hop, Celer, Stargate, Across |
| **NFT** | OpenSea Seaport, Blur, CryptoPunks, BAYC, LooksRare |
| **Governance** | Safe Multisig, Compound Governance |
| **Oracle** | Chainlink (Price Feed, VRF, CCIP) |
| **Other** | Pendle Finance, Maker DAO |

## Workspace Crates

| Crate | Purpose | crates.io |
|-------|---------|-----------|
| [`chaincodec-core`] | Shared traits, type normalizer, NormalizedValue | [![crates.io](https://img.shields.io/crates/v/chaincodec-core)](https://crates.io/crates/chaincodec-core) |
| [`chaincodec-evm`] | EVM ABI decoder, call decoder, encoder | [![crates.io](https://img.shields.io/crates/v/chaincodec-evm)](https://crates.io/crates/chaincodec-evm) |
| [`chaincodec-registry`] | CSDL parser, schema registry, remote fetch | [![crates.io](https://img.shields.io/crates/v/chaincodec-registry)](https://crates.io/crates/chaincodec-registry) |
| [`chaincodec-batch`] | Rayon parallel batch decode engine | [![crates.io](https://img.shields.io/crates/v/chaincodec-batch)](https://crates.io/crates/chaincodec-batch) |
| [`chaincodec-stream`] | Tokio real-time event streaming | [![crates.io](https://img.shields.io/crates/v/chaincodec-stream)](https://crates.io/crates/chaincodec-stream) |
| [`chaincodec-observability`] | OpenTelemetry metrics + tracing | [![crates.io](https://img.shields.io/crates/v/chaincodec-observability)](https://crates.io/crates/chaincodec-observability) |
| [`chaincodec-cli`] | `chaincodec` CLI tool | [![crates.io](https://img.shields.io/crates/v/chaincodec-cli)](https://crates.io/crates/chaincodec-cli) |

## CSDL Schema Format

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum, polygon, arbitrum, base, optimism, avalanche]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true,  description: "Sender" }
    to:    { type: address, indexed: true,  description: "Recipient" }
    value: { type: uint256, indexed: false, description: "Token amount" }
  meta:
    protocol: erc20
    category: token
    verified: true
```

## Performance

| Benchmark | Single-thread | 8-thread Rayon |
|-----------|--------------|----------------|
| Event decode | 1.2M events/sec | 6.8M events/sec |
| Call decode | 900K calls/sec | 5.1M calls/sec |
| Schema load (50 files) | 12ms | — |

## License

MIT — see [LICENSE](LICENSE)

## Related

- [chainerrors](../chainerrors/) — EVM revert decoder (require/panic/custom errors)
- [chainrpc](../chainrpc/) — Multi-provider JSON-RPC client with circuit breaker
- [chainindex](../chainindex/) — EVM event indexer with reorg detection
