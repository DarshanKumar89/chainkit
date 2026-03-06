//! Gas bumping and transaction replacement.
//!
//! When a transaction is stuck in the mempool (gas too low, network congested),
//! this module provides strategies to "bump" the gas price and resubmit.
//!
//! # EIP-1559 Replacement Rules
//!
//! To replace a pending transaction, the new transaction must:
//! - Use the **same nonce** as the stuck transaction
//! - Set `maxPriorityFeePerGas` at least 10% higher than the original
//! - Set `maxFeePerGas` at least 10% higher than the original
//!
//! For legacy transactions, `gasPrice` must be at least 10% higher.

use serde::Serialize;
use serde_json::Value;

use crate::error::TransportError;
use crate::gas::{compute_gas_recommendation, GasSpeed};
use crate::request::JsonRpcRequest;
use crate::transport::RpcTransport;
use crate::tx::{TrackedTx, TxStatus, TxTracker};

// ---------------------------------------------------------------------------
// BumpStrategy
// ---------------------------------------------------------------------------

/// Strategy for bumping gas on a stuck transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BumpStrategy {
    /// Increase gas by a fixed percentage (basis points, e.g. 1200 = 12%).
    Percentage(u32),
    /// Use a specific gas speed tier from fee history.
    SpeedTier(GasSpeed),
    /// Set gas to specific values (wei).
    Fixed {
        max_fee: u64,
        max_priority_fee: u64,
    },
    /// Aggressive: 2x the current gas.
    Double,
    /// Cancel: replace with 0-value self-transfer at minimum bump.
    Cancel,
}

impl Default for BumpStrategy {
    fn default() -> Self {
        Self::Percentage(1200) // 12% bump — minimum to replace
    }
}

// ---------------------------------------------------------------------------
// BumpConfig
// ---------------------------------------------------------------------------

/// Configuration controlling gas bump behavior.
#[derive(Debug, Clone)]
pub struct BumpConfig {
    /// Minimum percentage increase required by the network (default: 10% = 1000 bps).
    pub min_bump_bps: u32,
    /// Maximum gas price cap in wei (prevent runaway bumps).
    pub max_gas_price: u64,
    /// Maximum number of consecutive bumps for a single tx.
    pub max_bumps: u32,
}

impl Default for BumpConfig {
    fn default() -> Self {
        Self {
            min_bump_bps: 1000,               // 10%
            max_gas_price: 500_000_000_000,    // 500 gwei cap
            max_bumps: 5,
        }
    }
}

// ---------------------------------------------------------------------------
// BumpResult
// ---------------------------------------------------------------------------

/// The result of computing a gas bump.
#[derive(Debug, Clone, Serialize)]
pub struct BumpResult {
    /// Hash of the original transaction that was bumped.
    pub original_hash: String,
    /// New `maxFeePerGas` (or equivalent legacy gas price).
    pub new_max_fee: u64,
    /// New `maxPriorityFeePerGas`.
    pub new_max_priority_fee: u64,
    /// New legacy `gasPrice` (only set for legacy transactions).
    pub new_gas_price: Option<u64>,
    /// How many times this transaction has been bumped (including this one).
    pub bump_count: u32,
    /// Description of the strategy that was applied.
    pub strategy_used: String,
}

// ---------------------------------------------------------------------------
// compute_bump
// ---------------------------------------------------------------------------

/// Compute bumped gas parameters for a stuck transaction.
///
/// Given a tracked transaction and a bump strategy, computes the new gas
/// parameters that satisfy the network's replacement rules.
///
/// Returns `Err` if:
/// - The transaction is not in `Pending` status
/// - The bump would exceed `max_gas_price`
/// - The bump count exceeds `max_bumps`
pub fn compute_bump(
    tx: &TrackedTx,
    strategy: BumpStrategy,
    config: &BumpConfig,
    bump_count: u32,
    current_base_fee: Option<u128>,
    priority_fee_samples: &[u128],
) -> Result<BumpResult, TransportError> {
    // Check preconditions
    if tx.status != TxStatus::Pending {
        return Err(TransportError::Other(
            "can only bump pending transactions".into(),
        ));
    }
    if bump_count >= config.max_bumps {
        return Err(TransportError::Other(format!(
            "max bumps ({}) exceeded",
            config.max_bumps
        )));
    }

    // Get current gas values
    let current_max_fee = tx.max_fee.unwrap_or(tx.gas_price.unwrap_or(0));
    let current_priority = tx.max_priority_fee.unwrap_or(0);

    let (new_max_fee, new_priority_fee) = match strategy {
        BumpStrategy::Percentage(bps) => {
            let effective_bps = bps.max(config.min_bump_bps);
            let bump_multiplier = 10000 + effective_bps as u64;
            (
                current_max_fee * bump_multiplier / 10000,
                current_priority * bump_multiplier / 10000,
            )
        }
        BumpStrategy::Double => (current_max_fee * 2, current_priority * 2),
        BumpStrategy::Fixed {
            max_fee,
            max_priority_fee,
        } => {
            // Ensure fixed values meet minimum bump
            let min_max_fee =
                current_max_fee * (10000 + config.min_bump_bps as u64) / 10000;
            let min_priority =
                current_priority * (10000 + config.min_bump_bps as u64) / 10000;
            (max_fee.max(min_max_fee), max_priority_fee.max(min_priority))
        }
        BumpStrategy::SpeedTier(speed) => {
            if let Some(base_fee) = current_base_fee {
                let rec =
                    compute_gas_recommendation(base_fee, priority_fee_samples, 0);
                let estimate = match speed {
                    GasSpeed::Slow => &rec.slow,
                    GasSpeed::Standard => &rec.standard,
                    GasSpeed::Fast => &rec.fast,
                    GasSpeed::Urgent => &rec.urgent,
                };
                let new_fee = estimate.max_fee_per_gas as u64;
                let new_tip = estimate.max_priority_fee_per_gas as u64;
                // Ensure meets minimum bump
                let min_fee = current_max_fee
                    * (10000 + config.min_bump_bps as u64)
                    / 10000;
                let min_tip = current_priority
                    * (10000 + config.min_bump_bps as u64)
                    / 10000;
                (new_fee.max(min_fee), new_tip.max(min_tip))
            } else {
                // No base fee info, fall back to percentage
                let bps = config.min_bump_bps;
                let mult = 10000 + bps as u64;
                (
                    current_max_fee * mult / 10000,
                    current_priority * mult / 10000,
                )
            }
        }
        BumpStrategy::Cancel => {
            // Minimum bump for a cancellation (self-transfer with 0 value)
            let mult = 10000 + config.min_bump_bps as u64;
            (
                current_max_fee * mult / 10000,
                current_priority * mult / 10000,
            )
        }
    };

    // Ensure minimum priority fee (at least 1 gwei if currently 0)
    let new_priority_fee = if new_priority_fee == 0 {
        1_000_000_000
    } else {
        new_priority_fee
    };

    // Cap check
    if new_max_fee > config.max_gas_price {
        return Err(TransportError::Other(format!(
            "bumped gas {} exceeds cap {}",
            new_max_fee, config.max_gas_price
        )));
    }

    Ok(BumpResult {
        original_hash: tx.tx_hash.clone(),
        new_max_fee,
        new_max_priority_fee: new_priority_fee,
        new_gas_price: if tx.gas_price.is_some() {
            Some(new_max_fee)
        } else {
            None
        },
        bump_count: bump_count + 1,
        strategy_used: format!("{:?}", strategy),
    })
}

// ---------------------------------------------------------------------------
// bump_and_send
// ---------------------------------------------------------------------------

/// Submit a gas-bumped replacement transaction.
///
/// 1. Computes the new gas parameters via [`compute_bump`].
/// 2. The caller provides a `raw_tx_builder` closure that creates a new
///    signed transaction with the bumped gas params (signing is external).
/// 3. Sends via `eth_sendRawTransaction`.
/// 4. Updates the tracker: marks original as `Replaced`, tracks the new tx.
///
/// The `raw_tx_builder` receives `(nonce, new_max_fee, new_priority_fee)` and
/// returns the raw signed transaction hex string. This keeps signing external
/// to chainrpc.
pub async fn bump_and_send<F>(
    transport: &dyn RpcTransport,
    tracker: &TxTracker,
    tx_hash: &str,
    strategy: BumpStrategy,
    config: &BumpConfig,
    bump_count: u32,
    current_base_fee: Option<u128>,
    priority_fee_samples: &[u128],
    raw_tx_builder: F,
) -> Result<BumpResult, TransportError>
where
    F: FnOnce(u64, u64, u64) -> String, // (nonce, max_fee, priority_fee) -> raw_tx_hex
{
    let tx = tracker.get(tx_hash).ok_or_else(|| {
        TransportError::Other(format!("transaction {tx_hash} not tracked"))
    })?;

    let bump = compute_bump(
        &tx,
        strategy,
        config,
        bump_count,
        current_base_fee,
        priority_fee_samples,
    )?;

    // Build the replacement transaction
    let raw_tx =
        raw_tx_builder(tx.nonce, bump.new_max_fee, bump.new_max_priority_fee);

    // Send it
    let req = JsonRpcRequest::auto(
        "eth_sendRawTransaction",
        vec![Value::String(raw_tx)],
    );
    let resp = transport.send(req).await?;
    let result = resp.into_result().map_err(TransportError::Rpc)?;

    let new_hash = result
        .as_str()
        .ok_or_else(|| {
            TransportError::Other(
                "eth_sendRawTransaction did not return a hash".into(),
            )
        })?
        .to_string();

    // Update tracker — mark original as replaced
    tracker.update_status(
        tx_hash,
        TxStatus::Replaced {
            replacement_hash: new_hash.clone(),
        },
    );

    // Track the new transaction
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    tracker.track(TrackedTx {
        tx_hash: new_hash,
        from: tx.from.clone(),
        nonce: tx.nonce,
        submitted_at: now,
        status: TxStatus::Pending,
        gas_price: bump.new_gas_price,
        max_fee: Some(bump.new_max_fee),
        max_priority_fee: Some(bump.new_max_priority_fee),
        last_checked: now,
    });

    Ok(bump)
}

// ---------------------------------------------------------------------------
// compute_cancel
// ---------------------------------------------------------------------------

/// Compute cancellation parameters for a stuck transaction.
///
/// A cancellation is a 0-value self-transfer at the same nonce with the
/// minimum gas bump. The caller still needs to sign and build the raw tx.
pub fn compute_cancel(
    tx: &TrackedTx,
    config: &BumpConfig,
    bump_count: u32,
) -> Result<BumpResult, TransportError> {
    compute_bump(tx, BumpStrategy::Cancel, config, bump_count, None, &[])
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{JsonRpcResponse, RpcId};
    use crate::tx::TxTrackerConfig;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn pending_eip1559_tx(hash: &str, max_fee: u64, priority_fee: u64) -> TrackedTx {
        TrackedTx {
            tx_hash: hash.to_string(),
            from: "0xAlice".to_string(),
            nonce: 7,
            submitted_at: 1000,
            status: TxStatus::Pending,
            gas_price: None,
            max_fee: Some(max_fee),
            max_priority_fee: Some(priority_fee),
            last_checked: 1000,
        }
    }

    fn pending_legacy_tx(hash: &str, gas_price: u64) -> TrackedTx {
        TrackedTx {
            tx_hash: hash.to_string(),
            from: "0xBob".to_string(),
            nonce: 3,
            submitted_at: 1000,
            status: TxStatus::Pending,
            gas_price: Some(gas_price),
            max_fee: None,
            max_priority_fee: None,
            last_checked: 1000,
        }
    }

    // -----------------------------------------------------------------------
    // Mock transport
    // -----------------------------------------------------------------------

    struct MockTransport {
        responses: Mutex<HashMap<String, Value>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
            }
        }

        fn set_response(&self, method: &str, value: Value) {
            let mut map = self.responses.lock().unwrap();
            map.insert(method.to_string(), value);
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(
            &self,
            req: JsonRpcRequest,
        ) -> Result<JsonRpcResponse, TransportError> {
            let map = self.responses.lock().unwrap();
            let result = map.get(&req.method).cloned().unwrap_or(Value::Null);
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: RpcId::Number(1),
                result: Some(result),
                error: None,
            })
        }

        fn url(&self) -> &str {
            "mock://gas_bumper"
        }
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn bump_percentage_default() {
        // Default strategy: 12% bump (1200 bps)
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default();

        let result =
            compute_bump(&tx, BumpStrategy::default(), &config, 0, None, &[])
                .expect("should succeed");

        // 12% bump: 100_000_000_000 * 11200 / 10000 = 112_000_000_000
        assert_eq!(result.new_max_fee, 112_000_000_000);
        // 12% bump: 2_000_000_000 * 11200 / 10000 = 2_240_000_000
        assert_eq!(result.new_max_priority_fee, 2_240_000_000);
        assert_eq!(result.bump_count, 1);
        assert_eq!(result.original_hash, "0xabc");
        assert!(result.new_gas_price.is_none()); // EIP-1559 tx
    }

    #[test]
    fn bump_double() {
        let tx = pending_eip1559_tx("0xabc", 50_000_000_000, 1_000_000_000);
        let config = BumpConfig::default();

        let result =
            compute_bump(&tx, BumpStrategy::Double, &config, 0, None, &[])
                .expect("should succeed");

        assert_eq!(result.new_max_fee, 100_000_000_000);
        assert_eq!(result.new_max_priority_fee, 2_000_000_000);
        assert!(result.strategy_used.contains("Double"));
    }

    #[test]
    fn bump_fixed_enforces_minimum() {
        // Fixed values below the 10% minimum should be raised
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default(); // min_bump_bps = 1000 (10%)

        let result = compute_bump(
            &tx,
            BumpStrategy::Fixed {
                max_fee: 105_000_000_000,       // 5% bump — too low
                max_priority_fee: 2_100_000_000, // 5% bump — too low
            },
            &config,
            0,
            None,
            &[],
        )
        .expect("should succeed");

        // Minimum 10% bump: 100_000_000_000 * 11000 / 10000 = 110_000_000_000
        assert_eq!(result.new_max_fee, 110_000_000_000);
        // Minimum 10% bump: 2_000_000_000 * 11000 / 10000 = 2_200_000_000
        assert_eq!(result.new_max_priority_fee, 2_200_000_000);
    }

    #[test]
    fn bump_fixed_uses_higher_value_when_above_minimum() {
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default();

        let result = compute_bump(
            &tx,
            BumpStrategy::Fixed {
                max_fee: 200_000_000_000,       // 100% bump — well above min
                max_priority_fee: 5_000_000_000, // 150% bump — well above min
            },
            &config,
            0,
            None,
            &[],
        )
        .expect("should succeed");

        assert_eq!(result.new_max_fee, 200_000_000_000);
        assert_eq!(result.new_max_priority_fee, 5_000_000_000);
    }

    #[test]
    fn bump_cancel_minimum() {
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default();

        let result =
            compute_bump(&tx, BumpStrategy::Cancel, &config, 0, None, &[])
                .expect("should succeed");

        // Cancel uses minimum bump (10%)
        assert_eq!(result.new_max_fee, 110_000_000_000);
        assert_eq!(result.new_max_priority_fee, 2_200_000_000);
        assert!(result.strategy_used.contains("Cancel"));
    }

    #[test]
    fn bump_exceeds_cap() {
        let tx = pending_eip1559_tx("0xabc", 400_000_000_000, 2_000_000_000);
        let config = BumpConfig {
            max_gas_price: 500_000_000_000, // 500 gwei cap
            ..Default::default()
        };

        // A 30% bump would put us at 520 gwei, above the cap
        let err = compute_bump(
            &tx,
            BumpStrategy::Percentage(3000),
            &config,
            0,
            None,
            &[],
        )
        .unwrap_err();

        let msg = format!("{err}");
        assert!(msg.contains("exceeds cap"));
    }

    #[test]
    fn bump_max_bumps_exceeded() {
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig {
            max_bumps: 3,
            ..Default::default()
        };

        // bump_count = 3 (already bumped 3 times) >= max_bumps (3)
        let err = compute_bump(
            &tx,
            BumpStrategy::default(),
            &config,
            3,
            None,
            &[],
        )
        .unwrap_err();

        let msg = format!("{err}");
        assert!(msg.contains("max bumps"));
    }

    #[test]
    fn bump_non_pending_fails() {
        let mut tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        tx.status = TxStatus::Included {
            block_number: 42,
            block_hash: "0xblock".to_string(),
        };
        let config = BumpConfig::default();

        let err = compute_bump(
            &tx,
            BumpStrategy::default(),
            &config,
            0,
            None,
            &[],
        )
        .unwrap_err();

        let msg = format!("{err}");
        assert!(msg.contains("pending"));
    }

    #[tokio::test]
    async fn bump_and_send_updates_tracker() {
        let transport = MockTransport::new();
        transport.set_response(
            "eth_sendRawTransaction",
            Value::String("0xnew_hash".into()),
        );

        let tracker = TxTracker::new(TxTrackerConfig::default());
        let tx = pending_eip1559_tx("0xoriginal", 100_000_000_000, 2_000_000_000);
        tracker.track(tx);

        let result = bump_and_send(
            &transport,
            &tracker,
            "0xoriginal",
            BumpStrategy::default(),
            &BumpConfig::default(),
            0,
            None,
            &[],
            |_nonce, _max_fee, _priority_fee| "0xsigned_raw_tx".to_string(),
        )
        .await
        .expect("bump_and_send should succeed");

        assert_eq!(result.original_hash, "0xoriginal");
        assert_eq!(result.bump_count, 1);

        // Original should be marked as replaced
        let original = tracker.get("0xoriginal").expect("original should exist");
        assert_eq!(
            original.status,
            TxStatus::Replaced {
                replacement_hash: "0xnew_hash".to_string()
            }
        );

        // New tx should be tracked
        let new_tx = tracker.get("0xnew_hash").expect("new tx should be tracked");
        assert_eq!(new_tx.status, TxStatus::Pending);
        assert_eq!(new_tx.nonce, 7); // same nonce as original
        assert_eq!(new_tx.max_fee, Some(result.new_max_fee));
        assert_eq!(new_tx.max_priority_fee, Some(result.new_max_priority_fee));
    }

    #[test]
    fn bump_speed_tier_uses_recommendation() {
        let tx = pending_eip1559_tx("0xabc", 10_000_000_000, 1_000_000_000);
        let config = BumpConfig::default();

        // Provide a base fee and priority fee samples
        let base_fee: u128 = 30_000_000_000; // 30 gwei
        let mut samples: Vec<u128> = (1..=100)
            .map(|i| i * 100_000_000) // 0.1 to 10 gwei
            .collect();
        samples.sort();

        let result = compute_bump(
            &tx,
            BumpStrategy::SpeedTier(GasSpeed::Urgent),
            &config,
            0,
            Some(base_fee),
            &samples,
        )
        .expect("should succeed");

        // The urgent tier uses 1.5x base fee + 99th percentile tip.
        // base_fee * 1.5 = 45 gwei. tip ~ 9.9 gwei.
        // max_fee = 45 + 9.9 = ~54.9 gwei.
        // This should be well above the minimum bump of 10 gwei * 1.1 = 11 gwei.
        assert!(result.new_max_fee > 10_000_000_000);
        assert!(result.new_max_priority_fee > 0);
        assert!(result.strategy_used.contains("SpeedTier"));
    }

    #[test]
    fn bump_speed_tier_fallback_without_base_fee() {
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default();

        // No base fee available — should fall back to percentage
        let result = compute_bump(
            &tx,
            BumpStrategy::SpeedTier(GasSpeed::Fast),
            &config,
            0,
            None, // no base fee
            &[],
        )
        .expect("should succeed");

        // Falls back to min_bump_bps = 10%
        assert_eq!(result.new_max_fee, 110_000_000_000);
        assert_eq!(result.new_max_priority_fee, 2_200_000_000);
    }

    #[test]
    fn compute_cancel_works() {
        let tx = pending_eip1559_tx("0xabc", 100_000_000_000, 2_000_000_000);
        let config = BumpConfig::default();

        let result = compute_cancel(&tx, &config, 0).expect("should succeed");

        // Cancel uses minimum bump (10%)
        assert_eq!(result.new_max_fee, 110_000_000_000);
        assert_eq!(result.new_max_priority_fee, 2_200_000_000);
        assert!(result.strategy_used.contains("Cancel"));
        assert_eq!(result.bump_count, 1);
    }

    #[test]
    fn bump_legacy_tx_sets_gas_price() {
        let tx = pending_legacy_tx("0xlegacy", 20_000_000_000);
        let config = BumpConfig::default();

        let result =
            compute_bump(&tx, BumpStrategy::default(), &config, 0, None, &[])
                .expect("should succeed");

        // Legacy tx: gas_price is Some, so new_gas_price should be set
        assert!(result.new_gas_price.is_some());
        assert_eq!(result.new_gas_price.unwrap(), result.new_max_fee);
        // 12% bump: 20_000_000_000 * 11200 / 10000 = 22_400_000_000
        assert_eq!(result.new_max_fee, 22_400_000_000);
    }

    #[test]
    fn bump_zero_priority_gets_minimum() {
        // When priority fee is 0, ensure we set at least 1 gwei
        let mut tx = pending_eip1559_tx("0xabc", 100_000_000_000, 0);
        tx.max_priority_fee = Some(0);
        let config = BumpConfig::default();

        let result =
            compute_bump(&tx, BumpStrategy::default(), &config, 0, None, &[])
                .expect("should succeed");

        // Zero priority fee should be bumped to at least 1 gwei
        assert_eq!(result.new_max_priority_fee, 1_000_000_000);
    }

    #[tokio::test]
    async fn bump_and_send_untracked_tx_fails() {
        let transport = MockTransport::new();
        let tracker = TxTracker::new(TxTrackerConfig::default());
        // Don't track any transaction

        let err = bump_and_send(
            &transport,
            &tracker,
            "0xunknown",
            BumpStrategy::default(),
            &BumpConfig::default(),
            0,
            None,
            &[],
            |_nonce, _max_fee, _priority_fee| "0xraw".to_string(),
        )
        .await
        .unwrap_err();

        let msg = format!("{err}");
        assert!(msg.contains("not tracked"));
    }
}
