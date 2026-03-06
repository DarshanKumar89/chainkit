//! Provider capability routing — route requests to capable providers.
//!
//! Routes archive-state queries to archive nodes, trace calls to trace-capable
//! nodes, and recent queries to any available provider.

use std::collections::HashSet;

/// Capabilities of a provider node.
#[derive(Debug, Clone, Default)]
pub struct ProviderCapabilities {
    /// Whether the node has full historical state (archive mode).
    pub archive: bool,
    /// Whether the node supports debug_* and trace_* methods.
    pub trace: bool,
    /// Maximum block range supported for eth_getLogs (0 = unlimited).
    pub max_block_range: u64,
    /// Maximum batch size (0 = unlimited).
    pub max_batch_size: usize,
    /// Set of explicitly supported methods (empty = all methods).
    pub supported_methods: HashSet<String>,
}

impl ProviderCapabilities {
    /// Full node (recent state only, no traces).
    pub fn full_node() -> Self {
        Self {
            archive: false,
            trace: false,
            max_block_range: 10_000,
            max_batch_size: 100,
            supported_methods: HashSet::new(),
        }
    }

    /// Archive node with trace support.
    pub fn archive_node() -> Self {
        Self {
            archive: true,
            trace: true,
            max_block_range: 0,
            max_batch_size: 100,
            supported_methods: HashSet::new(),
        }
    }

    /// Check if this provider can handle the given request.
    pub fn can_handle(&self, requirement: &RequestRequirement) -> bool {
        if requirement.needs_archive && !self.archive {
            return false;
        }
        if requirement.needs_trace && !self.trace {
            return false;
        }
        if !self.supported_methods.is_empty() {
            if let Some(ref method) = requirement.method {
                if !self.supported_methods.contains(method.as_str()) {
                    return false;
                }
            }
        }
        true
    }
}

/// What a request requires from a provider.
#[derive(Debug, Clone, Default)]
pub struct RequestRequirement {
    /// Requires archive state (historical block queries).
    pub needs_archive: bool,
    /// Requires trace/debug capability.
    pub needs_trace: bool,
    /// The RPC method being called.
    pub method: Option<String>,
}

/// Determine the requirements for a given RPC method and parameters.
///
/// `block_param` is the block parameter if present (e.g. "0x1", "latest", "earliest").
/// `current_block` is the latest known block number.
pub fn analyze_request(
    method: &str,
    block_param: Option<&str>,
    current_block: u64,
) -> RequestRequirement {
    let mut req = RequestRequirement {
        method: Some(method.to_string()),
        ..Default::default()
    };

    // Trace/debug methods always need trace capability
    if method.starts_with("debug_") || method.starts_with("trace_") {
        req.needs_trace = true;
        req.needs_archive = true; // trace methods often need archive too
        return req;
    }

    // Check if block parameter refers to old history
    if let Some(block) = block_param {
        if is_historical_block(block, current_block) {
            req.needs_archive = true;
        }
    }

    req
}

/// Check if a block parameter refers to historical (likely pruned) state.
///
/// Full nodes typically keep ~128 blocks of state. We use a conservative
/// threshold of 256 blocks.
fn is_historical_block(block: &str, current_block: u64) -> bool {
    match block {
        "latest" | "pending" | "safe" | "finalized" => false,
        "earliest" => true,
        hex if hex.starts_with("0x") => {
            if let Ok(num) = u64::from_str_radix(hex.trim_start_matches("0x"), 16) {
                // If block is more than 256 blocks behind head, consider it historical
                current_block.saturating_sub(num) > 256
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Select the best provider index from a list of capabilities.
///
/// Returns the index of the first provider that can handle the request.
/// Prefers cheaper (non-archive) providers when the request doesn't need archive.
pub fn select_capable_provider(
    capabilities: &[ProviderCapabilities],
    allowed: &[bool],
    requirement: &RequestRequirement,
) -> Option<usize> {
    // First pass: find a matching allowed provider
    for (idx, (cap, &ok)) in capabilities.iter().zip(allowed.iter()).enumerate() {
        if ok && cap.can_handle(requirement) {
            return Some(idx);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_node_handles_recent() {
        let cap = ProviderCapabilities::full_node();
        let req = RequestRequirement::default();
        assert!(cap.can_handle(&req));
    }

    #[test]
    fn full_node_rejects_archive() {
        let cap = ProviderCapabilities::full_node();
        let req = RequestRequirement {
            needs_archive: true,
            ..Default::default()
        };
        assert!(!cap.can_handle(&req));
    }

    #[test]
    fn archive_node_handles_everything() {
        let cap = ProviderCapabilities::archive_node();
        assert!(cap.can_handle(&RequestRequirement {
            needs_archive: true,
            ..Default::default()
        }));
        assert!(cap.can_handle(&RequestRequirement {
            needs_trace: true,
            ..Default::default()
        }));
        assert!(cap.can_handle(&RequestRequirement::default()));
    }

    #[test]
    fn analyze_trace_method() {
        let req = analyze_request("debug_traceTransaction", None, 1000);
        assert!(req.needs_trace);
        assert!(req.needs_archive);
    }

    #[test]
    fn analyze_historical_block() {
        let req = analyze_request("eth_getBalance", Some("0x1"), 1_000_000);
        assert!(req.needs_archive);
    }

    #[test]
    fn analyze_recent_block() {
        let req = analyze_request("eth_getBalance", Some("latest"), 1_000_000);
        assert!(!req.needs_archive);
    }

    #[test]
    fn analyze_earliest() {
        let req = analyze_request("eth_getBalance", Some("earliest"), 1_000_000);
        assert!(req.needs_archive);
    }

    #[test]
    fn analyze_close_to_head() {
        let current = 1_000_000u64;
        let req = analyze_request(
            "eth_getBalance",
            Some(&format!("0x{:x}", current - 10)),
            current,
        );
        assert!(!req.needs_archive); // Only 10 blocks back
    }

    #[test]
    fn select_capable() {
        let caps = vec![
            ProviderCapabilities::full_node(),
            ProviderCapabilities::archive_node(),
        ];
        let allowed = [true, true];

        // Archive request should select provider 1
        let req = RequestRequirement {
            needs_archive: true,
            ..Default::default()
        };
        assert_eq!(select_capable_provider(&caps, &allowed, &req), Some(1));

        // Recent request should select provider 0 (first match)
        let req = RequestRequirement::default();
        assert_eq!(select_capable_provider(&caps, &allowed, &req), Some(0));
    }

    #[test]
    fn select_when_no_capable() {
        let caps = vec![ProviderCapabilities::full_node()];
        let allowed = [true];

        let req = RequestRequirement {
            needs_archive: true,
            ..Default::default()
        };
        assert_eq!(select_capable_provider(&caps, &allowed, &req), None);
    }
}
