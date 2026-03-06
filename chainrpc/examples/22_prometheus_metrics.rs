//! # Prometheus Metrics & Observability
//!
//! Demonstrates the full metrics and observability pipeline:
//!
//! 1. Create `ProviderMetrics` for two RPC providers.
//! 2. Record successes (with latency), failures, rate-limit hits, and
//!    circuit-breaker opens.
//! 3. Take a `MetricsSnapshot` and inspect its fields.
//! 4. Create an `RpcMetrics` aggregate across multiple providers.
//! 5. Export Prometheus exposition-format text via `to_prometheus()`.
//! 6. Conceptual `/metrics` HTTP endpoint.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::time::Duration;

use chainrpc_core::metrics::{MetricsSnapshot, ProviderMetrics, RpcMetrics};

#[tokio::main]
async fn main() {
    println!("=== Prometheus Metrics & Observability ===\n");

    // =====================================================================
    // 1. Create ProviderMetrics for two providers
    // =====================================================================
    let alchemy = ProviderMetrics::new("https://eth-mainnet.g.alchemy.com/v2/DEMO");
    let infura = ProviderMetrics::new("https://mainnet.infura.io/v3/DEMO");

    println!("Created metrics for 2 providers:");
    println!("  - {}", alchemy.url());
    println!("  - {}", infura.url());

    // =====================================================================
    // 2. Record events
    // =====================================================================
    println!("\n--- Recording events ---\n");

    // Alchemy: 5 successful requests with varying latencies.
    alchemy.record_success(Duration::from_millis(45));
    alchemy.record_success(Duration::from_millis(52));
    alchemy.record_success(Duration::from_millis(38));
    alchemy.record_success(Duration::from_millis(120));
    alchemy.record_success(Duration::from_millis(67));
    println!("  Alchemy: 5 successes recorded (45ms, 52ms, 38ms, 120ms, 67ms)");

    // Alchemy: 1 failure (e.g. timeout).
    alchemy.record_failure();
    println!("  Alchemy: 1 failure recorded");

    // Alchemy: 2 rate-limit rejections.
    alchemy.record_rate_limit();
    alchemy.record_rate_limit();
    println!("  Alchemy: 2 rate-limit hits recorded");

    // Infura: 3 successes, 2 failures, 1 circuit open.
    infura.record_success(Duration::from_millis(80));
    infura.record_success(Duration::from_millis(95));
    infura.record_success(Duration::from_millis(110));
    infura.record_failure();
    infura.record_failure();
    infura.record_circuit_open();
    println!("  Infura:  3 successes, 2 failures, 1 circuit open recorded");

    // =====================================================================
    // 3. Take snapshots and inspect fields
    // =====================================================================
    println!("\n--- Snapshots ---\n");

    let alchemy_snap: MetricsSnapshot = alchemy.snapshot();
    println!("Alchemy snapshot:");
    println!("  url                : {}", alchemy_snap.url);
    println!("  total_requests     : {}", alchemy_snap.total_requests);
    println!("  successful_requests: {}", alchemy_snap.successful_requests);
    println!("  failed_requests    : {}", alchemy_snap.failed_requests);
    println!("  avg_latency_ms     : {:.2}", alchemy_snap.avg_latency_ms);
    println!("  min_latency_ms     : {:.2}", alchemy_snap.min_latency_ms);
    println!("  max_latency_ms     : {:.2}", alchemy_snap.max_latency_ms);
    println!("  rate_limit_hits    : {}", alchemy_snap.rate_limit_hits);
    println!("  circuit_open_count : {}", alchemy_snap.circuit_open_count);
    println!("  success_rate       : {:.4}", alchemy_snap.success_rate);

    assert_eq!(alchemy_snap.total_requests, 6);
    assert_eq!(alchemy_snap.successful_requests, 5);
    assert_eq!(alchemy_snap.failed_requests, 1);
    assert_eq!(alchemy_snap.rate_limit_hits, 2);
    assert_eq!(alchemy_snap.circuit_open_count, 0);

    let infura_snap: MetricsSnapshot = infura.snapshot();
    println!("\nInfura snapshot:");
    println!("  url                : {}", infura_snap.url);
    println!("  total_requests     : {}", infura_snap.total_requests);
    println!("  successful_requests: {}", infura_snap.successful_requests);
    println!("  failed_requests    : {}", infura_snap.failed_requests);
    println!("  avg_latency_ms     : {:.2}", infura_snap.avg_latency_ms);
    println!("  rate_limit_hits    : {}", infura_snap.rate_limit_hits);
    println!("  circuit_open_count : {}", infura_snap.circuit_open_count);
    println!("  success_rate       : {:.4}", infura_snap.success_rate);

    assert_eq!(infura_snap.total_requests, 5);
    assert_eq!(infura_snap.circuit_open_count, 1);

    // Computed metrics.
    println!("\nComputed metrics:");
    println!(
        "  Alchemy avg latency: {:?}",
        alchemy.avg_latency(),
    );
    println!(
        "  Alchemy success rate: {:.2}%",
        alchemy.success_rate() * 100.0,
    );
    println!(
        "  Infura success rate:  {:.2}%",
        infura.success_rate() * 100.0,
    );

    // =====================================================================
    // 4. RpcMetrics — aggregate across providers
    // =====================================================================
    println!("\n--- RpcMetrics (aggregate) ---\n");

    let mut rpc_metrics = RpcMetrics::new();

    // Register providers and get references.  In a real application these
    // references would be stored alongside the transport for recording.
    let p1 = rpc_metrics.add_provider("https://eth-mainnet.g.alchemy.com/v2/DEMO")
        as *const ProviderMetrics;
    let p2 = rpc_metrics.add_provider("https://mainnet.infura.io/v3/DEMO")
        as *const ProviderMetrics;

    // Record some events through the aggregate's provider references.
    // Safety: pointers are valid for the lifetime of `rpc_metrics`.
    unsafe {
        (*p1).record_success(Duration::from_millis(42));
        (*p1).record_success(Duration::from_millis(58));
        (*p1).record_failure();
        (*p2).record_success(Duration::from_millis(90));
        (*p2).record_rate_limit();
    }

    println!("  provider_count  : {}", rpc_metrics.provider_count());
    println!("  total_requests  : {}", rpc_metrics.total_requests());
    assert_eq!(rpc_metrics.provider_count(), 2);
    assert_eq!(rpc_metrics.total_requests(), 4); // 3 from p1 + 1 from p2

    // snapshot_all returns a Vec<MetricsSnapshot>, one per provider.
    let all_snaps = rpc_metrics.snapshot_all();
    println!("  snapshots count : {}", all_snaps.len());
    for snap in &all_snaps {
        println!(
            "    {} -> {} reqs, {:.2}% success",
            snap.url,
            snap.total_requests,
            snap.success_rate * 100.0,
        );
    }

    // =====================================================================
    // 5. Prometheus exposition-format export
    // =====================================================================
    println!("\n--- Prometheus Export ---\n");

    // Per-provider export.
    let prom_alchemy = alchemy_snap.to_prometheus();
    println!("Alchemy Prometheus text:");
    for line in prom_alchemy.lines() {
        println!("  {line}");
    }

    // Full aggregate export (includes HELP/TYPE headers).
    let prom_all = rpc_metrics.to_prometheus();
    println!("Full aggregate Prometheus export:");
    println!("---");
    print!("{prom_all}");
    println!("---");

    // Verify the export contains expected metric names.
    assert!(prom_all.contains("chainrpc_requests_total"));
    assert!(prom_all.contains("chainrpc_latency_avg_ms"));
    assert!(prom_all.contains("chainrpc_success_rate"));
    assert!(prom_all.contains("# HELP"));
    assert!(prom_all.contains("# TYPE"));
    println!("[OK] Prometheus export validated");

    // =====================================================================
    // 6. Conceptual /metrics HTTP endpoint
    // =====================================================================
    println!("\n--- Conceptual /metrics Endpoint ---\n");

    // In production, you would serve the Prometheus text at an HTTP endpoint.
    // Using a lightweight framework like axum:
    //
    //   use axum::{routing::get, Router};
    //   use std::sync::Arc;
    //
    //   let metrics = Arc::new(rpc_metrics);
    //
    //   async fn metrics_handler(
    //       metrics: axum::extract::State<Arc<RpcMetrics>>,
    //   ) -> String {
    //       metrics.to_prometheus()
    //   }
    //
    //   let app = Router::new()
    //       .route("/metrics", get(metrics_handler))
    //       .with_state(metrics);
    //
    //   let listener = tokio::net::TcpListener::bind("0.0.0.0:9090").await?;
    //   axum::serve(listener, app).await?;
    //
    // Prometheus scrape config (prometheus.yml):
    //
    //   scrape_configs:
    //     - job_name: "chainrpc"
    //       static_configs:
    //         - targets: ["localhost:9090"]

    println!("  Serve rpc_metrics.to_prometheus() at GET /metrics on port 9090");
    println!("  Configure Prometheus to scrape http://localhost:9090/metrics");

    println!("\nDone.");
}
