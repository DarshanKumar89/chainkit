//! # Graceful Shutdown
//!
//! Demonstrates `ShutdownController` and `ShutdownSignal` for coordinated
//! graceful shutdown in an async application.  Shows:
//!
//! - Installing a signal handler for SIGTERM / SIGINT.
//! - Checking `is_shutdown()` in a main processing loop.
//! - Using `shutdown_with_timeout()` to drain in-flight work.
//! - Distributing multiple `ShutdownSignal` receivers across tasks.
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::sync::Arc;
use std::time::Duration;

use chainrpc_core::shutdown::{
    install_signal_handler, shutdown_with_timeout, ShutdownController, ShutdownSignal,
};

#[tokio::main]
async fn main() {
    println!("=== Graceful Shutdown ===\n");

    // =====================================================================
    // 1. Create a ShutdownController and its signal
    // =====================================================================
    // `ShutdownController::new()` returns a (controller, signal) pair.
    // The controller triggers shutdown; the signal observes it.
    let (controller, signal) = ShutdownController::new();

    println!("Created ShutdownController + ShutdownSignal");
    println!("  is_shutdown (before): {}", signal.is_shutdown());
    assert!(!signal.is_shutdown());

    // =====================================================================
    // 2. Distribute multiple signal receivers
    // =====================================================================
    // You can create as many signal receivers as you need.  Each one
    // independently observes the same shutdown event.
    let signal_for_worker_a = controller.signal();
    let signal_for_worker_b = controller.signal();

    println!("\nCreated 2 additional signal receivers (worker A, worker B)");
    assert!(!signal_for_worker_a.is_shutdown());
    assert!(!signal_for_worker_b.is_shutdown());

    // =====================================================================
    // 3. Main loop checking is_shutdown()
    // =====================================================================
    // A typical pattern: run a loop that processes work items and exits
    // when shutdown is signaled.
    let controller = Arc::new(controller);
    let ctrl_for_trigger = controller.clone();

    // Spawn a "worker" that loops until shutdown.
    let worker_signal = controller.signal();
    let worker_handle = tokio::spawn(async move {
        let mut iterations = 0u32;
        loop {
            // Check the shutdown flag on each iteration.
            if worker_signal.is_shutdown() {
                println!("  [worker] shutdown detected after {iterations} iterations");
                break;
            }

            // Simulate doing some work.
            iterations += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        iterations
    });

    // Let the worker run a few iterations, then trigger shutdown.
    tokio::time::sleep(Duration::from_millis(50)).await;
    ctrl_for_trigger.shutdown();
    println!("\nShutdown triggered.");

    let iterations = worker_handle.await.unwrap();
    println!("  Worker completed {iterations} iterations before exit");

    // All signal receivers now report true.
    assert!(signal.is_shutdown());
    assert!(signal_for_worker_a.is_shutdown());
    assert!(signal_for_worker_b.is_shutdown());
    println!("  All 3 signal receivers report is_shutdown = true");

    // =====================================================================
    // 4. shutdown_with_timeout() — drain with a deadline
    // =====================================================================
    // `shutdown_with_timeout` waits for the signal, then runs a drain
    // closure with a timeout.  If the drain doesn't finish in time, it
    // logs a warning and returns.
    println!("\n--- shutdown_with_timeout ---\n");

    let (controller2, mut signal2) = ShutdownController::new();

    // Trigger immediately so the signal is already set.
    controller2.shutdown();

    // The drain function simulates flushing in-flight requests.
    shutdown_with_timeout(&mut signal2, Duration::from_secs(5), || async {
        println!("  [drain] flushing 3 in-flight requests...");
        tokio::time::sleep(Duration::from_millis(30)).await;
        println!("  [drain] all requests drained");
    })
    .await;
    println!("  shutdown_with_timeout completed");

    // =====================================================================
    // 5. Install OS signal handler (conceptual)
    // =====================================================================
    // In a real application you would call `install_signal_handler()` once
    // at startup.  It spawns a Tokio task that listens for SIGTERM and
    // SIGINT and triggers the controller automatically.
    //
    //   let (controller, signal) = install_signal_handler();
    //
    //   // Pass `signal` clones to your workers:
    //   let worker_signal = controller.signal();
    //   tokio::spawn(async move {
    //       loop {
    //           if worker_signal.is_shutdown() { break; }
    //           // ... do work ...
    //       }
    //   });
    //
    //   // In the main task, drain on shutdown:
    //   let mut main_signal = controller.signal();
    //   shutdown_with_timeout(&mut main_signal, Duration::from_secs(30), || async {
    //       // flush connections, write checkpoints, etc.
    //   }).await;
    //
    // NOTE: We do not actually call install_signal_handler() here because
    // it registers a real OS signal handler that would interfere with this
    // demonstration.

    println!("\n[OK] Signal handler install is available via install_signal_handler()");

    println!("\nDone.");
}
