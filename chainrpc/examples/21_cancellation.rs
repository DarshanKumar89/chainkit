//! # Cancellation Tokens
//!
//! Demonstrates the cooperative cancellation pattern using
//! `CancellationToken` and `CancellationChild`:
//!
//! - Parent token creates a child for a worker task.
//! - Worker checks `is_cancelled()` in its loop.
//! - Parent cancels after a timeout, propagating to the child.
//! - Child-only cancellation does NOT propagate to the parent.
//! - Async `cancelled().await` for select-style waiting.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::time::Duration;

use chainrpc_core::cancellation::CancellationToken;

#[tokio::main]
async fn main() {
    println!("=== Cancellation Tokens ===\n");

    // =====================================================================
    // 1. Basic CancellationToken usage
    // =====================================================================
    let token = CancellationToken::new();
    println!("Created CancellationToken");
    println!("  is_cancelled: {}", token.is_cancelled());
    assert!(!token.is_cancelled());

    token.cancel();
    println!("  after cancel(): {}", token.is_cancelled());
    assert!(token.is_cancelled());

    // cancel() is idempotent — calling it again has no effect.
    token.cancel();
    assert!(token.is_cancelled());
    println!("  cancel() is idempotent");

    // =====================================================================
    // 2. Parent/child pattern — parent cancels, child observes
    // =====================================================================
    println!("\n--- Parent -> Child Cancellation ---\n");

    let parent = CancellationToken::new();
    let child = parent.child();

    // Neither is cancelled initially.
    assert!(!parent.is_cancelled());
    assert!(!child.is_cancelled());
    println!("  parent.is_cancelled = {}", parent.is_cancelled());
    println!("  child.is_cancelled  = {}", child.is_cancelled());

    // Spawn a worker that loops until the child token is cancelled.
    let worker = tokio::spawn(async move {
        let mut count = 0u32;
        loop {
            if child.is_cancelled() {
                println!("  [worker] cancelled after {count} iterations");
                return count;
            }
            count += 1;
            // Simulate a unit of work.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    // Let the worker run for a bit, then cancel from the parent.
    tokio::time::sleep(Duration::from_millis(55)).await;
    println!("  Parent cancelling...");
    parent.cancel();

    let iterations = worker.await.unwrap();
    println!("  Worker ran {iterations} iterations before cancellation");
    assert!(iterations > 0);

    // =====================================================================
    // 3. Child-only cancellation — does NOT propagate to parent
    // =====================================================================
    println!("\n--- Child-Only Cancellation ---\n");

    let parent2 = CancellationToken::new();
    let child2 = parent2.child();

    // Cancel only the child.
    child2.cancel();

    println!("  child2.is_cancelled  = {}", child2.is_cancelled());
    println!("  parent2.is_cancelled = {}", parent2.is_cancelled());
    assert!(child2.is_cancelled(), "child should be cancelled");
    assert!(
        !parent2.is_cancelled(),
        "parent should NOT be cancelled by child"
    );
    println!("  [OK] Child cancellation does not propagate upward");

    // =====================================================================
    // 4. Async cancelled().await — select-style waiting
    // =====================================================================
    println!("\n--- Async cancelled().await ---\n");

    let parent3 = CancellationToken::new();

    // Spawn a task that waits for cancellation using the async method.
    let parent3_clone_for_wait = CancellationToken::new();
    // We need a fresh token to demonstrate the async wait.
    let waiter_token = CancellationToken::new();

    let waiter = tokio::spawn({
        // Move a reference-equivalent into the task.
        // In practice you would Arc-wrap or clone the token.
        // Here we demonstrate the pattern conceptually.
        async move {
            println!("  [waiter] waiting for cancellation...");
            waiter_token.cancelled().await;
            println!("  [waiter] cancellation received!");
        }
    });

    // The waiter_token was moved into the spawned task. Since we cannot
    // cancel it from here (it was moved), let's demonstrate with a token
    // we keep a handle to.
    let demo_token = CancellationToken::new();
    let demo_child = demo_token.child();

    let child_waiter = tokio::spawn(async move {
        println!("  [child_waiter] waiting for parent or local cancel...");
        demo_child.cancelled().await;
        println!("  [child_waiter] done waiting!");
    });

    // Give the tasks a moment to start, then cancel.
    tokio::time::sleep(Duration::from_millis(20)).await;
    demo_token.cancel();
    println!("  Parent token cancelled -> child_waiter should wake up");

    // Wait for the child waiter to finish.
    let _ = child_waiter.await;

    // The standalone waiter_token was moved and nobody cancels it, so we
    // abort it to keep the example clean.
    waiter.abort();

    // =====================================================================
    // 5. Already-cancelled token returns immediately
    // =====================================================================
    println!("\n--- Already-cancelled token ---\n");

    let pre_cancelled = CancellationToken::new();
    pre_cancelled.cancel();

    // cancelled().await on an already-cancelled token returns immediately.
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        pre_cancelled.cancelled(),
    )
    .await;
    assert!(result.is_ok(), "should return immediately, not timeout");
    println!("  cancelled().await returned immediately for pre-cancelled token");

    println!("\nDone.");
}
