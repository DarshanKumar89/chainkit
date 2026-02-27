# ChainCodec Examples — Complete Walkthrough

> 13 runnable Rust programs, one per feature area, mapped to real-world use cases.

All examples live in `chaincodec/examples/src/bin/`. Run any with:
```bash
cd chaincodec
cargo run --bin <name>
```

---

## Overview

| Binary | Use Case | Key APIs |
|--------|----------|----------|
| [`decode_erc20`](#1-decode_erc20) | Basic EVM decode | `EvmDecoder`, `MemoryRegistry`, `CsdlParser` |
| [`batch_decode`](#2-batch_decode) | Bulk historical processing | `BatchEngine`, `ErrorMode::Collect` |
| [`stream_demo`](#3-stream_demo) | Real-time live events | `StreamEngine`, `EvmWsListener` |
| [`fetch_and_decode`](#4-fetch_and_decode) | Auto-fetch ABI + decode | `AbiFetcher`, Sourcify/4byte.directory |
| [`decode_multiprotocol`](#5-decode_multiprotocol) | Multi-protocol in one batch | `BatchEngine`, ERC-20 + Uniswap V3 + Aave V3 |
| [`csdl_registry`](#6-csdl_registry) | Schema management | `CsdlParser`, `MemoryRegistry`, multi-doc YAML |
| [`decode_call`](#7-decode_call) | Function call decoding | `EvmCallDecoder` |
| [`encode_call`](#8-encode_call) | ABI encoding | `EvmEncoder`, roundtrip |
| [`proxy_detect`](#9-proxy_detect) | Proxy pattern detection | `classify_from_storage`, `detect_eip1167_clone` |
| [`eip712_decode`](#10-eip712_decode) | EIP-712 typed data | `Eip712Parser`, phishing detection |
| [`decode_solana`](#11-decode_solana) | Solana/Anchor events | `SolanaDecoder`, Borsh payload |
| [`decode_cosmos`](#12-decode_cosmos) | Cosmos/CosmWasm events | `CosmosDecoder`, ABCI attributes |
| [`with_observability`](#13-with_observability) | Metrics + logging | `ChainCodecMetrics`, `init_tracing` |

---

## 1. decode_erc20

**File:** [examples/src/bin/decode_erc20.rs](../examples/src/bin/decode_erc20.rs)
**Use case:** §1 Blockchain Indexer — the fundamental building block

Shows the minimal end-to-end decode: load a schema from inline CSDL, create an EVM decoder, build a `RawEvent` from a real USDC Transfer log, and decode it to typed fields.

```bash
cargo run --bin decode_erc20
```

**Expected output:**
```
ChainCodec — ERC-20 Transfer Decode
═══════════════════════════════════════════════════════

Schema loaded: ERC20Transfer v1
  fingerprint: 0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef
  chains: ethereum

─── Decoded Event ───────────────────────────────────
  schema:    ERC20Transfer
  version:   1
  chain:     ethereum
  block:     #19000000
  tx:        0xabc123
  address:   0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48

  Fields:
    from     = 0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
    to       = 0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
    value    = 1000000

✓ Decode complete — no field errors
```

**Key code pattern:**
```rust
let fp = decoder.fingerprint(&raw);                          // extract event fingerprint from topics[0]
let schema = registry.get_by_fingerprint(&fp).unwrap();     // O(1) schema lookup
let decoded = decoder.decode_event(&raw, &schema)?;         // ABI decode
println!("{}", decoded.fields["value"]);                    // NormalizedValue::BigUint("1000000")
```

---

## 2. batch_decode

**File:** [examples/src/bin/batch_decode.rs](../examples/src/bin/batch_decode.rs)
**Use case:** §2 DeFi Analytics, §1 Indexer — bulk historical processing

Shows `BatchEngine` decoding a large vector of events in parallel (Rayon) with a progress callback and `Collect` error mode to gather errors without aborting.

```bash
cargo run --bin batch_decode
```

**Expected output:**
```
ChainCodec — Batch Decode
═══════════════════════════════════════════════════════

Prepared 1000 raw events (900 valid ERC-20, 100 unknown fingerprint)

─── Batch Decode (ErrorMode::Collect) ───────────────
  Progress: 100/1000 decoded...
  Progress: 200/1000 decoded...
  ...
  Progress: 1000/1000 decoded...

─── Results ─────────────────────────────────────────
  total input:  1000
  decoded:      900
  errors:       0
  skipped:      100  (no schema for fingerprint)

  Sample decoded[0]:
    schema: ERC20Transfer
    value:  1000000
    from:   0xd8dA...
    to:     0xAb58...

✓ Batch decode complete
```

**Key code pattern:**
```rust
let mut engine = BatchEngine::new(Arc::new(registry));
engine.add_decoder("ethereum", Arc::new(EvmDecoder::new()) as Arc<dyn ChainDecoder>);

let result = engine.decode(
    BatchRequest::new("ethereum", raw_events)
        .error_mode(ErrorMode::Collect)
        .on_progress(|done, total| println!("  Progress: {done}/{total}"))
)?;

println!("{} decoded, {} errors", result.events.len(), result.errors.len());
```

---

## 3. stream_demo

**File:** [examples/src/bin/stream_demo.rs](../examples/src/bin/stream_demo.rs)
**Use case:** §7 Quantitative Trading, §1 Indexer — real-time WebSocket streaming

Connects to an Ethereum WebSocket node (`eth_subscribe("logs")`), decodes ERC-20 Transfer events as they arrive, and prints them in real time. Handles Ctrl-C gracefully.

> **Requires a live WebSocket RPC URL.** Set the `ETH_WS_URL` environment variable.

```bash
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY \
  cargo run --bin stream_demo
```

**Expected output:**
```
ChainCodec — Live ERC-20 Transfer Stream
═══════════════════════════════════════════════════════

Connecting to wss://...
Subscribed to eth_logs (filter: ERC20Transfer)

[block #21300145]  USDC  from=0xd8dA...  to=0x1234...  value=500000000
[block #21300145]  USDC  from=0x5678...  to=0xAb58...  value=1000000
[block #21300146]  USDT  from=0x9abc...  to=0xdef0...  value=50000000000

^C  Shutting down stream...
  Total decoded: 3 events in 8.2s
✓ Stream demo complete
```

**Key code pattern:**
```rust
let (engine, mut rx) = StreamEngine::new(config, registry, decoder).await?;
engine.start().await;

while let Ok(event) = rx.recv().await {
    println!("[block #{}]  {}  value={}", event.block_number, event.schema, event.fields["value"]);
}
```

---

## 4. fetch_and_decode

**File:** [examples/src/bin/fetch_and_decode.rs](../examples/src/bin/fetch_and_decode.rs)
**Use case:** §4 Security, §5 Wallet — ad-hoc decode of any contract without a pre-written schema

Shows `AbiFetcher` querying Sourcify (no API key) and falling back to 4byte.directory to fetch a contract's ABI automatically, then decode a transaction's calldata and events.

```bash
cargo run --bin fetch_and_decode
```

**Expected output:**
```
ChainCodec — Fetch ABI + Decode
═══════════════════════════════════════════════════════

Fetching ABI for 0xa0b86991... (USDC on Ethereum mainnet)
  ✓ ABI fetched from Sourcify (5 functions, 3 events)

Decoding Transfer event from tx 0xabc...
  schema:   ERC20Transfer (from fetched ABI)
  from:     0xd8dA...
  to:       0xAb58...
  value:    1000000

✓ Fetch-and-decode complete
```

**Key code pattern:**
```rust
let fetcher = AbiFetcher::new();
let schema = fetcher.fetch("0xa0b86991...", 1).await?;    // chain_id = 1 for Ethereum
let decoded = decoder.decode_event(&raw, &schema)?;
```

---

## 5. decode_multiprotocol

**File:** [examples/src/bin/decode_multiprotocol.rs](../examples/src/bin/decode_multiprotocol.rs)
**Use case:** §2 DeFi Analytics — decode ERC-20, Uniswap V3, and Aave V3 events in one batch

Three schemas (ERC-20 Transfer, Uniswap V3 Swap, Aave V3 Borrow) registered in one `MemoryRegistry`. Three events sent to `BatchEngine` in a single call. Output grouped by protocol.

```bash
cargo run --bin decode_multiprotocol
```

**Expected output:**
```
ChainCodec — Multi-Protocol Batch Decode
═══════════════════════════════════════════════════════

✓ Registry loaded: 3 schemas
  • ERC20Transfer v1 (erc20)
  • UniswapV3Swap v2 (uniswap-v3)
  • AaveV3Borrow v1 (aave-v3)

Fingerprint → Schema routing:
  ERC-20 Transfer    fp=0xddf252ad1be2c89b... → ERC20Transfer
  Uniswap V3 Swap    fp=0xc42079f94a6350d... → UniswapV3Swap
  Aave V3 Borrow     fp=0xb3d084820fb1a9d... → AaveV3Borrow

─── Batch Decode Results ────────────────────────────
  decoded: 3  errors: 0  skipped: 0

─── By Protocol ─────────────────────────────────────
  aave-v3:
    tx=0xccc333 block=#19500002
      amount           = 10000000
      borrowRate       = 1000
      interestRateMode = 2
      onBehalfOf       = 0xd8dA...
      referralCode     = 0
      reserve          = 0xa0b8...
      user             = 0xd8dA...
  erc20:
    tx=0xaaa111 block=#19500000
      from             = 0x28c6...
      to               = 0xd8dA...
      value            = 100000000000
  uniswap-v3:
    tx=0xbbb222 block=#19500001
      amount0          = -1
      amount1          = 1
      liquidity        = 1000000000000
      recipient        = 0xd8dA...
      sender           = 0xe592...
      sqrtPriceX96     = ...
      tick             = 100

✓ Multi-protocol batch decode complete
```

**Key insight:** One `BatchEngine` call handles 3 different protocols. No `if protocol == "erc20"` branching — the schema fingerprint routes each event to the right decoder automatically.

---

## 6. csdl_registry

**File:** [examples/src/bin/csdl_registry.rs](../examples/src/bin/csdl_registry.rs)
**Use case:** §3 Protocol SDK — schema authoring and registry operations

Shows `CsdlParser::parse_all()` parsing multiple schemas from a single YAML string (`---` separator), loading them into `MemoryRegistry`, looking up by fingerprint, listing all schemas, and inspecting individual field types.

```bash
cargo run --bin csdl_registry
```

**Expected output:**
```
ChainCodec — CSDL Parser + MemoryRegistry
═══════════════════════════════════════════════════════

✓ Parsed 3 schemas from multi-document YAML:
  • ERC20Transfer v1  [erc20]  fingerprint=0xddf252ad1be2c89b...
  • ERC20Approval v1  [erc20]  fingerprint=0x8c5be1e5ebec7d5b...
  • UniswapV3Swap v2  [uniswap-v3]  fingerprint=0xc42079f94a6350...

✓ Registry contains 3 schemas

─── Fingerprint Lookup ──────────────────────────────
  ERC20Transfer   → ERC20Transfer v1  (protocol: erc20)
  UniswapV3Swap   → UniswapV3Swap v2  (protocol: uniswap-v3)
  UnknownEvent    → NOT FOUND

─── All Schemas in Registry ─────────────────────────
  ERC20Approval        v1  chains=[ethereum,polygon,arbitrum,base,optimism]  fields=3
  ERC20Transfer        v1  chains=[ethereum,polygon,arbitrum,base,optimism]  fields=3
  UniswapV3Swap        v2  chains=[ethereum,arbitrum,polygon,base,optimism]  fields=7

─── Schema Version Evolution ────────────────────────
  Adding AaveV3Supply v1
  Registry now has 4 schemas

─── Field Inspection: ERC20Transfer ─────────────────
  event:     Transfer
  fields:
    from         Address    indexed=true
    to           Address    indexed=true
    value        Uint(256)  indexed=false
  chains:    [ethereum, polygon, arbitrum, base, optimism]
  verified:  true
  trust:     maintainer_verified

─── Duplicate Detection ─────────────────────────────
  ✓ duplicate fingerprint rejected: already exists: ERC20Transfer v1
  ✓ duplicate fingerprint rejected: already exists: ERC20Approval v1
  ✓ duplicate fingerprint rejected: already exists: UniswapV3Swap v2

✓ CSDL registry examples complete
```

**Key insight:** `---` separates multiple schemas in one file. `registry.all_schemas()` returns the latest non-deprecated version of each schema. Duplicate `add()` is safely rejected.

---

## 7. decode_call

**File:** [examples/src/bin/decode_call.rs](../examples/src/bin/decode_call.rs)
**Use case:** §4 Security — decode transaction calldata to see what a transaction was trying to do

Shows `EvmCallDecoder` decoding raw calldata for `transfer()`, `approve()` (detecting max-approval amounts), and `transferFrom()` using a standard ABI JSON.

```bash
cargo run --bin decode_call
```

**Expected output:**
```
ChainCodec — EVM Function Call Decoder
═══════════════════════════════════════════════════════

Available functions (from ABI):
  transfer(address,uint256)       selector=0xa9059cbb
  approve(address,uint256)        selector=0x095ea7b3
  transferFrom(address,address,uint256)  selector=0x23b872dd

─── transfer(0xAb58..., 1000000) ────────────────────
  function:  transfer
  selector:  0xa9059cbb
  to:        0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
  amount:    1000000

─── approve(0x1f98..., MAX) ─────────────────────────
  function:  approve
  selector:  0x095ea7b3
  spender:   0x1F98431c8aD98523631AE4a59f267346ea31F984
  amount:    115792089237316195423570985008687907853269984665640564039457584007913129639935
  ⚠  MAX APPROVAL — unlimited spend authorized

─── transferFrom(0xd8dA..., 0xAb58..., 500000) ──────
  function:  transferFrom
  selector:  0x23b872dd
  from:      0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045
  to:        0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
  amount:    500000

✓ Call decode complete
```

**Key code pattern:**
```rust
let decoder = EvmCallDecoder::from_abi_json(ERC20_ABI)?;
let calldata = build_transfer_calldata(...);
let decoded = decoder.decode_call(&calldata, None)?;
println!("function: {}", decoded.function_name);
println!("to:       {}", decoded.inputs[0].1);   // (name, NormalizedValue)
println!("amount:   {}", decoded.inputs[1].1);
```

---

## 8. encode_call

**File:** [examples/src/bin/encode_call.rs](../examples/src/bin/encode_call.rs)
**Use case:** §5 Wallet — build transaction calldata programmatically, then verify by decoding

Shows `EvmEncoder` encoding a `transfer()` call from `NormalizedValue` arguments, then round-tripping through `EvmCallDecoder` to confirm the encoded bytes decode back to the same values.

```bash
cargo run --bin encode_call
```

**Expected output:**
```
ChainCodec — EVM Function Call Encoder
═══════════════════════════════════════════════════════

─── Encoding transfer(to, amount) ───────────────────
  to:      0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
  amount:  1000000

  Encoded calldata (68 bytes):
  a9059cbb
  000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b
  00000000000000000000000000000000000000000000000000000000000f4240

─── Roundtrip: decode what we just encoded ──────────
  function: transfer
  to:       0xAb5801a7D398351b8bE11C439e05C5B3259aeC9B
  amount:   1000000
  ✓ Values match original inputs

─── Encoding approve(spender, MAX_UINT256) ───────────
  Encoded calldata (68 bytes): 095ea7b3...
  Decoded amount: 115792089237316195423570985008687907853...

✓ Encode → decode roundtrip complete
```

**Key code pattern:**
```rust
let encoder = EvmEncoder::from_abi_json(ERC20_ABI)?;
let calldata = encoder.encode_call("transfer", &[
    NormalizedValue::Address("0xAb5801...".into()),
    NormalizedValue::Uint(1_000_000),
])?;
// calldata is now Vec<u8> — selector (4 bytes) + ABI-encoded args
```

---

## 9. proxy_detect

**File:** [examples/src/bin/proxy_detect.rs](../examples/src/bin/proxy_detect.rs)
**Use case:** §4 Security — detect whether a contract is a proxy before trying to decode its events

About 60% of major DeFi contracts are behind proxies (USDC, Aave pools, etc.). This example shows all four proxy detection patterns supported by ChainCodec.

```bash
cargo run --bin proxy_detect
```

**Expected output:**
```
ChainCodec — Proxy Detection
═══════════════════════════════════════════════════════

Proxy detection storage slots:
  EIP-1967 impl slot:   0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc
  EIP-1967 beacon slot: 0xa3f0ad74e5423aebfd80d3ef4346578335a9a72aeaee59ff6cb3582b35133d50
  UUPS proxiableUUID:   0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc

─── EIP-1967 Logic Proxy (e.g. USDC) ───────────────────
  kind:            LogicProxy (EIP-1967)
  proxy:           0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
  implementation:  0x43506849d7c04f9138d1a2050bbf3a0c054402dd
  ✓ Use implementation ABI to decode events

─── EIP-1967 Beacon Proxy ──────────────────────────────
  kind:  BeaconProxy (EIP-1967)
  proxy: 0xBeacon...
  beacon: 0x...
  ✓ Fetch beacon's implementation() to resolve ABI

─── EIP-1822 UUPS Proxy ─────────────────────────────────
  kind:           UUPS (EIP-1822)
  proxy:          0xUUPS...
  implementation: 0xImpl...

─── EIP-1167 Minimal Proxy (Clone) ─────────────────────
  Bytecode prefix detected: 0x363d3d37...
  kind:            MinimalProxy (EIP-1167)
  implementation:  0xMaster...
  ✓ Clone factory pattern — use master contract ABI

─── Unknown / Not a proxy ───────────────────────────────
  kind:  NotAProxy
  proxy: 0xDirect...
  ✓ Use this contract's own ABI directly

✓ Proxy detection complete
```

**Key code pattern:**
```rust
let (impl_slot, beacon_slot, uups_slot) = proxy_detection_slots();

// Simulate RPC storage slot reads, then classify:
let info = classify_from_storage(
    proxy_address,
    Some(impl_slot_value),   // eth_getStorageAt(addr, EIP1967_IMPL_SLOT)
    None,                    // beacon slot value
    None,                    // UUPS slot value
);

match info.kind {
    ProxyKind::LogicProxy => println!("impl: {}", info.implementation.unwrap()),
    ProxyKind::MinimalProxy => { /* detect_eip1167_clone(&bytecode) */ }
    ProxyKind::NotAProxy => println!("use contract's own ABI"),
    _ => {}
}
```

---

## 10. eip712_decode

**File:** [examples/src/bin/eip712_decode.rs](../examples/src/bin/eip712_decode.rs)
**Use case:** §4 Security, §5 Wallet — parse `eth_signTypedData_v4` payloads to show users what they're signing

Shows `Eip712Parser` parsing the canonical Mail example and a Uniswap Permit2 signature, demonstrating how wallets can display typed data and how security tools detect phishing.

```bash
cargo run --bin eip712_decode
```

**Expected output:**
```
ChainCodec — EIP-712 Typed Data Decoder
═══════════════════════════════════════════════════════

─── Mail (EIP-712 spec example) ─────────────────────
  Domain:       Ether Mail
  Primary type: Mail

  Defined types: Mail, Person

  Fields (Mail):
    from     → Person (object)
    to       → Person (object)
    contents → string

  Message values:
    from.name    = "Cow"
    from.wallet  = "0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826"
    to.name      = "Bob"
    to.wallet    = "0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB"
    contents     = "Hello, Bob!"

  Domain separator (hex):
    0xf2cee375fa42b42143804025fc449deafd50cc031ca257e0b194a650a912090f

─── Permit2 (phishing detection) ────────────────────
  ⚠  High-risk signature type detected: PermitTransferFrom
  Spender:   0x1234...malicious contract...
  Token:     0xa0b8...USDC...
  Amount:    1000000000000  (potentially ALL your USDC)
  Deadline:  2099-01-01 (far future — no expiry effectively)

  ✓ Wallet should warn: "This gives unlimited USDC to an unknown contract"

✓ EIP-712 decode complete
```

**Key code pattern:**
```rust
let parsed = Eip712Parser::parse(json_string)?;
println!("primary type: {}", parsed.primary_type);

// Inspect the type structure
for field in Eip712Parser::primary_type_fields(&parsed) {
    println!("{}: {}", field.name, field.ty);
}

// Read the domain separator (for wallet display)
let domain_sep = Eip712Parser::domain_separator_hex(&parsed);

// Detect dangerous pattern
if parsed.primary_type == "PermitTransferFrom" {
    println!("⚠  High-risk: unlimited token approval signature");
}
```

---

## 11. decode_solana

**File:** [examples/src/bin/decode_solana.rs](../examples/src/bin/decode_solana.rs)
**Use case:** §6 Cross-chain — decode Solana Anchor program events

Shows `SolanaDecoder` computing an Anchor discriminator (`SHA-256("event:AnchorTransfer")[..8]`), building a Borsh-encoded payload with pubkey + u64 fields, and decoding to `NormalizedValue::Pubkey` and `NormalizedValue::Uint`.

```bash
cargo run --bin decode_solana
```

**Expected output:**
```
ChainCodec — Solana/Anchor Decoder
═══════════════════════════════════════════════════════

Anchor discriminator for 'AnchorTransfer':
  SHA-256('event:AnchorTransfer')[..8] = 0x3b9aca0000000000
  schema registered: AnchorTransfer v1

  Borsh payload: 72 bytes (32 + 32 + 8)
  from (pubkey):  1111111111111111  (first 8 bytes of 0x11...11)
  to   (pubkey):  2222222222222222  (first 8 bytes of 0x22...22)
  amount (u64 LE): 5000000 lamports

  Fingerprint from raw event: 0x3b9aca0000000000
  Matched schema: AnchorTransfer v1

─── Decoded Solana Event ────────────────────────────
  schema:  AnchorTransfer
  chain:   solana-mainnet
  block:   #250000000
  program: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA

  amount   = 5000000
  from     = 11111111111111111111111111111111  (base58 pubkey)
  to       = 22222222222222222222222222222222  (base58 pubkey)

✓ All fields decoded (NormalizedValue — same type as EVM output)

─── Cross-chain type parity ─────────────────────────
  Solana Pubkey  → NormalizedValue::Pubkey(base58_string)
  Solana u64     → NormalizedValue::Uint(u128)
  EVM address    → NormalizedValue::Address(hex_checksummed)
  EVM uint256    → NormalizedValue::BigUint(decimal_string)
  ─── same consumer handles both ───────────────────────
  decoded.fields['amount'] works identically for both chains

✓ Solana decode complete
```

**Key code pattern:**
```rust
// Compute the Anchor discriminator for a given event name:
let fp = SolanaDecoder::fingerprint_for("AnchorTransfer");
// fp.as_hex() == SHA-256("event:AnchorTransfer")[..8] as hex

// Build Borsh payload manually (or from your Anchor program):
let mut payload = Vec::new();
payload.extend_from_slice(&from_pubkey);      // 32 bytes
payload.extend_from_slice(&to_pubkey);        // 32 bytes
payload.extend_from_slice(&amount.to_le_bytes()); // 8 bytes LE

// Create RawEvent with topics[0] = discriminator
let raw = RawEvent {
    chain: chains::solana_mainnet(),
    topics: vec![fp.as_hex().to_string()],
    data: payload,
    ..
};

let decoded = SolanaDecoder::new().decode_event(&raw, &schema)?;
// decoded.fields["from"] = NormalizedValue::Pubkey("111...111")
// decoded.fields["amount"] = NormalizedValue::Uint(5_000_000)
```

---

## 12. decode_cosmos

**File:** [examples/src/bin/decode_cosmos.rs](../examples/src/bin/decode_cosmos.rs)
**Use case:** §6 Cross-chain — decode CosmWasm ABCI events from Osmosis, Cosmos Hub, Neutron, etc.

Shows `CosmosDecoder` computing a fingerprint for `wasm/transfer` and `wasm/token_swapped`, building events with JSON attribute lists, and decoding `bech32address`, `uint128`, and `str` fields.

```bash
cargo run --bin decode_cosmos
```

**Expected output:**
```
ChainCodec — Cosmos/CosmWasm Decoder
═══════════════════════════════════════════════════════

Fingerprints:
  wasm/transfer      → 0xa3b1c2d4e5f60718
  wasm/token_swapped → 0x9f8e7d6c5b4a3918
  ✓ 2 schemas registered

─── CW-20 Transfer Event ────────────────────────────
  fingerprint: 0xa3b1c2d4e5f60718
  schema:      Cw20Transfer v1
  amount   = 1000000
  contract = osmo1qwerty1234567890abcdef
  from     = osmo1aabbccddeeff00112233445566778899aabbcc
  to       = osmo1ffeeddccbbaa99887766554433221100ffeedd
  ✓ clean decode

─── Osmosis Swap Event ──────────────────────────────
  schema: OsmosisSwap v1
  pool_id    = 1
  sender     = osmo1aabbccddeeff00112233445566778899aabbcc
  tokens_in  = 1000000uosmo
  tokens_out = 500000uatom

─── Cross-chain output comparison ───────────────────
  EVM    ERC-20 Transfer → fields['value']  = NormalizedValue::Uint(1000000)
  Cosmos CW-20 Transfer  → fields['amount'] = NormalizedValue::Uint(1000000)
  Solana SPL Token xfer  → fields['amount'] = NormalizedValue::Uint(1000000)
  ─── same downstream consumer for all three ───────────────────────

✓ Cosmos/CosmWasm decode complete
```

**Key code pattern:**
```rust
// Cosmos events have ABCI attributes as JSON key/value pairs
let attrs = serde_json::json!([
    {"key": "amount", "value": "1000000"},
    {"key": "from",   "value": "osmo1aabb..."},
    {"key": "to",     "value": "osmo1ffee..."},
]);

let raw = RawEvent {
    chain: ChainId::cosmos("osmosis"),
    topics: vec!["wasm".into(), "transfer".into()],  // [event_type, action]
    data: serde_json::to_vec(&attrs)?,               // JSON-encoded attributes
    ..
};

// CosmosDecoder uses topics[0]+"/"+topics[1] as the fingerprint key
let fp = CosmosDecoder::new().fingerprint(&raw);
let decoded = CosmosDecoder::new().decode_event(&raw, &schema)?;
// Cosmos "1000000" string → NormalizedValue::Uint(1000000)
```

---

## 13. with_observability

**File:** [examples/src/bin/with_observability.rs](../examples/src/bin/with_observability.rs)
**Use case:** §8 Node/RPC Ops — production monitoring with OpenTelemetry metrics and structured logging

Shows `ChainCodecMetrics` (counters and histograms) and `init_tracing` (JSON or human-readable structured logs), configured with per-component log levels. In production, connect an OTLP exporter to send metrics to Prometheus/Grafana.

```bash
cargo run --bin with_observability

# With JSON logs (ELK/Loki/CloudWatch compatible):
LOG_JSON=1 cargo run --bin with_observability

# With debug-level logging for the EVM crate:
RUST_LOG=info,chaincodec_evm=debug cargo run --bin with_observability
```

**Expected output:**
```
ChainCodec — Observability Demo
═══════════════════════════════════════════════════════
  (structured logs are emitted alongside this output)

  Metrics registered:
    chaincodec.events_decoded   (counter, chain + schema tags)
    chaincodec.events_skipped   (counter, chain + reason tags)
    chaincodec.decode_errors    (counter, chain + error_type tags)
    chaincodec.decode_latency_ms (histogram, chain tag)
    chaincodec.batch_size        (histogram)
    chaincodec.schema_cache_hits (counter)

─── Decode Loop with Metrics ────────────────────────
  [OK]   tx=0xabc001   schema=ERC20Transfer value=1000000000  latency=0.041ms
  [OK]   tx=0xabc002   schema=ERC20Transfer value=400000000   latency=0.018ms
  [SKIP] tx=0xabc003   — no schema for fingerprint

─── Metric Summary (counters recorded) ──────────────
  chaincodec.events_decoded   = 2
  chaincodec.events_skipped   = 1
  chaincodec.decode_errors    = 0

  (in production: export via OTLP to Prometheus/Grafana)
  (Grafana panel: rate(chaincodec_events_decoded_total[1m]))

─── Log config used ─────────────────────────────────
  global level:     info
  chaincodec_evm:   debug  (verbose field tracing)
  chaincodec_registry: warn (suppress hit/miss noise)
  JSON logs:        false
  (set LOG_JSON=1 for ELK/Loki/CloudWatch compatible output)

✓ Observability demo complete
```

**Key code pattern:**
```rust
// Initialize structured logging
let log_config = LogConfig {
    level: "info".into(),
    components: [
        ("chaincodec_evm".into(), "debug".into()),
    ].into(),
    json: std::env::var("LOG_JSON").is_ok(),
};
init_tracing(&log_config);

// Create metrics
let meter = opentelemetry::global::meter("my-service");
let metrics = ChainCodecMetrics::new(&meter);

// Record per-decode:
metrics.record_decoded("ethereum", "ERC20Transfer");
metrics.record_latency(latency_ms, "ethereum");

// Record skips and errors:
metrics.events_skipped.add(1, &[
    KeyValue::new("chain", "ethereum"),
    KeyValue::new("reason", "schema_not_found"),
]);
metrics.record_error("ethereum", "field_decode_error");

// Record batch metrics:
metrics.batch_size.record(event_count as u64, &[]);
metrics.schema_cache_hits.add(hits, &[]);
```

**Grafana dashboard queries (after OTLP setup):**
```promql
# Events decoded per second, by schema
rate(chaincodec_events_decoded_total[1m])

# Error rate (should be near 0)
rate(chaincodec_decode_errors_total[5m])

# p99 decode latency
histogram_quantile(0.99, chaincodec_decode_latency_ms_bucket)
```

---

## Running All Examples

```bash
cd chaincodec

# Run them all in sequence:
for bin in decode_erc20 batch_decode fetch_and_decode decode_multiprotocol \
           csdl_registry decode_call encode_call proxy_detect eip712_decode \
           decode_solana decode_cosmos with_observability; do
  echo "─── $bin ───"
  cargo run --bin $bin 2>&1
  echo
done

# stream_demo requires a live WebSocket endpoint:
ETH_WS_URL=wss://your-node/ws cargo run --bin stream_demo
```
