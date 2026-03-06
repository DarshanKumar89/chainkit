//! # Compute Unit Budget Tracking
//!
//! Demonstrates `CuTracker` — a per-provider compute-unit budget tracker that
//! monitors monthly CU consumption and triggers alerts or throttling when the
//! budget is nearly exhausted.
//!
//! Key APIs shown:
//!
//! - `CuTracker::record()` — log CU consumption for each RPC method
//! - `consumed()` / `remaining()` — running totals
//! - `usage_fraction()` — 0.0..1.0 progress toward the budget cap
//! - `is_alert()` — true when `usage_fraction >= alert_threshold` (default 80%)
//! - `should_throttle()` — true when alert fires AND `throttle_near_limit` is on
//! - `per_method_usage()` — breakdown of CU spent per method name
//! - `snapshot()` — serializable struct for dashboards / logging

use chainrpc_core::cu_tracker::{CuBudgetConfig, CuCostTable, CuSnapshot, CuTracker};

#[tokio::main]
async fn main() {
    // ---------------------------------------------------------------
    // 1. Create a CuTracker with a 10 000 CU monthly budget
    // ---------------------------------------------------------------
    let tracker = CuTracker::new(
        "https://eth-mainnet.g.alchemy.com/v2/MY_KEY",
        CuCostTable::alchemy_defaults(),
        CuBudgetConfig {
            monthly_budget: 10_000,
            alert_threshold: 0.8,      // alert at 80%
            throttle_near_limit: true,  // start throttling when alert fires
        },
    );

    println!("Provider : {}", tracker.url());
    println!("Budget   : 10 000 CU/month");
    println!("Alert at : 80%");
    println!();

    // ---------------------------------------------------------------
    // 2. Simulate a burst of mixed RPC calls
    // ---------------------------------------------------------------
    // 50 x eth_blockNumber (10 CU each)   =   500 CU
    for _ in 0..50 {
        tracker.record("eth_blockNumber");
    }

    // 20 x eth_call (26 CU each)          =   520 CU
    for _ in 0..20 {
        tracker.record("eth_call");
    }

    // 40 x eth_getLogs (75 CU each)        = 3 000 CU
    for _ in 0..40 {
        tracker.record("eth_getLogs");
    }

    // 10 x eth_getTransactionReceipt (15 CU each) = 150 CU
    for _ in 0..10 {
        tracker.record("eth_getTransactionReceipt");
    }

    // Total so far: 500 + 520 + 3000 + 150 = 4 170 CU
    println!("--- After initial burst ---");
    println!("  consumed      : {} CU", tracker.consumed());
    println!("  remaining     : {} CU", tracker.remaining());
    println!(
        "  usage_fraction: {:.1}%",
        tracker.usage_fraction() * 100.0
    );
    println!("  is_alert      : {}", tracker.is_alert());
    println!("  should_throttle: {}", tracker.should_throttle());
    // 4170 / 10000 = 41.7% — well below the 80% threshold.

    // ---------------------------------------------------------------
    // 3. Push past the 80% alert threshold
    // ---------------------------------------------------------------
    // Add 60 more eth_getLogs calls: 60 * 75 = 4 500 CU => total 8 670 CU (86.7%)
    for _ in 0..60 {
        tracker.record("eth_getLogs");
    }

    println!("\n--- After heavy eth_getLogs usage ---");
    println!("  consumed      : {} CU", tracker.consumed());
    println!("  remaining     : {} CU", tracker.remaining());
    println!(
        "  usage_fraction: {:.1}%",
        tracker.usage_fraction() * 100.0
    );
    println!("  is_alert      : {}", tracker.is_alert());       // true (>80%)
    println!("  should_throttle: {}", tracker.should_throttle()); // true
    println!("  is_exhausted  : {}", tracker.is_exhausted());    // false (still < 100%)

    // ---------------------------------------------------------------
    // 4. Per-method breakdown
    // ---------------------------------------------------------------
    println!("\n--- Per-method CU breakdown ---");
    let breakdown = tracker.per_method_usage();
    // Sort by CU descending for readability.
    let mut sorted: Vec<_> = breakdown.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    for (method, cu) in &sorted {
        println!("  {method:35} {cu:>6} CU");
    }

    // ---------------------------------------------------------------
    // 5. Snapshot — serializable to JSON for dashboards
    // ---------------------------------------------------------------
    let snap: CuSnapshot = tracker.snapshot();
    let json = serde_json::to_string_pretty(&snap).expect("snapshot is serializable");
    println!("\n--- Snapshot (JSON) ---");
    println!("{json}");

    // ---------------------------------------------------------------
    // 6. Cost lookup — check what a method would cost before calling
    // ---------------------------------------------------------------
    println!("\n--- Pre-flight cost check ---");
    let expensive_method = "debug_traceTransaction";
    let cost = tracker.cost_for(expensive_method);
    println!(
        "  {expensive_method} costs {cost} CU ({} remaining)",
        tracker.remaining()
    );
    if cost as u64 > tracker.remaining() {
        println!("  => Would exceed budget — skip or switch providers!");
    }

    // ---------------------------------------------------------------
    // 7. Reset — start a new billing period
    // ---------------------------------------------------------------
    tracker.reset();
    println!("\n--- After reset (new billing period) ---");
    println!("  consumed  : {} CU", tracker.consumed());
    println!("  remaining : {} CU", tracker.remaining());
    println!("  per_method: {:?}", tracker.per_method_usage());
    // Everything back to zero.
}
