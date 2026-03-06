//! Geographic-aware RPC routing.
//!
//! Routes requests to the geographically closest provider to minimize latency.
//! Supports manual region configuration and automatic fallback when a region's
//! providers are unhealthy.
//!
//! # Example
//! ```ignore
//! let mut geo = GeoRouter::new(Region::UsEast);
//! geo.add_provider(Region::UsEast, us_east_client);
//! geo.add_provider(Region::Europe, eu_client);
//! geo.add_provider(Region::Asia, asia_client);
//!
//! // Routes to US East (local region)
//! let resp = geo.send(req).await?;
//!
//! // If US East is down, falls back to next-closest region
//! ```

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;

use crate::error::TransportError;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// Region
// ---------------------------------------------------------------------------

/// Geographic region for endpoint classification.
///
/// Used to tag RPC providers with their physical location so the router
/// can prefer low-latency, geographically close endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    UsEast,
    UsWest,
    EuWest,
    EuCentral,
    AsiaSoutheast,
    AsiaEast,
    SouthAmerica,
    Oceania,
}

impl Region {
    /// Returns the string identifier for this region.
    pub fn as_str(&self) -> &'static str {
        match self {
            Region::UsEast => "us-east",
            Region::UsWest => "us-west",
            Region::EuWest => "eu-west",
            Region::EuCentral => "eu-central",
            Region::AsiaSoutheast => "asia-southeast",
            Region::AsiaEast => "asia-east",
            Region::SouthAmerica => "south-america",
            Region::Oceania => "oceania",
        }
    }

    /// Returns regions ordered by geographic proximity (closest first, excluding self).
    pub fn proximity_order(&self) -> Vec<Region> {
        use Region::*;
        match self {
            UsEast => vec![UsWest, EuWest, SouthAmerica, EuCentral, AsiaSoutheast, AsiaEast, Oceania],
            UsWest => vec![UsEast, AsiaEast, AsiaSoutheast, Oceania, SouthAmerica, EuWest, EuCentral],
            EuWest => vec![EuCentral, UsEast, SouthAmerica, AsiaSoutheast, UsWest, AsiaEast, Oceania],
            EuCentral => vec![EuWest, AsiaSoutheast, UsEast, AsiaEast, SouthAmerica, UsWest, Oceania],
            AsiaSoutheast => vec![AsiaEast, Oceania, EuCentral, EuWest, UsWest, UsEast, SouthAmerica],
            AsiaEast => vec![AsiaSoutheast, Oceania, UsWest, EuCentral, EuWest, UsEast, SouthAmerica],
            SouthAmerica => vec![UsEast, UsWest, EuWest, EuCentral, AsiaSoutheast, AsiaEast, Oceania],
            Oceania => vec![AsiaSoutheast, AsiaEast, UsWest, EuCentral, EuWest, UsEast, SouthAmerica],
        }
    }

    /// All known regions.
    pub fn all() -> &'static [Region] {
        &[
            Region::UsEast,
            Region::UsWest,
            Region::EuWest,
            Region::EuCentral,
            Region::AsiaSoutheast,
            Region::AsiaEast,
            Region::SouthAmerica,
            Region::Oceania,
        ]
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Region {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "us-east" | "us_east" | "useast" => Ok(Region::UsEast),
            "us-west" | "us_west" | "uswest" => Ok(Region::UsWest),
            "eu-west" | "eu_west" | "euwest" => Ok(Region::EuWest),
            "eu-central" | "eu_central" | "eucentral" => Ok(Region::EuCentral),
            "asia-southeast" | "asia_southeast" | "asiasoutheast" => Ok(Region::AsiaSoutheast),
            "asia-east" | "asia_east" | "asiaeast" => Ok(Region::AsiaEast),
            "south-america" | "south_america" | "southamerica" => Ok(Region::SouthAmerica),
            "oceania" => Ok(Region::Oceania),
            _ => Err(format!("unknown region: {s}")),
        }
    }
}

// ---------------------------------------------------------------------------
// RegionalHealth (internal)
// ---------------------------------------------------------------------------

/// Tracks per-region health stats.
#[derive(Debug)]
struct RegionalHealth {
    healthy: bool,
    avg_latency: Duration,
    last_checked: Instant,
    success_count: u64,
    failure_count: u64,
    /// Running sum of latencies in microseconds for computing the average.
    latency_sum_us: u128,
}

impl RegionalHealth {
    fn new() -> Self {
        Self {
            healthy: true,
            avg_latency: Duration::ZERO,
            last_checked: Instant::now(),
            success_count: 0,
            failure_count: 0,
            latency_sum_us: 0,
        }
    }

    fn record_success(&mut self, latency: Duration) {
        self.success_count += 1;
        self.latency_sum_us += latency.as_micros();
        let total = self.success_count + self.failure_count;
        if total > 0 {
            self.avg_latency = Duration::from_micros((self.latency_sum_us / total as u128) as u64);
        }
        self.last_checked = Instant::now();
        // Mark healthy if success rate > 50%
        self.healthy = self.success_rate() > 0.5;
    }

    fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_checked = Instant::now();
        // Mark unhealthy if failure rate >= 50%
        self.healthy = self.success_rate() > 0.5;
    }

    fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            return 1.0; // assume healthy with no data
        }
        self.success_count as f64 / total as f64
    }
}

// ---------------------------------------------------------------------------
// RegionHealthSummary (public API)
// ---------------------------------------------------------------------------

/// Public health summary for a single region.
#[derive(Debug, Clone, Serialize)]
pub struct RegionHealthSummary {
    /// Region identifier string.
    pub region: String,
    /// Number of providers configured for this region.
    pub provider_count: usize,
    /// Whether the region is considered healthy.
    pub healthy: bool,
    /// Average latency in milliseconds.
    pub avg_latency_ms: u64,
    /// Total successful requests routed to this region.
    pub success_count: u64,
    /// Total failed requests routed to this region.
    pub failure_count: u64,
}

// ---------------------------------------------------------------------------
// GeoRouter
// ---------------------------------------------------------------------------

/// Geographic-aware RPC router.
///
/// Routes requests to the nearest healthy region's provider, falling back
/// to the next-closest region when the primary is unavailable.
///
/// Within a single region, providers are selected via round-robin to
/// spread the load evenly.
pub struct GeoRouter {
    local_region: Region,
    providers: HashMap<Region, Vec<Arc<dyn RpcTransport>>>,
    health: Mutex<HashMap<Region, RegionalHealth>>,
    /// Round-robin cursors per region.
    cursors: Mutex<HashMap<Region, usize>>,
}

impl GeoRouter {
    /// Create a new geo-router with the specified local region.
    pub fn new(local_region: Region) -> Self {
        Self {
            local_region,
            providers: HashMap::new(),
            health: Mutex::new(HashMap::new()),
            cursors: Mutex::new(HashMap::new()),
        }
    }

    /// Add a single provider for a region.
    pub fn add_provider(&mut self, region: Region, transport: Arc<dyn RpcTransport>) {
        self.providers
            .entry(region)
            .or_insert_with(Vec::new)
            .push(transport);
        // Ensure health tracking exists for this region.
        self.health
            .lock()
            .unwrap()
            .entry(region)
            .or_insert_with(RegionalHealth::new);
    }

    /// Add multiple providers for a region at once.
    pub fn add_providers(&mut self, region: Region, transports: Vec<Arc<dyn RpcTransport>>) {
        for t in transports {
            self.add_provider(region, t);
        }
    }

    /// Get the local region.
    pub fn local_region(&self) -> Region {
        self.local_region
    }

    /// Get the total number of providers across all regions.
    pub fn provider_count(&self) -> usize {
        self.providers.values().map(|v| v.len()).sum()
    }

    /// Get the list of regions that have at least one provider configured.
    pub fn regions(&self) -> Vec<Region> {
        self.providers.keys().copied().collect()
    }

    /// Get a health summary for every configured region.
    pub fn health_summary(&self) -> Vec<RegionHealthSummary> {
        let health = self.health.lock().unwrap();
        self.providers
            .iter()
            .map(|(region, provs)| {
                let h = health.get(region);
                RegionHealthSummary {
                    region: region.to_string(),
                    provider_count: provs.len(),
                    healthy: h.map_or(true, |h| h.healthy),
                    avg_latency_ms: h.map_or(0, |h| h.avg_latency.as_millis() as u64),
                    success_count: h.map_or(0, |h| h.success_count),
                    failure_count: h.map_or(0, |h| h.failure_count),
                }
            })
            .collect()
    }

    /// Get the ordered list of regions to try (local first, then by proximity).
    ///
    /// Only regions that actually have providers are included.
    fn routing_order(&self) -> Vec<Region> {
        let mut order = vec![self.local_region];
        order.extend(self.local_region.proximity_order());
        // Only keep regions that have providers registered.
        order.retain(|r| self.providers.contains_key(r));
        order
    }

    /// Select a provider from a region using round-robin.
    fn select_from_region(&self, region: &Region) -> Option<Arc<dyn RpcTransport>> {
        let providers = self.providers.get(region)?;
        if providers.is_empty() {
            return None;
        }
        let mut cursors = self.cursors.lock().unwrap();
        let cursor = cursors.entry(*region).or_insert(0);
        let idx = *cursor % providers.len();
        *cursor = cursor.wrapping_add(1);
        Some(providers[idx].clone())
    }

    /// Record a successful request for a region.
    fn record_success(&self, region: &Region, latency: Duration) {
        let mut health = self.health.lock().unwrap();
        let entry = health.entry(*region).or_insert_with(RegionalHealth::new);
        entry.record_success(latency);
    }

    /// Record a failed request for a region.
    fn record_failure(&self, region: &Region) {
        let mut health = self.health.lock().unwrap();
        let entry = health.entry(*region).or_insert_with(RegionalHealth::new);
        entry.record_failure();
    }

    /// Check whether a region is currently considered healthy.
    fn is_region_healthy(&self, region: &Region) -> bool {
        let health = self.health.lock().unwrap();
        match health.get(region) {
            Some(h) => h.healthy,
            None => true, // no data yet — assume healthy
        }
    }
}

#[async_trait]
impl RpcTransport for GeoRouter {
    async fn send(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
        for region in self.routing_order() {
            if !self.is_region_healthy(&region) {
                continue;
            }
            if let Some(provider) = self.select_from_region(&region) {
                let start = Instant::now();
                match provider.send(req.clone()).await {
                    Ok(resp) => {
                        self.record_success(&region, start.elapsed());
                        return Ok(resp);
                    }
                    Err(e) if e.is_retryable() => {
                        self.record_failure(&region);
                        continue; // try next region
                    }
                    Err(e) => return Err(e), // non-retryable — propagate immediately
                }
            }
        }
        Err(TransportError::AllProvidersDown)
    }

    fn url(&self) -> &str {
        "geo-router"
    }

    fn health(&self) -> HealthStatus {
        if self.is_region_healthy(&self.local_region) {
            HealthStatus::Healthy
        } else if self.routing_order().iter().any(|r| self.is_region_healthy(r)) {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unhealthy
        }
    }
}

// ---------------------------------------------------------------------------
// Environment-based region detection
// ---------------------------------------------------------------------------

/// Detect the current region from common cloud environment variables.
///
/// Checks (in order): `AWS_REGION`, `AWS_DEFAULT_REGION`, `CLOUD_REGION`,
/// `FLY_REGION`.  Returns `None` if no variable is set or the value cannot
/// be mapped to a known [`Region`].
pub fn detect_region_from_env() -> Option<Region> {
    for var in &["AWS_REGION", "AWS_DEFAULT_REGION", "CLOUD_REGION", "FLY_REGION"] {
        if let Ok(val) = std::env::var(var) {
            if let Some(region) = parse_cloud_region(&val) {
                return Some(region);
            }
        }
    }
    None
}

/// Parse a cloud provider region string (e.g. `us-east-1`, `ewr`) into a
/// [`Region`].
fn parse_cloud_region(value: &str) -> Option<Region> {
    let lower = value.to_lowercase();
    if lower.contains("us-east") || lower.contains("iad") || lower.contains("ewr") {
        Some(Region::UsEast)
    } else if lower.contains("us-west") || lower.contains("lax") || lower.contains("sjc") || lower.contains("sea") {
        Some(Region::UsWest)
    } else if lower.contains("eu-west") || lower.contains("lhr") || lower.contains("dub") || lower.contains("cdg") {
        Some(Region::EuWest)
    } else if lower.contains("eu-central") || lower.contains("fra") {
        Some(Region::EuCentral)
    } else if lower.contains("ap-southeast") || lower.contains("sin") || lower.contains("sgp") {
        Some(Region::AsiaSoutheast)
    } else if lower.contains("ap-northeast") || lower.contains("ap-east") || lower.contains("nrt") || lower.contains("hnd") || lower.contains("hkg") {
        Some(Region::AsiaEast)
    } else if lower.contains("sa-east") || lower.contains("gru") {
        Some(Region::SouthAmerica)
    } else if lower.contains("ap-south") || lower.contains("syd") || lower.contains("mel") {
        Some(Region::Oceania)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Regional endpoint helpers
// ---------------------------------------------------------------------------

/// Known regional RPC endpoints for major providers.
///
/// These are convenience methods that return `(Region, String)` pairs suitable
/// for feeding into a [`GeoRouter`] once wrapped in a transport.
pub struct RegionalEndpoints;

impl RegionalEndpoints {
    /// Alchemy endpoints (Ethereum mainnet).
    pub fn alchemy(api_key: &str) -> Vec<(Region, String)> {
        vec![
            (Region::UsEast, format!("https://eth-mainnet.g.alchemy.com/v2/{api_key}")),
            (Region::EuCentral, format!("https://eth-mainnet.g.alchemy.com/v2/{api_key}")),
        ]
    }

    /// Ankr public endpoints (Ethereum mainnet).
    pub fn ankr() -> Vec<(Region, String)> {
        vec![
            (Region::UsEast, "https://rpc.ankr.com/eth".into()),
            (Region::EuWest, "https://rpc.ankr.com/eth".into()),
            (Region::AsiaSoutheast, "https://rpc.ankr.com/eth".into()),
        ]
    }

    /// Solana mainnet public RPC.
    pub fn public_solana() -> Vec<(Region, String)> {
        vec![
            (Region::UsEast, "https://api.mainnet-beta.solana.com".into()),
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RpcId;
    use std::sync::atomic::{AtomicU64, Ordering};

    // -- Mock transports ----------------------------------------------------

    /// A mock transport that always succeeds.
    struct SuccessTransport {
        name: String,
        call_count: AtomicU64,
    }

    impl SuccessTransport {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                call_count: AtomicU64::new(0),
            }
        }

        fn calls(&self) -> u64 {
            self.call_count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl RpcTransport for SuccessTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(serde_json::Value::String(format!("ok-{}", self.name))),
                error: None,
            })
        }

        fn url(&self) -> &str {
            &self.name
        }
    }

    /// A mock transport that always fails with a retryable error.
    struct FailTransport {
        name: String,
    }

    impl FailTransport {
        fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }

    #[async_trait]
    impl RpcTransport for FailTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Err(TransportError::Http("down".into()))
        }

        fn url(&self) -> &str {
            &self.name
        }
    }

    /// A mock transport that fails with a non-retryable error.
    struct NonRetryableFailTransport {
        name: String,
    }

    impl NonRetryableFailTransport {
        fn new(name: &str) -> Self {
            Self { name: name.to_string() }
        }
    }

    #[async_trait]
    impl RpcTransport for NonRetryableFailTransport {
        async fn send(&self, _req: JsonRpcRequest) -> Result<JsonRpcResponse, TransportError> {
            Err(TransportError::Other("permanent failure".into()))
        }

        fn url(&self) -> &str {
            &self.name
        }
    }

    fn make_request() -> JsonRpcRequest {
        JsonRpcRequest::new(1, "eth_blockNumber", vec![])
    }

    // -- Region tests -------------------------------------------------------

    #[test]
    fn region_proximity_us_east() {
        let order = Region::UsEast.proximity_order();
        assert_eq!(order[0], Region::UsWest, "UsEast's closest should be UsWest");
        assert_eq!(order.len(), 7, "should list all other regions");
        // Self must not appear in proximity list
        assert!(!order.contains(&Region::UsEast));
    }

    #[test]
    fn region_proximity_asia() {
        let order = Region::AsiaEast.proximity_order();
        assert_eq!(order[0], Region::AsiaSoutheast, "AsiaEast's closest should be AsiaSoutheast");
        assert!(!order.contains(&Region::AsiaEast));
    }

    #[test]
    fn region_display() {
        assert_eq!(Region::UsEast.as_str(), "us-east");
        assert_eq!(Region::EuCentral.as_str(), "eu-central");
        assert_eq!(Region::AsiaSoutheast.as_str(), "asia-southeast");
        assert_eq!(Region::Oceania.as_str(), "oceania");
        assert_eq!(Region::SouthAmerica.to_string(), "south-america");
    }

    #[test]
    fn region_from_str() {
        assert_eq!("us-east".parse::<Region>().unwrap(), Region::UsEast);
        assert_eq!("us_west".parse::<Region>().unwrap(), Region::UsWest);
        assert_eq!("eucentral".parse::<Region>().unwrap(), Region::EuCentral);
        assert_eq!("OCEANIA".parse::<Region>().unwrap(), Region::Oceania);
        assert!("mars".parse::<Region>().is_err());
    }

    // -- GeoRouter unit tests -----------------------------------------------

    #[test]
    fn routing_order_local_first() {
        let mut router = GeoRouter::new(Region::EuWest);
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));

        let order = router.routing_order();
        assert_eq!(order[0], Region::EuWest, "local region must be first");
    }

    #[test]
    fn routing_order_excludes_unconfigured() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));

        let order = router.routing_order();
        // Only configured regions should appear.
        for r in &order {
            assert!(
                *r == Region::UsEast || *r == Region::EuWest,
                "unexpected region {:?} in routing order",
                r,
            );
        }
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn provider_count_and_regions() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us2")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));

        assert_eq!(router.provider_count(), 3);

        let regions = router.regions();
        assert!(regions.contains(&Region::UsEast));
        assert!(regions.contains(&Region::EuWest));
        assert_eq!(regions.len(), 2);
    }

    // -- Async routing tests ------------------------------------------------

    #[tokio::test]
    async fn send_routes_to_local() {
        let us = Arc::new(SuccessTransport::new("us-east-1"));
        let eu = Arc::new(SuccessTransport::new("eu-west-1"));

        let us_ref = us.clone();
        let eu_ref = eu.clone();

        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, us);
        router.add_provider(Region::EuWest, eu);

        let resp = router.send(make_request()).await.unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result, serde_json::Value::String("ok-us-east-1".into()));

        assert_eq!(us_ref.calls(), 1, "local provider should have been called");
        assert_eq!(eu_ref.calls(), 0, "remote provider should NOT have been called");
    }

    #[tokio::test]
    async fn send_falls_back_on_failure() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(FailTransport::new("us-fail")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu-ok")));

        let resp = router.send(make_request()).await.unwrap();
        let result = resp.result.unwrap();
        assert_eq!(result, serde_json::Value::String("ok-eu-ok".into()));
    }

    #[tokio::test]
    async fn all_down_returns_error() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(FailTransport::new("us-fail")));
        router.add_provider(Region::EuWest, Arc::new(FailTransport::new("eu-fail")));

        let err = router.send(make_request()).await.unwrap_err();
        assert!(
            matches!(err, TransportError::AllProvidersDown),
            "expected AllProvidersDown, got {:?}",
            err,
        );
    }

    #[tokio::test]
    async fn non_retryable_error_propagates_immediately() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(
            Region::UsEast,
            Arc::new(NonRetryableFailTransport::new("us-perm-fail")),
        );
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu-ok")));

        let err = router.send(make_request()).await.unwrap_err();
        // Non-retryable error should NOT fall back — it propagates directly.
        assert!(
            matches!(err, TransportError::Other(_)),
            "expected Other error, got {:?}",
            err,
        );
    }

    #[tokio::test]
    async fn round_robin_within_region() {
        let a = Arc::new(SuccessTransport::new("a"));
        let b = Arc::new(SuccessTransport::new("b"));
        let a_ref = a.clone();
        let b_ref = b.clone();

        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, a);
        router.add_provider(Region::UsEast, b);

        // Send 4 requests; each provider should get 2.
        for _ in 0..4 {
            router.send(make_request()).await.unwrap();
        }

        assert_eq!(a_ref.calls(), 2, "provider A should get half the requests");
        assert_eq!(b_ref.calls(), 2, "provider B should get half the requests");
    }

    #[test]
    fn health_summary_reports() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us2")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));

        let summary = router.health_summary();
        assert_eq!(summary.len(), 2, "should have summaries for 2 regions");

        for entry in &summary {
            assert!(entry.healthy);
            assert!(
                entry.region == "us-east" || entry.region == "eu-west",
                "unexpected region: {}",
                entry.region,
            );
            if entry.region == "us-east" {
                assert_eq!(entry.provider_count, 2);
            } else {
                assert_eq!(entry.provider_count, 1);
            }
        }
    }

    #[tokio::test]
    async fn health_status_reflects_region_state() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));

        // Before any requests, everything is healthy.
        assert_eq!(router.health(), HealthStatus::Healthy);
    }

    // -- Environment detection tests ----------------------------------------

    #[test]
    fn detect_region_aws() {
        let result = parse_cloud_region("us-east-1");
        assert_eq!(result, Some(Region::UsEast));

        let result = parse_cloud_region("eu-west-1");
        assert_eq!(result, Some(Region::EuWest));

        let result = parse_cloud_region("ap-southeast-1");
        assert_eq!(result, Some(Region::AsiaSoutheast));

        let result = parse_cloud_region("sa-east-1");
        assert_eq!(result, Some(Region::SouthAmerica));
    }

    #[test]
    fn detect_region_fly() {
        assert_eq!(parse_cloud_region("ewr"), Some(Region::UsEast));
        assert_eq!(parse_cloud_region("lax"), Some(Region::UsWest));
        assert_eq!(parse_cloud_region("lhr"), Some(Region::EuWest));
        assert_eq!(parse_cloud_region("fra"), Some(Region::EuCentral));
        assert_eq!(parse_cloud_region("sin"), Some(Region::AsiaSoutheast));
        assert_eq!(parse_cloud_region("nrt"), Some(Region::AsiaEast));
        assert_eq!(parse_cloud_region("gru"), Some(Region::SouthAmerica));
        assert_eq!(parse_cloud_region("syd"), Some(Region::Oceania));
    }

    #[test]
    fn detect_region_unknown() {
        assert_eq!(parse_cloud_region("mars-north-1"), None);
        assert_eq!(parse_cloud_region(""), None);
    }

    // -- Regional endpoints helpers -----------------------------------------

    #[test]
    fn alchemy_endpoints() {
        let eps = RegionalEndpoints::alchemy("test-key");
        assert_eq!(eps.len(), 2);
        assert!(eps[0].1.contains("test-key"));
    }

    #[test]
    fn ankr_endpoints() {
        let eps = RegionalEndpoints::ankr();
        assert_eq!(eps.len(), 3);
        for (_, url) in &eps {
            assert!(url.starts_with("https://rpc.ankr.com"));
        }
    }

    // -- RegionalHealth internal tests --------------------------------------

    #[test]
    fn regional_health_starts_healthy() {
        let h = RegionalHealth::new();
        assert!(h.healthy);
        assert_eq!(h.success_count, 0);
        assert_eq!(h.failure_count, 0);
    }

    #[test]
    fn regional_health_success_rate() {
        let mut h = RegionalHealth::new();
        h.record_success(Duration::from_millis(10));
        h.record_success(Duration::from_millis(20));
        h.record_failure();
        // 2/3 success rate
        assert!(h.success_rate() > 0.6);
        assert!(h.healthy);
    }

    #[test]
    fn regional_health_becomes_unhealthy() {
        let mut h = RegionalHealth::new();
        // All failures — should become unhealthy.
        for _ in 0..5 {
            h.record_failure();
        }
        assert!(!h.healthy);
        assert_eq!(h.failure_count, 5);
    }

    #[tokio::test]
    async fn unhealthy_region_is_skipped() {
        let mut router = GeoRouter::new(Region::UsEast);
        router.add_provider(Region::UsEast, Arc::new(SuccessTransport::new("us1")));
        router.add_provider(Region::EuWest, Arc::new(SuccessTransport::new("eu1")));

        // Manually mark UsEast as unhealthy by recording many failures.
        {
            let mut health = router.health.lock().unwrap();
            let entry = health.entry(Region::UsEast).or_insert_with(RegionalHealth::new);
            for _ in 0..10 {
                entry.record_failure();
            }
        }

        let resp = router.send(make_request()).await.unwrap();
        let result = resp.result.unwrap();
        // Should have fallen back to EU since US is unhealthy.
        assert_eq!(result, serde_json::Value::String("ok-eu1".into()));
    }

    #[test]
    fn geo_router_url() {
        let router = GeoRouter::new(Region::UsEast);
        assert_eq!(router.url(), "geo-router");
    }

    #[test]
    fn local_region_accessor() {
        let router = GeoRouter::new(Region::AsiaSoutheast);
        assert_eq!(router.local_region(), Region::AsiaSoutheast);
    }
}
