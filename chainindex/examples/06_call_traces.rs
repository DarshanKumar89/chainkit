//! Example 06: Call Trace Indexing
//!
//! Demonstrates indexing internal transactions from debug_traceBlock
//! or trace_block RPC responses.
//!
//! Run: `cargo run --example 06_call_traces`

use chainindex_core::trace::*;

fn main() {
    println!("=== Call Trace Indexing Demo ===\n");

    // 1. Parse Geth-style traces (debug_traceBlockByNumber)
    println!("--- Geth Trace Parsing ---");
    let geth_json = serde_json::json!([{
        "txHash": "0xtxhash1",
        "result": {
            "type": "CALL",
            "from": "0xSender",
            "to": "0xRouter",
            "value": "0x0",
            "gas": "0x50000",
            "gasUsed": "0x30000",
            "input": "0xa9059cbb000000000000000000000000",
            "output": "0x",
            "calls": [
                {
                    "type": "CALL",
                    "from": "0xRouter",
                    "to": "0xPool",
                    "value": "0x0",
                    "gas": "0x20000",
                    "gasUsed": "0x15000",
                    "input": "0x022c0d9f",
                    "output": "0x"
                },
                {
                    "type": "DELEGATECALL",
                    "from": "0xRouter",
                    "to": "0xImpl",
                    "value": "0x0",
                    "gas": "0x10000",
                    "gasUsed": "0x8000",
                    "input": "0x128acb08",
                    "output": "0x"
                }
            ]
        }
    }]);

    let traces = parse_geth_traces(&geth_json, 19_000_100).unwrap();
    println!("Parsed {} traces from Geth format:", traces.len());
    for trace in &traces {
        println!(
            "  {:?} {} → {} (selector: {}, gas: {}, depth: {})",
            trace.call_type,
            trace.from,
            trace.to,
            trace.function_selector.as_deref().unwrap_or("none"),
            trace.gas_used,
            trace.depth
        );
    }

    // 2. Parse Parity/OpenEthereum traces (trace_block)
    println!("\n--- Parity Trace Parsing ---");
    let parity_json = serde_json::json!([
        {
            "action": {
                "callType": "call",
                "from": "0xSender",
                "to": "0xContract",
                "value": "0x0",
                "gas": "0x50000",
                "input": "0xa9059cbb"
            },
            "result": {
                "gasUsed": "0x30000",
                "output": "0x"
            },
            "traceAddress": [],
            "transactionHash": "0xtxhash2",
            "blockNumber": 19000101,
            "error": null
        },
        {
            "action": {
                "callType": "call",
                "from": "0xContract",
                "to": "0xToken",
                "value": "0x0",
                "gas": "0x20000",
                "input": "0x23b872dd"
            },
            "result": {
                "gasUsed": "0x10000",
                "output": "0x"
            },
            "traceAddress": [0],
            "transactionHash": "0xtxhash2",
            "blockNumber": 19000101,
            "error": null
        }
    ]);

    let traces = parse_parity_traces(&parity_json, 19_000_101).unwrap();
    println!("Parsed {} traces from Parity format:", traces.len());
    for trace in &traces {
        println!(
            "  {:?} {} → {} (selector: {}, gas: {})",
            trace.call_type,
            trace.from,
            trace.to,
            trace.function_selector.as_deref().unwrap_or("none"),
            trace.gas_used,
        );
    }

    // 3. Filter traces
    println!("\n--- Trace Filtering ---");
    let all_traces = parse_geth_traces(&geth_json, 19_000_100).unwrap();

    // Filter by address
    let filter = TraceFilter::new()
        .with_address("0xpool")
        .exclude_reverted(true);

    let matching: Vec<_> = all_traces.iter().filter(|t| filter.matches(t)).collect();
    println!(
        "Traces involving 0xPool: {} of {}",
        matching.len(),
        all_traces.len()
    );

    // Filter by selector (ERC-20 transfer)
    let transfer_filter = TraceFilter::new().with_selector("0xa9059cbb");

    let transfers: Vec<_> = all_traces
        .iter()
        .filter(|t| transfer_filter.matches(t))
        .collect();
    println!("Transfer traces (0xa9059cbb): {}", transfers.len());

    // Filter by call type
    let delegate_filter = TraceFilter::new().with_call_type(CallType::DelegateCall);

    let delegates: Vec<_> = all_traces
        .iter()
        .filter(|t| delegate_filter.matches(t))
        .collect();
    println!("DelegateCall traces: {}", delegates.len());

    println!("\nCall trace indexing demo complete!");
}
