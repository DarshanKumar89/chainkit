# chaincodec-cli

ChainCodec command-line tool — verify schemas, decode events, and run golden tests.

[![crates.io](https://img.shields.io/crates/v/chaincodec-cli)](https://crates.io/crates/chaincodec-cli)

## Install

```bash
cargo install chaincodec-cli
```

## Commands

### Decode a raw event

```bash
chaincodec decode \
  --schema schemas/erc20.csdl \
  --tx-hash 0xabc... \
  --topics 0xddf252ad... 0x000...from 0x000...to \
  --data 0x000...amount \
  --chain ethereum
```

### Verify a CSDL schema file

```bash
chaincodec verify schemas/erc20.csdl
```

### Run golden test fixtures

```bash
chaincodec test fixtures/evm/
```

### List registered schemas

```bash
chaincodec list --schema-dir schemas/
```

### Benchmark decode throughput

```bash
chaincodec bench --count 1000000 --schema schemas/erc20.csdl
```

## Output formats

All commands support `--output json` (default) or `--output table` for human-readable output.

## License

MIT — see [LICENSE](../LICENSE)
