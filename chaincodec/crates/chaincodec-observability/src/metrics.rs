//! ChainCodec metrics definitions.
//!
//! All metrics use OpenTelemetry conventions.
//! They can be exported via OTLP to Prometheus, Grafana, Datadog, etc.

use opentelemetry::{
    metrics::{Counter, Histogram, Meter},
    KeyValue,
};
use std::sync::Arc;

/// Central metrics handle for ChainCodec.
#[derive(Clone)]
pub struct ChainCodecMetrics {
    pub events_decoded: Counter<u64>,
    pub events_skipped: Counter<u64>,
    pub decode_errors: Counter<u64>,
    pub decode_latency_ms: Histogram<f64>,
    pub batch_size: Histogram<u64>,
    pub schema_cache_hits: Counter<u64>,
}

impl ChainCodecMetrics {
    pub fn new(meter: &Meter) -> Self {
        Self {
            events_decoded: meter
                .u64_counter("chaincodec.events_decoded")
                .with_description("Total number of successfully decoded events")
                .build(),
            events_skipped: meter
                .u64_counter("chaincodec.events_skipped")
                .with_description("Events skipped due to missing schema or filtering")
                .build(),
            decode_errors: meter
                .u64_counter("chaincodec.decode_errors")
                .with_description("Events that failed to decode")
                .build(),
            decode_latency_ms: meter
                .f64_histogram("chaincodec.decode_latency_ms")
                .with_description("Time to decode a single event in milliseconds")
                .build(),
            batch_size: meter
                .u64_histogram("chaincodec.batch_size")
                .with_description("Number of events in a batch decode request")
                .build(),
            schema_cache_hits: meter
                .u64_counter("chaincodec.schema_cache_hits")
                .with_description("Registry fingerprint lookup cache hits")
                .build(),
        }
    }

    pub fn record_decoded(&self, chain: &str, schema: &str) {
        self.events_decoded.add(
            1,
            &[
                KeyValue::new("chain", chain.to_string()),
                KeyValue::new("schema", schema.to_string()),
            ],
        );
    }

    pub fn record_error(&self, chain: &str, error_type: &str) {
        self.decode_errors.add(
            1,
            &[
                KeyValue::new("chain", chain.to_string()),
                KeyValue::new("error_type", error_type.to_string()),
            ],
        );
    }

    pub fn record_latency(&self, ms: f64, chain: &str) {
        self.decode_latency_ms
            .record(ms, &[KeyValue::new("chain", chain.to_string())]);
    }
}
