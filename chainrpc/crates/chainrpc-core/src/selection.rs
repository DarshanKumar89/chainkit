//! Provider selection strategies for the pool.
//!
//! Strategies:
//! - RoundRobin — distribute evenly across healthy providers
//! - Priority — try in priority order (lowest number = highest priority)
//! - WeightedRoundRobin — distribute proportionally by weight
//! - LatencyBased — route to the fastest responding provider
//! - Sticky — same provider for same key (e.g. address for nonce management)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// A selection strategy decides which provider index to use next.
#[derive(Debug, Clone, Default)]
pub enum SelectionStrategy {
    /// Round-robin across all allowed providers.
    #[default]
    RoundRobin,
    /// Try providers in priority order (based on their registration order).
    Priority,
    /// Weighted round-robin — higher weight gets more traffic.
    WeightedRoundRobin { weights: Vec<u32> },
    /// Route to the provider with the lowest observed latency.
    LatencyBased,
    /// Stick to the same provider for a given key (e.g. sender address).
    Sticky { key: String },
}

/// State for stateful selection strategies.
pub struct SelectionState {
    /// Round-robin cursor.
    rr_cursor: AtomicUsize,
    /// Per-provider latency (microseconds) for LatencyBased.
    latencies: Mutex<Vec<u64>>,
    /// Weighted round-robin state.
    #[allow(dead_code)]
    wrr_cursor: AtomicUsize,
    wrr_counter: AtomicU64,
}

impl SelectionState {
    /// Create state for a given number of providers.
    pub fn new(provider_count: usize) -> Self {
        Self {
            rr_cursor: AtomicUsize::new(0),
            latencies: Mutex::new(vec![0; provider_count]),
            wrr_cursor: AtomicUsize::new(0),
            wrr_counter: AtomicU64::new(0),
        }
    }

    /// Select the next provider index.
    ///
    /// `allowed` is a slice of booleans indicating which providers are available
    /// (circuit breaker allows requests).
    pub fn select(&self, strategy: &SelectionStrategy, allowed: &[bool]) -> Option<usize> {
        let count = allowed.len();
        if count == 0 {
            return None;
        }

        match strategy {
            SelectionStrategy::RoundRobin => self.select_round_robin(allowed),
            SelectionStrategy::Priority => self.select_priority(allowed),
            SelectionStrategy::WeightedRoundRobin { weights } => {
                self.select_weighted(allowed, weights)
            }
            SelectionStrategy::LatencyBased => self.select_latency(allowed),
            SelectionStrategy::Sticky { key } => self.select_sticky(allowed, key),
        }
    }

    /// Record observed latency for a provider (used by LatencyBased strategy).
    pub fn record_latency(&self, index: usize, latency: Duration) {
        let mut latencies = self.latencies.lock().unwrap();
        if index < latencies.len() {
            // Exponential moving average: new = 0.3 * sample + 0.7 * old
            let sample = latency.as_micros() as u64;
            let old = latencies[index];
            if old == 0 {
                latencies[index] = sample;
            } else {
                latencies[index] = (sample * 3 + old * 7) / 10;
            }
        }
    }

    // -- strategy implementations -------------------------------------------

    fn select_round_robin(&self, allowed: &[bool]) -> Option<usize> {
        let count = allowed.len();
        let start = self.rr_cursor.fetch_add(1, Ordering::Relaxed) % count;
        for i in 0..count {
            let idx = (start + i) % count;
            if allowed[idx] {
                return Some(idx);
            }
        }
        None
    }

    fn select_priority(&self, allowed: &[bool]) -> Option<usize> {
        // Try in order — first allowed provider wins.
        for (idx, &ok) in allowed.iter().enumerate() {
            if ok {
                return Some(idx);
            }
        }
        None
    }

    fn select_weighted(&self, allowed: &[bool], weights: &[u32]) -> Option<usize> {
        // Build effective weights (zero for disallowed)
        let effective: Vec<u32> = allowed
            .iter()
            .enumerate()
            .map(|(i, &ok)| {
                if ok {
                    weights.get(i).copied().unwrap_or(1)
                } else {
                    0
                }
            })
            .collect();

        let total: u64 = effective.iter().map(|&w| w as u64).sum();
        if total == 0 {
            return None;
        }

        let counter = self.wrr_counter.fetch_add(1, Ordering::Relaxed);
        let target = counter % total;

        let mut cumulative = 0u64;
        for (idx, &w) in effective.iter().enumerate() {
            cumulative += w as u64;
            if target < cumulative {
                return Some(idx);
            }
        }
        // Fallback (shouldn't reach)
        allowed.iter().position(|&ok| ok)
    }

    fn select_latency(&self, allowed: &[bool]) -> Option<usize> {
        let latencies = self.latencies.lock().unwrap();
        let mut best_idx = None;
        let mut best_latency = u64::MAX;

        for (idx, &ok) in allowed.iter().enumerate() {
            if !ok {
                continue;
            }
            let lat = latencies.get(idx).copied().unwrap_or(0);
            // Treat 0 (no data) as very fast — give new providers a chance
            let effective = if lat == 0 { 1 } else { lat };
            if effective < best_latency {
                best_latency = effective;
                best_idx = Some(idx);
            }
        }
        best_idx
    }

    fn select_sticky(&self, allowed: &[bool], key: &str) -> Option<usize> {
        let count = allowed.len();
        if count == 0 {
            return None;
        }

        // Hash the key to pick a consistent provider
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish() as usize;
        let preferred = hash % count;

        // If preferred is allowed, use it
        if allowed[preferred] {
            return Some(preferred);
        }

        // Otherwise, fall back to the next allowed provider
        for i in 1..count {
            let idx = (preferred + i) % count;
            if allowed[idx] {
                return Some(idx);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_basic() {
        let state = SelectionState::new(3);
        let allowed = [true, true, true];

        let a = state
            .select(&SelectionStrategy::RoundRobin, &allowed)
            .unwrap();
        let b = state
            .select(&SelectionStrategy::RoundRobin, &allowed)
            .unwrap();
        let c = state
            .select(&SelectionStrategy::RoundRobin, &allowed)
            .unwrap();

        // Should cycle through all 3
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn round_robin_skips_disallowed() {
        let state = SelectionState::new(3);
        let allowed = [true, false, true];

        let mut selected = std::collections::HashSet::new();
        for _ in 0..10 {
            let idx = state
                .select(&SelectionStrategy::RoundRobin, &allowed)
                .unwrap();
            selected.insert(idx);
            assert_ne!(idx, 1, "should never select disallowed provider");
        }
    }

    #[test]
    fn priority_selects_first_allowed() {
        let state = SelectionState::new(3);

        let allowed1 = [true, true, true];
        assert_eq!(
            state.select(&SelectionStrategy::Priority, &allowed1),
            Some(0)
        );

        let allowed2 = [false, true, true];
        assert_eq!(
            state.select(&SelectionStrategy::Priority, &allowed2),
            Some(1)
        );

        let allowed3 = [false, false, true];
        assert_eq!(
            state.select(&SelectionStrategy::Priority, &allowed3),
            Some(2)
        );
    }

    #[test]
    fn priority_none_when_all_down() {
        let state = SelectionState::new(3);
        let allowed = [false, false, false];
        assert_eq!(state.select(&SelectionStrategy::Priority, &allowed), None);
    }

    #[test]
    fn weighted_round_robin() {
        let state = SelectionState::new(3);
        let strategy = SelectionStrategy::WeightedRoundRobin {
            weights: vec![3, 1, 1],
        };
        let allowed = [true, true, true];

        let mut counts = [0u32; 3];
        for _ in 0..500 {
            let idx = state.select(&strategy, &allowed).unwrap();
            counts[idx] += 1;
        }

        // Provider 0 should get ~60% of traffic (3/5)
        assert!(
            counts[0] > counts[1],
            "weighted provider should get more traffic"
        );
        assert!(
            counts[0] > counts[2],
            "weighted provider should get more traffic"
        );
    }

    #[test]
    fn latency_based_selects_fastest() {
        let state = SelectionState::new(3);
        let allowed = [true, true, true];

        // Record latencies
        state.record_latency(0, Duration::from_millis(100));
        state.record_latency(1, Duration::from_millis(10)); // fastest
        state.record_latency(2, Duration::from_millis(50));

        let idx = state
            .select(&SelectionStrategy::LatencyBased, &allowed)
            .unwrap();
        assert_eq!(idx, 1, "should select fastest provider");
    }

    #[test]
    fn latency_based_skips_disallowed() {
        let state = SelectionState::new(3);
        let allowed = [true, false, true]; // provider 1 disallowed

        state.record_latency(0, Duration::from_millis(100));
        state.record_latency(1, Duration::from_millis(1)); // fastest but disallowed
        state.record_latency(2, Duration::from_millis(50));

        let idx = state
            .select(&SelectionStrategy::LatencyBased, &allowed)
            .unwrap();
        assert_eq!(idx, 2, "should select fastest ALLOWED provider");
    }

    #[test]
    fn sticky_consistent_hashing() {
        let state = SelectionState::new(3);
        let allowed = [true, true, true];
        let strategy = SelectionStrategy::Sticky {
            key: "0xAlice".to_string(),
        };

        let idx1 = state.select(&strategy, &allowed).unwrap();
        let idx2 = state.select(&strategy, &allowed).unwrap();
        let idx3 = state.select(&strategy, &allowed).unwrap();

        // Same key should always select same provider
        assert_eq!(idx1, idx2);
        assert_eq!(idx2, idx3);
    }

    #[test]
    fn sticky_different_keys() {
        let state = SelectionState::new(100);
        let allowed = vec![true; 100];

        let s1 = SelectionStrategy::Sticky {
            key: "0xAlice".to_string(),
        };
        let s2 = SelectionStrategy::Sticky {
            key: "0xBob".to_string(),
        };

        let idx1 = state.select(&s1, &allowed).unwrap();
        let idx2 = state.select(&s2, &allowed).unwrap();

        // Different keys should (usually) select different providers
        // With 100 providers, collision probability is low
        // But not guaranteed, so just verify both return valid indices
        assert!(idx1 < 100);
        assert!(idx2 < 100);
    }

    #[test]
    fn sticky_fallback_when_preferred_down() {
        let state = SelectionState::new(3);
        let allowed_all = [true, true, true];
        let strategy = SelectionStrategy::Sticky {
            key: "test".to_string(),
        };

        let preferred = state.select(&strategy, &allowed_all).unwrap();

        // Mark preferred as down
        let mut allowed_partial = [true, true, true];
        allowed_partial[preferred] = false;

        let fallback = state.select(&strategy, &allowed_partial).unwrap();
        assert_ne!(fallback, preferred);
    }

    #[test]
    fn latency_ema_smoothing() {
        let state = SelectionState::new(1);

        // Record a few samples
        state.record_latency(0, Duration::from_millis(100));
        state.record_latency(0, Duration::from_millis(200)); // EMA: 0.3*200 + 0.7*100 = 130ms

        let latencies = state.latencies.lock().unwrap();
        let lat_us = latencies[0];
        // Should be smoothed, not just the latest sample
        assert!(
            lat_us > 100_000 && lat_us < 200_000,
            "EMA should smooth: {lat_us}"
        );
    }
}
