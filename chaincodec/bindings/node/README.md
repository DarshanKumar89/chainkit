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
|----------|-------------|-----------|
| Linux (glibc) | x64 | ✅ |
| Linux (glibc) | arm64 | ✅ |
| Linux (musl / Alpine) | x64 | ✅ |
| macOS | x64 (Intel) | ✅ |
| macOS | arm64 (Apple Silicon) | ✅ |
| Windows | x64 | ✅ |

---

## Quick start

```typescript
import { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec';

// 1. Load schemas
const registry = new MemoryRegistry();
registry.loadDirectory('./node_modules/@chainfoundry/chaincodec/schemas');

// 2. Decode a raw log from eth_getLogs
const decoder = new EvmDecoder();

const rawLog = {
  chain: 'ethereum',
  txHash: '0xabc123...',
  blockNumber: 19500000n,
  blockTimestamp: 1710000000n,
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', // USDC
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef', // Transfer
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045', // from
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b', // to
  ],
  data: '0x00000000000000000000000000000000000000000000000000000000000f4240',
};

const fp = decoder.fingerprint(rawLog);
const schema = registry.getByFingerprint(fp);

if (schema) {
  const event = decoder.decodeEvent(rawLog, schema);
  console.log(event.schemaName);       // "ERC20Transfer"
  console.log(event.fields.from);      // "0xd8da6bf2..."
  console.log(event.fields.to);        // "0xab5801a7..."
  console.log(event.fields.value);     // "1000000"  (1 USDC)
}
```

---

## Decode function calldata

```typescript
import { EvmCallDecoder } from '@chainfoundry/chaincodec';
import { readFileSync } from 'fs';

// Load ABI JSON (from Etherscan, Hardhat artifacts, Foundry out/, etc.)
const abiJson = readFileSync('./abi/uniswap_v3_router.json', 'utf-8');
const decoder = EvmCallDecoder.fromAbiJson(abiJson);

// Decode raw calldata from a transaction's `input` field
const calldata = '0x414bf389...';
const call = decoder.decodeCall(calldata);

console.log(call.functionName);        // "exactInputSingle"
for (const [name, value] of Object.entries(call.inputs)) {
  console.log(`  ${name}: ${value}`);
}
// tokenIn:      0xC02aaA39b... (WETH)
// tokenOut:     0xA0b8699...  (USDC)
// amountIn:     1000000000000000000
// amountOutMin: 1800000000
```

---

## Batch decode

```typescript
import { BatchEngine, BatchRequest, ErrorMode } from '@chainfoundry/chaincodec';

const engine = new BatchEngine(registry);
engine.addDecoder('ethereum', new EvmDecoder());

const logs = await fetchLogsFromRpc(fromBlock, toBlock);  // RawLog[]

const result = engine.decode({
  chain: 'ethereum',
  logs,
  chunkSize: 10_000,
  errorMode: ErrorMode.Collect,
  onProgress: (decoded, total) => {
    process.stdout.write(`\r${decoded}/${total}`);
  },
});

console.log(`decoded: ${result.events.length}`);
console.log(`errors:  ${result.errors.length}`);
```

---

## Compute topic0 fingerprint

```typescript
import { computeFingerprint } from '@chainfoundry/chaincodec';

const fp = computeFingerprint('Transfer(address,address,uint256)');
// "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"

const fp2 = computeFingerprint('Swap(address,uint256,uint256,uint256,uint256,address)');
// Uniswap V2 Swap topic0
```

---

## API reference

### `EvmDecoder`

| Method | Description |
|--------|-------------|
| `fingerprint(log)` | Compute topic0 fingerprint from a raw log |
| `decodeEvent(log, schema)` | Decode an EVM log into named fields |

### `EvmCallDecoder`

| Method | Description |
|--------|-------------|
| `EvmCallDecoder.fromAbiJson(json)` | Create decoder from ABI JSON string |
| `decodeCall(calldata, blockNumber?)` | Decode function calldata |
| `encodeCall(functionName, args)` | ABI-encode a function call |

### `MemoryRegistry`

| Method | Description |
|--------|-------------|
| `loadFile(path)` | Load a CSDL schema file |
| `loadDirectory(path)` | Load all `.csdl` files in a directory |
| `getByFingerprint(fp)` | Look up schema by topic0 hash |
| `getByName(name)` | Look up schema by name |
| `allSchemas()` | Return all registered schemas |

### `BatchEngine`

| Method | Description |
|--------|-------------|
| `addDecoder(chainSlug, decoder)` | Register a decoder for a chain |
| `decode(request)` | Batch decode a list of raw logs |

---

## TypeScript types

```typescript
interface RawLog {
  chain: string;
  txHash: string;
  blockNumber: bigint;
  blockTimestamp: bigint;
  logIndex: number;
  address: string;
  topics: string[];
  data: string;
}

interface DecodedEvent {
  schemaName: string;
  chain: string;
  blockNumber: bigint;
  txHash: string;
  logIndex: number;
  fields: Record<string, string>;  // NormalizedValue as string
}

interface DecodedCall {
  functionName: string;
  inputs: Record<string, string>;
}
```

---

## Using CommonJS

```javascript
const { EvmDecoder, MemoryRegistry } = require('@chainfoundry/chaincodec');

const registry = new MemoryRegistry();
registry.loadDirectory('./schemas');

const decoder = new EvmDecoder();
```

---

## Bundled schemas

The package ships with 50+ CSDL schemas covering ERC-20/721/1155, Uniswap, Aave, Compound, ChainLink, and more. Schemas are in the `schemas/` subdirectory of the installed package.

```typescript
import { join } from 'path';
import { createRequire } from 'module';

const require = createRequire(import.meta.url);
const schemasDir = join(
  require.resolve('@chainfoundry/chaincodec/package.json'),
  '../schemas'
);
registry.loadDirectory(schemasDir);
```

---

## License

MIT — see [LICENSE](https://github.com/DarshanKumar89/chainkit/blob/main/LICENSE)
