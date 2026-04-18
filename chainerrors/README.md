# ChainErrors

> **Blockchain error decoder for EVM and Solana — decode reverts, panics, custom errors, and program failures.**

ChainErrors is part of [ChainFoundry](https://github.com/DarshanKumar89/chainfoundry), a monorepo of blockchain primitives for Rust, TypeScript, Python, Go, and Java.

---

## Features

| Feature | Status |
|---------|--------|
| EVM `Error(string)` revert decoding | ✅ |
| EVM `Panic(uint256)` decode with meanings | ✅ |
| EVM custom error decoding (40+ bundled) | ✅ |
| Solana System Program errors (0-17) | ✅ |
| Solana SPL Token errors (0-17) | ✅ |
| Solana Anchor framework errors (100-5000+) | ✅ |
| Solana program log parsing | ✅ |
| Confidence scoring (0.0 – 1.0) | ✅ |
| Human-readable suggestions | ✅ |
| Golden fixture test suite | ✅ |

## Quick Start

### Rust

```toml
[dependencies]
chainerrors-core = "0.1.0"
chainerrors-evm  = "0.1.0"
chainerrors-solana = "0.1.0"
```

```rust
use chainerrors_evm::EvmErrorDecoder;
use chainerrors_core::ErrorDecoder;

let decoder = EvmErrorDecoder::new();

// Decode a revert string
let data = hex::decode("08c379a0...").unwrap();
let result = decoder.decode(&data, None).unwrap();
println!("{result}");
// → "reverted: Not enough tokens"

// Decode a panic
let panic_data = hex::decode("4e487b71...0011").unwrap();
let result = decoder.decode(&panic_data, None).unwrap();
println!("{result}");
// → "panic 0x11: arithmetic overflow"
```

```rust
use chainerrors_solana::SolanaErrorDecoder;
use chainerrors_core::ErrorDecoder;

let decoder = SolanaErrorDecoder::new();

// Decode a system program error
let result = decoder.decode_error_code(
    1,
    Some("11111111111111111111111111111111"),
    None,
).unwrap();
println!("{result}");
// → "AccountAlreadyInUse(code=1)"

// Decode from program logs
let result = decoder.decode_log(
    "Program failed: custom program error: 0xbc4",
    None,
).unwrap();
println!("{result}");
// → "AccountNotInitialized(code=3012)"
```

### TypeScript (Node.js)

```bash
npm install @chainfoundry/chainerrors
```

```typescript
import { decodeEvmError } from '@chainfoundry/chainerrors';

const result = decodeEvmError('0x08c379a0...');
console.log(result.kind);    // "revert_string"
console.log(result.message); // "Not enough tokens"
```

### Python

```bash
pip install chainerrors
```

```python
from chainerrors import decode_evm_error

result = decode_evm_error("0x08c379a0...")
print(result["kind"])     # "revert_string"
print(result["message"])  # "Not enough tokens"
```

### CLI

```bash
cargo install chainerrors-cli

chainerrors decode --data 0x08c379a0...
# → Error(string): "Not enough tokens"

chainerrors decode --data 0x4e487b710000...0011
# → Panic(uint256): arithmetic overflow (0x11)
```

---

## EVM Error Types

### Revert Strings (`0x08c379a0`)

```
require(balance >= amount, "Insufficient balance")
→ Error(string): "Insufficient balance"
```

### Panic Codes (`0x4e487b71`)

| Code | Meaning |
|------|---------|
| 0x01 | `assert()` violation |
| 0x11 | Arithmetic overflow |
| 0x12 | Division by zero |
| 0x21 | Invalid enum conversion |
| 0x32 | Array out of bounds |
| 0x51 | Zero-initialized function pointer |

### Bundled Custom Errors (40+)

**ERC-20**: InsufficientBalance, InvalidSender, InvalidReceiver, InsufficientAllowance
**ERC-721**: InvalidOwner, NonexistentToken, IncorrectOwner, InsufficientApproval
**OpenZeppelin**: OwnableUnauthorizedAccount, AccessControlUnauthorizedAccount, ReentrancyGuardReentrantCall, EnforcedPause
**Uniswap V3**: T, LOK, TLU, TLM, TUM, AS, M0, M1, IIA, SPL, F0, F1, L, LS, LA
**EIP-4626**: ExceededMaxDeposit/Mint/Withdraw/Redeem
**SafeERC20**: FailedOperation, FailedDecreaseAllowance

---

## Solana Error Types

### System Program (18 errors)

AccountAlreadyInitialized, AccountAlreadyInUse, AccountDataTooSmall, AccountNotRentExempt, InsufficientFundsForFee, InvalidAccountDataLength, and more.

### SPL Token (18 errors)

NotRentExempt, InsufficientFunds, InvalidMint, MintMismatch, OwnerMismatch, UninitializedState, AccountFrozen, and more.

### Anchor Framework (40+ errors)

- **Instruction** (100-103): InstructionMissing, InstructionDidNotDeserialize
- **Constraint** (2000-2020): ConstraintMut, ConstraintHasOne, ConstraintSigner, ConstraintSeeds, ConstraintOwner
- **Account** (3000-3017): AccountDiscriminatorMismatch, AccountNotInitialized, AccountOwnedByWrongProgram, InvalidProgramId

### Program Log Parsing

```
"Program XYZ failed: custom program error: 0xbc4" → Code(3012) → AccountNotInitialized
"Program log: Error: insufficient funds" → Message("insufficient funds")
"Error Code: AccountNotInitialized. Error Number: 3012." → Code(3012)
```

---

## Architecture

```
chainerrors/
├── crates/
│   ├── chainerrors-core/     # ErrorDecoder trait, DecodedError, ErrorKind, registry
│   ├── chainerrors-evm/      # EvmErrorDecoder — revert, panic, custom errors
│   └── chainerrors-solana/   # SolanaErrorDecoder — system, token, anchor, logs
├── bindings/
│   ├── node/                 # TypeScript via napi-rs
│   ├── python/               # Python via PyO3/maturin
│   ├── go/                   # Go via cgo
│   └── java/                 # Java via JNI
└── cli/                      # chainerrors-cli binary
```

## Tests

```bash
cd chainerrors && cargo test --workspace
# 72 tests: 7 core + 17 evm + 10 golden + 35 solana + 3 doc-tests
```

---

## License

MIT — see [LICENSE](../LICENSE)

## Contact

Built by [@darshan_aqua](https://x.com/darshan_aqua) — questions, feedback, and contributions welcome.
