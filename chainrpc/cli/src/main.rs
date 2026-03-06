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
//! # Send a call with parameters
//! chainrpc call --url https://cloudflare-eth.com --method eth_getBalance --params '["0x...", "latest"]'
//!
//! # Benchmark an RPC endpoint
//! chainrpc bench --url https://cloudflare-eth.com --count 100 --concurrency 10
//!
//! # Test a provider pool
//! chainrpc pool --urls https://cloudflare-eth.com,https://rpc.ankr.com/eth --count 20
//!
//! # List supported provider profiles
//! chainrpc providers
//! ```

use std::env;
use std::process;
use std::sync::Arc;

use chainrpc_core::metrics::ProviderMetrics;
use chainrpc_core::request::JsonRpcRequest;
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
        "bench" => cmd_bench(&args[2..]).await,
        "pool" => cmd_pool(&args[2..]).await,
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
    println!("    bench      Benchmark an RPC endpoint (throughput, latency)");
    println!("    pool       Test a multi-provider pool (failover, health)");
    println!("    providers  List built-in provider profiles");
    println!("    version    Print version");
    println!("    help       Print this help\n");
    println!("FLAGS:");
    println!("    --url <URL>           RPC endpoint URL");
    println!("    --urls <URL1,URL2>    Comma-separated URLs (for pool command)");
    println!("    --method <METHOD>     JSON-RPC method name");
    println!("    --params <JSON>       JSON array of parameters (default: [])");
    println!("    --count <N>           Number of requests (default: 100 for bench, 20 for pool)");
    println!("    --concurrency <N>     Concurrent requests (default: 10, bench only)");
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
    let params_str = parse_flag(args, "--params").unwrap_or_else(|| "[]".to_string());

    let params: Vec<serde_json::Value> = serde_json::from_str(&params_str)
        .map_err(|e| format!("invalid --params JSON: {e}"))?;

    let client = HttpRpcClient::default_for(&url);
    let req = JsonRpcRequest::auto(method, params);
    let resp = client.send(req).await.map_err(|e| e.to_string())?;

    if let Some(err) = resp.error {
        return Err(format!("JSON-RPC error {}: {}", err.code, err.message));
    }

    let result = resp.result.unwrap_or(serde_json::Value::Null);
    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    Ok(())
}

async fn cmd_bench(args: &[String]) -> Result<(), String> {
    let url = parse_flag(args, "--url").ok_or("--url is required")?;
    let count: usize = parse_flag(args, "--count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let concurrency: usize = parse_flag(args, "--concurrency")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let client = Arc::new(HttpRpcClient::default_for(&url));
    let metrics = Arc::new(ProviderMetrics::new(&url));

    println!("Benchmarking {url}");
    println!("  Requests:    {count}");
    println!("  Concurrency: {concurrency}\n");

    let start = std::time::Instant::now();
    let sem = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::with_capacity(count);

    for _i in 0..count {
        let client = client.clone();
        let metrics = metrics.clone();
        let permit = sem.clone().acquire_owned().await.unwrap();
        handles.push(tokio::spawn(async move {
            let req_start = std::time::Instant::now();
            let req = JsonRpcRequest::auto(
                "eth_blockNumber".to_string(),
                vec![],
            );
            match client.send(req).await {
                Ok(_) => metrics.record_success(req_start.elapsed()),
                Err(_) => metrics.record_failure(),
            }
            drop(permit);
        }));
    }

    for h in handles {
        let _ = h.await;
    }

    let elapsed = start.elapsed();
    let snap = metrics.snapshot();
    let rps = if elapsed.as_secs_f64() > 0.0 {
        count as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!("Results:");
    println!("  Total time:   {:.2}s", elapsed.as_secs_f64());
    println!("  Requests/sec: {:.1}", rps);
    println!("  Success rate: {:.1}%", snap.success_rate * 100.0);
    println!("  Avg latency:  {:.1}ms", snap.avg_latency_ms);
    println!("  Min latency:  {:.1}ms", snap.min_latency_ms);
    println!("  Max latency:  {:.1}ms", snap.max_latency_ms);

    Ok(())
}

async fn cmd_pool(args: &[String]) -> Result<(), String> {
    let urls_str = parse_flag(args, "--urls").ok_or("--urls is required (comma-separated)")?;
    let urls: Vec<&str> = urls_str.split(',').collect();
    let count: usize = parse_flag(args, "--count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    println!("Testing provider pool ({} providers, {} requests)...\n", urls.len(), count);

    let pool = chainrpc_http::pool_from_urls(&urls)
        .map_err(|e| e.to_string())?;

    let mut successes = 0u64;
    let mut failures = 0u64;
    let start = std::time::Instant::now();

    for _ in 0..count {
        let req = JsonRpcRequest::auto(
            "eth_blockNumber".to_string(),
            vec![],
        );
        match pool.send(req).await {
            Ok(_) => successes += 1,
            Err(e) => {
                failures += 1;
                tracing::debug!(error = %e, "pool request failed");
            }
        }
    }

    let elapsed = start.elapsed();
    println!("  Total time:   {:.2}s", elapsed.as_secs_f64());
    println!("  Successes:    {successes}");
    println!("  Failures:     {failures}");
    println!("  Pool health:  {}", pool.health());

    println!("\nProvider status:");
    for report in pool.health_report() {
        println!("  {} — health: {}, circuit: {}",
            report["url"].as_str().unwrap_or("?"),
            report["health"].as_str().unwrap_or("?"),
            report["circuit"].as_str().unwrap_or("?"),
        );
    }

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

fn parse_flag(args: &[String], flag: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == flag)?;
    args.get(pos + 1).cloned()
}
