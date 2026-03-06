//! # Example 06: Tiered Response Cache
//!
//! Demonstrates `CacheTransport` with `CacheTierResolver` for intelligent,
//! tier-based caching of RPC responses.
//!
//! ## What this demonstrates
//!
//! - Setting up `CacheTransport` with `CacheTierResolver`
//! - Four cache tiers: Immutable, SemiStable, Volatile, NeverCache
//! - Immutable caching: tx receipts cached for 1 hour
//! - Volatile caching: `eth_blockNumber` expires in 2 seconds
//! - NeverCache: `eth_sendRawTransaction` always bypasses cache
//! - Reorg-based invalidation with `invalidate_for_reorg()`
//! - Cache statistics with `stats()` (hits, misses, size)

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chainrpc_core::cache::{CacheConfig, CacheStats, CacheTier, CacheTierResolver, CacheTransport};
use chainrpc_core::request::JsonRpcRequest;
use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;
use serde_json::json;

#[tokio::main]
async fn main() {
    // -----------------------------------------------------------------------
    // Step 1: Create the underlying HTTP transport.
    // -----------------------------------------------------------------------
    let http_client = Arc::new(
        HttpRpcClient::default_for("https://eth-mainnet.g.alchemy.com/v2/YOUR_API_KEY"),
    ) as Arc<dyn RpcTransport>;

    // -----------------------------------------------------------------------
    // Step 2: Configure the tiered cache.
    //
    // When `tier_resolver` is Some, the resolver classifies each request:
    //
    //   Immutable   (1h TTL) : eth_getTransactionReceipt, eth_getTransactionByHash,
    //                          eth_getBlockByHash, eth_getBlockByNumber (with hex number)
    //   SemiStable  (5m TTL) : eth_chainId, net_version, eth_getCode
    //   Volatile    (2s TTL) : eth_blockNumber, eth_gasPrice, eth_getBalance, eth_call
    //   NeverCache           : eth_sendRawTransaction, eth_subscribe, eth_sign
    //
    // The `cacheable_methods` set is ignored when a tier resolver is present.
    // -----------------------------------------------------------------------
    let cache_config = CacheConfig {
        default_ttl: Duration::from_secs(60),   // fallback TTL (rarely used with resolver)
        max_entries: 2048,                       // max cache entries before LRU eviction
        cacheable_methods: HashSet::new(),       // ignored when tier_resolver is Some
        tier_resolver: Some(CacheTierResolver::new()),
    };

    let cache = CacheTransport::new(http_client, cache_config);

    // -----------------------------------------------------------------------
    // Step 3: Immutable caching -- transaction receipts.
    //
    // Transaction receipts for confirmed transactions never change.
    // The CacheTierResolver classifies `eth_getTransactionReceipt` as
    // Immutable with a 1-hour TTL.
    // -----------------------------------------------------------------------
    println!("--- Immutable Tier (1h TTL) ---\n");

    let tx_hash = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
    let receipt_req = JsonRpcRequest::new(
        1,
        "eth_getTransactionReceipt",
        vec![json!(tx_hash)],
    );

    // First call: cache miss, hits the network.
    let _receipt = cache.send(receipt_req.clone()).await.expect("receipt fetch failed");
    let stats = cache.stats();
    println!("After 1st receipt fetch: misses={}, hits={}, size={}", stats.misses, stats.hits, stats.size);

    // Second call: cache hit, no network request.
    let _receipt = cache.send(receipt_req).await.expect("receipt fetch failed");
    let stats = cache.stats();
    println!("After 2nd receipt fetch: misses={}, hits={}, size={}", stats.misses, stats.hits, stats.size);

    // Verify: tier classification
    let resolver = CacheTierResolver::new();
    let tier = resolver.tier_for("eth_getTransactionReceipt", &[json!(tx_hash)]);
    println!("Tier: {:?} (TTL = {:?})", tier, tier.default_ttl());
    assert_eq!(tier, CacheTier::Immutable);

    // -----------------------------------------------------------------------
    // Step 4: Volatile caching -- eth_blockNumber.
    //
    // Block number changes every ~12 seconds on Ethereum. The resolver
    // classifies it as Volatile with a 2-second TTL.
    // -----------------------------------------------------------------------
    println!("\n--- Volatile Tier (2s TTL) ---\n");

    let block_req = JsonRpcRequest::new(2, "eth_blockNumber", vec![]);

    // First call: cache miss.
    let _block = cache.send(block_req.clone()).await.expect("blockNumber failed");
    let stats = cache.stats();
    println!("After 1st blockNumber: misses={}, hits={}, size={}", stats.misses, stats.hits, stats.size);

    // Immediate second call: cache hit (within 2s TTL).
    let _block = cache.send(block_req.clone()).await.expect("blockNumber failed");
    let stats = cache.stats();
    println!("After 2nd blockNumber (immediate): misses={}, hits={}, size={}", stats.misses, stats.hits, stats.size);

    // Wait for the volatile TTL to expire.
    println!("Waiting 2.1 seconds for volatile TTL to expire...");
    tokio::time::sleep(Duration::from_millis(2100)).await;

    // Third call: cache miss (TTL expired).
    let _block = cache.send(block_req).await.expect("blockNumber failed");
    let stats = cache.stats();
    println!("After 3rd blockNumber (after TTL): misses={}, hits={}, size={}", stats.misses, stats.hits, stats.size);

    // -----------------------------------------------------------------------
    // Step 5: NeverCache -- eth_sendRawTransaction.
    //
    // Write operations are never cached. Every call goes to the network.
    // -----------------------------------------------------------------------
    println!("\n--- NeverCache Tier ---\n");

    let tier = resolver.tier_for("eth_sendRawTransaction", &[]);
    println!("eth_sendRawTransaction tier: {:?} (TTL = {:?})", tier, tier.default_ttl());
    assert_eq!(tier, CacheTier::NeverCache);

    // If we sent two identical sendRawTransaction calls, both would hit the
    // network (cache is bypassed entirely for NeverCache methods).
    println!("NeverCache methods always bypass the cache.");

    // -----------------------------------------------------------------------
    // Step 6: Block-parameter awareness.
    //
    // `eth_getBlockByNumber` is classified based on its first parameter:
    //   - "0x10d4f" (concrete hex) => Immutable (finalized block, 1h TTL)
    //   - "latest" / "pending"     => Volatile (changes frequently, 2s TTL)
    // -----------------------------------------------------------------------
    println!("\n--- Block-Parameter Awareness ---\n");

    let tier_concrete = resolver.tier_for(
        "eth_getBlockByNumber",
        &[json!("0x10d4f"), json!(true)],
    );
    println!("eth_getBlockByNumber(0x10d4f): {:?}", tier_concrete);

    let tier_latest = resolver.tier_for(
        "eth_getBlockByNumber",
        &[json!("latest"), json!(true)],
    );
    println!("eth_getBlockByNumber(latest):  {:?}", tier_latest);

    assert_eq!(tier_concrete, CacheTier::Immutable);
    assert_eq!(tier_latest, CacheTier::Volatile);

    // -----------------------------------------------------------------------
    // Step 7: Reorg-based cache invalidation.
    //
    // When a chain reorganization is detected, cached data for blocks at
    // or above the reorg point must be invalidated.
    //
    // `invalidate_for_reorg(from_block)` removes all cache entries that
    // reference blocks >= from_block. Entries without a block reference
    // (like eth_chainId) are preserved.
    // -----------------------------------------------------------------------
    println!("\n--- Reorg Invalidation ---\n");

    // Cache some blocks.
    for block_num in [100u64, 200, 300] {
        let req = JsonRpcRequest::new(
            block_num,
            "eth_getBlockByNumber",
            vec![json!(format!("0x{:x}", block_num)), json!(true)],
        );
        let _ = cache.send(req).await;
    }

    let before = cache.stats();
    println!("Before reorg: cache size = {}", before.size);

    // Simulate a reorg at block 200: blocks 200 and 300 are now invalid.
    cache.invalidate_for_reorg(200);

    let after = cache.stats();
    println!("After reorg at block 200: cache size = {}", after.size);
    println!("Entries removed: {}", before.size - after.size);

    // -----------------------------------------------------------------------
    // Step 8: Cache statistics summary.
    // -----------------------------------------------------------------------
    println!("\n--- Final Cache Stats ---\n");
    let final_stats: CacheStats = cache.stats();
    println!("Total hits:   {}", final_stats.hits);
    println!("Total misses: {}", final_stats.misses);
    println!("Current size: {}", final_stats.size);

    let hit_rate = if (final_stats.hits + final_stats.misses) > 0 {
        final_stats.hits as f64 / (final_stats.hits + final_stats.misses) as f64
    } else {
        0.0
    };
    println!("Hit rate:     {:.1}%", hit_rate * 100.0);

    // -----------------------------------------------------------------------
    // Step 9: Manual invalidation.
    //
    // You can also invalidate specific methods or the entire cache.
    // -----------------------------------------------------------------------
    cache.invalidate_method("eth_blockNumber"); // remove all eth_blockNumber entries
    cache.invalidate();                         // clear the entire cache
    println!("\nCache cleared. Size: {}", cache.stats().size);
}
