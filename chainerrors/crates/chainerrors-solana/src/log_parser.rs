//! Parse Solana program log messages to extract error information.
//!
//! Solana programs emit structured log lines via `msg!()` and the runtime.
//! Common patterns:
//! - `"Program XYZ failed: custom program error: 0x1"`
//! - `"Program log: Error: insufficient funds"`
//! - `"Program log: AnchorError ... Error Code: AccountNotInitialized"`

/// A parsed error from a Solana program log line.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedError {
    /// A numeric error code (from `custom program error: 0xNN` or decimal).
    Code(u32),
    /// A text error message (from `Program log: Error: ...`).
    Message(String),
}

/// Try to parse an error from a Solana program log line.
///
/// Returns `Some(ParsedError)` if a recognized error pattern is found.
pub fn parse_program_error(log_line: &str) -> Option<ParsedError> {
    // Pattern 1: "custom program error: 0xNN" (hex)
    if let Some(rest) = log_line
        .find("custom program error: 0x")
        .map(|pos| &log_line[pos + 24..])
    {
        let hex_str: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if !hex_str.is_empty() {
            if let Ok(code) = u32::from_str_radix(&hex_str, 16) {
                return Some(ParsedError::Code(code));
            }
        }
    }

    // Pattern 2: "custom program error: NN" (decimal)
    if let Some(rest) = log_line
        .find("custom program error: ")
        .map(|pos| &log_line[pos + 22..])
    {
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num_str.is_empty() {
            if let Ok(code) = num_str.parse::<u32>() {
                return Some(ParsedError::Code(code));
            }
        }
    }

    // Pattern 3: "Program log: Error: <message>"
    if let Some(pos) = log_line.find("Program log: Error: ") {
        let message = log_line[pos + 20..].trim().to_string();
        if !message.is_empty() {
            return Some(ParsedError::Message(message));
        }
    }

    // Pattern 4: "Error Code: <name>. Error Number: <code>" (Anchor AnchorError)
    // Check BEFORE "Error Message:" since Anchor logs contain both,
    // and the numeric code is more precise for lookup.
    if let Some(pos) = log_line.find("Error Number: ") {
        let rest = &log_line[pos + 14..];
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !num_str.is_empty() {
            if let Ok(code) = num_str.parse::<u32>() {
                return Some(ParsedError::Code(code));
            }
        }
    }

    // Pattern 5: "Error Message: <message>" (Anchor style, standalone)
    if let Some(pos) = log_line.find("Error Message: ") {
        let message = log_line[pos + 15..].trim().to_string();
        if !message.is_empty() {
            return Some(ParsedError::Message(message));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_error_code() {
        let line = "Program ABC123 failed: custom program error: 0x1";
        assert_eq!(parse_program_error(line), Some(ParsedError::Code(1)));
    }

    #[test]
    fn parse_hex_error_code_large() {
        let line = "Program ABC123 failed: custom program error: 0xbc4";
        assert_eq!(parse_program_error(line), Some(ParsedError::Code(0xbc4)));
    }

    #[test]
    fn parse_decimal_error_code() {
        let line = "Program failed: custom program error: 3012";
        assert_eq!(parse_program_error(line), Some(ParsedError::Code(3012)));
    }

    #[test]
    fn parse_error_message() {
        let line = "Program log: Error: insufficient funds";
        assert_eq!(
            parse_program_error(line),
            Some(ParsedError::Message("insufficient funds".to_string()))
        );
    }

    #[test]
    fn parse_anchor_error_message() {
        let line = "Error Message: A seeds constraint was violated.";
        assert_eq!(
            parse_program_error(line),
            Some(ParsedError::Message(
                "A seeds constraint was violated.".to_string()
            ))
        );
    }

    #[test]
    fn parse_anchor_error_number() {
        let line = "Error Code: AccountNotInitialized. Error Number: 3012. Error Message: Account is not initialized.";
        assert_eq!(parse_program_error(line), Some(ParsedError::Code(3012)));
    }

    #[test]
    fn parse_unrecognized_returns_none() {
        assert!(parse_program_error("Program log: some info").is_none());
        assert!(parse_program_error("random text").is_none());
        assert!(parse_program_error("").is_none());
    }

    #[test]
    fn parse_zero_error_code() {
        let line = "Program failed: custom program error: 0x0";
        assert_eq!(parse_program_error(line), Some(ParsedError::Code(0)));
    }
}
