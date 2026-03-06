//! EIP-1559 gas estimation utilities.
//!
//! Provides speed-tier gas recommendations based on fee history.

use std::time::Duration;

use serde::Serialize;

/// Gas speed tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GasSpeed {
    /// Economy tier (~10th percentile).
    Slow,
    /// Standard tier (~50th percentile).
    Standard,
    /// Fast tier (~90th percentile).
    Fast,
    /// Urgent tier (~99th percentile).
    Urgent,
}

/// EIP-1559 gas recommendation for a single speed tier.
#[derive(Debug, Clone, Serialize)]
pub struct GasEstimate {
    /// Recommended max fee per gas (in wei).
    pub max_fee_per_gas: u128,
    /// Recommended max priority fee per gas (tip, in wei).
    pub max_priority_fee_per_gas: u128,
    /// Estimated time to inclusion.
    pub estimated_time: Option<Duration>,
    /// Speed tier this estimate corresponds to.
    pub speed: GasSpeed,
}

/// Complete gas recommendation across all speed tiers.
#[derive(Debug, Clone, Serialize)]
pub struct GasRecommendation {
    /// Current base fee (from latest block).
    pub base_fee: u128,
    /// Slow/economy estimate.
    pub slow: GasEstimate,
    /// Standard estimate.
    pub standard: GasEstimate,
    /// Fast estimate.
    pub fast: GasEstimate,
    /// Urgent estimate.
    pub urgent: GasEstimate,
    /// Block number this recommendation is based on.
    pub block_number: u64,
}

/// Compute gas recommendations from fee history data.
///
/// `base_fee` is the current base fee in wei.
/// `priority_fees` is a sorted (ascending) list of recent priority fee samples.
/// `block_number` is the current block number.
pub fn compute_gas_recommendation(
    base_fee: u128,
    priority_fees: &[u128],
    block_number: u64,
) -> GasRecommendation {
    let n = priority_fees.len();

    let percentile = |pct: f64| -> u128 {
        if n == 0 {
            return 1_000_000_000; // 1 gwei default
        }
        let idx = ((pct / 100.0) * (n as f64 - 1.0)).round() as usize;
        let idx = idx.min(n - 1);
        priority_fees[idx]
    };

    let slow_tip = percentile(10.0);
    let standard_tip = percentile(50.0);
    let fast_tip = percentile(90.0);
    let urgent_tip = percentile(99.0);

    // Base fee can increase up to 12.5% per block.
    // Apply safety multipliers:
    // slow: 1.0x base (risk of waiting)
    // standard: 1.125x (one block increase)
    // fast: 1.25x (two block increases)
    // urgent: 1.5x (multiple block increases)
    let make_estimate =
        |tip: u128, multiplier: f64, speed: GasSpeed, est_time: Option<Duration>| {
            let adjusted_base = (base_fee as f64 * multiplier) as u128;
            GasEstimate {
                max_fee_per_gas: adjusted_base + tip,
                max_priority_fee_per_gas: tip,
                estimated_time: est_time,
                speed,
            }
        };

    GasRecommendation {
        base_fee,
        slow: make_estimate(
            slow_tip,
            1.0,
            GasSpeed::Slow,
            Some(Duration::from_secs(120)),
        ),
        standard: make_estimate(
            standard_tip,
            1.125,
            GasSpeed::Standard,
            Some(Duration::from_secs(30)),
        ),
        fast: make_estimate(
            fast_tip,
            1.25,
            GasSpeed::Fast,
            Some(Duration::from_secs(15)),
        ),
        urgent: make_estimate(
            urgent_tip,
            1.5,
            GasSpeed::Urgent,
            Some(Duration::from_secs(6)),
        ),
        block_number,
    }
}

/// Apply a safety margin to a gas estimate (multiply gas limit).
///
/// Returns `estimated_gas * multiplier`, rounded up.
pub fn apply_gas_margin(estimated_gas: u64, multiplier: f64) -> u64 {
    (estimated_gas as f64 * multiplier).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_priority_fees() -> Vec<u128> {
        let mut fees: Vec<u128> = (1..=100)
            .map(|i| i * 100_000_000) // 0.1 to 10 gwei
            .collect();
        fees.sort();
        fees
    }

    #[test]
    fn compute_recommendation_basic() {
        let base_fee = 30_000_000_000u128; // 30 gwei
        let fees = sample_priority_fees();

        let rec = compute_gas_recommendation(base_fee, &fees, 1000);

        assert_eq!(rec.base_fee, base_fee);
        assert_eq!(rec.block_number, 1000);

        // Slow should have lower fees than urgent
        assert!(rec.slow.max_fee_per_gas < rec.urgent.max_fee_per_gas);
        assert!(rec.slow.max_priority_fee_per_gas < rec.urgent.max_priority_fee_per_gas);

        // Standard should be between slow and fast
        assert!(rec.standard.max_fee_per_gas > rec.slow.max_fee_per_gas);
        assert!(rec.standard.max_fee_per_gas < rec.fast.max_fee_per_gas);
    }

    #[test]
    fn compute_recommendation_empty_fees() {
        let rec = compute_gas_recommendation(30_000_000_000, &[], 1000);
        // Should use defaults (1 gwei) for priority fees
        assert!(rec.slow.max_priority_fee_per_gas > 0);
    }

    #[test]
    fn gas_margin_application() {
        assert_eq!(apply_gas_margin(100_000, 1.2), 120_000);
        assert_eq!(apply_gas_margin(21_000, 1.0), 21_000);
        assert_eq!(apply_gas_margin(50_000, 1.5), 75_000);
    }

    #[test]
    fn speed_tiers_ordering() {
        let rec = compute_gas_recommendation(10_000_000_000, &sample_priority_fees(), 1);

        assert!(rec.slow.max_fee_per_gas <= rec.standard.max_fee_per_gas);
        assert!(rec.standard.max_fee_per_gas <= rec.fast.max_fee_per_gas);
        assert!(rec.fast.max_fee_per_gas <= rec.urgent.max_fee_per_gas);
    }

    #[test]
    fn serializable() {
        let rec = compute_gas_recommendation(10_000_000_000, &sample_priority_fees(), 1);
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("base_fee"));
        assert!(json.contains("max_fee_per_gas"));
    }
}
