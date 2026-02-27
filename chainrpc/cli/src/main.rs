//! chainrpc CLI — test and inspect RPC providers from the terminal.
//!
//! Usage:
//! ```bash
//! # Test an RPC endpoint
//! chainrpc test --url https://cloudflare-eth.com
//!
//! # Send a raw JSON-RPC call
//! chainrpc call --url https://cloudflare-eth.com --method eth_blockNumber
//!
//! # List supported provider profiles
//! chainrpc providers
//! ```

use std::env;
use std::process;
use std::sync::Arc;

use chainrpc_core::transport::RpcTransport;
use chainrpc_http::HttpRpcClient;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "test" => cmd_test(&args[2..]).await,
        "call" => cmd_call(&args[2..]).await,
        "providers" => {
            cmd_providers();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("chainrpc {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

fn print_usage() {
    println!("chainrpc {}", env!("CARGO_PKG_VERSION"));
    println!("Test and inspect blockchain RPC providers\n");
    println!("USAGE:");
    println!("    chainrpc <COMMAND>\n");
    println!("COMMANDS:");
    println!("    test       Test an RPC endpoint (latency, block number)");
    println!("    call       Send a raw JSON-RPC call");
    println!("    providers  List built-in provider profiles");
    println!("    version    Print version");
    println!("    help       Print this help\n");
    println!("TEST FLAGS:");
    println!("    --url <URL>   RPC endpoint URL  [required]");
}

async fn cmd_test(args: &[String]) -> Result<(), String> {
    let url = parse_flag(args, "--url").ok_or("--url is required")?;
    let client = Arc::new(HttpRpcClient::default_for(&url));

    println!("Testing {url}...");

    let start = std::time::Instant::now();
    let block: String = client
        .call(1, "eth_blockNumber", vec![])
        .await
        .map_err(|e| e.to_string())?;
    let latency = start.elapsed();

    let block_num = u64::from_str_radix(block.trim_start_matches("0x"), 16)
        .unwrap_or(0);

    println!("  Status:       OK");
    println!("  Block number: {block_num} ({block})");
    println!("  Latency:      {}ms", latency.as_millis());
    println!("  Health:       {}", client.health());

    Ok(())
}

async fn cmd_call(args: &[String]) -> Result<(), String> {
    let url = parse_flag(args, "--url").ok_or("--url is required")?;
    let method = parse_flag(args, "--method").ok_or("--method is required")?;

    let client = HttpRpcClient::default_for(&url);
    let result: serde_json::Value = client
        .call(1, &method, vec![])
        .await
        .map_err(|e| e.to_string())?;

    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    Ok(())
}

fn cmd_providers() {
    println!("Built-in provider profiles:\n");
    println!("  alchemy   Alchemy (https://alchemy.com)");
    println!("            Chains: Ethereum, Polygon, Arbitrum, Optimism, Base");
    println!("            Auth:   API key");
    println!();
    println!("  infura    Infura (https://infura.io)");
    println!("            Chains: Ethereum, Polygon, Arbitrum, Optimism");
    println!("            Auth:   Project ID");
    println!();
    println!("  quicknode QuickNode (https://quicknode.com)");
    println!("            Chains: All EVM chains");
    println!("            Auth:   Endpoint URL");
    println!();
    println!("  public    Free public endpoints (no API key needed)");
    println!("            Cloudflare, Ankr, LlamaRPC");
}

fn parse_flag<'a>(args: &'a [String], flag: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).cloned()
}
