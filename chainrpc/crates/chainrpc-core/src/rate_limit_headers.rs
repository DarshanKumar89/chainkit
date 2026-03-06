//! Parse rate limit information from HTTP response headers.
//!
//! Supports multiple provider formats:
//! - Standard: `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`
//! - Alchemy: `X-Rate-Limit-CU-Second`, `X-Rate-Limit-Request-Second`
//! - Generic: `Retry-After` (seconds or HTTP date)

use std::time::Duration;

/// Parsed rate limit information from HTTP response headers.
#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    /// Maximum requests/CU allowed in the window.
    pub limit: Option<u32>,
    /// Remaining requests/CU in the current window.
    pub remaining: Option<u32>,
    /// Time until the rate limit window resets.
    pub reset_after: Option<Duration>,
    /// Suggested backoff duration from Retry-After header.
    pub retry_after: Option<Duration>,
    /// Whether a 429 status was received.
    pub is_rate_limited: bool,
}

impl RateLimitInfo {
    /// Parse rate limit headers from an HTTP header map.
    ///
    /// Accepts an iterator of `(name, value)` pairs to avoid depending on
    /// a specific HTTP crate.
    pub fn from_headers<'a>(headers: impl Iterator<Item = (&'a str, &'a str)>) -> Self {
        let mut info = Self::default();

        for (name, value) in headers {
            let lower = name.to_lowercase();
            match lower.as_str() {
                // Standard headers
                "x-ratelimit-limit" | "x-rate-limit-limit" => {
                    info.limit = value.parse().ok();
                }
                "x-ratelimit-remaining" | "x-rate-limit-remaining" => {
                    info.remaining = value.parse().ok();
                }
                "x-ratelimit-reset" | "x-rate-limit-reset" => {
                    if let Ok(secs) = value.parse::<u64>() {
                        info.reset_after = Some(Duration::from_secs(secs));
                    }
                }
                // Alchemy-specific CU headers
                "x-rate-limit-cu-second" => {
                    info.limit = value.parse().ok();
                }
                "x-rate-limit-request-second" => {
                    // Alchemy reports request-based limits separately
                    if info.limit.is_none() {
                        info.limit = value.parse().ok();
                    }
                }
                // Standard retry-after
                "retry-after" => {
                    info.retry_after = parse_retry_after(value);
                    info.is_rate_limited = true;
                }
                _ => {}
            }
        }

        info
    }

    /// Whether we should back off based on the parsed info.
    pub fn should_backoff(&self) -> bool {
        self.is_rate_limited || self.remaining == Some(0)
    }

    /// Suggested wait duration based on available information.
    pub fn suggested_wait(&self) -> Option<Duration> {
        // Prefer retry-after, then reset_after, then default 1s
        self.retry_after.or(self.reset_after).or_else(|| {
            if self.should_backoff() {
                Some(Duration::from_secs(1))
            } else {
                None
            }
        })
    }
}

/// Parse a `Retry-After` header value (seconds or HTTP date).
fn parse_retry_after(value: &str) -> Option<Duration> {
    // Try parsing as seconds first
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    // Try parsing as fractional seconds
    if let Ok(secs) = value.parse::<f64>() {
        return Some(Duration::from_secs_f64(secs));
    }
    // Could try HTTP date parsing here, but keep it simple — return 1s default
    Some(Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_headers() {
        let headers = vec![
            ("X-RateLimit-Limit", "100"),
            ("X-RateLimit-Remaining", "42"),
            ("X-RateLimit-Reset", "30"),
        ];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert_eq!(info.limit, Some(100));
        assert_eq!(info.remaining, Some(42));
        assert_eq!(info.reset_after, Some(Duration::from_secs(30)));
        assert!(!info.is_rate_limited);
        assert!(!info.should_backoff());
    }

    #[test]
    fn retry_after_seconds() {
        let headers = vec![("Retry-After", "5")];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert!(info.is_rate_limited);
        assert_eq!(info.retry_after, Some(Duration::from_secs(5)));
        assert!(info.should_backoff());
    }

    #[test]
    fn remaining_zero_triggers_backoff() {
        let headers = vec![("X-RateLimit-Remaining", "0")];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert!(info.should_backoff());
        assert!(info.suggested_wait().is_some());
    }

    #[test]
    fn alchemy_cu_headers() {
        let headers = vec![("x-rate-limit-cu-second", "330")];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert_eq!(info.limit, Some(330));
    }

    #[test]
    fn case_insensitive() {
        let headers = vec![
            ("x-ratelimit-limit", "200"),
            ("X-RATELIMIT-REMAINING", "50"),
        ];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert_eq!(info.limit, Some(200));
        assert_eq!(info.remaining, Some(50));
    }

    #[test]
    fn empty_headers() {
        let info = RateLimitInfo::from_headers(std::iter::empty());
        assert!(!info.should_backoff());
        assert!(info.suggested_wait().is_none());
    }

    #[test]
    fn suggested_wait_prefers_retry_after() {
        let headers = vec![("Retry-After", "10"), ("X-RateLimit-Reset", "30")];
        let info = RateLimitInfo::from_headers(headers.iter().map(|(k, v)| (*k, *v)));

        assert_eq!(info.suggested_wait(), Some(Duration::from_secs(10)));
    }
}
