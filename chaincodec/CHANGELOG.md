# Changelog

All notable changes to ChainCodec are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).
ChainCodec follows [Semantic Versioning](https://semver.org/).

Pre-1.0: minor versions may contain breaking changes (documented here).
Post-1.0: breaking changes only in major versions.

---

## [0.1.0] — 2024-01-xx — Initial Production Release

### Added

#### chaincodec-core

- `ChainDecoder` trait, `RawEvent`, `DecodedEvent`, `NormalizedValue` type system
- `DecodedCall` / `DecodedConstructor` — function call and constructor decoding output
- `ChainFamily`, `ChainId`, comprehensive error types, `SchemaRegistry` trait
- Full `serde` support throughout

#### chaincodec-evm

- `EvmDecoder` — event log decoding (alloy-rs); indexed topic handling for all types incl. reference types
- `EvmDecoder::decode_batch` / `decode_batch_parallel` — Rayon parallelism, >1M events/sec
- `EvmCallDecoder` — `decode_call` / `decode_constructor` using 4-byte selector matching
- `EvmEncoder` — bidirectional ABI encoding (`encode_call`, `encode_tuple`)
- `Eip712Parser` — EIP-712 typed structured data parsing and domain separator computation
- Proxy detection — EIP-1167 bytecode pattern, EIP-1967/EIP-1822 storage slot constants

#### chaincodec-registry

- `CsdlParser` with `parse_all()` — multi-document YAML (separated by `---`), field order via `IndexMap`
- `MemoryRegistry` — thread-safe, indexed by fingerprint (O(1)) and `(name, version)`
- `all_names()`, `all_schemas()`, version history, latest-non-deprecated resolution
- `AbiFetcher` (feature = `remote`) — Sourcify v1/v2, Etherscan, 4byte.directory

#### chaincodec-batch

- `BatchDecoder` with Rayon parallel engine
- Criterion benchmarks: sequential and parallel throughput (>1M / >5M events/sec)

#### chaincodec-stream

- `StreamEngine` with per-chain Tokio tasks, broadcast channel, `BlockListener` trait

#### chaincodec-observability

- OpenTelemetry metrics, structured tracing, OTLP export

#### chaincodec-cli (full production command set)

- `parse`, `decode-log`, `decode-call`, `encode-call`, `fetch-abi`, `detect-proxy`
- `verify`, `test`, `bench`
- `schemas list/search/validate`, `info`

#### Bundled schemas (50+ protocols across 5 categories)

- Tokens: ERC-20, ERC-721, ERC-1155, ERC-4626, WETH
- DEX: Uniswap V2, Uniswap V3, Curve, Balancer V2, Pendle
- Lending: Aave V3, Compound V2, Compound V3, Morpho Blue, MakerDAO
- Staking/Restaking: Lido, EigenLayer
- Perpetuals: GMX V1
- Oracles: Chainlink Price Feeds, Chainlink OCR2
- NFT: OpenSea Seaport, Blur
- Bridges: Across, Stargate
- Governance: Compound Governor Bravo

#### Language bindings

- TypeScript/Node.js (`@chainfoundry/chaincodec` npm) — 6 platform targets via napi-rs
- Python (`chaincodec` PyPI) — PyO3/maturin
- WASM (`@chainfoundry/chaincodec-wasm`) — wasm-bindgen

#### CI/CD

- `chaincodec.yml` — test matrix (Ubuntu, macOS), fmt + clippy, release build
- `publish.yml` — automated publish to crates.io + npm on `chaincodec-v*` tag
