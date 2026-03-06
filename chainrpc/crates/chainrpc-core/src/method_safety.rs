//! Method safety classification — prevents retrying unsafe (write) methods.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Classification of JSON-RPC method safety for retry/dedup decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodSafety {
    /// Safe to retry, deduplicate, and cache. Read-only methods.
    Safe,
    /// Idempotent — same tx hash is safe to re-submit, but NEVER auto-retry
    /// with different parameters. e.g. eth_sendRawTransaction (same raw tx = same hash).
    Idempotent,
    /// NEVER auto-retry. Retrying could cause double-spend or duplicate side effects.
    /// e.g. eth_sendTransaction (node signs, different nonce possible).
    Unsafe,
}

/// Classify a JSON-RPC method by its safety level.
pub fn classify_method(method: &str) -> MethodSafety {
    if unsafe_methods().contains(method) {
        MethodSafety::Unsafe
    } else if idempotent_methods().contains(method) {
        MethodSafety::Idempotent
    } else {
        MethodSafety::Safe
    }
}

/// Returns true if the method is safe to retry on transient failure.
pub fn is_safe_to_retry(method: &str) -> bool {
    classify_method(method) == MethodSafety::Safe
}

/// Returns true if the method is safe to deduplicate (coalesce concurrent identical requests).
pub fn is_safe_to_dedup(method: &str) -> bool {
    classify_method(method) == MethodSafety::Safe
}

/// Returns true if the method result can be cached.
pub fn is_cacheable(method: &str) -> bool {
    classify_method(method) == MethodSafety::Safe
}

fn unsafe_methods() -> &'static HashSet<&'static str> {
    static UNSAFE: OnceLock<HashSet<&'static str>> = OnceLock::new();
    UNSAFE.get_or_init(|| {
        [
            "eth_sendTransaction",
            "personal_sendTransaction",
            "eth_sign",
            "personal_sign",
            "eth_signTransaction",
        ]
        .into_iter()
        .collect()
    })
}

fn idempotent_methods() -> &'static HashSet<&'static str> {
    static IDEMPOTENT: OnceLock<HashSet<&'static str>> = OnceLock::new();
    IDEMPOTENT.get_or_init(|| ["eth_sendRawTransaction"].into_iter().collect())
}

// All other methods are Safe by default (eth_call, eth_getBalance, eth_blockNumber, etc.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_read_methods() {
        assert_eq!(classify_method("eth_blockNumber"), MethodSafety::Safe);
        assert_eq!(classify_method("eth_getBalance"), MethodSafety::Safe);
        assert_eq!(classify_method("eth_call"), MethodSafety::Safe);
        assert_eq!(classify_method("eth_getLogs"), MethodSafety::Safe);
        assert_eq!(
            classify_method("eth_getTransactionReceipt"),
            MethodSafety::Safe
        );
        assert_eq!(classify_method("eth_getBlockByNumber"), MethodSafety::Safe);
        assert_eq!(classify_method("eth_chainId"), MethodSafety::Safe);
        assert_eq!(classify_method("net_version"), MethodSafety::Safe);
    }

    #[test]
    fn idempotent_methods_test() {
        assert_eq!(
            classify_method("eth_sendRawTransaction"),
            MethodSafety::Idempotent
        );
    }

    #[test]
    fn unsafe_methods_test() {
        assert_eq!(classify_method("eth_sendTransaction"), MethodSafety::Unsafe);
        assert_eq!(
            classify_method("personal_sendTransaction"),
            MethodSafety::Unsafe
        );
        assert_eq!(classify_method("eth_sign"), MethodSafety::Unsafe);
    }

    #[test]
    fn retry_safety() {
        assert!(is_safe_to_retry("eth_blockNumber"));
        assert!(!is_safe_to_retry("eth_sendRawTransaction"));
        assert!(!is_safe_to_retry("eth_sendTransaction"));
    }

    #[test]
    fn dedup_safety() {
        assert!(is_safe_to_dedup("eth_getBalance"));
        assert!(!is_safe_to_dedup("eth_sendRawTransaction"));
    }

    #[test]
    fn unknown_methods_are_safe() {
        assert_eq!(classify_method("custom_rpc_method"), MethodSafety::Safe);
    }
}
