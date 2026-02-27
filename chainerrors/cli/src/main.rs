//! chainerrors CLI — decode EVM revert data from the terminal.
//!
//! Usage:
//! ```bash
//! # Decode raw hex revert data
//! chainerrors decode --data 0x08c379a0...
//!
//! # Decode with chain context
//! chainerrors decode --chain ethereum --data 0x08c379a0... --tx 0xabc...
//!
//! # Output as JSON
//! chainerrors decode --data 0x08c379a0... --json
//! ```

use std::env;
use std::process;

use chainerrors_core::{ErrorDecoder, ErrorContext};
use chainerrors_evm::EvmErrorDecoder;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "decode" => cmd_decode(&args[2..]),
        "help" | "--help" | "-h" => {
            print_usage();
        }
        "version" | "--version" | "-V" => {
            println!("chainerrors {}", env!("CARGO_PKG_VERSION"));
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            process::exit(1);
        }
    }
}

fn print_usage() {
    println!("chainerrors {}", env!("CARGO_PKG_VERSION"));
    println!("Decode EVM revert data\n");
    println!("USAGE:");
    println!("    chainerrors <COMMAND>\n");
    println!("COMMANDS:");
    println!("    decode    Decode hex revert data");
    println!("    version   Print version");
    println!("    help      Print this help\n");
    println!("DECODE FLAGS:");
    println!("    --data <HEX>      Revert data (0x-prefixed hex)  [required]");
    println!("    --chain <SLUG>    Chain name (e.g. ethereum)");
    println!("    --tx <HASH>       Transaction hash for context");
    println!("    --contract <ADDR> Contract address for context");
    println!("    --json            Output as JSON");
}

fn cmd_decode(args: &[String]) {
    let mut data_hex: Option<&str> = None;
    let mut chain: Option<String> = None;
    let mut tx_hash: Option<String> = None;
    let mut contract: Option<String> = None;
    let mut as_json = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--data" => {
                i += 1;
                data_hex = args.get(i).map(|s| s.as_str());
            }
            "--chain" => {
                i += 1;
                chain = args.get(i).cloned();
            }
            "--tx" => {
                i += 1;
                tx_hash = args.get(i).cloned();
            }
            "--contract" => {
                i += 1;
                contract = args.get(i).cloned();
            }
            "--json" => as_json = true,
            flag => {
                eprintln!("Unknown flag: {flag}");
                process::exit(1);
            }
        }
        i += 1;
    }

    let hex_str = match data_hex {
        Some(h) => h,
        None => {
            eprintln!("Error: --data is required");
            process::exit(1);
        }
    };

    let ctx = if chain.is_some() || tx_hash.is_some() || contract.is_some() {
        Some(ErrorContext {
            chain,
            tx_hash,
            contract_address: contract,
            call_selector: None,
            block_number: None,
        })
    } else {
        None
    };

    let decoder = EvmErrorDecoder::new();
    match decoder.decode_hex(hex_str, ctx) {
        Ok(decoded) => {
            if as_json {
                match serde_json::to_string_pretty(&decoded) {
                    Ok(json) => println!("{json}"),
                    Err(e) => {
                        eprintln!("JSON serialization error: {e}");
                        process::exit(1);
                    }
                }
            } else {
                println!("{decoded}");
                println!("  Kind:       {:?}", decoded.kind);
                println!("  Confidence: {:.0}%", decoded.confidence * 100.0);
                if let Some(hint) = &decoded.suggestion {
                    println!("  Hint:       {hint}");
                }
            }
        }
        Err(e) => {
            eprintln!("Decode error: {e}");
            process::exit(1);
        }
    }
}
