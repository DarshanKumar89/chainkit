# Contributing to ChainKit

Thank you for contributing! ChainKit is a monorepo of four independent Rust
modules. This guide covers setup, conventions, and the pull request process
that applies across all modules.

---

## Repository layout

```
chainkit/
├── chaincodec/     # Universal ABI decoder (EVM events, calls, EIP-712)
├── chainerrors/    # EVM revert / panic / custom error decoder
├── chainrpc/       # Resilient RPC transport with circuit breaker
├── chainindex/     # Reorg-safe blockchain indexer
└── .github/
    └── workflows/  # Per-module CI + shared publish workflow
```

Each module is a **standalone Cargo workspace**. You can work on one without
touching the others.

---

## Setup

```bash
# Prerequisites
rustup install stable
rustup target add wasm32-unknown-unknown
cargo install cargo-watch cargo-criterion cargo-audit

# Clone
git clone https://github.com/DarshanKumar89/chainkit.git
cd chainkit

# Build a specific module
cd chaincodec && cargo build --workspace

# Run all tests for a module
cd chaincodec && cargo test --workspace

# Watch for changes
cargo watch -x "test --workspace --lib"
```

---

## Code conventions

- **No `unsafe` blocks** without a comment explaining why the invariants hold
- **No `unwrap()`** in library code — use `?` or explicit error handling
- **Tests alongside code** — each module has `#[cfg(test)]` unit tests
- **Golden fixtures** — every new schema must ship with at least one fixture JSON
- Format and lint before opening a PR:
  ```bash
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  ```

---

## Adding a schema (chaincodec)

1. Create or edit a `.csdl` file under `chaincodec/schemas/`
2. Verify the fingerprint matches the on-chain event signature
3. Add a golden fixture in `chaincodec/fixtures/evm/<schema-name>.json`
4. Run `chaincodec test --fixtures chaincodec/fixtures/` — all must pass
5. Open a PR with the protocol name and a link to the contract

---

## Pull request process

1. Fork and create a branch: `git checkout -b feat/your-feature`
2. Write tests — PRs without tests are not merged
3. Run format + clippy (see above)
4. Push and open a PR against `main`
5. Describe: what changed, why, and link any related issues

---

## Security

Report vulnerabilities privately via X: [@darshan_aqua](https://x.com/darshan_aqua).
Do not open public issues for security bugs.

---

## License

MIT — see [LICENSE](./LICENSE)
