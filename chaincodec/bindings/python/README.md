# chaincodec (Python)

Universal blockchain ABI decoder — production-grade EVM event log and function
call decoding, with 50+ built-in DeFi/NFT/bridge protocol schemas.

[![PyPI](https://img.shields.io/pypi/v/chaincodec)](https://pypi.org/project/chaincodec/)
[![Python](https://img.shields.io/pypi/pyversions/chaincodec)](https://pypi.org/project/chaincodec/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## Install

```bash
pip install chaincodec
```

Prebuilt wheels for **Linux** (x64, arm64, musl), **macOS** (x64, arm64), and **Windows** (x64).
No Rust toolchain required.

## Quick Start

```python
import asyncio
from chaincodec import EvmDecoder, MemoryRegistry

async def main():
    # Load a schema
    registry = MemoryRegistry()
    registry.load_file("schemas/erc20.csdl")

    decoder = EvmDecoder()

    # Decode an ERC-20 Transfer event
    raw_log = {
        "address": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
        "topics": [
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045",
            "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b",
        ],
        "data": "0x000000000000000000000000000000000000000000000000000000003b9aca00",
    }

    fingerprint = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
    schema = registry.get_by_fingerprint(fingerprint)

    event = decoder.decode_event(raw_log, schema)
    print(event)
    # DecodedEvent(schema="ERC20Transfer", fields={"from": "0xd8dA...", "to": "0xAb58...", "value": 1000000000})

asyncio.run(main())
```

## Decode Function Calls

```python
from chaincodec import EvmCallDecoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_file("schemas/uniswap-v3.csdl")

decoder = EvmCallDecoder()
call_data = bytes.fromhex("414bf389...")  # exactInputSingle calldata

result = decoder.decode_call(call_data, registry)
print(result.function_name)  # "exactInputSingle"
print(result.inputs)         # [("tokenIn", "0x..."), ("amountIn", 1000000)]
```

## Encode Function Calls

```python
from chaincodec import EvmEncoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_file("schemas/erc20.csdl")

encoder = EvmEncoder()
calldata = encoder.encode_call("transfer", ["0xRecipient...", 1_000_000], registry)
# b'\xa9\x05\x9c\xbb...'
```

## Batch Decode

```python
from chaincodec import EvmDecoder, MemoryRegistry

registry = MemoryRegistry()
registry.load_directory("schemas/")   # loads all 50+ schemas

decoder = EvmDecoder()
events = decoder.decode_batch(raw_logs, registry)
# Parallel decode via Rayon — >1M events/sec
```

## Built-in Schemas (53 protocols)

| Category | Protocols |
|----------|-----------|
| **Tokens** | ERC-20, ERC-721, ERC-1155, WETH, ERC-4626, ENS, ERC-4337 |
| **DEX** | Uniswap V2/V3, SushiSwap, Curve, Balancer V2, Camelot, DODO, Bancor V3 |
| **Lending** | Aave V2/V3, Compound V2/V3, Morpho Blue, Spark |
| **Derivatives** | GMX, GMX V1, dYdX V4, Perpetual Protocol, Synthetix V3, Ribbon Finance |
| **LSD/Staking** | Lido, Rocket Pool, Frax Ether, Stader, EigenLayer, Convex, Yearn |
| **Bridges** | Wormhole, LayerZero V2, Hop, Celer, Stargate, Across |
| **NFT** | OpenSea, Blur, CryptoPunks, BAYC, LooksRare |
| **Governance** | Safe Multisig, Compound Governance |
| **Oracle** | Chainlink (Aggregator, VRF, CCIP) |
| **Other** | Pendle Finance, Maker DAO |

## Links

- **GitHub**: https://github.com/DarshanKumar89/chainkit
- **Rust crates**: https://crates.io/crates/chaincodec-core
- **npm (Node.js)**: https://www.npmjs.com/package/@chainfoundry/chaincodec
