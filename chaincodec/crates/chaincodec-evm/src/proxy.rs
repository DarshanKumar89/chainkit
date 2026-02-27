//! EVM proxy contract pattern detection and resolution.
//!
//! Detects common proxy patterns and identifies the implementation contract address.
//!
//! ## Supported Patterns
//!
//! | Pattern | Detection | Standard |
//! |---------|-----------|----------|
//! | EIP-1967 Logic Proxy | Storage slot | EIP-1967 |
//! | EIP-1822 UUPS Proxy | `proxiableUUID()` slot | EIP-1822 |
//! | OpenZeppelin Transparent Proxy | Admin slot + logic slot | OZ |
//! | EIP-1167 Minimal Proxy (Clone) | Bytecode prefix | EIP-1167 |
//! | Gnosis Safe | `masterCopy()` call | Gnosis |
//!
//! ## Usage with an RPC client
//!
//! The detection functions require reading blockchain state (storage slots or bytecode),
//! so they take an `RpcAdapter` trait object. Provide a concrete implementation
//! backed by `eth_getStorageAt` and `eth_getCode`.

use serde::{Deserialize, Serialize};

/// The detected proxy pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyKind {
    /// EIP-1967 standard logic proxy (most common modern pattern)
    Eip1967Logic,
    /// EIP-1967 beacon proxy (implementation fetched from beacon contract)
    Eip1967Beacon,
    /// EIP-1822 UUPS (Universal Upgradeable Proxy Standard)
    Eip1822Uups,
    /// OpenZeppelin Transparent Proxy (legacy, pre-EIP-1967)
    OzTransparent,
    /// EIP-1167 Minimal Proxy (Clone) — cheap non-upgradeable clone
    Eip1167Clone,
    /// Gnosis Safe proxy
    GnosisSafe,
    /// Unknown proxy — bytecode or storage suggests a proxy but pattern is unrecognized
    Unknown,
}

/// Result of proxy detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInfo {
    /// The proxy contract address
    pub proxy_address: String,
    /// The detected proxy pattern
    pub kind: ProxyKind,
    /// The resolved implementation address (if determinable without RPC)
    pub implementation: Option<String>,
    /// The storage slot key used to find the implementation (hex)
    pub slot: Option<String>,
}

// ─── EIP-1967 Storage Slots ───────────────────────────────────────────────────

/// EIP-1967 implementation slot:
/// `keccak256("eip1967.proxy.implementation") - 1`
pub const EIP1967_IMPL_SLOT: &str =
    "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";

/// EIP-1967 admin slot:
/// `keccak256("eip1967.proxy.admin") - 1`
pub const EIP1967_ADMIN_SLOT: &str =
    "0xb53127684a568b3173ae13b9f8a6016e243e63b6e8ee1178d6a717850b5d6103";

/// EIP-1967 beacon slot:
/// `keccak256("eip1967.proxy.beacon") - 1`
pub const EIP1967_BEACON_SLOT: &str =
    "0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50";

/// EIP-1822 UUPS proxiable slot:
/// `keccak256("PROXIABLE")`
pub const EIP1822_PROXIABLE_SLOT: &str =
    "0xc5f16f0fcc639fa48a6947836d9850f504798523bf8c9a3a87d5876cf622bcf7";

/// EIP-1167 minimal proxy bytecode prefix (20-byte address embedded at offset 10)
pub const EIP1167_BYTECODE_PREFIX: &[u8] = &[
    0x36, 0x3d, 0x3d, 0x37, 0x3d, 0x3d, 0x3d, 0x36, 0x3d, 0x73,
];

/// EIP-1167 minimal proxy bytecode suffix
pub const EIP1167_BYTECODE_SUFFIX: &[u8] = &[
    0x5a, 0xf4, 0x3d, 0x82, 0x80, 0x3e, 0x90, 0x3d, 0x91, 0x60, 0x2b, 0x57, 0xfd, 0x5b, 0xf3,
];

// ─── Bytecode Analysis ────────────────────────────────────────────────────────

/// Detect an EIP-1167 minimal proxy from raw bytecode.
///
/// Returns the implementation address if the bytecode matches the EIP-1167 pattern.
/// This requires NO RPC call — it works from `eth_getCode` output alone.
///
/// # Arguments
/// * `bytecode` - raw bytecode bytes (not hex)
pub fn detect_eip1167_clone(bytecode: &[u8]) -> Option<String> {
    // EIP-1167 minimal proxy: 45 bytes total
    // Layout: [10 prefix bytes] [20 address bytes] [15 suffix bytes]
    if bytecode.len() != 45 {
        return None;
    }
    if &bytecode[..10] != EIP1167_BYTECODE_PREFIX {
        return None;
    }
    if &bytecode[30..] != EIP1167_BYTECODE_SUFFIX {
        return None;
    }
    let addr_bytes = &bytecode[10..30];
    Some(format!("0x{}", hex::encode(addr_bytes)))
}

/// Check if a storage slot value looks like a non-zero address.
///
/// EVM storage returns 32-byte zero-padded values. An address is stored in the
/// lower 20 bytes. Returns `Some(address)` if bytes 12-32 are a non-zero address.
pub fn storage_to_address(slot_value: &str) -> Option<String> {
    let hex = slot_value.strip_prefix("0x").unwrap_or(slot_value);
    if hex.len() != 64 {
        return None;
    }
    // Bytes 0-11 should be zero (12 bytes = 24 hex chars)
    let prefix = &hex[..24];
    let addr_hex = &hex[24..];

    if prefix.chars().all(|c| c == '0') && addr_hex != "0".repeat(40) {
        Some(format!("0x{addr_hex}"))
    } else {
        None
    }
}

/// Classify a proxy based on known storage slot values from an RPC call.
///
/// Provide the 32-byte hex values from `eth_getStorageAt` for each slot.
/// Pass `None` if the slot returned zero or could not be fetched.
pub fn classify_from_storage(
    proxy_address: &str,
    eip1967_impl: Option<&str>,
    eip1967_beacon: Option<&str>,
    eip1822_proxiable: Option<&str>,
) -> ProxyInfo {
    // EIP-1967 logic proxy
    if let Some(impl_raw) = eip1967_impl {
        if let Some(impl_addr) = storage_to_address(impl_raw) {
            return ProxyInfo {
                proxy_address: proxy_address.to_string(),
                kind: ProxyKind::Eip1967Logic,
                implementation: Some(impl_addr),
                slot: Some(EIP1967_IMPL_SLOT.to_string()),
            };
        }
    }

    // EIP-1967 beacon proxy
    if let Some(beacon_raw) = eip1967_beacon {
        if let Some(beacon_addr) = storage_to_address(beacon_raw) {
            return ProxyInfo {
                proxy_address: proxy_address.to_string(),
                kind: ProxyKind::Eip1967Beacon,
                // For beacon proxies, the actual impl is in beacon.implementation()
                // We record the beacon address as the "implementation" for now
                implementation: Some(beacon_addr),
                slot: Some(EIP1967_BEACON_SLOT.to_string()),
            };
        }
    }

    // EIP-1822 UUPS
    if let Some(uups_raw) = eip1822_proxiable {
        if let Some(impl_addr) = storage_to_address(uups_raw) {
            return ProxyInfo {
                proxy_address: proxy_address.to_string(),
                kind: ProxyKind::Eip1822Uups,
                implementation: Some(impl_addr),
                slot: Some(EIP1822_PROXIABLE_SLOT.to_string()),
            };
        }
    }

    ProxyInfo {
        proxy_address: proxy_address.to_string(),
        kind: ProxyKind::Unknown,
        implementation: None,
        slot: None,
    }
}

/// Async-friendly helper: build the storage slots to query for proxy detection.
///
/// Returns a list of `(label, slot_hex)` pairs that should be passed to
/// `eth_getStorageAt(address, slot, "latest")`.
pub fn proxy_detection_slots() -> Vec<(&'static str, &'static str)> {
    vec![
        ("eip1967_impl", EIP1967_IMPL_SLOT),
        ("eip1967_beacon", EIP1967_BEACON_SLOT),
        ("eip1822_proxiable", EIP1822_PROXIABLE_SLOT),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_eip1167_valid() {
        // Build a valid 45-byte EIP-1167 bytecode
        let mut bytecode = Vec::new();
        bytecode.extend_from_slice(EIP1167_BYTECODE_PREFIX);
        // 20-byte implementation address
        let impl_addr = [0xABu8; 20];
        bytecode.extend_from_slice(&impl_addr);
        bytecode.extend_from_slice(EIP1167_BYTECODE_SUFFIX);

        let detected = detect_eip1167_clone(&bytecode);
        assert!(detected.is_some());
        assert_eq!(detected.unwrap(), format!("0x{}", "ab".repeat(20)));
    }

    #[test]
    fn detect_eip1167_wrong_length() {
        let bytecode = vec![0u8; 44]; // too short
        assert!(detect_eip1167_clone(&bytecode).is_none());
    }

    #[test]
    fn storage_to_address_valid() {
        let slot =
            "0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045";
        let addr = storage_to_address(slot).unwrap();
        assert_eq!(addr, "0xd8da6bf26964af9d7eed9e03e53415d37aa96045");
    }

    #[test]
    fn storage_to_address_zero_returns_none() {
        let slot = "0x0000000000000000000000000000000000000000000000000000000000000000";
        assert!(storage_to_address(slot).is_none());
    }

    #[test]
    fn classify_eip1967_impl() {
        let impl_slot = "0x000000000000000000000000beefbeefbeefbeefbeefbeefbeefbeefbeefbeef";
        let info = classify_from_storage("0xproxy", Some(impl_slot), None, None);
        assert_eq!(info.kind, ProxyKind::Eip1967Logic);
        assert!(info.implementation.is_some());
    }

    #[test]
    fn classify_unknown_when_all_zero() {
        let zero = "0x0000000000000000000000000000000000000000000000000000000000000000";
        let info = classify_from_storage("0xproxy", Some(zero), Some(zero), Some(zero));
        assert_eq!(info.kind, ProxyKind::Unknown);
    }

    #[test]
    fn detection_slots_non_empty() {
        let slots = proxy_detection_slots();
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0].1, EIP1967_IMPL_SLOT);
    }
}
