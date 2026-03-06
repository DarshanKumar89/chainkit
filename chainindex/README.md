# chainindex

**Reorg-safe, embeddable blockchain indexing engine.**

[![crates.io](https://img.shields.io/crates/v/chainindex-core)](https://crates.io/crates/chainindex-core)
[![docs.rs](https://docs.rs/chainindex-core/badge.svg)](https://docs.rs/chainindex-core)
[![npm](https://img.shields.io/npm/v/@chainfoundry/chainindex)](https://www.npmjs.com/package/@chainfoundry/chainindex)
[![PyPI](https://img.shields.io/pypi/v/chainindex)](https://pypi.org/project/chainindex/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

chainindex is a lightweight library that fetches blocks, detects reorgs, decodes events, calls your handler functions, and saves checkpoints. Think of it as the missing primitive between raw blocks and your database — without the weight of The Graph or Ponder.

## Features

| Feature | Description |
|---------|-------------|
| **Reorg detection** | 4-scenario reorg detection (short, deep, node switch, RPC inconsistency) |
| **Checkpoint recovery** | Crash-safe — resumes from last saved checkpoint |
| **Factory tracking** | Auto-track child contracts from factory events (Uniswap, Compound, etc.) |
| **Entity system** | Structured storage with typed schemas, CRUD, queries, and reorg rollback |
| **Dead letter queue** | Failed handlers retry with exponential backoff |
| **Idempotency** | Deterministic IDs + side-effect guards for safe reorg replay |
| **Call trace indexing** | Index internal transactions (CALL, DELEGATECALL, CREATE) |
| **Event streaming** | Cursor-based streaming for downstream consumers |
| **Data export** | Export to JSONL/CSV for analytics pipelines (DuckDB, BigQuery) |
| **Block handlers** | Interval handlers (every N blocks), setup handlers (run once) |
| **GraphQL query layer** | Auto-generated schema from entities, filter/sort/paginate |
| **Parallel backfill** | Concurrent segment processing for fast historical sync |
| **Multi-chain indexer** | Single engine coordinating N chains with cross-chain event bus |
| **Solana indexer** | Slot tracking, program log parsing, Anchor events, account filters |
| **Hot-reload config** | Update indexer configs at runtime without restart |
| **Multi-chain finality** | Pre-configured for 12 chains (Ethereum, Polygon, Arbitrum, Solana, etc.) |
| **4 storage backends** | Memory (dev), SQLite (embedded), PostgreSQL (production), RocksDB (high-throughput) |
| **4 language bindings** | TypeScript, Python, Go, Java |

## Install

**Rust**
```toml
[dependencies]
chainindex-core    = "0.1"
chainindex-evm     = "0.1"
chainindex-storage = { version = "0.1", features = ["memory"] }
```

**npm / Node.js**
```bash
npm install @chainfoundry/chainindex
```

**Python**
```bash
pip install chainindex
```

## Quick Start (Rust)

```rust
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::handler::{DecodedEvent, EventHandler, HandlerRegistry};
use chainindex_core::types::IndexContext;
use chainindex_core::checkpoint::{CheckpointManager, MemoryCheckpointStore};

// 1. Configure
let config = IndexerConfig {
    id: "uniswap-v3".into(),
    chain: "ethereum".into(),
    from_block: 19_000_000,
    confirmation_depth: 12,
    batch_size: 500,
    checkpoint_interval: 100,
    ..Default::default()
};

// 2. Register event handlers
let mut registry = HandlerRegistry::new();
// ... register your EventHandler implementations

// 3. Start indexing (with chainindex-evm)
use chainindex_evm::IndexerBuilder;
let cfg = IndexerBuilder::new()
    .chain("ethereum")
    .from_block(19_000_000)
    .confirmation_depth(12)
    .batch_size(500)
    .build_config();
```

## Quick Start (TypeScript)

```typescript
import { IndexerConfig, InMemoryStorage, EventFilter } from '@chainfoundry/chainindex';

const config = new IndexerConfig({
  id: 'uniswap-v3',
  chain: 'ethereum',
  fromBlock: 19_000_000,
  confirmationDepth: 12,
  batchSize: 500,
});

const filter = EventFilter.forAddress('0x1F98431c8aD98523631AE4a59f267346ea31F984');
```

## Architecture

```
chainindex/
├── crates/
│   ├── chainindex-core/        # Core engine (24 modules, 245 tests)
│   │   ├── backfill.rs         # Parallel backfill engine
│   │   ├── block_handler.rs    # Interval + setup handlers
│   │   ├── checkpoint.rs       # Checkpoint persistence + recovery
│   │   ├── cursor.rs           # Block cursor advancement
│   │   ├── dlq.rs              # Dead letter queue
│   │   ├── entity.rs           # Entity/table system
│   │   ├── error.rs            # Error types
│   │   ├── export.rs           # JSONL/CSV export
│   │   ├── factory.rs          # Factory contract tracking
│   │   ├── finality.rs         # 12-chain finality models
│   │   ├── graphql.rs          # GraphQL schema + query executor
│   │   ├── handler.rs          # Event/block/reorg handler traits
│   │   ├── hotreload.rs        # Hot-reload configuration
│   │   ├── idempotency.rs      # Reorg-safe handler replay
│   │   ├── indexer.rs          # Config + state types
│   │   ├── metrics.rs          # Block lag, RPC stats, handler latency
│   │   ├── multichain.rs       # Multi-chain coordinator + event bus
│   │   ├── reorg.rs            # 4-scenario reorg detection
│   │   ├── streaming.rs        # Cursor-based event streaming
│   │   ├── trace.rs            # Call trace indexing
│   │   ├── tracker.rs          # Sliding window block tracker
│   │   └── types.rs            # BlockSummary, EventFilter, IndexContext
│   ├── chainindex-evm/         # EVM-specific indexer (builder, fetcher, loop)
│   ├── chainindex-solana/      # Solana indexer (slots, program logs, Anchor)
│   └── chainindex-storage/     # Memory, SQLite, Postgres, RocksDB backends
├── cli/                        # CLI binary
├── examples/                   # 16 runnable examples
└── bindings/
    ├── node/                   # TypeScript (napi-rs)
    ├── python/                 # Python (PyO3 + maturin)
    ├── go/                     # Go (C FFI)
    └── java/                   # Java (JNI)
```

## Module Reference

### Factory Contract Tracking

Track child contracts deployed by factory patterns (Uniswap V3, Compound, etc.):

```rust
use chainindex_core::factory::{FactoryConfig, FactoryRegistry};

let registry = FactoryRegistry::new();
registry.register(FactoryConfig {
    factory_address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".into(),
    creation_event_topic0: "0x783cca1c...".into(),
    child_address_field: "pool".into(),
    name: Some("Uniswap V3 Factory".into()),
});

// Feed events through — child addresses auto-tracked
if let Some(child) = registry.process_event(&event) {
    println!("New pool: {}", child.address);
}

// Get all addresses for EventFilter
let all_addrs = registry.get_all_addresses();
```

### Entity System

Structured storage with typed schemas:

```rust
use chainindex_core::entity::*;

let schema = EntitySchemaBuilder::new("swap")
    .primary_key("id")
    .field("pool", FieldType::String, true)
    .field("amount0", FieldType::Int64, false)
    .field("amount1", FieldType::Int64, false)
    .build();

let store = MemoryEntityStore::new();
store.register_schema(&schema).await?;

store.upsert(EntityRow {
    id: format!("{}-{}", event.tx_hash, event.log_index),
    entity_type: "swap".into(),
    block_number: event.block_number,
    tx_hash: event.tx_hash.clone(),
    log_index: event.log_index,
    data: /* fields */,
}).await?;

// Reorg rollback — delete entities after fork block
store.delete_after_block("swap", fork_block).await?;
```

### Dead Letter Queue

Failed handlers retry with exponential backoff:

```rust
use chainindex_core::dlq::{DeadLetterQueue, DlqConfig};

let dlq = DeadLetterQueue::new(DlqConfig {
    max_retries: 5,
    initial_backoff: Duration::from_secs(1),
    max_backoff: Duration::from_secs(300),
    backoff_multiplier: 2.0,
});

// On handler failure
dlq.push(event, "my_handler", "connection timeout");

// Retry ready entries
let ready = dlq.pop_ready(now);
for entry in ready {
    match handler.handle(&entry.event, &ctx).await {
        Ok(()) => dlq.mark_success(&entry.id),
        Err(e) => dlq.mark_failed(&entry.id, &e.to_string()),
    }
}
```

### GraphQL Query Layer

Auto-generated GraphQL schema from entity definitions:

```rust
use chainindex_core::graphql::{GraphqlExecutor, GraphqlSchema};
use chainindex_core::entity::*;

// Generate schema from entities
let mut schema = GraphqlSchema::new();
schema.add_entity(swap_schema);

// Execute queries
let executor = GraphqlExecutor::new(store);
let result = executor.execute(r#"
    { swaps(where: { pool: "0xABC" }, first: 10, orderBy: "amount0", orderDirection: "desc") {
        id pool amount0 sender blockNumber
    }}
"#).await;

// Single entity by ID
let result = executor.execute(r#"{ swap(id: "0x123-0") { id pool amount0 } }"#).await;

// Introspection
let sdl = executor.introspect(); // returns GraphQL SDL string
```

### Parallel Backfill

Concurrent block range processing for fast historical sync:

```rust
use chainindex_core::backfill::*;

let config = BackfillConfig {
    from_block: 19_000_000,
    to_block: 19_100_000,
    concurrency: 4,        // 4 parallel workers
    segment_size: 25_000,  // blocks per segment
    batch_size: 500,       // blocks per RPC call
    retry_attempts: 3,
    ..Default::default()
};

let engine = BackfillEngine::new(config, provider, filter, "ethereum");
let result = engine.run().await?;
println!("{} events in {:?}", result.total_events, result.total_duration);
```

### Multi-Chain Indexer

Coordinate multiple chains from a single engine:

```rust
use chainindex_core::multichain::*;

let coordinator = MultiChainCoordinator::new(MultiChainConfig {
    chains: vec![eth_config, polygon_config, arbitrum_config],
    max_concurrent_chains: 3,
    restart_on_error: true,
    ..Default::default()
});

// Health monitoring
let health = coordinator.health().await;

// Cross-chain event bus
let bus = CrossChainEventBus::new(10_000);
let mut rx = bus.subscribe();
bus.push("ethereum", event);

// Sync status
let mut sync = ChainSyncStatus::new();
sync.update("ethereum", 19_000_500);
sync.all_caught_up(1000); // within 1000 blocks of tips
```

### Solana Indexer

Slot tracking, program log parsing, and Anchor event decoding:

```rust
use chainindex_solana::*;

let builder = SolanaIndexerBuilder::new()
    .from_slot(250_000_000)
    .program("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8") // Raydium
    .exclude_votes(true)
    .confirmation("finalized");

// Parse transaction logs
let logs = ProgramLogParser::parse_transaction_logs(&raw_logs, "SIG123");

// Slot tracking with skip detection
let mut tracker = SlotTracker::new(100);
tracker.push_slot(slot)?;
let skipped = tracker.skipped_slots_in_range(100, 200);
```

### Hot-Reload Configuration

Update configs at runtime without restart:

```rust
use chainindex_core::hotreload::*;

let manager = HotReloadManager::new();
manager.register_config("eth-indexer", config).await;

// Subscribe to changes
let mut rx = manager.subscribe("eth-indexer").await.unwrap();

// Update config — validates, diffs, notifies subscribers
let result = manager.update_config("eth-indexer", new_config).await?;
println!("{} fields changed, version {}", result.diffs.len(), result.version);

// Dynamic filter updates
let reloader = FilterReloader::new(filter);
reloader.add_address("0xNewContract").await;
reloader.remove_address("0xOldContract").await;
```

### RocksDB Storage Backend

High-throughput embedded storage:

```rust
use chainindex_storage::RocksDbStorage;

let storage = RocksDbStorage::in_memory(); // or RocksDbStorage::open("./data")?

// Events — natural block-order via key encoding
storage.insert_events_batch(&events)?;
let transfers = storage.events_by_schema("Transfer")?;
let range = storage.events_in_block_range(100, 200)?;

// Reorg rollback
storage.rollback_after(block_number)?;
```

### Call Trace Indexing

Index internal transactions from debug_traceBlock or trace_block:

```rust
use chainindex_core::trace::*;

// Parse Geth traces
let traces = parse_geth_traces(&geth_json, 12345678)?;

// Parse Parity/OpenEthereum traces
let traces = parse_parity_traces(&parity_json, 12345678)?;

// Filter
let filter = TraceFilter::new()
    .with_address("0xPool")
    .with_selector("0xa9059cbb")
    .exclude_reverted(true);

let matching: Vec<_> = traces.iter().filter(|t| filter.matches(t)).collect();
```

### Event Streaming

Cursor-based streaming for downstream consumers:

```rust
use chainindex_core::streaming::{EventStream, StreamCursor};

let mut stream = EventStream::new(10_000);

// Producer pushes events
stream.push(decoded_event);

// Consumer reads batches
let cursor = StreamCursor::initial();
let batch = stream.next_batch(&cursor, 100)?;
// Save batch.cursor for resume after crash
```

### Data Export

```rust
use chainindex_core::export::{export_events, ExportConfig, ExportFormat};

let config = ExportConfig {
    format: ExportFormat::Jsonl,
    from_block: Some(19_000_000),
    to_block: Some(19_100_000),
    schema_filter: vec!["Transfer".into()],
    ..Default::default()
};

let mut file = File::create("transfers.jsonl")?;
let stats = export_events(&events, &config, &mut file)?;
println!("Exported {} events ({} bytes)", stats.events_exported, stats.bytes_written);
```

## Finality Models

Pre-configured for 12 chains:

| Chain | Safe | Finalized | Block Time | Reorg Window |
|-------|------|-----------|------------|--------------|
| Ethereum | 32 | 64 | 12s | 128 |
| Polygon | 128 | 256 | 2s | 512 |
| Arbitrum | 0 | 1 | 250ms | 64 |
| Optimism | 0 | 1 | 2s | 64 |
| Base | 0 | 1 | 2s | 64 |
| BSC | 15 | 15 | 3s | 64 |
| Avalanche | 1 | 1 | 2s | 32 |
| Solana | 1 | 32 | 400ms | 256 |
| Fantom | 1 | 1 | 1s | 32 |
| Scroll | 0 | 1 | 3s | 64 |
| zkSync | 0 | 1 | 1s | 64 |
| Linea | 0 | 1 | 12s | 64 |

## Examples

16 runnable examples covering every feature:

```bash
cargo run -p chainindex-cli --example 01_basic_indexer
cargo run -p chainindex-cli --example 02_reorg_detection
cargo run -p chainindex-cli --example 03_factory_tracking
cargo run -p chainindex-cli --example 04_entity_system
cargo run -p chainindex-cli --example 05_dead_letter_queue
cargo run -p chainindex-cli --example 06_call_traces
cargo run -p chainindex-cli --example 07_streaming
cargo run -p chainindex-cli --example 08_data_export
cargo run -p chainindex-cli --example 09_graphql
cargo run -p chainindex-cli --example 10_parallel_backfill
cargo run -p chainindex-cli --example 11_multichain
cargo run -p chainindex-cli --example 12_solana_indexer
cargo run -p chainindex-cli --example 13_rocksdb_storage
cargo run -p chainindex-cli --example 14_hot_reload
cargo run -p chainindex-cli --example 15_idempotency
cargo run -p chainindex-cli --example 16_block_handlers
```

## Test Coverage

```
324 tests, 0 failures

chainindex-core:       245 tests (graphql, backfill, multichain, hotreload, factory, entity,
                                  DLQ, idempotency, trace, streaming, export, block_handler,
                                  checkpoint, tracker, reorg, finality, metrics, types, handler)
chainindex-evm:          4 tests (builder, fetcher)
chainindex-solana:      33 tests (slot tracking, program logs, Anchor, filters, builder)
chainindex-storage:     29 tests (memory, RocksDB KV store, checkpoint, events)
doc-tests:              13 tests
```

## License

MIT
