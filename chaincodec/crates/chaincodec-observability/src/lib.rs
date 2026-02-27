//! # chaincodec-observability
//!
//! OpenTelemetry-based observability for ChainCodec.
//!
//! ## Built-in metrics
//! - `chaincodec.events_decoded`    — counter, tagged with chain + schema
//! - `chaincodec.events_skipped`    — counter, tagged with chain + reason
//! - `chaincodec.decode_errors`     — counter, tagged with chain + error_type
//! - `chaincodec.decode_latency_ms` — histogram
//! - `chaincodec.batch_size`        — histogram
//! - `chaincodec.schema_cache_hits` — counter
//!
//! ## Structured logging
//! JSON-structured logs compatible with ELK, Loki, CloudWatch.
//! Log levels configurable per component.

pub mod metrics;
pub mod tracing_setup;

pub use metrics::ChainCodecMetrics;
pub use tracing_setup::{init_tracing, LogConfig, TracingConfig};
