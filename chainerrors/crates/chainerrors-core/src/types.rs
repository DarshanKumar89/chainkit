//! Core types for the ChainErrors error taxonomy.

use serde::{Deserialize, Serialize};
use std::fmt;

// ─── Severity ─────────────────────────────────────────────────────────────────

/// How critical a blockchain error is for end-user messaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Execution was successful — no error occurred.
    None,
    /// Execution reverted with a user-visible reason.
    UserError,
    /// A Solidity `assert` / invariant violation inside the contract.
    AssertionViolation,
    /// The transaction ran out of gas.
    OutOfGas,
    /// The contract was not deployed at the target address.
    ContractNotDeployed,
    /// An unknown or undecipherable error.
    Unknown,
}

// ─── Field value (simplified NormalizedValue for errors) ──────────────────────

/// A decoded parameter value in a custom error's argument list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum ErrorFieldValue {
    Uint(u128),
    BigUint(String),
    Int(i128),
    BigInt(String),
    Bool(bool),
    Bytes(Vec<u8>),
    Str(String),
    Address(String),
}

impl fmt::Display for ErrorFieldValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uint(v) => write!(f, "{v}"),
            Self::BigUint(v) => write!(f, "{v}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::BigInt(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Bytes(b) => write!(f, "0x{}", hex::encode(b)),
            Self::Str(s) => write!(f, "{s}"),
            Self::Address(a) => write!(f, "{a}"),
        }
    }
}

// ─── ErrorKind ────────────────────────────────────────────────────────────────

/// The kind/taxonomy of a decoded blockchain error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorKind {
    /// `require(condition, "message")` — a user-readable revert string.
    RevertString {
        message: String,
    },

    /// A Solidity 0.8.4+ custom error: `error Foo(uint256 bar)`.
    CustomError {
        /// Error name (e.g. `"InsufficientBalance"`).
        name: String,
        /// Decoded input fields: (param_name, value).
        inputs: Vec<(String, ErrorFieldValue)>,
    },

    /// A Solidity `assert` failure encoded as `Panic(uint256)`.
    Panic {
        /// The panic code as a u64.
        code: u64,
        /// Human-readable description of the panic code.
        meaning: String,
    },

    /// The transaction ran out of gas.
    OutOfGas,

    /// The target address has no deployed code.
    ContractNotDeployed,

    /// Revert data with a recognisable 4-byte selector but unknown ABI.
    RawRevert {
        /// The 4-byte selector, hex-encoded without `0x` prefix.
        selector: String,
        /// The full revert data.
        data: Vec<u8>,
    },

    /// No revert data (bare `revert` or out-of-gas with empty returndata).
    Empty,
}

impl ErrorKind {
    /// Returns the severity level of this error kind.
    pub fn severity(&self) -> Severity {
        match self {
            Self::RevertString { .. } | Self::CustomError { .. } => Severity::UserError,
            Self::Panic { .. } => Severity::AssertionViolation,
            Self::OutOfGas => Severity::OutOfGas,
            Self::ContractNotDeployed => Severity::ContractNotDeployed,
            Self::RawRevert { .. } | Self::Empty => Severity::Unknown,
        }
    }

    /// Returns `true` if this is a recoverable user error (revert / custom error).
    pub fn is_user_error(&self) -> bool {
        matches!(self, Self::RevertString { .. } | Self::CustomError { .. })
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevertString { message } => write!(f, "reverted: {message}"),
            Self::CustomError { name, inputs } => {
                let args: Vec<_> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
                write!(f, "{}({})", name, args.join(", "))
            }
            Self::Panic { code, meaning } => write!(f, "panic 0x{code:02x}: {meaning}"),
            Self::OutOfGas => write!(f, "out of gas"),
            Self::ContractNotDeployed => write!(f, "contract not deployed"),
            Self::RawRevert { selector, .. } => write!(f, "raw revert (selector 0x{selector})"),
            Self::Empty => write!(f, "empty revert"),
        }
    }
}

// ─── ErrorContext ──────────────────────────────────────────────────────────────

/// Metadata about the call/transaction that produced the error.
/// All fields are optional — fill in whatever is available.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorContext {
    /// Chain slug (e.g. `"ethereum"`, `"arbitrum"`).
    pub chain: Option<String>,
    /// Transaction hash (`0x…`).
    pub tx_hash: Option<String>,
    /// Contract address that reverted.
    pub contract_address: Option<String>,
    /// Solidity function selector that was called (4-byte hex).
    pub call_selector: Option<String>,
    /// Block number of the failed transaction.
    pub block_number: Option<u64>,
}

// ─── DecodedError ─────────────────────────────────────────────────────────────

/// The result of decoding revert data from a failed EVM (or Solana) call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedError {
    /// The classified error kind.
    pub kind: ErrorKind,

    /// Raw revert bytes as-is from the node.
    pub raw_data: Vec<u8>,

    /// 4-byte selector if present (first 4 bytes of `raw_data`).
    pub selector: Option<[u8; 4]>,

    /// A human-readable suggestion for how to fix the error (if known).
    pub suggestion: Option<String>,

    /// Decode confidence [0.0, 1.0].
    /// 1.0 = fully decoded with matching ABI. 0.0 = raw/empty.
    pub confidence: f32,

    /// Optional call context.
    pub context: Option<ErrorContext>,
}

impl DecodedError {
    /// Convenience constructor for an empty/bare revert.
    pub fn empty(raw_data: Vec<u8>) -> Self {
        Self {
            kind: ErrorKind::Empty,
            raw_data,
            selector: None,
            suggestion: None,
            confidence: 0.0,
            context: None,
        }
    }

    /// Returns `true` if the error was fully decoded with high confidence.
    pub fn is_decoded(&self) -> bool {
        self.confidence >= 0.8
    }

    /// Returns the error severity.
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

impl fmt::Display for DecodedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(hint) = &self.suggestion {
            write!(f, " — hint: {hint}")?;
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_display_revert_string() {
        let k = ErrorKind::RevertString {
            message: "insufficient balance".into(),
        };
        assert_eq!(k.to_string(), "reverted: insufficient balance");
    }

    #[test]
    fn error_kind_display_panic() {
        let k = ErrorKind::Panic {
            code: 0x11,
            meaning: "arithmetic overflow".into(),
        };
        assert_eq!(k.to_string(), "panic 0x11: arithmetic overflow");
    }

    #[test]
    fn error_kind_severity() {
        assert_eq!(
            ErrorKind::RevertString { message: "x".into() }.severity(),
            Severity::UserError
        );
        assert_eq!(
            ErrorKind::Panic { code: 1, meaning: String::new() }.severity(),
            Severity::AssertionViolation
        );
        assert_eq!(ErrorKind::OutOfGas.severity(), Severity::OutOfGas);
    }

    #[test]
    fn decoded_error_serde_roundtrip() {
        let err = DecodedError {
            kind: ErrorKind::RevertString {
                message: "Ownable: caller is not the owner".into(),
            },
            raw_data: vec![0x08, 0xc3, 0x79, 0xa0],
            selector: Some([0x08, 0xc3, 0x79, 0xa0]),
            suggestion: Some("Ensure you are calling from the owner address.".into()),
            confidence: 1.0,
            context: None,
        };
        let json = serde_json::to_string(&err).unwrap();
        let back: DecodedError = serde_json::from_str(&json).unwrap();
        assert_eq!(back.confidence, 1.0);
        assert!(matches!(back.kind, ErrorKind::RevertString { .. }));
    }
}
