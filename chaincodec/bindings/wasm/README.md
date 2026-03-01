# @chainfoundry/chaincodec-wasm

WebAssembly bindings for chaincodec — universal EVM ABI decoder for browsers, Deno, and edge runtimes.

[![npm](https://img.shields.io/npm/v/@chainfoundry/chaincodec-wasm)](https://www.npmjs.com/package/@chainfoundry/chaincodec-wasm)
[![license](https://img.shields.io/npm/l/@chainfoundry/chaincodec-wasm)](LICENSE)

Browser-ready WASM build of [chaincodec-evm](https://crates.io/crates/chaincodec-evm) — decode `eth_getLogs` entries, compute topic0 fingerprints, and decode ABI calldata entirely client-side. No server, no native dependencies.

> **Note on API style**: The WASM binding uses a JSON-in / JSON-out design. All complex inputs and outputs are passed as JSON strings for full compatibility across WASM runtimes. Use `JSON.parse()` / `JSON.stringify()` around calls.

---

## Install

```bash
# Browser / Vite / webpack
npm install @chainfoundry/chaincodec-wasm

# Node.js (WASM fallback — no native binaries required)
npm install @chainfoundry/chaincodec-wasm-node
```

For Node.js production workloads, prefer [@chainfoundry/chaincodec](https://www.npmjs.com/package/@chainfoundry/chaincodec) (native napi-rs bindings) for ~12× better throughput.

---

## Browser / Vite / webpack

```typescript
import init, { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec-wasm';

// WASM must be initialized once before any use
await init();

// 1. Load a CSDL schema
const csdl = await fetch('/schemas/erc20.csdl').then(r => r.text());
const registry = new MemoryRegistry();
registry.loadCsdl(csdl);

// 2. Decode a log — input/output are JSON strings
const decoder = new EvmDecoder();

const rawLog = {
  chain: 'ethereum',
  txHash: '0xabc...',
  blockNumber: 19500000,
  blockTimestamp: 1710000000,
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef',
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045',
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b',
  ],
  data: '0x00000000000000000000000000000000000000000000000000000000000f4240',
};

const eventJson = decoder.decodeEventJson(JSON.stringify(rawLog), registry);
const event = JSON.parse(eventJson);

console.log(event.schema);          // "ERC20Transfer"
console.log(event.fields.from);     // { type: 'address', value: '0xd8da6bf2...' }
console.log(event.fields.value);    // { type: 'biguint', value: '1000000' }
```

---

## Node.js (WASM fallback)

```javascript
// No init() needed for the Node.js CJS wasm-pack build
const { EvmDecoder, MemoryRegistry } = require('@chainfoundry/chaincodec-wasm-node');
const fs = require('fs');

const registry = new MemoryRegistry();
registry.loadCsdl(fs.readFileSync('./schemas/erc20.csdl', 'utf-8'));

const decoder = new EvmDecoder();
const eventJson = decoder.decodeEventJson(JSON.stringify(rawLog), registry);
const event = JSON.parse(eventJson);
```

---

## Decode function calldata

```typescript
import init, { EvmCallDecoder } from '@chainfoundry/chaincodec-wasm';
await init();

const abiJson = JSON.stringify([{
  name: 'transfer', type: 'function',
  inputs: [{ name: 'to', type: 'address' }, { name: 'amount', type: 'uint256' }],
  outputs: [{ name: '', type: 'bool' }], stateMutability: 'nonpayable',
}]);

const decoder = EvmCallDecoder.fromAbiJson(abiJson);

const calldata = '0xa9059cbb000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045000000000000000000000000000000000000000000000000000000000000000a';
const resultJson = decoder.decodeCallJson(calldata, null);
const call = JSON.parse(resultJson);

console.log(call.functionName);   // "transfer"
console.log(call.selector);       // "0xa9059cbb"
console.log(call.inputs);         // [["to", {...}], ["amount", {...}]]

// List functions in ABI
const names = JSON.parse(decoder.functionNamesJson());   // ["transfer"]
const sel = decoder.selectorFor('transfer');             // "0xa9059cbb"
```

---

## ABI encode a function call

```typescript
import init, { EvmEncoder } from '@chainfoundry/chaincodec-wasm';
await init();

const encoder = EvmEncoder.fromAbiJson(abiJson);

// argsJson must be JSON.stringify of NormalizedValue[]
const calldata = encoder.encodeCall(
  'transfer',
  JSON.stringify([
    { type: 'address', value: '0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045' },
    { type: 'uint',    value: 1000000 },
  ])
);

console.log(calldata); // "0xa9059cbb000000..."
```

---

## Batch decode

```typescript
import init, { EvmDecoder, MemoryRegistry } from '@chainfoundry/chaincodec-wasm';
await init();

const registry = new MemoryRegistry();
registry.loadCsdl(csdlString);

const decoder = new EvmDecoder();
const resultJson = decoder.decodeBatchJson(JSON.stringify(rawLogs), registry);
const { events, errors } = JSON.parse(resultJson);

console.log(`decoded: ${events.length}, errors: ${errors.length}`);
```

---

## Compute fingerprint

```typescript
import init, { EvmDecoder } from '@chainfoundry/chaincodec-wasm';
await init();

// Pass a minimal raw event with just topics[0] set
const decoder = new EvmDecoder();
const fp = decoder.fingerprintJson(JSON.stringify({
  chain: 'ethereum',
  txHash: '0x0',
  blockNumber: 0,
  blockTimestamp: 0,
  logIndex: 0,
  address: '0x0000000000000000000000000000000000000000',
  topics: ['0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef'],
  data: '0x',
}));
console.log(fp); // "0xddf252ad..."
```

---

## EIP-712 typed data

```typescript
import init, { Eip712Parser } from '@chainfoundry/chaincodec-wasm';
await init();

const parser = new Eip712Parser();

const typedDataJson = JSON.stringify({
  types: { EIP712Domain: [{ name: 'name', type: 'string' }], Transfer: [{ name: 'to', type: 'address' }] },
  primaryType: 'Transfer',
  domain: { name: 'MyToken' },
  message: { to: '0xd8dA...' },
});

const parsed = JSON.parse(parser.parseJson(typedDataJson));
console.log(parsed.primary_type);    // "Transfer"  (snake_case)

const domainHash = parser.domainSeparator(typedDataJson);
console.log(domainHash);             // "0x..."
```

---

## Deno

```typescript
import init, { EvmDecoder } from 'npm:@chainfoundry/chaincodec-wasm';
await init();

const decoder = new EvmDecoder();
const eventJson = decoder.decodeEventJson(JSON.stringify(rawLog), registry);
const event = JSON.parse(eventJson);
```

---

## Cloudflare Workers

```typescript
// wrangler.toml: compatibility_flags = ["nodejs_compat"]
import init, { EvmDecoder } from '@chainfoundry/chaincodec-wasm';

export default {
  async fetch(request: Request): Promise<Response> {
    await init();
    const decoder = new EvmDecoder();
    const eventJson = decoder.decodeEventJson(JSON.stringify(rawLog), registry);
    const event = JSON.parse(eventJson);
    return Response.json(event);
  },
};
```

---

## Bundle size

| Target | .wasm size | JS glue | Total (gzip) |
| --- | --- | --- | --- |
| Browser ESM | ~850 KB | ~12 KB | ~280 KB |
| Node.js CJS | ~850 KB | ~10 KB | ~280 KB |

The WASM binary is optimized with `wasm-opt -O3`. Serve the `.wasm` file with `Content-Type: application/wasm` for streaming instantiation.

---

## API reference

All methods use a **JSON string in / JSON string out** pattern for WASM compatibility.

### `MemoryRegistry`

| Method | Signature | Description |
| --- | --- | --- |
| `constructor` | `new MemoryRegistry()` | Create empty registry |
| `loadCsdl` | `(csdl: string) => number` | Parse CSDL YAML, returns count loaded |
| `schemaCount` | `readonly number` | Number of schemas registered |
| `schemaNamesJson` | `() => string` | JSON array of schema names |

### `EvmDecoder`

| Method | Signature | Description |
| --- | --- | --- |
| `constructor` | `new EvmDecoder()` | Create decoder |
| `decodeEventJson` | `(rawJson: string, registry: MemoryRegistry) => string` | Decode one log; input/output JSON |
| `decodeBatchJson` | `(rawsJson: string, registry: MemoryRegistry) => string` | Decode many logs; returns `{events,errors}` JSON |
| `fingerprintJson` | `(rawJson: string) => string` | Get topic0 fingerprint from log JSON |

### `EvmCallDecoder`

| Method | Signature | Description |
| --- | --- | --- |
| `fromAbiJson` | `(abiJson: string) => EvmCallDecoder` | Create from ABI JSON (static factory) |
| `decodeCallJson` | `(calldata: string, functionName?: string or null) => string` | Decode calldata, returns JSON |
| `functionNamesJson` | `() => string` | JSON array of function names |
| `selectorFor` | `(name: string) => string or null` | Get 4-byte selector hex |

### `EvmEncoder`

| Method | Signature | Description |
| --- | --- | --- |
| `fromAbiJson` | `(abiJson: string) => EvmEncoder` | Create from ABI JSON (static factory) |
| `encodeCall` | `(functionName: string, argsJson: string) => string` | Encode call, returns `0x`-hex |

### `Eip712Parser`

| Method | Signature | Description |
| --- | --- | --- |
| `constructor` | `new Eip712Parser()` | Create parser |
| `parseJson` | `(json: string) => string` | Parse EIP-712 JSON, returns TypedData JSON |
| `domainSeparator` | `(json: string) => string` | Compute domain separator hash |

---

## vs native Node.js package

| Feature | `@chainfoundry/chaincodec` (napi) | `@chainfoundry/chaincodec-wasm` |
| --- | --- | --- |
| Runtime | Node.js only | Browser, Deno, Workers, Node.js |
| Throughput | ~6M events/sec (Rayon) | ~500K events/sec (single-thread) |
| Install size | ~2 MB (.node binary) | ~850 KB (.wasm) |
| Native deps | Yes (pre-built) | None |
| `init()` required | No | Yes (browser ESM only) |
| API style | JS objects in/out | JSON strings in/out |

Use the napi package for server-side indexers. Use the WASM package for browser dApps, edge functions, and environments with no native binary support.

---

## License

MIT — see [LICENSE](https://github.com/DarshanKumar89/chainkit/blob/main/LICENSE)
