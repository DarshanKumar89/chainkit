# @chainfoundry/chaincodec-wasm

WebAssembly bindings for chaincodec — universal EVM ABI decoder for browsers, Deno, and edge runtimes.

[![npm](https://img.shields.io/npm/v/@chainfoundry/chaincodec-wasm)](https://www.npmjs.com/package/@chainfoundry/chaincodec-wasm)
[![license](https://img.shields.io/npm/l/@chainfoundry/chaincodec-wasm)](LICENSE)

Browser-ready WASM build of [chaincodec-evm](https://crates.io/crates/chaincodec-evm) — decode `eth_getLogs` entries, compute topic0 fingerprints, and decode ABI calldata entirely client-side. No server, no native dependencies.

---

## Packages

Two packages are published from the same WASM build:

| Package | Target | Use case |
|---------|--------|----------|
| [`@chainfoundry/chaincodec-wasm`](https://www.npmjs.com/package/@chainfoundry/chaincodec-wasm) | ESM (browser) | React, Vue, Svelte, Vite |
| [`@chainfoundry/chaincodec-wasm-node`](https://www.npmjs.com/package/@chainfoundry/chaincodec-wasm-node) | CJS (Node.js) | Node without native bindings |

For Node.js production workloads, prefer [@chainfoundry/chaincodec](https://www.npmjs.com/package/@chainfoundry/chaincodec) (native napi-rs bindings) for better throughput.

---

## Install

```bash
# Browser / Vite / webpack
npm install @chainfoundry/chaincodec-wasm

# Node.js (WASM fallback — no native binaries required)
npm install @chainfoundry/chaincodec-wasm-node
```

---

## Browser / Vite / webpack

```typescript
import init, { EvmDecoder, MemoryRegistry, computeFingerprint }
  from '@chainfoundry/chaincodec-wasm';

// WASM must be initialized once before use
await init();

// Load a CSDL schema from a string
const csdl = await fetch('/schemas/erc20.csdl').then(r => r.text());
const registry = new MemoryRegistry();
registry.loadFromString(csdl);

// Decode a log from eth_getLogs
const decoder = new EvmDecoder();

const rawLog = {
  chain: 'ethereum',
  txHash: '0xabc...',
  blockNumber: BigInt(19500000),
  logIndex: 0,
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
  topics: [
    '0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef',
    '0x000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045',
    '0x000000000000000000000000ab5801a7d398351b8be11c439e05c5b3259aec9b',
  ],
  data: '0x00000000000000000000000000000000000000000000000000000000000f4240',
};

const fp = decoder.fingerprint(rawLog);
const schema = registry.getByFingerprint(fp);

if (schema) {
  const event = decoder.decodeEvent(rawLog, schema);
  console.log(event.schemaName);      // "ERC20Transfer"
  console.log(event.fields.from);     // "0xd8da6bf2..."
  console.log(event.fields.value);    // "1000000"
}
```

---

## Node.js (WASM fallback)

```javascript
const { EvmDecoder, MemoryRegistry } = require('@chainfoundry/chaincodec-wasm-node');

// No init() needed for the Node.js CJS build
const registry = new MemoryRegistry();
registry.loadFromString(fs.readFileSync('./schemas/erc20.csdl', 'utf-8'));

const decoder = new EvmDecoder();
const fp = decoder.fingerprint(rawLog);
```

---

## Compute topic0 fingerprint

```typescript
import init, { computeFingerprint } from '@chainfoundry/chaincodec-wasm';
await init();

const fp = computeFingerprint('Transfer(address,address,uint256)');
// "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
```

This is useful for building event filters without a backend:

```typescript
const filter = {
  address: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48',
  topics: [computeFingerprint('Transfer(address,address,uint256)')],
};
// Pass to eth_getLogs, viem watchEvent, ethers.js filter, etc.
```

---

## Usage with Deno

```typescript
import init, { EvmDecoder } from 'npm:@chainfoundry/chaincodec-wasm';
await init();

const decoder = new EvmDecoder();
```

---

## Usage with Cloudflare Workers

```typescript
// wrangler.toml: compatibility_flags = ["nodejs_compat"]
import init, { EvmDecoder } from '@chainfoundry/chaincodec-wasm';

export default {
  async fetch(request: Request): Promise<Response> {
    await init();
    const decoder = new EvmDecoder();
    // ...
    return new Response('ok');
  },
};
```

---

## Bundle size

| Target | .wasm size | JS glue | Total (gzip) |
|--------|-----------|---------|-------------|
| Browser ESM | ~850 KB | ~12 KB | ~280 KB |
| Node.js CJS | ~850 KB | ~10 KB | ~280 KB |

The WASM binary is optimized with `wasm-opt -O3`. For browser use, serve the `.wasm` file with `Content-Type: application/wasm` for streaming instantiation.

---

## API reference

### `EvmDecoder`

| Method | Description |
|--------|-------------|
| `fingerprint(log)` | Compute topic0 fingerprint from a raw log |
| `decodeEvent(log, schema)` | Decode a log into named `NormalizedValue` fields |

### `MemoryRegistry`

| Method | Description |
|--------|-------------|
| `loadFromString(csdl)` | Parse and register schemas from a CSDL YAML string |
| `getByFingerprint(fp)` | Look up schema by topic0 hash |
| `getByName(name)` | Look up schema by name |

### Functions

| Function | Description |
|----------|-------------|
| `computeFingerprint(sig)` | Compute keccak256 topic0 from a Solidity event signature |
| `init()` | Initialize the WASM module (browser ESM only) |

---

## Differences from the native Node.js package

| Feature | `@chainfoundry/chaincodec` (napi) | `@chainfoundry/chaincodec-wasm` |
|---------|----------------------------------|--------------------------------|
| Runtime | Node.js only | Browser, Deno, Workers, Node.js |
| Throughput | ~6M events/sec (Rayon) | ~500K events/sec (single-thread) |
| Install size | ~2 MB (.node binary) | ~850 KB (.wasm) |
| Native deps | Yes (pre-built) | None |
| `init()` required | No | Yes (browser ESM only) |

Use the napi package for server-side indexers. Use the WASM package for browser dApps, edge functions, and zero-native-dep environments.

---

## License

MIT — see [LICENSE](https://github.com/DarshanKumar89/chainkit/blob/main/LICENSE)
