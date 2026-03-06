//! # Provider Selection Strategies
//!
//! Demonstrates all five `SelectionStrategy` variants and their behaviour when
//! picking from a pool of providers via `SelectionState::select()`.
//!
//! | Strategy             | Behaviour                                              |
//! |----------------------|--------------------------------------------------------|
//! | `RoundRobin`         | Cycle through providers evenly                         |
//! | `Priority`           | Always pick the first available (lowest index wins)     |
//! | `WeightedRoundRobin` | Distribute proportionally by weight                    |
//! | `LatencyBased`       | Pick the provider with the lowest observed latency     |
//! | `Sticky`             | Consistent-hash a key to always reach the same provider|
//!
//! `SelectionState` is the shared, thread-safe companion that tracks cursors,
//! latency EMA, and weight counters. A single `SelectionState` is created per
//! pool and shared across all requests.

use std::time::Duration;

use chainrpc_core::selection::{SelectionState, SelectionStrategy};

#[tokio::main]
async fn main() {
    println!("=== Provider Selection Strategies Demo ===\n");

    // We model a pool of 4 providers. The `allowed` slice tells the selector
    // which providers are currently healthy (circuit breaker closed).
    let provider_count = 4;
    let provider_names = ["Alchemy", "Infura", "QuickNode", "Ankr"];
    let all_allowed = [true, true, true, true];

    // ---------------------------------------------------------------
    // 1. RoundRobin — distribute requests evenly
    // ---------------------------------------------------------------
    println!("--- 1. RoundRobin ---");
    let state = SelectionState::new(provider_count);
    let strategy = SelectionStrategy::RoundRobin;

    let mut counts = [0u32; 4];
    for _ in 0..12 {
        let idx = state.select(&strategy, &all_allowed).unwrap();
        counts[idx] += 1;
    }

    println!("  12 requests distributed across 4 providers:");
    for (i, count) in counts.iter().enumerate() {
        println!("    {}: {} requests", provider_names[i], count);
    }
    // Each provider should get exactly 3 requests.

    // RoundRobin skips unhealthy providers.
    println!("\n  With provider 1 (Infura) down:");
    let partial = [true, false, true, true];
    let state2 = SelectionState::new(provider_count);
    let mut counts2 = [0u32; 4];
    for _ in 0..9 {
        let idx = state2.select(&strategy, &partial).unwrap();
        counts2[idx] += 1;
    }
    for (i, count) in counts2.iter().enumerate() {
        let status = if partial[i] { "UP" } else { "DOWN" };
        println!("    {} [{}]: {} requests", provider_names[i], status, count);
    }
    // Infura gets 0; the other three share the load evenly.

    // ---------------------------------------------------------------
    // 2. Priority — always pick the highest-priority (lowest index)
    // ---------------------------------------------------------------
    println!("\n--- 2. Priority ---");
    let state = SelectionState::new(provider_count);
    let strategy = SelectionStrategy::Priority;

    // All up — always picks index 0.
    let idx = state.select(&strategy, &all_allowed).unwrap();
    println!("  all healthy => picked: {} (index {idx})", provider_names[idx]);

    // Provider 0 is down — falls through to index 1.
    let no_alchemy = [false, true, true, true];
    let idx = state.select(&strategy, &no_alchemy).unwrap();
    println!("  Alchemy down => picked: {} (index {idx})", provider_names[idx]);

    // Providers 0 and 1 are down.
    let no_first_two = [false, false, true, true];
    let idx = state.select(&strategy, &no_first_two).unwrap();
    println!(
        "  Alchemy+Infura down => picked: {} (index {idx})",
        provider_names[idx]
    );

    // All providers down.
    let none_allowed = [false, false, false, false];
    let result = state.select(&strategy, &none_allowed);
    println!("  all down => {:?}", result);
    // Returns None — caller should return TransportError::AllProvidersDown.

    // ---------------------------------------------------------------
    // 3. WeightedRoundRobin — proportional traffic split
    // ---------------------------------------------------------------
    println!("\n--- 3. WeightedRoundRobin ---");
    // Alchemy gets 5x weight, others get 1x each (total weight = 8).
    let strategy = SelectionStrategy::WeightedRoundRobin {
        weights: vec![5, 1, 1, 1],
    };
    let state = SelectionState::new(provider_count);

    let mut counts = [0u32; 4];
    let total = 800;
    for _ in 0..total {
        let idx = state.select(&strategy, &all_allowed).unwrap();
        counts[idx] += 1;
    }

    println!("  weights: [5, 1, 1, 1]  (total=8)");
    println!("  {total} requests distributed:");
    for (i, count) in counts.iter().enumerate() {
        let pct = *count as f64 / total as f64 * 100.0;
        println!("    {}: {} ({:.0}%)", provider_names[i], count, pct);
    }
    // Alchemy should get ~62.5% (5/8), each other provider ~12.5% (1/8).

    // ---------------------------------------------------------------
    // 4. LatencyBased — pick the fastest provider
    // ---------------------------------------------------------------
    println!("\n--- 4. LatencyBased ---");
    let state = SelectionState::new(provider_count);
    let strategy = SelectionStrategy::LatencyBased;

    // Record observed latencies (exponential moving average internally).
    state.record_latency(0, Duration::from_millis(50));  // Alchemy: 50ms
    state.record_latency(1, Duration::from_millis(120)); // Infura: 120ms
    state.record_latency(2, Duration::from_millis(15));  // QuickNode: 15ms  <-- fastest
    state.record_latency(3, Duration::from_millis(80));  // Ankr: 80ms

    let idx = state.select(&strategy, &all_allowed).unwrap();
    println!("  Latencies: Alchemy=50ms, Infura=120ms, QuickNode=15ms, Ankr=80ms");
    println!("  selected : {} (index {idx})", provider_names[idx]);
    // QuickNode (index 2) wins — lowest latency.

    // EMA smoothing: a single spike does not immediately change routing.
    println!("\n  After a latency spike on QuickNode (500ms):");
    state.record_latency(2, Duration::from_millis(500));
    // EMA: 0.3 * 500 + 0.7 * 15 = 160.5ms — still might not be the fastest.
    let idx = state.select(&strategy, &all_allowed).unwrap();
    println!("  selected : {} (index {idx})", provider_names[idx]);
    // After smoothing, Alchemy (50ms) is now faster than QuickNode (~160ms).

    // Disallowed providers are skipped even if fastest.
    println!("\n  QuickNode is fastest but circuit breaker is open:");
    state.record_latency(2, Duration::from_millis(5)); // reset to fast
    let no_quicknode = [true, true, false, true];
    let idx = state.select(&strategy, &no_quicknode).unwrap();
    println!("  selected : {} (index {idx})", provider_names[idx]);
    // Falls through to the next fastest allowed provider.

    // ---------------------------------------------------------------
    // 5. Sticky — consistent hashing by key
    // ---------------------------------------------------------------
    println!("\n--- 5. Sticky ---");

    // Sticky hashing ensures the same key always reaches the same provider.
    // Useful for nonce management: all txs from one sender go to one node.
    let alice_strategy = SelectionStrategy::Sticky {
        key: "0xAlice".to_string(),
    };
    let bob_strategy = SelectionStrategy::Sticky {
        key: "0xBob".to_string(),
    };

    let state = SelectionState::new(provider_count);

    // Alice always goes to the same provider.
    let alice_1 = state.select(&alice_strategy, &all_allowed).unwrap();
    let alice_2 = state.select(&alice_strategy, &all_allowed).unwrap();
    let alice_3 = state.select(&alice_strategy, &all_allowed).unwrap();
    println!(
        "  0xAlice => provider {} (consistent: {})",
        provider_names[alice_1],
        alice_1 == alice_2 && alice_2 == alice_3
    );

    // Bob gets a (likely different) provider.
    let bob_1 = state.select(&bob_strategy, &all_allowed).unwrap();
    let bob_2 = state.select(&bob_strategy, &all_allowed).unwrap();
    println!(
        "  0xBob   => provider {} (consistent: {})",
        provider_names[bob_1],
        bob_1 == bob_2
    );

    // If Alice's preferred provider goes down, she falls back to the next one.
    let mut partial_allowed = all_allowed;
    partial_allowed[alice_1] = false;
    let alice_fallback = state.select(&alice_strategy, &partial_allowed).unwrap();
    println!(
        "  0xAlice (preferred {} down) => fallback to {}",
        provider_names[alice_1], provider_names[alice_fallback]
    );

    // When the original provider recovers, Alice goes back to it.
    let alice_recovered = state.select(&alice_strategy, &all_allowed).unwrap();
    println!(
        "  0xAlice (recovered) => back to {} (same as before: {})",
        provider_names[alice_recovered],
        alice_recovered == alice_1
    );

    println!("\n=== Done ===");
}
