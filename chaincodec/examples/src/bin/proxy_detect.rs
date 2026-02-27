//! # proxy_detect
//!
//! Demonstrates proxy contract pattern detection using `ProxyInfo`.
//!
//! ## Use-case coverage (chaincodec-usecase.md §4 — Security)
//! - Detect upgradeable contract proxies for security audits
//! - Monitor `Upgraded(address)` events to catch unexpected implementation swaps
//! - Identify EIP-1167 clones (cheap non-upgradeable copies)
//! - Know which storage slots to query with `eth_getStorageAt`
//!
//! Run with:
//! ```sh
//! cargo run --bin proxy_detect
//! ```

use chaincodec_evm::proxy::{
    classify_from_storage, detect_eip1167_clone, proxy_detection_slots, EIP1167_BYTECODE_PREFIX,
    EIP1167_BYTECODE_SUFFIX, EIP1967_IMPL_SLOT,
};

fn main() {
    println!("ChainCodec — Proxy Contract Detector");
    println!("═══════════════════════════════════════════════════════");

    // ── 1. Storage slots to query ──────────────────────────────────────────────
    //
    // To detect proxies via RPC you call:
    //   eth_getStorageAt(contract_address, slot, "latest")
    // for each of these slots, then pass the results to classify_from_storage().
    println!("\nStorage slots to query via eth_getStorageAt:");
    for (label, slot) in proxy_detection_slots() {
        println!("  {:<20} {}", label, slot);
    }

    // ── 2. EIP-1967 Logic Proxy ────────────────────────────────────────────────
    //
    // AAVE, Compound, Uniswap and most modern protocols use EIP-1967.
    // The implementation address lives at keccak256("eip1967.proxy.implementation")-1.
    let aave_proxy   = "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2"; // Aave V3 Pool (mainnet)
    let impl_slot_value =
        "0x000000000000000000000000fd56604da41a20d6b35cf50ac37e2f21ea2cf67b"; // example impl addr

    let info = classify_from_storage(aave_proxy, Some(impl_slot_value), None, None);

    println!("\n─── EIP-1967 Logic Proxy ────────────────────────────");
    println!("  proxy:          {}", info.proxy_address);
    println!("  kind:           {:?}", info.kind);
    println!("  implementation: {}", info.implementation.as_deref().unwrap_or("—"));
    println!("  slot:           {}", info.slot.as_deref().unwrap_or("—"));

    // ── 3. EIP-1967 Beacon Proxy ───────────────────────────────────────────────
    let zero_impl_slot  = "0x0000000000000000000000000000000000000000000000000000000000000000";
    let beacon_slot_val = "0x00000000000000000000000000000a89b5cc50a08b2b6f95d8e8e85b9c2fd1b5"; // example beacon

    let beacon_info = classify_from_storage(
        "0xBeaconProxy0001",
        Some(zero_impl_slot),
        Some(beacon_slot_val),
        None,
    );

    println!("\n─── EIP-1967 Beacon Proxy ───────────────────────────");
    println!("  kind:           {:?}", beacon_info.kind);
    println!("  beacon address: {}", beacon_info.implementation.as_deref().unwrap_or("—"));
    println!("  (call beacon.implementation() for the actual logic contract)");

    // ── 4. EIP-1822 UUPS Proxy ────────────────────────────────────────────────
    let uups_slot_val =
        "0x000000000000000000000000c0d3c0d3c0d3c0d3c0d3c0d3c0d3c0d3c0d3c0d3"; // example impl

    let uups_info = classify_from_storage(
        "0xUUPSProxy0001",
        Some(zero_impl_slot),
        Some(zero_impl_slot),
        Some(uups_slot_val),
    );

    println!("\n─── EIP-1822 UUPS Proxy ─────────────────────────────");
    println!("  kind:           {:?}", uups_info.kind);
    println!("  implementation: {}", uups_info.implementation.as_deref().unwrap_or("—"));

    // ── 5. Unknown proxy (all slots zero) ─────────────────────────────────────
    let unknown = classify_from_storage("0xUnknownProxy", Some(zero_impl_slot), None, None);

    println!("\n─── Unknown Proxy (all zero slots) ──────────────────");
    println!("  kind: {:?}", unknown.kind);
    println!("  → bytecode inspection needed (eth_getCode + detect_eip1167_clone)");

    // ── 6. EIP-1167 Minimal Proxy Clone (bytecode detection) ──────────────────
    //
    // EIP-1167 clones are exactly 45 bytes:
    //   [10 prefix bytes] [20 implementation address bytes] [15 suffix bytes]
    // This works directly on eth_getCode output — no storage slot needed.
    let impl_addr: [u8; 20] = [
        0xd8, 0xda, 0x6b, 0xf2, 0x69, 0x64, 0xaf, 0x9d, 0x7e, 0xed,
        0x9e, 0x03, 0xe5, 0x34, 0x15, 0xd3, 0x7a, 0xa9, 0x60, 0x45,
    ];
    let mut bytecode = Vec::new();
    bytecode.extend_from_slice(EIP1167_BYTECODE_PREFIX);
    bytecode.extend_from_slice(&impl_addr);
    bytecode.extend_from_slice(EIP1167_BYTECODE_SUFFIX);

    let detected = detect_eip1167_clone(&bytecode);

    println!("\n─── EIP-1167 Minimal Proxy Clone ────────────────────");
    println!("  bytecode length: {} bytes", bytecode.len());
    match detected {
        Some(addr) => {
            println!("  CLONE DETECTED ✓");
            println!("  delegates to: {addr}");
            println!("  → this contract forwards all calls to the implementation above");
        }
        None => println!("  not a clone"),
    }

    // ── 7. EIP-1167 detection on non-matching bytecode ────────────────────────
    let not_clone = vec![0x60u8, 0x80, 0x60, 0x40]; // standard contract bytecode prefix
    assert!(detect_eip1167_clone(&not_clone).is_none());
    println!("  ✓ non-clone correctly returns None");

    // ── 8. Production workflow summary ────────────────────────────────────────
    println!("\n─── Production Workflow ─────────────────────────────");
    println!("  1. Call eth_getCode(address) → if 45 bytes: check EIP-1167");
    println!("  2. Call eth_getStorageAt for the 3 slots above");
    println!("  3. Pass results to classify_from_storage()");
    println!("  4. Use {} for EIP-1967 impl lookups", EIP1967_IMPL_SLOT);
    println!("  5. Monitor Upgraded(address) events for live upgrade tracking");

    println!("\n✓ Proxy detection examples complete");
}
