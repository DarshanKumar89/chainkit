//! Anchor framework error codes.
//!
//! Anchor uses error code ranges:
//! - 100-299: Instruction errors
//! - 300-399: IDL errors
//! - 1000-1999: Constraint errors
//! - 2000-2999: Account errors
//! - 3000-4999: State/account errors
//! - 5000+: Custom program errors (user-defined via `#[error_code]`)

/// Look up an Anchor error by code.
/// Returns `(name, description)` if known.
pub fn lookup(code: u32) -> Option<(&'static str, &'static str)> {
    match code {
        // ── Instruction errors (100-299) ────────────────────────────────────
        100 => Some(("InstructionMissing", "8-byte instruction discriminator not found.")),
        101 => Some(("InstructionFallbackNotFound", "Fallback instruction handler not found.")),
        102 => Some(("InstructionDidNotDeserialize", "Failed to deserialize instruction data.")),
        103 => Some(("InstructionDidNotSerialize", "Failed to serialize instruction data.")),

        // ── IDL errors (300-399) ────────────────────────────────────────────
        300 => Some(("IdlInstructionStub", "IDL instruction stub — not implemented.")),
        301 => Some(("IdlInstructionInvalidProgram", "IDL instruction invalid program.")),

        // ── Constraint errors (1000-1999) ───────────────────────────────────
        2000 => Some(("ConstraintMut", "A mut constraint was violated.")),
        2001 => Some(("ConstraintHasOne", "A has_one constraint was violated.")),
        2002 => Some(("ConstraintSigner", "A signer constraint was violated.")),
        2003 => Some(("ConstraintRaw", "A raw constraint was violated.")),
        2004 => Some(("ConstraintOwner", "An owner constraint was violated.")),
        2005 => Some(("ConstraintRentExempt", "A rent exemption constraint was violated.")),
        2006 => Some(("ConstraintSeeds", "A seeds constraint was violated.")),
        2007 => Some(("ConstraintExecutable", "An executable constraint was violated.")),
        2008 => Some(("ConstraintState", "Deprecated — state constraint violated.")),
        2009 => Some(("ConstraintAssociated", "An associated constraint was violated.")),
        2010 => Some(("ConstraintAssociatedInit", "An associated init constraint was violated.")),
        2011 => Some(("ConstraintClose", "A close constraint was violated.")),
        2012 => Some(("ConstraintAddress", "An address constraint was violated.")),
        2013 => Some(("ConstraintZero", "Expected account to be zeroed.")),
        2014 => Some(("ConstraintTokenMint", "A token mint constraint was violated.")),
        2015 => Some(("ConstraintTokenOwner", "A token owner constraint was violated.")),
        2016 => Some(("ConstraintMintMintAuthority", "A mint authority constraint was violated.")),
        2017 => Some(("ConstraintMintFreezeAuthority", "A freeze authority constraint was violated.")),
        2018 => Some(("ConstraintMintDecimals", "A token decimals constraint was violated.")),
        2019 => Some(("ConstraintSpace", "An account space constraint was violated.")),
        2020 => Some(("ConstraintAccountIsNone", "Expected account to be Some, got None.")),

        // ── Account errors (3000-4999) ──────────────────────────────────────
        3000 => Some(("AccountDiscriminatorAlreadySet", "Account discriminator is already set.")),
        3001 => Some(("AccountDiscriminatorNotFound", "Account discriminator not found.")),
        3002 => Some(("AccountDiscriminatorMismatch", "Account discriminator does not match.")),
        3003 => Some(("AccountDidNotDeserialize", "Failed to deserialize account data.")),
        3004 => Some(("AccountDidNotSerialize", "Failed to serialize account data.")),
        3005 => Some(("AccountNotEnoughKeys", "Not enough account keys provided.")),
        3006 => Some(("AccountNotMutable", "Account was expected to be mutable.")),
        3007 => Some(("AccountOwnedByWrongProgram", "Account is owned by a different program.")),
        3008 => Some(("InvalidProgramId", "The program ID is invalid.")),
        3009 => Some(("InvalidProgramExecutable", "The program is not executable.")),
        3010 => Some(("AccountNotSigner", "The account did not sign the transaction.")),
        3011 => Some(("AccountNotSystemOwned", "Account is not owned by the system program.")),
        3012 => Some(("AccountNotInitialized", "Account is not initialized.")),
        3013 => Some(("AccountNotProgramData", "Account is not a ProgramData account.")),
        3014 => Some(("AccountNotAssociatedTokenAccount", "Not an associated token account.")),
        3015 => Some(("AccountSysvarMismatch", "Sysvar account mismatch.")),
        3016 => Some(("AccountReallocExceedsLimit", "Account realloc exceeds limit.")),
        3017 => Some(("AccountDuplicateReallocs", "Duplicate account reallocs in one tx.")),

        // ── State / misc errors ─────────────────────────────────────────────
        4000 => Some(("StateInvalidAddress", "Invalid state account address.")),

        // ── Deprecated / misc ───────────────────────────────────────────────
        4100 => Some(("Deprecated", "This instruction is deprecated.")),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instruction_errors() {
        assert_eq!(lookup(100).unwrap().0, "InstructionMissing");
        assert_eq!(lookup(102).unwrap().0, "InstructionDidNotDeserialize");
    }

    #[test]
    fn constraint_errors() {
        assert_eq!(lookup(2000).unwrap().0, "ConstraintMut");
        assert_eq!(lookup(2002).unwrap().0, "ConstraintSigner");
        assert_eq!(lookup(2006).unwrap().0, "ConstraintSeeds");
    }

    #[test]
    fn account_errors() {
        assert_eq!(lookup(3000).unwrap().0, "AccountDiscriminatorAlreadySet");
        assert_eq!(lookup(3012).unwrap().0, "AccountNotInitialized");
        assert_eq!(lookup(3007).unwrap().0, "AccountOwnedByWrongProgram");
    }

    #[test]
    fn unknown_returns_none() {
        assert!(lookup(50).is_none());
        assert!(lookup(999).is_none());
        assert!(lookup(6000).is_none()); // custom user errors — not in built-in table
    }
}
