# @chainfoundry/chaincodec

Universal blockchain ABI decoder for Node.js — production-grade EVM event & function call decoding.

[![npm](https://img.shields.io/npm/v/@chainfoundry/chaincodec)](https://www.npmjs.com/package/@chainfoundry/chaincodec)
[![license](https://img.shields.io/npm/l/@chainfoundry/chaincodec)](LICENSE)

Native Node.js bindings (via [napi-rs](https://napi.rs)) for the [chaincodec](https://crates.io/crates/chaincodec-evm) Rust library. Decode `eth_getLogs` entries, function calldata, and compute topic0 fingerprints — all at Rust speed with a TypeScript-first API.

---

## Install

```bash
npm install @chainfoundry/chaincodec
# or
yarn add @chainfoundry/chaincodec
# or
pnpm add @chainfoundry/chaincodec
```

No build step required — pre-built native binaries are bundled for all major platforms.

---

## Platform support

| Platform | Architecture | Supported |
| --- | --- | --- |
| Linux (glibc) | x64 | ✅ |
| Linux (glibc) | arm64 | ✅ |
| Linux (musl / Alpine) | x64 | ✅ |
| macOS | x64 (Intel) | ✅ |
| macOS | arm64 (Apple Silicon) | ✅ |
| Windows | x64 | ✅ |

---

## Quick start — decode an EVM event log

```typescript
import { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec';

// 1. Load your schemas (CSDL YAML format)
const registry = new MemoryRegistry();

// Load from inline CSDL string
registry.loadCsdl(`
schema ERC20Transfer:
  version: 1
  description: "ERC-20 standard Transfer event"
  chains: [ethereum, arbitrum, base, polygon, optimism]
  event: Transfer
  fingerprint: "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
  fields:
    from:  { type: address, indexed: true }
    to:    { type: address, indexed: true }
    value: { type: uint256, indexed: false }
  meta:
    protocol: erc20
    category: token
`);

// Or load from files on disk
// registry.loadFile('./schemas/erc20.csdl');
// registry.loadDirectory('./schemas');

// 2. Decode a raw log from eth_getLogs
const decoder = new EvmDecoder();

const rawLog = {
  chain: 'ethereum',
  txHash: '0xabc123...',
  blockNumber: 19500000,
  blockTimestamp: 1710000000,
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', // USDC
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef', // Transfer
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045', // from
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b', // to
  ],
  data: '0x00000000000000000000000000000000000000000000000000000000000f4240',
};

const event = decoder.decodeEvent(rawLog, registry);

console.log(event.schema);          // "ERC20Transfer"
console.log(event.fields.from);     // { type: 'address', value: '0xd8da6bf2...' }
console.log(event.fields.to);       // { type: 'address', value: '0xab5801a7...' }
console.log(event.fields.value);    // { type: 'biguint', value: '1000000' }
console.log(event.fingerprint);     // "0xddf252ad..."
```

---

## Decode function calldata

```typescript
import { EvmCallDecoder } from '@chainfoundry/chaincodec';
import { readFileSync } from 'fs';

// Load ABI JSON (from Etherscan, Hardhat artifacts, Foundry out/, etc.)
const abiJson = readFileSync('./abi/erc20.json', 'utf-8');
const decoder = EvmCallDecoder.fromAbiJson(abiJson);

// Decode raw calldata from a transaction's `input` field
const calldata = '0xa9059cbb000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045000000000000000000000000000000000000000000000000000000000000000a';
const call = decoder.decodeCall(calldata);

console.log(call.functionName);   // "transfer"
console.log(call.selector);       // "0xa9059cbb"

for (const [name, value] of call.inputs) {
  console.log(`  ${name}:`, value);
}
// to:     { type: 'address', value: '0xd8da6bf2...' }
// amount: { type: 'biguint', value: '10' }

// List all functions in the ABI
console.log(decoder.functionNames());            // ['transfer', 'approve', ...]
console.log(decoder.selectorFor('transfer'));    // "0xa9059cbb"
```

---

## ABI encode a function call

`encodeCall` takes `argsJson` — a JSON string of `NormalizedValue[]`.
Each `NormalizedValue` has `{ type, value }` matching the Solidity type.

```typescript
import { EvmEncoder } from '@chainfoundry/chaincodec';

const abiJson = JSON.stringify([{
  name: 'transfer',
  type: 'function',
  inputs: [
    { name: 'to',     type: 'address' },
    { name: 'amount', type: 'uint256' },
  ],
  outputs: [{ name: '', type: 'bool' }],
  stateMutability: 'nonpayable',
}]);

const encoder = EvmEncoder.fromAbiJson(abiJson);

const calldata = encoder.encodeCall(
  'transfer',
  JSON.stringify([
    { type: 'address', value: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045' },
    { type: 'uint',    value: 1000000 },
  ])
);

console.log(calldata);
// "0xa9059cbb000000000000000000000000d8da6bf2...000f4240"
// First 4 bytes (0xa9059cbb) = transfer(address,uint256) selector
```

### NormalizedValue input types for encoding

| Solidity type | `NormalizedValue` JSON |
| --- | --- |
| `address` | `{ "type": "address", "value": "0x..." }` |
| `uint256` | `{ "type": "uint", "value": 1000000 }` |
| `uint256` (large) | `{ "type": "biguint", "value": "99999999999999999999" }` |
| `int256` | `{ "type": "int", "value": -100 }` |
| `bool` | `{ "type": "bool", "value": true }` |
| `bytes` | `{ "type": "bytes", "value": [0xde, 0xad, 0xbe, 0xef] }` |
| `string` | `{ "type": "str", "value": "hello" }` |
| `address[]` | `{ "type": "array", "value": [{ "type": "address", "value": "0x..." }] }` |

---

## Batch decode

Decode thousands of logs in parallel using Rayon (Rust's parallel iterator):

```typescript
import { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec';

const registry = new MemoryRegistry();
registry.loadCsdl(/* your CSDL schemas */);

const decoder = new EvmDecoder();

// Decode a batch of raw logs — returns { events, errors }
const { events, errors } = decoder.decodeBatch(rawLogs, registry);

console.log(`decoded: ${events.length} events`);
console.log(`errors:  ${errors.length}`);

for (const event of events) {
  console.log(event.schema, event.fields);
}

for (const { index, error } of errors) {
  console.warn(`log[${index}] failed: ${error}`);
}
```

---

## Compute topic0 fingerprint

```typescript
const decoder = new EvmDecoder();

const fp = decoder.fingerprint({
  chain: 'ethereum',
  txHash: '0x0',
  blockNumber: 0,
  blockTimestamp: 0,
  logIndex: 0,
  address: '0x0000000000000000000000000000000000000000',
  topics: ['0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef'],
  data: '0x',
});
console.log(fp); // "0xddf252ad..."
```

---

## EIP-712 typed data

```typescript
import { Eip712Parser } from '@chainfoundry/chaincodec';

const parser = new Eip712Parser();

const typedDataJson = JSON.stringify({
  types: {
    EIP712Domain: [
      { name: 'name', type: 'string' },
      { name: 'version', type: 'string' },
      { name: 'chainId', type: 'uint256' },
    ],
    Transfer: [
      { name: 'to',     type: 'address' },
      { name: 'amount', type: 'uint256' },
    ],
  },
  primaryType: 'Transfer',
  domain: { name: 'MyToken', version: '1', chainId: 1 },
  message: { to: '0xd8dA...', amount: '1000000' },
});

const parsed = parser.parse(typedDataJson);
console.log(parsed.primary_type);  // "Transfer"  (snake_case from Rust serde)

const domainHash = parser.domainSeparator(typedDataJson);
console.log(domainHash);           // "0x..."
```

---

## Using CommonJS

```javascript
const { EvmDecoder, MemoryRegistry, EvmCallDecoder, EvmEncoder, Eip712Parser } = require('@chainfoundry/chaincodec');

const registry = new MemoryRegistry();
registry.loadCsdl(csdlString);

const decoder = new EvmDecoder();
const event = decoder.decodeEvent(rawLog, registry);
```

---

## API reference

### `MemoryRegistry`

| Method / Property | Signature | Description |
| --- | --- | --- |
| `constructor` | `new MemoryRegistry()` | Create empty registry |
| `loadCsdl` | `(csdl: string) => number` | Load schemas from CSDL YAML string; returns count loaded |
| `loadFile` | `(path: string) => number` | Load a single `.csdl` file |
| `loadDirectory` | `(path: string) => number` | Load all `.csdl` files in a directory |
| `schemaCount` | `readonly number` | Number of schemas registered |
| `schemaNames` | `() => string[]` | List all schema names |

### `EvmDecoder`

| Method | Signature | Description |
| --- | --- | --- |
| `constructor` | `new EvmDecoder()` | Create decoder |
| `decodeEvent` | `(raw: RawEvent, registry: MemoryRegistry) => DecodedEvent` | Decode a single EVM log |
| `decodeBatch` | `(raws: RawEvent[], registry: MemoryRegistry) => BatchDecodeResult` | Parallel-decode many logs |
| `fingerprint` | `(raw: RawEvent) => string` | Get topic0 fingerprint hex string |

### `EvmCallDecoder`

| Method | Signature | Description |
| --- | --- | --- |
| `fromAbiJson` | `(abiJson: string) => EvmCallDecoder` | Create from ABI JSON (static factory) |
| `decodeCall` | `(calldata: string, functionName?: string or null) => DecodedCall` | Decode calldata |
| `functionNames` | `() => string[]` | List all function names in ABI |
| `selectorFor` | `(functionName: string) => string or null` | Get 4-byte selector hex |

### `EvmEncoder`

| Method | Signature | Description |
| --- | --- | --- |
| `fromAbiJson` | `(abiJson: string) => EvmEncoder` | Create from ABI JSON (static factory) |
| `encodeCall` | `(functionName: string, argsJson: string) => string` | Encode; `argsJson` = `JSON.stringify(NormalizedValue[])`, returns `0x`-hex |

### `Eip712Parser`

| Method | Signature | Description |
| --- | --- | --- |
| `constructor` | `new Eip712Parser()` | Create parser |
| `parse` | `(json: string) => TypedData` | Parse EIP-712 typed data JSON |
| `domainSeparator` | `(json: string) => string` | Compute domain separator hash |

---

## TypeScript types

```typescript
interface RawEvent {
  chain: string;          // "ethereum" | "arbitrum" | "base" | "polygon" | "optimism" | numeric id
  txHash: string;
  blockNumber: number;
  blockTimestamp: number; // Unix seconds
  logIndex: number;
  address: string;        // contract address (hex, lowercase ok)
  topics: string[];       // topics[0] = event signature hash
  data: string;           // hex with 0x prefix
}

interface DecodedEvent {
  schema: string;                          // schema name, e.g. "ERC20Transfer"
  schemaVersion: number;
  chain: string;
  txHash: string;
  blockNumber: number;
  blockTimestamp: number;
  logIndex: number;
  address: string;
  fields: Record<string, NormalizedValue>; // decoded fields by name
  fingerprint: string;                     // keccak256 of topics[0]
  decodeErrors: Record<string, string>;    // fields that failed to decode
}

interface DecodedCall {
  functionName: string;
  selector: string | null;                 // "0xaabbccdd"
  inputs: Array<[string, NormalizedValue]>; // [name, value] pairs
  decodeErrors: Record<string, string>;
}

type NormalizedValue =
  | { type: 'uint';      value: number }
  | { type: 'biguint';   value: string }   // large uint256 as decimal string
  | { type: 'int';       value: number }
  | { type: 'bigint';    value: string }
  | { type: 'bool';      value: boolean }
  | { type: 'bytes';     value: number[] }
  | { type: 'str';       value: string }
  | { type: 'address';   value: string }   // "0x..." lowercase
  | { type: 'hash256';   value: string }
  | { type: 'timestamp'; value: number }
  | { type: 'array';     value: NormalizedValue[] }
  | { type: 'tuple';     value: Array<[string, NormalizedValue]> }
  | { type: 'null' }

interface BatchDecodeResult {
  events: DecodedEvent[];
  errors: Array<{ index: number; error: string }>;
}
```

---

## License

MIT — see [LICENSE](https://github.com/DarshanKumar89/chainkit/blob/main/LICENSE)
