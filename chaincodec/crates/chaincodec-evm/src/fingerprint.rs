//! EVM event fingerprint computation.
//!
//! The fingerprint of an EVM event is the keccak256 hash of its canonical
//! signature string, e.g.:
//!   keccak256("Swap(address,address,int256,int256,uint160,uint128,int24)")
//!   → 0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67
//!
//! For raw events, topics[0] IS the fingerprint — we do not need to recompute it.

use chaincodec_core::event::EventFingerprint;
use tiny_keccak::{Hasher, Keccak};

/// Compute the keccak256 fingerprint of an event signature string.
/// Input: `"EventName(type1,type2,...)"` — the canonical ABI signature.
pub fn keccak256_signature(signature: &str) -> EventFingerprint {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut output);
    EventFingerprint::new(format!("0x{}", hex::encode(output)))
}

/// Extract the fingerprint directly from a raw EVM event (topics[0]).
/// Returns `None` if topics is empty or the first topic is malformed.
pub fn from_topics(topics: &[String]) -> Option<EventFingerprint> {
    let first = topics.first()?;
    // Validate it looks like a 32-byte hex hash
    let hex = first.strip_prefix("0x").unwrap_or(first);
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(EventFingerprint::new(first.clone()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniswap_v3_swap_fingerprint() {
        // Well-known fingerprint for Uniswap V3 Swap event
        let sig = "Swap(address,address,int256,int256,uint160,uint128,int24)";
        let fp = keccak256_signature(sig);
        assert_eq!(
            fp.as_hex(),
            "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"
        );
    }

    #[test]
    fn erc20_transfer_fingerprint() {
        let sig = "Transfer(address,address,uint256)";
        let fp = keccak256_signature(sig);
        assert_eq!(
            fp.as_hex(),
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        );
    }

    #[test]
    fn from_topics_valid() {
        let topics = vec![
            "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67".to_string(),
        ];
        let fp = from_topics(&topics);
        assert!(fp.is_some());
    }

    #[test]
    fn from_topics_empty() {
        assert!(from_topics(&[]).is_none());
    }
}
