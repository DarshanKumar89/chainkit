//! # Example 25: Gas Bumping & Transaction Replacement
//!
//! Demonstrates how to speed up or cancel stuck transactions by bumping
//! gas prices. ChainRPC implements EIP-1559 replacement rules (10% minimum
//! increase) with multiple bump strategies.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use chainrpc_core::gas_bumper::{
    BumpStrategy, BumpConfig, BumpResult, compute_bump, compute_cancel,
};
use chainrpc_core::gas::GasSpeed;
use chainrpc_core::tx::{TrackedTx, TxStatus};

fn main() {
    println!("=== Gas Bumping & Transaction Replacement ===\n");

    // =====================================================================
    // 1. Create a stuck transaction
    // =====================================================================
    println!("--- Stuck Transaction ---\n");

    let stuck_tx = TrackedTx {
        tx_hash: "0xabc123...".to_string(),
        from: "0xAlice".to_string(),
        nonce: 42,
        submitted_at: 1700000000,
        status: TxStatus::Pending,
        gas_price: None,                       // EIP-1559 tx
        max_fee: Some(30_000_000_000),          // 30 gwei
        max_priority_fee: Some(2_000_000_000),  // 2 gwei
        last_checked: 1700000300,
    };

    println!("Stuck tx: hash={}, nonce={}", stuck_tx.tx_hash, stuck_tx.nonce);
    println!("  max_fee: 30 gwei, max_priority_fee: 2 gwei");

    let config = BumpConfig::default();
    // default: min_bump 10%, max_gas 500 gwei, max 5 bumps

    // =====================================================================
    // 2. Strategy: Percentage Bump (default 12%)
    // =====================================================================
    println!("\n--- Strategy 1: Percentage Bump (12%) ---\n");

    let bump = compute_bump(
        &stuck_tx, BumpStrategy::default(), &config, 0, None, &[],
    ).unwrap();
    println!("Old max_fee: 30 gwei -> New: {} wei", bump.new_max_fee);
    println!("Old priority: 2 gwei -> New: {} wei", bump.new_max_priority_fee);

    // =====================================================================
    // 3. Strategy: Double
    // =====================================================================
    println!("\n--- Strategy 2: Double ---\n");

    let bump = compute_bump(
        &stuck_tx, BumpStrategy::Double, &config, 0, None, &[],
    ).unwrap();
    println!("New max_fee: {} wei (2x)", bump.new_max_fee);

    // =====================================================================
    // 4. Strategy: Speed Tier (uses live fee data)
    // =====================================================================
    println!("\n--- Strategy 3: Speed Tier (Urgent) ---\n");

    let base_fee = 25_000_000_000u128; // 25 gwei
    let samples: Vec<u128> = (1..=100).map(|i| i * 100_000_000).collect(); // 0.1-10 gwei tips
    let bump = compute_bump(
        &stuck_tx,
        BumpStrategy::SpeedTier(GasSpeed::Urgent),
        &config,
        0,
        Some(base_fee),
        &samples,
    ).unwrap();
    println!("New max_fee: {} wei", bump.new_max_fee);
    println!("New priority: {} wei", bump.new_max_priority_fee);

    // =====================================================================
    // 5. Strategy: Fixed Values
    // =====================================================================
    println!("\n--- Strategy 4: Fixed Values ---\n");

    let bump = compute_bump(
        &stuck_tx,
        BumpStrategy::Fixed {
            max_fee: 50_000_000_000,       // 50 gwei
            max_priority_fee: 5_000_000_000, // 5 gwei
        },
        &config,
        0,
        None,
        &[],
    ).unwrap();
    println!("max_fee={}, priority={}", bump.new_max_fee, bump.new_max_priority_fee);

    // =====================================================================
    // 6. Strategy: Cancel (self-transfer at minimum bump)
    // =====================================================================
    println!("\n--- Strategy 5: Cancel ---\n");

    let cancel = compute_cancel(&stuck_tx, &config, 0).unwrap();
    println!("Cancel tx: max_fee={}, priority={}", cancel.new_max_fee, cancel.new_max_priority_fee);
    println!("Strategy: {}", cancel.strategy_used);

    // =====================================================================
    // 7. Safety: Cap and Max Bumps
    // =====================================================================
    // Bump exceeds 500 gwei cap -> error
    println!("\n--- Safety: Gas Cap ---\n");

    let rich_tx = TrackedTx {
        max_fee: Some(400_000_000_000), // 400 gwei — doubling exceeds 500 gwei cap
        ..stuck_tx.clone()
    };
    let result = compute_bump(&rich_tx, BumpStrategy::Double, &config, 0, None, &[]);
    println!("Double on 400 gwei tx exceeds cap: {}", result.is_err()); // true

    // =====================================================================
    // 8. In Production: bump_and_send()
    // =====================================================================
    // The async version sends the replacement tx and updates the tracker:
    //
    //   let result = bump_and_send(
    //       &transport,
    //       &tracker,
    //       "0xstuck_hash",
    //       BumpStrategy::SpeedTier(GasSpeed::Fast),
    //       &BumpConfig::default(),
    //       0,                    // first bump attempt
    //       Some(base_fee),
    //       &priority_samples,
    //       |nonce, max_fee, priority_fee| {
    //           // Your signing logic here — returns raw tx hex
    //           sign_eip1559_tx(nonce, max_fee, priority_fee)
    //       },
    //   ).await?;
    //
    //   // Original tx is now TxStatus::Replaced { replacement_hash }
    //   // New tx is tracked automatically

    println!("\n--- Production Usage ---\n");
    println!("Use bump_and_send() to sign and submit the replacement transaction.");
    println!("The original tx status becomes TxStatus::Replaced.");

    println!("\nDone.");
}
