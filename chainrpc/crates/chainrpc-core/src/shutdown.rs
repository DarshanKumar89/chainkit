//! Graceful shutdown — drain in-flight requests on SIGTERM/SIGINT.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

/// Shutdown signal receiver — check `is_shutdown()` in your loops.
#[derive(Clone)]
pub struct ShutdownSignal {
    rx: watch::Receiver<bool>,
}

impl ShutdownSignal {
    /// Check if shutdown has been signaled.
    pub fn is_shutdown(&self) -> bool {
        *self.rx.borrow()
    }

    /// Wait until shutdown is signaled.
    pub async fn wait(&mut self) {
        while !*self.rx.borrow() {
            if self.rx.changed().await.is_err() {
                return; // sender dropped
            }
        }
    }
}

/// Shutdown controller — owns the signal and triggers shutdown.
pub struct ShutdownController {
    tx: watch::Sender<bool>,
}

impl ShutdownController {
    /// Create a new controller and its signal receiver.
    pub fn new() -> (Self, ShutdownSignal) {
        let (tx, rx) = watch::channel(false);
        (Self { tx }, ShutdownSignal { rx })
    }

    /// Trigger shutdown.
    pub fn shutdown(&self) {
        let _ = self.tx.send(true);
        tracing::info!("shutdown signaled");
    }

    /// Create a signal receiver (can create multiple).
    pub fn signal(&self) -> ShutdownSignal {
        ShutdownSignal {
            rx: self.tx.subscribe(),
        }
    }
}

impl Default for ShutdownController {
    fn default() -> Self {
        Self::new().0
    }
}

/// Install OS signal handlers (SIGTERM, SIGINT) and return a shutdown signal.
///
/// When a signal is received, the returned `ShutdownSignal` will report `true`.
/// Call this once at application startup.
pub fn install_signal_handler() -> (Arc<ShutdownController>, ShutdownSignal) {
    let (controller, signal) = ShutdownController::new();
    let controller = Arc::new(controller);
    let ctrl = controller.clone();

    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();

        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm =
                signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
            tokio::select! {
                _ = ctrl_c => {
                    tracing::info!("received SIGINT");
                }
                _ = sigterm.recv() => {
                    tracing::info!("received SIGTERM");
                }
            }
        }

        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
            tracing::info!("received SIGINT");
        }

        ctrl.shutdown();
    });

    (controller, signal)
}

/// Run shutdown with timeout — wait for `drain_fn` to complete within `timeout`.
///
/// If the drain function doesn't complete in time, force exit.
pub async fn shutdown_with_timeout<F, Fut>(
    signal: &mut ShutdownSignal,
    timeout: Duration,
    drain_fn: F,
) where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    signal.wait().await;
    tracing::info!(
        "shutdown initiated, draining in-flight requests (timeout: {}s)...",
        timeout.as_secs()
    );

    match tokio::time::timeout(timeout, drain_fn()).await {
        Ok(()) => {
            tracing::info!("graceful shutdown complete");
        }
        Err(_) => {
            tracing::warn!("shutdown timeout exceeded, forcing exit");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_starts_false() {
        let (_controller, signal) = ShutdownController::new();
        assert!(!signal.is_shutdown());
    }

    #[test]
    fn signal_after_shutdown() {
        let (controller, signal) = ShutdownController::new();
        controller.shutdown();
        assert!(signal.is_shutdown());
    }

    #[test]
    fn multiple_signals() {
        let (controller, _signal) = ShutdownController::new();
        let s1 = controller.signal();
        let s2 = controller.signal();

        assert!(!s1.is_shutdown());
        assert!(!s2.is_shutdown());

        controller.shutdown();

        assert!(s1.is_shutdown());
        assert!(s2.is_shutdown());
    }

    #[tokio::test]
    async fn wait_for_shutdown() {
        let (controller, mut signal) = ShutdownController::new();

        let handle = tokio::spawn(async move {
            signal.wait().await;
            true
        });

        // Small delay then shutdown
        tokio::time::sleep(Duration::from_millis(10)).await;
        controller.shutdown();

        let result = handle.await.unwrap();
        assert!(result);
    }

    #[tokio::test]
    async fn shutdown_with_timeout_completes() {
        let (controller, mut signal) = ShutdownController::new();
        controller.shutdown();

        shutdown_with_timeout(&mut signal, Duration::from_secs(5), || async {
            // Quick drain
            tokio::time::sleep(Duration::from_millis(10)).await;
        })
        .await;
        // Should complete without panic
    }
}
