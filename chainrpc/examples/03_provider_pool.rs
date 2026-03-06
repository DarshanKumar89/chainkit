//! # Example 03: Multi-Provider Failover Pool
//!
//! Demonstrates `ProviderPool` -- a round-robin load balancer that
//! automatically skips unhealthy providers using per-provider circuit breakers.
//!
//! ## What this demonstrates
//!
//! - Building a pool from multiple `HttpRpcClient` instances
//! - Round-robin request distribution across providers
//! - `healthy_count()` to check how many providers are up
//! - `health_summary()` for a quick overview of each provider's state
//! - `health_report()` for detailed JSON reports including metrics
//! - Using named providers from `chainrpc_providers` (Alchemy, Infura, public)
//! - Automatic failover when a provider goes down
//! - Pool-level health status (Healthy / Degraded / Unhealthy)

use std::sync::Arc;

use chainrpc_core::pool::{ProviderPool, ProviderPoolConfig};
use chainrpc_core::transport::{HealthStatus, RpcTransport};
use chainrpc_http::HttpRpcClient;
use chainrpc_providers::{alchemy, infura, public};

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Step 1: Create individual provider clients.
    //
    // Each provider has its own retry, circuit breaker, and rate limiter
    // settings baked in via its provider profile.
    // -----------------------------------------------------------------------
    let alchemy_client = alchemy::http_client("YOUR_ALCHEMY_KEY", 1); // Ethereum mainnet
    let infura_client = infura::http_client("YOUR_INFURA_PROJECT_ID", 1);
    let public_client = public::ankr(1); // Free, no API key needed

    // -----------------------------------------------------------------------
    // Step 2: Wrap them in Arc<dyn RpcTransport> and build the pool.
    //
    // The pool uses round-robin selection by default. Each provider gets its
    // own circuit breaker from the ProviderPoolConfig.
    // -----------------------------------------------------------------------
    let transports: Vec<Arc<dyn RpcTransport>> = vec![
        Arc::new(alchemy_client),
        Arc::new(infura_client),
        Arc::new(public_client),
    ];

    // Use new_with_metrics() so the pool tracks per-provider request counts,
    // latency, and error rates.
    let pool = ProviderPool::new_with_metrics(transports, ProviderPoolConfig::default());

    println!("Pool created with {} providers", pool.len());
    println!("Initially healthy: {}/{}", pool.healthy_count(), pool.len());

    // -----------------------------------------------------------------------
    // Step 3: Make requests through the pool.
    //
    // The pool distributes requests round-robin across healthy providers.
    // If a provider's circuit breaker is open, it is skipped automatically.
    // -----------------------------------------------------------------------
    for i in 1..=6 {
        let block: Result<String, _> = pool.call(i, "eth_blockNumber", vec![]).await;

        match block {
            Ok(hex) => println!("Request {i}: block = {hex}"),
            Err(e) => println!("Request {i}: error = {e}"),
        }
    }

    // -----------------------------------------------------------------------
    // Step 4: Check health summary.
    //
    // `health_summary()` returns a Vec of (url, HealthStatus, circuit_state)
    // tuples -- one per provider.
    // -----------------------------------------------------------------------
    println!("\n--- Health Summary ---");
    for (url, health, circuit) in pool.health_summary() {
        println!("  {url}");
        println!("    health:  {health}");
        println!("    circuit: {circuit}");
    }

    // -----------------------------------------------------------------------
    // Step 5: Detailed health report with metrics.
    //
    // `health_report()` returns JSON values that include request counts,
    // success rates, and latency when metrics are enabled.
    // -----------------------------------------------------------------------
    println!("\n--- Detailed Health Report ---");
    for report in pool.health_report() {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }

    // -----------------------------------------------------------------------
    // Step 6: Pool-level health.
    //
    // The pool's overall health is derived from its providers:
    //   - Healthy   => all providers have closed circuits
    //   - Degraded  => at least one provider is down but not all
    //   - Unhealthy => all providers' circuits are open
    // -----------------------------------------------------------------------
    let pool_health = pool.health();
    println!("\nPool-level health: {pool_health}");
    println!("Healthy providers: {}/{}", pool.healthy_count(), pool.len());

    match pool_health {
        HealthStatus::Healthy => println!("All providers operational."),
        HealthStatus::Degraded => println!("Some providers are down; failover is active."),
        HealthStatus::Unhealthy => println!("All providers are down!"),
        HealthStatus::Unknown => println!("Health status not yet determined."),
    }

    // -----------------------------------------------------------------------
    // Step 7: Quick pool from URLs using the helper function.
    //
    // `pool_from_urls()` creates a pool where each URL gets an HttpRpcClient
    // with default config. This is the fastest way to set up a pool.
    // -----------------------------------------------------------------------
    let quick_pool = chainrpc_http::pool_from_urls(&[
        "https://eth-mainnet.g.alchemy.com/v2/KEY1",
        "https://mainnet.infura.io/v3/KEY2",
        "https://rpc.ankr.com/eth",
    ])
    .expect("failed to create pool");

    let block: String = quick_pool
        .call(100, "eth_blockNumber", vec![])
        .await
        .expect("pool request failed");
    println!("\nQuick pool block: {block}");

    // -----------------------------------------------------------------------
    // Step 8: Metrics snapshots for monitoring.
    //
    // When the pool was created with `new_with_metrics()`, each provider
    // tracks atomic counters for total/success/failed requests, latency,
    // rate-limit hits, and circuit-breaker events.
    // -----------------------------------------------------------------------
    println!("\n--- Provider Metrics ---");
    for snap in pool.metrics() {
        println!(
            "  {} | total={} success={} failed={} avg_latency={:.1}ms rate={:.2}%",
            snap.url,
            snap.total_requests,
            snap.successful_requests,
            snap.failed_requests,
            snap.avg_latency_ms,
            snap.success_rate * 100.0,
        );
    }
}
