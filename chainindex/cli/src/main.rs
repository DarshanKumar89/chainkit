//! chainindex CLI — inspect and manage indexer state.
//!
//! Usage:
//! ```bash
//! chainindex status --chain ethereum --id my-indexer
//! chainindex reset  --chain ethereum --id my-indexer
//! chainindex info
//! ```

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "info" => cmd_info(),
        "version" | "--version" | "-V" => {
            println!("chainindex {}", env!("CARGO_PKG_VERSION"));
        }
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            process::exit(1);
        }
    }
}

fn print_usage() {
    println!("chainindex {}", env!("CARGO_PKG_VERSION"));
    println!("Reorg-safe, embeddable blockchain indexing engine\n");
    println!("USAGE:");
    println!("    chainindex <COMMAND>\n");
    println!("COMMANDS:");
    println!("    info     Show ChainIndex configuration info");
    println!("    version  Print version");
    println!("    help     Print this help");
}

fn cmd_info() {
    println!("ChainIndex v{}", env!("CARGO_PKG_VERSION"));
    println!("  Default confirmation depth: 12 blocks");
    println!("  Default batch size: 1000 blocks/call");
    println!("  Default checkpoint interval: every 100 blocks");
    println!("  Storage backends: memory, SQLite (feature: sqlite)");
    println!("  Chains: EVM (Ethereum, Arbitrum, Base, Polygon, Optimism, ...)");
}
