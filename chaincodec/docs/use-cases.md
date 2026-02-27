# ChainCodec — Use Cases for Developers & Blockchain Experts

> Who uses ChainCodec, what they build with it, and why it replaces hand-rolled ABI decoders.

---

## Quick Reference

| If you're building... | Use ChainCodec for... |
|----------------------|----------------------|
| Blockchain indexer | `BatchEngine` + `StreamEngine` for all event decode |
| DeFi analytics | Batch decode historical logs, typed output for data warehouse |
| Protocol SDK | CSDL schema → auto-generate TypeScript/Python decoders via bindings |
| Security tooling | CLI for ad-hoc decode, `detect-proxy` for upgrade tracking |
| Web3 wallet / frontend | WASM binding for client-side decode, no backend needed |
| Cross-chain app | EVM + Solana + Cosmos decoders all produce `NormalizedValue` |
| Trading infrastructure | Sub-millisecond event decode via `StreamEngine` |
| On-chain monitoring | `StreamEngine` + schema filtering for targeted alerts |
| Node/RPC ops | `chaincodec test` for conformance, observability metrics |
| Education / tooling | WASM binding for interactive decode demos |

---

## 1. Blockchain Indexer Developers

**The problem today**: Every team building an indexer writes their own ABI decoder. It's boilerplate reimplemented thousands of times — usually wrong for edge cases like proxy contracts, reference-type topics, or large integers.

**What ChainCodec replaces**:

```rust
// Before — what every team writes manually, per protocol, forever:
fn decode_transfer(log: &Log) -> (Address, Address, U256) {
    let from = Address::from_slice(&log.topics[1][12..]);
    let to   = Address::from_slice(&log.topics[2][12..]);
    let val  = U256::from_big_endian(&log.data[..32]);
    (from, to, val)
}
```

```rust
// With ChainCodec — one decoder, all protocols:
let decoded = decoder.decode_event(&raw, &schema)?;
// decoded.fields["from"], decoded.fields["to"], decoded.fields["value"]
```

**Projects to build:**

- **Custom protocol indexer** — index Uniswap V3 positions, Aave borrows, Lido stakes without writing per-event decoders
- **Multi-protocol dashboard backend** — one `BatchEngine` call decodes 20 different event types without branching on protocol
- **Reorg-safe historical backfill** — feed archive node logs into `BatchEngine` at >1M events/sec
- **Event pipeline to Postgres/ClickHouse** — schema-tagged `DecodedEvent` maps directly to typed table columns

---

## 2. DeFi Analytics & Data Engineers

**The problem**: Raw blockchain data in BigQuery/Dune/ClickHouse is useless without decoding. Most teams maintain fragile Python scripts or SQL UDFs to decode specific protocol events that break on every upgrade.

**What ChainCodec gives you**: `NormalizedValue` typed output with `protocol` and `category` metadata attached to every decoded event — ready for Parquet/JSON column mapping.

**Projects to build:**

- **DEX aggregator** — pull Uniswap V2+V3, Curve, Balancer swaps into one normalized table, compute volume and slippage across all venues
- **DeFi risk dashboard** — decode Aave Borrow/Repay/Liquidation events to compute protocol-level health factors in real time
- **Yield tracker** — ERC-4626 Deposit/Withdraw events across all vaults, normalized to `assets_in` / `shares_out`
- **NFT sales analytics** — OpenSea and Blur `OrderFulfilled` events decoded to `price`, `token_id`, `collection`
- **Stablecoin flow monitor** — ERC-20 Transfer events for USDC/USDT/DAI to track flows between protocols and wallets
- **MEV analysis** — decode sandwich attack patterns from Uniswap V3 Swap events around targeted transactions
- **Governance tracker** — Compound Governor Bravo `VoteCast` / `ProposalCreated` events decoded to voter, support, votes

---

## 3. Protocol SDK Authors (DeFi Teams Shipping SDKs)

**The problem**: Every DeFi protocol shipping a developer SDK needs event parsing in TypeScript, Python, and Rust separately. Three codebases, three sets of bugs, tripled maintenance cost.

**What ChainCodec gives you**: Write the schema in CSDL once. All three language bindings (napi-rs, PyO3, wasm-bindgen) are generated from the same Rust decoder.

**Projects to build:**

- **Protocol TypeScript SDK** — ship `npm install @yourprotocol/sdk` where event decoding internally uses `@chainkit/chaincodec`. No hand-written ABI decoder maintained per language
- **Protocol Python SDK** — `pip install yourprotocol` exposes `decode_supply_event(log)` → typed Python dict, backed by the same Rust decoder as the TypeScript version
- **On-chain notification service** — "alert me when address X is liquidated on Aave" — use `StreamEngine` with the Aave `LiquidationCall` schema to subscribe and react in real time
- **Protocol health bot** — Telegram/Discord bot that decodes governance votes, large borrows, or staking events and posts human-readable summaries

---

## 4. Blockchain Security Researchers & Auditors

**The problem**: When a hack happens, researchers need to rapidly decode every transaction in the attack across multiple protocols simultaneously. Existing tools are protocol-specific; cross-protocol attacks are slow to analyze.

**What ChainCodec gives you**:
- `chaincodec decode-log` — decode any EVM log from the CLI in seconds
- `chaincodec detect-proxy` — reveal whether a contract is a proxy and what implementation it delegates to
- `chaincodec decode-call` — see exactly what calldata a suspicious transaction submitted
- `BatchEngine` — decode an entire attack block range at >1M events/sec

**Projects to build:**

- **Live attack monitor** — subscribe to `Transfer`, `Withdrawal`, `Swap` events across major protocols via `StreamEngine`, flag anomalous amounts (e.g. flash loan + large withdrawal in same block)
- **Exploit replay tool** — decode original exploit calldata with `EvmCallDecoder`, re-encode with `EvmEncoder` with modified parameters for PoC testing
- **Proxy upgrade watcher** — monitor `Upgraded(address)` events (EIP-1967) across all major contracts, alert when an implementation changes unexpectedly
- **Rug pull detector** — monitor `Approval` events where a new address gets max allowance from a large number of holders in a short window
- **Post-mortem timeline builder** — batch decode all events from exploit block range, produce human-readable timeline:
  ```
  block 19423001: Borrow 1,000,000 USDC from Aave
  block 19423001: Swap 1,000,000 USDC → 523 ETH on Uniswap V3
  block 19423001: Withdraw 523 ETH via malicious callback
  ```
- **Permit2 abuse detector** — decode EIP-712 `PermitTransferFrom` signed messages to identify phishing attempts using `Eip712Parser`

---

## 5. Wallet & Web3 Frontend Developers

**The problem**: Transaction history UIs display raw data. Decoding it for a clean UI requires ABI fetching + decoding per transaction — typically done by calling a centralized API (Etherscan, Alchemy), creating a dependency and rate-limit risk.

**What ChainCodec gives you**: The WASM binding (`@chainkit/chaincodec-wasm`) runs entirely in the browser — no backend required.

```javascript
import init, { EvmDecoder, MemoryRegistry } from '@chainkit/chaincodec-wasm';
await init();

const registry = new MemoryRegistry();
registry.loadCsdl(erc20CsdlString);

const decoder = new EvmDecoder();
const event = JSON.parse(decoder.decodeEventJson(JSON.stringify(rawLog), registry));
// event.fields.from, event.fields.to, event.fields.value — all in the browser
```

**Projects to build:**

- **Client-side transaction decoder** — paste a tx hash, see `transfer(to=0xAb58..., amount=1,000,000 USDC)` without any backend API call
- **Portfolio tracker** — decode ERC-20/ERC-721 Transfer events from address history to build holdings over time, entirely client-side
- **DeFi position viewer** — decode Aave Borrow/Supply and Uniswap V3 Mint/Burn events to reconstruct open positions without a centralized indexer
- **Smart contract explorer** — browser tool that fetches ABI from Sourcify via `AbiFetcher` and decodes all logs in a transaction receipt
- **Gas debugger** — decode calldata of failed transactions to show users what they were trying to do and why it reverted

---

## 6. Cross-Chain Infrastructure Developers

**The problem**: EVM uses ABI encoding. Solana uses Borsh. Cosmos uses JSON attribute lists. Building one system that processes events from all three requires three separate decoders producing incompatible types.

**What ChainCodec gives you**: All three decoders (`EvmDecoder`, `SolanaDecoder`, `CosmosDecoder`) produce the same `NormalizedValue` type. The downstream consumer never branches on chain family.

```rust
// One consumer, three chains:
let decoded = match raw.chain.family {
    ChainFamily::Evm    => evm_decoder.decode_event(&raw, &schema)?,
    ChainFamily::Solana => sol_decoder.decode_event(&raw, &schema)?,
    ChainFamily::Cosmos => cos_decoder.decode_event(&raw, &schema)?,
};
// decoded.fields["amount"] — same type regardless of source chain
```

**Projects to build:**

- **Cross-chain bridge monitor** — decode Stargate/Across events on Ethereum and corresponding receive events on destination chains, in one unified pipeline
- **Multi-chain DEX aggregator** — Uniswap/Curve swaps (EVM) + Orca/Phoenix swaps (Solana/Anchor) + Osmosis swaps (CosmWasm) → single normalized trade feed
- **Cross-chain stablecoin tracker** — USDC on Ethereum (ERC-20 Transfer) + native USDC on Solana (SPL via Anchor) + axlUSDC on Osmosis (CosmWasm transfer) → one table
- **Unified wallet history** — show a user their complete transaction history across Ethereum, Solana, and Cosmos with consistent field names and types

---

## 7. Quantitative Traders & Algo Trading Firms

**The problem**: Trading strategies using on-chain signals need to process events with millisecond latency. Python-based decoders are too slow. Custom Rust decoders take months to build.

**What ChainCodec gives you**: `StreamEngine` with `EvmWsListener` delivers decoded events as fast as the WebSocket connection allows. Rayon batch decode for signal generation over historical data.

**Projects to build:**

- **Order flow signal** — decode Uniswap V3 Swap events in real time, compute price impact and direction, feed into a trading model
- **Liquidation opportunity monitor** — decode Aave `BorrowAllowed` + price oracle `AnswerUpdated` events in real time to anticipate liquidation opportunities before they appear in mempools
- **Arbitrage detector** — parallel batch decode of swaps across Uniswap V2/V3, Curve, and Balancer in the same block to identify cross-venue price dislocations
- **Large transfer alert** — subscribe to ERC-20 Transfer events on USDC/WETH, filter for amounts >$10M, trigger automated action
- **Whale tracker** — decode all ERC-20 and DEX events for a watchlist of addresses, build a real-time position delta feed

---

## 8. Node / RPC Infrastructure Operators

**The problem**: After client upgrades or hard forks, operators need to verify that the events their node is returning decode correctly against known schemas.

**What ChainCodec gives you**: `chaincodec test` runs golden fixtures against any RPC endpoint. `chaincodec-observability` provides OpenTelemetry metrics out of the box.

**Projects to build:**

- **Node conformance test suite** — run `chaincodec test --fixtures ./fixtures --schema-dir ./schemas` against your RPC after every upgrade to verify log decode integrity
- **Protocol upgrade detector** — compare decoded event fields across schema versions to catch when a contract starts emitting new or renamed fields after an upgrade
- **Grafana dashboard** — `chaincodec.events_decoded` by chain + schema, `chaincodec.decode_errors` by error type, `chaincodec.decode_latency_ms` histogram — all out of the box via OpenTelemetry

---

## 9. Blockchain Educators & Tooling Builders

**The problem**: Learning ABI encoding is a painful prerequisite for anyone building blockchain tooling. Interactive tools that show the relationship between raw bytes and decoded values don't exist.

**What ChainCodec gives you**: The WASM binding makes it trivial to build browser-based interactive decode tools.

**Projects to build:**

- **Interactive ABI decoder** — web tool where users paste raw log topics + data and see decoded fields with type annotations explaining exactly how each byte maps to a value
- **Ethereum event explorer** — educational tool showing how `keccak256("Transfer(address,address,uint256)")` becomes `topics[0]` and how indexed vs non-indexed fields are stored differently
- **Protocol comparison tool** — side-by-side decoded events from Uniswap V2 vs V3 vs Curve to show how different swap implementations emit different data structures
- **Bootcamp project starter** — give students a working decode pipeline on day one so they can focus on business logic instead of ABI boilerplate

---

## One-Line Summary Per Audience

| Audience | What ChainCodec means for them |
|----------|-------------------------------|
| Backend Rust developer | Stop writing ABI decoders. One library, all chains, all protocols. |
| TypeScript developer | npm package that turns raw log bytes into typed objects. |
| Python / data engineer | pip package + >1M/sec batch decode for data warehouse pipelines. |
| DeFi protocol team | Write your event schema in YAML once, get decoders in 4 languages. |
| Security researcher | CLI that decodes any EVM log or calldata in one command, with proxy resolution. |
| Quant / algo trader | Real-time decoded event stream, <5ms latency, typed fields ready for signal processing. |
| Web3 frontend dev | Client-side WASM decoder — no backend API dependency for tx decoding. |
| Cross-chain builder | EVM + Solana + Cosmos all produce the same `NormalizedValue` output type. |
