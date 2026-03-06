//! Cooperative cancellation tokens for RPC operations.
//!
//! Provides a [`CancellationToken`] that can be shared across tasks to
//! signal cancellation, and a [`CancellationChild`] that inherits the
//! parent's cancellation while also supporting independent local
//! cancellation.

use tokio::sync::watch;

// ---------------------------------------------------------------------------
// CancellationToken
// ---------------------------------------------------------------------------

/// A cancellation token that can be shared across tasks.
///
/// The token uses a `tokio::sync::watch` channel internally so that
/// any number of tasks can observe the cancellation signal without
/// additional synchronisation.
///
/// # Examples
///
/// ```
/// use chainrpc_core::cancellation::CancellationToken;
///
/// let token = CancellationToken::new();
/// assert!(!token.is_cancelled());
///
/// token.cancel();
/// assert!(token.is_cancelled());
/// ```
pub struct CancellationToken {
    sender: watch::Sender<bool>,
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    /// Create a new, un-cancelled token.
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self { sender, receiver }
    }

    /// Cancel all operations watching this token.
    ///
    /// This is idempotent — calling `cancel()` more than once has no
    /// additional effect.
    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Get a child token that is cancelled when **either** the parent or the
    /// child itself is cancelled.
    pub fn child(&self) -> CancellationChild {
        let parent = self.sender.subscribe();
        let (local, local_rx) = watch::channel(false);
        CancellationChild {
            parent,
            local,
            local_rx,
        }
    }

    /// Wait until cancellation is requested.
    ///
    /// If the token is already cancelled this returns immediately.
    pub async fn cancelled(&self) {
        let mut rx = self.receiver.clone();
        // If already cancelled, return immediately.
        if *rx.borrow() {
            return;
        }
        // Wait for the value to change to `true`.
        loop {
            if rx.changed().await.is_err() {
                // Sender dropped — treat as cancellation.
                return;
            }
            if *rx.borrow() {
                return;
            }
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CancellationChild
// ---------------------------------------------------------------------------

/// A child token derived from a parent [`CancellationToken`].
///
/// The child is considered cancelled when **either** the parent is cancelled
/// or the child's own [`cancel()`](CancellationChild::cancel) method is
/// called.  Cancelling the child does **not** propagate upward to the parent.
pub struct CancellationChild {
    parent: watch::Receiver<bool>,
    local: watch::Sender<bool>,
    local_rx: watch::Receiver<bool>,
}

impl CancellationChild {
    /// Cancel this child token (does not cancel the parent).
    pub fn cancel(&self) {
        let _ = self.local.send(true);
    }

    /// Check if cancellation has been requested (either parent or local).
    pub fn is_cancelled(&self) -> bool {
        *self.parent.borrow() || *self.local_rx.borrow()
    }

    /// Wait until cancellation is requested from either parent or local.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let mut parent_rx = self.parent.clone();
        let mut local_rx = self.local_rx.clone();

        loop {
            tokio::select! {
                res = parent_rx.changed() => {
                    if res.is_err() || *parent_rx.borrow() {
                        return;
                    }
                }
                res = local_rx.changed() => {
                    if res.is_err() || *local_rx.borrow() {
                        return;
                    }
                }
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_starts_uncancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn cancel_propagates() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn child_inherits_parent_cancel() {
        let parent = CancellationToken::new();
        let child = parent.child();

        assert!(!child.is_cancelled());

        parent.cancel();

        // The child observes the parent's cancellation.
        assert!(child.is_cancelled());

        // The async wait should return immediately.
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            child.cancelled(),
        )
        .await
        .expect("child.cancelled() should complete immediately when parent is cancelled");
    }

    #[tokio::test]
    async fn child_local_cancel_independent() {
        let parent = CancellationToken::new();
        let child = parent.child();

        // Cancel only the child.
        child.cancel();

        assert!(child.is_cancelled());
        // Parent remains uncancelled.
        assert!(!parent.is_cancelled());

        // The child's async wait should return immediately.
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            child.cancelled(),
        )
        .await
        .expect("child.cancelled() should complete immediately after local cancel");
    }
}
