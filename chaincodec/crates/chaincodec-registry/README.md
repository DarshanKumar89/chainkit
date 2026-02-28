# chaincodec-registry

Schema registry for ChainCodec — CSDL parser, in-memory store, and version management.

[![crates.io](https://img.shields.io/crates/v/chaincodec-registry)](https://crates.io/crates/chaincodec-registry)
[![docs.rs](https://docs.rs/chaincodec-registry/badge.svg)](https://docs.rs/chaincodec-registry)

## What is CSDL?

CSDL (Chain Schema Definition Language) is a human-readable YAML format for defining
blockchain event and function schemas. Example:

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum, arbitrum, base]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    standard: ERC-20
```

## Usage

```toml
[dependencies]
chaincodec-registry = "0.1"
```

```rust
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_core::schema::SchemaRegistry;

// Parse from string
let schemas = CsdlParser::parse_all(csdl_yaml)?;

// Or load from file / directory
let mut registry = MemoryRegistry::new();
registry.load_file("schemas/erc20.csdl")?;
registry.load_directory("schemas/")?;

// Lookup by fingerprint (topic0 hash)
let schema = registry.get_by_fingerprint(&fingerprint).unwrap();
```

## Bundled schemas

ChainCodec ships with 20+ ready-to-use CSDL schemas under `chaincodec/schemas/`:
ERC-20, ERC-721, ERC-1155, Uniswap V2/V3, Aave V3, Compound V3, and more.

## License

MIT — see [LICENSE](../../LICENSE)
