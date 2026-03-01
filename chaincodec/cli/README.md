# chaincodec-cli

ChainCodec command-line tool — decode EVM events, verify CSDL schemas, run golden tests, and benchmark decode throughput.

[![crates.io](https://img.shields.io/crates/v/chaincodec-cli)](https://crates.io/crates/chaincodec-cli)
[![license](https://img.shields.io/crates/l/chaincodec-cli)](LICENSE)

---

## Install

```bash
cargo install chaincodec-cli
```

Or build from source:

```bash
git clone https://github.com/DarshanKumar89/chainkit
cd chainkit/chaincodec
cargo build --release -p chaincodec-cli
# binary: target/release/chaincodec
```

---

## Commands

### `decode` — decode a raw EVM log

Decode a raw log entry into a human-readable event using a CSDL schema:

```bash
chaincodec decode \
  --schema schemas/erc20.csdl \
  --chain ethereum \
  --address 0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48 \
  --topics \
    0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef \
    0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045 \
    0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b \
  --data 0x00000000000000000000000000000000000000000000000000000000000f4240
```

**JSON output (default):**
```json
{
  "schema": "ERC20Transfer",
  "chain": "ethereum",
  "fields": {
    "from":  "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
    "to":    "0xab5801a7d398351b8be11c439e05c5b3259aec9b",
    "value": "1000000"
  }
}
```

**Table output (`--output table`):**
```
Schema : ERC20Transfer
Chain  : ethereum

Field   Value
------  ------------------------------------------
from    0xd8da6bf26964af9d7eed9e03e53415d37aa96045
to      0xab5801a7d398351b8be11c439e05c5b3259aec9b
value   1000000
```

---

### `verify` — validate a CSDL schema file

Check that a CSDL file is syntactically valid and that the fingerprint matches the event signature:

```bash
chaincodec verify schemas/erc20.csdl
# ✓ ERC20Transfer v1 — fingerprint matches
# ✓ ERC20Approval v1 — fingerprint matches

chaincodec verify schemas/      # verify an entire directory
```

---

### `list` — list all registered schemas

Show all schemas loaded from a CSDL directory:

```bash
chaincodec list --schema-dir schemas/
```

```
NAME                    VERSION  EVENT      FINGERPRINT
ERC20Transfer           1        Transfer   0xddf252ad...
ERC20Approval           1        Approval   0x8c5be1e5...
UniswapV3Swap           1        Swap       0xc42079f9...
AaveV3Supply            1        Supply     0x2b627736...
...
53 schemas loaded
```

---

### `test` — run golden fixture tests

Run a directory of golden JSON fixtures through the decoder and assert the output matches:

```bash
chaincodec test fixtures/evm/
```

```
running 30 golden tests
✓ erc20-transfer
✓ erc20-approval
✓ uniswap-v3-swap
✓ aave-v3-supply
...
30 passed, 0 failed
```

**Golden fixture format** (`fixtures/evm/erc20-transfer.json`):

```json
{
  "description": "ERC-20 USDC Transfer — 1 USDC",
  "schema_file": "schemas/erc20.csdl",
  "raw": {
    "chain": "ethereum",
    "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
    "topics": [
      "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
      "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045",
      "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b"
    ],
    "data": "0x00000000000000000000000000000000000000000000000000000000000f4240"
  },
  "expected": {
    "schema": "ERC20Transfer",
    "fields": {
      "from":  "0xd8da6bf26964af9d7eed9e03e53415d37aa96045",
      "to":    "0xab5801a7d398351b8be11c439e05c5b3259aec9b",
      "value": "1000000"
    }
  }
}
```

---

### `bench` — benchmark decode throughput

Measure events/sec for a given schema and thread count:

```bash
chaincodec bench \
  --schema schemas/erc20.csdl \
  --count 1000000 \
  --threads 8
```

```
Benchmarking ERC20Transfer x 1,000,000 events

  Single-thread : 1,024,311 events/sec
  8-thread Rayon: 6,187,442 events/sec
  Speedup       : 6.04x

Latency (single-thread)
  P50  : 0.82 µs
  P95  : 1.21 µs
  P99  : 2.14 µs
```

---

## Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `--output` | `json` | Output format: `json` or `table` |
| `--schema-dir` | `schemas/` | Directory with CSDL schemas |
| `--chain` | `ethereum` | Chain slug for context |
| `--log-level` | `warn` | Log level: `trace`, `debug`, `info`, `warn`, `error` |

---

## License

MIT — see [LICENSE](../LICENSE)
