//! # with_observability
//!
//! Demonstrates ChainCodec metrics and structured logging via
//! `chaincodec-observability`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §8 — Node / RPC Ops)
//! - Track decode throughput, error rates, and latency via OpenTelemetry counters
//! - Structured JSON logging compatible with ELK, Grafana Loki, CloudWatch
//! - Per-component log levels (e.g. raise stream to debug, keep batch at info)
//! - Export metrics to Prometheus / Grafana via OTLP (endpoint configurable)
//!
//! Run with:
//! ```sh
//! cargo run --bin with_observability
//!
//! # With JSON logging:
//! LOG_JSON=1 cargo run --bin with_observability
//!
//! # With debug level for the evm crate:
//! RUST_LOG=info,chaincodec_evm=debug cargo run --bin with_observability
//! ```

use anyhow::Result;
use chaincodec_core::{
    chain::chains,
    decoder::ChainDecoder,
    event::{EventFingerprint, RawEvent},
    schema::SchemaRegistry,
};
use chaincodec_evm::EvmDecoder;
use chaincodec_observability::{
    metrics::ChainCodecMetrics,
    tracing_setup::{init_tracing, LogConfig},
};
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use opentelemetry::{global, KeyValue};
use std::time::Instant;
use tracing::{debug, info, warn};

const ERC20_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
"#;

fn main() -> Result<()> {
    // ── 1. Initialise structured logging ──────────────────────────────────────
    let log_config = LogConfig {
        level: "info".into(),
        components: [
            ("chaincodec_evm".into(), "debug".into()),
            ("chaincodec_registry".into(), "warn".into()),
        ]
        .into(),
        // Set LOG_JSON=1 to emit JSON-structured logs (ELK/Loki/CloudWatch)
        json: std::env::var("LOG_JSON").is_ok(),
    };
    init_tracing(&log_config);

    info!(
        component = "with_observability",
        log_json = log_config.json,
        level = %log_config.level,
        "ChainCodec observability demo starting"
    );

    println!("ChainCodec — Observability Demo");
    println!("═══════════════════════════════════════════════════════");
    println!("  (structured logs are emitted alongside this output)");

    // ── 2. Create OpenTelemetry meter + ChainCodecMetrics ─────────────────────
    //
    // In production: install an OTLP MeterProvider first:
    //   opentelemetry_otlp::new_pipeline().metrics(...).install_batch(...)
    //
    // Here we use the default global meter (no-op in the absence of a provider),
    // which lets the example run without a running OTLP server.
    let meter = global::meter("chaincodec-example");
    let metrics = ChainCodecMetrics::new(&meter);

    info!("OpenTelemetry meter initialised (no-op provider — metrics counted but not exported)");
    println!("\n  Metrics registered:");
    println!("    chaincodec.events_decoded   (counter, chain + schema tags)");
    println!("    chaincodec.events_skipped   (counter, chain + reason tags)");
    println!("    chaincodec.decode_errors    (counter, chain + error_type tags)");
    println!("    chaincodec.decode_latency_ms (histogram, chain tag)");
    println!("    chaincodec.batch_size        (histogram)");
    println!("    chaincodec.schema_cache_hits (counter)");

    // ── 3. Set up registry + decoder ──────────────────────────────────────────
    let registry = MemoryRegistry::new();
    for schema in CsdlParser::parse_all(ERC20_CSDL)? {
        registry.add(schema)?;
    }
    let decoder = EvmDecoder::new();

    // ── 4. Simulate decode events with metric recording ───────────────────────
    let events = vec![
        // A: valid Transfer
        (
            "valid",
            RawEvent {
                chain: chains::ethereum(),
                tx_hash: "0xabc001".into(),
                block_number: 19_000_000,
                block_timestamp: 1_700_000_000,
                log_index: 0,
                address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
                topics: vec![
                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
                    "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
                    "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
                ],
                data: hex::decode(
                    "000000000000000000000000000000000000000000000000000000003b9aca00",
                )?,
                raw_receipt: None,
            },
        ),
        // B: another valid Transfer
        (
            "valid",
            RawEvent {
                chain: chains::ethereum(),
                tx_hash: "0xabc002".into(),
                block_number: 19_000_001,
                block_timestamp: 1_700_000_012,
                log_index: 1,
                address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
                topics: vec![
                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
                    "0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b".into(),
                    "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045".into(),
                ],
                data: hex::decode(
                    "0000000000000000000000000000000000000000000000000000000017d78400",
                )?,
                raw_receipt: None,
            },
        ),
        // C: unknown fingerprint — will be skipped
        (
            "unknown",
            RawEvent {
                chain: chains::ethereum(),
                tx_hash: "0xabc003".into(),
                block_number: 19_000_002,
                block_timestamp: 1_700_000_024,
                log_index: 2,
                address: "0x0000000000000000000000000000000000000000".into(),
                topics: vec!["0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into()],
                data: vec![],
                raw_receipt: None,
            },
        ),
    ];

    println!("\n─── Decode Loop with Metrics ────────────────────────");

    let mut decoded_count = 0u64;
    let mut skipped_count = 0u64;
    let mut error_count   = 0u64;

    for (kind, raw) in &events {
        let start = Instant::now();
        let chain = raw.chain.slug.as_str();
        let fp = EventFingerprint::new(raw.topics.first().cloned().unwrap_or_default());

        match registry.get_by_fingerprint(&fp) {
            None => {
                warn!(chain, tx = %raw.tx_hash, "schema not found — skipping");
                metrics.events_skipped.add(1, &[
                    KeyValue::new("chain", chain.to_string()),
                    KeyValue::new("reason", "schema_not_found"),
                ]);
                skipped_count += 1;
                println!("  [SKIP] tx={} — no schema for fingerprint", &raw.tx_hash[..10]);
            }
            Some(schema) => {
                match decoder.decode_event(raw, &schema) {
                    Ok(decoded) => {
                        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
                        debug!(
                            chain,
                            schema = %schema.name,
                            block = decoded.block_number,
                            latency_ms,
                            "event decoded"
                        );
                        metrics.record_decoded(chain, &schema.name);
                        metrics.record_latency(latency_ms, chain);
                        decoded_count += 1;

                        let value_field = decoded.fields.get("value")
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        println!(
                            "  [OK]   tx={} schema={} value={}  latency={:.3}ms",
                            &raw.tx_hash[..10],
                            schema.name,
                            value_field,
                            latency_ms
                        );

                        if decoded.has_errors() {
                            for (field, err) in &decoded.decode_errors {
                                warn!(chain, schema = %schema.name, field, error = %err, "field decode error");
                                metrics.record_error(chain, "field_decode_error");
                                error_count += 1;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(chain, schema = %schema.name, error = %e, "decode failed");
                        metrics.record_error(chain, "decode_failed");
                        error_count += 1;
                        println!("  [ERR]  tx={} — {e}", &raw.tx_hash[..10]);
                    }
                }
            }
        }

        let _ = kind; // used for clarity in the vec
    }

    // ── 5. Final metric summary ───────────────────────────────────────────────
    println!("\n─── Metric Summary (counters recorded) ──────────────");
    println!("  chaincodec.events_decoded   = {decoded_count}");
    println!("  chaincodec.events_skipped   = {skipped_count}");
    println!("  chaincodec.decode_errors    = {error_count}");
    println!();
    println!("  (in production: export via OTLP to Prometheus/Grafana)");
    println!("  (Grafana panel: rate(chaincodec_events_decoded_total[1m]))");

    info!(
        decoded = decoded_count,
        skipped = skipped_count,
        errors = error_count,
        "decode session complete"
    );

    // ── 6. Batch size metric example ──────────────────────────────────────────
    metrics.batch_size.record(events.len() as u64, &[]);
    metrics.schema_cache_hits.add(2, &[]); // 2 schema lookups hit the same cached entry

    println!("\n─── Log config used ─────────────────────────────────");
    println!("  global level:     {}", log_config.level);
    println!("  chaincodec_evm:   debug  (verbose field tracing)");
    println!("  chaincodec_registry: warn (suppress hit/miss noise)");
    println!("  JSON logs:        {}", log_config.json);
    println!("  (set LOG_JSON=1 for ELK/Loki/CloudWatch compatible output)");

    println!("\n✓ Observability demo complete");
    Ok(())
}
