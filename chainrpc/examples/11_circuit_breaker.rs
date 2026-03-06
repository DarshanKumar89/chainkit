//! # Circuit Breaker Lifecycle
//!
//! Demonstrates the three-state circuit breaker that protects against cascading
//! failures when an RPC provider goes down:
//!
//! ```text
//!   Closed ──(5 failures)──> Open ──(wait 2s)──> HalfOpen
//!     ^                                             │
//!     └────────(probe success)──────────────────────┘
//!                                    │
//!              Open <──(probe failure)┘
//! ```
//!
//! - **Closed** — normal operation; requests flow through.
//! - **Open** — provider is down; all requests are immediately rejected via
//!   `is_allowed() == false` to avoid wasting time on a dead node.
//! - **HalfOpen** — the `open_duration` has elapsed; one probe request is
//!   allowed through. A success closes the circuit; a failure re-opens it.

use std::time::Duration;

use chainrpc_core::policy::circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitState,
};

#[tokio::main]
async fn main() {
    // ---------------------------------------------------------------
    // 1. Create a circuit breaker
    // ---------------------------------------------------------------
    // Opens after 5 consecutive failures, waits 2 seconds before probing.
    let cb = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 5,
        open_duration: Duration::from_secs(2),
        success_threshold: 1, // one successful probe closes the circuit
    });

    println!("=== Circuit Breaker Demo ===\n");
    println!("Initial state: {}", cb.state());
    assert_eq!(cb.state(), CircuitState::Closed);

    // ---------------------------------------------------------------
    // 2. Record failures — watch state transition to Open
    // ---------------------------------------------------------------
    println!("\n--- Recording 5 consecutive failures ---");
    for i in 1..=5 {
        cb.record_failure();
        let state = cb.state();
        let allowed = cb.is_allowed();
        println!(
            "  failure #{i}: state={state}, is_allowed={allowed}"
        );
    }
    // After the 5th failure the circuit opens.
    assert_eq!(cb.state(), CircuitState::Open);
    assert!(!cb.is_allowed());
    println!("\nCircuit is now OPEN — all requests will be rejected immediately.");

    // ---------------------------------------------------------------
    // 3. Show that requests are blocked while Open
    // ---------------------------------------------------------------
    println!("\n--- Attempting requests while Open ---");
    for i in 1..=3 {
        let allowed = cb.is_allowed();
        println!("  attempt #{i}: is_allowed={allowed}");
        // All false — no request reaches the provider.
    }

    // ---------------------------------------------------------------
    // 4. Wait for the open_duration to elapse (2 seconds)
    // ---------------------------------------------------------------
    println!("\n--- Waiting 2 seconds for reset timeout ---");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // After open_duration, state() transitions to HalfOpen automatically.
    let state = cb.state();
    println!("  state after timeout: {state}");
    assert_eq!(state, CircuitState::HalfOpen);
    println!("  is_allowed: {} (one probe allowed)", cb.is_allowed());

    // ---------------------------------------------------------------
    // 5. Probe succeeds — circuit closes
    // ---------------------------------------------------------------
    println!("\n--- Probe request succeeds ---");
    cb.record_success();
    let state = cb.state();
    println!("  state after success: {state}");
    assert_eq!(state, CircuitState::Closed);
    println!("  is_allowed: {} (back to normal)", cb.is_allowed());

    // ---------------------------------------------------------------
    // 6. Success resets the failure counter
    // ---------------------------------------------------------------
    println!("\n--- Intermittent failures with recovery ---");
    // 4 failures, then a success — counter resets; circuit stays closed.
    for i in 1..=4 {
        cb.record_failure();
        println!("  failure #{i}: state={}", cb.state());
    }
    cb.record_success();
    println!("  success:     state={}", cb.state());
    assert_eq!(cb.state(), CircuitState::Closed);
    println!("  (counter was reset before reaching threshold)");

    // 4 more failures — still only 4 since last reset, not enough to open.
    for i in 1..=4 {
        cb.record_failure();
        println!("  failure #{i}: state={}", cb.state());
    }
    assert_eq!(cb.state(), CircuitState::Closed);
    println!("  Circuit remains closed — need 5 *consecutive* failures.");

    // ---------------------------------------------------------------
    // 7. HalfOpen probe failure re-opens the circuit
    // ---------------------------------------------------------------
    println!("\n--- Probe failure re-opens circuit ---");

    // Force open via 5 consecutive failures.
    let cb2 = CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 5,
        open_duration: Duration::from_millis(100), // short for demo
        success_threshold: 1,
    });
    for _ in 0..5 {
        cb2.record_failure();
    }
    assert_eq!(cb2.state(), CircuitState::Open);

    // Wait for half-open transition.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(cb2.state(), CircuitState::HalfOpen);
    println!("  state: half-open (probe allowed)");

    // Probe fails — circuit goes back to Open.
    cb2.record_failure();
    assert_eq!(cb2.state(), CircuitState::Open);
    println!("  probe failed => state: {}", cb2.state());
    println!("  (circuit re-opened, must wait another open_duration)");

    println!("\n=== Done ===");
}
