//! # Token Bucket Rate Limiting
//!
//! Demonstrates the ChainRPC rate-limiting primitives:
//!
//! - **`RateLimiter`** — a simple token-bucket that consumes a fixed default cost
//!   per request. Good for providers that charge a flat rate per call.
//! - **`MethodAwareRateLimiter`** — wraps a token bucket with a `CuCostTable` so
//!   each RPC method automatically drains the correct number of compute units (CU).
//!   Expensive methods like `eth_getLogs` (75 CU) drain the bucket much faster than
//!   cheap ones like `eth_blockNumber` (10 CU).
//!
//! The Alchemy default cost table is used here, but you can build a custom one with
//! `CuCostTable::new(default)` and `set_cost()`.

use chainrpc_core::cu_tracker::CuCostTable;
use chainrpc_core::policy::rate_limiter::{
    MethodAwareRateLimiter, RateLimiter, RateLimiterConfig,
};

#[tokio::main]
async fn main() {
    // ---------------------------------------------------------------
    // 1. Basic RateLimiter — flat cost per request
    // ---------------------------------------------------------------
    // Capacity of 10 tokens, refilling at 2 tokens/second.
    let config = RateLimiterConfig {
        capacity: 10.0,
        refill_rate: 2.0,
    };
    let limiter = RateLimiter::new(config);

    // Drain the bucket one token at a time (default_cost = 1.0).
    let mut accepted = 0;
    for _ in 0..15 {
        if limiter.try_acquire() {
            accepted += 1;
        }
    }
    println!("[RateLimiter] accepted {accepted}/15 requests (capacity=10)");
    // Expected: 10 accepted, 5 rejected.

    // Check how long we need to wait before the next request is possible.
    let wait = limiter.wait_time();
    println!("[RateLimiter] wait_time for next request: {wait:?}");
    // At 2 tokens/sec the wait for 1 token is ~500ms.

    // ---------------------------------------------------------------
    // 2. MethodAwareRateLimiter — CU-weighted costs
    // ---------------------------------------------------------------
    // Use Alchemy's default compute-unit cost table.
    let cost_table = CuCostTable::alchemy_defaults();

    // Print a few notable CU costs for reference.
    println!("\n--- Alchemy CU costs ---");
    for method in [
        "eth_blockNumber",          // 10 CU
        "eth_getBalance",           // 19 CU
        "eth_call",                 // 26 CU
        "eth_getLogs",              // 75 CU
        "eth_sendRawTransaction",   // 250 CU
        "debug_traceTransaction",   // 309 CU
    ] {
        println!("  {method:30} = {} CU", cost_table.cost_for(method));
    }

    // Create a method-aware limiter with 300 CU capacity and near-zero refill
    // so we can observe the drain without time-based refills muddying the picture.
    let ma_limiter = MethodAwareRateLimiter::new(
        RateLimiterConfig {
            capacity: 300.0,
            refill_rate: 0.001, // near-zero refill for demo clarity
        },
        cost_table,
    );

    // -- eth_getLogs costs 75 CU each: fits 4 times in 300 CU budget --
    println!("\n--- eth_getLogs (75 CU each) ---");
    let mut logs_count = 0;
    for i in 1..=6 {
        let ok = ma_limiter.try_acquire_method("eth_getLogs");
        println!("  call #{i}: {}", if ok { "accepted" } else { "RATE LIMITED" });
        if ok {
            logs_count += 1;
        }
    }
    println!("  => {logs_count} eth_getLogs calls accepted before limit");
    // Expected: 4 accepted (4 * 75 = 300), 5th and 6th rejected.

    // Show wait time — tells the caller how long to sleep before retrying.
    let wait_logs = ma_limiter.wait_time_for_method("eth_getLogs");
    println!("  wait_time for next eth_getLogs: {wait_logs:?}");

    // -- Fresh bucket for cheap method demo --
    let ma_limiter2 = MethodAwareRateLimiter::new(
        RateLimiterConfig {
            capacity: 300.0,
            refill_rate: 0.001,
        },
        CuCostTable::alchemy_defaults(),
    );

    // -- eth_blockNumber costs 10 CU each: fits 30 times in 300 CU budget --
    println!("\n--- eth_blockNumber (10 CU each) ---");
    let mut bn_count = 0;
    for _ in 0..35 {
        if ma_limiter2.try_acquire_method("eth_blockNumber") {
            bn_count += 1;
        }
    }
    println!("  => {bn_count} eth_blockNumber calls accepted before limit");
    // Expected: 30 accepted (30 * 10 = 300), remaining 5 rejected.

    // ---------------------------------------------------------------
    // 3. Comparing drain rates side by side
    // ---------------------------------------------------------------
    println!("\n--- Side-by-side drain comparison (150 CU bucket) ---");
    let table = CuCostTable::alchemy_defaults();

    // Same capacity, same refill — only the method cost differs.
    let bucket_expensive = MethodAwareRateLimiter::new(
        RateLimiterConfig {
            capacity: 150.0,
            refill_rate: 0.001,
        },
        table.clone(),
    );
    let bucket_cheap = MethodAwareRateLimiter::new(
        RateLimiterConfig {
            capacity: 150.0,
            refill_rate: 0.001,
        },
        table,
    );

    let mut expensive_calls = 0;
    while bucket_expensive.try_acquire_method("eth_getLogs") {
        expensive_calls += 1;
    }

    let mut cheap_calls = 0;
    while bucket_cheap.try_acquire_method("eth_blockNumber") {
        cheap_calls += 1;
    }

    println!("  eth_getLogs    (75 CU): {expensive_calls} calls fit in 150 CU");
    println!("  eth_blockNumber (10 CU): {cheap_calls} calls fit in 150 CU");
    println!(
        "  Ratio: eth_blockNumber is {:.1}x cheaper",
        cheap_calls as f64 / expensive_calls as f64
    );
    // Expected: 2 vs 15 => 7.5x ratio.

    // ---------------------------------------------------------------
    // 4. Accessing the underlying bucket for fine-grained control
    // ---------------------------------------------------------------
    let custom = MethodAwareRateLimiter::new(
        RateLimiterConfig {
            capacity: 100.0,
            refill_rate: 50.0, // 50 CU/sec refill
        },
        CuCostTable::alchemy_defaults(),
    );
    let avail = custom.bucket().available();
    println!("\n[bucket] available tokens: {avail}");
    // Should be ~100.0 (full capacity, just created).
}
