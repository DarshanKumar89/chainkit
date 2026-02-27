//! Batch decode throughput benchmarks.
//!
//! Measures decode throughput at various batch sizes using Criterion.
//!
//! # Running
//! ```bash
//! cd chaincodec
//! cargo bench --package chaincodec-batch
//! ```
//!
//! # Targets
//! - Single-thread: >1M ERC-20 Transfer events/second
//! - Rayon 8-thread: >5M events/second

use chaincodec_core::{
    chain::chains,
    decoder::{ChainDecoder, ErrorMode},
    event::RawEvent,
    schema::{Schema, SchemaRegistry},
    event::EventFingerprint,
};
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// ─── Schema setup ─────────────────────────────────────────────────────────────

const ERC20_CSDL: &str = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta: {}
"#;

fn make_registry() -> MemoryRegistry {
    let mut reg = MemoryRegistry::new();
    let schema = CsdlParser::parse(ERC20_CSDL).expect("parse erc20 schema");
    reg.insert(schema).expect("insert schema");
    reg
}

// ─── Event factory ────────────────────────────────────────────────────────────

fn make_transfer_event(i: u64) -> RawEvent {
    // Generate a variety of transfer events to avoid branch prediction cheating
    let sender_byte = (i & 0xFF) as u8;
    let amount_bytes = i.to_be_bytes();

    let mut from_topic = vec![0u8; 32];
    from_topic[31] = sender_byte;
    let mut to_topic = vec![0u8; 32];
    to_topic[31] = (sender_byte.wrapping_add(1)) as u8;

    let mut data = vec![0u8; 32];
    data[24..].copy_from_slice(&amount_bytes);

    RawEvent {
        chain: chains::ethereum(),
        tx_hash: format!("0x{:064x}", i),
        block_number: 19_000_000 + i,
        block_timestamp: 1_700_000_000 + i,
        log_index: 0,
        topics: vec![
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
            format!("0x{}", hex::encode(&from_topic)),
            format!("0x{}", hex::encode(&to_topic)),
        ],
        data,
        address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
        raw_receipt: None,
    }
}

fn make_batch(n: usize) -> Vec<RawEvent> {
    (0..n).map(|i| make_transfer_event(i as u64)).collect()
}

// ─── Benchmarks ───────────────────────────────────────────────────────────────

fn bench_sequential_decode(c: &mut Criterion) {
    let registry = make_registry();
    // Use the EvmDecoder directly (sequential)
    let decoder = chaincodec_evm::EvmDecoder::new();

    let mut group = c.benchmark_group("sequential_decode");
    for batch_size in [100, 1_000, 10_000, 100_000] {
        let batch = make_batch(batch_size);
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch,
            |b, batch| {
                b.iter(|| {
                    for event in batch {
                        let fp = decoder.fingerprint(event);
                        if let Some(schema) = registry.get_by_fingerprint(&fp) {
                            let _ = decoder.decode_event(event, &schema);
                        }
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_parallel_decode(c: &mut Criterion) {
    let registry = make_registry();
    let decoder = chaincodec_evm::EvmDecoder::new();

    let mut group = c.benchmark_group("parallel_decode_rayon");
    for batch_size in [1_000, 10_000, 100_000, 1_000_000] {
        let batch = make_batch(batch_size);
        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch,
            |b, batch| {
                b.iter(|| {
                    let _ = decoder.decode_batch(batch, &registry, ErrorMode::Skip, None);
                });
            },
        );
    }
    group.finish();
}

fn bench_fingerprint(c: &mut Criterion) {
    let decoder = chaincodec_evm::EvmDecoder::new();
    let event = make_transfer_event(0);

    c.bench_function("fingerprint_single", |b| {
        b.iter(|| decoder.fingerprint(&event));
    });
}

fn bench_registry_lookup(c: &mut Criterion) {
    let registry = make_registry();
    let fp = EventFingerprint::new(
        "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
    );

    c.bench_function("registry_get_by_fingerprint", |b| {
        b.iter(|| registry.get_by_fingerprint(&fp));
    });
}

criterion_group!(
    benches,
    bench_sequential_decode,
    bench_parallel_decode,
    bench_fingerprint,
    bench_registry_lookup,
);
criterion_main!(benches);
