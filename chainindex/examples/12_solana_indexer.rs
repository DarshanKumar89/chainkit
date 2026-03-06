//! Example 12: Solana Indexer
//!
//! Demonstrates Solana-specific indexing: slot tracking, program log parsing,
//! account filtering, and the Anchor event decoder.
//!
//! Run: `cargo run --example 12_solana_indexer`

use chainindex_solana::*;

fn main() {
    println!("=== Solana Indexer Demo ===\n");

    // 1. Build Solana indexer config
    let builder = SolanaIndexerBuilder::new()
        .id("raydium-swaps")
        .from_slot(250_000_000)
        .program("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8") // Raydium AMM
        .program("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc") // Orca Whirlpool
        .exclude_votes(true)
        .exclude_failed(false)
        .confirmation("finalized")
        .batch_size(100);

    let config = builder.build_config();
    let filter = builder.build_filter();

    println!("Indexer config:");
    println!("  ID:           {}", config.id);
    println!("  Chain:        {}", config.chain);
    println!("  From slot:    {}", config.from_block);
    println!("  Confirmation: {}", config.confirmation_depth);
    println!("  Batch size:   {}", config.batch_size);
    println!("\nAccount filter:");
    println!("  Programs:     {:?}", filter.program_ids);
    println!("  Exclude votes: {}", filter.exclude_vote_txs);
    println!("  Exclude failed: {}", filter.exclude_failed_txs);

    // 2. Simulate a Solana slot
    let slot = SolanaSlot {
        slot: 250_000_001,
        parent_slot: 250_000_000,
        block_time: Some(1700000012),
        block_hash: "EXAMPLEhash123456789".into(),
        tx_count: 1500,
        leader: Some("EXAMPLE_VALIDATOR_PUBKEY".into()),
        rewards: vec![
            SlotReward {
                pubkey: "VOTER1".into(),
                lamports: 5000,
                reward_type: RewardType::Voting,
            },
            SlotReward {
                pubkey: "STAKER1".into(),
                lamports: 10000,
                reward_type: RewardType::Staking,
            },
        ],
    };

    let summary = slot.to_block_summary();
    println!("\n--- Slot → BlockSummary ---");
    println!("  Block number: {} (slot)", summary.number);
    println!("  Hash:         {}", summary.hash);
    println!("  Parent:       {}", summary.parent_hash);
    println!("  Timestamp:    {}", summary.timestamp);
    println!("  Tx count:     {}", summary.tx_count);

    // 3. Parse transaction logs
    println!("\n--- Program Log Parsing ---");
    let logs = vec![
        "Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 invoke [1]".into(),
        "Program log: Instruction: Swap".into(),
        "Program log: amount_in=1000000".into(),
        "Program log: amount_out=2500000".into(),
        "Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 consumed 35000 of 200000 compute units"
            .into(),
        "Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 success".into(),
    ];

    let parsed = ProgramLogParser::parse_transaction_logs(&logs, "SIG123abc");
    println!("Parsed {} program log(s):", parsed.len());
    for log in &parsed {
        println!("  Program: {}", log.program_id);
        println!("  Success: {}", log.success);
        println!("  Compute: {} units", log.compute_units);
        println!("  Messages:");
        for msg in &log.log_messages {
            println!("    {}", msg);
        }
    }

    // 4. Account filter matching
    println!("\n--- Account Filter ---");
    if !parsed.is_empty() {
        let matches = filter.matches(&parsed[0]);
        println!(
            "Filter matches Raydium log: {} (program_id in filter list)",
            matches
        );
    }

    // Test vote program exclusion
    let vote_log = ProgramLog {
        program_id: "Vote111111111111111111111111111111111111111".into(),
        instruction_index: 0,
        inner_instruction_index: None,
        log_messages: vec![],
        data: None,
        accounts: vec![],
        success: true,
        compute_units: 100,
    };
    println!(
        "Filter matches vote program: {} (excluded by exclude_vote_txs)",
        filter.matches(&vote_log)
    );

    // 5. Slot tracker with skip detection
    println!("\n--- Slot Tracker ---");
    let mut tracker = SlotTracker::new(100);

    // Push consecutive slots
    tracker
        .push_slot(SolanaSlot {
            slot: 100,
            parent_slot: 99,
            block_time: None,
            block_hash: "h100".into(),
            tx_count: 0,
            leader: None,
            rewards: vec![],
        })
        .unwrap();
    tracker
        .push_slot(SolanaSlot {
            slot: 103, // slots 101, 102 skipped!
            parent_slot: 102,
            block_time: None,
            block_hash: "h103".into(),
            tx_count: 0,
            leader: None,
            rewards: vec![],
        })
        .unwrap();
    tracker
        .push_slot(SolanaSlot {
            slot: 104,
            parent_slot: 103,
            block_time: None,
            block_hash: "h104".into(),
            tx_count: 0,
            leader: None,
            rewards: vec![],
        })
        .unwrap();

    println!("Head slot: {:?}", tracker.head_slot());
    println!("Slot 101 skipped: {}", tracker.is_slot_skipped(101));
    println!("Slot 102 skipped: {}", tracker.is_slot_skipped(102));
    println!("Slot 103 skipped: {}", tracker.is_slot_skipped(103));

    let skipped = tracker.skipped_slots_in_range(100, 104);
    println!("Skipped slots in [100, 104]: {:?}", skipped);

    // 6. Decode program log to DecodedEvent
    println!("\n--- Event Decoder ---");
    if !parsed.is_empty() {
        let event =
            SolanaEventDecoder::decode_program_log(&parsed[0], 250_000_001, "SIG123abc", "solana");
        println!("DecodedEvent:");
        println!("  Chain:  {}", event.chain);
        println!("  Schema: {}", event.schema);
        println!("  Block:  {}", event.block_number);
        println!("  Tx:     {}", event.tx_hash);
        println!("  Fields: {}", event.fields_json);
    }

    println!("\nSolana indexer demo complete!");
}
