# Publishing chaincodec

This document explains how to publish chaincodec crates to crates.io and the
npm package `@chainkit/chaincodec` to the npm registry.

Publishing is fully automated via the `publish.yml` workflow in the chainkit
monorepo root and is triggered by pushing a version tag.

---

## Required GitHub Secrets

Set these in **Repository → Settings → Secrets and variables → Actions**:

| Secret | Where to get it | Purpose |
|--------|----------------|---------|
| `CARGO_REGISTRY_TOKEN` | <https://crates.io/settings/tokens> | Publish Rust crates |
| `NPM_TOKEN` | <https://www.npmjs.com/settings/~/tokens> — create an "Automation" token | Publish `@chainkit/chaincodec` |

---

## How to publish a release

1. **Ensure CI passes** on `main`:

   ```bash
   cd chaincodec && cargo test --workspace
   ```

2. **Update the version** in `Cargo.toml` (workspace root):

   ```toml
   [workspace.package]
   version = "0.1.1"   # bump here — all crates inherit this
   ```

   Update `CHANGELOG.md` to move items from `[Unreleased]` to the new version.

3. **Commit and push**:

   ```bash
   git add chaincodec/Cargo.toml chaincodec/CHANGELOG.md
   git commit -m "chore(chaincodec): release v0.1.1"
   git push
   ```

4. **Tag the release** — the tag name must match `chaincodec-v*`:

   ```bash
   git tag chaincodec-v0.1.1
   git push origin chaincodec-v0.1.1
   ```

   Pushing the tag triggers the `publish.yml` workflow automatically.

---

## What the workflow does

```
push tag chaincodec-v*
│
├── Job: publish-rust            (sequential, crates.io)
│     chaincodec-core
│     chaincodec-registry
│     chaincodec-evm
│     chaincodec-batch
│     chaincodec-stream
│     chaincodec-observability
│     chaincodec-cli
│
├── Job: build-node-bindings     (parallel matrix, 6 platforms)
│     linux-x64-gnu
│     linux-x64-musl
│     linux-arm64-gnu
│     macos-x64
│     macos-arm64
│     windows-x64
│
├── Job: publish-npm             (after all build-node-bindings complete)
│     collect *.node artifacts
│     npm publish @chainkit/chaincodec
│
└── Job: github-release          (after publish-rust + publish-npm)
      create GitHub release with install instructions
```

---

## Publishing Python bindings manually

The Python bindings use [maturin](https://github.com/PyO3/maturin).
Automated PyPI publishing is not yet wired into CI — build and publish manually:

```bash
cd chaincodec/bindings/python

# Build wheels for the current platform
pip install maturin
maturin build --release

# Publish to PyPI (requires TWINE_PASSWORD or MATURIN_PYPI_TOKEN env var)
maturin publish
```

For multi-platform Python wheels, use
[cibuildwheel](https://cibuildwheel.pypa.io/) or maturin's GitHub Actions
integration (see `maturin action` in the maturin docs).

---

## Publishing WASM bindings manually

```bash
cd chaincodec/bindings/wasm

# Requires wasm-pack
cargo install wasm-pack
wasm-pack build --release --target bundler --out-dir pkg

# Publish
cd pkg
npm publish --access public
```

---

## Dry-run (verify without publishing)

```bash
# Rust — check all crates would publish cleanly
cd chaincodec
cargo publish -p chaincodec-core --dry-run
cargo publish -p chaincodec-evm  --dry-run

# npm
cd chaincodec/bindings/node
npm pack   # creates a .tgz to inspect
```
