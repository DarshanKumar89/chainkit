//! # stream_demo
//!
//! Demonstrates real-time streaming of decoded ERC-20 Transfer events from an
//! Ethereum WebSocket endpoint using `StreamEngine` + `EvmWsListener`.
//!
//! The example connects to a public Ethereum WebSocket endpoint, subscribes to
//! USDC Transfer events, and prints each decoded event as it arrives.
//! Press Ctrl-C to stop.
//!
//! # Environment Variables
//!
//! Set `ETH_WS_URL` to your WebSocket RPC endpoint (defaults to a public Cloudflare endpoint):
//! ```sh
//! export ETH_WS_URL="wss://mainnet.infura.io/ws/v3/YOUR_KEY"
//! cargo run --bin stream_demo
//! ```
//!
//! Note: The public Cloudflare endpoint may not support `eth_subscribe`. Use a
//! dedicated provider (Infura, Alchemy, QuickNode) for real-time subscriptions.

use anyhow::Result;
use chaincodec_core::chain::chains;
use chaincodec_evm::EvmDecoder;
use chaincodec_registry::{CsdlParser, MemoryRegistry};
use chaincodec_stream::{EvmWsListener, StreamConfig, StreamEngine};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    // Set up structured logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // ── 1. Load the ERC-20 Transfer schema ───────────────────────────────────
    let csdl = r#"
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true

schema ERC20Approval:
  version: 1
  chains: [ethereum]
  event: Approval
  fingerprint: "0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925"
  fields:
    owner:   { type: address, indexed: true }
    spender: { type: address, indexed: true }
    value:   { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
"#;

    let registry = Arc::new(MemoryRegistry::new());
    for schema in CsdlParser::parse_all(csdl)? {
        registry.add(schema)?;
    }
    println!("✓ Registry loaded ({} schemas)", registry.len());

    // ── 2. Configure the WebSocket endpoint ──────────────────────────────────
    let ws_url = std::env::var("ETH_WS_URL")
        .unwrap_or_else(|_| "wss://ethereum.publicnode.com".into());
    println!("✓ Using WebSocket endpoint: {}", ws_url);

    // ── 3. Build StreamConfig ─────────────────────────────────────────────────
    // Filter to USDC contract only; remove this to receive all ERC-20 events.
    let usdc_address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

    let config = StreamConfig::single_chain("ethereum", ws_url.clone());

    // ── 4. Build StreamEngine ─────────────────────────────────────────────────
    let (mut engine, mut rx) = StreamEngine::new(config, registry);

    // Register the EVM WebSocket listener (filtered to USDC)
    let listener = Arc::new(
        EvmWsListener::new(chains::ethereum(), &ws_url)
            .with_address(usdc_address),
    );
    engine.add_listener(listener);

    // Register the EVM decoder
    engine.add_decoder("ethereum", Arc::new(EvmDecoder::new()));

    // ── 5. Start the engine ───────────────────────────────────────────────────
    let engine = Arc::new(engine);
    engine.clone().run().await;

    println!(
        "\n─── Streaming USDC events (Ctrl-C to stop) ──────────────────────"
    );
    println!("  Watching: {usdc_address}");
    println!("  Schemas:  ERC20Transfer, ERC20Approval\n");

    // ── 6. Consume decoded events until Ctrl-C ────────────────────────────────
    let mut count = 0usize;
    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\n\n─── Shutting down ───────────────────────────────────────");
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        count += 1;
                        let mut field_names: Vec<_> = event.fields.keys().collect();
                        field_names.sort();

                        println!(
                            "[#{count}] {} | block #{} | tx {}",
                            event.schema,
                            event.block_number,
                            &event.tx_hash[..18],
                        );
                        for name in &field_names {
                            println!("    {:12} = {}", name, event.fields[*name]);
                        }

                        if event.has_errors() {
                            println!("    ⚠ {} decode error(s)", event.decode_errors.len());
                        }
                        println!();
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("⚠ Lagged: skipped {n} events (consumer too slow)");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        eprintln!("Stream closed unexpectedly");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                let m = engine.metrics();
                println!(
                    "  [heartbeat] decoded={} skipped={} errors={} reconnections={}",
                    m.events_decoded, m.events_skipped, m.decode_errors, m.reconnections
                );
            }
        }
    }

    let m = engine.metrics();
    println!(
        "\nFinal metrics: decoded={} skipped={} errors={} reconnections={}",
        m.events_decoded, m.events_skipped, m.decode_errors, m.reconnections
    );
    println!("Total events received by consumer: {count}");

    Ok(())
}
