//! # Transaction Lifecycle
//!
//! Walks through the complete transaction lifecycle primitives provided by
//! chainrpc-core:
//!
//! 1. **NonceLedger** -- local nonce bookkeeping (confirmed vs. pending).
//! 2. **TxTracker** -- track pending transactions and detect stuck ones.
//! 3. **ReceiptPoller** -- exponential-backoff delay schedule for receipt polling.
//! 4. Conceptual usage of `send_and_track()`, `poll_receipt()`,
//!    `refresh_status()`, and `detect_stuck()` (no real RPC calls).
//!
//! This is a documentation-only example and is not compiled as part of the
//! workspace.

use std::time::Duration;

use chainrpc_core::tx::{
    NonceLedger, ReceiptPoller, ReceiptPollerConfig, TrackedTx, TxStatus, TxTracker,
    TxTrackerConfig,
};

#[tokio::main]
async fn main() {
    println!("=== Transaction Lifecycle ===\n");

    let alice = "0xAlice";

    // =====================================================================
    // 1. NonceLedger — local nonce management
    // =====================================================================
    println!("--- NonceLedger ---\n");

    let ledger = NonceLedger::new();

    // Set the on-chain confirmed nonce (e.g. from eth_getTransactionCount).
    ledger.set_confirmed(alice, 5);
    println!("Confirmed nonce for Alice set to 5");

    // next() returns max(confirmed + 1, pending + 1).
    let next = ledger.next(alice);
    println!("Next nonce (confirmed only): {next}");
    assert_eq!(next, 6);

    // Mark nonces 6 and 7 as locally assigned (pending).
    ledger.mark_pending(alice, 6);
    ledger.mark_pending(alice, 7);
    println!("Marked nonces 6 and 7 as pending");

    let next = ledger.next(alice);
    println!("Next nonce (with pending):   {next}");
    assert_eq!(next, 8);

    // Detect gaps: if confirmed=5 and pending=10, nonces 6..9 are gaps.
    ledger.mark_pending(alice, 10);
    let gaps = ledger.gaps(alice);
    println!("Nonce gaps (confirmed=5, pending=10): {gaps:?}");
    assert_eq!(gaps, vec![6, 7, 8, 9]);

    // Confirming nonce 10 clears the pending entry.
    ledger.confirm(alice, 10);
    println!(
        "After confirm(10): confirmed={:?}, pending={:?}",
        ledger.confirmed_nonce(alice),
        ledger.pending_nonce(alice),
    );
    assert_eq!(ledger.confirmed_nonce(alice), Some(10));
    assert!(ledger.pending_nonce(alice).is_none());

    println!();

    // =====================================================================
    // 2. TxTracker — track pending transactions
    // =====================================================================
    println!("--- TxTracker ---\n");

    let config = TxTrackerConfig {
        confirmation_depth: 12,
        stuck_timeout_secs: 60, // 60 seconds before a tx is "stuck"
        poll_interval_secs: 3,
        max_tracked: 1000,
    };
    let tracker = TxTracker::new(config);

    // Simulate submitting a transaction at unix time 1000.
    let tx = TrackedTx {
        tx_hash: "0xaaa111".to_string(),
        from: alice.to_string(),
        nonce: 11,
        submitted_at: 1000,
        status: TxStatus::Pending,
        gas_price: Some(20_000_000_000),
        max_fee: None,
        max_priority_fee: None,
        last_checked: 1000,
    };
    tracker.track(tx);
    println!("Tracked tx 0xaaa111 (pending, submitted_at=1000)");
    println!("  count = {}", tracker.count());

    // Query pending transactions.
    let pending = tracker.pending();
    println!("  pending count = {}", pending.len());
    assert_eq!(pending.len(), 1);

    // Update status to Included (block mined but not yet confirmed).
    tracker.update_status(
        "0xaaa111",
        TxStatus::Included {
            block_number: 19_500_042,
            block_hash: "0xblockhash_42".to_string(),
        },
    );
    let updated = tracker.get("0xaaa111").unwrap();
    println!("  status after inclusion: {}", updated.status);

    // Update status to Confirmed (12+ confirmations reached).
    tracker.update_status(
        "0xaaa111",
        TxStatus::Confirmed {
            block_number: 19_500_042,
            confirmations: 12,
        },
    );
    let confirmed = tracker.get("0xaaa111").unwrap();
    println!("  status after confirmation: {}", confirmed.status);

    // Detect stuck transactions (demo with a second tx).
    let old_tx = TrackedTx {
        tx_hash: "0xbbb222".to_string(),
        from: alice.to_string(),
        nonce: 12,
        submitted_at: 900, // submitted 100 seconds before "now"
        status: TxStatus::Pending,
        gas_price: Some(15_000_000_000),
        max_fee: None,
        max_priority_fee: None,
        last_checked: 900,
    };
    tracker.track(old_tx);

    // At current_time=1000, the old tx has been pending for 100s (> 60s threshold).
    let stuck = tracker.stuck(1000);
    println!(
        "\nStuck transactions at t=1000 (timeout=60s): {}",
        stuck.len(),
    );
    for s in &stuck {
        println!(
            "  {} (pending for {}s)",
            s.tx_hash,
            1000 - s.submitted_at,
        );
    }
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].tx_hash, "0xbbb222");

    println!();

    // =====================================================================
    // 3. ReceiptPoller — exponential-backoff delay schedule
    // =====================================================================
    println!("--- ReceiptPoller ---\n");

    let poller = ReceiptPoller::new(ReceiptPollerConfig {
        initial_interval: Duration::from_secs(1),
        max_interval: Duration::from_secs(30),
        multiplier: 1.5,
        max_attempts: 20,
    });

    // Show the delay schedule for the first 5 attempts.
    println!("Delay schedule (initial=1s, multiplier=1.5x, cap=30s):");
    for attempt in 1..=5 {
        match poller.delay_for_attempt(attempt) {
            Some(delay) => {
                println!("  attempt {attempt}: wait {:.2}s", delay.as_secs_f64());
            }
            None => {
                println!("  attempt {attempt}: STOP (max attempts exceeded)");
            }
        }
    }

    // Verify the schedule grows: attempt 1=1s, 2=1.5s, 3=2.25s, 4=3.375s, 5=5.0625s
    let d1 = poller.delay_for_attempt(1).unwrap();
    let d2 = poller.delay_for_attempt(2).unwrap();
    let d5 = poller.delay_for_attempt(5).unwrap();
    assert!(d2 > d1, "delay should grow between attempts");
    assert!(d5 > d2, "delay should continue growing");

    // Beyond max_attempts, delay_for_attempt returns None.
    assert!(poller.delay_for_attempt(21).is_none());
    println!("  attempt 21: None (max_attempts=20 exceeded)");

    // should_continue mirrors the same logic.
    assert!(poller.should_continue(20));
    assert!(!poller.should_continue(21));

    println!();

    // =====================================================================
    // 4. Conceptual lifecycle: send_and_track / poll_receipt / etc.
    // =====================================================================
    println!("--- Conceptual Lifecycle (no real RPC) ---\n");

    // In production, you would call the async functions from
    // `chainrpc_core::tx_lifecycle`:
    //
    //   // Send a raw transaction and register it with the tracker:
    //   let tx_hash = send_and_track(&transport, &tracker, raw_tx, from, nonce).await?;
    //
    //   // Poll for the receipt with exponential backoff:
    //   let receipt = poll_receipt(&transport, &tx_hash, &poller).await?;
    //
    //   // Refresh a tracked transaction's status from chain:
    //   let status = refresh_status(&transport, &tracker, &tx_hash).await?;
    //
    //   // Detect and diagnose stuck transactions:
    //   let stuck = detect_stuck(&transport, &tracker, current_unix_time).await;
    //
    // Each function composes TxTracker / ReceiptPoller primitives with a
    // live RpcTransport, so the caller only needs to wire up the transport.

    println!("send_and_track()  -> sends eth_sendRawTransaction, registers with TxTracker");
    println!("poll_receipt()    -> calls eth_getTransactionReceipt with backoff");
    println!("refresh_status()  -> queries receipt, updates TxTracker status");
    println!("detect_stuck()    -> finds old Pending txs, refreshes on-chain nonces");

    println!("\nDone.");
}
