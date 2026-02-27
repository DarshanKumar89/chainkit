//! Error signature registry — maps 4-byte selectors to known error ABIs.

use serde::{Deserialize, Serialize};

/// A known error signature from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorSignature {
    /// Error name (e.g. `"InsufficientBalance"`).
    pub name: String,
    /// Full signature string (e.g. `"InsufficientBalance(address,uint256)"`).
    pub signature: String,
    /// 4-byte selector (keccak256 of signature, first 4 bytes).
    pub selector: [u8; 4],
    /// ABI-encoded parameter types in order.
    pub inputs: Vec<ErrorParam>,
    /// Source of this signature (e.g. `"bundled"`, `"4byte.directory"`, `"user"`).
    pub source: String,
    /// Optional human-readable suggestion for this error.
    pub suggestion: Option<String>,
}

/// A single parameter in an error signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorParam {
    /// Parameter name (may be empty for unnamed params).
    pub name: String,
    /// Solidity type string (e.g. `"address"`, `"uint256"`, `"bytes32"`).
    pub ty: String,
}

/// Trait for looking up error signatures by 4-byte selector.
pub trait ErrorSignatureRegistry: Send + Sync {
    /// Look up all known signatures matching a 4-byte selector.
    /// Returns multiple signatures when there are collisions.
    fn get_by_selector(&self, selector: [u8; 4]) -> Vec<ErrorSignature>;

    /// Look up a signature by full name (e.g. `"InsufficientBalance"`).
    fn get_by_name(&self, name: &str) -> Option<ErrorSignature>;

    /// Total number of registered signatures.
    fn len(&self) -> usize;

    /// Returns `true` if the registry is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ─── In-memory registry ───────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::RwLock;

/// A simple in-memory registry backed by `HashMap`.
pub struct MemoryErrorRegistry {
    /// selector → list of signatures (handle collisions)
    by_selector: RwLock<HashMap<[u8; 4], Vec<ErrorSignature>>>,
    /// name → first registered signature
    by_name: RwLock<HashMap<String, ErrorSignature>>,
}

impl MemoryErrorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            by_selector: RwLock::new(HashMap::new()),
            by_name: RwLock::new(HashMap::new()),
        }
    }

    /// Register a signature.
    pub fn register(&self, sig: ErrorSignature) {
        let mut by_sel = self.by_selector.write().unwrap();
        by_sel.entry(sig.selector).or_default().push(sig.clone());
        let mut by_name = self.by_name.write().unwrap();
        by_name.entry(sig.name.clone()).or_insert(sig);
    }

    /// Load signatures from a JSON array string.
    /// Expected format: `[{ "name": "...", "signature": "...", ... }, ...]`
    pub fn load_json(&self, json: &str) -> Result<usize, serde_json::Error> {
        let sigs: Vec<ErrorSignature> = serde_json::from_str(json)?;
        let count = sigs.len();
        for sig in sigs {
            self.register(sig);
        }
        Ok(count)
    }
}

impl Default for MemoryErrorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorSignatureRegistry for MemoryErrorRegistry {
    fn get_by_selector(&self, selector: [u8; 4]) -> Vec<ErrorSignature> {
        self.by_selector
            .read()
            .unwrap()
            .get(&selector)
            .cloned()
            .unwrap_or_default()
    }

    fn get_by_name(&self, name: &str) -> Option<ErrorSignature> {
        self.by_name.read().unwrap().get(name).cloned()
    }

    fn len(&self) -> usize {
        self.by_selector.read().unwrap().values().map(|v| v.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sig(name: &str, sig_str: &str, selector: [u8; 4]) -> ErrorSignature {
        ErrorSignature {
            name: name.to_string(),
            signature: sig_str.to_string(),
            selector,
            inputs: vec![],
            source: "test".to_string(),
            suggestion: None,
        }
    }

    #[test]
    fn register_and_lookup() {
        let reg = MemoryErrorRegistry::new();
        let selector = [0x08, 0xc3, 0x79, 0xa0];
        reg.register(make_sig("Error", "Error(string)", selector));

        let results = reg.get_by_selector(selector);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Error");
    }

    #[test]
    fn lookup_by_name() {
        let reg = MemoryErrorRegistry::new();
        reg.register(make_sig("Foo", "Foo(uint256)", [0x00, 0x00, 0x00, 0x01]));
        assert!(reg.get_by_name("Foo").is_some());
        assert!(reg.get_by_name("Bar").is_none());
    }

    #[test]
    fn load_json_empty_array() {
        let reg = MemoryErrorRegistry::new();
        let count = reg.load_json("[]").unwrap();
        assert_eq!(count, 0);
        assert!(reg.is_empty());
    }
}
