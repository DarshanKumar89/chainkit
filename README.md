# ChainKit

> **Production-grade blockchain primitives for Rust, TypeScript, Python, and WASM.**

ChainKit is a monorepo of four foundational Rust libraries for building blockchain data infrastructure. Each module is an independent Cargo workspace — use one, use all.

---

## Modules

| Module | Description | Status | crates.io |
|--------|-------------|--------|-----------|
| [`chaincodec`](./chaincodec/) | Universal ABI decoder — EVM events, calls, EIP-712, proxy detection | 🚧 In Development | — |
| [`chainerrors`](./chainerrors/) | EVM revert / panic / custom error decoder | 🚧 In Development | — |
| [`chainrpc`](./chainrpc/) | Resilient RPC transport with circuit breaker, rate limiter, auto-batch | 🚧 In Development | — |
| [`chainindex`](./chainindex/) | Reorg-safe blockchain indexer with pluggable storage | 🚧 In Development | — |

---

## chaincodec — First Module (In Development)

**chaincodec** is ChainKit's flagship module, currently in active development.

### What it does

```
EVM log → EvmDecoder → DecodedEvent { fields: { from, to, value }, ... }
Calldata → EvmCallDecoder → DecodedCall { function_name, inputs: [...] }
ABI JSON + args → EvmEncoder → 0xaabbccdd...
```

### Features

| Feature | Status |
|---------|--------|
| EVM event log decoding | ✅ |
| Function call decoding | ✅ |
| Constructor decoding | ✅ |
| ABI encoding (bidirectional) | ✅ |
| EIP-712 typed data | ✅ |
| Proxy detection (EIP-1967, EIP-1822, EIP-1167) | ✅ |
| Auto ABI fetch (Sourcify + Etherscan) | ✅ |
| CSDL schema format (YAML) | ✅ |
| 50+ bundled protocol schemas | ✅ |
| Parallel batch decode (Rayon) | ✅ |
| TypeScript / Node.js bindings (napi-rs) | ✅ |
| Python bindings (PyO3/maturin) | ✅ |
| WASM bindings (wasm-bindgen) | ✅ |

### Bundled schemas

chaincodec ships with production-ready schemas for 50+ protocols:

**Tokens**: ERC-20, ERC-721, ERC-1155, ERC-4626, WETH
**DEX**: Uniswap V2, Uniswap V3, Curve, Balancer V2, Pendle
**Lending**: Aave V3, Compound V2, Compound V3, Morpho Blue, MakerDAO
**Staking/Restaking**: Lido, EigenLayer
**Perpetuals**: GMX V1
**Oracles**: Chainlink Price Feeds, Chainlink OCR2
**NFT Marketplaces**: OpenSea Seaport, Blur
**Bridges**: Across Protocol, Stargate
**Governance**: Compound Governor Bravo

---

## Quick Start (Rust)

```toml
[dependencies]
chaincodec-evm      = "0.1"
chaincodec-registry = "0.1"
chaincodec-core     = "0.1"
```

```rust
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::MemoryRegistry;
use chaincodec_core::decoder::ChainDecoder;

// Load schemas
let registry = MemoryRegistry::new();
registry.load_directory("./schemas")?;

// Decode an event
let decoder = EvmDecoder::new();
let event = decoder.decode_event(&raw_log, &schema)?;
println!("{}: {:?}", event.schema, event.fields);
```

## Quick Start (TypeScript / Node.js)

```bash
npm install @chainkit/chaincodec
```

```typescript
import { EvmDecoder, MemoryRegistry } from '@chainkit/chaincodec';

const registry = new MemoryRegistry();
registry.loadDirectory('./node_modules/@chainkit/chaincodec/schemas');

const decoder = new EvmDecoder();
const event = decoder.decodeEvent({
  chain: 'ethereum',
  txHash: '0x...',
  blockNumber: 19000000,
  blockTimestamp: 1700000000,
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef',
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045',
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b',
  ],
  data: '0x0000000000000000000000000000000000000000000000000000000000989680',
}, registry);

console.log(event.schema);        // "ERC20Transfer"
console.log(event.fields.value);  // { type: "uint", value: 10000000 }
```

## Quick Start (Python)

```bash
pip install chaincodec
```

```python
from chaincodec import EvmDecoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_directory("./schemas")

decoder = EvmDecoder()
event = decoder.decode_event({
    "chain": "ethereum",
    "tx_hash": "0x...",
    "block_number": 19000000,
    "block_timestamp": 1700000000,
    "log_index": 0,
    "address": "0xa0b86991...",
    "topics": ["0xddf252ad...", "0x000...from", "0x000...to"],
    "data": "0x0000...value",
}, registry)

print(event["schema"])           # ERC20Transfer
print(event["fields"]["value"])  # 10000000
```

## Quick Start (WASM / Browser)

```javascript
import init, { EvmDecoder, MemoryRegistry } from '@chainkit/chaincodec-wasm';
import schemaCsdl from './schemas/erc20.csdl?raw';

await init();

const registry = new MemoryRegistry();
registry.loadCsdl(schemaCsdl);

const decoder = new EvmDecoder();
const eventJson = decoder.decodeEventJson(JSON.stringify(rawLog), registry);
const event = JSON.parse(eventJson);
```

---

## CLI

```bash
cargo install chaincodec-cli

# Parse and validate a schema
chaincodec parse --file schemas/tokens/erc20.csdl

# Decode a live event log
chaincodec decode-log \
  --topics 0xddf252ad... 0x000...from 0x000...to \
  --data 0x000...value \
  --schema-dir ./schemas \
  --chain ethereum

# Decode function call calldata
chaincodec decode-call \
  --calldata 0xa9059cbb000...to000...amount \
  --abi ./abis/erc20.json

# Encode a function call
chaincodec encode-call \
  --function transfer \
  --args '[{"type":"address","value":"0xd8dA..."},{"type":"uint","value":1000000}]' \
  --abi ./abis/erc20.json

# Detect proxy pattern
chaincodec detect-proxy --address 0xA0b86991...

# Fetch ABI from Sourcify/Etherscan
chaincodec fetch-abi --address 0xA0b86991... --chain-id 1

# List bundled schemas
chaincodec schemas list --dir ./schemas

# Benchmark decode throughput
chaincodec bench --schema ERC20Transfer --iterations 1000000
```

---

## CSDL Schema Format

ChainCodec uses **CSDL** (ChainCodec Schema Definition Language), a human-readable YAML format:

```yaml
schema ERC20Transfer:
  version: 1
  description: "ERC-20 standard Transfer event"
  chains: [ethereum, arbitrum, base, polygon, optimism]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true,  description: "Sender" }
    to:    { type: address, indexed: true,  description: "Recipient" }
    value: { type: uint256, indexed: false, description: "Amount" }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
---
schema ERC20Approval:
  version: 1
  # ... (multiple schemas per file supported with ---)
```

---

## Architecture

```
chainkit/
├── chaincodec/          # EVM ABI decoder — in development
│   ├── crates/
│   │   ├── chaincodec-core/        # Traits, types, NormalizedValue
│   │   ├── chaincodec-evm/         # EvmDecoder, EvmCallDecoder, EvmEncoder
│   │   │                           # EIP-712, Proxy detection
│   │   ├── chaincodec-registry/    # CSDL parser, MemoryRegistry, remote fetch
│   │   ├── chaincodec-batch/       # Rayon parallel decode + benchmarks
│   │   ├── chaincodec-stream/      # Real-time streaming engine
│   │   ├── chaincodec-observability/ # OpenTelemetry metrics + tracing
│   │   ├── chaincodec-solana/      # Solana decoder stub
│   │   └── chaincodec-cosmos/      # Cosmos decoder stub
│   ├── schemas/                    # 50+ bundled CSDL schemas
│   │   ├── tokens/   (erc20, erc721, erc1155, erc4626, weth)
│   │   ├── defi/     (uniswap-v2/v3, aave-v3, compound-v2/v3, curve, balancer, lido, ...)
│   │   ├── nft/      (opensea, blur)
│   │   ├── bridge/   (across, stargate)
│   │   └── governance/
│   ├── bindings/
│   │   ├── node/     # napi-rs → @chainkit/chaincodec (npm)
│   │   ├── python/   # PyO3 → chaincodec (PyPI)
│   │   ├── wasm/     # wasm-bindgen → @chainkit/chaincodec-wasm
│   │   ├── go/       # cgo bindings (planned)
│   │   └── java/     # JNI bindings (planned)
│   ├── registry-server/  # Schema registry HTTP server
│   ├── cli/          # chaincodec CLI binary
│   └── fixtures/     # Golden test fixtures
├── chainerrors/       # EVM error decoder
├── chainrpc/          # RPC transport with resilience
└── chainindex/        # Reorg-safe indexer
```

---

## Performance

| Operation | Throughput |
|-----------|-----------|
| Single-thread decode | >1M events/sec |
| Rayon 8-thread decode | >5M events/sec |
| Schema lookup (fingerprint) | O(1) HashMap |
| CSDL parse (per file) | <1ms |

Benchmarks run with `cargo bench --package chaincodec-batch`.

---

## Competitive Comparison

| Feature | chaincodec | viem | ethers.js | web3.py |
|---------|-----------|------|-----------|---------|
| Language | Rust + bindings | TS | TS/JS | Python |
| Multi-chain | ✅ | ❌ | ❌ | ❌ |
| Batch decode (parallel) | ✅ Rayon | ❌ | ❌ | ❌ |
| Schema registry | ✅ CSDL | ❌ | ❌ | ❌ |
| EIP-712 | ✅ | ✅ | ✅ | ✅ |
| Proxy detection | ✅ | Partial | Partial | ❌ |
| Auto ABI fetch | ✅ Sourcify+Etherscan | ❌ | ❌ | ❌ |
| WASM support | ✅ | ✅ | ❌ | ❌ |
| Python bindings | ✅ | ❌ | ❌ | N/A |
| Bundle size (WASM) | ~800KB | ~200KB | ~1.2MB | N/A |

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md). Each module has independent CI:

```bash
# Run all tests for chaincodec
cd chaincodec && cargo test --workspace

# Run benchmarks
cd chaincodec && cargo bench --package chaincodec-batch
```

---

## License

MIT — see [LICENSE](./LICENSE)

---

## Contact

Built by [@darshan_aqua](https://x.com/darshan_aqua) — questions, feedback, and contributions welcome.

---

## Roadmap

- **v0.1** (now): chaincodec production release — Rust + npm + Python + WASM
- **v0.2**: chainerrors next production release
- **v0.3**: chainrpc next production release with provider integrations
- **v0.4**: chainindex next production release with SQLite/Postgres
- **v1.0**: Full multi-chain support (Solana, Cosmos)
