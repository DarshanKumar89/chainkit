# Publishing chaincodec

Publishing is fully automated via `.github/workflows/publish.yml` — push a version
tag to trigger builds and publishing to all four platforms simultaneously.

---

## Required Secrets

Set in **Repository → Settings → Secrets → Actions**:

| Secret | Source | Used for |
|--------|--------|----------|
| `CARGO_REGISTRY_TOKEN` | https://crates.io/settings/tokens | Rust crates → crates.io |
| `NPM_TOKEN` | https://npmjs.com → Account → Access Tokens → "Automation" | npm packages |

**PyPI** uses **OIDC trusted publishing** — no token needed. Configure once at
https://pypi.org/manage/account/publishing/ with:
- Owner: `DarshanKumar89`
- Repo: `chainkit`
- Workflow: `publish.yml`
- Environment: `pypi`

---

## Release Process (One Command)

```bash
# 1. Bump version everywhere
NEW_VERSION="0.2.0"
sed -i '' "s/^version = .*/version = \"$NEW_VERSION\"/" \
    chaincodec/Cargo.toml \
    chaincodec/bindings/node/package.json \
    chaincodec/bindings/python/pyproject.toml

# 2. Update CHANGELOG.md — move [Unreleased] items to new version section

# 3. Commit
git add chaincodec/
git commit -m "chore(chaincodec): release v$NEW_VERSION"
git push

# 4. Tag — this triggers the full publish pipeline
git tag chaincodec-v$NEW_VERSION
git push origin chaincodec-v$NEW_VERSION
```

The tag push triggers **11 parallel/sequential jobs** automatically.

---

## What the Workflow Does

```
push tag  chaincodec-v*
│
├── publish-rust             [ubuntu]  — crates.io in dependency order
│     chaincodec-core
│     chaincodec-registry + chaincodec-evm
│     chaincodec-batch + chaincodec-stream + chaincodec-observability
│     chaincodec-cli
│
├── build-node-bindings      [matrix, 6 platforms]  — native .node files
│     linux-x64-gnu  linux-x64-musl  linux-arm64-gnu
│     macos-x64  macos-arm64  windows-x64
│
├── publish-npm              [ubuntu, after build-node-bindings]
│     collect *.node artifacts → npm publish @chainfoundry/chaincodec
│
├── build-python-wheels      [matrix, 6 platforms]  — manylinux/musl/macOS/win
│     linux-x64  linux-arm64  linux-x64-musl
│     macos-x64  macos-arm64  windows-x64
│
├── publish-pypi-chaincodec  [ubuntu, after build-python-wheels]
│     pypa/gh-action-pypi-publish → PyPI (OIDC, no token)
│
├── build-wasm               [ubuntu]  — three wasm-pack targets
│     web (ESM)  nodejs (CJS)  bundler (webpack/vite)
│
├── publish-wasm-npm         [ubuntu, after build-wasm]
│     @chainfoundry/chaincodec-wasm      → npm
│     @chainfoundry/chaincodec-wasm-node → npm
│
└── github-release           [ubuntu, after publish-rust + publish-npm]
      softprops/action-gh-release → GitHub Release with install instructions
```

---

## Pre-publish Verification (Run Locally First)

```bash
cd chaincodec

# 1. Rust — full test suite
cargo test --workspace

# 2. Rust — dry-run publish (catches missing fields, broken deps)
cargo publish -p chaincodec-core      --dry-run --allow-dirty
cargo publish -p chaincodec-evm       --dry-run --allow-dirty
cargo publish -p chaincodec-registry  --dry-run --allow-dirty

# 3. Node — inspect the npm package tarball
cd bindings/node
npm install
npm pack
ls -lh *.tgz
tar tzf *.tgz | head -20   # verify index.js, index.d.ts, *.node are included

# 4. Python — build local wheel and test import
cd ../python
pip install maturin
maturin develop --features extension-module  # installs into current venv
python -c "import chaincodec; print(chaincodec.__version__)"

# 5. WASM — local build test
cd ../wasm
cargo install wasm-pack
wasm-pack build --release --target web --out-dir pkg-web
ls pkg-web/                # verify chaincodec_wasm.js + .wasm exist

# 6. Dry-run PyPI upload
pip install twine
maturin build --release --features extension-module
twine check dist/*.whl
```

---

## Manual Publishing (If CI Fails)

### Rust → crates.io
```bash
export CARGO_REGISTRY_TOKEN="..."
cd chaincodec
cargo publish -p chaincodec-core
sleep 30
cargo publish -p chaincodec-registry
cargo publish -p chaincodec-evm
sleep 30
cargo publish -p chaincodec-batch
cargo publish -p chaincodec-stream
cargo publish -p chaincodec-observability
sleep 30
cargo publish -p chaincodec-cli
```

### npm (Node.js)
```bash
# Build for current platform only (for testing)
cd chaincodec/bindings/node
npm install
npx napi build --platform --release
npm publish --access public
```

### PyPI (Python)
```bash
cd chaincodec/bindings/python
pip install maturin
# Linux: use maturin-action Docker for manylinux wheels
maturin publish --features extension-module
# Or build + upload separately:
maturin build --release --features extension-module
pip install twine
twine upload dist/*.whl
```

### npm (WASM — two packages)
```bash
cd chaincodec/bindings/wasm
wasm-pack build --release --target web --out-dir pkg-web

# Patch package.json
node -e "
  const p = require('./pkg-web/package.json');
  p.name = '@chainfoundry/chaincodec-wasm';
  require('fs').writeFileSync('./pkg-web/package.json', JSON.stringify(p, null, 2));
"
cd pkg-web && npm publish --access public
```

---

## Package Names & URLs

| Platform | Package | URL |
|----------|---------|-----|
| crates.io | `chaincodec-core` | https://crates.io/crates/chaincodec-core |
| crates.io | `chaincodec-evm` | https://crates.io/crates/chaincodec-evm |
| crates.io | `chaincodec-registry` | https://crates.io/crates/chaincodec-registry |
| crates.io | `chaincodec-batch` | https://crates.io/crates/chaincodec-batch |
| crates.io | `chaincodec-stream` | https://crates.io/crates/chaincodec-stream |
| crates.io | `chaincodec-observability` | https://crates.io/crates/chaincodec-observability |
| crates.io | `chaincodec-cli` | https://crates.io/crates/chaincodec-cli |
| npm | `@chainfoundry/chaincodec` | https://npmjs.com/package/@chainfoundry/chaincodec |
| PyPI | `chaincodec` | https://pypi.org/project/chaincodec/ |
| npm (WASM) | `@chainfoundry/chaincodec-wasm` | https://npmjs.com/package/@chainfoundry/chaincodec-wasm |
| npm (WASM Node) | `@chainfoundry/chaincodec-wasm-node` | https://npmjs.com/package/@chainfoundry/chaincodec-wasm-node |
