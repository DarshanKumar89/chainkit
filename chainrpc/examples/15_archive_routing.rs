//! # Archive-Aware Provider Routing
//!
//! Demonstrates capability-based routing that directs requests to the right kind
//! of node based on what the request requires:
//!
//! - **Full nodes** keep only recent state (~128 blocks). They are cheaper and
//!   faster but cannot serve historical queries.
//! - **Archive nodes** retain the complete chain history and support
//!   `debug_*`/`trace_*` methods. They cost more per CU.
//!
//! Key APIs:
//!
//! - `ProviderCapabilities::full_node()` / `archive_node()` — preset capability sets
//! - `analyze_request()` — inspects the RPC method and block parameter to determine
//!   whether the request needs archive state or trace capability
//! - `select_capable_provider()` — picks the first allowed provider whose
//!   capabilities satisfy the request's requirements

use chainrpc_core::routing::{
    analyze_request, select_capable_provider, ProviderCapabilities, RequestRequirement,
};

#[tokio::main]
async fn main() {
    println!("=== Archive-Aware Provider Routing Demo ===\n");

    // ---------------------------------------------------------------
    // 1. Define two providers with different capabilities
    // ---------------------------------------------------------------
    let providers = vec![
        ProviderCapabilities::full_node(),    // index 0: cheap full node
        ProviderCapabilities::archive_node(), // index 1: expensive archive node
    ];
    let provider_names = ["Full node (Alchemy Growth)", "Archive node (QuickNode)"];

    println!("Provider 0: {} — {:?}", provider_names[0], providers[0]);
    println!("Provider 1: {} — {:?}", provider_names[1], providers[1]);

    // Both providers are healthy (circuit breakers allow traffic).
    let allowed = [true, true];

    // Assume the current head block is 20_000_000.
    let current_block: u64 = 20_000_000;

    // ---------------------------------------------------------------
    // 2. eth_getBalance at "latest" — full node is sufficient
    // ---------------------------------------------------------------
    println!("\n--- eth_getBalance at \"latest\" ---");
    let req = analyze_request("eth_getBalance", Some("latest"), current_block);
    println!("  needs_archive: {}", req.needs_archive);
    println!("  needs_trace  : {}", req.needs_trace);

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );
    // Full node (index 0) is selected — cheaper and capable.

    // ---------------------------------------------------------------
    // 3. eth_getBalance at block "0x100" — needs archive
    // ---------------------------------------------------------------
    println!("\n--- eth_getBalance at block \"0x100\" (block 256) ---");
    let req = analyze_request("eth_getBalance", Some("0x100"), current_block);
    println!("  needs_archive: {}", req.needs_archive);
    println!("  needs_trace  : {}", req.needs_trace);
    // Block 0x100 = 256, which is 19_999_744 blocks behind head — clearly historical.

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );
    // Archive node (index 1) is selected — only one that has the old state.

    // ---------------------------------------------------------------
    // 4. eth_getBalance close to head — full node is fine
    // ---------------------------------------------------------------
    println!("\n--- eth_getBalance 10 blocks behind head ---");
    let near_head = format!("0x{:x}", current_block - 10);
    let req = analyze_request("eth_getBalance", Some(&near_head), current_block);
    println!("  block_param  : {near_head}");
    println!("  needs_archive: {}", req.needs_archive);

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );
    // Only 10 blocks back — well within the full node's 128-block window.

    // ---------------------------------------------------------------
    // 5. debug_traceTransaction — needs trace + archive
    // ---------------------------------------------------------------
    println!("\n--- debug_traceTransaction ---");
    let req = analyze_request("debug_traceTransaction", None, current_block);
    println!("  needs_archive: {}", req.needs_archive);
    println!("  needs_trace  : {}", req.needs_trace);

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );
    // Archive node (index 1) — only one with trace support.

    // ---------------------------------------------------------------
    // 6. trace_block — also needs trace + archive
    // ---------------------------------------------------------------
    println!("\n--- trace_block ---");
    let req = analyze_request("trace_block", None, current_block);
    println!("  needs_archive: {}", req.needs_archive);
    println!("  needs_trace  : {}", req.needs_trace);

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );

    // ---------------------------------------------------------------
    // 7. "earliest" block — always needs archive
    // ---------------------------------------------------------------
    println!("\n--- eth_getBalance at \"earliest\" ---");
    let req = analyze_request("eth_getBalance", Some("earliest"), current_block);
    println!("  needs_archive: {} (genesis state is always pruned)", req.needs_archive);

    let idx = select_capable_provider(&providers, &allowed, &req);
    println!(
        "  selected     : provider {} ({})",
        idx.unwrap(),
        provider_names[idx.unwrap()]
    );

    // ---------------------------------------------------------------
    // 8. No capable provider — graceful failure
    // ---------------------------------------------------------------
    println!("\n--- No archive node available ---");
    let only_full = vec![ProviderCapabilities::full_node()];
    let allowed_one = [true];
    let req = analyze_request("eth_getBalance", Some("0x100"), current_block);

    let idx = select_capable_provider(&only_full, &allowed_one, &req);
    match idx {
        Some(i) => println!("  selected: provider {i}"),
        None => println!("  selected: NONE — no provider can serve this request!"),
    }
    // Returns None — the caller should return TransportError::AllProvidersDown
    // or try a different provider tier.

    // ---------------------------------------------------------------
    // 9. can_handle() — direct capability check
    // ---------------------------------------------------------------
    println!("\n--- Direct capability checks ---");
    let full = ProviderCapabilities::full_node();
    let archive = ProviderCapabilities::archive_node();

    let cases = [
        ("recent read",   RequestRequirement { needs_archive: false, needs_trace: false, method: None }),
        ("archive read",  RequestRequirement { needs_archive: true,  needs_trace: false, method: None }),
        ("trace call",    RequestRequirement { needs_archive: false, needs_trace: true,  method: None }),
        ("archive+trace", RequestRequirement { needs_archive: true,  needs_trace: true,  method: None }),
    ];

    println!("  {:20} full_node  archive_node", "requirement");
    for (label, req) in &cases {
        println!(
            "  {:20} {:>9}  {:>12}",
            label,
            full.can_handle(req),
            archive.can_handle(req)
        );
    }

    println!("\n=== Done ===");
}
