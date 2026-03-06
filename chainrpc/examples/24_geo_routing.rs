//! # Example 24: Geographic Routing
//!
//! Demonstrates `GeoRouter` — routes RPC requests to the geographically
//! closest provider to minimize latency, with automatic fallback to other
//! regions when the local region is unhealthy.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::sync::Arc;
use chainrpc_core::geo_routing::{
    GeoRouter, Region, RegionalEndpoints, detect_region_from_env,
};
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::{RpcTransport, HealthStatus};

#[tokio::main]
async fn main() {
    println!("=== Geographic Routing ===\n");

    // =====================================================================
    // 1. Region Detection
    // =====================================================================
    // Auto-detect from cloud environment variables (AWS_REGION, FLY_REGION, etc.)
    println!("--- Region Detection ---\n");

    if let Some(region) = detect_region_from_env() {
        println!("Detected region: {}", region.as_str());
    } else {
        println!("No cloud region detected, defaulting to UsEast");
    }

    // =====================================================================
    // 2. Region Proximity
    // =====================================================================
    // Each region knows its geographic neighbors:
    println!("\n--- Region Proximity ---\n");

    let region = Region::UsEast;
    println!("{} proximity order:", region.as_str());
    for (i, neighbor) in region.proximity_order().iter().enumerate() {
        println!("  {}. {}", i + 1, neighbor.as_str());
    }
    // UsEast -> UsWest -> EuWest -> SouthAmerica -> EuCentral -> ...

    // =====================================================================
    // 3. GeoRouter Setup
    // =====================================================================
    // In production with real providers:
    //
    //   let mut geo = GeoRouter::new(Region::UsEast);
    //
    //   // US East providers (primary)
    //   geo.add_provider(Region::UsEast, Arc::new(
    //       HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/KEY")
    //   ));
    //
    //   // EU providers (fallback)
    //   geo.add_provider(Region::EuWest, Arc::new(
    //       HttpRpcClient::default_for("https://eu-mainnet.infura.io/v3/KEY")
    //   ));
    //
    //   // Asia providers (fallback)
    //   geo.add_provider(Region::AsiaEast, Arc::new(
    //       HttpRpcClient::default_for("https://rpc.ankr.com/eth")
    //   ));
    //
    //   // Implements RpcTransport — routes to local region first
    //   let resp = geo.send(JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;
    //
    //   // If US East is down, automatically falls back to EU West
    //   // Then Asia East if EU is also down
    //
    //   // Health across all regions
    //   for summary in geo.health_summary() {
    //       println!("{}: {} providers, healthy={}, latency={}ms",
    //           summary.region, summary.provider_count,
    //           summary.healthy, summary.avg_latency_ms);
    //   }

    println!("\n--- GeoRouter ---\n");
    println!("GeoRouter routes requests to the nearest healthy region.");
    println!("Fallback order follows geographic proximity.");
    println!("Usage: GeoRouter::new(Region::UsEast) + .add_provider(region, transport)");

    // =====================================================================
    // 4. Regional Endpoints
    // =====================================================================
    // Pre-configured endpoint URLs by region:
    println!("\n--- Regional Endpoints ---\n");

    println!("Alchemy regional endpoints:");
    for (region, url) in RegionalEndpoints::alchemy("YOUR_KEY") {
        println!("  {} -> {}", region.as_str(), url);
    }

    println!("\nAnkr regional endpoints:");
    for (region, url) in RegionalEndpoints::ankr() {
        println!("  {} -> {}", region.as_str(), url);
    }

    println!("\nDone.");
}
