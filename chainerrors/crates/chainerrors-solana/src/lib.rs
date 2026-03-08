//! chainerrors-solana — Solana program error decoder.
//!
//! Decodes errors from Solana programs including:
//! - System program errors (codes 0-17)
//! - SPL Token program errors
//! - Anchor framework errors (100-5100+ range)
//! - Custom program error codes from transaction logs
//!
//! # Usage
//! ```rust
//! use chainerrors_solana::SolanaErrorDecoder;
//! use chainerrors_core::ErrorDecoder;
//!
//! let decoder = SolanaErrorDecoder::new();
//! // Decode a system program error code
//! let result = decoder.decode_error_code(1, None, None).unwrap();
//! println!("{result}");
//! ```

mod system_errors;
mod token_errors;
mod anchor_errors;
mod log_parser;

pub use log_parser::parse_program_error;

use chainerrors_core::decoder::{DecodeErrorError, ErrorDecoder};
use chainerrors_core::types::{DecodedError, ErrorContext, ErrorKind, ErrorFieldValue};

/// Solana error decoder.
///
/// Decodes raw error data from Solana program failures. Unlike EVM where
/// errors are ABI-encoded bytes, Solana errors come in several forms:
/// - Numeric error codes in transaction results
/// - Program log messages (`"Program failed: custom program error: 0x..."`)
/// - Instruction error enums
pub struct SolanaErrorDecoder;

impl SolanaErrorDecoder {
    /// Create a new Solana error decoder.
    pub fn new() -> Self {
        Self
    }

    /// Decode a Solana error from a numeric error code.
    ///
    /// `program_id` helps determine which error table to use:
    /// - `None` or `"11111111111111111111111111111111"` → System program
    /// - `"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"` → SPL Token
    /// - Other → try Anchor error codes, then fall back to generic
    pub fn decode_error_code(
        &self,
        code: u32,
        program_id: Option<&str>,
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError> {
        // Try program-specific errors first
        if let Some(program) = program_id {
            if program == "11111111111111111111111111111111" {
                if let Some((name, desc)) = system_errors::lookup(code) {
                    return Ok(DecodedError {
                        kind: ErrorKind::CustomError {
                            name: name.to_string(),
                            inputs: vec![
                                ("code".to_string(), ErrorFieldValue::Uint(code as u128)),
                            ],
                        },
                        raw_data: code.to_le_bytes().to_vec(),
                        selector: None,
                        suggestion: Some(desc.to_string()),
                        confidence: 1.0,
                        context: ctx,
                    });
                }
            }

            // SPL Token program
            if program == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
                || program == "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
            {
                if let Some((name, desc)) = token_errors::lookup(code) {
                    return Ok(DecodedError {
                        kind: ErrorKind::CustomError {
                            name: name.to_string(),
                            inputs: vec![
                                ("code".to_string(), ErrorFieldValue::Uint(code as u128)),
                            ],
                        },
                        raw_data: code.to_le_bytes().to_vec(),
                        selector: None,
                        suggestion: Some(desc.to_string()),
                        confidence: 1.0,
                        context: ctx,
                    });
                }
            }
        }

        // Try Anchor error codes (100-5100+ range)
        if let Some((name, desc)) = anchor_errors::lookup(code) {
            return Ok(DecodedError {
                kind: ErrorKind::CustomError {
                    name: name.to_string(),
                    inputs: vec![
                        ("code".to_string(), ErrorFieldValue::Uint(code as u128)),
                    ],
                },
                raw_data: code.to_le_bytes().to_vec(),
                selector: None,
                suggestion: Some(desc.to_string()),
                confidence: 0.9,
                context: ctx,
            });
        }

        // Try system program errors without program_id context
        if let Some((name, desc)) = system_errors::lookup(code) {
            return Ok(DecodedError {
                kind: ErrorKind::CustomError {
                    name: name.to_string(),
                    inputs: vec![
                        ("code".to_string(), ErrorFieldValue::Uint(code as u128)),
                    ],
                },
                raw_data: code.to_le_bytes().to_vec(),
                selector: None,
                suggestion: Some(desc.to_string()),
                confidence: 0.7,
                context: ctx,
            });
        }

        // Unknown error code
        Ok(DecodedError {
            kind: ErrorKind::CustomError {
                name: "UnknownProgramError".to_string(),
                inputs: vec![
                    ("code".to_string(), ErrorFieldValue::Uint(code as u128)),
                    ("program".to_string(), ErrorFieldValue::Str(
                        program_id.unwrap_or("unknown").to_string(),
                    )),
                ],
            },
            raw_data: code.to_le_bytes().to_vec(),
            selector: None,
            suggestion: Some(format!(
                "Unknown Solana program error code {code} (0x{code:04x})."
            )),
            confidence: 0.1,
            context: ctx,
        })
    }

    /// Decode a Solana error from a program log line.
    ///
    /// Parses patterns like:
    /// - `"Program failed: custom program error: 0x1"` → error code 1
    /// - `"Program log: Error: insufficient funds"` → revert string
    pub fn decode_log(
        &self,
        log_line: &str,
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError> {
        if let Some(parsed) = log_parser::parse_program_error(log_line) {
            match parsed {
                log_parser::ParsedError::Code(code) => {
                    self.decode_error_code(code, None, ctx)
                }
                log_parser::ParsedError::Message(msg) => {
                    Ok(DecodedError {
                        kind: ErrorKind::RevertString {
                            message: msg.clone(),
                        },
                        raw_data: msg.as_bytes().to_vec(),
                        selector: None,
                        suggestion: None,
                        confidence: 0.8,
                        context: ctx,
                    })
                }
            }
        } else {
            Ok(DecodedError::empty(log_line.as_bytes().to_vec()))
        }
    }
}

impl Default for SolanaErrorDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorDecoder for SolanaErrorDecoder {
    fn chain_family(&self) -> &'static str {
        "solana"
    }

    fn decode(
        &self,
        revert_data: &[u8],
        ctx: Option<ErrorContext>,
    ) -> Result<DecodedError, DecodeErrorError> {
        if revert_data.is_empty() {
            return Ok(DecodedError::empty(vec![]));
        }

        // Try to interpret as a UTF-8 log line
        if let Ok(log_line) = std::str::from_utf8(revert_data) {
            let result = self.decode_log(log_line, ctx.clone())?;
            if result.confidence > 0.0 {
                return Ok(result);
            }
        }

        // Try to interpret as a 4-byte little-endian error code
        if revert_data.len() == 4 {
            let code = u32::from_le_bytes(revert_data.try_into().unwrap());
            return self.decode_error_code(code, None, ctx);
        }

        // Unknown format
        Ok(DecodedError {
            kind: ErrorKind::RawRevert {
                selector: if revert_data.len() >= 4 {
                    hex::encode(&revert_data[..4])
                } else {
                    hex::encode(revert_data)
                },
                data: revert_data.to_vec(),
            },
            raw_data: revert_data.to_vec(),
            selector: None,
            suggestion: Some("Unknown Solana error data format.".into()),
            confidence: 0.0,
            context: ctx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chainerrors_core::ErrorDecoder;

    #[test]
    fn decode_system_error() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_error_code(1, Some("11111111111111111111111111111111"), None)
            .unwrap();
        assert!(result.is_decoded());
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "AccountAlreadyInUse");
            }
            _ => panic!("expected CustomError, got {:?}", result.kind),
        }
    }

    #[test]
    fn decode_spl_token_error() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_error_code(
                1,
                Some("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
                None,
            )
            .unwrap();
        assert!(result.is_decoded());
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "InsufficientFunds");
            }
            _ => panic!("expected CustomError, got {:?}", result.kind),
        }
    }

    #[test]
    fn decode_anchor_error() {
        let decoder = SolanaErrorDecoder::new();
        // Anchor AccountNotInitialized = 3012
        let result = decoder
            .decode_error_code(3012, None, None)
            .unwrap();
        assert!(result.confidence >= 0.8);
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "AccountNotInitialized");
            }
            _ => panic!("expected CustomError, got {:?}", result.kind),
        }
    }

    #[test]
    fn decode_unknown_code() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_error_code(99999, None, None)
            .unwrap();
        assert!(!result.is_decoded());
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "UnknownProgramError");
            }
            _ => panic!("expected CustomError"),
        }
    }

    #[test]
    fn decode_from_log_hex_code() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_log("Program failed: custom program error: 0x1", None)
            .unwrap();
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn decode_from_log_message() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_log("Program log: Error: insufficient funds", None)
            .unwrap();
        match &result.kind {
            ErrorKind::RevertString { message } => {
                assert_eq!(message, "insufficient funds");
            }
            _ => panic!("expected RevertString, got {:?}", result.kind),
        }
    }

    #[test]
    fn decode_empty_bytes() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder.decode(&[], None).unwrap();
        assert!(matches!(result.kind, ErrorKind::Empty));
    }

    #[test]
    fn decode_bytes_as_log() {
        let decoder = SolanaErrorDecoder::new();
        let log = b"Program failed: custom program error: 0x0";
        let result = decoder.decode(log, None).unwrap();
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn decode_4byte_error_code() {
        let decoder = SolanaErrorDecoder::new();
        let code: u32 = 1;
        let result = decoder.decode(&code.to_le_bytes(), None).unwrap();
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn chain_family_is_solana() {
        let decoder = SolanaErrorDecoder::new();
        assert_eq!(decoder.chain_family(), "solana");
    }

    #[test]
    fn decode_hex_helper_works() {
        let decoder = SolanaErrorDecoder::new();
        // 4 bytes LE = error code 1
        let result = decoder.decode_hex("01000000", None).unwrap();
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn decode_log_decimal_code() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_log("Program failed: custom program error: 3012", None)
            .unwrap();
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "AccountNotInitialized");
            }
            _ => panic!("expected CustomError, got {:?}", result.kind),
        }
    }

    #[test]
    fn decode_spl_token_2022_error() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_error_code(
                0,
                Some("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
                None,
            )
            .unwrap();
        assert!(result.is_decoded());
        match &result.kind {
            ErrorKind::CustomError { name, .. } => {
                assert_eq!(name, "NotRentExempt");
            }
            _ => panic!("expected CustomError"),
        }
    }

    #[test]
    fn decode_system_all_codes() {
        let decoder = SolanaErrorDecoder::new();
        let system = "11111111111111111111111111111111";
        // Verify all 18 system errors are decodable (0-17)
        for code in 0..=17 {
            let result = decoder
                .decode_error_code(code, Some(system), None)
                .unwrap();
            assert!(result.is_decoded(), "system error code {code} should decode");
        }
    }

    #[test]
    fn decode_token_all_codes() {
        let decoder = SolanaErrorDecoder::new();
        let token = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
        // Verify all 18 token errors are decodable (0-17)
        for code in 0..=17 {
            let result = decoder
                .decode_error_code(code, Some(token), None)
                .unwrap();
            assert!(result.is_decoded(), "token error code {code} should decode");
        }
    }

    #[test]
    fn decode_unrecognized_log() {
        let decoder = SolanaErrorDecoder::new();
        let result = decoder
            .decode_log("some random log line", None)
            .unwrap();
        assert!(matches!(result.kind, ErrorKind::Empty));
    }

    #[test]
    fn decode_context_preserved() {
        let decoder = SolanaErrorDecoder::new();
        let ctx = ErrorContext {
            chain: Some("solana".to_string()),
            tx_hash: Some("5abc...".to_string()),
            contract_address: None,
            call_selector: None,
            block_number: Some(200_000_000),
        };
        let result = decoder
            .decode_error_code(1, Some("11111111111111111111111111111111"), Some(ctx))
            .unwrap();
        assert_eq!(result.context.as_ref().unwrap().chain.as_deref(), Some("solana"));
        assert_eq!(result.context.as_ref().unwrap().block_number, Some(200_000_000));
    }
}
