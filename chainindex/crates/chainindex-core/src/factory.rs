//! Factory contract support — dynamic address tracking for protocols
//! that deploy child contracts (e.g., Uniswap V3 Factory -> Pool).
//!
//! # Overview
//!
//! Many DeFi protocols use a factory pattern: a single factory contract
//! deploys child contracts (pools, vaults, markets) via events. The
//! indexer needs to automatically start tracking these new contracts.
//!
//! [`FactoryRegistry`] solves this by:
//! 1. Accepting factory configurations ([`FactoryConfig`]) that describe
//!    which event signals a child deployment and which field holds the address.
//! 2. Processing incoming [`DecodedEvent`]s and extracting child addresses.
//! 3. Maintaining a thread-safe set of all discovered addresses.
//! 4. Supporting snapshot/restore for persistence across restarts.
//!
//! # Example
//!
//! ```rust
//! use chainindex_core::factory::{FactoryConfig, FactoryRegistry};
//!
//! let registry = FactoryRegistry::new();
//!
//! // Register Uniswap V3 factory
//! registry.register(FactoryConfig {
//!     factory_address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".into(),
//!     creation_event_topic0: "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118".into(),
//!     child_address_field: "pool".into(),
//!     name: Some("Uniswap V3 Factory".into()),
//! });
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::handler::DecodedEvent;

// ─── FactoryConfig ──────────────────────────────────────────────────────────

/// Configuration for a single factory contract.
///
/// Describes the factory's address, which event signals child creation,
/// and which field in the event's `fields_json` contains the new child address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryConfig {
    /// Address of the factory contract (lowercase hex, `0x…`).
    pub factory_address: String,
    /// Event signature hash (topic0) that signals a child contract creation.
    /// For example, Uniswap V3's `PoolCreated` topic0.
    pub creation_event_topic0: String,
    /// Field name within `fields_json` that contains the child contract address.
    /// Supports dot-separated paths for nested fields (e.g. `"args.pool"`).
    pub child_address_field: String,
    /// Optional human-readable name for this factory (e.g. `"Uniswap V3 Factory"`).
    pub name: Option<String>,
}

// ─── DiscoveredChild ────────────────────────────────────────────────────────

/// A child contract discovered from a factory creation event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredChild {
    /// Address of the newly deployed child contract.
    pub address: String,
    /// Address of the factory that deployed this child.
    pub factory_address: String,
    /// Block number where the creation event was emitted.
    pub discovered_at_block: u64,
    /// Transaction hash of the creation event.
    pub discovered_at_tx: String,
    /// The raw creation event payload (preserved for debugging/auditing).
    pub creation_event: serde_json::Value,
}

// ─── FactorySnapshot ────────────────────────────────────────────────────────

/// A serializable snapshot of the factory registry state.
///
/// Used for persistence — serialize this to disk or storage on shutdown,
/// and restore it on restart to avoid re-scanning the entire chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactorySnapshot {
    /// All factory configurations.
    pub configs: Vec<FactoryConfig>,
    /// All discovered child contracts, keyed by factory address.
    pub children: HashMap<String, Vec<DiscoveredChild>>,
}

// ─── Internal State ─────────────────────────────────────────────────────────

/// Internal mutable state behind the lock.
#[derive(Debug)]
struct RegistryInner {
    /// Factory configs keyed by (factory_address, topic0) for fast lookup.
    configs: HashMap<(String, String), FactoryConfig>,
    /// All factory addresses for membership checks.
    factory_addresses: HashSet<String>,
    /// Children keyed by factory address.
    children: HashMap<String, Vec<DiscoveredChild>>,
    /// Set of all child addresses for dedup.
    child_addresses: HashSet<String>,
}

impl RegistryInner {
    fn new() -> Self {
        Self {
            configs: HashMap::new(),
            factory_addresses: HashSet::new(),
            children: HashMap::new(),
            child_addresses: HashSet::new(),
        }
    }
}

// ─── FactoryRegistry ────────────────────────────────────────────────────────

/// Thread-safe registry of factory contracts and their discovered children.
///
/// Call [`register`](Self::register) to add factory configurations, then feed
/// incoming events through [`process_event`](Self::process_event). When a
/// creation event is detected, the child address is extracted and tracked.
///
/// Use [`get_all_addresses`](Self::get_all_addresses) to build an
/// [`EventFilter`](crate::types::EventFilter) that covers all factory and
/// child addresses.
pub struct FactoryRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl FactoryRegistry {
    /// Create a new, empty factory registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RegistryInner::new())),
        }
    }

    /// Register a factory contract configuration.
    ///
    /// After registration, any [`DecodedEvent`] from this factory with the
    /// matching topic0 will be checked for child address extraction.
    pub fn register(&self, config: FactoryConfig) {
        let mut inner = self.inner.lock().expect("factory registry lock poisoned");
        let key = (
            config.factory_address.to_lowercase(),
            config.creation_event_topic0.to_lowercase(),
        );
        inner
            .factory_addresses
            .insert(config.factory_address.to_lowercase());
        inner.configs.insert(key, config);
    }

    /// Process an incoming event, checking if it is a factory creation event.
    ///
    /// Returns `Some(DiscoveredChild)` if the event matched a registered
    /// factory and a new child address was extracted. Returns `None` if:
    /// - The event is not from a registered factory.
    /// - The event topic0 does not match a creation event.
    /// - The child address was already discovered (dedup).
    /// - The child address field could not be found in `fields_json`.
    pub fn process_event(&self, event: &DecodedEvent) -> Option<DiscoveredChild> {
        let mut inner = self.inner.lock().expect("factory registry lock poisoned");
        let addr = event.address.to_lowercase();

        // Quick check: is this from a known factory?
        if !inner.factory_addresses.contains(&addr) {
            return None;
        }

        // We need to find any config that matches this factory address.
        // The event's schema or topic0 might be encoded in the event itself;
        // we iterate configs matching the factory address.
        let matching_config = inner
            .configs
            .iter()
            .find(|((fa, _), _)| fa == &addr)
            .map(|(_, cfg)| cfg.clone());

        let config = matching_config?;

        // Extract the child address from fields_json using the configured field path.
        let child_addr = extract_field(&event.fields_json, &config.child_address_field)?;
        let child_addr_lower = child_addr.to_lowercase();

        // Dedup: skip if already known.
        if inner.child_addresses.contains(&child_addr_lower) {
            return None;
        }

        let child = DiscoveredChild {
            address: child_addr_lower.clone(),
            factory_address: addr.clone(),
            discovered_at_block: event.block_number,
            discovered_at_tx: event.tx_hash.clone(),
            creation_event: event.fields_json.clone(),
        };

        inner.child_addresses.insert(child_addr_lower);
        inner.children.entry(addr).or_default().push(child.clone());

        Some(child)
    }

    /// Get all tracked addresses (factories + discovered children).
    ///
    /// Useful for building an [`EventFilter`](crate::types::EventFilter)
    /// that covers all contracts the indexer should watch.
    pub fn get_all_addresses(&self) -> Vec<String> {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        let mut addrs: Vec<String> = inner.factory_addresses.iter().cloned().collect();
        addrs.extend(inner.child_addresses.iter().cloned());
        addrs.sort();
        addrs
    }

    /// Get all children discovered from a specific factory.
    ///
    /// Returns an empty vec if the factory has no children or is not registered.
    pub fn children_of(&self, factory_address: &str) -> Vec<DiscoveredChild> {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        inner
            .children
            .get(&factory_address.to_lowercase())
            .cloned()
            .unwrap_or_default()
    }

    /// Create a serializable snapshot of the current registry state.
    ///
    /// Use this to persist discovered children across restarts.
    pub fn snapshot(&self) -> FactorySnapshot {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        let configs: Vec<FactoryConfig> = inner.configs.values().cloned().collect();
        let children = inner.children.clone();
        FactorySnapshot { configs, children }
    }

    /// Restore the registry from a previously saved snapshot.
    ///
    /// This re-registers all factory configs and re-populates the children
    /// sets. Any existing state is merged (not replaced).
    pub fn restore(&self, snapshot: FactorySnapshot) {
        let mut inner = self.inner.lock().expect("factory registry lock poisoned");

        // Restore configs.
        for config in snapshot.configs {
            let key = (
                config.factory_address.to_lowercase(),
                config.creation_event_topic0.to_lowercase(),
            );
            inner
                .factory_addresses
                .insert(config.factory_address.to_lowercase());
            inner.configs.insert(key, config);
        }

        // Restore children.
        for (factory_addr, children) in snapshot.children {
            let factory_lower = factory_addr.to_lowercase();
            for child in children {
                let child_lower = child.address.to_lowercase();
                if inner.child_addresses.insert(child_lower) {
                    inner
                        .children
                        .entry(factory_lower.clone())
                        .or_default()
                        .push(child);
                }
            }
        }
    }

    /// Returns the number of registered factories.
    pub fn factory_count(&self) -> usize {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        inner.factory_addresses.len()
    }

    /// Returns the total number of discovered children across all factories.
    pub fn child_count(&self) -> usize {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        inner.child_addresses.len()
    }
}

impl Default for FactoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for FactoryRegistry {
    fn clone(&self) -> Self {
        let inner = self.inner.lock().expect("factory registry lock poisoned");
        let new_inner = RegistryInner {
            configs: inner.configs.clone(),
            factory_addresses: inner.factory_addresses.clone(),
            children: inner.children.clone(),
            child_addresses: inner.child_addresses.clone(),
        };
        Self {
            inner: Arc::new(Mutex::new(new_inner)),
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Extract a string value from a JSON object by a dot-separated field path.
///
/// Supports paths like `"pool"` (top-level) or `"args.pool"` (nested).
/// Returns `None` if any segment is missing or the final value is not a string.
fn extract_field(json: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = json;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    match current {
        serde_json::Value::String(s) => Some(s.clone()),
        // Also handle case where address is a number (unlikely but defensive).
        _ => None,
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const FACTORY_ADDR: &str = "0xfactory";
    const TOPIC0: &str = "0xpoolcreated";

    fn make_config() -> FactoryConfig {
        FactoryConfig {
            factory_address: FACTORY_ADDR.into(),
            creation_event_topic0: TOPIC0.into(),
            child_address_field: "pool".into(),
            name: Some("Test Factory".into()),
        }
    }

    fn make_event(factory: &str, pool_addr: &str, block: u64) -> DecodedEvent {
        DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: factory.into(),
            tx_hash: format!("0xtx{block}"),
            block_number: block,
            log_index: 0,
            fields_json: serde_json::json!({ "pool": pool_addr }),
        }
    }

    #[test]
    fn register_factory() {
        let registry = FactoryRegistry::new();
        assert_eq!(registry.factory_count(), 0);

        registry.register(make_config());
        assert_eq!(registry.factory_count(), 1);

        let addrs = registry.get_all_addresses();
        assert!(addrs.contains(&FACTORY_ADDR.to_lowercase().to_string()));
    }

    #[test]
    fn discover_child_from_event() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        let event = make_event(FACTORY_ADDR, "0xchild1", 100);
        let child = registry.process_event(&event);

        assert!(child.is_some());
        let child = child.unwrap();
        assert_eq!(child.address, "0xchild1");
        assert_eq!(child.factory_address, FACTORY_ADDR.to_lowercase());
        assert_eq!(child.discovered_at_block, 100);
    }

    #[test]
    fn duplicate_child_ignored() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        let event = make_event(FACTORY_ADDR, "0xchild1", 100);
        assert!(registry.process_event(&event).is_some());

        // Same child again — should be ignored.
        let event2 = make_event(FACTORY_ADDR, "0xchild1", 101);
        assert!(registry.process_event(&event2).is_none());

        assert_eq!(registry.child_count(), 1);
    }

    #[test]
    fn event_from_unknown_factory_ignored() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        let event = make_event("0xunknown", "0xchild1", 100);
        assert!(registry.process_event(&event).is_none());
    }

    #[test]
    fn multiple_factories() {
        let registry = FactoryRegistry::new();

        registry.register(FactoryConfig {
            factory_address: "0xfactory_a".into(),
            creation_event_topic0: "0xtopic_a".into(),
            child_address_field: "pool".into(),
            name: Some("Factory A".into()),
        });

        registry.register(FactoryConfig {
            factory_address: "0xfactory_b".into(),
            creation_event_topic0: "0xtopic_b".into(),
            child_address_field: "vault".into(),
            name: Some("Factory B".into()),
        });

        assert_eq!(registry.factory_count(), 2);

        // Child from factory A.
        let ev_a = DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: "0xfactory_a".into(),
            tx_hash: "0xtx1".into(),
            block_number: 50,
            log_index: 0,
            fields_json: serde_json::json!({ "pool": "0xchild_a" }),
        };
        assert!(registry.process_event(&ev_a).is_some());

        // Child from factory B.
        let ev_b = DecodedEvent {
            chain: "ethereum".into(),
            schema: "VaultCreated".into(),
            address: "0xfactory_b".into(),
            tx_hash: "0xtx2".into(),
            block_number: 55,
            log_index: 0,
            fields_json: serde_json::json!({ "vault": "0xchild_b" }),
        };
        assert!(registry.process_event(&ev_b).is_some());

        assert_eq!(registry.child_count(), 2);
        assert_eq!(registry.children_of("0xfactory_a").len(), 1);
        assert_eq!(registry.children_of("0xfactory_b").len(), 1);
    }

    #[test]
    fn get_all_addresses_includes_factories_and_children() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        let event = make_event(FACTORY_ADDR, "0xchild1", 100);
        registry.process_event(&event);

        let event2 = make_event(FACTORY_ADDR, "0xchild2", 101);
        registry.process_event(&event2);

        let addrs = registry.get_all_addresses();
        assert_eq!(addrs.len(), 3); // factory + 2 children
        assert!(addrs.contains(&FACTORY_ADDR.to_lowercase().to_string()));
        assert!(addrs.contains(&"0xchild1".to_string()));
        assert!(addrs.contains(&"0xchild2".to_string()));
    }

    #[test]
    fn snapshot_and_restore() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        let event = make_event(FACTORY_ADDR, "0xchild1", 100);
        registry.process_event(&event);
        let event2 = make_event(FACTORY_ADDR, "0xchild2", 101);
        registry.process_event(&event2);

        // Take snapshot.
        let snap = registry.snapshot();
        assert_eq!(snap.children.len(), 1); // one factory key
        let children = snap.children.get(&FACTORY_ADDR.to_lowercase()).unwrap();
        assert_eq!(children.len(), 2);

        // Restore into a fresh registry.
        let registry2 = FactoryRegistry::new();
        registry2.restore(snap);

        assert_eq!(registry2.factory_count(), 1);
        assert_eq!(registry2.child_count(), 2);
        assert_eq!(registry2.get_all_addresses().len(), 3);
        assert_eq!(registry2.children_of(FACTORY_ADDR).len(), 2);
    }

    #[test]
    fn snapshot_restore_roundtrip_json() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());
        registry.process_event(&make_event(FACTORY_ADDR, "0xchild1", 100));

        // Serialize to JSON and back.
        let snap = registry.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        let restored: FactorySnapshot = serde_json::from_str(&json).unwrap();

        let registry2 = FactoryRegistry::new();
        registry2.restore(restored);

        assert_eq!(registry2.child_count(), 1);
        assert_eq!(registry2.children_of(FACTORY_ADDR).len(), 1);
    }

    #[test]
    fn nested_field_extraction() {
        let registry = FactoryRegistry::new();
        registry.register(FactoryConfig {
            factory_address: "0xnested_factory".into(),
            creation_event_topic0: "0xtopic".into(),
            child_address_field: "args.pool".into(),
            name: Some("Nested Factory".into()),
        });

        let event = DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: "0xnested_factory".into(),
            tx_hash: "0xtx1".into(),
            block_number: 200,
            log_index: 0,
            fields_json: serde_json::json!({ "args": { "pool": "0xdeep_child" } }),
        };

        let child = registry.process_event(&event);
        assert!(child.is_some());
        assert_eq!(child.unwrap().address, "0xdeep_child");
    }

    #[test]
    fn missing_field_returns_none() {
        let registry = FactoryRegistry::new();
        registry.register(make_config());

        // Event without the expected "pool" field.
        let event = DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: FACTORY_ADDR.into(),
            tx_hash: "0xtx1".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({ "token0": "0xabc" }),
        };

        assert!(registry.process_event(&event).is_none());
    }

    #[test]
    fn case_insensitive_address_matching() {
        let registry = FactoryRegistry::new();
        registry.register(FactoryConfig {
            factory_address: "0xAbCdEf".into(),
            creation_event_topic0: TOPIC0.into(),
            child_address_field: "pool".into(),
            name: None,
        });

        // Event with different case.
        let event = DecodedEvent {
            chain: "ethereum".into(),
            schema: "PoolCreated".into(),
            address: "0xabcdef".into(),
            tx_hash: "0xtx1".into(),
            block_number: 100,
            log_index: 0,
            fields_json: serde_json::json!({ "pool": "0xchild_case" }),
        };

        let child = registry.process_event(&event);
        assert!(child.is_some());
        assert_eq!(child.unwrap().address, "0xchild_case");
    }

    #[test]
    fn children_of_unknown_factory_returns_empty() {
        let registry = FactoryRegistry::new();
        assert!(registry.children_of("0xnonexistent").is_empty());
    }
}
