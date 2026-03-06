//! Compute unit (CU) budget tracking per provider.
//!
//! Tracks CU consumption and alerts when approaching budget limits.
//! Integrates with the rate limiter to throttle when near budget cap.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Per-method compute unit costs.
///
/// Default costs follow Alchemy's CU pricing model.
#[derive(Debug, Clone)]
pub struct CuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl CuCostTable {
    /// Create a new cost table with a default cost for unknown methods.
    pub fn new(default_cost: u32) -> Self {
        Self {
            costs: HashMap::new(),
            default_cost,
        }
    }

    /// Create the standard Alchemy-style cost table.
    pub fn alchemy_defaults() -> Self {
        let mut table = Self::new(50);
        let defaults = [
            ("eth_blockNumber", 10),
            ("eth_getBalance", 19),
            ("eth_getTransactionCount", 26),
            ("eth_call", 26),
            ("eth_estimateGas", 87),
            ("eth_sendRawTransaction", 250),
            ("eth_getTransactionReceipt", 15),
            ("eth_getBlockByNumber", 16),
            ("eth_getLogs", 75),
            ("eth_subscribe", 10),
            ("eth_getCode", 19),
            ("eth_getStorageAt", 17),
            ("eth_gasPrice", 19),
            ("eth_feeHistory", 10),
            ("eth_maxPriorityFeePerGas", 10),
            ("eth_getTransactionByHash", 17),
            ("debug_traceTransaction", 309),
            ("trace_block", 500),
            ("trace_transaction", 309),
        ];
        for (method, cost) in defaults {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    /// Set the cost for a specific method.
    pub fn set_cost(&mut self, method: &str, cost: u32) {
        self.costs.insert(method.to_string(), cost);
    }

    /// Get the CU cost for a method.
    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for CuCostTable {
    fn default() -> Self {
        Self::alchemy_defaults()
    }
}

/// Budget configuration for a provider.
#[derive(Debug, Clone)]
pub struct CuBudgetConfig {
    /// Monthly CU budget (0 = unlimited).
    pub monthly_budget: u64,
    /// Alert threshold as a fraction (0.0-1.0). Default: 0.8 (80%).
    pub alert_threshold: f64,
    /// Whether to throttle when approaching the limit.
    pub throttle_near_limit: bool,
}

impl Default for CuBudgetConfig {
    fn default() -> Self {
        Self {
            monthly_budget: 0, // unlimited
            alert_threshold: 0.8,
            throttle_near_limit: false,
        }
    }
}

/// Per-provider CU consumption tracker.
pub struct CuTracker {
    /// Provider identifier.
    url: String,
    /// Cost lookup table.
    cost_table: CuCostTable,
    /// Budget configuration.
    config: CuBudgetConfig,
    /// Total CU consumed in current period.
    consumed: AtomicU64,
    /// Per-method CU consumption (for debugging/reporting).
    per_method: Mutex<HashMap<String, u64>>,
}

impl CuTracker {
    /// Create a new tracker for the given provider.
    pub fn new(url: impl Into<String>, cost_table: CuCostTable, config: CuBudgetConfig) -> Self {
        Self {
            url: url.into(),
            cost_table,
            config,
            consumed: AtomicU64::new(0),
            per_method: Mutex::new(HashMap::new()),
        }
    }

    /// Record CU consumption for a method call.
    pub fn record(&self, method: &str) {
        let cost = self.cost_table.cost_for(method) as u64;
        self.consumed.fetch_add(cost, Ordering::Relaxed);
        let mut pm = self.per_method.lock().unwrap();
        *pm.entry(method.to_string()).or_insert(0) += cost;
    }

    /// Get the CU cost that would be charged for this method.
    pub fn cost_for(&self, method: &str) -> u32 {
        self.cost_table.cost_for(method)
    }

    /// Total CU consumed in the current period.
    pub fn consumed(&self) -> u64 {
        self.consumed.load(Ordering::Relaxed)
    }

    /// Remaining CU budget. Returns `u64::MAX` if unlimited.
    pub fn remaining(&self) -> u64 {
        if self.config.monthly_budget == 0 {
            return u64::MAX;
        }
        self.config
            .monthly_budget
            .saturating_sub(self.consumed.load(Ordering::Relaxed))
    }

    /// Usage fraction (0.0-1.0). Returns 0.0 if unlimited.
    pub fn usage_fraction(&self) -> f64 {
        if self.config.monthly_budget == 0 {
            return 0.0;
        }
        self.consumed.load(Ordering::Relaxed) as f64 / self.config.monthly_budget as f64
    }

    /// Whether the budget alert threshold has been exceeded.
    pub fn is_alert(&self) -> bool {
        self.config.monthly_budget > 0 && self.usage_fraction() >= self.config.alert_threshold
    }

    /// Whether the budget is exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.config.monthly_budget > 0
            && self.consumed.load(Ordering::Relaxed) >= self.config.monthly_budget
    }

    /// Whether we should throttle (near limit + throttling enabled).
    pub fn should_throttle(&self) -> bool {
        self.config.throttle_near_limit && self.is_alert()
    }

    /// Reset the consumed counter (e.g. at start of new billing period).
    pub fn reset(&self) {
        self.consumed.store(0, Ordering::Relaxed);
        let mut pm = self.per_method.lock().unwrap();
        pm.clear();
    }

    /// Get per-method breakdown of CU consumption.
    pub fn per_method_usage(&self) -> HashMap<String, u64> {
        let pm = self.per_method.lock().unwrap();
        pm.clone()
    }

    /// Provider URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Produce a snapshot for reporting.
    pub fn snapshot(&self) -> CuSnapshot {
        CuSnapshot {
            url: self.url.clone(),
            consumed: self.consumed.load(Ordering::Relaxed),
            budget: self.config.monthly_budget,
            usage_fraction: self.usage_fraction(),
            alert: self.is_alert(),
            exhausted: self.is_exhausted(),
            per_method: self.per_method_usage(),
        }
    }
}

/// Immutable snapshot of CU tracking state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CuSnapshot {
    pub url: String,
    pub consumed: u64,
    pub budget: u64,
    pub usage_fraction: f64,
    pub alert: bool,
    pub exhausted: bool,
    pub per_method: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_table_defaults() {
        let table = CuCostTable::alchemy_defaults();
        assert_eq!(table.cost_for("eth_blockNumber"), 10);
        assert_eq!(table.cost_for("eth_call"), 26);
        assert_eq!(table.cost_for("eth_sendRawTransaction"), 250);
        assert_eq!(table.cost_for("debug_traceTransaction"), 309);
        assert_eq!(table.cost_for("unknown_method"), 50); // default
    }

    #[test]
    fn tracker_records_consumption() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig::default(),
        );

        tracker.record("eth_blockNumber"); // 10
        tracker.record("eth_call"); // 26
        tracker.record("eth_getLogs"); // 75

        assert_eq!(tracker.consumed(), 10 + 26 + 75);
    }

    #[test]
    fn budget_tracking() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig {
                monthly_budget: 1000,
                alert_threshold: 0.8,
                throttle_near_limit: true,
            },
        );

        // Consume 750 CU (75%)
        for _ in 0..75 {
            tracker.record("eth_blockNumber"); // 10 each
        }
        assert_eq!(tracker.consumed(), 750);
        assert!(!tracker.is_alert());
        assert!(!tracker.should_throttle());
        assert_eq!(tracker.remaining(), 250);

        // Consume 100 more (85% > 80% threshold)
        for _ in 0..10 {
            tracker.record("eth_blockNumber");
        }
        assert!(tracker.is_alert());
        assert!(tracker.should_throttle());
        assert!(!tracker.is_exhausted());
    }

    #[test]
    fn budget_exhaustion() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig {
                monthly_budget: 100,
                ..Default::default()
            },
        );

        for _ in 0..10 {
            tracker.record("eth_blockNumber"); // 10 each = 100 total
        }
        assert!(tracker.is_exhausted());
        assert_eq!(tracker.remaining(), 0);
    }

    #[test]
    fn unlimited_budget() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig::default(), // monthly_budget = 0
        );

        for _ in 0..1000 {
            tracker.record("eth_call");
        }
        assert_eq!(tracker.remaining(), u64::MAX);
        assert!(!tracker.is_alert());
        assert!(!tracker.is_exhausted());
    }

    #[test]
    fn per_method_breakdown() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig::default(),
        );

        tracker.record("eth_blockNumber");
        tracker.record("eth_blockNumber");
        tracker.record("eth_call");

        let breakdown = tracker.per_method_usage();
        assert_eq!(*breakdown.get("eth_blockNumber").unwrap(), 20);
        assert_eq!(*breakdown.get("eth_call").unwrap(), 26);
    }

    #[test]
    fn reset_clears_counters() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig::default(),
        );

        tracker.record("eth_blockNumber");
        assert!(tracker.consumed() > 0);

        tracker.reset();
        assert_eq!(tracker.consumed(), 0);
        assert!(tracker.per_method_usage().is_empty());
    }

    #[test]
    fn snapshot_serializable() {
        let tracker = CuTracker::new(
            "https://rpc.example.com",
            CuCostTable::alchemy_defaults(),
            CuBudgetConfig {
                monthly_budget: 1000,
                ..Default::default()
            },
        );
        tracker.record("eth_call");

        let snap = tracker.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"consumed\":26"));
        assert!(json.contains("\"budget\":1000"));
    }
}
