# chaincodec-observability

OpenTelemetry metrics, distributed tracing, and structured logging for ChainCodec.

[![crates.io](https://img.shields.io/crates/v/chaincodec-observability)](https://crates.io/crates/chaincodec-observability)
[![docs.rs](https://docs.rs/chaincodec-observability/badge.svg)](https://docs.rs/chaincodec-observability)

## Features

- **Metrics** — decode throughput, error rate, schema hit/miss ratio (OTLP export)
- **Tracing** — distributed spans for decode pipelines (Jaeger / Tempo compatible)
- **Structured logging** — JSON log output with `tracing-subscriber`
- **Zero-overhead** when disabled — all instrumentation behind feature flags

## Usage

```toml
[dependencies]
chaincodec-observability = "0.1"
```

```rust
use chaincodec_observability::init_telemetry;

// Initialize OpenTelemetry + tracing subscriber
let _guard = init_telemetry("chaincodec", "http://localhost:4317")?;

// All ChainCodec operations are now automatically instrumented
let decoder = EvmDecoder::new();
let event = decoder.decode_event(&raw, &schema)?; // emits spans + metrics
```

## Exported metrics

| Metric | Type | Description |
|--------|------|-------------|
| `chaincodec.decode.events_total` | Counter | Total events decoded |
| `chaincodec.decode.errors_total` | Counter | Decode errors |
| `chaincodec.decode.duration_ms` | Histogram | Per-event decode latency |
| `chaincodec.registry.cache_hits` | Counter | Schema cache hits |

## License

MIT — see [LICENSE](../../LICENSE)
