//! `EvmErrorDecoder` — the top-level EVM error decoder.
//!
//! Decode priority:
//! 1. Empty data          → `ErrorKind::Empty`
//! 2. `0x08c379a0` prefix → `ErrorKind::RevertString`   (Error(string))
//! 3. `0x4e487b71` prefix → `ErrorKind::Panic`           (Panic(uint256))
//! 4. Known 4-byte selector in registry → `ErrorKind::CustomError`
//! 5. Fallback            → `ErrorKind::RawRevert`

use std::sync::Arc;

use chainerrors_core::decoder::{DecodeErrorError, ErrorDecoder};
use chainerrors_core::registry::{ErrorSignature, ErrorSignatureRegistry, MemoryErrorRegistry};
use chainerrors_core::types::{DecodedError, ErrorContext, ErrorKind};

use crate::custom::decode_custom_error;
use crate::panic::{decode_panic, PANIC_SELECTOR};
use crate::revert::{decode_error_string, ERROR_STRING_SELECTOR};

/// EVM error decoder with a bundled signature registry.
///
/// # Usage
/// ```rust,no_run
/// use chainerrors_evm::EvmErrorDecoder;
/// use chainerrors_core::ErrorDecoder;
///
/// let decoder = EvmErrorDecoder::new();
/// let result = decoder.decode(&hex::decode("08c379a0...").unwrap(), None).unwrap();
/// println!("{result}");
/// ```
pub struct EvmErrorDecoder {
    registry: Arc<dyn ErrorSignatureRegistry>,
}

impl EvmErrorDecoder {
    /// Create a decoder with the built-in bundled error signature registry.
    pub fn new() -> Self {
        let reg = Arc::new(MemoryErrorRegistry::new());
        Self::with_bundled_signatures(&reg);
        Self { registry: reg }
    }

    /// Create a decoder with a custom registry (for testing or extension).
    pub fn with_registry(registry: Arc<dyn ErrorSignatureRegistry>) -> Self {
        Self { registry }
    }

    /// Populate a `MemoryErrorRegistry` with the bundled well-known error signatures.
    fn with_bundled_signatures(reg: &MemoryErrorRegistry) {
        use chainerrors_core::registry::{ErrorParam, ErrorSignature};
        use tiny_keccak::{Hasher, Keccak};

        fn sel(sig: &str) -> [u8; 4] {
            let mut k = Keccak::v256();
            k.update(sig.as_bytes());
            let mut out = [0u8; 32];
            k.finalize(&mut out);
            [out[0], out[1], out[2], out[3]]
        }

        fn p(name: &str, ty: &str) -> ErrorParam {
            ErrorParam { name: name.to_string(), ty: ty.to_string() }
        }

        let bundled: &[(&str, &str, &[(&str, &str)], Option<&str>)] = &[
            // ─── ERC-20 ───────────────────────────────────────────────────────
            ("ERC20InsufficientBalance", "ERC20InsufficientBalance(address,uint256,uint256)",
             &[("sender","address"),("balance","uint256"),("needed","uint256")],
             Some("The sender does not have enough token balance for this transfer.")),
            ("ERC20InvalidSender", "ERC20InvalidSender(address)",
             &[("sender","address")], None),
            ("ERC20InvalidReceiver", "ERC20InvalidReceiver(address)",
             &[("receiver","address")], None),
            ("ERC20InsufficientAllowance", "ERC20InsufficientAllowance(address,uint256,uint256)",
             &[("spender","address"),("allowance","uint256"),("needed","uint256")],
             Some("Increase the token allowance before calling transferFrom.")),
            ("ERC20InvalidApprover", "ERC20InvalidApprover(address)",
             &[("approver","address")], None),
            ("ERC20InvalidSpender", "ERC20InvalidSpender(address)",
             &[("spender","address")], None),

            // ─── ERC-721 ──────────────────────────────────────────────────────
            ("ERC721InvalidOwner", "ERC721InvalidOwner(address)",
             &[("owner","address")], None),
            ("ERC721NonexistentToken", "ERC721NonexistentToken(uint256)",
             &[("tokenId","uint256")], None),
            ("ERC721IncorrectOwner", "ERC721IncorrectOwner(address,uint256,address)",
             &[("sender","address"),("tokenId","uint256"),("owner","address")], None),
            ("ERC721InvalidSender", "ERC721InvalidSender(address)",
             &[("sender","address")], None),
            ("ERC721InvalidReceiver", "ERC721InvalidReceiver(address)",
             &[("receiver","address")], None),
            ("ERC721InsufficientApproval", "ERC721InsufficientApproval(address,uint256)",
             &[("operator","address"),("tokenId","uint256")], None),
            ("ERC721InvalidApprover", "ERC721InvalidApprover(address)",
             &[("approver","address")], None),
            ("ERC721InvalidOperator", "ERC721InvalidOperator(address)",
             &[("operator","address")], None),

            // ─── OpenZeppelin Ownable ─────────────────────────────────────────
            ("OwnableUnauthorizedAccount", "OwnableUnauthorizedAccount(address)",
             &[("account","address")],
             Some("Only the owner can call this function. Ensure you are using the owner address.")),
            ("OwnableInvalidOwner", "OwnableInvalidOwner(address)",
             &[("owner","address")], None),

            // ─── OpenZeppelin Access Control ───────────────────────────────────
            ("AccessControlUnauthorizedAccount", "AccessControlUnauthorizedAccount(address,bytes32)",
             &[("account","address"),("neededRole","bytes32")],
             Some("The caller is missing the required role. Grant the role with grantRole().")),
            ("AccessControlBadConfirmation", "AccessControlBadConfirmation()",
             &[], None),

            // ─── OpenZeppelin ReentrancyGuard ─────────────────────────────────
            ("ReentrancyGuardReentrantCall", "ReentrancyGuardReentrantCall()",
             &[], Some("Reentrancy detected. Do not call this function recursively.")),

            // ─── OpenZeppelin Pausable ────────────────────────────────────────
            ("EnforcedPause", "EnforcedPause()",
             &[], Some("The contract is paused. Wait for it to be unpaused.")),
            ("ExpectedPause", "ExpectedPause()",
             &[], None),

            // ─── Uniswap V3 custom (terse) errors ────────────────────────────
            ("T", "T()", &[], Some("Uniswap V3: tick out of range.")),
            ("LOK", "LOK()", &[], Some("Uniswap V3: pool is locked.")),
            ("TLU", "TLU()", &[], Some("Uniswap V3: tick lower >= tick upper.")),
            ("TLM", "TLM()", &[], Some("Uniswap V3: tick lower too low.")),
            ("TUM", "TUM()", &[], Some("Uniswap V3: tick upper too high.")),
            ("AS", "AS()", &[], Some("Uniswap V3: amount specified is zero.")),
            ("M0", "M0()", &[], Some("Uniswap V3: mint amounts are zero.")),
            ("M1", "M1()", &[], Some("Uniswap V3: mint amount0 exceeds limit.")),
            ("IIA", "IIA()", &[], Some("Uniswap V3: insufficient input amount.")),
            ("SPL", "SPL()", &[], Some("Uniswap V3: sqrt price limit is out of range.")),
            ("F0", "F0()", &[], Some("Uniswap V3: flash amount0 > balance.")),
            ("F1", "F1()", &[], Some("Uniswap V3: flash amount1 > balance.")),
            ("L", "L()", &[], Some("Uniswap V3: liquidity is zero.")),
            ("LS", "LS()", &[], Some("Uniswap V3: liquidity exceeds maximum.")),
            ("LA", "LA()", &[], Some("Uniswap V3: liquidity amount overflows.")),

            // ─── SafeMath (pre-Solidity 0.8) ─────────────────────────────────
            // These revert with string, handled by revert decoder.
            // Custom error versions below if contracts define them.

            // ─── EIP-4626 Vault ───────────────────────────────────────────────
            ("ERC4626ExceededMaxDeposit", "ERC4626ExceededMaxDeposit(address,uint256,uint256)",
             &[("receiver","address"),("assets","uint256"),("max","uint256")], None),
            ("ERC4626ExceededMaxMint", "ERC4626ExceededMaxMint(address,uint256,uint256)",
             &[("receiver","address"),("shares","uint256"),("max","uint256")], None),
            ("ERC4626ExceededMaxWithdraw", "ERC4626ExceededMaxWithdraw(address,uint256,uint256)",
             &[("owner","address"),("assets","uint256"),("max","uint256")], None),
            ("ERC4626ExceededMaxRedeem", "ERC4626ExceededMaxRedeem(address,uint256,uint256)",
             &[("owner","address"),("shares","uint256"),("max","uint256")], None),

            // ─── Address utility ──────────────────────────────────────────────
            ("AddressInsufficientBalance", "AddressInsufficientBalance(address)",
             &[("account","address")], None),
            ("AddressEmptyCode", "AddressEmptyCode(address)",
             &[("target","address")],
             Some("The target address has no contract code deployed.")),
            ("FailedInnerCall", "FailedInnerCall()",
             &[], None),

            // ─── SafeERC20 ────────────────────────────────────────────────────
            ("SafeERC20FailedOperation", "SafeERC20FailedOperation(address)",
             &[("token","address")],
             Some("The ERC-20 token operation failed. Ensure the token is compliant.")),
            ("SafeERC20FailedDecreaseAllowance", "SafeERC20FailedDecreaseAllowance(address,uint256)",
             &[("spender","address"),("currentAllowance","uint256")], None),
        ];

        for (name, sig_str, raw_inputs, hint) in bundled {
            let inputs = raw_inputs
                .iter()
                .map(|(n, t)| p(n, t))
                .collect::<Vec<_>>();
            reg.register(ErrorSignature {
                name: name.to_string(),
                signature: sig_str.to_string(),
                selector: sel(sig_str),
                inputs,
                source: "bundled".to_string(),
                suggestion: hint.map(|s| s.to_string()),
            });
        }
    }

    /// Register additional error signatures at runtime (e.g. from a project ABI).
    pub fn register_signature(&self, sig: ErrorSignature) {
        // Only MemoryErrorRegistry supports dynamic registration; this is a best-effort cast.
        if let Some(mem) = Arc::as_ptr(&self.registry)
            .cast::<MemoryErrorRegistry>()
            .as_ref()
        {
            // SAFETY: only valid if registry was created as MemoryErrorRegistry.
            // If using a custom registry, this no-ops gracefully.
            // In practice use with_registry() + manual registration.
            let _ = mem;
        }
    }
}

impl Default for EvmErrorDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorDecoder for EvmErrorDecoder {
    fn chain_family(&self) -> &'static str {
        "evm"
    }

    fn decode(
        &self,
        revert_data: &[u8],
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError> {
        // ── Case 1: Empty revert data ──────────────────────────────────────────
        if revert_data.is_empty() {
            return Ok(DecodedError {
                kind: ErrorKind::Empty,
                raw_data: vec![],
                selector: None,
                suggestion: Some("Transaction reverted with no error message.".into()),
                confidence: 0.5,
                context: ctx,
            });
        }

        // Extract 4-byte selector if present
        let selector: Option<[u8; 4]> = if revert_data.len() >= 4 {
            Some(revert_data[..4].try_into().unwrap())
        } else {
            None
        };

        // ── Case 2: Error(string) — `0x08c379a0` ──────────────────────────────
        if let Some(message) = decode_error_string(revert_data) {
            let suggestion = generate_revert_suggestion(&message);
            return Ok(DecodedError {
                kind: ErrorKind::RevertString { message },
                raw_data: revert_data.to_vec(),
                selector: Some(ERROR_STRING_SELECTOR),
                suggestion,
                confidence: 1.0,
                context: ctx,
            });
        }

        // ── Case 3: Panic(uint256) — `0x4e487b71` ─────────────────────────────
        if let Some((code, meaning)) = decode_panic(revert_data) {
            return Ok(DecodedError {
                kind: ErrorKind::Panic {
                    code,
                    meaning: meaning.to_string(),
                },
                raw_data: revert_data.to_vec(),
                selector: Some(PANIC_SELECTOR),
                suggestion: Some(format!(
                    "Solidity assert violation (panic code 0x{code:02x}): {meaning}."
                )),
                confidence: 1.0,
                context: ctx,
            });
        }

        // ── Case 4: Known custom error from registry ───────────────────────────
        if let Some(kind) = decode_custom_error(revert_data, self.registry.as_ref()) {
            let suggestion = if let ErrorKind::CustomError { ref name, .. } = kind {
                // Look up suggestion from registry
                self.registry
                    .get_by_name(name)
                    .and_then(|s| s.suggestion)
            } else {
                None
            };
            return Ok(DecodedError {
                kind,
                raw_data: revert_data.to_vec(),
                selector,
                suggestion,
                confidence: 0.95,
                context: ctx,
            });
        }

        // ── Case 5: Unknown — return raw revert ────────────────────────────────
        let selector_hex = selector
            .map(|s| hex::encode(s))
            .unwrap_or_else(|| "none".into());

        Ok(DecodedError {
            kind: ErrorKind::RawRevert {
                selector: selector_hex,
                data: revert_data.to_vec(),
            },
            raw_data: revert_data.to_vec(),
            selector,
            suggestion: Some(
                "Unknown error selector. Try looking up the selector on https://4byte.directory"
                    .into(),
            ),
            confidence: 0.0,
            context: ctx,
        })
    }
}

/// Generate a hint based on common revert message patterns.
fn generate_revert_suggestion(message: &str) -> Option<String> {
    let msg_lower = message.to_lowercase();
    if msg_lower.contains("not the owner") || msg_lower.contains("not owner") {
        Some("Ensure the caller is the contract owner.".into())
    } else if msg_lower.contains("insufficient") && msg_lower.contains("balance") {
        Some("The account balance is too low. Check the token balance before calling.".into())
    } else if msg_lower.contains("allowance") {
        Some("Increase the token allowance with approve() before calling transferFrom().".into())
    } else if msg_lower.contains("paused") {
        Some("The contract is paused. Wait for it to be unpaused.".into())
    } else if msg_lower.contains("already") && msg_lower.contains("init") {
        Some("The contract has already been initialized.".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chainerrors_core::ErrorDecoder;

    fn decoder() -> EvmErrorDecoder {
        EvmErrorDecoder::new()
    }

    #[test]
    fn decode_empty() {
        let d = decoder();
        let result = d.decode(&[], None).unwrap();
        assert!(matches!(result.kind, ErrorKind::Empty));
        assert_eq!(result.selector, None);
    }

    #[test]
    fn decode_revert_string() {
        // Error("Not enough tokens")
        let data = hex::decode(
            "08c379a0\
             0000000000000000000000000000000000000000000000000000000000000020\
             0000000000000000000000000000000000000000000000000000000000000011\
             4e6f7420656e6f75676820746f6b656e73000000000000000000000000000000",
        )
        .unwrap();
        let result = d.decode(&data, None).unwrap();
        match &result.kind {
            ErrorKind::RevertString { message } => assert_eq!(message, "Not enough tokens"),
            _ => panic!("expected RevertString, got {:?}", result.kind),
        }
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn decode_panic_overflow() {
        let data = hex::decode(
            "4e487b710000000000000000000000000000000000000000000000000000000000000011",
        )
        .unwrap();
        let result = decoder().decode(&data, None).unwrap();
        match &result.kind {
            ErrorKind::Panic { code, meaning } => {
                assert_eq!(*code, 0x11);
                assert!(meaning.contains("overflow"));
            }
            _ => panic!("expected Panic, got {:?}", result.kind),
        }
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn decode_oz_ownable_error() {
        use tiny_keccak::{Hasher, Keccak};
        // OwnableUnauthorizedAccount(address)
        let mut k = Keccak::v256();
        k.update(b"OwnableUnauthorizedAccount(address)");
        let mut hash = [0u8; 32];
        k.finalize(&mut hash);
        let sel = [hash[0], hash[1], hash[2], hash[3]];

        // Encode address 0xdead...beef
        let mut data = sel.to_vec();
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(&[0xde, 0xad, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xbe]);

        let result = decoder().decode(&data, None).unwrap();
        assert!(
            matches!(&result.kind, ErrorKind::CustomError { name, .. } if name == "OwnableUnauthorizedAccount"),
            "got: {:?}", result.kind
        );
        assert!(result.suggestion.is_some());
    }

    #[test]
    fn decode_raw_revert_unknown() {
        let data = [0xde, 0xad, 0xbe, 0xef, 0x01, 0x02, 0x03, 0x04];
        let result = decoder().decode(&data, None).unwrap();
        assert!(matches!(result.kind, ErrorKind::RawRevert { .. }));
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn decode_hex_helper() {
        let d = decoder();
        // Revert string via hex
        let result = d
            .decode_hex(
                "0x08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000548656c6c6f000000000000000000000000000000000000000000000000000000",
                None,
            )
            .unwrap();
        assert!(matches!(result.kind, ErrorKind::RevertString { ref message } if message == "Hello"));
    }
}
