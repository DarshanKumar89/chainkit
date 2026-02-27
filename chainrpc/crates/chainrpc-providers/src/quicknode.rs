//! QuickNode provider profile.

use chainrpc_http::{HttpClientConfig, HttpRpcClient};
use std::time::Duration;

/// Build a QuickNode HTTP client for a given endpoint URL.
/// QuickNode uses personal subdomain URLs, not a shared API key template.
pub fn http_client(endpoint_url: impl Into<String>) -> HttpRpcClient {
    let config = HttpClientConfig {
        request_timeout: Duration::from_secs(30),
        ..Default::default()
    };
    HttpRpcClient::new(endpoint_url, config)
}
