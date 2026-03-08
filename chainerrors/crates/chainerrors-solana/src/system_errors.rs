//! Solana System Program error codes (0-17).

/// Look up a system program error by code.
/// Returns `(name, description)` if known.
pub fn lookup(code: u32) -> Option<(&'static str, &'static str)> {
    match code {
        0 => Some(("AccountAlreadyInitialized", "Account is already initialized.")),
        1 => Some(("AccountAlreadyInUse", "Account is already in use.")),
        2 => Some(("AccountDataTooSmall", "Account data is too small.")),
        3 => Some(("AccountNotRentExempt", "Account is not rent exempt.")),
        4 => Some(("InsufficientFundsForFee", "Insufficient funds to pay transaction fee.")),
        5 => Some(("InvalidAccountDataLength", "Account data length is invalid.")),
        6 => Some(("InsufficientFundsForRent", "Insufficient funds for rent.")),
        7 => Some(("MaxSeedLengthExceeded", "Seed length exceeds the maximum allowed.")),
        8 => Some(("InvalidSeeds", "Provided seeds do not produce a valid PDA.")),
        9 => Some(("InvalidRealloc", "Account reallocation is not valid.")),
        10 => Some(("InvalidAccountOwner", "The account owner is invalid.")),
        11 => Some(("ArithmeticOverflow", "Arithmetic overflow occurred.")),
        12 => Some(("UnsupportedSysvar", "Unsupported sysvar.")),
        13 => Some(("IllegalOwner", "Illegal owner — cannot assign to this program.")),
        14 => Some(("AccountNotAssociatedTokenAccount", "Not an associated token account.")),
        15 => Some(("InvalidProgramId", "Invalid program ID.")),
        16 => Some(("InvalidInstructionData", "Invalid instruction data.")),
        17 => Some(("MaxAccountsExceeded", "Too many accounts passed to the instruction.")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_system_codes_defined() {
        for code in 0..=17 {
            assert!(lookup(code).is_some(), "code {code} should be defined");
        }
    }

    #[test]
    fn unknown_code_returns_none() {
        assert!(lookup(18).is_none());
        assert!(lookup(100).is_none());
    }

    #[test]
    fn spot_check_names() {
        assert_eq!(lookup(0).unwrap().0, "AccountAlreadyInitialized");
        assert_eq!(lookup(4).unwrap().0, "InsufficientFundsForFee");
        assert_eq!(lookup(11).unwrap().0, "ArithmeticOverflow");
    }
}
