# CSDL Reference — ChainCodec Schema Definition Language

> CSDL is the human-readable YAML format for defining blockchain event schemas. Write once, decode in Rust, TypeScript, Python, and WASM.

---

## Minimal Example

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
```

---

## Full Schema Format

```yaml
schema <SchemaName>:           # required — unique name for this event type
  version: <u32>               # required — integer version, starts at 1
  description: "<string>"      # optional — human-readable description
  chains: [<slug>, ...]        # required — list of chain slugs this schema applies to
  address: "<hex>"             # optional — lock schema to a specific contract address
  event: <EventName>           # required — the Solidity event name
  fingerprint: "<hex>"         # required — the event's unique identifier (see below)
  supersedes: <u32>            # optional — version number this schema replaces
  superseded_by: <u32>         # optional — version number that replaces this one
  deprecated: true             # optional — mark schema as no longer in use
  fields:                      # required — ordered map of field name → field definition
    <field_name>:
      type: <CanonicalType>    # required — field type (see type reference below)
      indexed: true|false      # required for EVM — whether stored in topics vs data
      description: "<string>"  # optional — field documentation
  meta:                        # required — protocol metadata
    protocol: <string>         # optional — protocol identifier (e.g. "erc20", "uniswap-v3")
    category: <string>         # optional — event category (e.g. "token", "dex", "lending")
    verified: true|false       # optional — whether schema has been verified against on-chain data
    trust_level: <TrustLevel>  # optional — trust level (see below)
    tags: [<string>, ...]      # optional — arbitrary tags for search/filtering
    source_url: "<url>"        # optional — link to protocol documentation
    audited_by: "<string>"     # optional — auditor name if formally reviewed
```

---

## Multi-Document Files

A single `.csdl` file can contain multiple schemas separated by `---`:

```yaml
schema ERC20Transfer:
  version: 1
  chains: [ethereum, arbitrum]
  event: Transfer
  fingerprint: "0xddf252ad..."
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
---
schema ERC20Approval:
  version: 1
  chains: [ethereum, arbitrum]
  event: Approval
  fingerprint: "0x8c5be1e5..."
  fields:
    owner:   { type: address, indexed: true }
    spender: { type: address, indexed: true }
    value:   { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
    verified: true
    trust_level: maintainer_verified
```

Parse all schemas from a multi-document file:
```rust
let schemas = CsdlParser::parse_all(csdl_string)?;
// Returns Vec<Schema> — all documents in order
```

---

## Fingerprint

The fingerprint is the key used to route a raw event to its schema. It is always a hex string.

### EVM (Ethereum and compatible chains)

The fingerprint is `keccak256(event_signature)` — the value in `topics[0]`:

```
Transfer(address,address,uint256)
→ keccak256 →
0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef
```

You can compute it with:
```bash
# Using cast (Foundry)
cast keccak "Transfer(address,address,uint256)"

# Using Python
python3 -c "from eth_hash.auto import keccak; print('0x' + keccak(b'Transfer(address,address,uint256)').hex())"
```

### Solana / Anchor

The fingerprint is the first 8 bytes of `SHA-256("event:<EventName>")`:

```rust
let fp = SolanaDecoder::fingerprint_for("Transfer");
// fp.as_hex() == "0x" + hex(SHA256("event:Transfer")[:8])
```

### Cosmos / CosmWasm

The fingerprint is the first 16 bytes of `SHA-256("event:<type>/<action>")`:

```rust
let fp = CosmosDecoder::fingerprint_for("wasm/transfer");
// fp.as_hex() == "0x" + hex(SHA256("event:wasm/transfer")[:16])
```

---

## Type Reference

### Integer Types

| CSDL type | Description | NormalizedValue |
|-----------|-------------|-----------------|
| `uint8` | 0–255 | `Uint(u128)` |
| `uint16` | 0–65535 | `Uint(u128)` |
| `uint32` | 0–4294967295 | `Uint(u128)` |
| `uint64` | 0–18446744073709551615 | `Uint(u128)` |
| `uint128` | 0–340282366... | `Uint(u128)` |
| `uint160` | EVM internal | `BigUint(String)` |
| `uint256` | Large integers (EVM standard) | `BigUint(String)` |
| `int8` | -128 to 127 | `Int(i128)` |
| `int16` | -32768 to 32767 | `Int(i128)` |
| `int24` | Uniswap V3 tick | `Int(i128)` |
| `int32` | | `Int(i128)` |
| `int64` | | `Int(i128)` |
| `int128` | | `Int(i128)` |
| `int256` | Large signed (EVM) | `BigInt(String)` |

All unsigned types larger than `uint128` are stored as decimal strings in `BigUint`. This avoids silent overflow — you get the full value as a string for big-integer arithmetic.

### Address Types

| CSDL type | Description | NormalizedValue |
|-----------|-------------|-----------------|
| `address` | 20-byte EVM address, EIP-55 checksummed | `Address(String)` |
| `pubkey` | 32-byte Solana public key, base58 encoded | `Pubkey(String)` |
| `bech32address` | Cosmos bech32 address (e.g. `osmo1...`) | `Bech32(String)` |

### Byte Types

| CSDL type | Description | NormalizedValue |
|-----------|-------------|-----------------|
| `bytes` | Dynamic byte array | `Bytes(Vec<u8>)` |
| `bytes1` – `bytes32` | Fixed-length byte arrays | `Bytes(Vec<u8>)` |
| `hash256` | 32-byte hash (alias for bytes32) | `Hash256(String)` — `"0x" + hex` |

### Other Types

| CSDL type | Description | NormalizedValue |
|-----------|-------------|-----------------|
| `bool` | Boolean | `Bool(bool)` |
| `string` or `str` | UTF-8 string | `Str(String)` |
| `timestamp` | Unix timestamp (i64 seconds) | `Timestamp(i64)` |
| `decimal{decimals=N}` | Fixed-point number with N decimal places | `Uint(u128)` (raw value, divide by 10^N) |

### Collection Types

```yaml
# Fixed-length array
my_field: { type: "uint256[3]", indexed: false }

# Dynamic array
my_field: { type: "uint256[]", indexed: false }

# Tuple (struct)
my_field: { type: "(address,uint256)", indexed: false }
```

| CSDL type | NormalizedValue |
|-----------|-----------------|
| `T[N]` | `Array(Vec<NormalizedValue>)` |
| `T[]` | `Array(Vec<NormalizedValue>)` |
| `(T1,T2,...)` | `Tuple(Vec<(String, NormalizedValue)>)` |

---

## Chain Slugs

Use these slugs in the `chains` list:

| Chain | Slug |
|-------|------|
| Ethereum Mainnet | `ethereum` |
| Arbitrum One | `arbitrum` |
| Base | `base` |
| Polygon | `polygon` |
| Optimism | `optimism` |
| Avalanche C-Chain | `avalanche` |
| BNB Smart Chain | `bsc` |
| Solana Mainnet | `solana-mainnet` |
| Cosmos Hub | `cosmos` |
| Osmosis | `osmosis` |

Schemas may list multiple chains — the fingerprint is what actually routes the event:
```yaml
chains: [ethereum, arbitrum, polygon, base, optimism]
```

---

## Trust Levels

| Value | Meaning |
|-------|---------|
| `maintainer_verified` | Verified by ChainCodec maintainers against on-chain data |
| `community_verified` | Verified by community contributors |
| `protocol_provided` | Schema provided by the protocol team |
| `unverified` | Not yet verified (use with caution) |

---

## Indexed vs Non-Indexed Fields (EVM)

EVM ABI encoding stores indexed and non-indexed parameters differently:

| | Indexed (`indexed: true`) | Non-Indexed (`indexed: false`) |
|-|--------------------------|-------------------------------|
| Storage | One of `topics[1..3]` | Packed sequentially in `data` |
| Max count | 3 per event (topics[1], [2], [3]) | Unlimited |
| Gas | More gas to emit | Less gas |
| Filtering | `eth_getLogs` can filter by value | Cannot filter |

**Critical**: The CSDL field order must exactly match the Solidity event signature. ABI decoding is positional — wrong order = wrong values.

For `Transfer(address indexed from, address indexed to, uint256 value)`:
```yaml
fields:
  from:  { type: address, indexed: true }   # → topics[1]
  to:    { type: address, indexed: true }   # → topics[2]
  value: { type: uint256, indexed: false }  # → data[0..32]
```

For Solana and Cosmos, `indexed` is accepted but ignored — all fields come from the data payload.

---

## Schema Versioning

When an event's ABI changes, create a new schema version:

```yaml
# v1 — original
schema AaveV3Supply:
  version: 1
  superseded_by: 2
  deprecated: true
  fingerprint: "0xabc..."
  fields:
    reserve: { type: address, indexed: true }
    amount:  { type: uint256, indexed: false }
  meta: { protocol: aave-v3, category: lending, verified: true, trust_level: maintainer_verified }
---
# v2 — added referralCode
schema AaveV3Supply:
  version: 2
  supersedes: 1
  fingerprint: "0xdef..."   # new fingerprint if signature changed
  fields:
    reserve:      { type: address, indexed: true }
    onBehalfOf:   { type: address, indexed: true }
    amount:       { type: uint256, indexed: false }
    referralCode: { type: uint16,  indexed: true  }
  meta: { protocol: aave-v3, category: lending, verified: true, trust_level: maintainer_verified }
```

The registry automatically returns the latest non-deprecated version when `version: None` is requested:
```rust
let schema = registry.get_by_name("AaveV3Supply", None)?;
// Returns v2 (latest non-deprecated)

let schema_v1 = registry.get_by_name("AaveV3Supply", Some(1))?;
// Returns v1 explicitly
```

---

## Contract-Specific Schemas

Lock a schema to a specific contract address to avoid false matches when multiple contracts emit the same event signature:

```yaml
schema UniswapV3Swap:
  version: 1
  chains: [ethereum]
  address: "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"  # USDC/ETH 0.05% pool only
  event: Swap
  fingerprint: "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"
  ...
```

---

## Loading CSDL in Code

### Rust

```rust
use chaincodec_registry::{CsdlParser, MemoryRegistry};

// From string
let schemas = CsdlParser::parse_all(csdl_string)?;

// From file
let registry = MemoryRegistry::new();
registry.load_file(std::path::Path::new("./schemas/tokens/erc20.csdl"))?;

// From directory (recursively loads all .csdl files)
registry.load_directory(std::path::Path::new("./schemas"))?;
```

### CLI validation

```bash
# Parse and validate a schema file
chaincodec parse --file schemas/tokens/erc20.csdl

# Validate all schemas in a directory
chaincodec schemas validate --dir ./schemas
```

---

## Example: Writing a New Protocol Schema

Suppose you want to decode events from a hypothetical lending protocol called `LendingPool` with this Solidity event:

```solidity
event Deposit(
    address indexed depositor,
    address indexed asset,
    uint256 amount,
    uint256 shares
);
```

**Step 1:** Compute the fingerprint:
```bash
cast keccak "Deposit(address,address,uint256,uint256)"
# 0x5548c837ab068cf56a2105366d016042adee1cc6ea42927ea5c7384e0d24b225  (example)
```

**Step 2:** Write the CSDL:
```yaml
schema LendingPoolDeposit:
  version: 1
  chains: [ethereum]
  event: Deposit
  fingerprint: "0x5548c837ab068cf56a2105366d016042adee1cc6ea42927ea5c7384e0d24b225"
  fields:
    depositor: { type: address, indexed: true }
    asset:     { type: address, indexed: true }
    amount:    { type: uint256, indexed: false }
    shares:    { type: uint256, indexed: false }
  meta:
    protocol: lending-pool
    category: lending
    verified: false
    trust_level: unverified
```

**Step 3:** Load and decode:
```rust
let registry = MemoryRegistry::new();
registry.load_file(Path::new("./lending-pool.csdl"))?;

let decoder = EvmDecoder::new();
let fp = decoder.fingerprint(&raw_event);
let schema = registry.get_by_fingerprint(&fp).expect("schema not found");
let decoded = decoder.decode_event(&raw_event, &schema)?;

println!("depositor: {}", decoded.fields["depositor"]);  // EIP-55 address
println!("amount:    {}", decoded.fields["amount"]);     // decimal string
println!("shares:    {}", decoded.fields["shares"]);     // decimal string
```

---

## Bundled Schemas

ChainCodec ships schemas for 24 major protocols in `chaincodec/schemas/`:

```
schemas/
├── tokens/
│   ├── erc20.csdl          # Transfer, Approval
│   ├── erc721.csdl         # Transfer, Approval, ApprovalForAll
│   ├── erc1155.csdl        # TransferSingle, TransferBatch
│   ├── erc4626.csdl        # Deposit, Withdraw, Transfer
│   └── weth.csdl           # Deposit, Withdrawal
├── defi/
│   ├── uniswap-v2.csdl     # Swap, Mint, Burn, Sync
│   ├── uniswap-v3.csdl     # Swap, Mint, Burn, Flash, Collect
│   ├── aave-v3.csdl        # Supply, Borrow, Repay, LiquidationCall
│   ├── compound-v2.csdl    # Mint, Redeem, Borrow, RepayBorrow
│   ├── compound-v3.csdl    # Supply, Withdraw, AbsorbDebt
│   ├── curve.csdl          # TokenExchange, AddLiquidity, RemoveLiquidity
│   ├── balancer-v2.csdl    # Swap, PoolBalanceChanged
│   ├── maker.csdl          # Frob, Bite, LogNote
│   ├── lido.csdl           # Submitted, Transfer
│   ├── morpho.csdl         # Supply, Borrow, Repay, Liquidate
│   ├── pendle.csdl         # Swap, AddLiquidity
│   ├── gmx.csdl            # IncreasePosition, DecreasePosition
│   └── eigenlayer.csdl     # Deposit, WithdrawalQueued
├── nft/
│   ├── opensea-seaport.csdl  # OrderFulfilled
│   └── blur.csdl             # OrdersMatched
├── bridge/
│   ├── across.csdl           # FundsDeposited, FilledRelay
│   └── stargate.csdl         # Swap, SendMsg
└── governance/
    └── compound-governor.csdl  # ProposalCreated, VoteCast, ProposalExecuted
```
