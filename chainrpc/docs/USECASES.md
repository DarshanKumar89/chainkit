# ChainRPC — Use Cases & Patterns

Real-world scenarios and how to solve them with ChainRPC.

---

## 1. Simple RPC Client with Retry

**Scenario**: You need a reliable HTTP client that retries on transient failures.

```rust
use chainrpc_http::{HttpRpcClient, HttpClientConfig};
use chainrpc_core::transport::RpcTransport;
use chainrpc_core::request::JsonRpcRequest;

let client = HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY");

// Simple typed call
let block_number: String = client.call(1, "eth_blockNumber", vec![]).await?;
println!("Block: {block_number}");

// Or raw request/response
let req = JsonRpcRequest::auto("eth_chainId", vec![]);
let resp = client.send(req).await?;
```

The client automatically:
- Retries up to 3 times with exponential backoff on transient errors
- Skips retries for write methods (`eth_sendRawTransaction`)
- Tracks circuit breaker state
- Respects rate limits

---

## 2. Multi-Provider Failover

**Scenario**: You have 3 RPC providers and want automatic failover if one goes down.

```rust
use chainrpc_http::pool_from_urls;

let pool = pool_from_urls(&[
    "https://eth-mainnet.g.alchemy.com/v2/KEY1",
    "https://mainnet.infura.io/v3/KEY2",
    "https://rpc.ankr.com/eth",
])?;

// Requests are distributed round-robin across healthy providers.
// If one provider's circuit breaker opens (5 consecutive failures),
// it's skipped automatically until the cooldown period passes.
let resp = pool.send(JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;

// Check pool health
println!("Healthy providers: {}/{}", pool.healthy_count(), pool.len());
for report in pool.health_report() {
    println!("{}", serde_json::to_string_pretty(&report)?);
}
```

---

## 3. Response Caching with Reorg Safety

**Scenario**: You're querying the same block data repeatedly and want caching, but need to handle chain reorgs.

```rust
use chainrpc_core::cache::{CacheTransport, CacheConfig, CacheTierResolver};

let cache = CacheTransport::new(
    Arc::new(client),
    CacheConfig {
        tier_resolver: Some(CacheTierResolver::new()),
        max_entries: 4096,
        ..Default::default()
    },
);

// eth_getTransactionReceipt → Immutable tier (1 hour TTL)
// eth_blockNumber → Volatile tier (2 second TTL)
// eth_sendRawTransaction → NeverCache (always passes through)
// eth_getBlockByNumber("latest") → Volatile
// eth_getBlockByNumber("0x100") → Immutable

let resp = cache.send(JsonRpcRequest::auto("eth_getTransactionReceipt", vec![
    serde_json::json!("0xabc123...")
])).await?;

// On reorg detection — invalidate affected blocks
cache.invalidate_for_reorg(19_500_000);
// ^ Removes all cached entries referencing block >= 19,500,000
// Keeps eth_chainId, net_version, etc. (no block_ref)
```

---

## 4. Request Deduplication

**Scenario**: Multiple parts of your app call `eth_blockNumber` simultaneously. You want one RPC call, shared across all callers.

```rust
use chainrpc_core::dedup::DedupTransport;

let dedup = DedupTransport::new(Arc::new(client));

// These 3 concurrent calls produce only 1 actual RPC request:
let (r1, r2, r3) = tokio::join!(
    dedup.send(JsonRpcRequest::auto("eth_blockNumber", vec![])),
    dedup.send(JsonRpcRequest::auto("eth_blockNumber", vec![])),
    dedup.send(JsonRpcRequest::auto("eth_blockNumber", vec![])),
);
// All 3 get the same response. Only 1 HTTP call was made.
```

---

## 5. CU Budget Tracking (Alchemy/Infura)

**Scenario**: You're on Alchemy's free tier (300M CU/month) and want to track consumption and throttle before hitting the limit.

```rust
use chainrpc_core::cu_tracker::{CuTracker, CuCostTable, CuBudgetConfig};

let tracker = CuTracker::new(
    "https://eth-mainnet.g.alchemy.com/v2/KEY",
    CuCostTable::alchemy_defaults(),
    CuBudgetConfig {
        monthly_budget: 300_000_000, // 300M CU
        alert_threshold: 0.8,        // alert at 80%
        throttle_near_limit: true,    // auto-throttle when near limit
    },
);

// Before each request:
tracker.record("eth_getLogs");  // records 75 CU

// Check budget status
println!("Used: {} CU", tracker.consumed());
println!("Remaining: {} CU", tracker.remaining());
println!("Usage: {:.1}%", tracker.usage_fraction() * 100.0);

if tracker.should_throttle() {
    // Slow down or switch to a different provider
}

// Per-method breakdown for cost optimization
for (method, cu) in tracker.per_method_usage() {
    println!("  {method}: {cu} CU");
}
```

---

## 6. CU-Aware Rate Limiting

**Scenario**: You want rate limiting that understands that `eth_getLogs` (75 CU) costs 7.5x more than `eth_blockNumber` (10 CU).

```rust
use chainrpc_core::policy::{MethodAwareRateLimiter, RateLimiterConfig};
use chainrpc_core::cu_tracker::CuCostTable;

let limiter = MethodAwareRateLimiter::new(
    RateLimiterConfig {
        capacity: 300.0,    // 300 CU bucket
        refill_rate: 300.0, // 300 CU/sec (Alchemy default)
    },
    CuCostTable::alchemy_defaults(),
);

// eth_blockNumber costs 10 CU — fits 30 times in the bucket
if limiter.try_acquire_method("eth_blockNumber") {
    // Send request
}

// eth_getLogs costs 75 CU — fits only 4 times in the bucket
if !limiter.try_acquire_method("eth_getLogs") {
    let wait = limiter.wait_time_for_method("eth_getLogs");
    tokio::time::sleep(wait).await;
}
```

---

## 7. Multi-Chain Application

**Scenario**: Your app queries Ethereum, Polygon, and Arbitrum simultaneously.

```rust
use chainrpc_core::multi_chain::ChainRouter;

let mut router = ChainRouter::new();
router.add_chain(1, Arc::new(eth_client));      // Ethereum
router.add_chain(137, Arc::new(polygon_client)); // Polygon
router.add_chain(42161, Arc::new(arb_client));   // Arbitrum

// Route to specific chain
let eth_block = router.send_to(1, JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;
let poly_block = router.send_to(137, JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;

// Parallel cross-chain queries
let results = router.parallel(vec![
    (1, JsonRpcRequest::auto("eth_blockNumber", vec![])),
    (137, JsonRpcRequest::auto("eth_blockNumber", vec![])),
    (42161, JsonRpcRequest::auto("eth_blockNumber", vec![])),
]).await;
// All 3 run concurrently via futures::future::join_all

// Health across all chains
for (chain_id, health) in router.health_summary() {
    println!("Chain {chain_id}: {health}");
}
```

---

## 8. MEV Protection

**Scenario**: Your bot submits swaps and you want to protect against sandwich attacks.

```rust
use chainrpc_core::mev::{is_mev_susceptible, should_use_relay, MevConfig, relay_urls};

let calldata = "0x38ed1739..."; // swapExactTokensForTokens

if is_mev_susceptible(calldata) {
    println!("This transaction is MEV-susceptible!");

    let config = MevConfig {
        enabled: true,
        relay_url: Some("https://relay.flashbots.net".into()),
        auto_detect: true,
    };

    if should_use_relay(calldata, &config) {
        // Route to Flashbots relay instead of public mempool
        let relays = relay_urls();
        println!("Sending via private relay: {}", relays[0]);
    }
}
```

Detects 12 known MEV-susceptible function selectors (Uniswap V2/V3 swaps, WETH deposit/withdraw).

---

## 9. EIP-1559 Gas Estimation

**Scenario**: You need gas price recommendations for different speed tiers.

```rust
use chainrpc_core::gas::{compute_gas_recommendation, GasSpeed};

// Get fee history from the node
let base_fee: u64 = 30_000_000_000; // 30 gwei
let priority_fee_samples = vec![
    1_000_000_000, 1_500_000_000, 2_000_000_000,
    2_500_000_000, 3_000_000_000, 5_000_000_000,
];

let fast = compute_gas_recommendation(base_fee, &priority_fee_samples, GasSpeed::Fast);
println!("Fast: max_fee={}wei, priority={}wei", fast.max_fee_per_gas, fast.max_priority_fee_per_gas);

let slow = compute_gas_recommendation(base_fee, &priority_fee_samples, GasSpeed::Slow);
println!("Slow: max_fee={}wei, priority={}wei", slow.max_fee_per_gas, slow.max_priority_fee_per_gas);

// Base fee multipliers:
// Slow     = 1.0x  base fee
// Standard = 1.125x base fee
// Fast     = 1.25x base fee
// Urgent   = 1.5x  base fee
```

---

## 10. Request Hedging (Latency-Sensitive)

**Scenario**: You need the lowest possible latency for a read call and have two providers.

```rust
use chainrpc_core::hedging::hedged_send;

let req = JsonRpcRequest::auto("eth_getBalance", vec![
    serde_json::json!("0xdead..."),
    serde_json::json!("latest"),
]);

// Fires primary immediately, starts backup after 100ms delay.
// Returns whichever responds first.
let resp = hedged_send(
    &primary_client,
    &backup_client,
    req,
    Duration::from_millis(100),
).await?;

// Only hedges safe (read) methods.
// eth_sendRawTransaction goes to primary only — no hedging.
```

---

## 11. Archive Node Routing

**Scenario**: You have a mix of full and archive nodes and need historical queries to go to the right node.

```rust
use chainrpc_core::routing::{ProviderCapabilities, analyze_request, select_capable_provider};

let providers = vec![
    (0, ProviderCapabilities {
        archive: false, trace: false,
        max_block_range: 10_000, max_batch_size: 100,
        supported_methods: HashSet::new(),
    }),
    (1, ProviderCapabilities {
        archive: true, trace: true,
        max_block_range: 100_000, max_batch_size: 1000,
        supported_methods: HashSet::new(),
    }),
];

let req = analyze_request("eth_getBalance", &[
    serde_json::json!("0xdead..."),
    serde_json::json!("0x100"),  // block 256 — historical!
]);

let idx = select_capable_provider(
    &providers.iter().map(|(i, c)| (*i, c)).collect::<Vec<_>>(),
    &req,
);
// idx = Some(1) — routes to the archive node
```

---

## 12. Transaction Lifecycle Management

**Scenario**: You submit a transaction and want to monitor it until confirmation with stuck detection.

```rust
use chainrpc_core::tx::{TxTracker, TxTrackerConfig, ReceiptPoller, ReceiptPollerConfig};
use chainrpc_core::tx_lifecycle::{send_and_track, poll_receipt, detect_stuck};

let tracker = TxTracker::new(TxTrackerConfig {
    confirmation_depth: 12,
    stuck_timeout_secs: 300,
    ..Default::default()
});

// Send and auto-track
let tx_hash = send_and_track(
    &client, &tracker,
    "0xf86c...",   // raw signed tx
    "0xAlice",     // sender
    42,            // nonce
).await?;

// Poll for receipt with exponential backoff
let poller = ReceiptPoller::new(ReceiptPollerConfig::default());
let receipt = poll_receipt(&client, &tx_hash, &poller).await?;

// Check for stuck transactions
let current_time = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
let stuck_txs = detect_stuck(&client, &tracker, current_time).await;
for tx in stuck_txs {
    println!("Stuck tx: {} (pending for {}s)", tx.tx_hash,
        current_time - tx.submitted_at);
}
```

---

## 13. Graceful Shutdown

**Scenario**: Your service needs to drain in-flight requests on SIGTERM before shutting down.

```rust
use chainrpc_core::shutdown::{ShutdownController, install_signal_handler, shutdown_with_timeout};

let (controller, signal) = ShutdownController::new();
let ctrl = Arc::new(controller);

// Install SIGTERM/SIGINT handler
install_signal_handler(ctrl.clone());

// In your main loop:
let mut sig = signal.clone();
tokio::select! {
    _ = main_loop() => {},
    _ = sig.wait() => {
        println!("Shutting down...");
    }
}

// Drain with timeout
let drained = shutdown_with_timeout(ctrl, Duration::from_secs(30)).await;
if !drained {
    println!("Forced shutdown — some requests may have been dropped");
}
```

---

## 14. Backpressure / Load Shedding

**Scenario**: You want to reject new requests when too many are already in flight.

```rust
use chainrpc_core::backpressure::BackpressureTransport;

let bp = BackpressureTransport::new(Arc::new(client), 100); // max 100 concurrent

match bp.send(req).await {
    Ok(resp) => { /* success */ }
    Err(TransportError::Overloaded { queue_depth }) => {
        // 100 requests already in flight — shed load
        println!("Overloaded! {queue_depth} in flight, returning 503");
    }
    Err(e) => { /* other error */ }
}

// Observability
println!("In-flight: {}, Full: {}", bp.in_flight(), bp.is_full());
```

---

## 15. Prometheus Metrics Export

**Scenario**: You want to expose RPC metrics to your monitoring stack.

```rust
use chainrpc_core::metrics::{RpcMetrics, ProviderMetrics};

let m1 = Arc::new(ProviderMetrics::new("alchemy"));
let m2 = Arc::new(ProviderMetrics::new("infura"));

// Record metrics (done automatically by HttpRpcClient/ProviderPool when using with_metrics)
m1.record_success(Duration::from_millis(45));
m1.record_success(Duration::from_millis(52));
m2.record_failure();

// Aggregate + export
let rpc_metrics = RpcMetrics::new(vec![m1, m2]);
let prometheus_text = rpc_metrics.prometheus_export();
// Returns standard Prometheus text format:
//   chainrpc_requests_total{provider="alchemy"} 2
//   chainrpc_requests_successful{provider="alchemy"} 2
//   chainrpc_latency_avg_ms{provider="alchemy"} 48.5
//   chainrpc_requests_total{provider="infura"} 1
//   chainrpc_requests_failed{provider="infura"} 1
//   ...

// Serve at /metrics endpoint in your HTTP server
```

---

## 16. Cancellation Tokens

**Scenario**: You want to cancel a long-running operation (e.g., batch indexing) from another task.

```rust
use chainrpc_core::cancellation::CancellationToken;

let token = CancellationToken::new();
let child = token.child(); // for the worker task

// Worker task
tokio::spawn(async move {
    loop {
        if child.is_cancelled() { break; }
        // ... do work ...
    }
});

// Cancel from parent after timeout
tokio::time::sleep(Duration::from_secs(60)).await;
token.cancel(); // child.is_cancelled() becomes true

// Or async wait:
// child.cancelled().await;  // resolves when token.cancel() is called
```

---

## Composing Layers

The real power of ChainRPC is stacking these layers. Here's a production-grade setup:

```rust
use std::sync::Arc;

// 1. Create HTTP clients
let alchemy = Arc::new(HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/KEY"));
let infura = Arc::new(HttpRpcClient::default_for("https://mainnet.infura.io/v3/KEY"));

// 2. Pool with failover
let pool: Arc<dyn RpcTransport> = Arc::new(
    ProviderPool::new_with_metrics(vec![alchemy, infura], ProviderPoolConfig::default())
);

// 3. Add backpressure
let bp = Arc::new(BackpressureTransport::new(pool, 200));

// 4. Add caching with tiered TTL
let cached = CacheTransport::new(bp, CacheConfig {
    tier_resolver: Some(CacheTierResolver::new()),
    max_entries: 8192,
    ..Default::default()
});

// 5. Add request deduplication
let dedup = DedupTransport::new(Arc::new(cached));

// 6. Add auto-batching
let batched = BatchingTransport::new(Arc::new(dedup), Duration::from_millis(5));

// Now: dedup → cache → backpressure → pool → (alchemy | infura)
//       with retry, circuit breaker, rate limiter built into each HTTP client
```

---

## 17. Solana RPC with Commitment Levels

**Scenario**: You need to query Solana with specific commitment levels, automatically injected into every request.

```rust
use chainrpc_core::solana::{SolanaTransport, SolanaCommitment, classify_solana_method, SolanaCuCostTable};
use chainrpc_http::HttpRpcClient;

let http = Arc::new(HttpRpcClient::default_for("https://api.mainnet-beta.solana.com"));

// Wrap with Solana commitment — every request gets commitment injected
let solana = SolanaTransport::new(http, SolanaCommitment::Finalized);

// getSlot — commitment "finalized" auto-injected into params
let slot: String = solana.call(1, "getSlot", vec![]).await?;
println!("Finalized slot: {slot}");

// getAccountInfo — commitment config merged into params object
let account = solana.call(1, "getAccountInfo", vec![
    serde_json::json!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
]).await?;

// Method safety classification (50+ Solana methods)
let safety = classify_solana_method("getBalance");         // Safe
let safety2 = classify_solana_method("sendTransaction");   // Idempotent
let safety3 = classify_solana_method("requestAirdrop");    // Unsafe

// Solana-specific CU costs
let cu_table = SolanaCuCostTable::defaults();
println!("getProgramAccounts costs {} CU", cu_table.cost_for("getProgramAccounts")); // 100
println!("getSlot costs {} CU", cu_table.cost_for("getSlot"));                       // 5
```

---

## 18. Geographic Load Balancing

**Scenario**: You have RPC providers in multiple regions and want requests routed to the closest one with automatic fallback.

```rust
use chainrpc_core::geo_routing::{GeoRouter, Region, RegionalEndpoints, detect_region_from_env};
use chainrpc_http::HttpRpcClient;

// Auto-detect region from cloud provider env vars (AWS_REGION, FLY_REGION, etc.)
let local_region = detect_region_from_env().unwrap_or(Region::UsEast);

// Set up providers across regions
let us_east = Arc::new(HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/KEY"));
let eu_west = Arc::new(HttpRpcClient::default_for("https://eth-mainnet-eu.g.alchemy.com/v2/KEY"));
let asia = Arc::new(HttpRpcClient::default_for("https://eth-mainnet-asia.g.alchemy.com/v2/KEY"));

let router = GeoRouter::new(local_region, vec![
    (Region::UsEast, us_east),
    (Region::EuWest, eu_west),
    (Region::AsiaEast, asia),
]);

// Requests go to the closest region automatically
// If local region fails → falls back to next-closest by proximity table
let block = router.send(JsonRpcRequest::auto("eth_blockNumber", vec![])).await?;

// Health summary per region
for summary in router.health_summary() {
    println!("Region {:?}: latency={:?}ms, success={}, failures={}",
        summary.region, summary.avg_latency_ms, summary.success_count, summary.failure_count);
}

// Or use pre-configured regional endpoints
let endpoints = RegionalEndpoints::alchemy("YOUR_KEY");
// Returns Vec<(Region, String)> for all Alchemy regional URLs
```

---

## 19. Speeding Up Stuck Transactions

**Scenario**: A transaction is stuck in the mempool and you need to speed it up or cancel it with EIP-1559 compliant gas replacement.

```rust
use chainrpc_core::gas_bumper::{compute_bump, bump_and_send, compute_cancel, BumpStrategy, BumpConfig};
use chainrpc_core::tx::TxTracker;

let config = BumpConfig {
    min_bump_bps: 1000,                // 10% minimum increase (EIP-1559 rule)
    max_gas_price: 500_000_000_000,    // 500 gwei cap
    max_bumps: 5,                       // max 5 replacement attempts
};

// Strategy 1: Percentage bump (12% increase)
let result = compute_bump(
    30_000_000_000,  // current max_fee (30 gwei)
    2_000_000_000,   // current priority_fee (2 gwei)
    &BumpStrategy::Percentage(1200),
    &config,
)?;
println!("New max_fee: {} wei, new priority: {} wei", result.new_max_fee, result.new_priority_fee);

// Strategy 2: Double gas
let result2 = compute_bump(30_000_000_000, 2_000_000_000, &BumpStrategy::Double, &config)?;
// max_fee = 60 gwei, priority = 4 gwei

// Strategy 3: Speed tier from fee history
let result3 = compute_bump(30_000_000_000, 2_000_000_000,
    &BumpStrategy::SpeedTier(GasSpeed::Urgent), &config)?;

// Full async flow: compute bump, sign, send replacement, update tracker
let tracker = TxTracker::new(TxTrackerConfig::default());
let new_hash = bump_and_send(
    &client,
    &tracker,
    "0xstuck_tx_hash...",
    &BumpStrategy::Percentage(1200),
    &config,
    |max_fee, priority_fee| {
        // Your signing logic here — returns raw signed tx bytes
        sign_replacement_tx(max_fee, priority_fee)
    },
).await?;
println!("Replacement tx: {new_hash}");

// Cancel a stuck tx (minimum bump, 0-value self-transfer)
let cancel = compute_cancel(30_000_000_000, 2_000_000_000, &config)?;
println!("Cancel with max_fee={}, priority={}", cancel.new_max_fee, cancel.new_priority_fee);
```

---

## 20. Reorg Detection & Cache Safety

**Scenario**: You need to detect chain reorganizations at the RPC layer and invalidate any cached data for affected blocks.

```rust
use chainrpc_core::reorg::{ReorgDetector, ReorgConfig};
use chainrpc_core::cache::CacheTransport;

let detector = ReorgDetector::new(ReorgConfig {
    window_size: 128,          // track last 128 block hashes
    safe_depth: 64,            // blocks older than tip-64 are "safe"
    use_finalized_tag: false,  // set true for PoS chains with finalized tag
});

// Register callback for reorg events
let cache_clone = cache.clone();
detector.on_reorg(move |event| {
    println!("REORG detected at block {}! depth={}, old={}, new={}",
        event.fork_block, event.depth, event.old_hash, event.new_hash);

    // Invalidate cached data for affected blocks
    cache_clone.invalidate_for_reorg(event.fork_block);
});

// In your block processing loop:
loop {
    // Poll the chain and check for reorgs in one call
    match detector.poll_and_check(&client).await? {
        Some(event) => {
            // Reorg detected — callbacks already fired
            // Re-process blocks from event.fork_block
            println!("Re-processing from block {}", event.fork_block);
        }
        None => {
            // No reorg — process normally
        }
    }

    // Only trust blocks beyond safe depth
    let safe = detector.safe_block();
    println!("Safe to index up to block {safe}");

    // For PoS chains, query the finalized block directly
    // let finalized = detector.fetch_finalized_block(&client).await?;

    tokio::time::sleep(Duration::from_secs(12)).await;
}
```
