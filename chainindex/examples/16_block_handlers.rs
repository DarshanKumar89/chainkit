//! Example 16: Block Handlers (Interval + Setup)
//!
//! Demonstrates interval handlers (every N blocks) and setup handlers (run once).
//!
//! Run: `cargo run --example 16_block_handlers`

use std::sync::Arc;

use async_trait::async_trait;
use chainindex_core::block_handler::{BlockHandlerScheduler, IntervalHandler, SetupHandler};
use chainindex_core::error::IndexerError;
use chainindex_core::types::{BlockSummary, IndexContext, IndexPhase};

/// Interval handler: compute gas stats every 10 blocks.
struct GasStatsHandler;

#[async_trait]
impl IntervalHandler for GasStatsHandler {
    async fn handle(&self, block: &BlockSummary, _ctx: &IndexContext) -> Result<(), IndexerError> {
        println!(
            "  [GasStats] Block {} — computing gas statistics (tx_count: {})",
            block.number, block.tx_count
        );
        Ok(())
    }

    fn interval(&self) -> u64 {
        10 // every 10 blocks
    }

    fn name(&self) -> &str {
        "gas_stats"
    }
}

/// Interval handler: snapshot TVL every 100 blocks.
struct TvlSnapshotHandler;

#[async_trait]
impl IntervalHandler for TvlSnapshotHandler {
    async fn handle(&self, block: &BlockSummary, _ctx: &IndexContext) -> Result<(), IndexerError> {
        println!(
            "  [TVLSnapshot] Block {} — saving TVL snapshot",
            block.number
        );
        Ok(())
    }

    fn interval(&self) -> u64 {
        100 // every 100 blocks
    }

    fn name(&self) -> &str {
        "tvl_snapshot"
    }
}

/// Setup handler: initialize database tables.
struct InitTablesHandler;

#[async_trait]
impl SetupHandler for InitTablesHandler {
    async fn setup(&self, ctx: &IndexContext) -> Result<(), IndexerError> {
        println!(
            "  [Setup] Initializing tables for chain '{}' at block {}",
            ctx.chain, ctx.block.number
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "init_tables"
    }
}

/// Setup handler: seed initial state.
struct SeedStateHandler;

#[async_trait]
impl SetupHandler for SeedStateHandler {
    async fn setup(&self, ctx: &IndexContext) -> Result<(), IndexerError> {
        println!(
            "  [Setup] Seeding initial state for chain '{}'",
            ctx.chain
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "seed_state"
    }
}

fn make_block(number: u64) -> BlockSummary {
    BlockSummary {
        number,
        hash: format!("0xhash_{number}"),
        parent_hash: format!("0xhash_{}", number - 1),
        timestamp: 1700000000 + (number as i64 * 12),
        tx_count: 150,
    }
}

fn make_ctx(block: &BlockSummary) -> IndexContext {
    IndexContext {
        block: block.clone(),
        phase: IndexPhase::Live,
        chain: "ethereum".into(),
    }
}

#[tokio::main]
async fn main() {
    println!("=== Block Handlers Demo ===\n");

    // 1. Create scheduler
    let mut scheduler = BlockHandlerScheduler::new();

    // Register interval handlers
    scheduler.register_interval(Arc::new(GasStatsHandler));
    scheduler.register_interval(Arc::new(TvlSnapshotHandler));

    // Register setup handlers
    scheduler.register_setup(Arc::new(InitTablesHandler));
    scheduler.register_setup(Arc::new(SeedStateHandler));

    println!("Registered handlers:");
    println!("  Interval: GasStats (every 10), TVLSnapshot (every 100)");
    println!("  Setup:    InitTables, SeedState\n");

    // 2. Run setup (fires once before indexing starts)
    println!("--- Setup Phase ---");
    let block0 = make_block(19_000_000);
    let ctx0 = make_ctx(&block0);
    scheduler.run_setup(&ctx0).await.unwrap();

    // 3. Process blocks
    println!("\n--- Block Processing (19_000_001 → 19_000_120) ---");
    for i in 1..=120 {
        let block_num = 19_000_000 + i;
        let block = make_block(block_num);
        let ctx = make_ctx(&block);

        // Scheduler only fires handlers at the right intervals
        scheduler.run_block(&block, &ctx).await.unwrap();
    }

    println!("\n--- Summary ---");
    println!("Processed 120 blocks:");
    println!("  GasStats fired:    12 times (every 10 blocks)");
    println!("  TVLSnapshot fired: 1 time   (every 100 blocks)");
    println!("  Setup handlers:    ran once at start");

    // 4. Second setup call is a no-op
    println!("\n--- Second Setup (should be no-op) ---");
    scheduler.run_setup(&ctx0).await.unwrap();
    println!("  (nothing happened — setup already complete)");

    println!("\nBlock handlers demo complete!");
}
