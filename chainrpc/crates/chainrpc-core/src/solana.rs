//! Solana RPC support — method safety, commitment levels, and transport helpers.
//!
//! Solana uses JSON-RPC like Ethereum, so the [`RpcTransport`] trait works
//! directly. This module adds Solana-specific semantics:
//!
//! - [`SolanaCommitment`] — processed / confirmed / finalized
//! - [`classify_solana_method`] — safe / idempotent / unsafe for Solana methods
//! - [`SolanaCuCostTable`] — Solana-specific CU costs
//! - [`SolanaTransport`] — wrapper that injects commitment config into requests
//! - [`solana_mainnet_endpoints`] / [`solana_devnet_endpoints`] / [`solana_testnet_endpoints`]
//!   — known public Solana RPC endpoints

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::TransportError;
use crate::method_safety::MethodSafety;
use crate::request::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::{HealthStatus, RpcTransport};

// ---------------------------------------------------------------------------
// SolanaCommitment
// ---------------------------------------------------------------------------

/// Solana commitment levels — controls the consistency/finality guarantee of
/// a query.  Equivalent in spirit to Ethereum block tags ("latest",
/// "safe", "finalized"), but Solana-specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SolanaCommitment {
    /// Fastest — the node has processed the transaction/block but the cluster
    /// has **not** voted on it yet.  Risk of rollback is highest.
    Processed,
    /// A super-majority of the cluster has voted to confirm the block.
    /// This is the default and strikes a balance between speed and safety.
    Confirmed,
    /// The block has been rooted (31+ confirmed blocks on top) and **cannot**
    /// be rolled back.  Safest for indexing and accounting.
    Finalized,
}

impl SolanaCommitment {
    /// Return the lowercase string representation used in Solana JSON-RPC.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Processed => "processed",
            Self::Confirmed => "confirmed",
            Self::Finalized => "finalized",
        }
    }

    /// Whether this commitment level is safe for **indexing** (i.e. persisting
    /// data that should never be rolled back).  Only `Finalized` qualifies.
    pub fn is_safe_for_indexing(&self) -> bool {
        matches!(self, Self::Finalized)
    }

    /// Whether this commitment level is safe for **display** purposes (e.g.
    /// showing a balance to a user).  Both `Confirmed` and `Finalized`
    /// qualify.
    pub fn is_safe_for_display(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Finalized)
    }
}

impl Default for SolanaCommitment {
    fn default() -> Self {
        Self::Confirmed
    }
}

impl std::fmt::Display for SolanaCommitment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Solana method classification
// ---------------------------------------------------------------------------

/// Classify a Solana JSON-RPC method by its safety level.
///
/// - **Safe** — read-only, retryable, cacheable, dedup-able.
/// - **Idempotent** — same input always produces the same result, but has
///   side effects.  `sendTransaction` with the same signed bytes will always
///   produce the same signature.
/// - **Unsafe** — must **never** be retried.  `requestAirdrop` would create
///   duplicate airdrops.
///
/// Unknown methods default to `Safe` (same convention as the EVM classifier).
pub fn classify_solana_method(method: &str) -> MethodSafety {
    if solana_unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if solana_idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

/// Returns `true` if the Solana method is safe to retry on transient failure.
pub fn is_solana_safe_to_retry(method: &str) -> bool {
    classify_solana_method(method) == MethodSafety::Safe
}

/// Returns `true` if concurrent identical requests for this Solana method
/// can be deduplicated (coalesced).
pub fn is_solana_safe_to_dedup(method: &str) -> bool {
    classify_solana_method(method) == MethodSafety::Safe
}

/// Returns `true` if the result of this Solana method can be cached.
pub fn is_solana_cacheable(method: &str) -> bool {
    classify_solana_method(method) == MethodSafety::Safe
}

/// Set of Solana methods that are **unsafe** — must never be retried.
fn solana_unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(|| ["requestAirdrop"].into_iter().collect())
}

/// Set of Solana methods that are **idempotent** — same input yields the same
/// result but has side effects.
fn solana_idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| ["sendTransaction"].into_iter().collect())
}

// All other Solana methods are Safe by default (read-only queries).

// ---------------------------------------------------------------------------
// Methods that accept a commitment configuration parameter
// ---------------------------------------------------------------------------

/// Returns `true` if the given Solana method accepts a `commitment`
/// configuration object.
fn accepts_commitment(method: &str) -> bool {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "getAccountInfo",
            "getBalance",
            "getBlock",
            "getBlockHeight",
            "getEpochInfo",
            "getLatestBlockhash",
            "getSlot",
            "getTransaction",
            "getSignaturesForAddress",
            "getTokenAccountBalance",
            "getTokenAccountsByOwner",
            "getProgramAccounts",
            "getMultipleAccounts",
            "sendTransaction",
            "simulateTransaction",
            "getSignatureStatuses",
        ]
        .into_iter()
        .collect()
    })
    .contains(method)
}

// ---------------------------------------------------------------------------
// SolanaCuCostTable
// ---------------------------------------------------------------------------

/// Per-method compute-unit cost table for Solana RPC methods.
///
/// The weights are approximate and intended for rate-limiting and cost
/// tracking, not for on-chain CU budgeting.
#[derive(Debug, Clone)]
pub struct SolanaCuCostTable {
    costs: HashMap<String, u32>,
    default_cost: u32,
}

impl SolanaCuCostTable {
    /// Create the standard Solana cost table with sensible defaults.
    pub fn defaults() -> Self {
        let mut table = Self::new(15);
        let entries: &[(&str, u32)] = &[
            ("getAccountInfo", 10),
            ("getBalance", 10),
            ("getBlock", 50),
            ("getBlockHeight", 5),
            ("getTransaction", 20),
            ("getProgramAccounts", 100),
            ("getSignaturesForAddress", 30),
            ("getTokenAccountsByOwner", 30),
            ("getSlot", 5),
            ("getLatestBlockhash", 10),
            ("sendTransaction", 10),
            ("simulateTransaction", 50),
            ("getMultipleAccounts", 30),
        ];
        for &(method, cost) in entries {
            table.costs.insert(method.to_string(), cost);
        }
        table
    }

    /// Create an empty cost table with the given default cost.
    pub fn new(default_cost: u32) -> Self {
        Self {
            costs: HashMap::new(),
            default_cost,
        }
    }

    /// Set (or override) the CU cost for a specific method.
    pub fn set_cost(&mut self, method: &str, cost: u32) {
        self.costs.insert(method.to_string(), cost);
    }

    /// Return the CU cost for a method, falling back to the default cost.
    pub fn cost_for(&self, method: &str) -> u32 {
        self.costs.get(method).copied().unwrap_or(self.default_cost)
    }
}

impl Default for SolanaCuCostTable {
    fn default() -> Self {
        Self::defaults()
    }
}

// ---------------------------------------------------------------------------
// SolanaTransport
// ---------------------------------------------------------------------------

/// A wrapper around any [`RpcTransport`] that adds Solana-aware behaviour.
///
/// On every outgoing request whose method accepts a commitment configuration,
/// the transport automatically injects the configured [`SolanaCommitment`]
/// into the request parameters (unless the caller already provided one).
pub struct SolanaTransport {
    inner: Arc<dyn RpcTransport>,
    commitment: SolanaCommitment,
}

impl SolanaTransport {
    /// Wrap an existing transport with the given default commitment level.
    pub fn new(inner: Arc<dyn RpcTransport>, commitment: SolanaCommitment) -> Self {
        Self { inner, commitment }
    }

    /// Return a new `SolanaTransport` that shares the same inner transport
    /// but uses a different commitment level.
    pub fn with_commitment(&self, commitment: SolanaCommitment) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            commitment,
        }
    }

    /// Return the configured commitment level.
    pub fn commitment(&self) -> SolanaCommitment {
        self.commitment
    }

    /// Inject `{"commitment": "<level>"}` into the request's params if:
    ///
    /// 1. The method supports it (per [`accepts_commitment`]).
    /// 2. No commitment has already been specified by the caller.
    ///
    /// Solana RPC convention: the **last** parameter is typically a config
    /// object.  If the last param is an object, we merge `commitment` into it
    /// (only if not already present).  Otherwise we append a new config object.
    fn inject_commitment(&self, req: &mut JsonRpcRequest) {
        if !accepts_commitment(&req.method) {
            return;
        }

        let commitment_value = Value::String(self.commitment.as_str().to_string());

        // Look for an existing config object as the last param.
        if let Some(last) = req.params.last_mut() {
            if let Value::Object(map) = last {
                // Only inject if the caller did not already set commitment.
                map.entry("commitment")
                    .or_insert(commitment_value);
                return;
            }
        }

        // No config object found — append one.
        let mut config = serde_json::Map::new();
        config.insert("commitment".to_string(), commitment_value);
        req.params.push(Value::Object(config));
    }
}

#[async_trait]
impl RpcTransport for SolanaTransport {
    async fn send(
        &self,
        req: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, TransportError> {
        let mut req = req;
        self.inject_commitment(&mut req);
        self.inner.send(req).await
    }

    async fn send_batch(
        &self,
        reqs: Vec<JsonRpcRequest>,
    ) -> Result<Vec<JsonRpcResponse>, TransportError> {
        let reqs: Vec<JsonRpcRequest> = reqs
            .into_iter()
            .map(|mut r| {
                self.inject_commitment(&mut r);
                r
            })
            .collect();
        self.inner.send_batch(reqs).await
    }

    fn health(&self) -> HealthStatus {
        self.inner.health()
    }

    fn url(&self) -> &str {
        self.inner.url()
    }
}

// ---------------------------------------------------------------------------
// Known endpoints
// ---------------------------------------------------------------------------

/// Well-known public Solana mainnet-beta RPC endpoints.
pub fn solana_mainnet_endpoints() -> Vec<&'static str> {
    vec![
        "https://api.mainnet-beta.solana.com",
        "https://solana-mainnet.g.alchemy.com/v2",
        "https://rpc.ankr.com/solana",
    ]
}

/// Well-known public Solana devnet RPC endpoints.
pub fn solana_devnet_endpoints() -> Vec<&'static str> {
    vec!["https://api.devnet.solana.com"]
}

/// Well-known public Solana testnet RPC endpoints.
pub fn solana_testnet_endpoints() -> Vec<&'static str> {
    vec!["https://api.testnet.solana.com"]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -- helpers ----------------------------------------------------------

    /// A trivial in-memory transport that records every request it receives
    /// and returns a canned success response.
    struct MockTransport {
        url: String,
        sent: Mutex<Vec<JsonRpcRequest>>,
    }

    impl MockTransport {
        fn new(url: &str) -> Self {
            Self {
                url: url.to_string(),
                sent: Mutex::new(Vec::new()),
            }
        }

        fn sent_requests(&self) -> Vec<JsonRpcRequest> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RpcTransport for MockTransport {
        async fn send(
            &self,
            req: JsonRpcRequest,
        ) -> Result<JsonRpcResponse, TransportError> {
            self.sent.lock().unwrap().push(req.clone());
            Ok(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(Value::Null),
                error: None,
            })
        }

        fn url(&self) -> &str {
            &self.url
        }
    }

    // -- 1. classify_safe_methods -----------------------------------------

    #[test]
    fn classify_safe_methods() {
        let safe_methods = [
            "getBalance",
            "getSlot",
            "getAccountInfo",
            "getBlock",
            "getBlockHeight",
            "getBlockProduction",
            "getBlockCommitment",
            "getBlockTime",
            "getClusterNodes",
            "getEpochInfo",
            "getEpochSchedule",
            "getFeeForMessage",
            "getFirstAvailableBlock",
            "getGenesisHash",
            "getHealth",
            "getHighestSnapshotSlot",
            "getIdentity",
            "getInflationGovernor",
            "getInflationRate",
            "getInflationReward",
            "getLargestAccounts",
            "getLatestBlockhash",
            "getLeaderSchedule",
            "getMaxRetransmitSlot",
            "getMaxShredInsertSlot",
            "getMinimumBalanceForRentExemption",
            "getMultipleAccounts",
            "getProgramAccounts",
            "getRecentPerformanceSamples",
            "getRecentPrioritizationFees",
            "getSignatureStatuses",
            "getSignaturesForAddress",
            "getSlotLeader",
            "getSlotLeaders",
            "getStakeActivation",
            "getStakeMinimumDelegation",
            "getSupply",
            "getTokenAccountBalance",
            "getTokenAccountsByDelegate",
            "getTokenAccountsByOwner",
            "getTokenLargestAccounts",
            "getTokenSupply",
            "getTransaction",
            "getTransactionCount",
            "getVersion",
            "getVoteAccounts",
            "isBlockhashValid",
            "minimumLedgerSlot",
            "simulateTransaction",
        ];
        for method in &safe_methods {
            assert_eq!(
                classify_solana_method(method),
                MethodSafety::Safe,
                "expected {method} to be Safe"
            );
        }
    }

    // -- 2. classify_idempotent_methods -----------------------------------

    #[test]
    fn classify_idempotent_methods() {
        assert_eq!(
            classify_solana_method("sendTransaction"),
            MethodSafety::Idempotent,
        );
    }

    // -- 3. classify_unsafe_methods ---------------------------------------

    #[test]
    fn classify_unsafe_methods() {
        assert_eq!(
            classify_solana_method("requestAirdrop"),
            MethodSafety::Unsafe,
        );
    }

    // -- 4. retry_safety --------------------------------------------------

    #[test]
    fn retry_safety() {
        // Safe methods are retryable.
        assert!(is_solana_safe_to_retry("getBalance"));
        assert!(is_solana_safe_to_retry("getSlot"));
        assert!(is_solana_safe_to_retry("getAccountInfo"));

        // Idempotent is NOT safe to retry (by our conservative policy).
        assert!(!is_solana_safe_to_retry("sendTransaction"));

        // Unsafe is NOT safe to retry.
        assert!(!is_solana_safe_to_retry("requestAirdrop"));
    }

    // -- 5. commitment_default --------------------------------------------

    #[test]
    fn commitment_default() {
        assert_eq!(SolanaCommitment::default(), SolanaCommitment::Confirmed);
    }

    // -- 6. commitment_serialization --------------------------------------

    #[test]
    fn commitment_serialization() {
        let json = serde_json::to_string(&SolanaCommitment::Processed).unwrap();
        assert_eq!(json, "\"processed\"");

        let json = serde_json::to_string(&SolanaCommitment::Confirmed).unwrap();
        assert_eq!(json, "\"confirmed\"");

        let json = serde_json::to_string(&SolanaCommitment::Finalized).unwrap();
        assert_eq!(json, "\"finalized\"");

        // Round-trip.
        let parsed: SolanaCommitment =
            serde_json::from_str("\"finalized\"").unwrap();
        assert_eq!(parsed, SolanaCommitment::Finalized);
    }

    // -- 7. commitment_safety ---------------------------------------------

    #[test]
    fn commitment_safety() {
        // Finalized is safe for both indexing and display.
        assert!(SolanaCommitment::Finalized.is_safe_for_indexing());
        assert!(SolanaCommitment::Finalized.is_safe_for_display());

        // Confirmed is safe for display but not indexing.
        assert!(!SolanaCommitment::Confirmed.is_safe_for_indexing());
        assert!(SolanaCommitment::Confirmed.is_safe_for_display());

        // Processed is not safe for either.
        assert!(!SolanaCommitment::Processed.is_safe_for_indexing());
        assert!(!SolanaCommitment::Processed.is_safe_for_display());
    }

    // -- 8. cu_cost_table -------------------------------------------------

    #[test]
    fn cu_cost_table() {
        let table = SolanaCuCostTable::defaults();

        // Specific costs.
        assert_eq!(table.cost_for("getProgramAccounts"), 100);
        assert_eq!(table.cost_for("getSlot"), 5);
        assert_eq!(table.cost_for("getBlock"), 50);
        assert_eq!(table.cost_for("getBalance"), 10);
        assert_eq!(table.cost_for("sendTransaction"), 10);
        assert_eq!(table.cost_for("simulateTransaction"), 50);
        assert_eq!(table.cost_for("getMultipleAccounts"), 30);

        // getProgramAccounts (100) should cost more than getSlot (5).
        assert!(
            table.cost_for("getProgramAccounts") > table.cost_for("getSlot")
        );

        // Unknown methods get the default cost (15).
        assert_eq!(table.cost_for("someUnknownMethod"), 15);

        // Custom override.
        let mut custom = SolanaCuCostTable::new(42);
        custom.set_cost("getSlot", 999);
        assert_eq!(custom.cost_for("getSlot"), 999);
        assert_eq!(custom.cost_for("anythingElse"), 42);
    }

    // -- 9. inject_commitment ---------------------------------------------

    #[tokio::test]
    async fn inject_commitment() {
        let mock = Arc::new(MockTransport::new("https://api.devnet.solana.com"));
        let transport =
            SolanaTransport::new(Arc::clone(&mock) as Arc<dyn RpcTransport>, SolanaCommitment::Finalized);

        // -- (a) Method that accepts commitment and has no config object yet.
        let req = JsonRpcRequest::new(
            1,
            "getBalance",
            vec![Value::String(
                "83astBRguLMdt2h5U1Tbd4hAZbs9sRhfns3EGNHpGT8o".into(),
            )],
        );
        transport.send(req).await.unwrap();

        let sent = mock.sent_requests();
        assert_eq!(sent.len(), 1);
        let last_param = sent[0].params.last().unwrap();
        assert_eq!(
            last_param.get("commitment").and_then(Value::as_str),
            Some("finalized"),
        );

        // -- (b) Method that already has a config object with commitment.
        let mut config = serde_json::Map::new();
        config.insert(
            "commitment".to_string(),
            Value::String("processed".into()),
        );
        let req = JsonRpcRequest::new(
            2,
            "getAccountInfo",
            vec![
                Value::String("Vote111111111111111111111111111111111111111".into()),
                Value::Object(config),
            ],
        );
        transport.send(req).await.unwrap();

        let sent = mock.sent_requests();
        assert_eq!(sent.len(), 2);
        // The caller's commitment should be preserved, NOT overwritten.
        let last_param = sent[1].params.last().unwrap();
        assert_eq!(
            last_param.get("commitment").and_then(Value::as_str),
            Some("processed"),
        );

        // -- (c) Method that does NOT accept commitment (e.g. getVersion).
        let req = JsonRpcRequest::new(3, "getVersion", vec![]);
        transport.send(req).await.unwrap();

        let sent = mock.sent_requests();
        assert_eq!(sent.len(), 3);
        // No params should have been added.
        assert!(sent[2].params.is_empty());
    }

    // -- 10. solana_transport_delegates -----------------------------------

    #[tokio::test]
    async fn solana_transport_delegates() {
        let mock = Arc::new(MockTransport::new("https://api.mainnet-beta.solana.com"));
        let transport = SolanaTransport::new(
            Arc::clone(&mock) as Arc<dyn RpcTransport>,
            SolanaCommitment::Confirmed,
        );

        // url() delegates.
        assert_eq!(transport.url(), "https://api.mainnet-beta.solana.com");

        // health() delegates.
        assert_eq!(transport.health(), HealthStatus::Unknown);

        // send() delegates and returns the inner response.
        let req = JsonRpcRequest::new(1, "getSlot", vec![]);
        let resp = transport.send(req).await.unwrap();
        assert!(resp.is_ok());

        // send_batch() delegates.
        let reqs = vec![
            JsonRpcRequest::new(2, "getSlot", vec![]),
            JsonRpcRequest::new(3, "getBlockHeight", vec![]),
        ];
        let resps = transport.send_batch(reqs).await.unwrap();
        assert_eq!(resps.len(), 2);
        assert!(resps.iter().all(|r| r.is_ok()));

        // with_commitment() shares the inner transport.
        let finalized = transport.with_commitment(SolanaCommitment::Finalized);
        assert_eq!(finalized.commitment(), SolanaCommitment::Finalized);
        assert_eq!(finalized.url(), "https://api.mainnet-beta.solana.com");
    }

    // -- 11. endpoints_not_empty ------------------------------------------

    #[test]
    fn endpoints_not_empty() {
        assert!(!solana_mainnet_endpoints().is_empty());
        assert!(!solana_devnet_endpoints().is_empty());
        assert!(!solana_testnet_endpoints().is_empty());

        // Mainnet should include the official Solana endpoint.
        assert!(solana_mainnet_endpoints()
            .contains(&"https://api.mainnet-beta.solana.com"));
    }

    // -- 12. unknown_method_is_safe ---------------------------------------

    #[test]
    fn unknown_method_is_safe() {
        assert_eq!(
            classify_solana_method("customFooBarMethod"),
            MethodSafety::Safe,
        );
        assert!(is_solana_safe_to_retry("customFooBarMethod"));
        assert!(is_solana_safe_to_dedup("customFooBarMethod"));
        assert!(is_solana_cacheable("customFooBarMethod"));
    }

    // -- bonus: commitment Display impl -----------------------------------

    #[test]
    fn commitment_display() {
        assert_eq!(SolanaCommitment::Processed.to_string(), "processed");
        assert_eq!(SolanaCommitment::Confirmed.to_string(), "confirmed");
        assert_eq!(SolanaCommitment::Finalized.to_string(), "finalized");
    }

    // -- bonus: dedup and cacheable helpers --------------------------------

    #[test]
    fn dedup_and_cacheable() {
        assert!(is_solana_safe_to_dedup("getBalance"));
        assert!(!is_solana_safe_to_dedup("sendTransaction"));
        assert!(!is_solana_safe_to_dedup("requestAirdrop"));

        assert!(is_solana_cacheable("getAccountInfo"));
        assert!(!is_solana_cacheable("sendTransaction"));
        assert!(!is_solana_cacheable("requestAirdrop"));
    }

    // -- bonus: inject commitment into batch ------------------------------

    #[tokio::test]
    async fn inject_commitment_in_batch() {
        let mock = Arc::new(MockTransport::new("https://api.devnet.solana.com"));
        let transport = SolanaTransport::new(
            Arc::clone(&mock) as Arc<dyn RpcTransport>,
            SolanaCommitment::Finalized,
        );

        let reqs = vec![
            JsonRpcRequest::new(
                1,
                "getBalance",
                vec![Value::String("addr1".into())],
            ),
            JsonRpcRequest::new(2, "getVersion", vec![]),
        ];

        transport.send_batch(reqs).await.unwrap();

        let sent = mock.sent_requests();
        assert_eq!(sent.len(), 2);

        // getBalance should have commitment injected.
        let balance_params = &sent[0].params;
        let config = balance_params.last().unwrap();
        assert_eq!(
            config.get("commitment").and_then(Value::as_str),
            Some("finalized"),
        );

        // getVersion should NOT have commitment injected.
        assert!(sent[1].params.is_empty());
    }

    // -- bonus: SolanaCuCostTable Default trait ----------------------------

    #[test]
    fn cu_cost_table_default_trait() {
        let table: SolanaCuCostTable = Default::default();
        assert_eq!(table.cost_for("getBlock"), 50);
        assert_eq!(table.cost_for("unknown"), 15);
    }
}
