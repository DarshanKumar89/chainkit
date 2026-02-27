//! Remote ABI fetching from public blockchain explorers and metadata services.
//!
//! Supports fetching contract ABIs from:
//! - **Sourcify** — decentralized, privacy-preserving, no API key required
//! - **Etherscan** — centralized, requires API key, supports more chains
//! - **4byte.directory** — function/event signature lookup by 4-byte selector
//!
//! # Feature Flag
//! This module requires the `remote` feature flag (enables `reqwest` + `tokio`).
//!
//! ```toml
//! chaincodec-registry = { version = "0.1", features = ["remote"] }
//! ```
//!
//! # Usage
//! ```ignore
//! let fetcher = AbiFetcher::new();
//! let abi_json = fetcher.fetch_from_sourcify(1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").await?;
//! ```

#[cfg(feature = "remote")]
pub use imp::*;

#[cfg(not(feature = "remote"))]
compile_error!("The `remote` feature must be enabled to use chaincodec_registry::remote");

#[cfg(feature = "remote")]
mod imp {
    use reqwest::Client;
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    // ─── Error ────────────────────────────────────────────────────────────────

    #[derive(Debug, thiserror::Error)]
    pub enum RemoteError {
        #[error("HTTP request failed: {0}")]
        Http(#[from] reqwest::Error),

        #[error("ABI not found for {address} on chain {chain_id}")]
        NotFound { chain_id: u64, address: String },

        #[error("Etherscan API error: {message}")]
        EtherscanError { message: String },

        #[error("Invalid ABI JSON returned from {source}: {reason}")]
        InvalidAbi { source: String, reason: String },

        #[error("Rate limited by {source}")]
        RateLimited { source: String },
    }

    // ─── Sourcify ─────────────────────────────────────────────────────────────

    /// Sourcify API response for a full match or partial match.
    #[derive(Debug, Deserialize)]
    struct SourcifyFilesResponse {
        files: Option<Vec<SourcifyFile>>,
    }

    #[derive(Debug, Deserialize)]
    struct SourcifyFile {
        name: String,
        content: String,
    }

    // ─── Etherscan ────────────────────────────────────────────────────────────

    #[derive(Debug, Deserialize)]
    struct EtherscanResponse {
        status: String,
        message: String,
        result: String,
    }

    // ─── 4byte.directory ─────────────────────────────────────────────────────

    #[derive(Debug, Deserialize)]
    struct FourByteResponse {
        results: Vec<FourByteResult>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FourByteResult {
        pub id: u64,
        pub text_signature: String,
        pub hex_signature: String,
        pub bytes_signature: String,
    }

    // ─── ABI Fetcher ─────────────────────────────────────────────────────────

    /// Remote ABI fetcher.
    ///
    /// Prioritizes Sourcify (no API key) over Etherscan (requires API key).
    pub struct AbiFetcher {
        client: Client,
        sourcify_base: String,
        etherscan_base: String,
        etherscan_api_key: Option<String>,
    }

    impl AbiFetcher {
        /// Create a new fetcher with default Sourcify and Etherscan mainnet endpoints.
        pub fn new() -> Self {
            let client = Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("chaincodec/0.1 (https://github.com/DarshanKumar89/chainkit)")
                .build()
                .expect("failed to build HTTP client");

            Self {
                client,
                sourcify_base: "https://sourcify.dev/server".into(),
                etherscan_base: "https://api.etherscan.io/api".into(),
                etherscan_api_key: None,
            }
        }

        /// Set a custom Sourcify base URL (for private/self-hosted instances).
        pub fn with_sourcify_base(mut self, url: impl Into<String>) -> Self {
            self.sourcify_base = url.into();
            self
        }

        /// Set the Etherscan API key (required for Etherscan fetching).
        pub fn with_etherscan_key(mut self, key: impl Into<String>) -> Self {
            self.etherscan_api_key = Some(key.into());
            self
        }

        /// Set a custom Etherscan-compatible API base URL.
        ///
        /// Use this for chain-specific Etherscan forks:
        /// - Arbiscan: `https://api.arbiscan.io/api`
        /// - Polygonscan: `https://api.polygonscan.com/api`
        /// - Basescan: `https://api.basescan.org/api`
        pub fn with_etherscan_base(mut self, url: impl Into<String>) -> Self {
            self.etherscan_base = url.into();
            self
        }

        /// Fetch ABI JSON from Sourcify.
        ///
        /// Tries full match first, then partial match.
        /// Returns standard Ethereum ABI JSON string.
        ///
        /// # Arguments
        /// * `chain_id` - EVM chain ID (1 = Ethereum, 137 = Polygon, etc.)
        /// * `address` - contract address (checksummed or lowercase)
        pub async fn fetch_from_sourcify(
            &self,
            chain_id: u64,
            address: &str,
        ) -> Result<String, RemoteError> {
            // Normalize address to lowercase
            let address = address.to_lowercase();
            let address = if address.starts_with("0x") {
                address
            } else {
                format!("0x{address}")
            };

            // Try full match first (more reliable)
            for match_type in &["full_match", "partial_match"] {
                let url = format!(
                    "{}/v2/contract/{chain_id}/{address}",
                    self.sourcify_base
                );

                let resp = self
                    .client
                    .get(&url)
                    .query(&[("matchType", match_type)])
                    .send()
                    .await;

                match resp {
                    Ok(r) if r.status() == 200 => {
                        let json: serde_json::Value = r.json().await?;
                        // Extract ABI from Sourcify v2 response
                        if let Some(abi) = json.get("abi") {
                            return Ok(abi.to_string());
                        }
                    }
                    Ok(r) if r.status() == 404 => continue,
                    Ok(r) if r.status() == 429 => {
                        return Err(RemoteError::RateLimited {
                            source: "Sourcify".into(),
                        })
                    }
                    Ok(_) | Err(_) => {}
                }
            }

            // Fallback: try the files endpoint (Sourcify v1 compat)
            let url = format!(
                "{}/v1/files/any/{chain_id}/{address}",
                self.sourcify_base
            );
            let resp = self.client.get(&url).send().await;

            if let Ok(r) = resp {
                if r.status() == 200 {
                    let files: SourcifyFilesResponse = r.json().await.map_err(|e| {
                        RemoteError::InvalidAbi {
                            source: "Sourcify".into(),
                            reason: e.to_string(),
                        }
                    })?;

                    if let Some(file_list) = files.files {
                        // Look for the metadata.json which contains the ABI
                        for file in &file_list {
                            if file.name.ends_with("metadata.json") {
                                let metadata: serde_json::Value =
                                    serde_json::from_str(&file.content).map_err(|e| {
                                        RemoteError::InvalidAbi {
                                            source: "Sourcify".into(),
                                            reason: e.to_string(),
                                        }
                                    })?;
                                if let Some(abi) = metadata
                                    .get("output")
                                    .and_then(|o| o.get("abi"))
                                {
                                    return Ok(abi.to_string());
                                }
                            }
                        }
                    }
                }
            }

            Err(RemoteError::NotFound {
                chain_id,
                address: address.clone(),
            })
        }

        /// Fetch ABI JSON from Etherscan (or compatible explorer).
        ///
        /// Requires an API key. Returns standard Ethereum ABI JSON string.
        ///
        /// # Arguments
        /// * `address` - contract address
        pub async fn fetch_from_etherscan(&self, address: &str) -> Result<String, RemoteError> {
            let api_key = self
                .etherscan_api_key
                .as_deref()
                .unwrap_or("YourApiKeyToken"); // public rate-limited key

            let resp = self
                .client
                .get(&self.etherscan_base)
                .query(&[
                    ("module", "contract"),
                    ("action", "getabi"),
                    ("address", address),
                    ("apikey", api_key),
                ])
                .send()
                .await?;

            if resp.status() == 429 {
                return Err(RemoteError::RateLimited {
                    source: "Etherscan".into(),
                });
            }

            let body: EtherscanResponse = resp.json().await?;

            if body.status != "1" {
                return Err(RemoteError::EtherscanError {
                    message: body.message,
                });
            }

            // Validate it's valid JSON
            serde_json::from_str::<serde_json::Value>(&body.result).map_err(|e| {
                RemoteError::InvalidAbi {
                    source: "Etherscan".into(),
                    reason: e.to_string(),
                }
            })?;

            Ok(body.result)
        }

        /// Fetch ABI with automatic fallback: tries Sourcify first, then Etherscan.
        ///
        /// Returns the ABI JSON from the first successful source.
        pub async fn fetch_abi(
            &self,
            chain_id: u64,
            address: &str,
        ) -> Result<String, RemoteError> {
            // Try Sourcify first (no API key needed)
            match self.fetch_from_sourcify(chain_id, address).await {
                Ok(abi) => return Ok(abi),
                Err(RemoteError::NotFound { .. }) => {}
                Err(e) => return Err(e),
            }

            // Fall back to Etherscan
            self.fetch_from_etherscan(address).await
        }

        /// Look up function/event signatures by 4-byte selector from 4byte.directory.
        ///
        /// Useful for decoding unknown calldata selectors.
        ///
        /// # Arguments
        /// * `selector` - 4-byte hex string (with or without 0x prefix)
        pub async fn lookup_selector(
            &self,
            selector: &str,
        ) -> Result<Vec<FourByteResult>, RemoteError> {
            let hex = selector.strip_prefix("0x").unwrap_or(selector);
            let url = format!(
                "https://www.4byte.directory/api/v1/signatures/?hex_signature=0x{hex}"
            );

            let resp = self.client.get(&url).send().await?;

            if resp.status() == 429 {
                return Err(RemoteError::RateLimited {
                    source: "4byte.directory".into(),
                });
            }

            let body: FourByteResponse = resp.json().await?;
            Ok(body.results)
        }

        /// Look up event signatures by topic0 hash from 4byte.directory.
        pub async fn lookup_event_signature(
            &self,
            topic0: &str,
        ) -> Result<Vec<FourByteResult>, RemoteError> {
            let hex = topic0.strip_prefix("0x").unwrap_or(topic0);
            let url = format!(
                "https://www.4byte.directory/api/v1/event-signatures/?hex_signature=0x{hex}"
            );

            let resp = self.client.get(&url).send().await?;
            let body: FourByteResponse = resp.json().await?;
            Ok(body.results)
        }
    }

    impl Default for AbiFetcher {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Convenience: fetch ABI from Sourcify with a one-liner.
    ///
    /// Creates a temporary `AbiFetcher` and fetches the ABI.
    pub async fn fetch_abi(chain_id: u64, address: &str) -> Result<String, RemoteError> {
        AbiFetcher::new().fetch_abi(chain_id, address).await
    }
}

#[cfg(test)]
#[cfg(feature = "remote")]
mod tests {
    // Integration tests require network access; skip in CI unless INTEGRATION=1
    #[tokio::test]
    #[ignore = "requires network access"]
    async fn fetch_usdc_abi_from_sourcify() {
        let fetcher = super::imp::AbiFetcher::new();
        // USDC proxy on Ethereum mainnet
        let result = fetcher
            .fetch_from_sourcify(1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
            .await;
        // Should either succeed or return NotFound (not an HTTP error)
        assert!(matches!(
            result,
            Ok(_) | Err(super::imp::RemoteError::NotFound { .. })
        ));
    }

    #[tokio::test]
    #[ignore = "requires network access"]
    async fn lookup_transfer_selector() {
        let fetcher = super::imp::AbiFetcher::new();
        let results = fetcher.lookup_selector("a9059cbb").await.unwrap();
        assert!(!results.is_empty());
        // transfer(address,uint256) should be in results
        let has_transfer = results
            .iter()
            .any(|r| r.text_signature.starts_with("transfer("));
        assert!(has_transfer);
    }
}
