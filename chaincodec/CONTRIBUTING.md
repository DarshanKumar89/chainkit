# Contributing to ChainCodec

Thank you for contributing! This guide covers the development setup, code conventions, and pull request process.

## Setup

```bash
# Prerequisites
rustup install stable
rustup target add wasm32-unknown-unknown
cargo install cargo-watch cargo-criterion cargo-audit

# Clone and build
git clone https://github.com/DarshanKumar89/chainkit.git
cd chainkit/chaincodec
cargo build --workspace

# Run all tests
cargo test --workspace

# Watch for changes
cargo watch -x "test --workspace --lib"
```

## Repository Layout

| Path | Description |
| --- | --- |
| `crates/chaincodec-core/` | Core traits, types, errors — no chain-specific code |
| `crates/chaincodec-evm/` | EVM ABI decoder (alloy-rs + Rayon) |
| `crates/chaincodec-registry/` | CSDL parser, in-memory + file-backed registry |
| `crates/chaincodec-stream/` | Tokio-based streaming engine |
| `crates/chaincodec-batch/` | Rayon batch decode engine |
| `crates/chaincodec-observability/` | OpenTelemetry metrics + tracing |
| `cli/` | `chaincodec` CLI tool |
| `schemas/` | Built-in CSDL schema library |
| `fixtures/` | Golden test fixtures (real on-chain data) |
| `bindings/` | Language bindings (node, wasm, python, java, go) |

## Code Conventions

- **No `unsafe` blocks** without a detailed comment explaining why safety invariants hold
- **No `unwrap()`** in library code — use `?` or explicit error handling
- **Tests alongside the code** — each module has `#[cfg(test)]` tests
- **Golden fixtures** — every new schema must ship with at least one fixture JSON
- **CSDL fingerprints** — always verify fingerprints against on-chain data before submitting

## Adding a New Schema

1. Create or edit a `.csdl` file in `schemas/defi/` or `schemas/tokens/`
2. Verify the fingerprint with `chaincodec verify --schema <Name> --chain <slug> --tx <hash>`
3. Add a golden fixture in `fixtures/evm/<schema-name>.json`
4. Run `chaincodec test --fixtures fixtures/` — all fixtures must pass
5. Open a PR with a description of the protocol and a link to the contract

## Schema Trust Levels

| Level | Meaning |
| --- | --- |
| `unverified` | Submitted but not reviewed |
| `community_verified` | Reviewed by community members, golden test passes |
| `maintainer_verified` | Reviewed by core maintainers |
| `protocol_verified` | Signed by the protocol team's deployer key |

New submissions start at `unverified`. Maintainers upgrade the level after review.

## Adding a New Chain Decoder

1. Create a new crate `crates/chaincodec-<chain>/`
2. Implement `ChainDecoder` from `chaincodec-core`
3. Add your crate to `Cargo.toml` `[workspace.members]`
4. Write unit tests covering at minimum: fingerprint, decode_event, batch decode
5. Add golden fixtures for the top 5 event types on that chain
6. Add the chain to the CI matrix

## Pull Request Process

1. Fork the repo and create a branch: `git checkout -b feat/your-feature`
2. Write tests — PRs without tests are not merged
3. Run `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings`
4. Push and open a PR against `main`
5. Describe: what changed, why, and link any related issues

## Security

Report security vulnerabilities privately via X: [@darshan_aqua](https://x.com/darshan_aqua).
Do not open public GitHub issues for security bugs.
