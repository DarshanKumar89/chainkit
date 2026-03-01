# chaincodec-observability

OpenTelemetry metrics, distributed tracing, and structured logging for ChainCodec.

[![crates.io](https://img.shields.io/crates/v/chaincodec-observability)](https://crates.io/crates/chaincodec-observability)
[![docs.rs](https://docs.rs/chaincodec-observability/badge.svg)](https://docs.rs/chaincodec-observability)
[![license](https://img.shields.io/crates/l/chaincodec-observability)](LICENSE)

`chaincodec-observability` wires up OpenTelemetry metrics and `tracing` spans so you can monitor decode throughput, error rates, and latency from any OTLP-compatible backend — Prometheus, Grafana Tempo, Jaeger, Datadog, or the OpenTelemetry Collector.

---

## Features

- **Metrics** — decode throughput, error rate, batch size, schema cache hit ratio via OTLP counters + histograms
- **Tracing** — distributed spans for decode pipelines, compatible with Jaeger, Tempo, and Zipkin
- **Structured logging** — JSON log output via `tracing-subscriber` with field-level context
- **OTLP export** — pushes telemetry to any OpenTelemetry Collector over gRPC
- **Prometheus compatible** — pair with `opentelemetry-prometheus` for pull-based metrics scraping
- **Low overhead** — metrics are batched; tracing supports configurable sampling rates

---

## Installation

```toml
[dependencies]
chaincodec-observability = "0.1"
```

---

## Quick start

```rust
use chaincodec_observability::{init_telemetry, ChainCodecMetrics};
use opentelemetry::global;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize OpenTelemetry + tracing subscriber in one call.
    // Exports spans and metrics to an OTLP collector (gRPC).
    let _guard = init_telemetry(
        "chaincodec-service",          // service.name in your APM
        "http://localhost:4317",       // OTLP gRPC endpoint
    )?;

    // Create a metrics handle from the global meter
    let meter = global::meter("chaincodec");
    let metrics = ChainCodecMetrics::new(&meter);

    // Record after each decode operation
    metrics.record_decoded("ethereum", "ERC20Transfer");
    metrics.record_latency(0.42, "ethereum");          // 0.42 ms per event
    metrics.record_error("ethereum", "schema_not_found");

    Ok(())
}
```

---

## Exported metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `chaincodec.events_decoded` | Counter | `chain`, `schema` | Events successfully decoded |
| `chaincodec.events_skipped` | Counter | `chain` | Events with no schema match |
| `chaincodec.decode_errors` | Counter | `chain`, `error_type` | Decode failures by type |
| `chaincodec.decode_latency_ms` | Histogram | `chain` | Per-event decode time (ms) |
| `chaincodec.batch_size` | Histogram | — | Events per batch request |
| `chaincodec.schema_cache_hits` | Counter | — | Schema fingerprint cache hits |

---

## Structured logging

```rust
// JSON logs — for production log aggregators (Loki, Datadog, CloudWatch)
tracing_subscriber::fmt()
    .json()
    .with_env_filter("chaincodec=info,warn")
    .init();

// Human-readable — for local development
tracing_subscriber::fmt()
    .with_env_filter("chaincodec=debug")
    .init();
```

Set log level via environment variable:

```bash
RUST_LOG=chaincodec=debug,info ./my-service
```

---

## Prometheus integration

Pair with `opentelemetry-prometheus` for pull-based metrics:

```toml
[dependencies]
chaincodec-observability  = "0.1"
opentelemetry-prometheus  = "0.16"
prometheus                = "0.13"
axum                      = "0.7"
```

```rust
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::Registry;

let prom_registry = Registry::new();
let _exporter = opentelemetry_prometheus::exporter()
    .with_registry(prom_registry.clone())
    .build()?;

// Expose /metrics endpoint (Axum)
let app = axum::Router::new().route(
    "/metrics",
    axum::routing::get(move || {
        let reg = prom_registry.clone();
        async move {
            use prometheus::Encoder;
            let mut buf = Vec::new();
            prometheus::TextEncoder::new().encode(&reg.gather(), &mut buf).unwrap();
            String::from_utf8(buf).unwrap()
        }
    }),
);
```

---

## Distributed tracing

When instrumented, every decode operation emits a span visible in Jaeger / Grafana Tempo:

```
chaincodec.decode_event
  chain: "ethereum"
  schema: "ERC20Transfer"
  block_number: 19500000
  duration: 0.42ms
```

Configure sampling in the OTLP exporter to control overhead on high-throughput paths.

---

## Ecosystem

| Crate | Purpose |
|-------|---------|
| [chaincodec-core](https://crates.io/crates/chaincodec-core) | Traits, types, primitives |
| [chaincodec-evm](https://crates.io/crates/chaincodec-evm) | EVM ABI event & call decoder |
| [chaincodec-batch](https://crates.io/crates/chaincodec-batch) | Rayon parallel batch decode |
| [chaincodec-stream](https://crates.io/crates/chaincodec-stream) | Live WebSocket event streaming |
| **chaincodec-observability** | OpenTelemetry metrics & tracing (this crate) |

---

## License

MIT — see [LICENSE](../../LICENSE)
