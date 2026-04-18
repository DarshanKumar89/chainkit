# Changelog

All notable repo-wide changes to ChainFoundry are documented here. Module-level changes live in each module's own `CHANGELOG.md` (e.g. [chaincodec/CHANGELOG.md](chaincodec/CHANGELOG.md)).

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Released — 2026-04-18 — `chainrpc-v0.2.1` (retagged)

Ships PR [#3](https://github.com/DarshanKumar89/chainfoundry/pull/3) (Chainstack provider) to crates.io, npm, and PyPI.

**First attempt failed silently** — tag `chainrpc-v0.2.1` was pushed without bumping the workspace `version` field, so every registry rejected the upload as a duplicate of `0.2.0` ([run 24609136632](https://github.com/DarshanKumar89/chainfoundry/actions/runs/24609136632)). Deleted the tag + GitHub Release, bumped versions, re-tagged.

- Version bumps:
  - [chainrpc/Cargo.toml](chainrpc/Cargo.toml) workspace `version = "0.2.1"` (propagates to `chainrpc-core`, `chainrpc-http`, `chainrpc-ws`, `chainrpc-providers` via `version.workspace = true`)
  - [chainrpc/bindings/node/package.json](chainrpc/bindings/node/package.json) `"version": "0.2.1"`
  - [chainrpc/bindings/python/pyproject.toml](chainrpc/bindings/python/pyproject.toml) `version = "0.2.1"`
- Contents: adds `chainstack.rs` to [chainrpc/crates/chainrpc-providers/](chainrpc/crates/chainrpc-providers/) — 5th RPC provider alongside Alchemy, Infura, QuickNode, public. 30 EVM networks, Global + Dedicated/Trader endpoint styles, plan-tier RPS constants (Developer 25 / Growth 250 / Pro 400 / Business 600), 7 unit tests.
- Author: external contributor akegaviar
- Gap fixed: PR #3 merged 2026-04-06 but `chainrpc-v0.2.0` was cut on 2026-03-08, so Chainstack shipped 12 days late.

### Known issue — Python musl wheel build fails on Python 3.13

[Build Python wheel · linux-x64-musl] fails with `the configured Python interpreter version (3.13) is newer than PyO3's maximum supported version (3.12)`. `pyo3 v0.21.2` caps at Python 3.12; the musl runner now ships 3.13. Fix requires bumping `pyo3` to ≥0.22 or pinning Python to 3.12 in the workflow. Unrelated to the version-bump issue — will bite again until fixed.

### Changed — 2026-04-18 — Repo renamed `chainkit` → `chainfoundry`

Brought the GitHub repo name in line with the package-registry identity (npm `@chainfoundry/*`, PyPI `chainfoundry-*`, Maven `io.chainfoundry`) which was already `chainfoundry`.

**New URL:** https://github.com/DarshanKumar89/chainfoundry

**Files touched (38 total, 76 insertions / 76 deletions):**

- **Git remote**
  - `origin` → `https://github.com/DarshanKumar89/chainfoundry.git`

- **Go module paths (breaking for Go consumers)**
  - [chaincodec/bindings/go/go.mod](chaincodec/bindings/go/go.mod) — `github.com/DarshanKumar89/chainfoundry/chaincodec`
  - [chainrpc/bindings/go/go.mod](chainrpc/bindings/go/go.mod) — `github.com/DarshanKumar89/chainfoundry/chainrpc`
  - [chainerrors/bindings/go/go.mod](chainerrors/bindings/go/go.mod) — `github.com/DarshanKumar89/chainfoundry/chainerrors`
  - [chainindex/bindings/go/go.mod](chainindex/bindings/go/go.mod) — `github.com/DarshanKumar89/chainfoundry/chainindex`

- **Cargo.toml `repository` / `homepage` / `authors`**
  - [chaincodec/Cargo.toml](chaincodec/Cargo.toml)
  - [chainrpc/Cargo.toml](chainrpc/Cargo.toml)
  - [chainerrors/Cargo.toml](chainerrors/Cargo.toml)
  - [chainindex/Cargo.toml](chainindex/Cargo.toml)

- **Binding package metadata**
  - npm `package.json` (×5): chaincodec/node, chaincodec/wasm, chainrpc/node, chainerrors/node, chainindex/node
  - PyPI `pyproject.toml` (×4): chaincodec, chainrpc, chainerrors, chainindex
  - Maven `pom.xml` (×4): chaincodec, chainrpc, chainerrors, chainindex

- **Source code**
  - [chaincodec/crates/chaincodec-registry/src/remote.rs:108](chaincodec/crates/chaincodec-registry/src/remote.rs#L108) — HTTP user-agent URL

- **Docs & config**
  - [README.md](README.md), [CONTRIBUTING.md](CONTRIBUTING.md), [llms.txt](llms.txt), [test-all.sh](test-all.sh)
  - Per-module: [chaincodec/PUBLISHING.md](chaincodec/PUBLISHING.md), [chaincodec/CONTRIBUTING.md](chaincodec/CONTRIBUTING.md), [chaincodec/cli/README.md](chaincodec/cli/README.md), [chaincodec/bindings/node/README.md](chaincodec/bindings/node/README.md), [chaincodec/bindings/python/README.md](chaincodec/bindings/python/README.md), [chaincodec/bindings/wasm/README.md](chaincodec/bindings/wasm/README.md), [chaincodec/docs/architecture.md](chaincodec/docs/architecture.md), [chaincodec/docs/chaincodec-explain.md](chaincodec/docs/chaincodec-explain.md), [chaincodec/docs/getting-started.md](chaincodec/docs/getting-started.md)
  - [chainrpc/README.md](chainrpc/README.md), [chainrpc/docs/IMPLEMENTATION.md](chainrpc/docs/IMPLEMENTATION.md)
  - [chainerrors/README.md](chainerrors/README.md)

**Unchanged (deliberate):**

- Local filesystem directory remains `/Users/darshankumar/Daemongodwiz/personal-proj/chainkit/` — renaming would invalidate Claude memory paths.
- Already-published artifacts on crates.io, npm (`@chainfoundry/*`), PyPI (`chainfoundry-*`), Maven (`io.chainfoundry`) — immutable once published; registry names were already `chainfoundry`.
- Crate names on crates.io (`chaincodec`, `chainrpc`, `chainerrors`, `chainindex`) — never contained `chainkit`.
- CI workflow file names (per-module: `chaincodec.yml`, etc.) — no repo-name references inside.

**Migration notes for consumers:**

- **Rust / npm / PyPI / Maven users:** no action required. Package names are unchanged.
- **Go users:** update imports to `github.com/DarshanKumar89/chainfoundry/<module>`. GitHub redirects handle `git clone` but `go get` resolution through module proxies may fail until the cache expires.
- **Clone URLs in scripts:** GitHub 301-redirects indefinitely; no action strictly required.
