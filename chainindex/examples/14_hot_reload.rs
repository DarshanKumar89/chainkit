//! Example 14: Hot-Reload Configuration
//!
//! Demonstrates updating indexer configs at runtime without restarting.
//!
//! Run: `cargo run --example 14_hot_reload`

use chainindex_core::hotreload::*;
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::EventFilter;

#[tokio::main]
async fn main() {
    println!("=== Hot-Reload Configuration Demo ===\n");

    // 1. Create a reloadable config
    let config = IndexerConfig {
        id: "uniswap-v3".into(),
        chain: "ethereum".into(),
        from_block: 19_000_000,
        batch_size: 500,
        poll_interval_ms: 2000,
        confirmation_depth: 12,
        ..Default::default()
    };

    let mut reloadable = ReloadableConfig::new(config.clone());
    println!("Initial config (version {}):", reloadable.version);
    println!("  batch_size:       {}", reloadable.inner.batch_size);
    println!("  poll_interval_ms: {}", reloadable.inner.poll_interval_ms);

    // 2. Update the config
    let mut new_config = config.clone();
    new_config.batch_size = 1000;
    new_config.poll_interval_ms = 5000;

    let new_version = reloadable.update(new_config.clone());
    println!("\nUpdated config (version {}):", new_version);
    println!("  batch_size:       {}", reloadable.inner.batch_size);
    println!("  poll_interval_ms: {}", reloadable.inner.poll_interval_ms);

    // 3. Diff configs
    println!("\n--- Config Diff ---");
    let diffs = diff_configs(&config, &new_config);
    for diff in &diffs {
        println!(
            "  {} changed: {} → {}",
            diff.field, diff.old_value, diff.new_value
        );
    }

    // 4. Validate changes
    println!("\n--- Validation ---");
    let warnings = ConfigValidator::validate(&config, &new_config).unwrap();
    if warnings.is_empty() {
        println!("  No warnings — safe to apply");
    } else {
        for w in &warnings {
            println!("  [{:?}] {}: {}", w.severity, w.field, w.message);
        }
    }

    println!(
        "  is_safe_reload: {}",
        ConfigValidator::is_safe_reload(&config, &new_config)
    );

    // 5. Dangerous change — try to change chain
    println!("\n--- Dangerous Change (chain) ---");
    let mut bad_config = config.clone();
    bad_config.chain = "polygon".into();

    match ConfigValidator::validate(&config, &bad_config) {
        Ok(_) => println!("  Accepted (unexpected)"),
        Err(e) => println!("  REJECTED: {}", e),
    }

    println!(
        "  is_safe_reload: {}",
        ConfigValidator::is_safe_reload(&config, &bad_config)
    );

    // 6. HotReloadManager
    println!("\n--- HotReloadManager ---");
    let manager = HotReloadManager::new();

    // Register configs
    manager.register_config("eth-indexer", config.clone()).await;
    manager
        .register_config(
            "poly-indexer",
            IndexerConfig {
                id: "polygon-tracker".into(),
                chain: "polygon".into(),
                from_block: 50_000_000,
                batch_size: 1000,
                ..Default::default()
            },
        )
        .await;

    println!("Registered configs: {:?}", manager.configs().await);

    // Subscribe to changes
    let mut rx = manager.subscribe("eth-indexer").await.unwrap();
    println!("Initial version: {}", *rx.borrow());

    // Update config
    let result = manager
        .update_config(
            "eth-indexer",
            IndexerConfig {
                id: "uniswap-v3".into(),
                chain: "ethereum".into(),
                from_block: 19_000_000,
                batch_size: 2000,
                poll_interval_ms: 3000,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    println!("\nReload result:");
    println!("  Version: {}", result.version);
    println!("  Diffs:   {}", result.diffs.len());
    for d in &result.diffs {
        println!("    {} : {} → {}", d.field, d.old_value, d.new_value);
    }
    println!("  Warnings: {}", result.warnings.len());

    // Check version changed
    rx.changed().await.unwrap();
    println!("  Subscriber notified: version now {}", *rx.borrow());

    // 7. History
    println!("\n--- Reload History ---");
    let history = manager.history("eth-indexer").await;
    println!("{} reload(s) recorded:", history.len());
    for record in &history {
        println!(
            "  v{}: {} change(s) at {}",
            record.version,
            record.diffs.len(),
            record.applied_at
        );
    }

    // 8. Filter reloader
    println!("\n--- Filter Reloader ---");
    let reloader = FilterReloader::new(EventFilter::address("0xToken1"));
    println!("Initial: {:?}", reloader.current().await.addresses);

    reloader.add_address("0xToken2").await;
    reloader.add_address("0xToken3").await;
    println!("After adding: {:?}", reloader.current().await.addresses);

    reloader.remove_address("0xToken1").await;
    println!("After removing 0xToken1: {:?}", reloader.current().await.addresses);

    reloader.add_topic0("0xa9059cbb").await;
    println!("Topics: {:?}", reloader.current().await.topic0_values);

    println!("\nHot-reload demo complete!");
}
