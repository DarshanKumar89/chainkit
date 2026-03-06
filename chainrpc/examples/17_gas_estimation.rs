//! # EIP-1559 Gas Estimation
//!
//! Demonstrates computing gas recommendations across all four speed tiers
//! (Slow, Standard, Fast, Urgent) using recent priority-fee samples and the
//! current base fee, plus applying a safety margin to a gas-limit estimate.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::time::Duration;

use chainrpc_core::gas::{
    apply_gas_margin, compute_gas_recommendation, GasEstimate, GasRecommendation, GasSpeed,
};

/// Helper: format wei as a human-readable Gwei string.
fn wei_to_gwei(wei: u128) -> f64 {
    wei as f64 / 1_000_000_000.0
}

/// Pretty-print a single gas estimate.
fn print_estimate(label: &str, est: &GasEstimate) {
    let time_str = match est.estimated_time {
        Some(d) => format!("~{}s", d.as_secs()),
        None => "unknown".to_string(),
    };
    println!(
        "  {:<10} max_fee={:>8.2} Gwei  priority={:>6.2} Gwei  eta={time_str}",
        label,
        wei_to_gwei(est.max_fee_per_gas),
        wei_to_gwei(est.max_priority_fee_per_gas),
    );
}

#[tokio::main]
async fn main() {
    println!("=== EIP-1559 Gas Estimation ===\n");

    // ── 1. Simulate fee-history data ─────────────────────────────────────────
    // In production you would call `eth_feeHistory` to obtain these values.
    // Here we use a sorted list of 100 priority-fee samples ranging from
    // 0.1 Gwei to 10 Gwei, representing recent block reward distributions.
    let priority_fee_samples: Vec<u128> = (1..=100)
        .map(|i| i * 100_000_000) // 0.1 Gwei increments
        .collect();

    // Current base fee from the latest block header (30 Gwei).
    let base_fee: u128 = 30_000_000_000;

    // Current block number (for reference in the recommendation).
    let block_number: u64 = 19_500_000;

    println!(
        "Input: base_fee = {:.2} Gwei, {} priority-fee samples, block #{}",
        wei_to_gwei(base_fee),
        priority_fee_samples.len(),
        block_number,
    );

    // ── 2. Compute gas recommendations for all 4 speed tiers ─────────────────
    // `compute_gas_recommendation` picks percentiles from the sorted samples:
    //   Slow     -> 10th percentile, base multiplier 1.0x
    //   Standard -> 50th percentile, base multiplier 1.125x
    //   Fast     -> 90th percentile, base multiplier 1.25x
    //   Urgent   -> 99th percentile, base multiplier 1.5x
    let rec: GasRecommendation =
        compute_gas_recommendation(base_fee, &priority_fee_samples, block_number);

    println!(
        "\nRecommendation (base_fee={:.2} Gwei, block #{}):",
        wei_to_gwei(rec.base_fee),
        rec.block_number,
    );

    // Print each tier.
    print_estimate("Slow", &rec.slow);
    print_estimate("Standard", &rec.standard);
    print_estimate("Fast", &rec.fast);
    print_estimate("Urgent", &rec.urgent);

    // Verify ordering: Slow < Standard < Fast < Urgent.
    assert!(rec.slow.max_fee_per_gas <= rec.standard.max_fee_per_gas);
    assert!(rec.standard.max_fee_per_gas <= rec.fast.max_fee_per_gas);
    assert!(rec.fast.max_fee_per_gas <= rec.urgent.max_fee_per_gas);
    println!("\n[OK] Fee ordering: Slow <= Standard <= Fast <= Urgent");

    // ── 3. Apply a safety margin to gas-limit estimates ──────────────────────
    // `apply_gas_margin` multiplies an estimated gas limit by a safety factor,
    // rounding up.  This prevents out-of-gas failures when the actual execution
    // path is slightly more expensive than the estimate.
    let estimated_gas: u64 = 150_000;

    let with_20_pct = apply_gas_margin(estimated_gas, 1.20);
    let with_50_pct = apply_gas_margin(estimated_gas, 1.50);
    let no_margin = apply_gas_margin(estimated_gas, 1.0);

    println!("\nGas-limit margins (base estimate = {estimated_gas}):");
    println!("  1.0x  (none)  -> {no_margin}");
    println!("  1.2x  (+20%)  -> {with_20_pct}");
    println!("  1.5x  (+50%)  -> {with_50_pct}");

    assert_eq!(no_margin, 150_000);
    assert_eq!(with_20_pct, 180_000);
    assert_eq!(with_50_pct, 225_000);
    println!("\n[OK] Gas margins applied correctly");

    // ── 4. Edge case: empty fee history ──────────────────────────────────────
    // When no priority-fee samples are available (e.g. on a brand-new chain or
    // after an extended period of empty blocks), the function falls back to a
    // 1 Gwei default for every tier.
    let empty_rec = compute_gas_recommendation(base_fee, &[], block_number);
    println!("\nEdge case — empty fee history:");
    println!(
        "  Slow priority fee  = {:.2} Gwei (default fallback)",
        wei_to_gwei(empty_rec.slow.max_priority_fee_per_gas),
    );
    assert!(empty_rec.slow.max_priority_fee_per_gas > 0);
    println!("[OK] Fallback defaults applied for empty samples");

    println!("\nDone.");
}
