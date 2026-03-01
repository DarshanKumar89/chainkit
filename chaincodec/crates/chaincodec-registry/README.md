# chaincodec-registry

Schema registry for ChainCodec — CSDL parser, in-memory store, and fingerprint-based lookup.

[![crates.io](https://img.shields.io/crates/v/chaincodec-registry)](https://crates.io/crates/chaincodec-registry)
[![docs.rs](https://docs.rs/chaincodec-registry/badge.svg)](https://docs.rs/chaincodec-registry)
[![license](https://img.shields.io/crates/l/chaincodec-registry)](LICENSE)

`chaincodec-registry` manages blockchain event schemas in the CSDL (Chain Schema Definition Language) format and provides fast O(1) fingerprint-based lookup. Load a directory of schemas once at startup, then resolve any `eth_getLogs` topic0 to a full schema instantly.

---

## Features

- **CSDL parser** — parse human-readable YAML schema definitions (single or multi-doc)
- **In-memory registry** — thread-safe `MemoryRegistry` indexed by fingerprint and name
- **Directory loading** — load an entire folder of `.csdl` files in one call
- **Fingerprint lookup** — O(1) schema resolution from topic0 hash during live decoding
- **Version management** — schemas carry a `version` field; multiple versions coexist safely
- **50+ bundled schemas** — ERC-20/721/1155, Uniswap, Aave, Compound, ChainLink, and more

---

## Installation

```toml
[dependencies]
chaincodec-registry = "0.1"
chaincodec-core     = "0.1"
```

---

## CSDL schema format

CSDL (Chain Schema Definition Language) is a concise YAML format:

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum, arbitrum, base, polygon, optimism]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:   { type: address, indexed: true  }
    to:     { type: address, indexed: true  }
    value:  { type: uint256, indexed: false }
  meta:
    standard: ERC-20
    description: "ERC-20 token transfer"
```

One file can hold multiple schemas separated by `---`:

```yaml
schema ERC20Transfer:
  # ...
---
schema ERC20Approval:
  version: 1
  event: Approval
  fingerprint: "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
  fields:
    owner:   { type: address, indexed: true  }
    spender: { type: address, indexed: true  }
    value:   { type: uint256, indexed: false }
```

### Supported field types

| CSDL type | Solidity type | NormalizedValue variant |
|-----------|--------------|------------------------|
| `address` | `address` | `Address(String)` |
| `uint256` | `uint256` | `Uint256(u128)` |
| `uint128` | `uint128` | `Uint256(u128)` |
| `int256` | `int256` | `Int256(i128)` |
| `bool` | `bool` | `Bool(bool)` |
| `bytes32` | `bytes32` | `Bytes(Vec<u8>)` |
| `bytes` | `bytes` | `BytesDynamic(Vec<u8>)` |
| `string` | `string` | `String(String)` |
| `address[]` | `address[]` | `Array(Vec<NormalizedValue>)` |
| `tuple` | struct | `Tuple(Vec<NormalizedValue>)` |

---

## Quick start

```rust
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_core::schema::SchemaRegistry;

// Option A: parse from a YAML string
let schemas = CsdlParser::parse_all(include_str!("schemas/erc20.csdl"))?;
println!("parsed {} schemas", schemas.len());

// Option B: load a single file
let mut registry = MemoryRegistry::new();
registry.load_file("schemas/erc20.csdl")?;

// Option C: load all .csdl files in a directory (recursive)
registry.load_directory("schemas/")?;
println!("loaded {} schemas", registry.len());
```

---

## Looking up schemas

```rust
// By fingerprint — used during live log decoding (O(1))
let fp = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
if let Some(schema) = registry.get_by_fingerprint(fp) {
    println!("schema: {} v{}", schema.name, schema.version);
    for (field_name, field_def) in &schema.fields {
        println!("  {} — type: {}, indexed: {}", field_name, field_def.ty, field_def.indexed);
    }
}

// By name
if let Some(schema) = registry.get_by_name("ERC20Transfer") {
    println!("chains: {:?}", schema.chains);
}

// Iterate all schemas
for schema in registry.all_schemas() {
    println!("{} ({})", schema.name, schema.fingerprint);
}
```

---

## Registering schemas programmatically

```rust
use chaincodec_core::schema::{Schema, FieldDef};
use indexmap::IndexMap;

let mut fields = IndexMap::new();
fields.insert("from".to_string(), FieldDef { ty: "address".to_string(), indexed: true });
fields.insert("to".to_string(),   FieldDef { ty: "address".to_string(), indexed: true });
fields.insert("value".to_string(), FieldDef { ty: "uint256".to_string(), indexed: false });

let schema = Schema {
    name: "MyTransfer".to_string(),
    version: 1,
    fingerprint: "0x...".to_string(),
    chains: vec!["ethereum".to_string()],
    event: "Transfer".to_string(),
    fields,
    meta: Default::default(),
};

registry.register(schema)?;
```

---

## Bundled schemas (50+)

Load all schemas from the `chaincodec/schemas/` directory:

```rust
registry.load_directory("path/to/chaincodec/schemas/")?;
```

| Category | Schemas |
|----------|---------|
| Token standards | ERC-20, ERC-721, ERC-1155, ERC-4337, WETH |
| DEXes | Uniswap V2, Uniswap V3, Curve, Balancer, SushiSwap, DODO |
| Lending | Aave V2, Aave V3, Compound V2, Compound V3, MakerDAO |
| Oracles | ChainLink Aggregator, ChainLink VRF, ChainLink CCIP |
| Liquid staking | Lido, Rocket Pool, Stader, Frax |
| Yield | Yearn, Convex, Synthetix V3 |
| Derivatives | GMX V1, dYdX V4, Perpetual Protocol |
| Bridges | Wormhole, LayerZero V2, Hop, Celer |
| Other | ENS, Safe Multisig, CryptoPunks, BAYC, Bancor V3 |

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| [chaincodec-core](https://crates.io/crates/chaincodec-core) | Traits, types, primitives |
| [chaincodec-evm](https://crates.io/crates/chaincodec-evm) | EVM ABI event & call decoder |
| **chaincodec-registry** | CSDL schema registry (this crate) |
| [chaincodec-batch](https://crates.io/crates/chaincodec-batch) | Rayon parallel batch decode |
| [chaincodec-stream](https://crates.io/crates/chaincodec-stream) | Live WebSocket event streaming |

---

## License

MIT — see [LICENSE](../../LICENSE)
