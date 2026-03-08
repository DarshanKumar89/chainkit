//! SPL Token program error codes (0-17).
//!
//! Covers both SPL Token (TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA)
//! and Token-2022 (TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb).

/// Look up an SPL Token error by code.
/// Returns `(name, description)` if known.
pub fn lookup(code: u32) -> Option<(&'static str, &'static str)> {
    match code {
        0 => Some(("NotRentExempt", "Token account is not rent exempt. Fund the account with more SOL.")),
        1 => Some(("InsufficientFunds", "Insufficient token balance for this transfer.")),
        2 => Some(("InvalidMint", "Invalid mint — the token mint does not match.")),
        3 => Some(("MintMismatch", "Account mint does not match the expected mint.")),
        4 => Some(("OwnerMismatch", "Owner of the account does not match the expected owner.")),
        5 => Some(("FixedSupply", "This token has a fixed supply and cannot be minted.")),
        6 => Some(("AlreadyInUse", "This token account is already initialized.")),
        7 => Some(("InvalidNumberOfProvidedSigners", "Invalid number of signers provided.")),
        8 => Some(("InvalidNumberOfRequiredSigners", "Invalid number of required signers.")),
        9 => Some(("UninitializedState", "Token account or mint is not initialized.")),
        10 => Some(("NativeNotSupported", "This instruction does not support native SOL tokens.")),
        11 => Some(("NonNativeHasBalance", "Non-native token account has a non-zero SOL balance.")),
        12 => Some(("InvalidInstruction", "Invalid SPL Token instruction.")),
        13 => Some(("InvalidState", "Token account state is invalid for this operation.")),
        14 => Some(("Overflow", "Arithmetic overflow in token amount calculation.")),
        15 => Some(("AuthorityTypeNotSupported", "This authority type is not supported.")),
        16 => Some(("MintCannotFreeze", "This mint does not support freezing accounts.")),
        17 => Some(("AccountFrozen", "Token account is frozen. Thaw it before operating.")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_token_codes_defined() {
        for code in 0..=17 {
            assert!(lookup(code).is_some(), "code {code} should be defined");
        }
    }

    #[test]
    fn unknown_returns_none() {
        assert!(lookup(18).is_none());
        assert!(lookup(255).is_none());
    }

    #[test]
    fn spot_check_names() {
        assert_eq!(lookup(0).unwrap().0, "NotRentExempt");
        assert_eq!(lookup(1).unwrap().0, "InsufficientFunds");
        assert_eq!(lookup(9).unwrap().0, "UninitializedState");
        assert_eq!(lookup(17).unwrap().0, "AccountFrozen");
    }
}
