//! ChainCodec CLI — the production command-line interface for ChainCodec.
//!
//! # Commands
//! ```
//! chaincodec parse       --file <path.csdl>
//! chaincodec decode-log  --topics <...> --data <hex> --schema <dir>
//! chaincodec decode-call --calldata <hex> --abi <path.json>
//! chaincodec encode-call --function <name> --args <json> --abi <path.json>
//! chaincodec fetch-abi   --address <addr> --chain-id <num>
//! chaincodec detect-proxy --address <addr> --rpc <url>
//! chaincodec verify      --schema <Name> --chain <slug> --tx <hash>
//! chaincodec test        --fixtures <dir>
//! chaincodec bench       --schema <Name> --iterations <N>
//! chaincodec info
//! chaincodec schemas     list|search|validate
//! ```

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod cmd_parse;
mod cmd_test;
mod cmd_verify;

#[derive(Parser)]
#[command(
    name = "chaincodec",
    about = "Universal blockchain ABI decoder — ChainCodec CLI",
    long_about = "
ChainCodec CLI: decode EVM events, function calls, and constructor data.
Built on alloy-rs. Supports Ethereum, Arbitrum, Base, Polygon, Optimism.

ENVIRONMENT VARIABLES:
  CHAINCODEC_RPC_ETHEREUM    Ethereum mainnet RPC URL
  CHAINCODEC_RPC_ARBITRUM    Arbitrum RPC URL
  CHAINCODEC_ETHERSCAN_KEY   Etherscan API key (for fetch-abi)
",
    version
)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Parse and validate a CSDL schema file
    Parse {
        /// Path to the .csdl file
        #[arg(short, long)]
        file: String,
        /// Show parsed field details
        #[arg(long)]
        verbose: bool,
    },

    /// Decode an EVM event log from raw topics + data
    #[command(name = "decode-log")]
    DecodeLog {
        /// topics[0] = event signature hash, topics[1..] = indexed params
        #[arg(long, num_args = 1..)]
        topics: Vec<String>,
        /// Non-indexed params (hex, 0x-prefixed)
        #[arg(long, default_value = "0x")]
        data: String,
        /// Directory containing CSDL schema files (default: ./schemas)
        #[arg(long, default_value = "./schemas")]
        schema_dir: String,
        /// Chain name (default: ethereum)
        #[arg(long, default_value = "ethereum")]
        chain: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Decode function call calldata using an ABI JSON file
    #[command(name = "decode-call")]
    DecodeCall {
        /// Raw calldata (0x-prefixed hex)
        #[arg(long)]
        calldata: String,
        /// Path to the ABI JSON file
        #[arg(long)]
        abi: String,
        /// Hint: the expected function name (optional)
        #[arg(long)]
        function: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Encode a function call to ABI calldata
    #[command(name = "encode-call")]
    EncodeCall {
        /// Function name
        #[arg(long)]
        function: String,
        /// JSON array of arguments, e.g. '["0xabc...", "1000000"]'
        #[arg(long)]
        args: String,
        /// Path to the ABI JSON file
        #[arg(long)]
        abi: String,
    },

    /// Fetch contract ABI from Sourcify or Etherscan
    #[command(name = "fetch-abi")]
    FetchAbi {
        /// Contract address
        #[arg(long)]
        address: String,
        /// EVM chain ID (default: 1 = Ethereum mainnet)
        #[arg(long, default_value_t = 1)]
        chain_id: u64,
        /// Save ABI to this file (default: stdout)
        #[arg(long)]
        output: Option<String>,
        /// Force fetch from Etherscan even if Sourcify succeeds
        #[arg(long)]
        etherscan: bool,
    },

    /// Detect and classify proxy contract patterns
    #[command(name = "detect-proxy")]
    DetectProxy {
        /// Contract address to inspect
        #[arg(long)]
        address: String,
        /// RPC URL for storage slot reads
        #[arg(long)]
        rpc: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Verify a schema against real on-chain transaction data
    Verify {
        /// Schema name to verify, e.g. UniswapV3Swap
        #[arg(long)]
        schema: String,
        /// Chain slug, e.g. ethereum
        #[arg(long)]
        chain: String,
        /// Transaction hash
        #[arg(long)]
        tx: String,
        /// RPC URL (overrides env CHAINCODEC_RPC_<CHAIN>)
        #[arg(long)]
        rpc: Option<String>,
    },

    /// Run golden test fixtures
    Test {
        /// Directory containing fixture JSON files
        #[arg(long, default_value = "./fixtures")]
        fixtures: String,
        /// Directory containing CSDL schema files (default: ./schemas)
        #[arg(long, default_value = "./schemas")]
        schema_dir: String,
        /// Only run fixtures matching this schema name
        #[arg(long)]
        schema: Option<String>,
        /// Run in verbose mode
        #[arg(short, long)]
        verbose: bool,
    },

    /// Benchmark decode throughput
    Bench {
        /// Schema name to benchmark
        #[arg(long)]
        schema: String,
        /// Number of iterations
        #[arg(long, default_value_t = 100_000)]
        iterations: u64,
        /// Number of parallel Rayon threads (0 = use default)
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Schema registry management
    Schemas {
        #[command(subcommand)]
        action: SchemasAction,
    },

    /// Show ChainCodec build and capability info
    Info,
}

#[derive(Subcommand)]
enum SchemasAction {
    /// List all schemas in a directory
    List {
        #[arg(long, default_value = "./schemas")]
        dir: String,
    },
    /// Search schemas by protocol, category, or event name
    Search {
        #[arg(long)]
        protocol: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        event: Option<String>,
        #[arg(long, default_value = "./schemas")]
        dir: String,
    },
    /// Validate all schema files in a directory
    Validate {
        #[arg(long, default_value = "./schemas")]
        dir: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Parse { file, verbose } => {
            cmd_parse::run(&file, verbose)
        }

        Commands::DecodeLog { topics, data, schema_dir, chain, json } => {
            cmd_decode_log(&topics, &data, &schema_dir, &chain, json)
        }

        Commands::DecodeCall { calldata, abi, function, json } => {
            cmd_decode_call(&calldata, &abi, function.as_deref(), json)
        }

        Commands::EncodeCall { function, args, abi } => {
            cmd_encode_call(&function, &args, &abi)
        }

        Commands::FetchAbi { address, chain_id, output, etherscan } => {
            cmd_fetch_abi(&address, chain_id, output.as_deref(), etherscan).await
        }

        Commands::DetectProxy { address, rpc, json } => {
            cmd_detect_proxy(&address, rpc.as_deref(), json).await
        }

        Commands::Verify { schema, chain, tx, rpc } => {
            cmd_verify::run(&schema, &chain, &tx, rpc.as_deref()).await
        }

        Commands::Test { fixtures, schema_dir, schema, verbose } => {
            cmd_test::run(&fixtures, &schema_dir, schema.as_deref(), verbose).await
        }

        Commands::Bench { schema, iterations, threads } => {
            cmd_bench(&schema, iterations, threads)
        }

        Commands::Schemas { action } => match action {
            SchemasAction::List { dir } => cmd_schemas_list(&dir),
            SchemasAction::Search { protocol, category, event, dir } => {
                cmd_schemas_search(&dir, protocol.as_deref(), category.as_deref(), event.as_deref())
            }
            SchemasAction::Validate { dir } => cmd_schemas_validate(&dir),
        },

        Commands::Info => cmd_info(),
    }
}

// ─── Command implementations ─────────────────────────────────────────────────

fn cmd_decode_log(
    topics: &[String],
    data: &str,
    schema_dir: &str,
    chain: &str,
    as_json: bool,
) -> Result<()> {
    use chaincodec_core::{chain::chains, decoder::ChainDecoder, event::RawEvent};
    use chaincodec_evm::EvmDecoder;
    use chaincodec_registry::MemoryRegistry;

    let registry = MemoryRegistry::new();
    let loaded = registry.load_directory(std::path::Path::new(schema_dir))?;
    if loaded == 0 {
        anyhow::bail!("no schemas found in '{}'", schema_dir);
    }

    let chain_id = match chain.to_lowercase().as_str() {
        "ethereum" | "eth" => chains::ethereum(),
        "arbitrum" => chains::arbitrum(),
        "base" => chains::base(),
        "polygon" => chains::polygon(),
        "optimism" => chains::optimism(),
        "avalanche" => chains::avalanche(),
        "bsc" => chains::bsc(),
        _ => chains::ethereum(),
    };

    let data_bytes = hex::decode(data.strip_prefix("0x").unwrap_or(data))
        .context("invalid data hex")?;

    let raw = RawEvent {
        chain: chain_id,
        tx_hash: "0x0".into(),
        block_number: 0,
        block_timestamp: 0,
        log_index: 0,
        address: "0x0".into(),
        topics: topics.to_vec(),
        data: data_bytes,
        raw_receipt: None,
    };

    let decoder = EvmDecoder::new();
    let fp = decoder.fingerprint(&raw);
    let schema = registry.get_by_fingerprint(&fp)
        .ok_or_else(|| anyhow!("no schema found for fingerprint {}", fp.as_hex()))?;

    let decoded = decoder.decode_event(&raw, &schema)?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&decoded)?);
    } else {
        println!("Schema:  {} v{}", decoded.schema, decoded.schema_version);
        println!("Fields:");
        for (name, val) in &decoded.fields {
            println!("  {}: {}", name, val);
        }
        if !decoded.decode_errors.is_empty() {
            println!("Errors:");
            for (k, v) in &decoded.decode_errors {
                println!("  {}: {}", k, v);
            }
        }
    }
    Ok(())
}

fn cmd_decode_call(
    calldata: &str,
    abi_path: &str,
    function: Option<&str>,
    as_json: bool,
) -> Result<()> {
    use chaincodec_evm::EvmCallDecoder;

    let abi_json = std::fs::read_to_string(abi_path)
        .with_context(|| format!("read ABI file '{}'", abi_path))?;
    let decoder = EvmCallDecoder::from_abi_json(&abi_json)?;

    let bytes = hex::decode(calldata.strip_prefix("0x").unwrap_or(calldata))
        .context("invalid calldata hex")?;

    let decoded = decoder.decode_call(&bytes, function)?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&decoded)?);
    } else {
        println!("Function:  {}", decoded.function_name);
        println!("Selector:  {}", decoded.selector_hex().unwrap_or_default());
        println!("Inputs:");
        for (name, val) in &decoded.inputs {
            println!("  {}: {}", name, val);
        }
    }
    Ok(())
}

fn cmd_encode_call(function: &str, args_json: &str, abi_path: &str) -> Result<()> {
    use chaincodec_core::types::NormalizedValue;
    use chaincodec_evm::EvmEncoder;

    let abi_json = std::fs::read_to_string(abi_path)
        .with_context(|| format!("read ABI file '{}'", abi_path))?;
    let encoder = EvmEncoder::from_abi_json(&abi_json)?;

    let args: Vec<NormalizedValue> = serde_json::from_str(args_json)
        .context("parse args JSON")?;

    let calldata = encoder.encode_call(function, &args)?;
    println!("0x{}", hex::encode(&calldata));
    Ok(())
}

async fn cmd_fetch_abi(
    address: &str,
    chain_id: u64,
    output: Option<&str>,
    force_etherscan: bool,
) -> Result<()> {
    use chaincodec_registry::AbiFetcher;

    println!("Fetching ABI for {} (chain_id={}) ...", address, chain_id);

    let mut fetcher = AbiFetcher::new();

    // Set Etherscan API key from env if available
    if let Ok(key) = std::env::var("CHAINCODEC_ETHERSCAN_KEY") {
        fetcher = fetcher.with_etherscan_key(key);
    }

    // Use chain-specific Etherscan fork
    let etherscan_base = match chain_id {
        1       => "https://api.etherscan.io/api",
        42161   => "https://api.arbiscan.io/api",
        8453    => "https://api.basescan.org/api",
        137     => "https://api.polygonscan.com/api",
        10      => "https://api-optimistic.etherscan.io/api",
        56      => "https://api.bscscan.com/api",
        43114   => "https://api.snowtrace.io/api",
        _       => "https://api.etherscan.io/api",
    };
    fetcher = fetcher.with_etherscan_base(etherscan_base);

    let source = if force_etherscan { "Etherscan" } else { "Sourcify → Etherscan (fallback)" };
    println!("Source: {}", source);

    let abi_json = if force_etherscan {
        fetcher.fetch_from_etherscan(address).await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    } else {
        fetcher.fetch_abi(chain_id, address).await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    };

    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<serde_json::Value>(&abi_json)
            .context("returned ABI is not valid JSON")?,
    )?;

    match output {
        Some(path) => {
            std::fs::write(path, &pretty)
                .with_context(|| format!("write ABI to '{}'", path))?;
            println!("✓ ABI written to '{}'", path);
        }
        None => println!("{}", pretty),
    }
    Ok(())
}

async fn cmd_detect_proxy(address: &str, rpc: Option<&str>, as_json: bool) -> Result<()> {
    use chaincodec_evm::proxy::{
        classify_from_storage, detect_eip1167_clone, proxy_detection_slots, storage_to_address,
    };

    let slots = proxy_detection_slots();

    // If no RPC URL, show static slot list
    let Some(rpc_url) = rpc else {
        if as_json {
            let info = serde_json::json!({
                "address": address,
                "note": "Pass --rpc <url> for live proxy detection",
                "storage_slots": slots.iter().map(|(label, slot)| {
                    serde_json::json!({ "label": label, "slot": slot })
                }).collect::<Vec<_>>()
            });
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!("Proxy detection for: {}", address);
            println!("(Pass --rpc <url> for live detection)");
            println!("\nStorage slots to check:");
            for (label, slot) in &slots {
                println!("  {:22} {}", label, slot);
            }
        }
        return Ok(());
    };

    // Live detection via JSON-RPC
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    println!("Detecting proxy pattern for {} ...", address);

    // Fetch bytecode to check EIP-1167 minimal proxy
    let bytecode = rpc_call::<String>(
        &client,
        rpc_url,
        "eth_getCode",
        serde_json::json!([address, "latest"]),
    )
    .await
    .unwrap_or_default();

    let bytecode_bytes =
        hex::decode(bytecode.strip_prefix("0x").unwrap_or(&bytecode)).unwrap_or_default();

    let clone_impl = detect_eip1167_clone(&bytecode_bytes);

    // Query each storage slot
    let mut slot_values: std::collections::HashMap<&str, String> = Default::default();
    for (label, slot) in &slots {
        let val = rpc_call::<String>(
            &client,
            rpc_url,
            "eth_getStorageAt",
            serde_json::json!([address, slot, "latest"]),
        )
        .await
        .unwrap_or_else(|_| "0x".repeat(32));
        slot_values.insert(label, val);
    }

    // classify_from_storage takes raw slot hex values (not addresses)
    let eip1967_impl_raw = slot_values.get("eip1967_impl").map(String::as_str);
    let eip1967_beacon_raw = slot_values.get("eip1967_beacon").map(String::as_str);
    let eip1822_raw = slot_values.get("eip1822_proxiable").map(String::as_str);

    let mut info = classify_from_storage(address, eip1967_impl_raw, eip1967_beacon_raw, eip1822_raw);

    // If not classified by storage, check for EIP-1167 clone from bytecode
    if info.kind == chaincodec_evm::proxy::ProxyKind::Unknown {
        if let Some(clone_addr) = clone_impl {
            info.kind = chaincodec_evm::proxy::ProxyKind::Eip1167Clone;
            info.implementation = Some(clone_addr);
        }
    }

    if as_json {
        println!("{}", serde_json::to_string_pretty(&serde_json::to_value(&info)?)?);
    } else {
        println!("  Kind:           {:?}", info.kind);
        println!("  Implementation: {}", info.implementation.as_deref().unwrap_or("none"));
        if let Some(slot) = &info.slot {
            println!("  Via slot:       {}", slot);
        }
    }
    Ok(())
}

async fn rpc_call<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<T> {
    #[derive(serde::Deserialize)]
    struct Resp<T> {
        result: Option<T>,
        error: Option<serde_json::Value>,
    }
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": method, "params": params
    });
    let resp: Resp<T> = client
        .post(url)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    if let Some(e) = resp.error {
        anyhow::bail!("RPC error: {}", e);
    }
    resp.result.ok_or_else(|| anyhow::anyhow!("null result from {}", method))
}

fn cmd_bench(schema: &str, iterations: u64, threads: usize) -> Result<()> {
    use std::time::Instant;
    use chaincodec_core::{chain::chains, decoder::{ChainDecoder, ErrorMode}, event::RawEvent};
    use chaincodec_evm::EvmDecoder;
    use chaincodec_registry::{CsdlParser, MemoryRegistry};

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

    let registry = MemoryRegistry::new();
    registry.insert(CsdlParser::parse(ERC20_CSDL)?)?;

    // Generate synthetic events
    let batch: Vec<RawEvent> = (0..iterations)
        .map(|i| {
            let mut from = vec![0u8; 32];
            from[31] = (i & 0xFF) as u8;
            let mut to = vec![0u8; 32];
            to[31] = ((i + 1) & 0xFF) as u8;
            let mut data = vec![0u8; 32];
            let b = (i as u64).to_be_bytes();
            data[24..].copy_from_slice(&b);

            RawEvent {
                chain: chains::ethereum(),
                tx_hash: format!("0x{:064x}", i),
                block_number: 19_000_000 + i,
                block_timestamp: (1_700_000_000i64) + i as i64,
                log_index: 0,
                topics: vec![
                    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef".into(),
                    format!("0x{}", hex::encode(&from)),
                    format!("0x{}", hex::encode(&to)),
                ],
                data,
                address: "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".into(),
                raw_receipt: None,
            }
        })
        .collect();

    let decoder = EvmDecoder::new();

    if threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .ok();
    }

    println!("Benchmarking '{}': {} iterations ...", schema, iterations);

    let start = Instant::now();
    let result = decoder.decode_batch(&batch, &registry, ErrorMode::Skip, None)?;
    let elapsed = start.elapsed();

    let total = iterations;
    let success = result.events.len() as u64;
    let errors = result.errors.len() as u64;
    let throughput = total as f64 / elapsed.as_secs_f64();

    println!("Results:");
    println!("  Total:      {} events", total);
    println!("  Decoded:    {} ({:.1}%)", success, 100.0 * success as f64 / total as f64);
    println!("  Errors:     {}", errors);
    println!("  Duration:   {:.3}s", elapsed.as_secs_f64());
    println!("  Throughput: {:.0} events/sec", throughput);
    if throughput >= 1_000_000.0 {
        println!("  ✓ Exceeds 1M events/sec target");
    }

    Ok(())
}

fn cmd_schemas_list(dir: &str) -> Result<()> {
    use chaincodec_registry::MemoryRegistry;

    let mut registry = MemoryRegistry::new();
    let count = registry.load_directory(std::path::Path::new(dir))?;

    println!("Loaded {} schemas from '{}'", count, dir);
    let mut names = registry.all_names();
    names.sort();
    for name in &names {
        println!("  {}", name);
    }
    Ok(())
}

fn cmd_schemas_search(
    dir: &str,
    protocol: Option<&str>,
    category: Option<&str>,
    event_name: Option<&str>,
) -> Result<()> {
    use chaincodec_registry::MemoryRegistry;

    let mut registry = MemoryRegistry::new();
    registry.load_directory(std::path::Path::new(dir))?;

    let all = registry.all_schemas();
    let mut matches = Vec::new();

    for schema in &all {
        let proto_match = protocol.map_or(true, |p| {
            schema.meta.protocol.as_deref().unwrap_or("").contains(p)
        });
        let cat_match = category.map_or(true, |c| {
            schema.meta.category.as_deref().unwrap_or("").contains(c)
        });
        let event_match = event_name.map_or(true, |e| {
            schema.event.to_lowercase().contains(&e.to_lowercase())
        });

        if proto_match && cat_match && event_match {
            matches.push(schema);
        }
    }

    println!("Found {} matching schemas:", matches.len());
    for s in matches {
        println!(
            "  {:40} protocol={:20} category={:15} event={}",
            s.name,
            s.meta.protocol.as_deref().unwrap_or(""),
            s.meta.category.as_deref().unwrap_or(""),
            s.event
        );
    }
    Ok(())
}

fn cmd_schemas_validate(dir: &str) -> Result<()> {
    use chaincodec_registry::CsdlParser;
    use std::path::Path;

    let mut ok = 0;
    let mut errors = 0;

    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "csdl"))
    {
        let path = entry.path();
        let content = std::fs::read_to_string(path)?;
        match CsdlParser::parse_all(&content) {
            Ok(schemas) => {
                ok += schemas.len();
                println!("  ✓ {} ({} schemas)", path.display(), schemas.len());
            }
            Err(e) => {
                errors += 1;
                eprintln!("  ✗ {}: {}", path.display(), e);
            }
        }
    }

    println!("\n{} schemas valid, {} files with errors", ok, errors);
    if errors > 0 {
        anyhow::bail!("{} schema files failed validation", errors);
    }
    Ok(())
}

fn cmd_info() -> Result<()> {
    println!("ChainCodec v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Capabilities:");
    println!("  ✓ EVM event decoding       (alloy-core)");
    println!("  ✓ Function call decoding   (alloy-json-abi)");
    println!("  ✓ ABI encoding             (alloy-core)");
    println!("  ✓ EIP-712 typed data       (structured)");
    println!("  ✓ Proxy detection          (EIP-1967, EIP-1822, EIP-1167)");
    println!("  ✓ CSDL schema parser       (multi-doc YAML)");
    println!("  ✓ In-memory registry       (indexed by fingerprint + name)");
    println!("  ✓ Parallel batch decode    (Rayon)");
    println!("  ✓ Remote ABI fetch         (Sourcify + Etherscan, feature=remote)");
    println!();
    println!("Bundled schemas:             tokens/ defi/ nft/ bridge/ governance/");
    println!("Supported chains:            Ethereum, Arbitrum, Base, Polygon, Optimism,");
    println!("                             Avalanche, BSC, and any EVM-compatible chain");
    println!();
    println!("Bindings:");
    println!("  npm:    @chainfoundry/chaincodec      (napi-rs)");
    println!("  pypi:   chaincodec                (PyO3/maturin)");
    println!("  wasm:   @chainfoundry/chaincodec-wasm (wasm-bindgen)");
    Ok(())
}
